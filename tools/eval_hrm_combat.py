#!/usr/bin/env python3
"""Run a trained combat HRM closed-loop through all boss-entry puzzles.

The user-facing entry point is the zero-argument sts-hrm-eval Rust binary. A
persistent JSON-line protocol keeps exact game transitions in the Rust engine
while this process performs batched checkpoint inference on the GPU.
"""

from __future__ import annotations

import argparse
import collections
import json
import lzma
from pathlib import Path
import shutil
import statistics
import subprocess
import tempfile
import time
from typing import Any

import torch

from train_hrm_combat import (
    CombatHrm,
    MODEL_DEFAULTS,
    action_key,
    masked_logits,
    state_tokens,
)


def log(message: str) -> None:
    print(message, flush=True)


class CheckpointPolicy:
    def __init__(self, checkpoint_path: Path, requested_device: str):
        checkpoint = torch.load(
            checkpoint_path,
            map_location="cpu",
            weights_only=False,
        )
        if checkpoint.get("model_defaults") != MODEL_DEFAULTS:
            raise ValueError(
                "checkpoint model defaults differ from this evaluator; "
                "use the trainer revision that created the checkpoint"
            )
        if requested_device == "auto":
            requested_device = "cuda" if torch.cuda.is_available() else "cpu"
        self.device = torch.device(requested_device)
        if self.device.type == "cuda" and not torch.cuda.is_available():
            raise RuntimeError("CUDA requested but unavailable")
        self.vocabulary: list[str] = checkpoint["vocabulary"]
        self.token_to_id = {
            token: index for index, token in enumerate(self.vocabulary)
        }
        self.action_list: list[str] = checkpoint["action_list"]
        self.action_to_id = {
            action: index for index, action in enumerate(self.action_list)
        }
        self.split_map = {
            int(puzzle_index): split
            for puzzle_index, split in checkpoint["split_map"].items()
        }
        self.model = CombatHrm(
            len(self.vocabulary),
            len(self.action_list),
        ).to(self.device)
        self.model.load_state_dict(checkpoint["model_state"], strict=True)
        self.model.eval()

    def _encode(
        self,
        decision: dict[str, Any],
    ) -> tuple[list[int], list[int]]:
        puzzle = {"boss": decision["boss"]}
        tokens = state_tokens(puzzle, decision)
        input_ids = [
            self.token_to_id.get(token, 1)
            for token in tokens[: MODEL_DEFAULTS["max_tokens"]]
        ]
        known_legal = []
        for action in decision["legal_actions"]:
            action_id = self.action_to_id.get(action_key(action))
            if action_id is not None:
                known_legal.append(action_id)
        return input_ids, known_legal

    def choose(self, decisions: list[dict[str, Any]]) -> list[dict[str, Any]]:
        choices: list[dict[str, Any] | None] = [None] * len(decisions)
        batch_size = MODEL_DEFAULTS["batch_size"] * 2
        for start in range(0, len(decisions), batch_size):
            batch = decisions[start : start + batch_size]
            input_ids = torch.zeros(
                (len(batch), MODEL_DEFAULTS["max_tokens"]),
                dtype=torch.int64,
                device=self.device,
            )
            legal_mask = torch.zeros(
                (len(batch), len(self.action_list)),
                dtype=torch.bool,
                device=self.device,
            )
            fallback_rows = set()
            for row, decision in enumerate(batch):
                encoded, known_legal = self._encode(decision)
                input_ids[row, : len(encoded)] = torch.tensor(
                    encoded,
                    dtype=torch.int64,
                    device=self.device,
                )
                if known_legal:
                    legal_mask[row, known_legal] = True
                else:
                    # The fixed action head cannot score a semantic action it
                    # never observed. Keep the rollout alive with a visible,
                    # deterministic first-legal fallback.
                    fallback_rows.add(row)

            with torch.inference_mode(), torch.autocast(
                device_type=self.device.type,
                dtype=torch.bfloat16,
                enabled=self.device.type == "cuda",
            ):
                carry = None
                for _ in range(MODEL_DEFAULTS["deep_supervision_segments"]):
                    carry, logits, _progress = self.model.segment(
                        input_ids,
                        carry,
                    )
                predictions = masked_logits(logits, legal_mask).argmax(dim=-1)

            for row, decision in enumerate(batch):
                if row in fallback_rows:
                    action = decision["legal_actions"][0]
                    fallback = True
                else:
                    action = json.loads(self.action_list[int(predictions[row])])
                    fallback = False
                choices[start + row] = {
                    "puzzle_index": decision["puzzle_index"],
                    "action": action,
                    "fallback": fallback,
                }
        if any(choice is None for choice in choices):
            raise AssertionError("missing checkpoint choice")
        return [choice for choice in choices if choice is not None]


