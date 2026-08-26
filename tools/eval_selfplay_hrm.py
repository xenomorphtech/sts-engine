#!/usr/bin/env python3
"""Closed-loop evaluation of a teacher-free whole-run HRM checkpoint."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import subprocess
import time
from typing import Any

import torch

from train_selfplay_hrm import (
    MAX_ACTION_FEATURES,
    MAX_STATE_FEATURES,
    SelfPlayHrm,
    TARGET_NAMES,
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


def signed_i64(value: int) -> int:
    return value if value < 1 << 63 else value - (1 << 64)


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


def utilities(prediction: torch.Tensor, combat: torch.Tensor, policy: str) -> torch.Tensor:
    head = {name: prediction[:, index] for index, name in enumerate(TARGET_NAMES)}
    if policy == "floor":
        return head["max_floor"] + 0.15 * head["terminal_margin"]
    combat_local = (
        2.0 * head["combat_margin"]
        + 0.50 * head["max_floor"]
        + 0.20 * head["terminal_margin"]
        + 0.25 * head["hp_delta_1"]
        - 0.25 * head["enemy_hp_delta_1"]
        + 0.10 * head["hp_delta_8"]
        - 0.10 * head["enemy_hp_delta_8"]
    )
    if policy == "local":
        outside = head["max_floor"] + 0.10 * head["terminal_margin"]
    else:
        outside = (
            head["max_floor"]
            + 0.25 * head["terminal_margin"]
            + 0.10 * head["floor_delta_32"]
            + 0.05 * head["gold_delta_32"]
            + 0.05 * head["relic_delta_128"]
            + 0.05 * head["upgrade_delta_128"]
        )
    return torch.where(combat, combat_local, outside)


@torch.inference_mode()
def choose_actions(
    model: SelfPlayHrm,
    rows: list[dict[str, Any]],
    device: torch.device,
    policy: str,
    tried_actions: list[dict[tuple[int, ...], set[int]]],
) -> list[int | None]:
    state_rows: list[list[int]] = []
    action_rows: list[list[int]] = []
    owners: list[tuple[int, int]] = []
    combat_flags: list[bool] = []
    for environment, row in enumerate(rows):
        observation = row.get("observation")
        if observation is None:
            continue
        for action_index, action in enumerate(observation["actions"]):
            state_rows.append(observation["state_features"])
            action_rows.append(action["features"])
            owners.append((environment, action_index))
            combat_flags.append(row["measurements"]["enemy_max_hp"] > 0)
    choices: list[int | None] = [None] * len(rows)
    if not owners:
        return choices
    state = pad_features(state_rows, MAX_STATE_FEATURES, device)
    action = pad_features(action_rows, MAX_ACTION_FEATURES, device)
    combat = torch.tensor(combat_flags, device=device, dtype=torch.bool)
    with torch.autocast(
        device_type=device.type,
        dtype=torch.float16,
        enabled=device.type == "cuda",
    ):
        prediction = model(state, action)
        utility = utilities(prediction.float(), combat, policy)
    ranked: dict[int, list[tuple[float, int]]] = {}
    for (environment, action_index), score in zip(owners, utility.cpu().tolist()):
        ranked.setdefault(environment, []).append((score, action_index))
    for environment, candidates in ranked.items():
        candidates.sort(reverse=True)
        state_key = tuple(rows[environment]["observation"]["state_features"])
        tried = tried_actions[environment].setdefault(state_key, set())
        action_index = next(
            (action for _, action in candidates if action not in tried),
            candidates[0][1],
        )
        tried.add(action_index)
        choices[environment] = action_index
    return choices


def evaluate(args: argparse.Namespace) -> None:
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    if checkpoint.get("teacher") is not None:
        raise ValueError("checkpoint is not marked teacher-free")
    if checkpoint.get("format") != "sts-selfplay-hrm-v1":
        raise ValueError(f"unsupported checkpoint format {checkpoint.get('format')!r}")
    device = torch.device(
        "cuda" if args.device == "auto" and torch.cuda.is_available() else args.device
    )
    model = SelfPlayHrm(checkpoint["config"])
    model.load_state_dict(checkpoint["model"])
    model.to(device).eval()

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
    try:
        rows = request(process, {"op": "reset", "seeds": seeds})
        tried_actions: list[dict[tuple[int, ...], set[int]]] = [
            {} for _ in seeds
        ]
        while len(terminal_rows) < len(seeds):
            actions = choose_actions(model, rows, device, args.policy, tried_actions)
            rows = request(process, {"op": "step", "actions": actions})
            total_steps += sum(action is not None for action in actions)
            for row in rows:
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
                "outcome": row["outcome"],
                "terminal_score": row["terminal_score"],
                "terminal": row["measurements"],
            }
        )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as output:
        for result in results:
            output.write(json.dumps(result, separators=(",", ":")) + "\n")
    elapsed = time.monotonic() - started
    wins = sum(result["outcome"] == "act3_boss_victory" for result in results)
    mean_floor = sum(result["terminal"]["floor"] for result in results) / len(results)
    mean_steps = sum(result["steps"] for result in results) / len(results)
    summary = {
        "checkpoint": str(args.checkpoint),
        "policy": args.policy,
        "teacher": None,
        "seed_source": args.seed_source,
        "episodes": len(results),
        "wins": wins,
        "win_rate": wins / len(results),
        "mean_floor": mean_floor,
        "mean_steps": mean_steps,
        "elapsed_seconds": elapsed,
        "engine_steps_per_second": total_steps / elapsed,
        "output": str(args.output),
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
    parser.add_argument("--policy", choices=("floor", "local", "hybrid"), default="hybrid")
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--count", type=int, default=100)
    parser.add_argument("--seed-source", type=int, default=20260827)
    parser.add_argument("--max-steps", type=int, default=5_000)
    args = parser.parse_args()
    if args.count <= 0 or args.max_steps <= 0:
        parser.error("count and max steps must be positive")
    return args


if __name__ == "__main__":
    evaluate(parse_args())
