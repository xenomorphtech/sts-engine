#!/usr/bin/env python3
"""Fit a teacher-free listwise branch-ranking head on exact self-play search."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path
import time
from typing import Any

import torch
from torch import nn
from torch.nn import functional as F
from torch.utils.data import DataLoader, TensorDataset

from train_selfplay_hrm import (
    ACTION_PARAMETER_SPECS,
    MAX_ACTION_FEATURES,
    MAX_CANDIDATE_IDENTITIES,
    MAX_HISTORY_STEPS,
    MAX_INVENTORY_IDENTITIES,
    MAX_STATE_FEATURES,
    SelfPlayHrm,
    action_parameter_vector,
    candidate_identity_features,
    decision_signature,
    iter_episodes,
    iter_branch_rows,
    measurement_vector,
    split_for_seed,
    symlog_scaled,
)


def group_key(row: dict[str, Any]) -> tuple[Any, ...]:
    observation = row["observation"]
    menu = tuple(
        (
            tuple(action["features"]),
            tuple(action.get("candidate_identities", [])),
            tuple(sorted((action.get("parameters") or {}).items())),
        )
        for action in observation["actions"]
    )
    return (
        int(row["seed"]),
        int(row["step"]),
        tuple(observation["state_features"]),
        tuple(observation.get("inventory_identities", [])),
        tuple(row.get("history", [])),
        menu,
    )


def aggregate_action_rollouts(
    rows: list[dict[str, Any]], score_optimism: float
) -> list[dict[str, Any]]:
    by_action: dict[int, list[dict[str, Any]]] = {}
    for row in rows:
        by_action.setdefault(int(row["action_index"]), []).append(row)
    aggregated: list[dict[str, Any]] = []
    for action_rows in by_action.values():
        scores = [float(row["branch_score"]) for row in action_rows]
        mean = sum(scores) / len(scores)
        variance = sum((score - mean) ** 2 for score in scores) / len(scores)
        row = dict(action_rows[0])
        row["branch_score"] = mean + score_optimism * math.sqrt(variance)
        aggregated.append(row)
    return aggregated


def iter_winning_imitation_rows(paths: list[Path]):
    """Turn self-discovered wins into full-menu preference supervision."""
    for episode in iter_episodes(paths):
        if episode["result"]["outcome"] != "act3_boss_victory":
            continue
        history: list[int] = []
        seed = int(episode["result"]["seed"])
        for step, transition in enumerate(episode["transitions"]):
            observation = transition["observation"]
            chosen = int(transition["action_index"])
            for action in observation["actions"]:
                yield {
                    "seed": seed,
                    "step": step,
                    "observation": observation,
                    "before": transition["before"],
                    "history": list(history),
                    "action_index": int(action["index"]),
                    "branch_score": (
                        1_000.0 if int(action["index"]) == chosen else -1_000.0
                    ),
                }
            history.append(decision_signature(observation, chosen))


def iter_floor_return_rows(paths: list[Path]):
    """Emit observed actions labeled only by their episode's final floor."""
    for episode in iter_episodes(paths):
        history: list[int] = []
        seed = int(episode["result"]["seed"])
        final_floor = float(episode["result"]["max_floor"])
        for step, transition in enumerate(episode["transitions"]):
            observation = transition["observation"]
            chosen = int(transition["action_index"])
            yield {
                "seed": seed,
                "step": step,
                "observation": observation,
                "before": transition["before"],
                "history": list(history),
                "action_index": chosen,
                "branch_score": final_floor,
            }
            history.append(decision_signature(observation, chosen))


