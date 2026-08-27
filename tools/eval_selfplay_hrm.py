#!/usr/bin/env python3
"""Closed-loop evaluation of a teacher-free whole-run HRM checkpoint."""

from __future__ import annotations

import argparse
import json
import lzma
import math
from pathlib import Path
import subprocess
import time
from typing import Any, TextIO

import torch

from train_selfplay_hrm import (
    FLOOR_SURVIVAL_NAMES,
    MAX_ACTION_FEATURES,
    MAX_CANDIDATE_IDENTITIES,
    MAX_HISTORY_STEPS,
    MAX_INVENTORY_IDENTITIES,
    MAX_STATE_FEATURES,
    SelfPlayHrm,
    action_parameter_vector,
    candidate_identity_features,
    decision_signature,
    measurement_vector,
)


INFERENCE_BATCH_SIZE = 512


class SplitMix64:
    def __init__(self, state: int):
        self.state = state & ((1 << 64) - 1)

    def next(self) -> int:
        mask = (1 << 64) - 1
        self.state = (self.state + 0x9E3779B97F4A7C15) & mask
        value = self.state
        value = ((value ^ (value >> 30)) * 0xBF58476D1CE4E5B9) & mask
        value = ((value ^ (value >> 27)) * 0x94D049BB133111EB) & mask
        return (value ^ (value >> 31)) & mask

    def uniform_open(self) -> float:
        # The half-step keeps the transform strictly inside (0, 1).
        return (self.next() + 0.5) / float(1 << 64)

    def gumbel(self) -> float:
        return -math.log(-math.log(self.uniform_open()))


def signed_i64(value: int) -> int:
    return value if value < 1 << 63 else value - (1 << 64)


