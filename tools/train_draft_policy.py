#!/usr/bin/env python3
"""Monte Carlo policy-gradient training for the compressed boss draft env.

The learner controls only indexed formation choices. Normal/elite fights stay
abstracted by ``BossDraftBatch``. The Act 1 build snapshot must beat all three
Act 1 bosses from 60/75 HP, then the completed build is scored against the
three Act 3 bosses plus the Corrupt Heart.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import subprocess
import sys
import zlib
from collections import Counter
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path
from typing import Any, Iterable


CHECKPOINT_VERSION = 2
FEATURE_SCHEMA = "candidate-context-cross-v2"
BOSS_SUITE_SCHEMA = "act1-snapshot-60-of-75-plus-late-bosses-v2"
ACT1_REQUIRED_WINS = 3


class DraftClient:
    def __init__(self, binary: Path) -> None:
        self.process = subprocess.Popen(
            [str(binary)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=None,
            text=True,
            bufsize=1,
        )

    def request(self, payload: dict[str, Any]) -> dict[str, Any]:
        if self.process.stdin is None or self.process.stdout is None:
            raise RuntimeError("draft subprocess pipes are unavailable")
        self.process.stdin.write(json.dumps(payload, separators=(",", ":")) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            return_code = self.process.poll()
            raise RuntimeError(f"sts-draft exited unexpectedly ({return_code})")
        response = json.loads(line)
        if not response.get("ok"):
            raise RuntimeError(response.get("error", "unknown sts-draft error"))
        return response

    def close(self) -> None:
        if self.process.stdin is not None:
            self.process.stdin.close()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.terminate()
            self.process.wait(timeout=5)

    def __enter__(self) -> DraftClient:
        return self

    def __exit__(self, *_args: object) -> None:
        self.close()


SparseFeatures = dict[int, float]
PolicyStep = tuple[list[SparseFeatures], list[float], int]
GradientStatistics = tuple[int, float, float, list[float], list[float]]


def flatten_offer(value: Any, prefix: str = "") -> Iterable[str]:
    if isinstance(value, dict):
        for key in sorted(value):
            if key in {"action_index", "index"}:
                continue
            child = f"{prefix}.{key}" if prefix else key
            yield from flatten_offer(value[key], child)
    elif isinstance(value, list):
        for item in value:
            yield from flatten_offer(item, prefix)
    elif value is not None:
        yield f"{prefix}={value}"


class HashedSoftmaxPolicy:
    def __init__(self, dimensions: int, learning_rate: float) -> None:
        if dimensions < 256 or dimensions & (dimensions - 1):
            raise ValueError("--dimensions must be a power of two and at least 256")
        self.dimensions = dimensions
        self.learning_rate = learning_rate
        self.weights = [0.0] * dimensions
        self.first_moment = [0.0] * dimensions
        self.second_moment = [0.0] * dimensions
        self.updates = 0

    def _index(self, token: str) -> int:
        return zlib.crc32(token.encode("utf-8")) & (self.dimensions - 1)

    def _add(self, features: SparseFeatures, token: str, value: float = 1.0) -> None:
        index = self._index(token)
        features[index] = features.get(index, 0.0) + value

    def features(self, observation: dict[str, Any], offer: dict[str, Any]) -> SparseFeatures:
        features: SparseFeatures = {}
        phase = observation["phase"]
        offer_tokens = tuple(flatten_offer(offer["action"]))
        hp_ratio = observation["hp"] / max(1, observation["max_hp"])
        scalar_context = (
            f"phase={phase}",
            f"screen={observation['engine_screen']}",
            f"act={observation['act']}",
            f"source={observation.get('source')}",
            f"gold_bucket={min(observation['gold'] // 50, 10)}",
            f"hp_bucket={min(int(hp_ratio * 10), 10)}",
            f"deck_bucket={min(len(observation['deck']) // 5, 10)}",
            f"offers={len(observation['offers'])}",
            f"shop_slots={observation['shop_purchase_slots_remaining']}",
            f"opportunities_bucket={min(observation['opportunities_remaining'] // 5, 15)}",
            f"energy={observation['energy_master']}",
        )

        deck = Counter(card["id"] for card in observation["deck"])
        upgraded = Counter(
            card["id"] for card in observation["deck"] if card["upgraded"]
        )
        relics = {relic["id"] for relic in observation["relics"]}

        for offer_token in offer_tokens:
            # The candidate identity is the base preference. Every other
            # feature is explicitly crossed with it so owned state changes
            # relative action scores instead of adding a softmax-wide constant.
            self._add(features, f"offer:{offer_token}")
            for context in scalar_context:
                self._add(features, f"offer_context:{offer_token}|{context}")
            for card_id, count in deck.items():
                value = math.sqrt(count)
                self._add(
                    features,
                    f"offer_deck:{offer_token}|{card_id}",
                    value,
                )
                self._add(
                    features,
                    f"offer_deck_count:{offer_token}|{card_id}|{min(count, 6)}",
                )
            for card_id, count in upgraded.items():
                self._add(
                    features,
                    f"offer_upgraded:{offer_token}|{card_id}",
                    math.sqrt(count),
                )
            for relic_id in relics:
                self._add(features, f"offer_relic:{offer_token}|{relic_id}")

        # Pair the most specific candidate tokens with aggregate deck/relic
        # shape as a lower-variance route to learning broad archetype choices.
        for token in offer_tokens:
            self._add(features, f"offer_shape:{token}|unique_cards={min(len(deck), 20)}")
            self._add(features, f"offer_shape:{token}|relics={min(len(relics), 20)}")
            self._add(
                features,
                f"offer_shape:{token}|upgraded={min(sum(upgraded.values()), 20)}",
            )
        return features

    def _score(self, features: SparseFeatures) -> float:
        return sum(self.weights[index] * value for index, value in features.items())

    def distribution(
        self, observation: dict[str, Any]
    ) -> tuple[list[SparseFeatures], list[float]]:
        action_features = [self.features(observation, offer) for offer in observation["offers"]]
        if not action_features:
            raise RuntimeError(f"no offers during active phase {observation['phase']}")
        scores = [self._score(features) for features in action_features]
        maximum = max(scores)
        exponentials = [math.exp(max(-40.0, min(40.0, score - maximum))) for score in scores]
        total = sum(exponentials)
        return action_features, [value / total for value in exponentials]

    def choose(
        self,
        observation: dict[str, Any],
        rng: random.Random,
        greedy: bool,
        forbidden: set[int] | None = None,
    ) -> tuple[int, PolicyStep]:
        action_features, probabilities = self.distribution(observation)
        allowed = [
            index
            for index in range(len(probabilities))
            if forbidden is None or index not in forbidden
        ]
        if not allowed:
            allowed = list(range(len(probabilities)))
        masked_total = sum(probabilities[index] for index in allowed)
        probabilities = [
            probability / masked_total if index in allowed else 0.0
            for index, probability in enumerate(probabilities)
        ]
        if greedy:
            chosen = max(allowed, key=probabilities.__getitem__)
        else:
            point = rng.random()
            cumulative = 0.0
            chosen = len(probabilities) - 1
            for index, probability in enumerate(probabilities):
                cumulative += probability
                if point <= cumulative:
                    chosen = index
                    break
        return chosen, (action_features, probabilities, chosen)

    def update(self, trajectories: list[list[PolicyStep]], rewards: list[float]) -> dict[str, float]:
        return self.update_from_statistics(
            policy_gradient_statistics(trajectories, rewards, self.dimensions)
        )

    def update_from_statistics(self, statistics: GradientStatistics) -> dict[str, float]:
        count, reward_sum, reward_square_sum, direction_sum, reward_direction_sum = statistics
        if count <= 0:
            raise ValueError("policy-gradient statistics must contain episodes")
        mean = reward_sum / count
        variance = max(0.0, reward_square_sum / count - mean * mean)
        deviation = math.sqrt(variance)
        if deviation < 1e-9:
            return {"reward_mean": mean, "reward_std": deviation, "gradient_norm": 0.0}
        scale = 1.0 / (deviation * count)
        gradient = [
            (reward_value - mean * direction_value) * scale
            for direction_value, reward_value in zip(
                direction_sum, reward_direction_sum
            )
        ]
        norm = self._apply_gradient(gradient)
        return {"reward_mean": mean, "reward_std": deviation, "gradient_norm": norm}

    def imitate(self, steps: list[PolicyStep]) -> dict[str, float]:
        if not steps:
            return {"imitation_steps": 0, "imitation_accuracy": 0.0, "gradient_norm": 0.0}
        gradient = [0.0] * self.dimensions
        correct = 0
        for action_features, probabilities, chosen in steps:
            if max(range(len(probabilities)), key=probabilities.__getitem__) == chosen:
                correct += 1
            for index, value in action_features[chosen].items():
                gradient[index] += value
            for probability, features in zip(probabilities, action_features):
                for index, value in features.items():
                    gradient[index] -= probability * value
        inverse_steps = 1.0 / len(steps)
        norm = self._apply_gradient([value * inverse_steps for value in gradient])
        return {
            "imitation_steps": len(steps),
            "imitation_accuracy": correct / len(steps),
            "gradient_norm": norm,
        }

    def _apply_gradient(self, gradient: list[float]) -> float:
        norm = math.sqrt(sum(value * value for value in gradient))
        if norm > 5.0:
            scale = 5.0 / norm
            gradient = [value * scale for value in gradient]
            norm = 5.0

        self.updates += 1
        beta1, beta2 = 0.9, 0.999
        correction1 = 1.0 - beta1**self.updates
        correction2 = 1.0 - beta2**self.updates
        for index, value in enumerate(gradient):
            self.first_moment[index] = beta1 * self.first_moment[index] + (1.0 - beta1) * value
            self.second_moment[index] = (
                beta2 * self.second_moment[index] + (1.0 - beta2) * value * value
            )
            adjusted = (self.first_moment[index] / correction1) / (
                math.sqrt(self.second_moment[index] / correction2) + 1e-8
            )
            self.weights[index] += self.learning_rate * adjusted
        return norm


def policy_gradient_statistics(
    trajectories: list[list[PolicyStep]],
    rewards: list[float],
    dimensions: int,
) -> GradientStatistics:
    if len(trajectories) != len(rewards) or not rewards:
        raise ValueError("trajectories and rewards must have the same positive length")
    direction_sum = [0.0] * dimensions
    reward_direction_sum = [0.0] * dimensions
    for trajectory, reward in zip(trajectories, rewards):
        if not trajectory:
            continue
        inverse_steps = 1.0 / len(trajectory)
        for action_features, probabilities, chosen in trajectory:
            for index, value in action_features[chosen].items():
                contribution = inverse_steps * value
                direction_sum[index] += contribution
                reward_direction_sum[index] += reward * contribution
            for probability, features in zip(probabilities, action_features):
                for index, value in features.items():
                    contribution = -inverse_steps * probability * value
                    direction_sum[index] += contribution
                    reward_direction_sum[index] += reward * contribution
    return (
        len(rewards),
        sum(rewards),
        sum(reward * reward for reward in rewards),
        direction_sum,
        reward_direction_sum,
    )


def merge_gradient_statistics(parts: list[GradientStatistics]) -> GradientStatistics:
    if not parts:
        raise ValueError("at least one policy-gradient statistics part is required")
    count = sum(part[0] for part in parts)
    reward_sum = sum(part[1] for part in parts)
    reward_square_sum = sum(part[2] for part in parts)
    dimensions = len(parts[0][3])
    direction_sum = [0.0] * dimensions
    reward_direction_sum = [0.0] * dimensions
    for _, _, _, directions, reward_directions in parts:
        if len(directions) != dimensions or len(reward_directions) != dimensions:
            raise ValueError("policy-gradient statistics dimensions differ")
        for index, value in enumerate(directions):
            direction_sum[index] += value
        for index, value in enumerate(reward_directions):
            reward_direction_sum[index] += value
    return count, reward_sum, reward_square_sum, direction_sum, reward_direction_sum


def suite_reward(result: dict[str, Any]) -> float:
    # The early checkpoint is a requirement, not three optional bonus fights.
    # A complete trio earns a large gate bonus; every missing Act 1 win is
    # penalized. Older fixture-shaped dictionaries omit the fields and retain
    # the legacy reward, which keeps small unit/evaluation helpers compatible.
    act1_requirement = 0.0
    if "act1_wins" in result:
        act1_wins = min(ACT1_REQUIRED_WINS, int(result["act1_wins"]))
        act1_requirement = (
            1000.0
            if result.get("act1_all_won", False)
            else -500.0 * (ACT1_REQUIRED_WINS - act1_wins)
        )
    return (
        250.0 * result["wins"]
        - 100.0 * result["timeouts"]
        + 0.02 * result["boss_damage_dealt_sum"]
        - 0.01 * result["boss_hp_remaining_sum"]
        + 0.01 * result["player_hp_remaining_sum"]
        + act1_requirement
    )


def summarize(results: list[dict[str, Any]], decision_counts: list[int]) -> dict[str, Any]:
    rewards = [suite_reward(result) for result in results]
    return {
        "episodes": len(results),
        "mean_reward": sum(rewards) / len(rewards),
        "fights": sum(result["fights_started"] for result in results),
        "wins": sum(result["wins"] for result in results),
        "losses": sum(result["losses"] for result in results),
        "timeouts": sum(result["timeouts"] for result in results),
        "act1_wins": sum(result["act1_wins"] for result in results),
        "act1_full_clears": sum(result["act1_all_won"] for result in results),
        "act1_full_clear_rate": sum(result["act1_all_won"] for result in results)
        / len(results),
        "boss_hp_remaining_sum": sum(result["boss_hp_remaining_sum"] for result in results),
        "boss_damage_dealt_sum": sum(result["boss_damage_dealt_sum"] for result in results),
        "mean_decisions": sum(decision_counts) / len(decision_counts),
    }


def run_policy_batch(
    client: DraftClient,
    seeds: list[int],
    policy: HashedSoftmaxPolicy,
    rng: random.Random,
    ascension: int,
    max_decisions: int,
    max_boss_steps: int,
    greedy: bool,
) -> tuple[list[dict[str, Any]], list[list[PolicyStep]], dict[str, Any]]:
    response = client.request(
        {
            "op": "batch_reset",
            "seeds": seeds,
            "character": "DEFECT",
            "config": {"ascension": ascension},
        }
    )
    observations = response["observations"]
    trajectories: list[list[PolicyStep]] = [[] for _ in seeds]
    tried_actions: list[dict[str, set[int]]] = [{} for _ in seeds]
    for _ in range(max_decisions):
        if all(observation["ready_for_bosses"] for observation in observations):
            break
        actions: list[int | None] = []
        for index, observation in enumerate(observations):
            if observation["ready_for_bosses"]:
                actions.append(None)
                continue
            fingerprint = observation_fingerprint(observation)
            forbidden = tried_actions[index].setdefault(fingerprint, set())
            action, step = policy.choose(
                observation,
                rng,
                greedy,
                forbidden,
            )
            actions.append(action)
            trajectories[index].append(step)
            forbidden.add(action)
        observations = client.request(
            {"op": "batch_step", "action_indices": actions}
        )["observations"]
    if not all(observation["ready_for_bosses"] for observation in observations):
        ready = sum(observation["ready_for_bosses"] for observation in observations)
        raise RuntimeError(f"formation cap reached with {ready}/{len(seeds)} ready")

    results = client.request(
        {"op": "batch_evaluate", "max_steps_per_boss": max_boss_steps}
    )["evaluations"]
    decisions = [observation["metrics"]["decision_steps"] for observation in observations]
    return results, trajectories, summarize(results, decisions)


def run_training_shard(
    binary: Path,
    seeds: list[int],
    weights: list[float],
    dimensions: int,
    learning_rate: float,
    rng_seed: int,
    ascension: int,
    max_decisions: int,
    max_boss_steps: int,
) -> tuple[dict[str, Any], GradientStatistics]:
    policy = HashedSoftmaxPolicy(dimensions, learning_rate)
    policy.weights = weights
    with DraftClient(binary) as client:
        results, trajectories, summary = run_policy_batch(
            client,
            seeds,
            policy,
            random.Random(rng_seed),
            ascension,
            max_decisions,
            max_boss_steps,
            False,
        )
    rewards = [suite_reward(result) for result in results]
    return summary, policy_gradient_statistics(trajectories, rewards, dimensions)


def merge_summaries(parts: list[dict[str, Any]]) -> dict[str, Any]:
    episodes = sum(part["episodes"] for part in parts)
    if episodes <= 0:
        raise ValueError("parallel batch produced no episodes")
    return {
        "episodes": episodes,
        "mean_reward": sum(
            part["mean_reward"] * part["episodes"] for part in parts
        )
        / episodes,
        "fights": sum(part["fights"] for part in parts),
        "wins": sum(part["wins"] for part in parts),
        "losses": sum(part["losses"] for part in parts),
        "timeouts": sum(part["timeouts"] for part in parts),
        "act1_wins": sum(part["act1_wins"] for part in parts),
        "act1_full_clears": sum(part["act1_full_clears"] for part in parts),
        "act1_full_clear_rate": sum(
            part["act1_full_clears"] for part in parts
        )
        / episodes,
        "boss_hp_remaining_sum": sum(
            part["boss_hp_remaining_sum"] for part in parts
        ),
        "boss_damage_dealt_sum": sum(
            part["boss_damage_dealt_sum"] for part in parts
        ),
        "mean_decisions": sum(
            part["mean_decisions"] * part["episodes"] for part in parts
        )
        / episodes,
    }


def run_parallel_training_batch(
    executor: ProcessPoolExecutor,
    binary: Path,
    seeds: list[int],
    policy: HashedSoftmaxPolicy,
    rng_seed: int,
    workers: int,
    ascension: int,
    max_decisions: int,
    max_boss_steps: int,
) -> tuple[dict[str, Any], GradientStatistics]:
    shard_count = min(workers, len(seeds))
    shard_size = (len(seeds) + shard_count - 1) // shard_count
    futures = []
    for shard_index, start in enumerate(range(0, len(seeds), shard_size)):
        shard = seeds[start : start + shard_size]
        futures.append(
            executor.submit(
                run_training_shard,
                binary,
                shard,
                policy.weights,
                policy.dimensions,
                policy.learning_rate,
                rng_seed ^ (0x9E3779B97F4A7C15 * (shard_index + 1)),
                ascension,
                max_decisions,
                max_boss_steps,
            )
        )
    completed = [future.result() for future in futures]
    return (
        merge_summaries([summary for summary, _ in completed]),
        merge_gradient_statistics([statistics for _, statistics in completed]),
    )


def observation_fingerprint(observation: dict[str, Any]) -> str:
    stable = {
        "phase": observation["phase"],
        "engine_screen": observation["engine_screen"],
        "source": observation.get("source"),
        "hp": observation["hp"],
        "max_hp": observation["max_hp"],
        "gold": observation["gold"],
        "deck": observation["deck"],
        "relics": observation["relics"],
        "offers": observation["offers"],
        "shop_purchase_slots_remaining": observation["shop_purchase_slots_remaining"],
    }
    return json.dumps(stable, sort_keys=True, separators=(",", ":"))


def run_htn_batch(
    client: DraftClient,
    seeds: list[int],
    ascension: int,
    max_decisions: int,
    max_boss_steps: int,
) -> dict[str, Any]:
    client.request(
        {
            "op": "batch_reset",
            "seeds": seeds,
            "character": "DEFECT",
            "config": {"ascension": ascension},
        }
    )
    observations = client.request(
        {"op": "batch_baseline", "max_decisions": max_decisions}
    )["observations"]
    results = client.request(
        {"op": "batch_evaluate", "max_steps_per_boss": max_boss_steps}
    )["evaluations"]
    decisions = [observation["metrics"]["decision_steps"] for observation in observations]
    return summarize(results, decisions)


def derived_seeds(master_seed: int, label: str, generation: int, count: int) -> list[int]:
    source = zlib.crc32(f"{master_seed}:{label}:{generation}".encode("utf-8"))
    rng = random.Random((master_seed << 32) ^ source)
    return [rng.randrange(-(1 << 62), 1 << 62) for _ in range(count)]


def better(left: dict[str, Any], right: dict[str, Any] | None) -> bool:
    if right is None:
        return True
    left_key = (
        left.get("act1_full_clears", 0),
        left.get("act1_wins", 0),
        left["wins"],
        -left["timeouts"],
        left["mean_reward"],
        left["boss_damage_dealt_sum"],
        -left["boss_hp_remaining_sum"],
    )
    right_key = (
        right.get("act1_full_clears", 0),
        right.get("act1_wins", 0),
        right["wins"],
        -right["timeouts"],
        right["mean_reward"],
        right["boss_damage_dealt_sum"],
        -right["boss_hp_remaining_sum"],
    )
    return left_key > right_key


def atomic_checkpoint(path: Path, state: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp.{os.getpid()}")
    with temporary.open("w", encoding="utf-8") as handle:
        json.dump(state, handle, separators=(",", ":"))
        handle.write("\n")
    os.replace(temporary, path)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/sts-draft"))
    parser.add_argument(
        "--state", type=Path, default=Path("tools/draft_policy_synergy_a20.json")
    )
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--generations", type=int, default=25)
    parser.add_argument("--batch-size", type=int, default=256)
    parser.add_argument(
        "--workers", type=int, default=max(1, min(10, os.cpu_count() or 1))
    )
    parser.add_argument("--validation-size", type=int, default=12)
    parser.add_argument("--test-size", type=int, default=16)
    parser.add_argument("--validation-every", type=int, default=5)
    parser.add_argument("--dimensions", type=int, default=32768)
    parser.add_argument("--learning-rate", type=float, default=0.02)
    parser.add_argument("--max-decisions", type=int, default=200)
    parser.add_argument("--max-boss-steps", type=int, default=2000)
    parser.add_argument("--ascension", type=int, default=20)
    parser.add_argument("--seed", type=int, default=20260823)
    return parser.parse_args()


def validate_args(args: argparse.Namespace) -> None:
    if not args.binary.is_file():
        raise ValueError(f"draft binary does not exist: {args.binary}")
    if args.generations < 1 or args.batch_size < 2:
        raise ValueError("generations must be positive and batch size must be at least two")
    if args.workers < 1:
        raise ValueError("workers must be positive")
    if args.validation_size < 1 or args.test_size < 1 or args.validation_every < 1:
        raise ValueError("validation/test sizes and cadence must be positive")
    if not 0 <= args.ascension <= 20:
        raise ValueError("ascension must be between zero and twenty")
    if args.state.exists() and not args.resume:
        raise ValueError(f"state already exists; pass --resume or choose another path: {args.state}")


def main() -> int:
    args = parse_args()
    validate_args(args)
    policy = HashedSoftmaxPolicy(args.dimensions, args.learning_rate)
    history: list[dict[str, Any]] = []
    generation = 0
    best_generation = 0
    best_validation: dict[str, Any] | None = None
    best_weights = policy.weights[:]

    if args.resume:
        with args.state.open(encoding="utf-8") as handle:
            state = json.load(handle)
        if state["version"] != CHECKPOINT_VERSION:
            raise ValueError("unsupported checkpoint version")
        if state.get("feature_schema") != FEATURE_SCHEMA:
            raise ValueError("checkpoint feature schema differs from this trainer")
        if state["dimensions"] != args.dimensions or state["ascension"] != args.ascension:
            raise ValueError("checkpoint dimensions or ascension differ from arguments")
        generation = state["generation"]
        policy.weights = state["weights"]
        policy.first_moment = state["first_moment"]
        policy.second_moment = state["second_moment"]
        policy.updates = state["updates"]
        history = state["history"]
        best_generation = state["best_generation"]
        best_validation = state["best_validation"]
        best_weights = state["best_weights"]
        if state.get("boss_suite_schema") != BOSS_SUITE_SCHEMA:
            # Retain the trained policy and optimizer state, but do not compare
            # seven-fight validation against the obsolete four-fight metric.
            best_generation = generation
            best_validation = None
            best_weights = policy.weights[:]

    validation_seeds = derived_seeds(args.seed, "validation", 0, args.validation_size)
    test_seeds = derived_seeds(args.seed, "test", 0, args.test_size)
    print(
        json.dumps(
            {
                "event": "start",
                "generation": generation,
                "target_generation": generation + args.generations,
                "batch_size": args.batch_size,
                "workers": args.workers,
                "ascension": args.ascension,
                "dimensions": args.dimensions,
                "boss_suite_schema": BOSS_SUITE_SCHEMA,
                "state": str(args.state),
            }
        ),
        flush=True,
    )

    with (
        ProcessPoolExecutor(max_workers=args.workers) as executor,
        DraftClient(args.binary) as client,
    ):
        if generation == 0:
            initial_results, _, initial_validation = run_policy_batch(
                client,
                validation_seeds,
                policy,
                random.Random(args.seed),
                args.ascension,
                args.max_decisions,
                args.max_boss_steps,
                True,
            )
            del initial_results
            best_validation = initial_validation
            best_weights = policy.weights[:]
            print(json.dumps({"event": "validation", "generation": 0, **initial_validation}), flush=True)

        target_generation = generation + args.generations
        while generation < target_generation:
            train_seeds = derived_seeds(args.seed, "train", generation, args.batch_size)
            train_summary, statistics = run_parallel_training_batch(
                executor,
                args.binary,
                train_seeds,
                policy,
                (args.seed << 16) ^ generation,
                args.workers,
                args.ascension,
                args.max_decisions,
                args.max_boss_steps,
            )
            update = policy.update_from_statistics(statistics)
            generation += 1
            record = {"generation": generation, **train_summary, **update}
            history.append(record)
            print(json.dumps({"event": "generation", **record}), flush=True)

            if generation % args.validation_every == 0 or generation == target_generation:
                _, _, validation = run_policy_batch(
                    client,
                    validation_seeds,
                    policy,
                    random.Random(args.seed),
                    args.ascension,
                    args.max_decisions,
                    args.max_boss_steps,
                    True,
                )
                print(json.dumps({"event": "validation", "generation": generation, **validation}), flush=True)
                if better(validation, best_validation):
                    best_generation = generation
                    best_validation = validation
                    best_weights = policy.weights[:]

            atomic_checkpoint(
                args.state,
                {
                    "version": CHECKPOINT_VERSION,
                    "feature_schema": FEATURE_SCHEMA,
                    "boss_suite_schema": BOSS_SUITE_SCHEMA,
                    "generation": generation,
                    "ascension": args.ascension,
                    "dimensions": args.dimensions,
                    "learning_rate": args.learning_rate,
                    "batch_size": args.batch_size,
                    "workers": args.workers,
                    "master_seed": args.seed,
                    "default_generation": generation,
                    "default_weight_source": "weights",
                    "weights": policy.weights,
                    "first_moment": policy.first_moment,
                    "second_moment": policy.second_moment,
                    "updates": policy.updates,
                    "best_generation": best_generation,
                    "best_validation": best_validation,
                    "best_weights": best_weights,
                    "history": history,
                },
            )

        _, _, test_summary = run_policy_batch(
            client,
            test_seeds,
            policy,
            random.Random(args.seed),
            args.ascension,
            args.max_decisions,
            args.max_boss_steps,
            True,
        )
        zero_policy = HashedSoftmaxPolicy(args.dimensions, args.learning_rate)
        _, _, zero_summary = run_policy_batch(
            client,
            test_seeds,
            zero_policy,
            random.Random(args.seed),
            args.ascension,
            args.max_decisions,
            args.max_boss_steps,
            True,
        )
        htn_summary = run_htn_batch(
            client,
            test_seeds,
            args.ascension,
            args.max_decisions,
            args.max_boss_steps,
        )
        print(
            json.dumps(
                {
                    "event": "complete",
                    "generation": generation,
                    "default_generation": generation,
                    "best_generation": best_generation,
                    "best_validation": best_validation,
                    "learned_test": test_summary,
                    "generation_zero_test": zero_summary,
                    "htn_test": htn_summary,
                    "state": str(args.state),
                }
            ),
            flush=True,
        )
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError) as error:
        print(f"error: {error}", file=sys.stderr)
        raise SystemExit(2)
