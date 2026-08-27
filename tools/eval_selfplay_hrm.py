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
    MAX_ACTION_FEATURES,
    MAX_HISTORY_STEPS,
    MAX_STATE_FEATURES,
    SelfPlayHrm,
    decision_signature,
    measurement_vector,
)


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


def load_seeds(path: Path) -> list[int]:
    seeds: list[int] = []
    with path.open("r", encoding="utf-8") as source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            value = json.loads(line)
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
    policy: str,
    target_names: tuple[str, ...],
) -> torch.Tensor:
    head = {name: prediction[:, index] for index, name in enumerate(target_names)}
    zero = torch.zeros_like(prediction[:, 0])

    def value(name: str) -> torch.Tensor:
        return head.get(name, zero)

    if policy == "floor":
        return value("max_floor") + 0.15 * value("terminal_margin")
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
        + 0.50 * value("max_floor")
        + 0.20 * value("terminal_margin")
        + 0.25 * value("hp_delta_1")
        - 0.25 * value("enemy_hp_delta_1")
        + 0.10 * value("hp_delta_8")
        - 0.10 * value("enemy_hp_delta_8")
        + 1.50 * value("search_value") * search_supported
        + milestone
    )
    if policy == "local":
        outside = value("max_floor") + 0.10 * value("terminal_margin") + milestone
    else:
        outside = (
            value("max_floor")
            + 0.25 * value("terminal_margin")
            + 0.10 * value("floor_delta_32")
            + 0.05 * value("gold_delta_32")
            + 0.05 * value("relic_delta_128")
            + 0.05 * value("upgrade_delta_128")
            + milestone
        )
    return torch.where(combat, combat_local, outside)


