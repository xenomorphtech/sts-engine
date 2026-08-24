#!/usr/bin/env python3
"""Jointly train compressed deck building and staged seven-boss combat policies."""

from __future__ import annotations

import argparse
import json
import math
import os
import random
import sys
from collections import Counter
from pathlib import Path
from typing import Any

# Direct execution places ``tools/`` rather than the repository root on
# sys.path.  Keep the package import working both as a script and in tests.
if __package__ in (None, ""):
    sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from tools.train_draft_policy import (
    BOSS_SUITE_SCHEMA,
    CHECKPOINT_VERSION as DECK_CHECKPOINT_VERSION,
    FEATURE_SCHEMA as DECK_FEATURE_SCHEMA,
    DraftClient,
    HashedSoftmaxPolicy,
    PolicyStep,
    atomic_checkpoint,
    better,
    derived_seeds,
    flatten_offer,
    observation_fingerprint,
    suite_reward,
    summarize,
)


JOINT_CHECKPOINT_VERSION = 1
COMBAT_FEATURE_SCHEMA = "combat-candidate-state-cross-v1"


def default_deck_weights(checkpoint: dict[str, Any]) -> list[float]:
    source = checkpoint.get("default_weight_source", "weights")
    if source not in {"weights", "best_weights"}:
        raise ValueError(f"unsupported default deck weight source {source!r}")
    weights = checkpoint.get(source)
    if not isinstance(weights, list) or len(weights) != checkpoint["dimensions"]:
        raise ValueError(f"deck checkpoint {source} are missing or malformed")
    return weights[:]


class CombatSoftmaxPolicy(HashedSoftmaxPolicy):
    def features(self, observation: dict[str, Any], offer: dict[str, Any]) -> dict[int, float]:
        features: dict[int, float] = {}
        offer_identity = f"label={offer['label']}"
        self._add(features, f"combat_offer:{offer_identity}")

        hp_ratio = observation["player_hp"] / max(1, observation["player_max_hp"])
        context: list[tuple[str, float]] = [
            (f"boss={observation['boss']}", 1.0),
            (f"screen={observation['screen']}", 1.0),
            (f"turn={min(observation['turn'], 20)}", 1.0),
            (f"energy={observation['energy']}", 1.0),
            (f"hp_bucket={min(int(hp_ratio * 10), 10)}", 1.0),
            (f"block_bucket={min(observation['player_block'] // 5, 20)}", 1.0),
            (f"cards_played={min(observation['cards_played_this_turn'], 12)}", 1.0),
            (f"hand_size={len(observation['hand'])}", 1.0),
            (f"boss_index={observation['boss_index']}", 1.0),
        ]

        for pile_name in ("hand", "draw", "discard", "exhaust"):
            cards = Counter(card["id"] for card in observation[pile_name])
            for card_id, count in cards.items():
                context.append((f"{pile_name}_card={card_id}", math.sqrt(count)))
                context.append((f"{pile_name}_count={card_id}:{min(count, 6)}", 1.0))
        for card in observation["hand"]:
            context.append(
                (
                    f"hand_state={card['id']}:cost={card['cost_for_turn']}:up={int(card['upgraded'])}",
                    1.0,
                )
            )
        for relic in observation["relics"]:
            context.append((f"relic={relic['id']}", 1.0))
            context.append((f"relic_counter={relic['id']}:{relic['counter']}", 1.0))
        for potion in observation["potions"]:
            context.append((f"potion={potion}", 1.0))
        for power in observation["powers"]:
            context.append((f"player_power={power['id']}:{power['amount']}", 1.0))
        for orb_index, orb in enumerate(observation["orbs"]):
            context.append((f"orb={orb_index}:{orb['kind']}:{orb['evoke']}", 1.0))
        for monster in observation["monsters"]:
            if monster["dead"] or monster["escaped"]:
                context.append((f"enemy={monster['index']}:{monster['id']}:gone", 1.0))
                continue
            monster_ratio = monster["hp"] / max(1, monster["max_hp"])
            prefix = f"enemy={monster['index']}:{monster['id']}"
            context.extend(
                [
                    (f"{prefix}:intent={monster['intent']}", 1.0),
                    (
                        f"{prefix}:damage_per_hit={min(monster['intent_damage_per_hit'], 100)}",
                        1.0,
                    ),
                    (f"{prefix}:hits={min(monster['intent_hits'], 20)}", 1.0),
                    (
                        f"{prefix}:total_damage={min(monster['intent_total_damage'], 200)}",
                        1.0,
                    ),
                    (f"{prefix}:hp_bucket={min(int(monster_ratio * 10), 10)}", 1.0),
                    (f"{prefix}:block_bucket={min(monster['block'] // 5, 20)}", 1.0),
                ]
            )
            for power in monster["powers"]:
                context.append(
                    (f"{prefix}:power={power['id']}:{power['amount']}", 1.0)
                )

        for token, value in context:
            self._add(features, f"combat_cross:{offer_identity}|{token}", value)
        for token in flatten_offer(offer["action"]):
            self._add(features, f"combat_action:{token}")
        return features