def decode_fixture(source: Path, destination: Path) -> None:
    if source.suffix != ".xz":
        shutil.copyfile(source, destination)
        return
    with lzma.open(source, "rb") as compressed, destination.open("wb") as plain:
        shutil.copyfileobj(compressed, plain)


def monster_summary(monsters: list[dict[str, Any]]) -> str:
    if not monsters:
        return "-"
    parts = []
    for monster in monsters:
        phase = f":{monster['phase']}" if monster.get("phase") else ""
        parts.append(
            f"{monster['id']}{phase}:{monster['hp']}/{monster['max_hp']}"
        )
    return ",".join(parts)


def summarize(
    results: list[dict[str, Any]],
    checkpoint_path: Path,
    elapsed: float,
) -> dict[str, Any]:
    outcomes = collections.Counter(result["outcome"] for result in results)
    by_boss: dict[str, collections.Counter[str]] = collections.defaultdict(
        collections.Counter
    )
    by_split: dict[str, collections.Counter[str]] = collections.defaultdict(
        collections.Counter
    )
    for result in results:
        by_boss[result["boss"]][result["outcome"]] += 1
        by_split[result["split"]][result["outcome"]] += 1
    terminal = [result for result in results if result["outcome"] != "capped"]
    score_by_outcome = {}
    for outcome in ("win", "loss", "capped", "stopped"):
        selected = [result for result in results if result["outcome"] == outcome]
        if selected:
            score_by_outcome[outcome] = {
                "kind": selected[0]["score_kind"],
                "count": len(selected),
                "mean": sum(result["score"] for result in selected)
                / len(selected),
                "median": statistics.median(
                    result["score"] for result in selected
                ),
            }
    return {
        "schema_version": 1,
        "checkpoint": str(checkpoint_path.resolve()),
        "puzzles": len(results),
        "elapsed_seconds": elapsed,
        "outcomes": dict(outcomes),
        "win_rate": outcomes["win"] / max(1, len(results)),
        "mean_turns_played": sum(row["turns_played"] for row in results)
        / max(1, len(results)),
        "median_turns_played": statistics.median(
            row["turns_played"] for row in results
        ),
        "mean_actions_played": sum(row["actions_played"] for row in results)
        / max(1, len(results)),
        "median_actions_played": statistics.median(
            row["actions_played"] for row in results
        ),
        "terminal_puzzles": len(terminal),
        "terminal_mean_actions_played": sum(
            row["actions_played"] for row in terminal
        )
        / max(1, len(terminal)),
        "fallback_actions": sum(row["fallback_actions"] for row in results),
        "score_by_outcome": score_by_outcome,
        "by_boss": {boss: dict(counts) for boss, counts in sorted(by_boss.items())},
        "by_split": {
            split: dict(counts) for split, counts in sorted(by_split.items())
        },
    }


def write_results(
    results: list[dict[str, Any]],
    summary: dict[str, Any],
    output_dir: Path,
    checkpoint_path: Path,
) -> tuple[Path, Path, Path]:
    output_dir.mkdir(parents=True, exist_ok=True)
    stem = checkpoint_path.stem
    jsonl_path = output_dir / f"{stem}-rollouts.jsonl"
    tsv_path = output_dir / f"{stem}-rollouts.tsv"
    summary_path = output_dir / f"{stem}-rollout-summary.json"

    jsonl_pending = jsonl_path.with_suffix(".jsonl.pending")
    with jsonl_pending.open("w", encoding="utf-8") as output:
        for result in results:
            output.write(json.dumps(result, sort_keys=True) + "\n")
    jsonl_pending.replace(jsonl_path)

    columns = (
        "seed",
        "boss",
        "split",
        "outcome",
        "turns_played",
        "actions_played",
        "fallback_actions",
        "entry_hp",
        "player_hp",
        "player_hp_delta",
        "entry_encounter_hp",
        "encounter_hp_remaining",
        "encounter_hp_removed",
        "score_kind",
        "score",
        "final_screen",
        "last_action",
        "monsters_hp_remaining",
    )
    tsv_pending = tsv_path.with_suffix(".tsv.pending")
    with tsv_pending.open("w", encoding="utf-8") as output:
        output.write("\t".join(columns) + "\n")
        for result in results:
            row = dict(result)
            row["last_action"] = (
                action_key(result["last_action"])
                if result.get("last_action") is not None
                else "-"
            )
            row["monsters_hp_remaining"] = monster_summary(
                result["monsters_hp_remaining"]
            )
            output.write("\t".join(str(row[column]) for column in columns) + "\n")
    tsv_pending.replace(tsv_path)

    summary_pending = summary_path.with_suffix(".json.pending")
    summary_pending.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    summary_pending.replace(summary_path)
    return jsonl_path, tsv_path, summary_path