def observation_key(observation: dict[str, Any]) -> tuple[int, ...]:
    """Identify the complete visible decision, including its legal-action menu."""
    key = list(observation["state_features"])
    key.append(0)
    for action in observation["actions"]:
        features = action["features"]
        key.append(len(features))
        key.extend(features)
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
) -> dict[int, list[tuple[float, int]]]:
    state_rows: list[list[int]] = []
    action_rows: list[list[int]] = []
    numeric_rows: list[list[float]] = []
    history_rows: list[list[int]] = []
    owners: list[tuple[int, int]] = []
    combat_flags: list[bool] = []
    search_flags: list[bool] = []
    for environment, row in enumerate(rows):
        observation = row.get("observation")
        if observation is None:
            continue
        for action_index, action in enumerate(observation["actions"]):
            state_rows.append(observation["state_features"])
            action_rows.append(action["features"])
            numeric_rows.append(measurement_vector(row["measurements"]))
            history_rows.append(histories[environment])
            owners.append((environment, action_index))
            combat_flags.append(row["measurements"]["enemy_max_hp"] > 0)
            search_flags.append(row["measurements"]["floor"] >= 16)
    if not owners:
        return {}
    state = pad_features(state_rows, MAX_STATE_FEATURES, device)
    action = pad_features(action_rows, MAX_ACTION_FEATURES, device)
    numeric = torch.tensor(numeric_rows, dtype=torch.float32, device=device)
    history = pad_history(history_rows, device)
    combat = torch.tensor(combat_flags, device=device, dtype=torch.bool)
    search_supported = torch.tensor(search_flags, device=device, dtype=torch.float32)
    amp_dtype = (
        torch.bfloat16
        if device.type == "cuda" and torch.cuda.is_bf16_supported()
        else torch.float16
    )
    with torch.autocast(
        device_type=device.type,
        dtype=amp_dtype,
        enabled=device.type == "cuda",
    ):
        prediction = model(state, action, numeric, history)
        utility = utilities(
            prediction.float(), combat, search_supported, policy, target_names
        )
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
    )
    branch_roots: list[tuple[int, int, float, int]] = []
    for environment, candidates in ranked.items():
        measurements = rows[environment]["measurements"]
        if (
            measurements["floor"] < args.lookahead_min_floor
            or measurements["enemy_max_hp"] < args.lookahead_min_enemy_hp
        ):
            continue
        state_key = observation_key(rows[environment]["observation"])
        tried = tried_actions[environment].get(state_key, set())
        untried = [candidate for candidate in candidates if candidate[1] not in tried]
        for prior, action in untried[: args.lookahead_candidates]:
            for rollout in range(args.lookahead_rollouts):
                branch_roots.append((environment, action, prior, rollout))
    if not branch_roots:
        return base_choices, 0, 0

    branches = [
        {"environment": environment, "action": action}
        for environment, action, _, _ in branch_roots
    ]
    branch_rows = request(process, {"op": "fork", "branches": branches})
    branch_histories: list[list[int]] = []
    for environment, action, _, _ in branch_roots:
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
        for environment, action, _, rollout in branch_roots
    ]
    simulated_steps = len(branch_rows)
    for _ in range(1, args.lookahead_depth):
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
        )
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
    )
    action_samples: dict[tuple[int, int], list[float]] = {}
    for branch, ((environment, action, prior, _), leaf) in enumerate(
        zip(branch_roots, branch_rows)
    ):
        learned_leaf = leaf_ranked.get(branch, [(prior, action)])[0][0]
        score = branch_score(rows[environment]["measurements"], leaf, learned_leaf)
        key = (environment, action)
        action_samples.setdefault(key, []).append(score)
    action_values = {
        key: sum(samples) / len(samples) for key, samples in action_samples.items()
    }
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
    model.to(device).eval()

    if args.seeds_jsonl is not None:
        seeds = load_seeds(args.seeds_jsonl)
    else:
        seed_rng = SplitMix64(args.seed_source)
        seeds = [signed_i64(seed_rng.next()) for _ in range(args.count)]
    binary = args.engine
    if not binary.is_file():
        raise FileNotFoundError(f"build sts-selfplay first; missing {binary}")
    process = subprocess.Popen(
        [str(binary), "--serve-jsonl", "--max-steps", str(args.max_steps)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        bufsize=1,
    )
    started = time.monotonic()
    terminal_rows: dict[int, dict[str, Any]] = {}
    total_steps = 0
    branch_steps = 0
    lookahead_decisions = 0
    traces: list[list[dict[str, Any]]] = [[] for _ in seeds]
    branch_records: list[dict[str, Any]] = []
    max_floors = [0 for _ in seeds]
    try:
        rows = request(process, {"op": "reset", "seeds": seeds})
        tried_actions: list[dict[tuple[int, ...], set[int]]] = [
            {} for _ in seeds
        ]
        histories: list[list[int]] = [[] for _ in seeds]
        exploration_rngs = [
            SplitMix64((seed & ((1 << 64) - 1)) ^ args.exploration_seed)
            for seed in seeds
        ]
        for row in rows:
            max_floors[row["index"]] = row["measurements"]["floor"]
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
                False,
            )
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
            if total_steps and total_steps % 25_000 < len(seeds):
                print(
                    f"progress terminal={len(terminal_rows)}/{len(seeds)} "
                    f"steps={total_steps}",
                    flush=True,
                )
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
        "temperature": args.temperature,
        "exploration_seed": args.exploration_seed,
        "seed_source": args.seed_source,
        "seeds_jsonl": str(args.seeds_jsonl) if args.seeds_jsonl is not None else None,
        "episodes": len(results),
        "wins": wins,
        "win_rate": wins / len(results),
        "mean_floor": mean_floor,
        "mean_steps": mean_steps,
        "elapsed_seconds": elapsed,
        "engine_steps_per_second": total_steps / elapsed,
        "lookahead_depth": args.lookahead_depth,
        "lookahead_candidates": args.lookahead_candidates,
        "lookahead_rollouts": args.lookahead_rollouts,
        "lookahead_temperature": args.lookahead_temperature,
        "lookahead_min_enemy_hp": args.lookahead_min_enemy_hp,
        "lookahead_min_floor": args.lookahead_min_floor,
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
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--seed-source", type=int, default=20260827)
    parser.add_argument(
        "--seeds-jsonl",
        type=Path,
        help="evaluate explicit integer or result-object seeds from their first choice",
    )
    parser.add_argument(
        "--temperature",
        type=float,
        default=0.0,
        help="Gumbel action-exploration temperature; zero is greedy",
    )
    parser.add_argument("--exploration-seed", type=int, default=0x51F9A7)
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
        "--lookahead-temperature",
        type=float,
        default=0.03,
        help="Gumbel temperature inside cloned continuations",
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
    args = parser.parse_args()
    if (
        args.count <= 0
        or args.max_steps <= 0
        or args.temperature < 0
        or args.lookahead_depth < 0
        or args.lookahead_candidates <= 0
        or args.lookahead_rollouts <= 0
        or args.lookahead_temperature < 0
        or args.lookahead_min_enemy_hp < 0
        or args.lookahead_min_floor < 0
    ):
        parser.error("counts must be positive; temperatures and thresholds cannot be negative")
    return args


if __name__ == "__main__":
    evaluate(parse_args())
