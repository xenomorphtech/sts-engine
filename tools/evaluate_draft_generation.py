#!/usr/bin/env python3
"""Evaluate one stored draft-policy generation on a large fixed seed set."""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import random
import struct
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path
from typing import Any

if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tools.train_draft_policy import (
    BOSS_SUITE_SCHEMA,
    CHECKPOINT_VERSION,
    FEATURE_SCHEMA,
    DraftClient,
    HashedSoftmaxPolicy,
    atomic_checkpoint,
    derived_seeds,
    observation_fingerprint,
    suite_reward,
)


RESULT_VERSION = 2


def reverse_last_adam_step(state: dict[str, Any]) -> list[float]:
    """Recover w[t-1] exactly from the stored post-update Adam state."""
    updates = state["updates"]
    beta1, beta2 = 0.9, 0.999
    correction1 = 1.0 - beta1**updates
    correction2 = 1.0 - beta2**updates
    learning_rate = state["learning_rate"]
    return [
        weight
        - learning_rate
        * ((first / correction1) / (math.sqrt(second / correction2) + 1e-8))
        for weight, first, second in zip(
            state["weights"], state["first_moment"], state["second_moment"]
        )
    ]


def weights_for_generation(state: dict[str, Any], generation: int) -> tuple[list[float], str]:
    if generation == state["best_generation"]:
        return state["best_weights"][:], "best_weights"
    if generation == state["generation"]:
        return state["weights"][:], "current_weights"
    if generation == state["generation"] - 1:
        return reverse_last_adam_step(state), "reversed_last_adam_step"
    raise ValueError(
        f"generation {generation} is unavailable: checkpoint stores best "
        f"generation {state['best_generation']}, current generation "
        f"{state['generation']}, and reconstructable generation "
        f"{state['generation'] - 1}"
    )


def weights_sha256(weights: list[float]) -> str:
    digest = hashlib.sha256()
    for weight in weights:
        digest.update(struct.pack("<d", weight))
    return digest.hexdigest()


def finish_formations(
    client: DraftClient,
    seeds: list[int],
    policy: HashedSoftmaxPolicy,
    ascension: int,
    max_decisions: int,
) -> list[dict[str, Any]]:
    observations = client.request(
        {
            "op": "batch_reset",
            "seeds": seeds,
            "character": "DEFECT",
            "config": {"ascension": ascension},
        }
    )["observations"]
    tried_actions: list[dict[str, set[int]]] = [{} for _ in seeds]
    evaluation_rng = random.Random(0)
    for _ in range(max_decisions):
        if all(observation["ready_for_bosses"] for observation in observations):
            return observations
        actions: list[int | None] = []
        for index, observation in enumerate(observations):
            if observation["ready_for_bosses"]:
                actions.append(None)
                continue
            fingerprint = observation_fingerprint(observation)
            forbidden = tried_actions[index].setdefault(fingerprint, set())
            action, _ = policy.choose(observation, evaluation_rng, True, forbidden)
            forbidden.add(action)
            actions.append(action)
        observations = client.request(
            {"op": "batch_step", "action_indices": actions}
        )["observations"]
    ready = sum(observation["ready_for_bosses"] for observation in observations)
    raise RuntimeError(f"formation cap reached with {ready}/{len(seeds)} ready")


def empty_totals() -> dict[str, Any]:
    return {
        "episodes": 0,
        "reward_sum": 0.0,
        "fights": 0,
        "wins": 0,
        "losses": 0,
        "timeouts": 0,
        "act1_wins": 0,
        "act1_full_clears": 0,
        "player_hp_remaining_sum": 0,
        "initial_boss_hp_sum": 0,
        "boss_hp_remaining_sum": 0,
        "boss_damage_dealt_sum": 0,
        "decision_steps_sum": 0,
        "wins_per_build": {str(index): 0 for index in range(8)},
        "bosses": {},
    }


def add_results(
    totals: dict[str, Any],
    suites: list[dict[str, Any]],
    observations: list[dict[str, Any]],
) -> None:
    for suite, observation in zip(suites, observations):
        totals["episodes"] += 1
        totals["reward_sum"] += suite_reward(suite)
        totals["fights"] += suite["fights_started"]
        totals["wins"] += suite["wins"]
        totals["losses"] += suite["losses"]
        totals["timeouts"] += suite["timeouts"]
        totals["act1_wins"] += suite["act1_wins"]
        totals["act1_full_clears"] += int(suite["act1_all_won"])
        totals["player_hp_remaining_sum"] += suite["player_hp_remaining_sum"]
        totals["initial_boss_hp_sum"] += suite["initial_boss_hp_sum"]
        totals["boss_hp_remaining_sum"] += suite["boss_hp_remaining_sum"]
        totals["boss_damage_dealt_sum"] += suite["boss_damage_dealt_sum"]
        totals["decision_steps_sum"] += observation["metrics"]["decision_steps"]
        totals["wins_per_build"][str(suite["wins"])] += 1
        for fight in suite["fights"]:
            boss = totals["bosses"].setdefault(
                fight["boss"],
                {
                    "fights": 0,
                    "wins": 0,
                    "losses": 0,
                    "timeouts": 0,
                    "player_hp_remaining_sum": 0,
                    "initial_boss_hp_sum": 0,
                    "boss_hp_remaining_sum": 0,
                    "boss_damage_dealt_sum": 0,
                },
            )
            boss["fights"] += int(fight["fought"])
            boss["wins"] += int(fight["won"])
            boss["timeouts"] += int(fight["timed_out"])
            boss["losses"] += int(fight["fought"] and not fight["won"] and not fight["timed_out"])
            boss["player_hp_remaining_sum"] += fight["player_hp_remaining"]
            boss["initial_boss_hp_sum"] += fight["initial_boss_hp"]
            boss["boss_hp_remaining_sum"] += fight["boss_hp_remaining"]
            boss["boss_damage_dealt_sum"] += fight["boss_damage_dealt"]