def iter_seed_elite_imitation_rows(paths: list[Path], elites_per_seed: int):
    """Imitate each seed's best self-play rollout, selected only by final floor."""
    elites: dict[int, list[tuple[float, int, dict[str, Any]]]] = {}
    serial = 0
    for episode in iter_episodes(paths):
        seed = int(episode["result"]["seed"])
        final_floor = float(episode["result"]["max_floor"])
        candidates = elites.setdefault(seed, [])
        candidates.append((final_floor, -serial, episode))
        candidates.sort(key=lambda item: (item[0], item[1]), reverse=True)
        del candidates[elites_per_seed:]
        serial += 1

    for seed, candidates in elites.items():
        for elite_index, (_, _, episode) in enumerate(candidates):
            history: list[int] = []
            for step, transition in enumerate(episode["transitions"]):
                observation = transition["observation"]
                chosen = int(transition["action_index"])
                for action in observation["actions"]:
                    yield {
                        "seed": seed,
                        "step": step,
                        "observation": observation,
                        "before": transition["before"],
                        "history": list(history),
                        "action_index": int(action["index"]),
                        "branch_score": (
                            1_000.0 if int(action["index"]) == chosen else -1_000.0
                        ),
                        "elite_index": elite_index,
                    }
                history.append(decision_signature(observation, chosen))


