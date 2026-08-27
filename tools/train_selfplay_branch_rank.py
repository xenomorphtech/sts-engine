#!/usr/bin/env python3
"""Fit a teacher-free listwise branch-ranking head on exact self-play search."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import time
from typing import Any

import torch
from torch import nn
from torch.nn import functional as F
from torch.utils.data import DataLoader, TensorDataset

from train_selfplay_hrm import (
    MAX_ACTION_FEATURES,
    MAX_HISTORY_STEPS,
    MAX_STATE_FEATURES,
    SelfPlayHrm,
    TARGET_NAMES,
    iter_branch_rows,
    measurement_vector,
    split_for_seed,
    symlog_scaled,
)


def group_key(row: dict[str, Any]) -> tuple[Any, ...]:
    observation = row["observation"]
    menu = tuple(tuple(action["features"]) for action in observation["actions"])
    return (
        int(row["seed"]),
        int(row["step"]),
        tuple(observation["state_features"]),
        menu,
    )


def prepare_groups(paths: list[Path]) -> dict[str, TensorDataset]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = {}
    for row in iter_branch_rows(paths):
        grouped.setdefault(group_key(row), []).append(row)
    groups = [
        rows
        for rows in grouped.values()
        if len(rows) >= 2
        and max(row["branch_score"] for row in rows)
        > min(row["branch_score"] for row in rows)
    ]
    max_candidates = max(len(rows) for rows in groups)
    split_groups = {"train": [], "validation": []}
    for rows in groups:
        split_groups[split_for_seed(int(rows[0]["seed"]))].append(rows)

    datasets: dict[str, TensorDataset] = {}
    for split, rows_by_group in split_groups.items():
        count = len(rows_by_group)
        state = torch.zeros(
            (count, max_candidates, MAX_STATE_FEATURES), dtype=torch.int32
        )
        action = torch.zeros(
            (count, max_candidates, MAX_ACTION_FEATURES), dtype=torch.int32
        )
        numeric = torch.zeros(
            (count, max_candidates, len(measurement_vector(rows_by_group[0][0]["before"]))),
            dtype=torch.float32,
        )
        history = torch.zeros(
            (count, max_candidates, MAX_HISTORY_STEPS), dtype=torch.int32
        )
        target = torch.zeros((count, max_candidates), dtype=torch.float32)
        mask = torch.zeros((count, max_candidates), dtype=torch.bool)
        for group_index, rows in enumerate(rows_by_group):
            for candidate, row in enumerate(rows):
                observation = row["observation"]
                selected = observation["actions"][row["action_index"]]
                state_ids = observation["state_features"][:MAX_STATE_FEATURES]
                action_ids = selected["features"][:MAX_ACTION_FEATURES]
                state[group_index, candidate, : len(state_ids)] = torch.tensor(state_ids)
                action[group_index, candidate, : len(action_ids)] = torch.tensor(action_ids)
                numeric[group_index, candidate] = torch.tensor(
                    measurement_vector(row["before"])
                )
                history_ids = row.get("history", [])[-MAX_HISTORY_STEPS:]
                if history_ids:
                    history[group_index, candidate, -len(history_ids) :] = torch.tensor(
                        history_ids
                    )
                target[group_index, candidate] = symlog_scaled(
                    float(row["branch_score"]), 1_000.0
                )
                mask[group_index, candidate] = True
        datasets[split] = TensorDataset(state, action, numeric, history, target, mask)
    print(
        f"prepared listwise groups train={len(datasets['train'])} "
        f"validation={len(datasets['validation'])} max_candidates={max_candidates}",
        flush=True,
    )
    return datasets


def forward_groups(
    model: SelfPlayHrm,
    batch: tuple[torch.Tensor, ...],
    device: torch.device,
    search_index: int,
) -> tuple[torch.Tensor, torch.Tensor, torch.Tensor]:
    state, action, numeric, history, target, mask = batch
    batch_size, candidates, _ = state.shape
    state = state.flatten(0, 1).to(device=device, dtype=torch.long, non_blocking=True)
    action = action.flatten(0, 1).to(device=device, dtype=torch.long, non_blocking=True)
    numeric = numeric.flatten(0, 1).to(device=device, non_blocking=True)
    history = history.flatten(0, 1).to(
        device=device, dtype=torch.long, non_blocking=True
    )
    target = target.to(device=device, non_blocking=True)
    mask = mask.to(device=device, non_blocking=True)
    amp_dtype = (
        torch.bfloat16
        if device.type == "cuda" and torch.cuda.is_bf16_supported()
        else torch.float16
    )
    with torch.autocast(
        device_type=device.type, dtype=amp_dtype, enabled=device.type == "cuda"
    ):
        prediction = model(state, action, numeric, history)[:, search_index]
    return prediction.reshape(batch_size, candidates).float(), target, mask


def listwise_loss(
    prediction: torch.Tensor,
    target: torch.Tensor,
    mask: torch.Tensor,
    target_temperature: float,
) -> torch.Tensor:
    floor = torch.finfo(prediction.dtype).min
    prediction = prediction.masked_fill(~mask, floor)
    target = (target / target_temperature).masked_fill(~mask, floor)
    preference = F.softmax(target, dim=-1)
    return -(preference * F.log_softmax(prediction, dim=-1)).sum(-1).mean()


@torch.inference_mode()
def evaluate(
    model: SelfPlayHrm,
    loader: DataLoader,
    device: torch.device,
    search_index: int,
) -> dict[str, float | int]:
    model.eval()
    groups = 0
    top1 = 0
    pair_correct = 0
    pair_count = 0
    for batch in loader:
        prediction, target, mask = forward_groups(model, batch, device, search_index)
        prediction = prediction.masked_fill(~mask, -torch.inf)
        target = target.masked_fill(~mask, -torch.inf)
        top1 += int(prediction.argmax(-1).eq(target.argmax(-1)).sum())
        groups += prediction.shape[0]
        prediction_delta = prediction.unsqueeze(2) - prediction.unsqueeze(1)
        target_delta = target.unsqueeze(2) - target.unsqueeze(1)
        pairs = mask.unsqueeze(2) & mask.unsqueeze(1) & target_delta.ne(0)
        pair_correct += int(((prediction_delta * target_delta > 0) & pairs).sum())
        pair_count += int(pairs.sum())
    return {
        "groups": groups,
        "top1_accuracy": top1 / max(groups, 1),
        "pairwise_accuracy": pair_correct / max(pair_count, 1),
    }


def train(args: argparse.Namespace) -> None:
    torch.manual_seed(args.seed)
    device = torch.device(
        "cuda" if args.device == "auto" and torch.cuda.is_available() else args.device
    )
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    if checkpoint.get("teacher") is not None:
        raise ValueError("checkpoint is not teacher-free")
    if tuple(checkpoint["target_names"]) != TARGET_NAMES:
        raise ValueError("checkpoint target heads do not match branch trainer")
    config = dict(checkpoint["config"])
    config["target_names"] = TARGET_NAMES
    model = SelfPlayHrm(config)
    model.load_state_dict(checkpoint["model"])
    for parameter in model.parameters():
        parameter.requires_grad_(False)
    for parameter in model.output[-1].parameters():
        parameter.requires_grad_(True)
    model.to(device)

    datasets = prepare_groups(args.branch_dataset)
    train_loader = DataLoader(
        datasets["train"],
        batch_size=args.batch_size,
        shuffle=True,
        num_workers=2,
        pin_memory=device.type == "cuda",
        persistent_workers=True,
    )
    validation_loader = DataLoader(
        datasets["validation"],
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=2,
        pin_memory=device.type == "cuda",
        persistent_workers=True,
    )
    optimizer = torch.optim.AdamW(
        model.output[-1].parameters(), lr=args.learning_rate, weight_decay=0.0
    )
    search_index = TARGET_NAMES.index("search_value")
    started = time.monotonic()
    updates = 0
    model.train()
    while time.monotonic() - started < args.seconds:
        for batch in train_loader:
            if time.monotonic() - started >= args.seconds:
                break
            optimizer.zero_grad(set_to_none=True)
            prediction, target, mask = forward_groups(
                model, batch, device, search_index
            )
            loss = listwise_loss(
                prediction, target, mask, args.target_temperature
            )
            if not torch.isfinite(loss):
                raise RuntimeError("non-finite listwise loss")
            loss.backward()
            nn.utils.clip_grad_norm_(model.output[-1].parameters(), 1.0)
            optimizer.step()
            updates += 1
    metrics = evaluate(model, validation_loader, device, search_index)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result = dict(checkpoint)
    result["model"] = model.state_dict()
    result["initialized_from"] = str(args.checkpoint)
    result["teacher"] = None
    result["branch_rank"] = {
        "method": "listwise_softmax",
        "updates": updates,
        "seconds": time.monotonic() - started,
        "target_temperature": args.target_temperature,
        "validation": metrics,
    }
    torch.save(result, args.output)
    args.output.with_suffix(".metrics.json").write_text(
        json.dumps(result["branch_rank"], indent=2, sort_keys=True) + "\n"
    )
    print(json.dumps(result["branch_rank"], indent=2, sort_keys=True), flush=True)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--checkpoint",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-generation7-branch-bf16-hrm-150s.pt"),
    )
    parser.add_argument("--branch-dataset", type=Path, action="append", required=True)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-generation7-branch-rank.pt"),
    )
    parser.add_argument("--seconds", type=float, default=30.0)
    parser.add_argument("--batch-size", type=int, default=64)
    parser.add_argument("--learning-rate", type=float, default=1e-3)
    parser.add_argument("--target-temperature", type=float, default=0.20)
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--seed", type=int, default=20260827)
    args = parser.parse_args()
    if args.seconds <= 0 or args.batch_size <= 0 or args.target_temperature <= 0:
        parser.error("seconds, batch size, and target temperature must be positive")
    if args.device == "auto":
        args.device = "cuda" if torch.cuda.is_available() else "cpu"
    return args


if __name__ == "__main__":
    train(parse_args())