def evaluate_chunk(
    binary: Path,
    seeds: list[int],
    weights: list[float],
    dimensions: int,
    learning_rate: float,
    ascension: int,
    max_decisions: int,
    max_boss_steps: int,
) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    policy = HashedSoftmaxPolicy(dimensions, learning_rate)
    policy.weights = weights
    with DraftClient(binary) as client:
        observations = finish_formations(
            client, seeds, policy, ascension, max_decisions
        )
        suites = client.request(
            {"op": "batch_evaluate", "max_steps_per_boss": max_boss_steps}
        )["evaluations"]
    return suites, observations


def report(result: dict[str, Any]) -> dict[str, Any]:
    totals = result["totals"]
    fights = max(1, totals["fights"])
    episodes = max(1, totals["episodes"])
    return {
        "generation": result["generation"],
        "completed": result["completed"],
        "target": result["count"],
        "wins": totals["wins"],
        "fights": totals["fights"],
        "win_rate": totals["wins"] / fights,
        "act1_wins": totals["act1_wins"],
        "act1_full_clears": totals["act1_full_clears"],
        "act1_full_clear_rate": totals["act1_full_clears"] / episodes,
        "mean_reward": totals["reward_sum"] / episodes,
        "boss_hp_remaining_sum": totals["boss_hp_remaining_sum"],
        "boss_damage_dealt_sum": totals["boss_damage_dealt_sum"],
        "mean_decisions": totals["decision_steps_sum"] / episodes,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--generation", type=int, required=True)
    parser.add_argument("--count", type=int, default=5_000)
    parser.add_argument("--chunk-size", type=int, default=64)
    parser.add_argument("--workers", type=int, default=1)
    parser.add_argument("--checkpoint", type=Path, default=Path("tools/draft_policy_synergy_a20.json"))
    parser.add_argument("--binary", type=Path, default=Path("target/release/sts-draft"))
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--seed", type=int, default=20260823)
    parser.add_argument("--max-decisions", type=int, default=200)
    parser.add_argument("--max-boss-steps", type=int, default=2_000)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    if args.count <= 0 or args.chunk_size <= 0 or args.workers <= 0:
        raise ValueError("count, chunk size, and workers must be positive")
    with args.checkpoint.open(encoding="utf-8") as handle:
        checkpoint = json.load(handle)
    if checkpoint["version"] != CHECKPOINT_VERSION:
        raise ValueError("unsupported draft checkpoint version")
    if checkpoint.get("feature_schema") != FEATURE_SCHEMA:
        raise ValueError("draft checkpoint feature schema mismatch")
    weights, source = weights_for_generation(checkpoint, args.generation)
    weight_hash = weights_sha256(weights)
    label = "draft-generation-comparison-5000-v2-act1-snapshot"
    seeds = derived_seeds(args.seed, label, 0, args.count)
    expected = {
        "version": RESULT_VERSION,
        "boss_suite_schema": BOSS_SUITE_SCHEMA,
        "generation": args.generation,
        "weight_source": source,
        "weights_sha256": weight_hash,
        "checkpoint_generation": checkpoint["generation"],
        "count": args.count,
        "master_seed": args.seed,
        "seed_label": label,
        "ascension": checkpoint["ascension"],
        "max_decisions": args.max_decisions,
        "max_boss_steps": args.max_boss_steps,
    }
    if args.output.exists():
        with args.output.open(encoding="utf-8") as handle:
            result = json.load(handle)
        for key, value in expected.items():
            if result.get(key) != value:
                raise ValueError(f"existing result differs at {key}")
    else:
        result = {**expected, "completed": 0, "totals": empty_totals()}

    print(
        json.dumps(
            {
                "event": "start",
                **report(result),
                "workers": args.workers,
                "output": str(args.output),
            }
        ),
        flush=True,
    )
    with ProcessPoolExecutor(max_workers=args.workers) as executor:
        while result["completed"] < args.count:
            pending = []
            start = result["completed"]
            for _ in range(args.workers):
                if start >= args.count:
                    break
                stop = min(start + args.chunk_size, args.count)
                future = executor.submit(
                    evaluate_chunk,
                    args.binary,
                    seeds[start:stop],
                    weights,
                    checkpoint["dimensions"],
                    checkpoint["learning_rate"],
                    checkpoint["ascension"],
                    args.max_decisions,
                    args.max_boss_steps,
                )
                pending.append((stop, future))
                start = stop
            for stop, future in pending:
                suites, observations = future.result()
                add_results(result["totals"], suites, observations)
                result["completed"] = stop
                atomic_checkpoint(args.output, result)
                print(json.dumps({"event": "progress", **report(result)}), flush=True)
    print(json.dumps({"event": "complete", **report(result), "output": str(args.output)}), flush=True)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
