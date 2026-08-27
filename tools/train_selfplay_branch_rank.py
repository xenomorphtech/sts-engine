#!/usr/bin/env python3
"""Fit a teacher-free listwise branch-ranking head on exact self-play search."""

from __future__ import annotations

import argparse
from collections import Counter
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
    rows: list[dict[str, Any]],
    score_optimism: float,
    return_aggregation: str = "mean",
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
        row["branch_score"] = (
            max(scores)
            if return_aggregation == "max"
            else mean + score_optimism * math.sqrt(variance)
        )
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
    """Emit sampled actions labeled by population progress and entry health."""
    for episode in iter_episodes(paths):
        history: list[int] = []
        seed = int(episode["result"]["seed"])
        progress_return = episode_progress_return(episode)
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
                "branch_score": progress_return,
            }
            history.append(decision_signature(observation, chosen))


def centered_policy_advantages(returns: list[float]) -> list[float]:
    """Standardize complete-run returns within one random-seed cohort."""
    if not returns:
        return []
    mean = sum(returns) / len(returns)
    variance = sum((value - mean) ** 2 for value in returns) / len(returns)
    if variance <= 1e-12:
        return [0.0] * len(returns)
    scale = math.sqrt(variance)
    return [(value - mean) / scale for value in returns]


def iter_seed_policy_gradient_rows(
    paths: list[Path], noncombat_only: bool = False, combat_only: bool = False
):
    """Turn random rollouts into seed-centered positive and negative menus.

    Every action in the sampled menu is emitted.  The selected action receives
    the standardized complete-run floor/entry-HP advantage and alternatives
    receive zero.  Aggregating identical visible menus therefore estimates the
    covariance between choosing an action and population progress, while poor
    random trajectories remain useful negative evidence.
    """
    returns_by_seed: dict[int, list[float]] = {}
    for episode in iter_episodes(paths):
        seed = int(episode["result"]["seed"])
        returns_by_seed.setdefault(seed, []).append(
            episode_progress_return(episode)
        )
    advantages_by_seed = {
        seed: iter(centered_policy_advantages(returns))
        for seed, returns in returns_by_seed.items()
    }
    for episode in iter_episodes(paths):
        seed = int(episode["result"]["seed"])
        advantage = next(advantages_by_seed[seed])
        if advantage == 0.0:
            continue
        history: list[int] = []
        for step, transition in enumerate(episode["transitions"]):
            observation = transition["observation"]
            chosen = int(transition["action_index"])
            if noncombat_only and int(transition["before"]["enemy_max_hp"]) > 0:
                history.append(decision_signature(observation, chosen))
                continue
            if combat_only and int(transition["before"]["enemy_max_hp"]) <= 0:
                history.append(decision_signature(observation, chosen))
                continue
            if len(observation["actions"]) < 2:
                history.append(decision_signature(observation, chosen))
                continue
            for action in observation["actions"]:
                action_index = int(action["index"])
                yield {
                    "seed": seed,
                    "step": step,
                    "observation": observation,
                    "before": transition["before"],
                    "history": list(history),
                    "action_index": action_index,
                    "branch_score": (
                        advantage * 1_000.0 if action_index == chosen else 0.0
                    ),
                }
            history.append(decision_signature(observation, chosen))


def episode_progress_key(episode: dict[str, Any]) -> tuple[int, int, int, float]:
    """Rank sampled runs by broad progress, then health carried forward.

    Floor is deliberately the primary component: this is a mean-floor
    objective, not a boss/frontier threshold.  Among runs that reach the same
    floor, prefer the one that entered it with more current HP, then more max
    HP.  The terminal score is only a final tie-break (normally remaining
    player HP after a win or negated remaining monster HP after a loss).
    """
    result = episode["result"]
    max_floor = int(result["max_floor"])
    entry: dict[str, Any] | None = None
    for transition in episode["transitions"]:
        for state in (transition["before"], transition["after"]):
            if int(state["floor"]) == max_floor and int(state["hp"]) > 0:
                entry = state
                break
        if entry is not None:
            break
    if entry is None:
        entry = result["terminal"]
    return (
        max_floor,
        int(entry["hp"]),
        int(entry["max_hp"]),
        float(result["terminal_score"]),
    )