def reset_builds(client: DraftClient, seeds: list[int], ascension: int) -> list[dict[str, Any]]:
    return client.request(
        {
            "op": "batch_reset",
            "seeds": seeds,
            "character": "DEFECT",
            "config": {"ascension": ascension},
        }
    )["observations"]


def run_formation(
    client: DraftClient,
    seeds: list[int],
    policy: HashedSoftmaxPolicy,
    rng: random.Random,
    ascension: int,
    max_decisions: int,
    greedy: bool,
) -> tuple[list[list[PolicyStep]], list[dict[str, Any]]]:
    observations = reset_builds(client, seeds, ascension)
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
                observation, rng, greedy, forbidden
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
    return trajectories, observations


def combat_fingerprint(observation: dict[str, Any]) -> str:
    stable = {
        key: value
        for key, value in observation.items()
        if key not in {"steps", "max_steps", "boss_damage_dealt"}
    }
    return json.dumps(stable, sort_keys=True, separators=(",", ":"))


def fight_reward(fight: dict[str, Any]) -> float:
    return (
        500.0 * float(fight["won"])
        - 250.0 * float(fight["timed_out"])
        + 0.10 * fight["boss_damage_dealt"]
        - 0.05 * fight["boss_hp_remaining"]
        + 0.05 * fight["player_hp_remaining"]
    )


def fight_summary(suites: list[dict[str, Any]]) -> dict[str, Any]:
    fights = [fight for suite in suites for fight in suite["fights"]]
    return {
        "fights": len(fights),
        "wins": sum(fight["won"] for fight in fights),
        "losses": sum(not fight["won"] and not fight["timed_out"] for fight in fights),
        "timeouts": sum(fight["timed_out"] for fight in fights),
        "boss_hp_remaining_sum": sum(fight["boss_hp_remaining"] for fight in fights),
        "boss_damage_dealt_sum": sum(fight["boss_damage_dealt"] for fight in fights),
        "mean_combat_steps": sum(fight["combat_steps"] for fight in fights) / len(fights),
        "mean_fight_reward": sum(fight_reward(fight) for fight in fights) / len(fights),
    }


def run_combats(
    client: DraftClient,
    policy: CombatSoftmaxPolicy,
    rng: random.Random,
    max_steps: int,
    greedy: bool,
    imitate_htn: bool = False,
) -> tuple[list[list[PolicyStep]], list[dict[str, Any]], dict[str, Any]]:
    observations = client.request(
        {"op": "batch_fight_reset", "max_steps_per_fight": max_steps}
    )["observations"]
    trajectories: list[list[PolicyStep]] = [[] for _ in observations]
    tried_actions: list[dict[str, set[int]]] = [{} for _ in observations]
    imitation_steps = 0
    imitation_correct = 0

    for _ in range(max_steps):
        if all(observation["done"] for observation in observations):
            break
        teacher_actions = None
        if imitate_htn:
            teacher_actions = client.request({"op": "batch_fight_baseline_actions"})[
                "action_indices"
            ]
        actions: list[int | None] = []
        round_imitation: list[PolicyStep] = []
        for index, observation in enumerate(observations):
            if observation["done"]:
                actions.append(None)
                continue
            if imitate_htn:
                chosen = teacher_actions[index]
                if chosen is None:
                    raise RuntimeError(f"HTN supplied no action for active fight {index}")
                action_features, probabilities = policy.distribution(observation)
                step = (action_features, probabilities, chosen)
                imitation_correct += int(
                    max(range(len(probabilities)), key=probabilities.__getitem__) == chosen
                )
                imitation_steps += 1
                round_imitation.append(step)
                action = chosen
            else:
                fingerprint = combat_fingerprint(observation)
                forbidden = tried_actions[index].setdefault(fingerprint, set())
                action, step = policy.choose(
                    observation, rng, greedy, forbidden
                )
                forbidden.add(action)
            actions.append(action)
            trajectories[index].append(step)
        if round_imitation:
            policy.imitate(round_imitation)
        observations = client.request(
            {"op": "batch_fight_step", "action_indices": actions}
        )["observations"]
    if not all(observation["done"] for observation in observations):
        done = sum(observation["done"] for observation in observations)
        raise RuntimeError(f"combat cap reached with {done}/{len(observations)} done")
    suites = client.request({"op": "batch_fight_results"})["evaluations"]
    summary = fight_summary(suites)
    if imitate_htn:
        summary["imitation_steps"] = imitation_steps
        summary["imitation_accuracy"] = imitation_correct / max(1, imitation_steps)
    return trajectories, suites, summary