def prepare_groups(
    paths: list[Path],
    numeric_size: int,
    action_numeric_size: int,
    score_optimism: float = 0.0,
    winning_paths: list[Path] | None = None,
    floor_return_paths: list[Path] | None = None,
    seed_elite_paths: list[Path] | None = None,
    elites_per_seed: int = 1,
    parameterized_menus_only: bool = False,
) -> dict[str, TensorDataset]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = {}
    for row in iter_branch_rows(paths):
        grouped.setdefault(group_key(row), []).append(row)
    for row in iter_winning_imitation_rows(winning_paths or []):
        grouped.setdefault(("imitation", *group_key(row)), []).append(row)
    for row in iter_floor_return_rows(floor_return_paths or []):
        grouped.setdefault(("floor_return", *group_key(row)), []).append(row)
    for row in iter_seed_elite_imitation_rows(
        seed_elite_paths or [], elites_per_seed
    ):
        grouped.setdefault(
            ("seed_elite", int(row["elite_index"]), *group_key(row)), []
        ).append(row)
    groups = [
        aggregated
        for rows in grouped.values()
        if len(aggregated := aggregate_action_rollouts(rows, score_optimism)) >= 2
        and max(row["branch_score"] for row in aggregated)
        > min(row["branch_score"] for row in aggregated)
        and (
            not parameterized_menus_only
            or any(
                bool((action.get("parameters") or {}).get("known", False))
                for action in aggregated[0]["observation"]["actions"]
            )
        )
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
        inventory = torch.zeros(
            (count, max_candidates, MAX_INVENTORY_IDENTITIES), dtype=torch.int32
        )
        candidate_identity = torch.zeros(
            (count, max_candidates, MAX_CANDIDATE_IDENTITIES), dtype=torch.int32
        )
        numeric = torch.zeros(
            (count, max_candidates, numeric_size), dtype=torch.float32
        )
        action_numeric = torch.zeros(
            (count, max_candidates, action_numeric_size), dtype=torch.float32
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
                inventory_ids = observation.get("inventory_identities", [])
                candidate_ids = candidate_identity_features(
                    observation,
                    selected,
                    row["before"]["enemy_max_hp"] > 0,
                )
                inventory_ids = inventory_ids[:MAX_INVENTORY_IDENTITIES]
                candidate_ids = candidate_ids[:MAX_CANDIDATE_IDENTITIES]
                state[group_index, candidate, : len(state_ids)] = torch.tensor(state_ids)
                action[group_index, candidate, : len(action_ids)] = torch.tensor(action_ids)
                inventory[
                    group_index, candidate, : len(inventory_ids)
                ] = torch.tensor(inventory_ids)
                candidate_identity[
                    group_index, candidate, : len(candidate_ids)
                ] = torch.tensor(candidate_ids)
                numeric[group_index, candidate] = torch.tensor(
                    measurement_vector(row["before"], numeric_size)
                )
                if action_numeric_size:
                    action_numeric[group_index, candidate] = torch.tensor(
                        action_parameter_vector(
                            selected, row["before"], action_numeric_size
                        )
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
        datasets[split] = TensorDataset(
            state,
            action,
            inventory,
            candidate_identity,
            numeric,
            action_numeric,
            history,
            target,
            mask,
        )
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
    (
        state,
        action,
        inventory,
        candidate,
        numeric,
        action_numeric,
        history,
        target,
        mask,
    ) = batch
    batch_size, candidates, _ = state.shape
    state = state.flatten(0, 1).to(device=device, dtype=torch.long, non_blocking=True)
    action = action.flatten(0, 1).to(device=device, dtype=torch.long, non_blocking=True)
    inventory = inventory.flatten(0, 1).to(
        device=device, dtype=torch.long, non_blocking=True
    )
    candidate = candidate.flatten(0, 1).to(
        device=device, dtype=torch.long, non_blocking=True
    )
    numeric = numeric.flatten(0, 1).to(device=device, non_blocking=True)
    action_numeric = action_numeric.flatten(0, 1).to(
        device=device, non_blocking=True
    )
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
        prediction = model(
            state,
            action,
            numeric,
            history,
            inventory,
            candidate,
            action_numeric,
        )[:, search_index]
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
    actor_target_names = tuple(checkpoint["target_names"])
    if "search_value" not in actor_target_names:
        raise ValueError("checkpoint has no search-value head")
    target_names = actor_target_names
    config = dict(checkpoint["config"])
    if args.add_choice_critic:
        if config.get("architecture") != "hrm_state_ssm":
            raise ValueError(
                "add-choice-critic currently requires an hrm_state_ssm actor"
            )
        config["architecture"] = "hrm_choice_critic_ssm"
        config["actor_target_names"] = actor_target_names
        target_names = (*actor_target_names, "choice_value")
    if args.add_action_parameters:
        if config.get("architecture") != "hrm_choice_critic_ssm":
            raise ValueError(
                "action parameters currently require the isolated choice critic"
            )
        already_parameterized = int(
            config.get("action_numeric_measurements", 0)
        ) > 0
        config["action_numeric_measurements"] = len(ACTION_PARAMETER_SPECS)
        config["action_numeric_mode"] = (
            "additive_gated_residual"
            if already_parameterized
            else "gated_residual"
        )
    config["target_names"] = target_names
    model = SelfPlayHrm(config)
    actor_keys: list[str] = []
    if args.add_choice_critic or args.add_action_parameters:
        composed_state = model.state_dict()
        for name, value in checkpoint["model"].items():
            if name in composed_state and composed_state[name].shape == value.shape:
                composed_state[name] = value
                actor_keys.append(name)
        model.load_state_dict(composed_state)
    else:
        model.load_state_dict(checkpoint["model"])
    if args.actor_checkpoint is not None:
        actor_checkpoint = torch.load(
            args.actor_checkpoint, map_location="cpu", weights_only=False
        )
        if actor_checkpoint.get("teacher") is not None:
            raise ValueError("actor checkpoint is not teacher-free")
        if tuple(actor_checkpoint["target_names"]) != actor_target_names:
            raise ValueError("actor and critic target heads do not match")
        actor_state = actor_checkpoint["model"]
        composed_state = model.state_dict()
        for name, value in actor_state.items():
            if (
                not name.startswith("choice_critic.")
                and name in composed_state
                and composed_state[name].shape == value.shape
            ):
                composed_state[name] = value
                actor_keys.append(name)
        if not actor_keys:
            raise ValueError("actor checkpoint has no compatible policy parameters")
        model.load_state_dict(composed_state)
    for parameter in model.parameters():
        parameter.requires_grad_(False)
    search_module = (
        model.choice_critic.menu_residual
        if args.add_action_parameters and model.choice_critic is not None
        else model.choice_critic
        if model.choice_critic is not None
        else model.output[-1]
    )
    if search_module is None:
        raise ValueError("requested menu residual was not constructed")
    for parameter in search_module.parameters():
        parameter.requires_grad_(True)
    model.to(device)

    datasets = prepare_groups(
        args.branch_dataset,
        model.numeric_size,
        model.action_numeric_size,
        args.score_optimism,
        args.winning_trace,
        args.floor_return_trace,
        args.seed_elite_trace,
        args.elites_per_seed,
        args.add_action_parameters,
    )
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
        search_module.parameters(), lr=args.learning_rate, weight_decay=0.0
    )
    search_index = target_names.index(
        "choice_value" if model.choice_critic is not None else "search_value"
    )
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
            if args.pointwise_weight:
                loss = loss + args.pointwise_weight * F.smooth_l1_loss(
                    prediction[mask], target[mask]
                )
            if not torch.isfinite(loss):
                raise RuntimeError("non-finite listwise loss")
            loss.backward()
            nn.utils.clip_grad_norm_(search_module.parameters(), 1.0)
            optimizer.step()
            updates += 1
    metrics = evaluate(model, validation_loader, device, search_index)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    result = dict(checkpoint)
    result["model"] = model.state_dict()
    result["config"] = config
    result["target_names"] = target_names
    result["initialized_from"] = str(args.checkpoint)
    result["actor_initialized_from"] = (
        str(args.actor_checkpoint)
        if args.actor_checkpoint is not None
        else str(args.checkpoint)
        if args.add_choice_critic
        else None
    )
    result["actor_parameter_tensors"] = len(actor_keys)
    result["teacher"] = None
    result["search_value_supported"] = True
    result["branch_rank"] = {
        "method": "listwise_softmax",
        "updates": updates,
        "seconds": time.monotonic() - started,
        "target_temperature": args.target_temperature,
        "pointwise_weight": args.pointwise_weight,
        "score_optimism": args.score_optimism,
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
    parser.add_argument(
        "--actor-checkpoint",
        type=Path,
        help=(
            "copy compatible non-critic weights from this teacher-free actor "
            "before fitting the isolated choice critic"
        ),
    )
    parser.add_argument(
        "--add-choice-critic",
        action="store_true",
        help=(
            "wrap an hrm_state_ssm checkpoint in a fresh isolated relational "
            "critic while preserving every compatible actor tensor"
        ),
    )
    parser.add_argument(
        "--add-action-parameters",
        action="store_true",
        help=(
            "add a fresh numeric candidate-cost projection to the isolated "
            "choice critic while preserving compatible checkpoint tensors"
        ),
    )
    parser.add_argument(
        "--branch-dataset", type=Path, action="append", default=[]
    )
    parser.add_argument(
        "--winning-trace",
        type=Path,
        action="append",
        default=[],
        help="self-discovered winning trajectory JSONL/XZ; repeat to mix wins",
    )
    parser.add_argument(
        "--floor-return-trace",
        type=Path,
        action="append",
        default=[],
        help=(
            "repeated-seed teacher-free trajectories; identical visible states "
            "are grouped and actions are ranked by observed final floor"
        ),
    )
    parser.add_argument(
        "--seed-elite-trace",
        type=Path,
        action="append",
        default=[],
        help=(
            "repeated-seed teacher-free trajectories; imitate the rollout with "
            "the highest observed final floor for each seed"
        ),
    )
    parser.add_argument("--elites-per-seed", type=int, default=1)
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-generation7-branch-rank.pt"),
    )
    parser.add_argument("--seconds", type=float, default=30.0)
    parser.add_argument("--batch-size", type=int, default=64)
    parser.add_argument("--learning-rate", type=float, default=1e-3)
    parser.add_argument("--target-temperature", type=float, default=0.20)
    parser.add_argument("--pointwise-weight", type=float, default=0.0)
    parser.add_argument("--score-optimism", type=float, default=0.0)
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--seed", type=int, default=20260827)
    args = parser.parse_args()
    if (
        args.seconds <= 0
        or args.batch_size <= 0
        or args.target_temperature <= 0
        or args.pointwise_weight < 0
        or args.score_optimism < 0
        or args.elites_per_seed <= 0
    ):
        parser.error(
            "seconds, batch size, and target temperature must be positive; "
            "pointwise weight cannot be negative"
        )
    if args.add_choice_critic and args.actor_checkpoint is not None:
        parser.error("add-choice-critic and actor-checkpoint are mutually exclusive")
    if not (
        args.branch_dataset
        or args.winning_trace
        or args.floor_return_trace
        or args.seed_elite_trace
    ):
        parser.error(
            "at least one branch-dataset, winning-trace, floor-return-trace, "
            "or seed-elite-trace is required"
        )
    if args.device == "auto":
        args.device = "cuda" if torch.cuda.is_available() else "cpu"
    return args


if __name__ == "__main__":
    train(parse_args())