def load_seeds(path: Path, min_floor: int = 0) -> list[int]:
    seeds: list[int] = []
    with path.open("r", encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            value = json.loads(line)
            if (
                isinstance(value, dict)
                and "max_floor" in value
                and int(value["max_floor"]) < min_floor
            ):
                continue
            seed = value.get("seed") if isinstance(value, dict) else value
            if not isinstance(seed, int):
                raise ValueError(f"{path}:{line_number}: seed must be an integer")
            seeds.append(seed)
    if not seeds:
        raise ValueError(f"{path}: no seeds")
    return seeds


def request(process: subprocess.Popen[str], body: dict[str, Any]) -> list[dict[str, Any]]:
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(json.dumps(body, separators=(",", ":")) + "\n")
    process.stdin.flush()
    line = process.stdout.readline()
    if not line:
        error = process.stderr.read() if process.stderr is not None else ""
        raise RuntimeError(f"self-play engine closed the protocol: {error}")
    return json.loads(line)


def contains_expected(actual: Any, expected: Any) -> bool:
    """Compare an older trace against a schema-compatible richer observation."""
    if isinstance(expected, dict):
        return isinstance(actual, dict) and all(
            key in ("inventory_identities", "candidate_identities")
            or (key in actual and contains_expected(actual[key], value))
            for key, value in expected.items()
        )
    if isinstance(expected, list):
        return (
            isinstance(actual, list)
            and len(actual) == len(expected)
            and all(contains_expected(a, e) for a, e in zip(actual, expected))
        )
    return actual == expected


def pad_features(rows: list[list[int]], width: int, device: torch.device) -> torch.Tensor:
    tensor = torch.zeros((len(rows), width), dtype=torch.long)
    for index, row in enumerate(rows):
        row = row[:width]
        tensor[index, : len(row)] = torch.tensor(row, dtype=torch.long)
    return tensor.to(device, non_blocking=True)


def pad_history(rows: list[list[int]], device: torch.device) -> torch.Tensor:
    tensor = torch.zeros((len(rows), MAX_HISTORY_STEPS), dtype=torch.long)
    for index, row in enumerate(rows):
        row = row[-MAX_HISTORY_STEPS:]
        if row:
            tensor[index, -len(row) :] = torch.tensor(row, dtype=torch.long)
    return tensor.to(device, non_blocking=True)


def utilities(
    prediction: torch.Tensor,
    combat: torch.Tensor,
    search_supported: torch.Tensor,
    choice_supported: torch.Tensor,
    policy: str,
    target_names: tuple[str, ...],
    supported_targets: frozenset[str],
    combat_search_weight: float,
    counterfactual_search_weight: float,
    outside_search_weight: float,
    counterfactual_outside_weight: float,
) -> torch.Tensor:
    head = {name: prediction[:, index] for index, name in enumerate(target_names)}
    zero = torch.zeros_like(prediction[:, 0])

    def value(name: str) -> torch.Tensor:
        return head.get(name, zero) if name in supported_targets else zero

    survival_logits = [
        head[name]
        for name in FLOOR_SURVIVAL_NAMES
        if name in head and name in supported_targets
    ]
    expected_floor = (
        torch.stack(survival_logits, dim=1).sigmoid().sum(dim=1)
        if survival_logits
        else value("max_floor")
    )
    if policy == "floor":
        return expected_floor
    milestone = (
        0.15 * value("reach_act1_boss")
        + 0.30 * value("reach_act2")
        + 0.45 * value("reach_act2_boss")
        + 0.60 * value("reach_act3")
        + 0.80 * value("reach_act3_boss")
        + value("act3_win")
    )
    combat_local = (
        2.0 * value("combat_margin")
        + 0.50 * expected_floor
        + 0.20 * value("terminal_margin")
        + 0.25 * value("hp_delta_1")
        - 0.25 * value("enemy_hp_delta_1")
        + 0.10 * value("hp_delta_8")
        - 0.10 * value("enemy_hp_delta_8")
        + combat_search_weight * value("search_value") * search_supported
        + counterfactual_search_weight * value("choice_value")
        + milestone
    )
    outside_choice_value = (
        value("choice_value") * choice_supported
        if "choice_value" in head
        else value("search_value") * search_supported
    )
    if policy == "local":
        outside = expected_floor + 0.10 * value("terminal_margin") + milestone
    else:
        outside = (
            expected_floor
            + 0.25 * value("terminal_margin")
            + 0.10 * value("floor_delta_32")
            + 0.05 * value("gold_delta_32")
            + 0.05 * value("relic_delta_128")
            + 0.05 * value("upgrade_delta_128")
            + milestone
            + outside_search_weight * outside_choice_value
            + counterfactual_outside_weight * value("choice_value")
        )
    return torch.where(combat, combat_local, outside)


def observation_key(observation: dict[str, Any]) -> tuple[int, ...]:
    """Identify the complete visible decision, including its legal-action menu."""
    key = list(observation["state_features"])
    key.append(0)
    key.extend(observation.get("inventory_identities", []))
    key.append(0)
    for action in observation["actions"]:
        features = action["features"]
        key.append(len(features))
        key.extend(features)
        identities = action.get("candidate_identities", [])
        key.append(len(identities))
        key.extend(identities)
    return tuple(key)


@torch.inference_mode()
def rank_actions(
    model: SelfPlayHrm,
    rows: list[dict[str, Any]],
    device: torch.device,
    policy: str,
    exploration_rngs: list[SplitMix64],
    temperature: float,
    target_names: tuple[str, ...],
    histories: list[list[int]],
    combat_search_weight: float,
    counterfactual_search_weight: float,
    outside_search_weight: float,
    counterfactual_outside_weight: float,
) -> dict[int, list[tuple[float, int]]]:
    state_rows: list[list[int]] = []
    action_rows: list[list[int]] = []
    inventory_rows: list[list[int]] = []
    candidate_rows: list[list[int]] = []
    numeric_rows: list[list[float]] = []
    action_numeric_rows: list[list[float]] = []
    history_rows: list[list[int]] = []
    owners: list[tuple[int, int]] = []
    combat_flags: list[bool] = []
    search_flags: list[bool] = []
    choice_flags: list[bool] = []
    for environment, row in enumerate(rows):
        observation = row.get("observation")
        if observation is None:
            continue
        for action_index, action in enumerate(observation["actions"]):
            state_rows.append(observation["state_features"])
            action_rows.append(action["features"])
            inventory_rows.append(observation.get("inventory_identities", []))
            candidate_ids = candidate_identity_features(
                observation,
                action,
                row["measurements"]["enemy_max_hp"] > 0,
            )
            candidate_rows.append(candidate_ids)
            numeric_rows.append(
                measurement_vector(row["measurements"], model.numeric_size)
            )
            action_numeric_rows.append(
                action_parameter_vector(
                    action,
                    row["measurements"],
                    model.action_numeric_size,
                )
            )
            history_rows.append(histories[environment])
            owners.append((environment, action_index))
            combat_flags.append(row["measurements"]["enemy_max_hp"] > 0)
            search_flags.append(
                row["measurements"]["floor"]
                >= int(getattr(model, "search_value_min_floor", 16))
                and bool(getattr(model, "search_value_supported", True))
            )
            choice_flags.append(bool(candidate_ids))
    if not owners:
        return {}
    state = pad_features(state_rows, MAX_STATE_FEATURES, device)
    action = pad_features(action_rows, MAX_ACTION_FEATURES, device)
    inventory = pad_features(
        inventory_rows, MAX_INVENTORY_IDENTITIES, device
    )
    candidate = pad_features(
        candidate_rows, MAX_CANDIDATE_IDENTITIES, device
    )
    numeric = torch.tensor(numeric_rows, dtype=torch.float32, device=device)
    action_numeric = torch.tensor(
        action_numeric_rows, dtype=torch.float32, device=device
    )
    history = pad_history(history_rows, device)
    combat = torch.tensor(combat_flags, device=device, dtype=torch.bool)
    search_supported = torch.tensor(search_flags, device=device, dtype=torch.float32)
    choice_supported = torch.tensor(choice_flags, device=device, dtype=torch.float32)
    amp_dtype = (
        torch.bfloat16
        if device.type == "cuda" and torch.cuda.is_bf16_supported()
        else torch.float16
    )
    utility_parts: list[torch.Tensor] = []
    for start in range(0, len(owners), INFERENCE_BATCH_SIZE):
        end = min(start + INFERENCE_BATCH_SIZE, len(owners))
        with torch.autocast(
            device_type=device.type,
            dtype=amp_dtype,
            enabled=device.type == "cuda",
        ):
            prediction = model(
                state[start:end],
                action[start:end],
                numeric[start:end],
                history[start:end],
                inventory[start:end],
                candidate[start:end],
                action_numeric[start:end],
            )
            utility_parts.append(
                utilities(
                    prediction.float(),
                    combat[start:end],
                    search_supported[start:end],
                    choice_supported[start:end],
                    policy,
                    target_names,
                    model.policy_supported_targets,
                    combat_search_weight,
                    counterfactual_search_weight,
                    outside_search_weight,
                    counterfactual_outside_weight,
                )
            )
    utility = torch.cat(utility_parts)
    ranked: dict[int, list[tuple[float, int]]] = {}
    for (environment, action_index), score in zip(owners, utility.cpu().tolist()):
        if temperature > 0.0:
            score += temperature * exploration_rngs[environment].gumbel()
        ranked.setdefault(environment, []).append((score, action_index))
    for candidates in ranked.values():
        candidates.sort(reverse=True)
    return ranked


def choose_actions(
    model: SelfPlayHrm,
    rows: list[dict[str, Any]],
    device: torch.device,
    policy: str,
    tried_actions: list[dict[tuple[int, ...], set[int]]],
    exploration_rngs: list[SplitMix64],
    temperature: float,
    target_names: tuple[str, ...],
    histories: list[list[int]],
    combat_search_weight: float,
    counterfactual_search_weight: float,
    outside_search_weight: float,
    counterfactual_outside_weight: float,
    record_tried: bool = True,
) -> list[int | None]:
    ranked = rank_actions(
        model,
        rows,
        device,
        policy,
        exploration_rngs,
        temperature,
        target_names,
        histories,
        combat_search_weight,
        counterfactual_search_weight,
        outside_search_weight,
        counterfactual_outside_weight,
    )
    choices: list[int | None] = [None] * len(rows)
    for environment, candidates in ranked.items():
        state_key = observation_key(rows[environment]["observation"])
        tried = tried_actions[environment].setdefault(state_key, set())
        action_index = next(
            (action for _, action in candidates if action not in tried),
            candidates[0][1],
        )
        if record_tried:
            tried.add(action_index)
        choices[environment] = action_index
    return choices


def branch_score(
    root: dict[str, Any],
    leaf: dict[str, Any],
    learned_leaf_value: float,
) -> float:
    """Score an exact self-play branch without a scripted action target."""
    measurements = leaf["measurements"]
    if leaf["outcome"] == "act3_boss_victory":
        return 1_000_000.0 + float(measurements["hp"])
    floor_progress = measurements["floor"] - root["floor"]
    hp_fraction = measurements["hp"] / max(measurements["max_hp"], 1)
    if root["enemy_max_hp"] <= 0:
        if leaf["outcome"] == "player_death":
            return -1_000.0 + 100.0 * floor_progress
        stalled = all(
            measurements[name] == root[name]
            for name in (
                "floor",
                "hp",
                "gold",
                "deck_size",
                "upgraded_cards",
                "relics",
            )
        )
        if stalled:
            return -500.0 + learned_leaf_value
        return (
            100.0 * floor_progress
            + 30.0 * hp_fraction
            + 8.0 * (measurements["relics"] - root["relics"])
            + 5.0 * (measurements["upgraded_cards"] - root["upgraded_cards"])
            + 0.02 * (measurements["gold"] - root["gold"])
            + learned_leaf_value
        )
    enemy_scale = max(root["enemy_max_hp"], 1)
    damage_progress = (root["enemy_hp"] - measurements["enemy_hp"]) / enemy_scale
    if leaf["outcome"] == "player_death":
        return -1_000.0 + 100.0 * damage_progress
    if root["enemy_max_hp"] > 0 and measurements["enemy_max_hp"] == 0:
        return 1_000.0 + 100.0 * hp_fraction
    stalled = all(
        measurements[name] == root[name]
        for name in (
            "hp",
            "enemy_hp",
            "combat_turn",
            "energy",
            "hand_size",
            "draw_size",
            "discard_size",
        )
    )
    if stalled:
        return -500.0 + learned_leaf_value
    return (
        50.0 * damage_progress
        + 20.0 * hp_fraction
        - 0.10 * measurements["incoming_attack"]
        + learned_leaf_value
    )


def improve_with_exact_combat_beam(
    process: subprocess.Popen[str],
    model: SelfPlayHrm,
    rows: list[dict[str, Any]],
    base_choices: list[int | None],
    histories: list[list[int]],
    tried_actions: list[dict[tuple[int, ...], set[int]]],
    device: torch.device,
    args: argparse.Namespace,
    target_names: tuple[str, ...],
    seeds: list[int],
    branch_records: list[dict[str, Any]],
) -> tuple[list[int | None], int, int]:
    """Repeatedly branch exact combat states instead of rolling out one policy.

    Width is shared by all root actions for one live environment. A root that
    leaves the beam retains its exact frontier value, while combat wins and
    deaths become terminal leaves. This makes width one equivalent to the
    legacy single-continuation experiment and keeps the new path opt-in.
    """
    if args.lookahead_depth <= 0 or args.lookahead_beam_width <= 1:
        return base_choices, 0, 0
    rank_rngs = [SplitMix64(args.exploration_seed ^ index) for index in range(len(rows))]
    root_ranked = rank_actions(
        model,
        rows,
        device,
        args.policy,
        rank_rngs,
        0.0,
        target_names,
        histories,
        args.combat_search_weight,
        args.counterfactual_search_weight,
        args.outside_search_weight,
        args.counterfactual_outside_weight,
    )
    roots: list[tuple[int, int, float]] = []
    for environment, candidates in root_ranked.items():
        measurements = rows[environment]["measurements"]
        if (
            measurements["enemy_max_hp"] < args.lookahead_min_enemy_hp
            or measurements["floor"] < args.lookahead_min_floor
        ):
            continue
        state_key = observation_key(rows[environment]["observation"])
        tried = tried_actions[environment].get(state_key, set())
        untried = [candidate for candidate in candidates if candidate[1] not in tried]
        for prior, action in untried[: args.lookahead_candidates]:
            roots.append((environment, action, prior))
    if not roots:
        return base_choices, 0, 0

    current_rows = request(
        process,
        {
            "op": "fork",
            "branches": [
                {"environment": environment, "action": action}
                for environment, action, _ in roots
            ],
        },
    )
    current_meta: list[dict[str, Any]] = []
    for environment, action, prior in roots:
        history = list(histories[environment])
        history.append(decision_signature(rows[environment]["observation"], action))
        current_meta.append(
            {
                "environment": environment,
                "root_action": action,
                "root_prior": prior,
                "history": history,
            }
        )
    simulated_steps = len(current_rows)
    leaf_values: dict[tuple[int, int], list[tuple[int, float]]] = {}

    for depth in range(1, args.lookahead_depth + 1):
        branch_rngs = [
            SplitMix64(args.exploration_seed ^ (depth << 32) ^ index)
            for index in range(len(current_rows))
        ]
        branch_histories = [meta["history"] for meta in current_meta]
        ranked = rank_actions(
            model,
            current_rows,
            device,
            args.policy,
            branch_rngs,
            0.0,
            target_names,
            branch_histories,
            args.combat_search_weight,
            args.counterfactual_search_weight,
            args.outside_search_weight,
            args.counterfactual_outside_weight,
        )
        scores: list[float] = []
        active_by_environment: dict[int, list[int]] = {}
        for index, (leaf, meta) in enumerate(zip(current_rows, current_meta)):
            candidates = ranked.get(
                index, [(float(meta["root_prior"]), int(meta["root_action"]))]
            )
            learned_leaf = candidates[0][0]
            environment = int(meta["environment"])
            score = branch_score(
                rows[environment]["measurements"], leaf, learned_leaf
            )
            scores.append(score)
            key = (environment, int(meta["root_action"]))
            combat_finished = (
                leaf["outcome"] != "running"
                or leaf["measurements"]["enemy_max_hp"] == 0
            )
            if combat_finished or depth == args.lookahead_depth or index not in ranked:
                leaf_values.setdefault(key, []).append((depth, score))
            else:
                active_by_environment.setdefault(environment, []).append(index)

        selected: list[int] = []
        for indices in active_by_environment.values():
            indices.sort(key=lambda index: scores[index], reverse=True)
            selected.extend(indices[: args.lookahead_beam_width])
            for index in indices[args.lookahead_beam_width :]:
                meta = current_meta[index]
                key = (int(meta["environment"]), int(meta["root_action"]))
                leaf_values.setdefault(key, []).append((depth, scores[index]))
        if depth == args.lookahead_depth or not selected:
            break

        expansions: list[dict[str, int]] = []
        next_meta: list[dict[str, Any]] = []
        for parent in selected:
            meta = current_meta[parent]
            observation = current_rows[parent]["observation"]
            for _, action in ranked[parent][: args.lookahead_beam_expansion]:
                expansions.append({"environment": parent, "action": action})
                history = list(meta["history"])
                history.append(decision_signature(observation, action))
                next_meta.append({**meta, "history": history})
        if not expansions:
            break
        current_rows = request(
            process, {"op": "branch_fork", "branches": expansions}
        )
        current_meta = next_meta
        simulated_steps += len(current_rows)

    root_values: dict[tuple[int, int], float] = {}
    for key, values in leaf_values.items():
        winning = [score for _, score in values if score >= 1_000.0]
        if winning:
            root_values[key] = max(winning)
            continue
        deepest = max(depth for depth, _ in values)
        root_values[key] = max(
            score for depth, score in values if depth == deepest
        )
    if args.branches_output is not None:
        for (environment, action), score in root_values.items():
            branch_records.append(
                {
                    "schema_version": 1,
                    "seed": seeds[environment],
                    "step": rows[environment]["steps"],
                    "observation": rows[environment]["observation"],
                    "before": rows[environment]["measurements"],
                    "history": histories[environment][-MAX_HISTORY_STEPS:],
                    "action_index": action,
                    "branch_score": score,
                }
            )
    best: dict[int, tuple[float, int]] = {}
    for (environment, action), score in root_values.items():
        if environment not in best or score > best[environment][0]:
            best[environment] = (score, action)
    improved = list(base_choices)
    for environment, (_, action) in best.items():
        improved[environment] = action
    return improved, simulated_steps, len(best)


def improve_with_exact_lookahead(
    process: subprocess.Popen[str],
    model: SelfPlayHrm,
    rows: list[dict[str, Any]],
    base_choices: list[int | None],
    histories: list[list[int]],
    tried_actions: list[dict[tuple[int, ...], set[int]]],
    device: torch.device,
    args: argparse.Namespace,
    target_names: tuple[str, ...],
    seeds: list[int],
    branch_records: list[dict[str, Any]],
) -> tuple[list[int | None], int, int]:
    if args.lookahead_depth <= 0:
        return base_choices, 0, 0
    rank_rngs = [SplitMix64(args.exploration_seed ^ index) for index in range(len(rows))]
    ranked = rank_actions(
        model,
        rows,
        device,
        args.policy,
        rank_rngs,
        0.0,
        target_names,
        histories,
        args.combat_search_weight,
        args.counterfactual_search_weight,
        args.outside_search_weight,
        args.counterfactual_outside_weight,
    )
    branch_roots: list[tuple[int, int, float, int, int]] = []
    for environment, candidates in ranked.items():
        measurements = rows[environment]["measurements"]
        observation = rows[environment]["observation"]
        identity_choice = (
            measurements["enemy_max_hp"] == 0
            and sum(
                bool(action.get("candidate_identities"))
                for action in observation["actions"]
            )
            >= 1
        )
        if measurements["floor"] < args.lookahead_min_floor:
            continue
        if args.lookahead_beam_width > 1 and measurements["enemy_max_hp"] > 0:
            continue
        if args.lookahead_noncombat_only:
            if measurements["enemy_max_hp"] > 0:
                continue
        elif args.lookahead_identity_choices_only:
            if not identity_choice:
                continue
        elif (
            measurements["enemy_max_hp"] < args.lookahead_min_enemy_hp
            and not (
                args.lookahead_include_identity_choices and identity_choice
            )
        ):
            continue
        root_depth = (
            args.lookahead_boss_depth
            if measurements["enemy_max_hp"] > 0
            and measurements["floor"] >= 50
            and args.lookahead_boss_depth is not None
            else args.lookahead_identity_depth
            if identity_choice
            and not args.lookahead_identity_choices_only
            and args.lookahead_identity_depth is not None
            else args.lookahead_depth
        )
        state_key = observation_key(observation)
        tried = tried_actions[environment].get(state_key, set())
        untried = [candidate for candidate in candidates if candidate[1] not in tried]
        for prior, action in untried[: args.lookahead_candidates]:
            for rollout in range(args.lookahead_rollouts):
                branch_roots.append(
                    (environment, action, prior, rollout, root_depth)
                )
    if not branch_roots:
        return base_choices, 0, 0

    branches = [
        {"environment": environment, "action": action}
        for environment, action, _, _, _ in branch_roots
    ]
    branch_rows = request(process, {"op": "fork", "branches": branches})
    branch_histories: list[list[int]] = []
    for environment, action, _, _, _ in branch_roots:
        history = list(histories[environment])
        history.append(decision_signature(rows[environment]["observation"], action))
        branch_histories.append(history)
    branch_tried: list[dict[tuple[int, ...], set[int]]] = [
        {} for _ in branch_roots
    ]
    branch_rngs = [
        SplitMix64(
            args.exploration_seed
            ^ (environment << 32)
            ^ action
            ^ rows[environment]["steps"]
            ^ (rollout * 0x9E3779B97F4A7C15)
        )
        for environment, action, _, rollout, _ in branch_roots
    ]
    simulated_steps = len(branch_rows)
    for branch_step in range(1, max(root[-1] for root in branch_roots)):
        branch_actions = choose_actions(
            model,
            branch_rows,
            device,
            args.policy,
            branch_tried,
            branch_rngs,
            args.lookahead_temperature,
            target_names,
            branch_histories,
            args.combat_search_weight,
            args.counterfactual_search_weight,
            args.outside_search_weight,
            args.counterfactual_outside_weight,
        )
        branch_actions = [
            action if branch_step < root_depth else None
            for action, (_, _, _, _, root_depth) in zip(
                branch_actions, branch_roots
            )
        ]
        if not any(action is not None for action in branch_actions):
            break
        before = branch_rows
        branch_rows = request(
            process, {"op": "branch_step", "actions": branch_actions}
        )
        simulated_steps += sum(action is not None for action in branch_actions)
        for index, action in enumerate(branch_actions):
            if action is not None:
                branch_histories[index].append(
                    decision_signature(before[index]["observation"], action)
                )

    leaf_rngs = [SplitMix64(args.exploration_seed ^ index) for index in range(len(branch_rows))]
    leaf_ranked = rank_actions(
        model,
        branch_rows,
        device,
        args.policy,
        leaf_rngs,
        0.0,
        target_names,
        branch_histories,
        args.combat_search_weight,
        args.counterfactual_search_weight,
        args.outside_search_weight,
        args.counterfactual_outside_weight,
    )
    action_samples: dict[tuple[int, int], list[float]] = {}
    for branch, ((environment, action, prior, _, _), leaf) in enumerate(
        zip(branch_roots, branch_rows)
    ):
        learned_leaf = leaf_ranked.get(branch, [(prior, action)])[0][0]
        score = branch_score(rows[environment]["measurements"], leaf, learned_leaf)
        key = (environment, action)
        action_samples.setdefault(key, []).append(score)
    action_values: dict[tuple[int, int], float] = {}
    for key, samples in action_samples.items():
        mean = sum(samples) / len(samples)
        variance = sum((sample - mean) ** 2 for sample in samples) / len(samples)
        action_values[key] = mean + args.lookahead_optimism * math.sqrt(variance)
    if args.branches_output is not None:
        for (environment, action), score in action_values.items():
            branch_records.append(
                {
                    "schema_version": 1,
                    "seed": seeds[environment],
                    "step": rows[environment]["steps"],
                    "observation": rows[environment]["observation"],
                    "before": rows[environment]["measurements"],
                    "history": histories[environment][-MAX_HISTORY_STEPS:],
                    "action_index": action,
                    "branch_score": score,
                }
            )
    best: dict[int, tuple[float, int]] = {}
    for (environment, action), score in action_values.items():
        if environment not in best or score > best[environment][0]:
            best[environment] = (score, action)
    improved = list(base_choices)
    for environment, (_, action) in best.items():
        improved[environment] = action
    return improved, simulated_steps, len(best)


def open_output(path: Path) -> TextIO:
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.suffix == ".xz":
        return lzma.open(path, "wt", encoding="utf-8")
    return path.open("w", encoding="utf-8")


def write_jsonl(path: Path, rows: list[dict[str, Any]]) -> None:
    with open_output(path) as output:
        for row in rows:
            output.write(json.dumps(row, separators=(",", ":")) + "\n")


def load_resume_prefixes(
    path: Path,
    min_floor: int,
    min_enemy_hp: int,
    max_enemy_hp: int | None,
    max_terminal_enemy_hp: int | None,
) -> tuple[list[int], list[list[dict[str, Any]]], list[dict[str, Any]]]:
    source: TextIO
    if path.suffix == ".xz":
        source = lzma.open(path, "rt", encoding="utf-8")
    else:
        source = path.open("r", encoding="utf-8")
    seeds: list[int] = []
    prefixes: list[list[dict[str, Any]]] = []
    roots: list[dict[str, Any]] = []
    with source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            episode = json.loads(line)
            if episode.get("schema_version") != 1:
                raise ValueError(f"{path}:{line_number}: unsupported trace schema")
            if max_terminal_enemy_hp is not None:
                terminal_enemy_hp = int(
                    episode.get("result", {}).get("terminal", {}).get(
                        "enemy_hp", 2**31 - 1
                    )
                )
                if terminal_enemy_hp > max_terminal_enemy_hp:
                    continue
            transitions = episode["transitions"]
            resume_index = next(
                (
                    index
                    for index, transition in enumerate(transitions)
                    if transition["before"]["floor"] >= min_floor
                    and transition["before"]["enemy_hp"] >= min_enemy_hp
                    and (
                        max_enemy_hp is None
                        or transition["before"]["enemy_hp"] <= max_enemy_hp
                    )
                ),
                None,
            )
            if resume_index is None:
                continue
            seeds.append(int(episode["result"]["seed"]))
            prefixes.append(transitions[:resume_index])
            roots.append(transitions[resume_index]["observation"])
    if not seeds:
        raise ValueError(
            f"{path}: no resumable state at floor>={min_floor} "
            f"and enemy_hp>={min_enemy_hp}"
        )
    return seeds, prefixes, roots


def evaluate(args: argparse.Namespace) -> None:
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    if checkpoint.get("teacher") is not None:
        raise ValueError("checkpoint is not marked teacher-free")
    if checkpoint.get("format") != "sts-selfplay-hrm-v1":
        raise ValueError(f"unsupported checkpoint format {checkpoint.get('format')!r}")
    device = torch.device(
        "cuda" if args.device == "auto" and torch.cuda.is_available() else args.device
    )
    target_names = tuple(checkpoint["target_names"])
    model_config = dict(checkpoint["config"])
    model_config["target_names"] = target_names
    model = SelfPlayHrm(model_config)
    model.load_state_dict(checkpoint["model"])
    if args.counterfactual_adapter_scale is not None:
        if model.counterfactual_value_adapter is None:
            raise ValueError(
                "counterfactual-adapter-scale requires an adapter checkpoint"
            )
        model.counterfactual_adapter_scale = args.counterfactual_adapter_scale
    if args.counterfactual_adapter_min_enemy_hp is not None:
        if model.counterfactual_value_adapter is None:
            raise ValueError(
                "counterfactual-adapter-min-enemy-hp requires an adapter checkpoint"
            )
        model.counterfactual_adapter_min_enemy_hp = (
            args.counterfactual_adapter_min_enemy_hp
        )
    if args.menu_residual_scale is not None:
        if model.choice_critic is None or model.choice_critic.menu_residual is None:
            raise ValueError(
                "menu-residual-scale requires a checkpoint with a menu residual"
            )
        model.choice_critic.menu_residual_scale = args.menu_residual_scale
    dataset_signature = checkpoint.get("dataset_signature", {})
    model.search_value_supported = bool(
        checkpoint.get(
            "search_value_supported",
            dataset_signature.get("branch_datasets", ["legacy-checkpoint"]),
        )
    )
    model.search_value_min_floor = int(checkpoint.get("search_value_min_floor", 16))
    model.policy_supported_targets = frozenset(
        checkpoint.get("policy_supported_targets", target_names)
    )
    model.to(device).eval()

    resume_prefixes: list[list[dict[str, Any]]] | None = None
    resume_roots: list[dict[str, Any]] | None = None
    if args.resume_traces_jsonl is not None:
        seeds, resume_prefixes, resume_roots = load_resume_prefixes(
            args.resume_traces_jsonl,
            args.resume_min_floor,
            args.resume_min_enemy_hp,
            args.resume_max_enemy_hp,
            args.resume_max_terminal_enemy_hp,
        )
        if args.resume_copies > 1:
            seeds = [seed for seed in seeds for _ in range(args.resume_copies)]
            resume_prefixes = [
                prefix
                for prefix in resume_prefixes
                for _ in range(args.resume_copies)
            ]
            resume_roots = [
                root for root in resume_roots for _ in range(args.resume_copies)
            ]
    elif args.seeds_jsonl is not None:
        seeds = load_seeds(args.seeds_jsonl, args.seeds_min_floor)
        if args.seed_limit is not None:
            seeds = seeds[: args.seed_limit]
    else:
        seed_rng = SplitMix64(args.seed_source)
        seeds = [signed_i64(seed_rng.next()) for _ in range(args.count)]
    if args.resume_traces_jsonl is None and args.seed_copies > 1:
        seeds = [seed for seed in seeds for _ in range(args.seed_copies)]
    binary = args.engine
    if not binary.is_file():
        raise FileNotFoundError(f"build sts-selfplay first; missing {binary}")
    process = subprocess.Popen(
        [
            str(binary),
            "--serve-jsonl",
            "--ascension",
            str(args.ascension),
            "--max-steps",
            str(args.max_steps),
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    started = time.monotonic()
    terminal_rows: dict[int, dict[str, Any]] = {}
    total_steps = 0
    replay_steps = 0
    branch_steps = 0
    lookahead_decisions = 0
    next_progress_step = 25_000
    # Rebuild replayed prefixes from current engine observations. This keeps
    # old traces schema-compatible while enriching the complete trajectory
    # when new visible features are introduced.
    traces: list[list[dict[str, Any]]] = [[] for _ in seeds]
    branch_records: list[dict[str, Any]] = []
    max_floors = [0 for _ in seeds]
    try:
        rows = request(process, {"op": "reset", "seeds": seeds})
        tried_actions: list[dict[tuple[int, ...], set[int]]] = [
            {} for _ in seeds
        ]
        histories: list[list[int]] = [[] for _ in seeds]
        if resume_prefixes is not None:
            for index, prefix in enumerate(resume_prefixes):
                histories[index] = [
                    decision_signature(
                        transition["observation"], transition["action_index"]
                    )
                    for transition in prefix
                ]
                for transition in prefix:
                    max_floors[index] = max(
                        max_floors[index],
                        transition["before"]["floor"],
                        transition["after"]["floor"],
                    )
            for replay_step in range(max(map(len, resume_prefixes), default=0)):
                before_replay = rows
                replay_actions: list[int | None] = []
                for index, prefix in enumerate(resume_prefixes):
                    if replay_step >= len(prefix):
                        replay_actions.append(None)
                        continue
                    expected = prefix[replay_step]
                    if not contains_expected(
                        rows[index]["observation"], expected["observation"]
                    ):
                        raise RuntimeError(
                            f"resume replay diverged for seed {seeds[index]} "
                            f"at prefix step {replay_step}"
                        )
                    replay_actions.append(int(expected["action_index"]))
                rows = request(process, {"op": "step", "actions": replay_actions})
                if args.transitions_output is not None:
                    for index, action_index in enumerate(replay_actions):
                        if action_index is None:
                            continue
                        before = before_replay[index]
                        after = rows[index]
                        traces[index].append(
                            {
                                "step": before["steps"],
                                "observation": before["observation"],
                                "action_index": action_index,
                                "before": before["measurements"],
                                "after": after["measurements"],
                                "reward": after["reward"],
                                "outcome": after["outcome"],
                            }
                        )
                replay_steps += sum(action is not None for action in replay_actions)
            assert resume_roots is not None
            for index, expected_root in enumerate(resume_roots):
                if not contains_expected(rows[index]["observation"], expected_root):
                    raise RuntimeError(
                        f"resume root diverged for seed {seeds[index]} "
                        f"after {len(resume_prefixes[index])} prefix steps"
                    )
        exploration_rngs = [
            SplitMix64(
                (seed & ((1 << 64) - 1))
                ^ args.exploration_seed
                ^ (index * 0x9E3779B97F4A7C15)
            )
            for index, seed in enumerate(seeds)
        ]
        for row in rows:
            max_floors[row["index"]] = max(
                max_floors[row["index"]], row["measurements"]["floor"]
            )
        while len(terminal_rows) < len(seeds):
            before_rows = rows
            actions = choose_actions(
                model,
                before_rows,
                device,
                args.policy,
                tried_actions,
                exploration_rngs,
                args.temperature,
                target_names,
                histories,
                args.combat_search_weight,
                args.counterfactual_search_weight,
                args.outside_search_weight,
                args.counterfactual_outside_weight,
                False,
            )
            actions, simulated, improved = improve_with_exact_combat_beam(
                process,
                model,
                before_rows,
                actions,
                histories,
                tried_actions,
                device,
                args,
                target_names,
                seeds,
                branch_records,
            )
            branch_steps += simulated
            lookahead_decisions += improved
            actions, simulated, improved = improve_with_exact_lookahead(
                process,
                model,
                before_rows,
                actions,
                histories,
                tried_actions,
                device,
                args,
                target_names,
                seeds,
                branch_records,
            )
            branch_steps += simulated
            lookahead_decisions += improved
            for index, action in enumerate(actions):
                if action is not None and before_rows[index].get("observation") is not None:
                    key = observation_key(before_rows[index]["observation"])
                    tried_actions[index].setdefault(key, set()).add(action)
            rows = request(process, {"op": "step", "actions": actions})
            total_steps += sum(action is not None for action in actions)
            for row in rows:
                index = row["index"]
                max_floors[index] = max(max_floors[index], row["measurements"]["floor"])
                action_index = actions[index]
                if action_index is not None:
                    histories[index].append(
                        decision_signature(before_rows[index]["observation"], action_index)
                    )
                if action_index is not None and args.transitions_output is not None:
                    before = before_rows[index]
                    traces[index].append(
                        {
                            "step": before["steps"],
                            "observation": before["observation"],
                            "action_index": action_index,
                            "before": before["measurements"],
                            "after": row["measurements"],
                            "reward": row["reward"],
                            "outcome": row["outcome"],
                        }
                    )
                if row["outcome"] != "running" and row["index"] not in terminal_rows:
                    terminal_rows[row["index"]] = row
            if total_steps >= next_progress_step:
                print(
                    f"progress terminal={len(terminal_rows)}/{len(seeds)} "
                    f"steps={total_steps}",
                    flush=True,
                )
                while next_progress_step <= total_steps:
                    next_progress_step += 25_000
    finally:
        if process.stdin is not None:
            process.stdin.close()
        process.terminate()
        process.wait(timeout=10)

    results = []
    for index, seed in enumerate(seeds):
        row = terminal_rows[index]
        results.append(
            {
                "seed": seed,
                "steps": row["steps"],
                "max_floor": max_floors[index],
                "outcome": row["outcome"],
                "terminal_score": row["terminal_score"],
                "terminal": row["measurements"],
            }
        )
    write_jsonl(args.output, results)
    if args.transitions_output is not None:
        episodes = [
            {
                "schema_version": 1,
                "result": result,
                "transitions": traces[index],
            }
            for index, result in enumerate(results)
        ]
        write_jsonl(args.transitions_output, episodes)
    if args.branches_output is not None:
        write_jsonl(args.branches_output, branch_records)
    elapsed = time.monotonic() - started
    wins = sum(result["outcome"] == "act3_boss_victory" for result in results)
    mean_floor = sum(result["max_floor"] for result in results) / len(results)
    mean_steps = sum(result["steps"] for result in results) / len(results)
    summary = {
        "checkpoint": str(args.checkpoint),
        "policy": args.policy,
        "teacher": None,
        "ascension": args.ascension,
        "temperature": args.temperature,
        "exploration_seed": args.exploration_seed,
        "seed_source": args.seed_source,
        "seeds_jsonl": str(args.seeds_jsonl) if args.seeds_jsonl is not None else None,
        "seed_limit": args.seed_limit,
        "seed_copies": args.seed_copies,
        "resume_traces_jsonl": (
            str(args.resume_traces_jsonl)
            if args.resume_traces_jsonl is not None
            else None
        ),
        "resume_copies": args.resume_copies,
        "resume_max_enemy_hp": args.resume_max_enemy_hp,
        "resume_max_terminal_enemy_hp": args.resume_max_terminal_enemy_hp,
        "replay_steps": replay_steps,
        "episodes": len(results),
        "wins": wins,
        "win_rate": wins / len(results),
        "mean_floor": mean_floor,
        "mean_steps": mean_steps,
        "elapsed_seconds": elapsed,
        "engine_steps_per_second": (total_steps + replay_steps) / elapsed,
        "lookahead_depth": args.lookahead_depth,
        "lookahead_candidates": args.lookahead_candidates,
        "lookahead_rollouts": args.lookahead_rollouts,
        "lookahead_beam_width": args.lookahead_beam_width,
        "lookahead_beam_expansion": args.lookahead_beam_expansion,
        "lookahead_temperature": args.lookahead_temperature,
        "lookahead_optimism": args.lookahead_optimism,
        "lookahead_min_enemy_hp": args.lookahead_min_enemy_hp,
        "lookahead_min_floor": args.lookahead_min_floor,
        "lookahead_identity_choices_only": args.lookahead_identity_choices_only,
        "lookahead_include_identity_choices": args.lookahead_include_identity_choices,
        "lookahead_identity_depth": args.lookahead_identity_depth,
        "lookahead_boss_depth": args.lookahead_boss_depth,
        "lookahead_noncombat_only": args.lookahead_noncombat_only,
        "combat_search_weight": args.combat_search_weight,
        "search_value_min_floor": model.search_value_min_floor,
        "counterfactual_search_weight": args.counterfactual_search_weight,
        "counterfactual_adapter_scale": (
            model.counterfactual_adapter_scale
            if model.counterfactual_value_adapter is not None
            else None
        ),
        "counterfactual_adapter_min_enemy_hp": (
            model.counterfactual_adapter_min_enemy_hp
            if model.counterfactual_value_adapter is not None
            else None
        ),
        "outside_search_weight": args.outside_search_weight,
        "counterfactual_outside_weight": args.counterfactual_outside_weight,
        "menu_residual_scale": (
            model.choice_critic.menu_residual_scale
            if model.choice_critic is not None
            and model.choice_critic.menu_residual is not None
            else None
        ),
        "lookahead_decisions": lookahead_decisions,
        "branch_steps": branch_steps,
        "output": str(args.output),
        "transitions_output": (
            str(args.transitions_output) if args.transitions_output is not None else None
        ),
        "branches_output": (
            str(args.branches_output) if args.branches_output is not None else None
        ),
        "branch_records": len(branch_records),
    }
    args.output.with_suffix(".summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n"
    )
    print(json.dumps(summary, indent=2, sort_keys=True), flush=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-selfplay-hrm-20s.pt"),
    )
    parser.add_argument(
        "--engine", type=Path, default=Path("target/release/sts-selfplay")
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-selfplay-eval.jsonl"),
    )
    parser.add_argument(
        "--transitions-output",
        type=Path,
        help="write complete teacher-free policy trajectories (optionally .xz)",
    )
    parser.add_argument(
        "--branches-output",
        type=Path,
        help="write exact self-play branch values for dynamic legal actions",
    )
    parser.add_argument("--policy", choices=("floor", "local", "hybrid"), default="hybrid")
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--ascension", type=int, choices=range(21), default=0)
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--seed-source", type=int, default=20260827)
    parser.add_argument(
        "--seed-copies",
        type=int,
        default=1,
        help=(
            "independently explore each first-choice seed this many times; "
            "useful for teacher-free common-random-number action comparisons"
        ),
    )
    parser.add_argument(
        "--seeds-jsonl",
        type=Path,
        help="evaluate explicit integer or result-object seeds from their first choice",
    )
    parser.add_argument(
        "--seeds-min-floor",
        type=int,
        default=0,
        help="when seed rows contain max_floor, keep only prior frontier runs",
    )
    parser.add_argument(
        "--seed-limit",
        type=int,
        help="limit an explicit seed file after filtering, preserving its order",
    )
    parser.add_argument(
        "--resume-traces-jsonl",
        type=Path,
        help="replay verified self-play prefixes and resume at a deep state",
    )
    parser.add_argument("--resume-min-floor", type=int, default=33)
    parser.add_argument("--resume-min-enemy-hp", type=int, default=1)
    parser.add_argument(
        "--resume-max-enemy-hp",
        type=int,
        help="resume at the first matching state no more than this enemy HP",
    )
    parser.add_argument(
        "--resume-max-terminal-enemy-hp",
        type=int,
        help="resume only source episodes whose terminal enemy HP is at most this value",
    )
    parser.add_argument(
        "--resume-copies",
        type=int,
        default=1,
        help="run this many independently explored suffixes from each resumed state",
    )
    parser.add_argument(
        "--temperature",
        type=float,
        default=0.0,
        help="Gumbel action-exploration temperature; zero is greedy",
    )
    parser.add_argument("--exploration-seed", type=int, default=0x51F9A7)
    parser.add_argument(
        "--combat-search-weight",
        type=float,
        default=1.5,
        help="weight of the learned exact-branch value in combat",
    )
    parser.add_argument(
        "--outside-search-weight",
        type=float,
        default=0.0,
        help="weight of the learned exact-branch value outside combat",
    )
    parser.add_argument(
        "--counterfactual-search-weight",
        type=float,
        default=0.0,
        help="residual weight of the isolated counterfactual critic in combat",
    )
    parser.add_argument(
        "--counterfactual-outside-weight",
        type=float,
        default=0.0,
        help="weight of the isolated critic on every non-combat decision",
    )
    parser.add_argument(
        "--counterfactual-adapter-scale",
        type=float,
        help=(
            "override the isolated exact-branch residual scale; zero reproduces "
            "the underlying policy and choice critic"
        ),
    )
    parser.add_argument(
        "--counterfactual-adapter-min-enemy-hp",
        type=float,
        help="apply the isolated residual only in combats at or above this max HP",
    )
    parser.add_argument(
        "--menu-residual-scale",
        type=float,
        help=(
            "override only the gated menu adapter scale; zero reproduces the "
            "underlying critic exactly"
        ),
    )
    parser.add_argument("--max-steps", type=int, default=5_000)
    parser.add_argument(
        "--lookahead-depth",
        type=int,
        default=0,
        help="exact cloned rollout depth; zero disables model-based planning",
    )
    parser.add_argument("--lookahead-candidates", type=int, default=4)
    parser.add_argument("--lookahead-rollouts", type=int, default=1)
    parser.add_argument(
        "--lookahead-beam-width",
        type=int,
        default=1,
        help="active exact combat paths retained per live environment",
    )
    parser.add_argument(
        "--lookahead-beam-expansion",
        type=int,
        default=2,
        help="top model actions forked from each retained combat path",
    )
    parser.add_argument(
        "--lookahead-temperature",
        type=float,
        default=0.03,
        help="Gumbel temperature inside cloned continuations",
    )
    parser.add_argument(
        "--lookahead-optimism",
        type=float,
        default=0.0,
        help="standard-deviation bonus over mean cloned-return value",
    )
    parser.add_argument(
        "--lookahead-min-enemy-hp",
        type=int,
        default=100,
        help="plan only combats at or above this visible max-HP threshold",
    )
    parser.add_argument(
        "--lookahead-min-floor",
        type=int,
        default=0,
        help="plan only at or beyond this reached floor",
    )
    parser.add_argument(
        "--lookahead-identity-choices-only",
        action="store_true",
        help="plan only non-combat choices that add, remove, or select inventory identities",
    )
    parser.add_argument(
        "--lookahead-include-identity-choices",
        action="store_true",
        help="plan inventory-identity menus in addition to qualifying combats",
    )
    parser.add_argument(
        "--lookahead-identity-depth",
        type=int,
        help=(
            "separate exact rollout depth for included inventory menus; "
            "defaults to lookahead-depth"
        ),
    )
    parser.add_argument(
        "--lookahead-boss-depth",
        type=int,
        help=(
            "separate exact rollout depth for floor-50 boss combat; "
            "defaults to lookahead-depth"
        ),
    )
    parser.add_argument(
        "--lookahead-noncombat-only",
        action="store_true",
        help="plan route, event, rest, shop, and reward choices but not combat actions",
    )
    args = parser.parse_args()
    if (
        args.count <= 0
        or args.max_steps <= 0
        or args.seed_copies <= 0
        or args.temperature < 0
        or args.lookahead_depth < 0
        or args.lookahead_beam_width <= 0
        or args.lookahead_beam_expansion <= 0
        or (
            args.lookahead_identity_depth is not None
            and args.lookahead_identity_depth <= 0
        )
        or (
            args.lookahead_boss_depth is not None
            and args.lookahead_boss_depth <= 0
        )
        or args.lookahead_candidates <= 0
        or args.lookahead_rollouts <= 0
        or args.lookahead_temperature < 0
        or args.lookahead_optimism < 0
        or args.lookahead_min_enemy_hp < 0
        or args.lookahead_min_floor < 0
        or args.seeds_min_floor < 0
        or (args.seed_limit is not None and args.seed_limit <= 0)
        or args.resume_min_floor < 0
        or args.resume_min_enemy_hp < 0
        or (args.resume_max_enemy_hp is not None and args.resume_max_enemy_hp < 0)
        or (
            args.resume_max_terminal_enemy_hp is not None
            and args.resume_max_terminal_enemy_hp < 0
        )
        or args.resume_copies <= 0
        or args.combat_search_weight < 0
        or args.counterfactual_search_weight < 0
        or args.outside_search_weight < 0
        or args.counterfactual_outside_weight < 0
        or (
            args.counterfactual_adapter_scale is not None
            and args.counterfactual_adapter_scale < 0
        )
        or (
            args.counterfactual_adapter_min_enemy_hp is not None
            and args.counterfactual_adapter_min_enemy_hp < 0
        )
        or (args.menu_residual_scale is not None and args.menu_residual_scale < 0)
    ):
        parser.error("counts must be positive; temperatures and thresholds cannot be negative")
    if args.seeds_jsonl is not None and args.resume_traces_jsonl is not None:
        parser.error("seeds-jsonl and resume-traces-jsonl are mutually exclusive")
    if args.lookahead_identity_choices_only and args.lookahead_noncombat_only:
        parser.error(
            "identity-choices-only and noncombat-only lookahead are mutually exclusive"
        )
    if (
        args.lookahead_identity_choices_only
        and args.lookahead_include_identity_choices
    ):
        parser.error(
            "identity-choices-only already includes inventory choices"
        )
    return args


if __name__ == "__main__":
    evaluate(parse_args())