def joint_evaluation(
    client: DraftClient,
    seeds: list[int],
    deck_policy: HashedSoftmaxPolicy,
    fight_policy: CombatSoftmaxPolicy,
    args: argparse.Namespace,
) -> dict[str, Any]:
    _, formations = run_formation(
        client,
        seeds,
        deck_policy,
        random.Random(args.seed),
        args.ascension,
        args.max_decisions,
        True,
    )
    _, suites, fights = run_combats(
        client,
        fight_policy,
        random.Random(args.seed),
        args.max_fight_steps,
        True,
    )
    decisions = [observation["metrics"]["decision_steps"] for observation in formations]
    return {**summarize(suites, decisions), **{f"combat_{key}": value for key, value in fights.items()}}


def deck_policy_htn_fight(
    client: DraftClient,
    seeds: list[int],
    deck_policy: HashedSoftmaxPolicy,
    args: argparse.Namespace,
) -> dict[str, Any]:
    _, formations = run_formation(
        client,
        seeds,
        deck_policy,
        random.Random(args.seed),
        args.ascension,
        args.max_decisions,
        True,
    )
    suites = client.request(
        {"op": "batch_evaluate", "max_steps_per_boss": args.max_fight_steps}
    )["evaluations"]
    decisions = [observation["metrics"]["decision_steps"] for observation in formations]
    return summarize(suites, decisions)


def htn_deck(
    client: DraftClient,
    seeds: list[int],
    args: argparse.Namespace,
) -> list[dict[str, Any]]:
    reset_builds(client, seeds, args.ascension)
    return client.request(
        {"op": "batch_baseline", "max_decisions": args.max_decisions}
    )["observations"]


def htn_deck_learned_fight(
    client: DraftClient,
    seeds: list[int],
    fight_policy: CombatSoftmaxPolicy,
    args: argparse.Namespace,
) -> dict[str, Any]:
    formations = htn_deck(client, seeds, args)
    _, suites, fights = run_combats(
        client,
        fight_policy,
        random.Random(args.seed),
        args.max_fight_steps,
        True,
    )
    decisions = [observation["metrics"]["decision_steps"] for observation in formations]
    return {**summarize(suites, decisions), **{f"combat_{key}": value for key, value in fights.items()}}


def htn_both(
    client: DraftClient,
    seeds: list[int],
    args: argparse.Namespace,
) -> dict[str, Any]:
    formations = htn_deck(client, seeds, args)
    suites = client.request(
        {"op": "batch_evaluate", "max_steps_per_boss": args.max_fight_steps}
    )["evaluations"]
    decisions = [observation["metrics"]["decision_steps"] for observation in formations]
    return summarize(suites, decisions)


def policy_state(policy: HashedSoftmaxPolicy) -> dict[str, Any]:
    return {
        "dimensions": policy.dimensions,
        "learning_rate": policy.learning_rate,
        "weights": policy.weights,
        "first_moment": policy.first_moment,
        "second_moment": policy.second_moment,
        "updates": policy.updates,
    }


def restore_policy(policy: HashedSoftmaxPolicy, state: dict[str, Any]) -> None:
    if policy.dimensions != state["dimensions"]:
        raise ValueError("policy dimensions differ from checkpoint")
    policy.weights = state["weights"]
    policy.first_moment = state["first_moment"]
    policy.second_moment = state["second_moment"]
    policy.updates = state["updates"]


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/sts-draft"))
    parser.add_argument("--deck-state", type=Path, default=Path("tools/draft_policy_synergy_a20.json"))
    parser.add_argument("--state", type=Path, default=Path("tools/joint_policy_a20.json"))
    parser.add_argument("--resume", action="store_true")
    parser.add_argument("--warmup-generations", type=int, default=3)
    parser.add_argument("--generations", type=int, default=20)
    parser.add_argument("--batch-size", type=int, default=6)
    parser.add_argument("--validation-size", type=int, default=6)
    parser.add_argument("--test-size", type=int, default=8)
    parser.add_argument("--validation-every", type=int, default=5)
    parser.add_argument("--fight-dimensions", type=int, default=65536)
    parser.add_argument("--deck-learning-rate", type=float, default=0.005)
    parser.add_argument("--fight-learning-rate", type=float, default=0.005)
    parser.add_argument("--max-decisions", type=int, default=200)
    parser.add_argument("--max-fight-steps", type=int, default=500)
    parser.add_argument("--ascension", type=int, default=20)
    parser.add_argument("--seed", type=int, default=20260823)
    return parser.parse_args()


