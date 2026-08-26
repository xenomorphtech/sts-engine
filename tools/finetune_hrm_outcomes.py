#!/usr/bin/env python3
"""Fine-tune a combat HRM from simulator-scored alternative actions."""

from __future__ import annotations

import argparse
import json
import random
import time
from pathlib import Path
from typing import Any

import torch
import torch.nn.functional as F
from torch import nn
from torch.utils.data import DataLoader, TensorDataset

from export_hrm_onnx import export_checkpoint
from train_hrm_combat import (
    BOSS_NAMES,
    MODEL_DEFAULTS,
    CombatHrm,
    action_key,
    choose_device,
    evaluate,
    make_loader,
    masked_logits,
    state_tokens,
)


def log(message: str) -> None:
    print(message, flush=True)


def iter_jsonl(path: Path):
    with path.open() as source:
        for line_number, line in enumerate(source, 1):
            if line.strip():
                try:
                    yield json.loads(line)
                except json.JSONDecodeError as error:
                    raise ValueError(f"{path}:{line_number}: {error}") from error


def prepare_preferences(
    path: Path,
    vocabulary: list[str],
    action_list: list[str],
    split_map: dict[int, str],
    temperature: float,
    minimum_span: float,
) -> tuple[dict[str, torch.Tensor], dict[str, Any]]:
    token_to_id = {token: index for index, token in enumerate(vocabulary)}
    action_to_id = {key: index for index, key in enumerate(action_list)}
    boss_to_id = {boss: index for index, boss in enumerate(BOSS_NAMES)}
    rows: list[dict[str, Any]] = []
    unknown_tokens = 0
    skipped_flat = 0

    for branch in iter_jsonl(path):
        puzzle_index = int(branch["puzzle_index"])
        if split_map.get(puzzle_index) != "train":
            raise ValueError(
                f"branch puzzle {puzzle_index} is not in the training split"
            )
        candidates = branch["candidates"]
        utilities = [float(candidate["utility"]) for candidate in candidates]
        if max(utilities) - min(utilities) < minimum_span:
            skipped_flat += 1
            continue
        decision = {
            "state": branch["state"],
            "legal_actions": branch["legal_actions"],
        }
        tokens = state_tokens(branch, decision)
        token_ids = []
        for token in tokens[: MODEL_DEFAULTS["max_tokens"]]:
            token_id = token_to_id.get(token, 1)
            unknown_tokens += int(token_id == 1)
            token_ids.append(token_id)

        legal_ids = [action_to_id[action_key(action)] for action in branch["legal_actions"]]
        candidate_ids = [action_to_id[action_key(row["action"])] for row in candidates]
        if sorted(legal_ids) != sorted(candidate_ids):
            raise ValueError(
                f"branch puzzle {puzzle_index} does not score every legal action"
            )
        candidate_utilities = torch.tensor(utilities, dtype=torch.float32)
        probabilities = torch.softmax(candidate_utilities / temperature, dim=0)
        rows.append(
            {
                "input_ids": token_ids,
                "legal_ids": legal_ids,
                "candidate_ids": candidate_ids,
                "probabilities": probabilities,
                "utilities": candidate_utilities,
                "boss": boss_to_id[branch["boss"]],
                "puzzle_index": puzzle_index,
            }
        )

    count = len(rows)
    action_count = len(action_list)
    tensors = {
        "input_ids": torch.zeros(
            (count, MODEL_DEFAULTS["max_tokens"]), dtype=torch.int32
        ),
        "legal_mask": torch.zeros((count, action_count), dtype=torch.bool),
        "target_distribution": torch.zeros(
            (count, action_count), dtype=torch.float32
        ),
        "utility": torch.full((count, action_count), -1.0, dtype=torch.float32),
        "best_utility": torch.empty(count, dtype=torch.float32),
        "boss": torch.empty(count, dtype=torch.int8),
    }
    for index, row in enumerate(rows):
        token_ids = row["input_ids"]
        tensors["input_ids"][index, : len(token_ids)] = torch.tensor(
            token_ids, dtype=torch.int32
        )
        tensors["legal_mask"][index, row["legal_ids"]] = True
        tensors["target_distribution"][index, row["candidate_ids"]] = row[
            "probabilities"
        ]
        tensors["utility"][index, row["candidate_ids"]] = row["utilities"]
        tensors["best_utility"][index] = row["utilities"].max()
        tensors["boss"][index] = row["boss"]
    return tensors, {
        "examples": count,
        "skipped_flat": skipped_flat,
        "unknown_tokens": unknown_tokens,
        "temperature": temperature,
        "minimum_span": minimum_span,
    }


def preference_loader(
    tensors: dict[str, torch.Tensor], batch_size: int, shuffle: bool, seed: int
) -> DataLoader:
    generator = torch.Generator().manual_seed(seed)
    return DataLoader(
        TensorDataset(
            tensors["input_ids"],
            tensors["legal_mask"],
            tensors["target_distribution"],
            tensors["utility"],
            tensors["best_utility"],
            tensors["boss"],
        ),
        batch_size=batch_size,
        shuffle=shuffle,
        generator=generator,
        drop_last=shuffle,
        pin_memory=torch.cuda.is_available(),
    )