def evaluate(args: argparse.Namespace) -> dict[str, Any]:
    checkpoint_path = Path(args.checkpoint)
    source_fixture = Path(args.source_fixture)
    engine_path = Path(args.engine)
    policy = CheckpointPolicy(checkpoint_path, args.device)
    device_name = (
        torch.cuda.get_device_name(policy.device)
        if policy.device.type == "cuda"
        else "CPU"
    )
    log(f"checkpoint={checkpoint_path}; device={policy.device} ({device_name})")

    started = time.monotonic()
    with tempfile.TemporaryDirectory(prefix="sts-hrm-eval-") as temporary:
        decoded_fixture = Path(temporary) / "boss-entries.jsonl"
        decode_fixture(source_fixture, decoded_fixture)
        engine = subprocess.Popen(
            [
                str(engine_path),
                "--character",
                "DEFECT",
                "--a0",
                "--serve-hrm-rollouts-jsonl",
                str(decoded_fixture),
                "--rollout-max-actions",
                str(args.max_actions),
            ],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            bufsize=1,
        )
        if engine.stdin is None or engine.stdout is None:
            raise RuntimeError("could not open rollout engine protocol pipes")
        results = None
        inference_round = 0
        try:
            for line in engine.stdout:
                message = json.loads(line)
                if message.get("type") == "batch":
                    decisions = message["decisions"]
                    actions = policy.choose(decisions)
                    engine.stdin.write(
                        json.dumps({"type": "actions", "actions": actions}) + "\n"
                    )
                    engine.stdin.flush()
                    inference_round += 1
                    if inference_round % 10 == 0:
                        log(
                            f"checkpoint rollout: round {inference_round}, "
                            f"batch={len(decisions)}"
                        )
                elif message.get("type") == "complete":
                    results = message["results"]
                    break
                else:
                    raise RuntimeError(
                        f"unexpected rollout protocol message {message.get('type')!r}"
                    )
        finally:
            engine.stdin.close()
        engine_exit = engine.wait()
        if engine_exit != 0:
            raise RuntimeError(f"rollout engine exited with status {engine_exit}")
        if results is None:
            raise RuntimeError("rollout engine exited without terminal results")

    if len(results) != 500:
        raise AssertionError(f"expected 500 rollout results, got {len(results)}")
    puzzle_indices = {int(result["puzzle_index"]) for result in results}
    if puzzle_indices != set(range(500)):
        raise AssertionError("rollout results do not cover puzzle indices 0..499")
    for result in results:
        result["split"] = policy.split_map[int(result["puzzle_index"])]
    results.sort(key=lambda result: int(result["puzzle_index"]))

    elapsed = time.monotonic() - started
    summary = summarize(results, checkpoint_path, elapsed)
    jsonl_path, tsv_path, summary_path = write_results(
        results,
        summary,
        Path(args.output_dir),
        checkpoint_path,
    )
    log(
        f"closed-loop result: wins={summary['outcomes'].get('win', 0)}/500 "
        f"({100 * summary['win_rate']:.2f}%); "
        f"fallback_actions={summary['fallback_actions']}; "
        f"elapsed={elapsed:.2f}s"
    )
    log(f"rollouts_jsonl={jsonl_path}")
    log(f"rollouts_tsv={tsv_path}")
    log(f"summary={summary_path}")
    log("RESULT " + json.dumps(summary, sort_keys=True))
    return summary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Evaluate a combat HRM closed-loop.")
    parser.add_argument(
        "--checkpoint",
        default="artifacts/hrm/combat-hrm-5m.pt",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--source-fixture",
        default=(
            "fixtures/htn/"
            "defect-a0-act3-boss-winning-entry-500.jsonl.xz"
        ),
        help=argparse.SUPPRESS,
    )
    parser.add_argument("--engine", required=True, help=argparse.SUPPRESS)
    parser.add_argument("--output-dir", default="artifacts/hrm")
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--max-actions", type=int, default=1000)
    args = parser.parse_args()
    if args.max_actions <= 0:
        parser.error("--max-actions must be positive")
    return args


if __name__ == "__main__":
    try:
        evaluate(parse_args())
    except KeyboardInterrupt:
        raise SystemExit(130)