def episode_progress_return(episode: dict[str, Any]) -> float:
    """Scalar Monte-Carlo return aligned with mean floor and entry HP.

    A complete floor remains more valuable than any possible Defect HP
    difference: 200 current HP is worth one floor.  The smaller terms make
    health, max health, and terminal
    combat margin useful when sampled continuations reach the same floor.
    Averaging this value across repeated state/action samples estimates the
    population objective directly rather than applying a frontier threshold.
    """
    floor, hp, max_hp, terminal_score = episode_progress_key(episode)
    return (
        float(floor)
        + float(hp) / 200.0
        + float(max_hp) / 200_000.0
        + terminal_score / 1_000_000_000.0
    )


def progress_prefix(episode: dict[str, Any]) -> list[dict[str, Any]]:
    """Keep only decisions that caused the selected floor/HP achievement.

    Once the run has entered its final reached floor, later decisions belong to
    the failed attempt to leave that floor.  Imitating them would turn the
    terminal loss into a positive target even though it did not contribute to
    the selected progress-and-health result.
    """
    max_floor = int(episode["result"]["max_floor"])
    prefix: list[dict[str, Any]] = []
    for transition in episode["transitions"]:
        prefix.append(transition)
        after = transition["after"]
        if int(after["floor"]) == max_floor and int(after["hp"]) > 0:
            break
    return prefix