def validate_args(args: argparse.Namespace) -> None:
    for path in (args.binary, args.deck_state):
        if not path.is_file():
            raise ValueError(f"required input does not exist: {path}")
    if args.state.exists() and not args.resume:
        raise ValueError(f"state already exists; pass --resume or choose another path: {args.state}")
    if min(args.generations, args.batch_size, args.validation_size, args.test_size) < 1:
        raise ValueError("generation and batch sizes must be positive")


def save_state(
    path: Path,
    generation: int,
    warmup_completed: int,
    deck_policy: HashedSoftmaxPolicy,
    fight_policy: CombatSoftmaxPolicy,
    best_generation: int,
    best_validation: dict[str, Any] | None,
    best_deck_weights: list[float],
    best_fight_weights: list[float],
    history: list[dict[str, Any]],
) -> None:
    atomic_checkpoint(
        path,
        {
            "version": JOINT_CHECKPOINT_VERSION,
            "combat_feature_schema": COMBAT_FEATURE_SCHEMA,
            "boss_suite_schema": BOSS_SUITE_SCHEMA,
            "generation": generation,
            "warmup_completed": warmup_completed,
            "deck_policy": policy_state(deck_policy),
            "fight_policy": policy_state(fight_policy),
            "best_generation": best_generation,
            "best_validation": best_validation,
            "best_deck_weights": best_deck_weights,
            "best_fight_weights": best_fight_weights,
            "history": history,
        },
    )


