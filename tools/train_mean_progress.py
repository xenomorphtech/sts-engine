"""Train the compact mean-floor/retained-HP policy from scratch.

With no data arguments the command consumes all local Defect A20 trajectory
and exact-branch files. It seed-balances trajectories, samples decisions across
each run, and combines Monte-Carlo progress regression with exact pairwise
action ranking. There is intentionally no checkpoint-initialization mode.
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
from mean_progress_model import TARGET_NAMES, MeanProgressModel
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
    iter_branch_rows,
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
        self.target = np.zeros((capacity, len(TARGET_NAMES)), dtype=np.float32)
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


class PairReservoir:
    def __init__(self, capacity: int, seed: int):
        self.capacity = capacity
        self.rng = random.Random(seed)
        self.seen = 0
        self.size = 0
        self.state = np.zeros((capacity, 2, MAX_STATE_FEATURES), dtype=np.uint16)
        self.action = np.zeros((capacity, 2, MAX_ACTION_FEATURES), dtype=np.uint16)
        self.inventory = np.zeros(
            (capacity, 2, MAX_INVENTORY_IDENTITIES), dtype=np.uint16
        )
        self.candidate = np.zeros(
            (capacity, 2, MAX_CANDIDATE_IDENTITIES), dtype=np.uint16
        )
        self.numeric = np.zeros((capacity, 2, len(MEASUREMENT_SPECS)), dtype=np.float32)
        self.action_numeric = np.zeros(
            (capacity, 2, len(ACTION_PARAMETER_SPECS)), dtype=np.float32
        )
        self.history = np.zeros((capacity, 2, MAX_HISTORY_STEPS), dtype=np.uint16)
        self.seed = np.zeros(capacity, dtype=np.int64)

    def add(
        self,
        better: tuple[np.ndarray, ...],
        worse: tuple[np.ndarray, ...],
        seed: int,
    ) -> None:
        self.seen += 1
        if self.size < self.capacity:
            index = self.size
            self.size += 1
        else:
            index = self.rng.randrange(self.seen)
            if index >= self.capacity:
                return
        for destination, first, second in zip(
            (
                self.state,
                self.action,
                self.inventory,
                self.candidate,
                self.numeric,
                self.action_numeric,
                self.history,
            ),
            better,
            worse,
        ):
            destination[index, 0] = first
            destination[index, 1] = second
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


def branch_group_key(row: dict[str, Any]) -> tuple[Any, ...]:
    return (
        int(row["seed"]),
        int(row.get("step", -1)),
        tuple(row["observation"]["state_features"]),
    )


def add_branch_group(
    rows: list[dict[str, Any]], reservoir: PairReservoir, pairs_per_menu: int
) -> None:
    if not rows:
        return
    by_action: dict[int, list[dict[str, Any]]] = defaultdict(list)
    for row in rows:
        by_action[int(row["action_index"])].append(row)
    ranked = sorted(
        (
            (
                sum(float(row["branch_score"]) for row in action_rows)
                / len(action_rows),
                action_rows[0],
            )
            for action_rows in by_action.values()
        ),
        key=lambda item: item[0],
        reverse=True,
    )
    if len(ranked) < 2 or ranked[0][0] == ranked[-1][0]:
        return
    best_score, best = ranked[0]
    comparisons = [item for item in reversed(ranked[1:]) if item[0] < best_score]
    for _, worse in comparisons[:pairs_per_menu]:
        reservoir.add(
            encode_candidate(
                best["observation"],
                best["before"],
                int(best["action_index"]),
                list(best.get("history", [])),
            ),
            encode_candidate(
                worse["observation"],
                worse["before"],
                int(worse["action_index"]),
                list(worse.get("history", [])),
            ),
            int(best["seed"]),
        )


def collect_branches(
    paths: list[Path],
    reservoir: PairReservoir,
    pairs_per_menu: int,
    seed: int,
) -> dict[str, int]:
    rng = random.Random(seed)
    paths = list(paths)
    rng.shuffle(paths)
    menus = 0
    for path in paths:
        key = None
        rows: list[dict[str, Any]] = []
        for row in iter_branch_rows([path]):
            row_key = branch_group_key(row)
            if key is not None and row_key != key:
                add_branch_group(rows, reservoir, pairs_per_menu)
                menus += 1
                rows = []
            key = row_key
            rows.append(row)
        if rows:
            add_branch_group(rows, reservoir, pairs_per_menu)
            menus += 1
    return {
        "files": len(paths),
        "menus": menus,
        "pairs_seen": reservoir.seen,
        "pairs_kept": reservoir.size,
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
    trajectory_paths = args.trajectory or sorted(
        Path("artifacts/selfplay").glob("defect-a20-*-traces.jsonl*")
    )
    branch_paths = args.branch or sorted(
        Path("artifacts/selfplay").glob("defect-a20-*-branches.jsonl*")
    )
    signature = {
        "trajectories": file_signature(trajectory_paths),
        "branches": file_signature(branch_paths),
        "max_pointwise": args.max_pointwise,
        "max_branch_pairs": args.max_branch_pairs,
        "steps_per_episode": args.steps_per_episode,
        "episodes_per_seed": args.episodes_per_seed,
        "pairs_per_menu": args.pairs_per_menu,
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
    pairs = PairReservoir(args.max_branch_pairs, args.seed + 1)
    branch_stats = collect_branches(
        branch_paths, pairs, args.pairs_per_menu, args.seed + 1
    )
    print(f"exact branch evidence: {branch_stats}", flush=True)
    prepared = {
        "format": "sts-mean-progress-data-v1",
        "signature_sha256": digest,
        "signature": signature,
        "trajectory_stats": trajectory_stats,
        "branch_stats": branch_stats,
        "pointwise": split_tensors(pointwise.tensors(), -1),
        "pairs": split_tensors(pairs.tensors(), -1),
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


def to_device(
    batch: tuple[torch.Tensor, ...], device: torch.device
) -> tuple[torch.Tensor, ...]:
    return tuple(value.to(device, non_blocking=True) for value in batch)


@torch.inference_mode()
def validate(
    model: MeanProgressModel,
    point_loader: DataLoader,
    pair_loader: DataLoader,
    device: torch.device,
) -> dict[str, float | int]:
    model.eval()
    progress_error = floor_error = hp_error = 0.0
    points = 0
    for batch in point_loader:
        *inputs, target = to_device(batch, device)
        prediction = model_call(model, tuple(inputs))
        progress_error += (prediction[:, 0] - target[:, 0]).abs().sum().item()
        floor_error += (prediction[:, 1] - target[:, 1]).abs().sum().item() * 50.0
        hp_error += (prediction[:, 2] - target[:, 2]).abs().sum().item()
        points += len(target)
    correct = pairs = 0
    for batch in pair_loader:
        inputs = to_device(batch, device)
        batch_size = inputs[0].shape[0]
        flat = tuple(value.flatten(0, 1) for value in inputs)
        value = model_call(model, flat)[:, 0].reshape(batch_size, 2)
        correct += value[:, 0].gt(value[:, 1]).sum().item()
        pairs += batch_size
    return {
        "pointwise_samples": points,
        "progress_mae": progress_error / max(points, 1),
        "floor_mae": floor_error / max(points, 1),
        "entry_hp_fraction_mae": hp_error / max(points, 1),
        "branch_pairs": pairs,
        "branch_pair_accuracy": correct / max(pairs, 1),
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
    pair_train = TensorDataset(*prepared["pairs"]["train"])
    pair_validation = TensorDataset(*prepared["pairs"]["validation"])
    if not point_train or not pair_train:
        raise ValueError("training requires both trajectory and exact branch evidence")
    point_loader = DataLoader(
        point_train, batch_size=args.batch_size, shuffle=True, pin_memory=True
    )
    pair_loader = DataLoader(
        pair_train,
        batch_size=max(args.batch_size // 2, 1),
        shuffle=True,
        pin_memory=True,
    )
    point_validation_loader = DataLoader(
        point_validation, batch_size=args.batch_size, shuffle=False
    )
    pair_validation_loader = DataLoader(
        pair_validation, batch_size=max(args.batch_size // 2, 1), shuffle=False
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
    point_iterator = iter(point_loader)
    pair_iterator = iter(pair_loader)
    model.train()
    while time.monotonic() - started < args.seconds:
        try:
            point_batch = next(point_iterator)
        except StopIteration:
            point_iterator = iter(point_loader)
            point_batch = next(point_iterator)
        try:
            pair_batch = next(pair_iterator)
        except StopIteration:
            pair_iterator = iter(pair_loader)
            pair_batch = next(pair_iterator)
        *point_inputs, point_target = to_device(point_batch, device)
        pair_inputs = to_device(pair_batch, device)
        pair_batch_size = pair_inputs[0].shape[0]
        pair_flat = tuple(value.flatten(0, 1) for value in pair_inputs)
        optimizer.zero_grad(set_to_none=True)
        with torch.autocast(
            device_type=device.type, dtype=amp_dtype, enabled=scaler_enabled
        ):
            point_prediction = model_call(model, tuple(point_inputs))
            point_loss = (
                F.smooth_l1_loss(point_prediction[:, 0], point_target[:, 0])
                + 0.5 * F.smooth_l1_loss(point_prediction[:, 1], point_target[:, 1])
                + 0.25 * F.smooth_l1_loss(point_prediction[:, 2], point_target[:, 2])
            )
            pair_value = model_call(model, pair_flat)[:, 0].reshape(pair_batch_size, 2)
            pair_loss = F.softplus(
                -args.pair_margin_scale * (pair_value[:, 0] - pair_value[:, 1])
            ).mean()
            loss = point_loss + args.pair_loss_weight * pair_loss
        loss.backward()
        nn.utils.clip_grad_norm_(model.parameters(), 1.0)
        optimizer.step()
        steps += 1
        if steps % 100 == 0:
            print(
                f"step={steps} point={point_loss.item():.5f} "
                f"pair={pair_loss.item():.5f}",
                flush=True,
            )

    metrics = validate(
        model,
        point_validation_loader,
        pair_validation_loader,
        device,
    )
    parameters = sum(parameter.numel() for parameter in model.parameters())
    checkpoint = {
        "format": "sts-mean-progress-v1",
        "teacher": None,
        "config": config,
        "target_names": TARGET_NAMES,
        "model": {
            name: value.detach().cpu() for name, value in model.state_dict().items()
        },
        "dataset_signature": prepared["signature"],
        "training": {
            "seconds": time.monotonic() - started,
            "steps": steps,
            "parameters": parameters,
            "learning_rate": args.learning_rate,
            "weight_decay": args.weight_decay,
            "pair_loss_weight": args.pair_loss_weight,
            "pair_margin_scale": args.pair_margin_scale,
        },
        "validation": metrics,
    }
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
                "validation": metrics,
            },
            indent=2,
        ),
        flush=True,
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--trajectory", type=Path, action="append")
    parser.add_argument("--branch", type=Path, action="append")
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path("artifacts/selfplay/defect-a20-mean-progress-v1-data.pt"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a20-mean-progress-v1.pt"),
    )
    parser.add_argument("--rebuild-cache", action="store_true")
    parser.add_argument("--seconds", type=float, default=180.0)
    parser.add_argument("--hidden-size", type=int, default=96)
    parser.add_argument("--batch-size", type=int, default=256)
    parser.add_argument("--learning-rate", type=float, default=3e-4)
    parser.add_argument("--weight-decay", type=float, default=0.02)
    parser.add_argument("--max-pointwise", type=int, default=160_000)
    parser.add_argument("--max-branch-pairs", type=int, default=100_000)
    parser.add_argument("--steps-per-episode", type=int, default=16)
    parser.add_argument("--episodes-per-seed", type=int, default=4)
    parser.add_argument("--pairs-per-menu", type=int, default=4)
    parser.add_argument("--pair-loss-weight", type=float, default=0.5)
    parser.add_argument("--pair-margin-scale", type=float, default=4.0)
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
            args.max_branch_pairs,
            args.steps_per_episode,
            args.episodes_per_seed,
            args.pairs_per_menu,
            args.pair_margin_scale,
        )
        <= 0
        or args.weight_decay < 0
        or args.pair_loss_weight < 0
    ):
        parser.error("training sizes and rates must be positive")
    return args


if __name__ == "__main__":
    train(parse_args())
