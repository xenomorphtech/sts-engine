"""Train the compact mean-floor/retained-HP policy from scratch.

With no data arguments the command consumes all local Defect A20 trajectories
for Monte-Carlo progress calibration and the retained generation-4 planner
trajectories for full-menu policy distillation. There is intentionally no
checkpoint-initialization mode.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import time
from collections import defaultdict
from pathlib import Path
from typing import Any

import numpy as np
import torch
from mean_progress_model import LEGACY_TARGET_NAMES, TARGET_NAMES, MeanProgressModel
from torch import nn
from torch.nn import functional as F
from torch.utils.data import DataLoader, TensorDataset
from training_schema import (
    ACTION_PARAMETER_SPECS,
    MAX_ACTION_FEATURES,
    MAX_CANDIDATE_IDENTITIES,
    MAX_HISTORY_STEPS,
    MAX_INVENTORY_IDENTITIES,
    MAX_STATE_FEATURES,
    MEASUREMENT_SPECS,
    action_parameter_vector,
    candidate_identity_features,
    decision_signature,
    iter_episodes,
    measurement_vector,
)

ID_WIDTHS = (
    MAX_STATE_FEATURES,
    MAX_ACTION_FEATURES,
    MAX_INVENTORY_IDENTITIES,
    MAX_CANDIDATE_IDENTITIES,
    MAX_HISTORY_STEPS,
)

MAX_IMITATION_ACTIONS = 32


def file_signature(paths: list[Path]) -> list[dict[str, Any]]:
    return [
        {
            "path": str(path.resolve()),
            "size": path.stat().st_size,
            "mtime_ns": path.stat().st_mtime_ns,
        }
        for path in paths
    ]


def final_entry(episode: dict[str, Any]) -> tuple[int, int, int]:
    result = episode["result"]
    floor = int(result["max_floor"])
    for transition in episode["transitions"]:
        for state in (transition["before"], transition["after"]):
            if int(state["floor"]) == floor and int(state["hp"]) > 0:
                return floor, int(state["hp"]), int(state["max_hp"])
    terminal = result["terminal"]
    return floor, max(int(terminal["hp"]), 0), int(terminal["max_hp"])


def progress_target(episode: dict[str, Any]) -> np.ndarray:
    floor, hp, max_hp = final_entry(episode)
    hp_fraction = hp / max(max_hp, 1)
    progress = (floor + hp / 200.0) / 50.0
    return np.asarray((progress, floor / 50.0, hp_fraction), dtype=np.float32)


def selected_action(observation: dict[str, Any], action_index: int) -> dict[str, Any]:
    for action in observation["actions"]:
        if int(action["index"]) == action_index:
            return action
    raise ValueError(f"action {action_index} is absent from its observation")


def encode_candidate(
    observation: dict[str, Any],
    measurements: dict[str, Any],
    action_index: int,
    history: list[int],
) -> tuple[np.ndarray, ...]:
    action = selected_action(observation, action_index)
    state = np.zeros(MAX_STATE_FEATURES, dtype=np.uint16)
    action_ids = np.zeros(MAX_ACTION_FEATURES, dtype=np.uint16)
    inventory = np.zeros(MAX_INVENTORY_IDENTITIES, dtype=np.uint16)
    candidate = np.zeros(MAX_CANDIDATE_IDENTITIES, dtype=np.uint16)
    history_ids = np.zeros(MAX_HISTORY_STEPS, dtype=np.uint16)

    state_values = observation["state_features"][:MAX_STATE_FEATURES]
    action_values = action["features"][:MAX_ACTION_FEATURES]
    inventory_values = observation.get("inventory_identities", [])[
        :MAX_INVENTORY_IDENTITIES
    ]
    candidate_values = candidate_identity_features(
        observation,
        action,
        int(measurements["enemy_max_hp"]) > 0,
    )[:MAX_CANDIDATE_IDENTITIES]
    history_values = history[-MAX_HISTORY_STEPS:]
    state[: len(state_values)] = state_values
    action_ids[: len(action_values)] = action_values
    inventory[: len(inventory_values)] = inventory_values
    candidate[: len(candidate_values)] = candidate_values
    if history_values:
        history_ids[-len(history_values) :] = history_values
    numeric = np.asarray(measurement_vector(measurements), dtype=np.float32)
    action_numeric = np.asarray(
        action_parameter_vector(action, measurements), dtype=np.float32
    )
    return (
        state,
        action_ids,
        inventory,
        candidate,
        numeric,
        action_numeric,
        history_ids,
    )


class CandidateReservoir:
    def __init__(self, capacity: int, seed: int):
        self.capacity = capacity
        self.rng = random.Random(seed)
        self.seen = 0
        self.size = 0
        self.state = np.zeros((capacity, MAX_STATE_FEATURES), dtype=np.uint16)
        self.action = np.zeros((capacity, MAX_ACTION_FEATURES), dtype=np.uint16)
        self.inventory = np.zeros((capacity, MAX_INVENTORY_IDENTITIES), dtype=np.uint16)
        self.candidate = np.zeros((capacity, MAX_CANDIDATE_IDENTITIES), dtype=np.uint16)
        self.numeric = np.zeros((capacity, len(MEASUREMENT_SPECS)), dtype=np.float32)
        self.action_numeric = np.zeros(
            (capacity, len(ACTION_PARAMETER_SPECS)), dtype=np.float32
        )
        self.history = np.zeros((capacity, MAX_HISTORY_STEPS), dtype=np.uint16)
        self.target = np.zeros((capacity, len(LEGACY_TARGET_NAMES)), dtype=np.float32)
        self.seed = np.zeros(capacity, dtype=np.int64)

    def add(
        self, encoded: tuple[np.ndarray, ...], target: np.ndarray, seed: int
    ) -> None:
        self.seen += 1
        if self.size < self.capacity:
            index = self.size
            self.size += 1
        else:
            index = self.rng.randrange(self.seen)
            if index >= self.capacity:
                return
        for destination, source in zip(
            (
                self.state,
                self.action,
                self.inventory,
                self.candidate,
                self.numeric,
                self.action_numeric,
                self.history,
            ),
            encoded,
        ):
            destination[index] = source
        self.target[index] = target
        self.seed[index] = seed

    def tensors(self) -> tuple[torch.Tensor, ...]:
        arrays = (
            self.state,
            self.action,
            self.inventory,
            self.candidate,
            self.numeric,
            self.action_numeric,
            self.history,
            self.target,
            self.seed,
        )
        return tuple(torch.from_numpy(value[: self.size].copy()) for value in arrays)


class ImitationReservoir:
    def __init__(self, capacity: int, seed: int):
        self.capacity = capacity
        self.rng = random.Random(seed)
        self.seen = 0
        self.size = 0
        self.state = np.zeros((capacity, MAX_STATE_FEATURES), dtype=np.uint16)
        self.inventory = np.zeros((capacity, MAX_INVENTORY_IDENTITIES), dtype=np.uint16)
        self.numeric = np.zeros((capacity, len(MEASUREMENT_SPECS)), dtype=np.float32)
        self.history = np.zeros((capacity, MAX_HISTORY_STEPS), dtype=np.uint16)
        self.action = np.zeros(
            (capacity, MAX_IMITATION_ACTIONS, MAX_ACTION_FEATURES), dtype=np.uint16
        )
        self.candidate = np.zeros(
            (capacity, MAX_IMITATION_ACTIONS, MAX_CANDIDATE_IDENTITIES),
            dtype=np.uint16,
        )
        self.action_numeric = np.zeros(
            (capacity, MAX_IMITATION_ACTIONS, len(ACTION_PARAMETER_SPECS)),
            dtype=np.float32,
        )
        self.mask = np.zeros((capacity, MAX_IMITATION_ACTIONS), dtype=np.bool_)
        self.selected = np.zeros(capacity, dtype=np.int64)
        self.seed = np.zeros(capacity, dtype=np.int64)

    def add(
        self,
        observation: dict[str, Any],
        measurements: dict[str, Any],
        selected_action_index: int,
        history: list[int],
        seed: int,
    ) -> None:
        actions = observation["actions"]
        if len(actions) < 2 or len(actions) > MAX_IMITATION_ACTIONS:
            return
        selected_position = next(
            (
                position
                for position, action in enumerate(actions)
                if int(action["index"]) == selected_action_index
            ),
            None,
        )
        if selected_position is None:
            raise ValueError(f"selected action {selected_action_index} is absent")
        self.seen += 1
        if self.size < self.capacity:
            index = self.size
            self.size += 1
        else:
            index = self.rng.randrange(self.seen)
            if index >= self.capacity:
                return
        encoded = [
            encode_candidate(observation, measurements, int(action["index"]), history)
            for action in actions
        ]
        first = encoded[0]
        self.state[index] = first[0]
        self.inventory[index] = first[2]
        self.numeric[index] = first[4]
        self.history[index] = first[6]
        self.action[index].fill(0)
        self.candidate[index].fill(0)
        self.action_numeric[index].fill(0)
        self.mask[index].fill(False)
        for action_position, values in enumerate(encoded):
            self.action[index, action_position] = values[1]
            self.candidate[index, action_position] = values[3]
            self.action_numeric[index, action_position] = values[5]
        self.mask[index, : len(encoded)] = True
        self.selected[index] = selected_position
        self.seed[index] = seed

    def tensors(self) -> tuple[torch.Tensor, ...]:
        arrays = (
            self.state,
            self.inventory,
            self.numeric,
            self.history,
            self.action,
            self.candidate,
            self.action_numeric,
            self.mask,
            self.selected,
            self.seed,
        )
        return tuple(torch.from_numpy(value[: self.size].copy()) for value in arrays)


def collect_trajectories(
    paths: list[Path],
    reservoir: CandidateReservoir,
    steps_per_episode: int,
    episodes_per_seed: int,
    seed: int,
) -> dict[str, int]:
    rng = random.Random(seed)
    paths = list(paths)
    rng.shuffle(paths)
    episode_counts: dict[int, int] = defaultdict(int)
    episodes = 0
    for episode in iter_episodes(paths):
        run_seed = int(episode["result"]["seed"])
        if episode_counts[run_seed] >= episodes_per_seed:
            continue
        episode_counts[run_seed] += 1
        episodes += 1
        transitions = episode["transitions"]
        if not transitions:
            continue
        count = min(steps_per_episode, len(transitions))
        chosen_steps = set(
            np.linspace(0, len(transitions) - 1, count, dtype=np.int64).tolist()
        )
        target = progress_target(episode)
        history: list[int] = []
        for index, transition in enumerate(transitions):
            action_index = int(transition["action_index"])
            observation = transition["observation"]
            if index in chosen_steps:
                reservoir.add(
                    encode_candidate(
                        observation,
                        transition["before"],
                        action_index,
                        history,
                    ),
                    target,
                    run_seed,
                )
            history.append(decision_signature(observation, action_index))
    return {
        "files": len(paths),
        "episodes": episodes,
        "unique_seeds": len(episode_counts),
        "candidate_samples_seen": reservoir.seen,
        "candidate_samples_kept": reservoir.size,
    }


def collect_imitation(
    paths: list[Path], reservoir: ImitationReservoir, seed: int
) -> dict[str, int]:
    rng = random.Random(seed)
    paths = list(paths)
    rng.shuffle(paths)
    episodes = 0
    for episode in iter_episodes(paths):
        episodes += 1
        run_seed = int(episode["result"]["seed"])
        history: list[int] = []
        for transition in episode["transitions"]:
            action_index = int(transition["action_index"])
            observation = transition["observation"]
            reservoir.add(
                observation,
                transition["before"],
                action_index,
                history,
                run_seed,
            )
            history.append(decision_signature(observation, action_index))
    return {
        "files": len(paths),
        "episodes": episodes,
        "menus_seen": reservoir.seen,
        "menus_kept": reservoir.size,
    }


def split_tensors(
    tensors: tuple[torch.Tensor, ...], seed_index: int
) -> dict[str, tuple[torch.Tensor, ...]]:
    seeds = tensors[seed_index]
    validation = seeds.abs().remainder(10).eq(0)
    return {
        "train": tuple(value[~validation] for value in tensors[:-1]),
        "validation": tuple(value[validation] for value in tensors[:-1]),
    }


def prepare(args: argparse.Namespace) -> dict[str, Any]:
    if (
        args.cache.exists()
        and not args.rebuild_cache
        and args.trajectory is None
        and args.imitation is None
    ):
        cached = torch.load(args.cache, map_location="cpu", weights_only=False)
        if cached.get("format") != "sts-mean-progress-data-v2":
            raise ValueError(f"{args.cache}: unsupported dataset cache")
        print(f"loaded frozen canonical dataset cache {args.cache}", flush=True)
        return cached

    trajectory_paths = args.trajectory or sorted(
        Path("artifacts/selfplay").glob("defect-a20-*-traces.jsonl*")
    )
    imitation_paths = args.imitation or sorted(
        path
        for path in Path("artifacts/selfplay").glob(
            "defect-a20-mean-progress-v4-*-traces.jsonl*"
        )
        if "planned" in path.name or "onpolicy-branches" in path.name
    )
    signature = {
        "trajectories": file_signature(trajectory_paths),
        "imitation": file_signature(imitation_paths),
        "max_pointwise": args.max_pointwise,
        "max_imitation_menus": args.max_imitation_menus,
        "max_imitation_actions": MAX_IMITATION_ACTIONS,
        "steps_per_episode": args.steps_per_episode,
        "episodes_per_seed": args.episodes_per_seed,
        "measurement_specs": MEASUREMENT_SPECS,
        "action_parameter_specs": ACTION_PARAMETER_SPECS,
    }
    digest = hashlib.sha256(
        json.dumps(signature, sort_keys=True).encode("utf-8")
    ).hexdigest()
    if args.cache.exists() and not args.rebuild_cache:
        cached = torch.load(args.cache, map_location="cpu", weights_only=False)
        if cached.get("signature_sha256") == digest:
            print(f"loaded canonical dataset cache {args.cache}", flush=True)
            return cached

    pointwise = CandidateReservoir(args.max_pointwise, args.seed)
    trajectory_stats = collect_trajectories(
        trajectory_paths,
        pointwise,
        args.steps_per_episode,
        args.episodes_per_seed,
        args.seed,
    )
    print(f"trajectory evidence: {trajectory_stats}", flush=True)
    imitation = ImitationReservoir(args.max_imitation_menus, args.seed + 1)
    imitation_stats = collect_imitation(imitation_paths, imitation, args.seed + 1)
    print(f"planner imitation evidence: {imitation_stats}", flush=True)
    prepared = {
        "format": "sts-mean-progress-data-v2",
        "signature_sha256": digest,
        "signature": signature,
        "trajectory_stats": trajectory_stats,
        "imitation_stats": imitation_stats,
        "pointwise": split_tensors(pointwise.tensors(), -1),
        "imitation": split_tensors(imitation.tensors(), -1),
    }
    args.cache.parent.mkdir(parents=True, exist_ok=True)
    torch.save(prepared, args.cache)
    print(f"saved canonical dataset cache {args.cache}", flush=True)
    return prepared


def model_call(
    model: MeanProgressModel, tensors: tuple[torch.Tensor, ...]
) -> torch.Tensor:
    state, action, inventory, candidate, numeric, action_numeric, history = tensors
    return model(state, action, numeric, history, inventory, candidate, action_numeric)


def selection_score(metrics: dict[str, float | int]) -> float:
    return float(metrics["progress_mae"]) + 0.1 * (
        1.0 - float(metrics["imitation_top_accuracy"])
    )


def cpu_state_dict(model: MeanProgressModel) -> dict[str, torch.Tensor]:
    return {
        name: value.detach().cpu().clone() for name, value in model.state_dict().items()
    }


def checkpoint_payload(
    model_state: dict[str, torch.Tensor],
    config: dict[str, Any],
    prepared: dict[str, Any],
    training: dict[str, Any],
    metrics: dict[str, float | int],
) -> dict[str, Any]:
    return {
        "format": "sts-mean-progress-v1",
        "teacher": None,
        "config": config,
        "target_names": TARGET_NAMES,
        "model": model_state,
        "dataset_signature": prepared["signature"],
        "training": training,
        "validation": metrics,
    }


def to_device(
    batch: tuple[torch.Tensor, ...], device: torch.device
) -> tuple[torch.Tensor, ...]:
    return tuple(value.to(device, non_blocking=True) for value in batch)


def imitation_logits(
    model: MeanProgressModel, tensors: tuple[torch.Tensor, ...]
) -> torch.Tensor:
    state, inventory, numeric, history, action, candidate, action_numeric, mask = (
        tensors
    )
    visible = mask.flatten().nonzero(as_tuple=False).squeeze(1)
    owners = (
        torch.arange(len(state), device=state.device)[:, None]
        .expand_as(mask)
        .masked_select(mask)
    )
    prediction = model_call(
        model,
        (
            state.index_select(0, owners),
            action.flatten(0, 1).index_select(0, visible),
            inventory.index_select(0, owners),
            candidate.flatten(0, 1).index_select(0, visible),
            numeric.index_select(0, owners),
            action_numeric.flatten(0, 1).index_select(0, visible),
            history.index_select(0, owners),
        ),
    )[:, 0]
    return prediction.new_full(mask.shape, -torch.inf).masked_scatter(mask, prediction)


def masked_cross_entropy(
    logits: torch.Tensor,
    selected: torch.Tensor,
    mask: torch.Tensor,
    label_smoothing: float,
) -> torch.Tensor:
    log_probabilities = F.log_softmax(logits, dim=1)
    selected_loss = -log_probabilities.gather(1, selected[:, None]).squeeze(1)
    smooth_loss = -log_probabilities.masked_fill(~mask, 0.0).sum(1) / mask.sum(1)
    return (
        (1.0 - label_smoothing) * selected_loss + label_smoothing * smooth_loss
    ).mean()


@torch.inference_mode()
def validate(
    model: MeanProgressModel,
    point_loader: DataLoader,
    imitation_loader: DataLoader,
    device: torch.device,
) -> dict[str, float | int]:
    model.eval()
    progress_error = floor_error = hp_error = 0.0
    points = 0
    for batch in point_loader:
        *inputs, target = to_device(batch, device)
        prediction = model_call(model, tuple(inputs))
        progress_error += (prediction[:, 1] - target[:, 0]).abs().sum().item()
        floor_error += (prediction[:, 2] - target[:, 1]).abs().sum().item() * 50.0
        hp_error += (prediction[:, 3] - target[:, 2]).abs().sum().item()
        points += len(target)
    correct = menus = 0
    for batch in imitation_loader:
        *inputs, selected = to_device(batch, device)
        logits = imitation_logits(model, tuple(inputs))
        correct += logits.argmax(1).eq(selected).sum().item()
        menus += len(selected)
    return {
        "pointwise_samples": points,
        "progress_mae": progress_error / max(points, 1),
        "floor_mae": floor_error / max(points, 1),
        "entry_hp_fraction_mae": hp_error / max(points, 1),
        "imitation_menus": menus,
        "imitation_top_accuracy": correct / max(menus, 1),
    }


def train(args: argparse.Namespace) -> None:
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)
        torch.set_float32_matmul_precision("high")
    device = torch.device(
        "cuda" if args.device == "auto" and torch.cuda.is_available() else args.device
    )
    prepared = prepare(args)
    point_train = TensorDataset(*prepared["pointwise"]["train"])
    point_validation = TensorDataset(*prepared["pointwise"]["validation"])
    imitation_train = TensorDataset(*prepared["imitation"]["train"])
    imitation_validation = TensorDataset(*prepared["imitation"]["validation"])
    if not point_train or not imitation_train:
        raise ValueError("training requires trajectory and planner imitation evidence")
    point_loader = DataLoader(
        point_train, batch_size=args.batch_size, shuffle=True, pin_memory=True
    )
    imitation_loader = DataLoader(
        imitation_train,
        batch_size=max(args.batch_size // 2, 1),
        shuffle=True,
        pin_memory=True,
    )
    point_validation_loader = DataLoader(
        point_validation, batch_size=args.batch_size, shuffle=False
    )
    imitation_validation_loader = DataLoader(
        imitation_validation, batch_size=max(args.batch_size // 2, 1), shuffle=False
    )
    config = {
        "hidden_size": args.hidden_size,
        "numeric_measurements": len(MEASUREMENT_SPECS),
        "action_numeric_measurements": len(ACTION_PARAMETER_SPECS),
        "target_names": TARGET_NAMES,
    }
    model = MeanProgressModel(config).to(device)
    optimizer = torch.optim.AdamW(
        model.parameters(), lr=args.learning_rate, weight_decay=args.weight_decay
    )
    scaler_enabled = device.type == "cuda"
    amp_dtype = (
        torch.bfloat16
        if device.type == "cuda" and torch.cuda.is_bf16_supported()
        else torch.float16
    )
    started = time.monotonic()
    steps = 0
    best_step = 0
    best_elapsed_seconds = 0.0
    best_score = float("inf")
    best_metrics: dict[str, float | int] | None = None
    best_model: dict[str, torch.Tensor] | None = None
    next_validation = args.validation_interval_seconds
    point_iterator = iter(point_loader)
    imitation_iterator = iter(imitation_loader)
    model.train()
    while time.monotonic() - started < args.seconds:
        try:
            point_batch = next(point_iterator)
        except StopIteration:
            point_iterator = iter(point_loader)
            point_batch = next(point_iterator)
        try:
            imitation_batch = next(imitation_iterator)
        except StopIteration:
            imitation_iterator = iter(imitation_loader)
            imitation_batch = next(imitation_iterator)
        *point_inputs, point_target = to_device(point_batch, device)
        *imitation_inputs, imitation_selected = to_device(imitation_batch, device)
        optimizer.zero_grad(set_to_none=True)
        with torch.autocast(
            device_type=device.type, dtype=amp_dtype, enabled=scaler_enabled
        ):
            point_prediction = model_call(model, tuple(point_inputs))
            point_loss = (
                F.smooth_l1_loss(point_prediction[:, 1], point_target[:, 0])
                + 0.5 * F.smooth_l1_loss(point_prediction[:, 2], point_target[:, 1])
                + 0.25 * F.smooth_l1_loss(point_prediction[:, 3], point_target[:, 2])
            )
            logits = imitation_logits(model, tuple(imitation_inputs))
            imitation_loss = masked_cross_entropy(
                logits,
                imitation_selected,
                imitation_inputs[-1],
                args.imitation_label_smoothing,
            )
            loss = point_loss + args.imitation_loss_weight * imitation_loss
        loss.backward()
        nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        steps += 1
        if steps % 100 == 0:
            print(
                f"step={steps} point={point_loss.item():.5f} "
                f"imitation={imitation_loss.item():.5f}",
                flush=True,
            )

        elapsed = time.monotonic() - started
        if elapsed >= next_validation:
            metrics = validate(
                model,
                point_validation_loader,
                imitation_validation_loader,
                device,
            )
            score = selection_score(metrics)
            model_state = cpu_state_dict(model)
            if score < best_score:
                best_step = steps
                best_elapsed_seconds = elapsed
                best_score = score
                best_metrics = metrics
                best_model = model_state
            if args.snapshot_dir is not None:
                args.snapshot_dir.mkdir(parents=True, exist_ok=True)
                snapshot_training = {
                    "seconds": elapsed,
                    "steps": steps,
                    "parameters": sum(
                        parameter.numel() for parameter in model.parameters()
                    ),
                    "learning_rate": args.learning_rate,
                    "weight_decay": args.weight_decay,
                    "imitation_loss_weight": args.imitation_loss_weight,
                    "imitation_label_smoothing": args.imitation_label_smoothing,
                    "selection_score": score,
                }
                torch.save(
                    checkpoint_payload(
                        model_state,
                        config,
                        prepared,
                        snapshot_training,
                        metrics,
                    ),
                    args.snapshot_dir / f"step-{steps}.pt",
                )
            print(
                f"selection step={steps} score={score:.6f} "
                f"best={best_score:.6f} progress_mae={metrics['progress_mae']:.6f} "
                f"imitation_accuracy={metrics['imitation_top_accuracy']:.6f}",
                flush=True,
            )
            next_validation += args.validation_interval_seconds
            model.train()

    final_metrics = validate(
        model,
        point_validation_loader,
        imitation_validation_loader,
        device,
    )
    final_score = selection_score(final_metrics)
    if final_score < best_score:
        best_step = steps
        best_elapsed_seconds = time.monotonic() - started
        best_score = final_score
        best_metrics = final_metrics
        best_model = cpu_state_dict(model)
    assert best_model is not None and best_metrics is not None
    parameters = sum(parameter.numel() for parameter in model.parameters())
    checkpoint = checkpoint_payload(
        best_model,
        config,
        prepared,
        {
            "seconds": time.monotonic() - started,
            "steps": steps,
            "parameters": parameters,
            "learning_rate": args.learning_rate,
            "weight_decay": args.weight_decay,
            "imitation_loss_weight": args.imitation_loss_weight,
            "imitation_label_smoothing": args.imitation_label_smoothing,
            "selected_step": best_step,
            "selected_elapsed_seconds": best_elapsed_seconds,
            "selection_score": best_score,
            "final_unselected_validation": final_metrics,
        },
        best_metrics,
    )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    torch.save(checkpoint, args.output)
    metrics_path = args.output.with_suffix(".metrics.json")
    metrics_path.write_text(
        json.dumps(checkpoint | {"model": None}, indent=2, default=str)
    )
    print(
        json.dumps(
            {
                "output": str(args.output),
                "parameters": parameters,
                "steps": steps,
                "selected_step": best_step,
                "selection_score": best_score,
                "validation": best_metrics,
            },
            indent=2,
        ),
        flush=True,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trajectory", type=Path, action="append")
    parser.add_argument("--imitation", type=Path, action="append")
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path("artifacts/selfplay/defect-a20-mean-progress-v10-distill-data.pt"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path(
            "artifacts/selfplay/defect-a20-mean-progress-v10-distill-selected-10m.pt"
        ),
    )
    parser.add_argument("--rebuild-cache", action="store_true")
    parser.add_argument("--seconds", type=float, default=600.0)
    parser.add_argument("--hidden-size", type=int, default=96)
    parser.add_argument("--batch-size", type=int, default=256)
    parser.add_argument("--learning-rate", type=float, default=3e-4)
    parser.add_argument("--weight-decay", type=float, default=0.02)
    parser.add_argument("--max-pointwise", type=int, default=160_000)
    parser.add_argument("--max-imitation-menus", type=int, default=64_000)
    parser.add_argument("--steps-per-episode", type=int, default=16)
    parser.add_argument("--episodes-per-seed", type=int, default=4)
    parser.add_argument("--imitation-loss-weight", type=float, default=1.0)
    parser.add_argument("--imitation-label-smoothing", type=float, default=0.05)
    parser.add_argument("--validation-interval-seconds", type=float, default=60.0)
    parser.add_argument("--snapshot-dir", type=Path)
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--seed", type=int, default=20260827)
    args = parser.parse_args()
    if (
        min(
            args.seconds,
            args.hidden_size,
            args.batch_size,
            args.learning_rate,
            args.max_pointwise,
            args.max_imitation_menus,
            args.steps_per_episode,
            args.episodes_per_seed,
            args.validation_interval_seconds,
        )
        <= 0
        or args.weight_decay < 0
        or args.imitation_loss_weight < 0
        or not 0 <= args.imitation_label_smoothing < 1
    ):
        parser.error("training sizes and rates must be positive")
    return args


if __name__ == "__main__":
    train(parse_args())