def evaluate_preferences(
    model: CombatHrm,
    tensors: dict[str, torch.Tensor],
    device: torch.device,
) -> dict[str, Any]:
    loader = preference_loader(
        tensors, MODEL_DEFAULTS["batch_size"] * 2, False, 0
    )
    total = 0
    exact_best = 0
    near_best = 0
    regret = 0.0
    model.eval()
    with torch.inference_mode():
        for input_ids, legal, _targets, utility, best, _boss in loader:
            input_ids = input_ids.to(device, non_blocking=True).long()
            legal = legal.to(device, non_blocking=True)
            utility = utility.to(device, non_blocking=True)
            best = best.to(device, non_blocking=True)
            carry = None
            with torch.autocast(
                device_type=device.type,
                dtype=torch.bfloat16,
                enabled=device.type == "cuda",
            ):
                for _ in range(MODEL_DEFAULTS["deep_supervision_segments"]):
                    carry, logits, _progress = model.segment(input_ids, carry)
                prediction = masked_logits(logits, legal).argmax(dim=-1)
            chosen = utility.gather(1, prediction[:, None]).squeeze(1)
            difference = best - chosen
            total += prediction.numel()
            exact_best += int(difference.le(1e-6).sum())
            near_best += int(difference.le(0.05).sum())
            regret += float(difference.sum())
    return {
        "examples": total,
        "exact_best_accuracy": exact_best / max(1, total),
        "near_best_accuracy": near_best / max(1, total),
        "mean_utility_regret": regret / max(1, total),
    }


def next_batch(iterator, loader):
    try:
        return next(iterator), iterator
    except StopIteration:
        iterator = iter(loader)
        return next(iterator), iterator