def iter_seed_elite_imitation_rows(paths: list[Path], elites_per_seed: int):
    """Imitate each seed's best progress-and-health self-play rollout."""
    elites: dict[
        int, list[tuple[tuple[int, int, int, float], int, dict[str, Any]]]
    ] = {}
    serial = 0
    for episode in iter_episodes(paths):
        seed = int(episode["result"]["seed"])
        candidates = elites.setdefault(seed, [])
        candidates.append((episode_progress_key(episode), -serial, episode))
        candidates.sort(key=lambda item: (item[0], item[1]), reverse=True)
        del candidates[elites_per_seed:]
        serial += 1

    for seed, candidates in elites.items():
        for elite_index, (_, _, episode) in enumerate(candidates):
            history: list[int] = []
            for step, transition in enumerate(progress_prefix(episode)):
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
    seed_policy_gradient_paths: list[Path] | None = None,
    seed_elite_paths: list[Path] | None = None,
    elites_per_seed: int = 1,
    parameterized_menus_only: bool = False,
    min_action_samples: int = 1,
    noncombat_only: bool = False,
    combat_only: bool = False,
    include_pointwise_groups: bool = False,
    return_aggregation: str = "mean",
) -> dict[str, TensorDataset]:
    grouped: dict[tuple[Any, ...], list[dict[str, Any]]] = {}
    for row in iter_branch_rows(paths):
        grouped.setdefault(group_key(row), []).append(row)
    for row in iter_winning_imitation_rows(winning_paths or []):
        grouped.setdefault(("imitation", *group_key(row)), []).append(row)
    for row in iter_floor_return_rows(floor_return_paths or []):
        grouped.setdefault(("floor_return", *group_key(row)), []).append(row)
    for row in iter_seed_policy_gradient_rows(
        seed_policy_gradient_paths or [], noncombat_only, combat_only
    ):
        grouped.setdefault(("seed_policy_gradient", *group_key(row)), []).append(row)
    for row in iter_seed_elite_imitation_rows(
        seed_elite_paths or [], elites_per_seed
    ):
        grouped.setdefault(
            ("seed_elite", int(row["elite_index"]), *group_key(row)), []
        ).append(row)
    groups = [
        aggregated
        for rows in grouped.values()
        if min(
            Counter(int(row["action_index"]) for row in rows).values(),
            default=0,
        )
        >= min_action_samples
        if not noncombat_only or int(rows[0]["before"]["enemy_max_hp"]) <= 0
        if not combat_only or int(rows[0]["before"]["enemy_max_hp"]) > 0
        if len(
            aggregated := aggregate_action_rollouts(
                rows, score_optimism, return_aggregation
            )
        )
        >= (1 if include_pointwise_groups else 2)
        and (
            include_pointwise_groups
            or max(row["branch_score"] for row in aggregated)
            > min(row["branch_score"] for row in aggregated)
        )
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


def policy_gradient_loss(
    prediction: torch.Tensor,
    advantage: torch.Tensor,
    mask: torch.Tensor,
) -> torch.Tensor:
    """Signed seed-centered REINFORCE loss over complete legal-action menus."""
    floor = torch.finfo(prediction.dtype).min
    log_policy = F.log_softmax(prediction.masked_fill(~mask, floor), dim=-1)
    log_policy = log_policy.masked_fill(~mask, 0.0)
    return -(advantage.masked_fill(~mask, 0.0) * log_policy).sum(-1).mean()


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
    if args.add_combat_menu_residual:
        if config.get("architecture") != "hrm_choice_critic_ssm":
            raise ValueError(
                "combat menu residual requires the isolated choice critic"
            )
        if int(config.get("action_numeric_measurements", 0)) <= 0:
            raise ValueError(
                "combat menu residual requires an action-parameter checkpoint"
            )
        config["combat_menu_residual"] = True
    if args.add_population_adapter:
        if config.get("architecture") != "hrm_choice_critic_ssm":
            raise ValueError(
                "population adapter requires the isolated choice critic"
            )
        if "choice_value" not in target_names:
            raise ValueError("population adapter requires a choice-value head")
        config["population_value_adapter"] = True
        config["population_adapter_combat_identities"] = True
        config["population_adapter_noncombat_only"] = args.noncombat_only
        config["population_adapter_combat_only"] = args.combat_only
        config["population_relational_inventory"] = True
        config["population_action_attention"] = True
        config["counterfactual_adapter_scale"] = (
            args.incumbent_counterfactual_adapter_scale
        )
        config["menu_residual_scale"] = args.incumbent_menu_residual_scale
    if args.counterfactual_adapter_only and not config.get(
        "counterfactual_value_adapter", False
    ):
        raise ValueError(
            "counterfactual-adapter-only requires an adapter checkpoint"
        )
    config["target_names"] = target_names
    model = SelfPlayHrm(config)
    actor_keys: list[str] = []
    if (
        args.add_choice_critic
        or args.add_action_parameters
        or args.add_combat_menu_residual
        or args.add_population_adapter
    ):
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
    if args.add_population_adapter:
        search_module = nn.ModuleList(
            [
                module
                for module in (
                    model.population_value_adapter,
                    model.population_inventory_memory,
                    model.population_state_attention,
                )
                if module is not None
            ]
        )
    else:
        search_module = (
            model.counterfactual_value_adapter
            if args.counterfactual_adapter_only
            else model.choice_critic.combat_menu_residual
            if args.add_combat_menu_residual and model.choice_critic is not None
            else model.choice_critic.menu_residual
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
        args.seed_policy_gradient_trace,
        args.seed_elite_trace,
        args.elites_per_seed,
        args.add_action_parameters and not args.add_combat_menu_residual,
        args.min_action_samples,
        args.noncombat_only,
        args.combat_only,
        args.include_pointwise_groups,
        args.return_aggregation,
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
            loss = (
                policy_gradient_loss(prediction, target, mask)
                if args.policy_gradient_loss
                else listwise_loss(
                    prediction, target, mask, args.target_temperature
                )
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
        "method": (
            "seed_centered_reinforce"
            if args.policy_gradient_loss
            else "listwise_softmax"
        ),
        "trained_module": (
            "population_value_adapter"
            if args.add_population_adapter
            else
            "counterfactual_value_adapter"
            if args.counterfactual_adapter_only
            else "choice_critic"
        ),
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
        "--add-combat-menu-residual",
        action="store_true",
        help=(
            "add and train an isolated enemy-HP-gated combat ranking residual "
            "while preserving the actor and existing menu critic"
        ),
    )
    parser.add_argument(
        "--counterfactual-adapter-only",
        action="store_true",
        help=(
            "freeze the policy and critic and train only an existing isolated "
            "counterfactual-value adapter"
        ),
    )
    parser.add_argument(
        "--add-population-adapter",
        action="store_true",
        help=(
            "add and train a zero-initialized population-return adapter while "
            "preserving the incumbent policy and counterfactual adapter"
        ),
    )
    parser.add_argument(
        "--incumbent-counterfactual-adapter-scale",
        type=float,
        default=1.0,
        help="counterfactual adapter scale used by the frozen incumbent",
    )
    parser.add_argument(
        "--incumbent-menu-residual-scale",
        type=float,
        default=1.0,
        help="menu residual scale used by the frozen incumbent",
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
            "are grouped and sampled actions are ranked by mean final floor, "
            "then HP carried into that floor"
        ),
    )
    parser.add_argument(
        "--seed-policy-gradient-trace",
        type=Path,
        action="append",
        default=[],
        help=(
            "repeated-seed random trajectories; train selected actions from "
            "their mean-floor/entry-HP advantage over other copies of the same seed"
        ),
    )
    parser.add_argument(
        "--seed-elite-trace",
        type=Path,
        action="append",
        default=[],
        help=(
            "repeated-seed teacher-free trajectories; imitate the rollout with "
            "the highest observed final floor, then highest entry HP, for each seed"
        ),
    )
    parser.add_argument("--elites-per-seed", type=int, default=1)
    parser.add_argument(
        "--min-action-samples",
        type=int,
        default=1,
        help=(
            "require this many sampled returns for every action in a grouped "
            "common-random-number menu"
        ),
    )
    parser.add_argument(
        "--noncombat-only",
        action="store_true",
        help="train only path, reward, shop, rest, card, and relic decisions",
    )
    parser.add_argument(
        "--combat-only",
        action="store_true",
        help="train and apply the population adapter only during combat",
    )
    parser.add_argument(
        "--include-pointwise-groups",
        action="store_true",
        help=(
            "retain one-action sampled states for Monte-Carlo return regression; "
            "requires a positive pointwise weight"
        ),
    )
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
    parser.add_argument(
        "--return-aggregation",
        choices=("mean", "max"),
        default="mean",
        help="back up either mean sampled return or the best sampled continuation",
    )
    parser.add_argument(
        "--policy-gradient-loss",
        action="store_true",
        help=(
            "optimize signed seed-centered advantages directly instead of "
            "converting them to listwise target probabilities"
        ),
    )
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
        or args.min_action_samples <= 0
        or args.incumbent_counterfactual_adapter_scale < 0
        or args.incumbent_menu_residual_scale < 0
    ):
        parser.error(
            "seconds, batch size, and target temperature must be positive; "
            "pointwise weight cannot be negative"
        )
    if (
        args.add_choice_critic or args.counterfactual_adapter_only
    ) and args.actor_checkpoint is not None:
        parser.error(
            "add-choice-critic/counterfactual-adapter-only and actor-checkpoint "
            "are mutually exclusive"
        )
    if args.include_pointwise_groups and args.pointwise_weight <= 0:
        parser.error("include-pointwise-groups requires a positive pointwise weight")
    if args.noncombat_only and args.combat_only:
        parser.error("noncombat-only and combat-only are mutually exclusive")
    if args.policy_gradient_loss and not args.seed_policy_gradient_trace:
        parser.error("policy-gradient-loss requires seed-policy-gradient-trace")
    if args.policy_gradient_loss and (
        args.branch_dataset
        or args.winning_trace
        or args.floor_return_trace
        or args.seed_elite_trace
    ):
        parser.error("policy-gradient-loss accepts only seed-policy-gradient traces")
    if sum(
        (
            args.add_choice_critic,
            args.add_action_parameters,
            args.add_combat_menu_residual,
            args.counterfactual_adapter_only,
            args.add_population_adapter,
        )
    ) > 1:
        parser.error(
            "add-choice-critic, add-action-parameters, add-combat-menu-residual, "
            "counterfactual-adapter-only, and add-population-adapter are "
            "mutually exclusive"
        )
    if not (
        args.branch_dataset
        or args.winning_trace
        or args.floor_return_trace
        or args.seed_policy_gradient_trace
        or args.seed_elite_trace
    ):
        parser.error(
            "at least one branch-dataset, winning-trace, floor-return-trace, "
            "seed-policy-gradient-trace, or seed-elite-trace is required"
        )
    if args.device == "auto":
        args.device = "cuda" if torch.cuda.is_available() else "cpu"
    return args


if __name__ == "__main__":
    train(parse_args())