def main() -> int:
    args = parse_args()
    validate_args(args)
    with args.deck_state.open(encoding="utf-8") as handle:
        deck_checkpoint = json.load(handle)
    if deck_checkpoint["version"] != DECK_CHECKPOINT_VERSION:
        raise ValueError("deck checkpoint version is incompatible")
    if deck_checkpoint.get("feature_schema") != DECK_FEATURE_SCHEMA:
        raise ValueError("deck checkpoint feature schema is incompatible")

    deck_policy = HashedSoftmaxPolicy(
        deck_checkpoint["dimensions"], args.deck_learning_rate
    )
    deck_policy.weights = default_deck_weights(deck_checkpoint)
    fight_policy = CombatSoftmaxPolicy(
        args.fight_dimensions, args.fight_learning_rate
    )
    generation = 0
    warmup_completed = 0
    history: list[dict[str, Any]] = []
    best_generation = 0
    best_validation: dict[str, Any] | None = None
    best_deck_weights = deck_policy.weights[:]
    best_fight_weights = fight_policy.weights[:]

    if args.resume:
        with args.state.open(encoding="utf-8") as handle:
            state = json.load(handle)
        if state["version"] != JOINT_CHECKPOINT_VERSION:
            raise ValueError("joint checkpoint version is incompatible")
        if state["combat_feature_schema"] != COMBAT_FEATURE_SCHEMA:
            raise ValueError("joint combat feature schema is incompatible")
        generation = state["generation"]
        warmup_completed = state["warmup_completed"]
        restore_policy(deck_policy, state["deck_policy"])
        restore_policy(fight_policy, state["fight_policy"])
        best_generation = state["best_generation"]
        best_validation = state["best_validation"]
        best_deck_weights = state["best_deck_weights"]
        best_fight_weights = state["best_fight_weights"]
        history = state["history"]
        if state.get("boss_suite_schema") != BOSS_SUITE_SCHEMA:
            # Preserve both learned policies, but re-run HTN imitation for the
            # three newly introduced bosses and reset incomparable validation.
            warmup_completed = 0
            best_generation = generation
            best_validation = None
            best_deck_weights = deck_policy.weights[:]
            best_fight_weights = fight_policy.weights[:]

    validation_seeds = derived_seeds(args.seed, "joint-validation", 0, args.validation_size)
    test_seeds = derived_seeds(args.seed, "joint-test", 0, args.test_size)
    print(
        json.dumps(
            {
                "event": "start",
                "generation": generation,
                "target_generation": generation + args.generations,
                "warmup_completed": warmup_completed,
                "warmup_target": args.warmup_generations,
                "batch_size": args.batch_size,
                "boss_suite_schema": BOSS_SUITE_SCHEMA,
                "state": str(args.state),
            }
        ),
        flush=True,
    )

    with DraftClient(args.binary) as client:
        while warmup_completed < args.warmup_generations:
            seeds = derived_seeds(args.seed, "joint-warmup", warmup_completed, args.batch_size)
            run_formation(
                client,
                seeds,
                deck_policy,
                random.Random(args.seed ^ warmup_completed),
                args.ascension,
                args.max_decisions,
                True,
            )
            _, _, warmup = run_combats(
                client,
                fight_policy,
                random.Random(args.seed),
                args.max_fight_steps,
                False,
                imitate_htn=True,
            )
            warmup_completed += 1
            print(
                json.dumps(
                    {"event": "combat_warmup", "round": warmup_completed, **warmup}
                ),
                flush=True,
            )
            save_state(
                args.state,
                generation,
                warmup_completed,
                deck_policy,
                fight_policy,
                best_generation,
                best_validation,
                best_deck_weights,
                best_fight_weights,
                history,
            )

        if generation == 0 and best_validation is None:
            validation = joint_evaluation(
                client, validation_seeds, deck_policy, fight_policy, args
            )
            best_validation = validation
            best_deck_weights = deck_policy.weights[:]
            best_fight_weights = fight_policy.weights[:]
            print(
                json.dumps({"event": "validation", "generation": 0, **validation}),
                flush=True,
            )

        target_generation = generation + args.generations
        while generation < target_generation:
            seeds = derived_seeds(args.seed, "joint-train", generation, args.batch_size)
            rng = random.Random((args.seed << 16) ^ generation)
            deck_trajectories, formations = run_formation(
                client,
                seeds,
                deck_policy,
                rng,
                args.ascension,
                args.max_decisions,
                False,
            )
            fight_trajectories, suites, fights = run_combats(
                client,
                fight_policy,
                rng,
                args.max_fight_steps,
                False,
            )
            deck_update = deck_policy.update(
                deck_trajectories, [suite_reward(suite) for suite in suites]
            )
            flat_fights = [fight for suite in suites for fight in suite["fights"]]
            fight_update = fight_policy.update(
                fight_trajectories, [fight_reward(fight) for fight in flat_fights]
            )
            generation += 1
            decisions = [
                observation["metrics"]["decision_steps"] for observation in formations
            ]
            record = {
                "generation": generation,
                **summarize(suites, decisions),
                **{f"combat_{key}": value for key, value in fights.items()},
                "deck_gradient_norm": deck_update["gradient_norm"],
                "fight_gradient_norm": fight_update["gradient_norm"],
            }
            history.append(record)
            print(json.dumps({"event": "generation", **record}), flush=True)

            if generation % args.validation_every == 0 or generation == target_generation:
                validation = joint_evaluation(
                    client, validation_seeds, deck_policy, fight_policy, args
                )
                print(
                    json.dumps(
                        {"event": "validation", "generation": generation, **validation}
                    ),
                    flush=True,
                )
                if better(validation, best_validation):
                    best_generation = generation
                    best_validation = validation
                    best_deck_weights = deck_policy.weights[:]
                    best_fight_weights = fight_policy.weights[:]

            save_state(
                args.state,
                generation,
                warmup_completed,
                deck_policy,
                fight_policy,
                best_generation,
                best_validation,
                best_deck_weights,
                best_fight_weights,
                history,
            )

        deck_policy.weights = best_deck_weights[:]
        fight_policy.weights = best_fight_weights[:]
        learned_both = joint_evaluation(
            client, test_seeds, deck_policy, fight_policy, args
        )
        learned_deck_htn_fight = deck_policy_htn_fight(
            client, test_seeds, deck_policy, args
        )
        htn_deck_learned = htn_deck_learned_fight(
            client, test_seeds, fight_policy, args
        )
        baseline = htn_both(client, test_seeds, args)
        print(
            json.dumps(
                {
                    "event": "complete",
                    "generation": generation,
                    "best_generation": best_generation,
                    "best_validation": best_validation,
                    "learned_deck_learned_fight": learned_both,
                    "learned_deck_htn_fight": learned_deck_htn_fight,
                    "htn_deck_learned_fight": htn_deck_learned,
                    "htn_deck_htn_fight": baseline,
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