def train(args: argparse.Namespace) -> dict[str, Any]:
    random.seed(args.seed)
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)
        torch.backends.cuda.matmul.allow_tf32 = True
        torch.set_float32_matmul_precision("high")
    device = choose_device(args.device)
    checkpoint = torch.load(args.checkpoint, map_location="cpu", weights_only=False)
    prepared = torch.load(args.prepared, map_location="cpu", weights_only=False)
    vocabulary = checkpoint["vocabulary"]
    action_list = checkpoint["action_list"]
    if prepared["vocabulary"] != vocabulary or prepared["action_list"] != action_list:
        raise ValueError("base checkpoint and prepared demonstration tensors disagree")
    split_map = {int(key): value for key, value in checkpoint["split_map"].items()}
    preferences, preference_stats = prepare_preferences(
        args.branches,
        vocabulary,
        action_list,
        split_map,
        args.temperature,
        args.minimum_span,
    )
    if preference_stats["examples"] == 0:
        raise ValueError("no informative branch examples")

    model = CombatHrm(len(vocabulary), len(action_list)).to(device)
    model.load_state_dict(checkpoint["model_state"])
    before_preferences = evaluate_preferences(model, preferences, device)
    before_validation = evaluate(model, prepared["tensors"]["val"], device)
    before_test = evaluate(model, prepared["tensors"]["test"], device)
    log(f"preference data: {json.dumps(preference_stats, sort_keys=True)}")
    log(f"before preference metrics: {json.dumps(before_preferences, sort_keys=True)}")

    if args.train_scope == "action-head":
        for name, parameter in model.named_parameters():
            parameter.requires_grad_(name.startswith("action_head."))
    trainable_parameters = [
        parameter for parameter in model.parameters() if parameter.requires_grad
    ]
    optimizer = torch.optim.AdamW(
        trainable_parameters,
        lr=args.learning_rate,
        betas=(0.9, 0.95),
        weight_decay=0.01,
    )
    pref_loader = preference_loader(
        preferences, MODEL_DEFAULTS["batch_size"], True, args.seed
    )
    demo_loader = make_loader(
        prepared["tensors"]["train"],
        MODEL_DEFAULTS["batch_size"],
        True,
        args.seed + 1,
    )
    pref_iterator = iter(pref_loader)
    demo_iterator = iter(demo_loader)
    started = time.monotonic()
    deadline = started + args.train_seconds
    next_report = started + min(30.0, args.train_seconds / 4)
    updates = 0
    loss_ema = None
    model.train()

    while time.monotonic() < deadline:
        pref_batch, pref_iterator = next_batch(pref_iterator, pref_loader)
        demo_batch, demo_iterator = next_batch(demo_iterator, demo_loader)
        pref_input, pref_legal, pref_target, _utility, _best, _boss = pref_batch
        demo_input, demo_legal, demo_target, demo_score, _boss = demo_batch
        pref_input = pref_input.to(device, non_blocking=True).long()
        pref_legal = pref_legal.to(device, non_blocking=True)
        pref_target = pref_target.to(device, non_blocking=True)
        demo_input = demo_input.to(device, non_blocking=True).long()
        demo_legal = demo_legal.to(device, non_blocking=True)
        demo_target = demo_target.to(device, non_blocking=True)
        demo_score = demo_score.to(device, non_blocking=True)
        pref_carry = None
        demo_carry = None

        for _segment in range(MODEL_DEFAULTS["deep_supervision_segments"]):
            if time.monotonic() >= deadline and updates > 0:
                break
            optimizer.zero_grad(set_to_none=True)
            with torch.autocast(
                device_type=device.type,
                dtype=torch.bfloat16,
                enabled=device.type == "cuda",
            ):
                pref_carry, pref_logits, _pref_progress = model.segment(
                    pref_input, pref_carry
                )
                demo_carry, demo_logits, demo_progress = model.segment(
                    demo_input, demo_carry
                )
                pref_log_prob = F.log_softmax(
                    masked_logits(pref_logits, pref_legal).float(), dim=-1
                )
                preference_loss = -(pref_target * pref_log_prob).sum(dim=-1).mean()
                imitation_loss = F.cross_entropy(
                    masked_logits(demo_logits, demo_legal).float(), demo_target
                )
                progress_loss = F.smooth_l1_loss(demo_progress.float(), demo_score)
                progress_weight = (
                    MODEL_DEFAULTS["progress_loss_weight"]
                    if args.train_scope == "all"
                    else 0.0
                )
                loss = (
                    args.preference_weight * preference_loss
                    + args.imitation_weight * imitation_loss
                    + progress_weight * progress_loss
                )
            loss.backward()
            nn.utils.clip_grad_norm_(
                trainable_parameters, MODEL_DEFAULTS["grad_clip"]
            )
            optimizer.step()
            updates += 1
            value = float(loss.detach())
            loss_ema = value if loss_ema is None else 0.98 * loss_ema + 0.02 * value

        now = time.monotonic()
        if now >= next_report:
            log(
                f"outcome fine-tuning {now-started:.1f}/{args.train_seconds:.1f}s; "
                f"updates={updates}; loss_ema={loss_ema:.4f}"
            )
            next_report += 30.0

    if device.type == "cuda":
        torch.cuda.synchronize()
    elapsed = time.monotonic() - started
    after_preferences = evaluate_preferences(model, preferences, device)
    validation = evaluate(model, prepared["tensors"]["val"], device)
    test = evaluate(model, prepared["tensors"]["test"], device)
    train_sample = evaluate(
        model, prepared["tensors"]["train"], device, max_examples=2048
    )
    metrics = {
        "method": "simulator_branch_soft_preferences",
        "base_checkpoint": str(args.checkpoint.resolve()),
        "branches": str(args.branches.resolve()),
        "device": str(device),
        "seed": args.seed,
        "parameter_count": sum(parameter.numel() for parameter in model.parameters()),
        "model_defaults": MODEL_DEFAULTS,
        "requested_training_seconds": args.train_seconds,
        "actual_training_seconds": elapsed,
        "optimizer_updates": updates,
        "learning_rate": args.learning_rate,
        "train_scope": args.train_scope,
        "trainable_parameter_count": sum(
            parameter.numel() for parameter in trainable_parameters
        ),
        "preference_weight": args.preference_weight,
        "imitation_weight": args.imitation_weight,
        "final_loss_ema": loss_ema,
        "preference_data": preference_stats,
        "before_preferences": before_preferences,
        "after_preferences": after_preferences,
        "before_validation": before_validation,
        "before_test": before_test,
        "train_sample": train_sample,
        "validation": validation,
        "test": test,
    }
    output = args.output
    output.parent.mkdir(parents=True, exist_ok=True)
    torch.save(
        {
            "model_state": model.state_dict(),
            "vocabulary": vocabulary,
            "action_list": action_list,
            "model_defaults": MODEL_DEFAULTS,
            "split_map": split_map,
            "metrics": metrics,
        },
        output,
    )
    metrics_path = output.with_name(output.stem + "-metrics.json")
    metrics_path.write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n")
    if not args.skip_export:
        export_checkpoint(
            output,
            output.with_suffix(".onnx"),
            output.with_suffix(".runtime.json"),
            "float16",
        )
    log(f"checkpoint={output}")
    log(f"metrics={metrics_path}")
    log("RESULT " + json.dumps(metrics, sort_keys=True))
    return metrics


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--branches", type=Path, required=True)
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--train-seconds", type=float, default=120.0)
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--learning-rate", type=float, default=3e-5)
    parser.add_argument("--temperature", type=float, default=0.10)
    parser.add_argument("--minimum-span", type=float, default=0.02)
    parser.add_argument("--preference-weight", type=float, default=1.0)
    parser.add_argument("--imitation-weight", type=float, default=1.0)
    parser.add_argument(
        "--train-scope", choices=("action-head", "all"), default="action-head"
    )
    parser.add_argument("--skip-export", action="store_true")
    parser.add_argument("--seed", type=int, default=20260827)
    args = parser.parse_args()
    if args.train_seconds <= 0 or args.learning_rate <= 0 or args.temperature <= 0:
        parser.error("training seconds, learning rate, and temperature must be positive")
    return args


if __name__ == "__main__":
    train(parse_args())
