#!/usr/bin/env python3
"""Train a teacher-free HRM value model on whole Defect A0 trajectories.

The input is produced only by ``sts-selfplay --transitions-jsonl``. There are
no expert actions, HTN decisions, imitation labels, seed tokens, or RNG-state
tokens in this pipeline. The model scores dynamic legal actions by predicting
outcome and multi-horizon environment measurements for a state/action pair.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import lzma
import math
from pathlib import Path
import time
from typing import Any, Iterable

try:
    import torch
    from torch import nn
    from torch.nn import functional as F
    from torch.utils.data import DataLoader, TensorDataset
except ImportError as exc:
    raise SystemExit("PyTorch is required; invoke this script through uv") from exc


FEATURE_BUCKETS = 32_768
MAX_STATE_FEATURES = 256
MAX_ACTION_FEATURES = 16
MAX_HISTORY_STEPS = 64
MEASUREMENT_SPECS = (
    ("act", 3.0),
    ("floor", 52.0),
    ("hp", 100.0),
    ("max_hp", 100.0),
    ("block", 100.0),
    ("gold", 500.0),
    ("energy", 10.0),
    ("energy_master", 10.0),
    ("deck_size", 50.0),
    ("upgraded_cards", 50.0),
    ("distinct_cards", 50.0),
    ("deck_base_damage", 500.0),
    ("deck_base_block", 500.0),
    ("deck_base_magic", 100.0),
    ("relics", 30.0),
    ("potions", 5.0),
    ("player_power_amount", 100.0),
    ("hand_size", 10.0),
    ("draw_size", 50.0),
    ("discard_size", 50.0),
    ("exhaust_size", 50.0),
    ("playable_cards", 10.0),
    ("zero_cost_cards", 10.0),
    ("orb_slots", 10.0),
    ("filled_orbs", 10.0),
    ("dark_evoke", 500.0),
    ("combat_turn", 50.0),
    ("cards_played_this_turn", 20.0),
    ("living_enemies", 10.0),
    ("enemy_hp", 500.0),
    ("enemy_max_hp", 500.0),
    ("enemy_block", 500.0),
    ("enemy_power_amount", 500.0),
    ("incoming_attack", 200.0),
    ("legal_actions", 100.0),
)
TARGET_NAMES = (
    "act3_win",
    "reach_act1_boss",
    "reach_act2",
    "reach_act2_boss",
    "reach_act3",
    "reach_act3_boss",
    "max_floor",
    "terminal_margin",
    "combat_margin",
    "search_value",
    "hp_delta_1",
    "enemy_hp_delta_1",
    "hp_delta_8",
    "enemy_hp_delta_8",
    "floor_delta_32",
    "gold_delta_32",
    "relic_delta_128",
    "upgrade_delta_128",
)
TARGET_SCALES = (
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    1.0,
    52.0,
    300.0,
    300.0,
    1_000.0,
    100.0,
    300.0,
    100.0,
    300.0,
    32.0,
    500.0,
    10.0,
    10.0,
)
DEFAULTS = {
    "hidden_size": 128,
    "expansion": 3,
    "h_cycles": 2,
    "l_cycles": 2,
    "segments": 2,
    "batch_size": 512,
    "learning_rate": 3e-4,
    "weight_decay": 0.03,
    "seconds": 120.0,
}


def log(message: str) -> None:
    print(message, flush=True)


def open_jsonl(path: Path) -> Iterable[str]:
    if path.suffix == ".xz":
        return lzma.open(path, "rt", encoding="utf-8")
    return path.open("r", encoding="utf-8")


def iter_episodes(paths: list[Path]) -> Iterable[dict[str, Any]]:
    for path in paths:
        with open_jsonl(path) as source:
            for line_number, line in enumerate(source, 1):
                if not line.strip():
                    continue
                episode = json.loads(line)
                if episode.get("schema_version") != 1:
                    raise ValueError(
                        f"{path}:{line_number}: unsupported schema "
                        f"{episode.get('schema_version')!r}"
                    )
                yield episode


def iter_branch_rows(paths: list[Path]) -> Iterable[dict[str, Any]]:
    for path in paths:
        with open_jsonl(path) as source:
            for line_number, line in enumerate(source, 1):
                if not line.strip():
                    continue
                row = json.loads(line)
                if row.get("schema_version") != 1:
                    raise ValueError(
                        f"{path}:{line_number}: unsupported branch schema "
                        f"{row.get('schema_version')!r}"
                    )
                yield row


def symlog_scaled(value: float, scale: float) -> float:
    if scale == 1.0:
        return value
    return math.copysign(math.log1p(abs(value)), value) / math.log1p(scale)


def measurement_vector(measurements: dict[str, Any]) -> list[float]:
    return [
        symlog_scaled(float(measurements[name]), scale)
        for name, scale in MEASUREMENT_SPECS
    ]


def decision_signature(observation: dict[str, Any], action_index: int) -> int:
    """Fold one visible state/action pair into a stable nonzero history token."""
    value = 0xCBF29CE484222325
    action = observation["actions"][action_index]
    for feature in observation["state_features"][:4] + action["features"]:
        value ^= int(feature)
        value = (value * 0x100000001B3) & ((1 << 64) - 1)
    return value % FEATURE_BUCKETS + 1


def split_for_seed(seed: int) -> str:
    digest = hashlib.sha256(f"selfplay-eval-v1:{seed}".encode()).digest()
    return "validation" if int.from_bytes(digest[:8], "little") % 10 == 0 else "train"


def combat_margins(transitions: list[dict[str, Any]]) -> list[tuple[float, bool]]:
    margins: list[tuple[float, bool]] = [(0.0, False)] * len(transitions)
    start = 0
    while start < len(transitions):
        before = transitions[start]["before"]
        if before["enemy_max_hp"] <= 0:
            start += 1
            continue
        end = start
        while end + 1 < len(transitions) and transitions[end]["after"]["enemy_max_hp"] > 0:
            end += 1
        final = transitions[end]
        won = final["after"]["enemy_max_hp"] == 0 and final["after"]["hp"] > 0
        margin = float(final["after"]["hp"] if won else -final["after"]["enemy_hp"])
        for index in range(start, end + 1):
            margins[index] = (margin, True)
        start = end + 1
    return margins


def target_rows(episode: dict[str, Any]) -> tuple[list[list[float]], list[list[bool]]]:
    transitions = episode["transitions"]
    result = episode["result"]
    combat = combat_margins(transitions)
    targets: list[list[float]] = []
    masks: list[list[bool]] = []
    won = result["outcome"] == "act3_boss_victory"
    max_floor = int(result["max_floor"])
    for index, transition in enumerate(transitions):
        before = transition["before"]
        h1 = transitions[index]["after"]
        h8 = transitions[min(index + 7, len(transitions) - 1)]["after"]
        h32 = transitions[min(index + 31, len(transitions) - 1)]["after"]
        h128 = transitions[min(index + 127, len(transitions) - 1)]["after"]
        same_combat_8 = (
            before["enemy_max_hp"] > 0
            and all(
                row["before"]["enemy_max_hp"] > 0
                for row in transitions[index : min(index + 8, len(transitions))]
            )
        )
        raw = [
            float(won),
            float(max_floor >= 16),
            float(max_floor >= 17),
            float(max_floor >= 33),
            float(max_floor >= 34),
            float(max_floor >= 50),
            float(max_floor),
            float(result["terminal_score"]),
            combat[index][0],
            0.0,
            float(h1["hp"] - before["hp"]),
            float(h1["enemy_hp"] - before["enemy_hp"]),
            float(h8["hp"] - before["hp"]),
            float(h8["enemy_hp"] - before["enemy_hp"]),
            float(h32["floor"] - before["floor"]),
            float(h32["gold"] - before["gold"]),
            float(h128["relics"] - before["relics"]),
            float(h128["upgraded_cards"] - before["upgraded_cards"]),
        ]
        targets.append(
            [symlog_scaled(value, scale) for value, scale in zip(raw, TARGET_SCALES)]
        )
        masks.append(
            [
                True,
                True,
                True,
                True,
                True,
                True,
                True,
                True,
                combat[index][1],
                False,
                True,
                before["enemy_max_hp"] > 0,
                True,
                same_combat_8,
                True,
                True,
                True,
                True,
            ]
        )
    return targets, masks


def episode_priority(result: dict[str, Any]) -> float:
    """Prioritize self-discovered frontiers without changing their labels."""
    floor = int(result["max_floor"])
    priority = 1.0 + 2.0 * min(floor, 16) / 16.0
    priority += 2.0 if floor >= 16 else 0.0
    priority += 6.0 if floor >= 17 else 0.0
    priority += 8.0 if floor >= 33 else 0.0
    priority += 10.0 if floor >= 34 else 0.0
    priority += 12.0 if floor >= 50 else 0.0
    return priority


def prepare(
    paths: list[Path],
    branch_paths: list[Path],
    cache: Path,
    branch_only: bool = False,
) -> dict[str, Any]:
    signature = {
        "datasets": [
            {
                "path": str(path.resolve()),
                "size": path.stat().st_size,
                "mtime_ns": path.stat().st_mtime_ns,
            }
            for path in paths
        ],
        "branch_datasets": [
            {
                "path": str(path.resolve()),
                "size": path.stat().st_size,
                "mtime_ns": path.stat().st_mtime_ns,
            }
            for path in branch_paths
        ],
        "feature_buckets": FEATURE_BUCKETS,
        "max_state_features": MAX_STATE_FEATURES,
        "max_action_features": MAX_ACTION_FEATURES,
        "max_history_steps": MAX_HISTORY_STEPS,
        "measurement_specs": MEASUREMENT_SPECS,
        "targets": TARGET_NAMES,
        "branch_only": branch_only,
        "preprocess_version": 4,
    }
    if cache.exists():
        prepared = torch.load(cache, map_location="cpu", weights_only=False)
        if prepared.get("signature") == signature:
            log(f"loaded prepared tensors from {cache}")
            return prepared

    started = time.monotonic()
    counts = {"train": 0, "validation": 0}
    episodes = {"train": 0, "validation": 0}
    branch_counts = {"train": 0, "validation": 0}
    if not branch_only:
        for episode in iter_episodes(paths):
            split = split_for_seed(int(episode["result"]["seed"]))
            episodes[split] += 1
            counts[split] += len(episode["transitions"])
    for row in iter_branch_rows(branch_paths):
        split = split_for_seed(int(row["seed"]))
        branch_counts[split] += 1
        counts[split] += 1
    tensors: dict[str, dict[str, torch.Tensor]] = {}
    for split, count in counts.items():
        tensors[split] = {
            "state": torch.zeros((count, MAX_STATE_FEATURES), dtype=torch.int32),
            "action": torch.zeros((count, MAX_ACTION_FEATURES), dtype=torch.int32),
            "numeric": torch.zeros((count, len(MEASUREMENT_SPECS)), dtype=torch.float32),
            "history": torch.zeros((count, MAX_HISTORY_STEPS), dtype=torch.int32),
            "target": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.float32),
            "mask": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.bool),
            "weight": torch.zeros(count, dtype=torch.float32),
        }

    offsets = {"train": 0, "validation": 0}
    state_truncated = 0
    action_truncated = 0
    if not branch_only:
        for episode in iter_episodes(paths):
            split = split_for_seed(int(episode["result"]["seed"]))
            targets, masks = target_rows(episode)
            priority = episode_priority(episode["result"])
            target_tensors = tensors[split]
            history: list[int] = []
            for transition, target, mask in zip(episode["transitions"], targets, masks):
                offset = offsets[split]
                offsets[split] += 1
                observation = transition["observation"]
                state = observation["state_features"]
                selected = observation["actions"][transition["action_index"]]
                action = selected["features"]
                if selected["index"] != transition["action_index"]:
                    raise ValueError("legal action index mismatch in trace")
                state_truncated += len(state) > MAX_STATE_FEATURES
                action_truncated += len(action) > MAX_ACTION_FEATURES
                state = state[:MAX_STATE_FEATURES]
                action = action[:MAX_ACTION_FEATURES]
                target_tensors["state"][offset, : len(state)] = torch.tensor(state)
                target_tensors["action"][offset, : len(action)] = torch.tensor(action)
                target_tensors["numeric"][offset] = torch.tensor(
                    measurement_vector(transition["before"])
                )
                recent_history = history[-MAX_HISTORY_STEPS:]
                if recent_history:
                    target_tensors["history"][offset, -len(recent_history) :] = torch.tensor(
                        recent_history
                    )
                target_tensors["target"][offset] = torch.tensor(target)
                target_tensors["mask"][offset] = torch.tensor(mask)
                target_tensors["weight"][offset] = priority
                history.append(decision_signature(observation, transition["action_index"]))

    search_value_index = TARGET_NAMES.index("search_value")
    for row in iter_branch_rows(branch_paths):
        split = split_for_seed(int(row["seed"]))
        target_tensors = tensors[split]
        offset = offsets[split]
        offsets[split] += 1
        observation = row["observation"]
        state = observation["state_features"][:MAX_STATE_FEATURES]
        selected = observation["actions"][row["action_index"]]
        action = selected["features"][:MAX_ACTION_FEATURES]
        if selected["index"] != row["action_index"]:
            raise ValueError("legal action index mismatch in branch record")
        target_tensors["state"][offset, : len(state)] = torch.tensor(state)
        target_tensors["action"][offset, : len(action)] = torch.tensor(action)
        target_tensors["numeric"][offset] = torch.tensor(
            measurement_vector(row["before"])
        )
        history = row.get("history", [])[-MAX_HISTORY_STEPS:]
        if history:
            target_tensors["history"][offset, -len(history) :] = torch.tensor(history)
        target_tensors["target"][offset, search_value_index] = symlog_scaled(
            float(row["branch_score"]), 1_000.0
        )
        target_tensors["mask"][offset, search_value_index] = True
        target_tensors["weight"][offset] = 3.0

    prepared = {
        "signature": signature,
        "tensors": tensors,
        "episodes": episodes,
        "branch_counts": branch_counts,
        "counts": counts,
        "state_truncated": state_truncated,
        "action_truncated": action_truncated,
    }
    cache.parent.mkdir(parents=True, exist_ok=True)
    torch.save(prepared, cache)
    log(
        f"prepared {sum(counts.values())} decisions from {sum(episodes.values())} "
        f"teacher-free episodes in {time.monotonic() - started:.1f}s; "
        f"split={counts}; state_truncated={state_truncated}; "
        f"action_truncated={action_truncated}; branches={branch_counts}"
    )
    return prepared


class GatedBlock(nn.Module):
    def __init__(self, hidden_size: int, expansion: int):
        super().__init__()
        inner = hidden_size * expansion
        self.norm = nn.RMSNorm(hidden_size)
        self.gate = nn.Linear(hidden_size, inner, bias=False)
        self.value = nn.Linear(hidden_size, inner, bias=False)
        self.down = nn.Linear(inner, hidden_size, bias=False)

    def forward(self, hidden: torch.Tensor) -> torch.Tensor:
        normalized = self.norm(hidden)
        update = self.down(F.silu(self.gate(normalized)) * self.value(normalized))
        return hidden + update


class SelectiveSsmMemory(nn.Module):
    """A compact Mamba-style selective diagonal state-space memory."""

    def __init__(self, hidden_size: int):
        super().__init__()
        self.norm = nn.RMSNorm(hidden_size)
        self.select = nn.Linear(hidden_size, hidden_size * 3, bias=False)
        self.log_decay = nn.Parameter(torch.zeros(hidden_size))
        self.output = nn.Linear(hidden_size, hidden_size, bias=False)

    def forward(self, embedded: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        selected = self.select(self.norm(embedded)).float()
        delta, candidate, gate = selected.chunk(3, dim=-1)
        rate = F.softplus(self.log_decay.float()).view(1, 1, -1)
        decay = torch.exp(-rate * (0.05 + 0.95 * torch.sigmoid(delta)))
        update = (1.0 - decay) * torch.tanh(candidate)
        visible = mask.unsqueeze(-1)
        decay = torch.where(visible, decay, torch.ones_like(decay))
        update = torch.where(visible, update, torch.zeros_like(update))

        # Vectorized scan for h_t = a_t h_(t-1) + b_t. Sixty-four
        # decisions remain numerically safe in FP32 and avoid a Python loop.
        prefix = torch.cumprod(decay, dim=1).clamp_min(1e-20)
        state = prefix * torch.cumsum(update / prefix, dim=1)
        memory = state[:, -1] * torch.sigmoid(gate[:, -1])
        return self.output(memory.to(embedded.dtype))


class SelfPlayHrm(nn.Module):
    """Action-conditioned HRM core with multi-horizon world-value heads."""

    def __init__(self, config: dict[str, Any]):
        super().__init__()
        hidden = int(config["hidden_size"])
        expansion = int(config["expansion"])
        self.h_cycles = int(config["h_cycles"])
        self.l_cycles = int(config["l_cycles"])
        self.segments = int(config["segments"])
        self.architecture = str(config.get("architecture", "hrm"))
        self.numeric_size = int(config.get("numeric_measurements", 0))
        self.embedding = nn.Embedding(FEATURE_BUCKETS + 1, hidden, padding_idx=0)
        self.state_projection = nn.Sequential(
            nn.RMSNorm(hidden),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.action_projection = nn.Sequential(
            nn.RMSNorm(hidden),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.numeric_projection = (
            nn.Sequential(
                nn.RMSNorm(self.numeric_size),
                nn.Linear(self.numeric_size, hidden),
                nn.SiLU(),
                nn.Linear(hidden, hidden, bias=False),
            )
            if self.numeric_size
            else None
        )
        self.history_memory = (
            SelectiveSsmMemory(hidden) if self.architecture == "hrm_ssm" else None
        )
        self.low = GatedBlock(hidden, expansion)
        self.high = GatedBlock(hidden, expansion)
        output_count = len(config.get("target_names", TARGET_NAMES))
        output_width = hidden * (4 if self.history_memory is not None else 3)
        self.output = nn.Sequential(
            nn.RMSNorm(output_width),
            nn.Linear(output_width, hidden * 2),
            nn.SiLU(),
            nn.Linear(hidden * 2, output_count),
        )

    def pool(self, ids: torch.Tensor) -> torch.Tensor:
        embedded = self.embedding(ids)
        mask = ids.ne(0).unsqueeze(-1)
        return (embedded * mask).sum(1) / mask.sum(1).clamp_min(1)

    def forward(
        self,
        state_ids: torch.Tensor,
        action_ids: torch.Tensor,
        numeric: torch.Tensor | None = None,
        history_ids: torch.Tensor | None = None,
    ) -> torch.Tensor:
        state = self.state_projection(self.pool(state_ids))
        action = self.action_projection(self.pool(action_ids))
        if self.numeric_projection is not None:
            if numeric is None:
                raise ValueError("numeric measurements are required by this checkpoint")
            state = state + self.numeric_projection(numeric)
        memory = torch.zeros_like(state)
        if self.history_memory is not None:
            if history_ids is None:
                raise ValueError("history IDs are required by the HRM-SSM checkpoint")
            memory = self.history_memory(self.embedding(history_ids), history_ids.ne(0))
        problem = state + action + memory
        high = torch.zeros_like(problem)
        low = torch.zeros_like(problem)
        for segment in range(self.segments):
            for _ in range(self.h_cycles):
                for _ in range(self.l_cycles):
                    low = self.low(low + high + problem)
                high = self.high(high + low)
            if segment + 1 < self.segments:
                # Preserve HRM's bounded-memory, one-step gradient approximation.
                high = high.detach()
                low = low.detach()
        parts = (high, state, action, memory) if self.history_memory is not None else (high, state, action)
        return self.output(torch.cat(parts, dim=-1))


def masked_loss(prediction: torch.Tensor, target: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
    component = F.smooth_l1_loss(prediction, target, reduction="none")
    return (component * mask).sum() / mask.sum().clamp_min(1)


@torch.inference_mode()
def evaluate(model: nn.Module, loader: DataLoader, device: torch.device) -> dict[str, Any]:
    model.eval()
    squared = torch.zeros(len(TARGET_NAMES), device=device)
    absolute = torch.zeros(len(TARGET_NAMES), device=device)
    counts = torch.zeros(len(TARGET_NAMES), device=device)
    for state, action, numeric, history, target, mask in loader:
        state = state.to(device=device, dtype=torch.long, non_blocking=True)
        action = action.to(device=device, dtype=torch.long, non_blocking=True)
        numeric = numeric.to(device=device, non_blocking=True)
        history = history.to(device=device, dtype=torch.long, non_blocking=True)
        target = target.to(device=device, non_blocking=True)
        mask = mask.to(device=device, non_blocking=True)
        error = model(state, action, numeric, history) - target
        squared += (error.square() * mask).sum(0)
        absolute += (error.abs() * mask).sum(0)
        counts += mask.sum(0)
    rmse = (squared / counts.clamp_min(1)).sqrt().cpu().tolist()
    mae = (absolute / counts.clamp_min(1)).cpu().tolist()
    return {
        name: {"rmse": rmse[index], "mae": mae[index], "count": int(counts[index])}
        for index, name in enumerate(TARGET_NAMES)
    }


def train(args: argparse.Namespace) -> None:
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)
        torch.set_float32_matmul_precision("high")
    device = torch.device(
        "cuda" if args.device == "auto" and torch.cuda.is_available() else args.device
    )
    prepared = prepare(
        args.dataset,
        args.branch_dataset,
        args.cache,
        branch_only=args.search_head_only,
    )
    train_tensors = prepared["tensors"]["train"]
    validation_tensors = prepared["tensors"]["validation"]
    train_set = TensorDataset(
        train_tensors["state"],
        train_tensors["action"],
        train_tensors["numeric"],
        train_tensors["history"],
        train_tensors["target"],
        train_tensors["mask"],
    )
    validation_set = TensorDataset(
        validation_tensors["state"],
        validation_tensors["action"],
        validation_tensors["numeric"],
        validation_tensors["history"],
        validation_tensors["target"],
        validation_tensors["mask"],
    )
    sampling_weight = 1.0 + args.frontier_priority_scale * (
        train_tensors["weight"].double() - 1.0
    )
    sampler = torch.utils.data.WeightedRandomSampler(
        sampling_weight,
        num_samples=len(train_set),
        replacement=True,
    )
    train_loader = DataLoader(
        train_set,
        batch_size=args.batch_size,
        sampler=sampler,
        num_workers=2,
        pin_memory=device.type == "cuda",
        persistent_workers=True,
    )
    validation_loader = DataLoader(
        validation_set,
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=2,
        pin_memory=device.type == "cuda",
        persistent_workers=True,
    )
    initial_checkpoint = None
    if args.init_checkpoint is not None:
        initial_checkpoint = torch.load(
            args.init_checkpoint, map_location="cpu", weights_only=False
        )
        if initial_checkpoint.get("teacher") is not None:
            raise ValueError("initial checkpoint is not teacher-free")
        if tuple(initial_checkpoint["target_names"]) != TARGET_NAMES:
            raise ValueError("initial checkpoint target heads do not match")
        config = dict(initial_checkpoint["config"])
        config["target_names"] = TARGET_NAMES
    else:
        config = {
            **DEFAULTS,
            "hidden_size": args.hidden_size,
            "batch_size": args.batch_size,
            "target_names": TARGET_NAMES,
            "numeric_measurements": len(MEASUREMENT_SPECS),
            "architecture": args.architecture,
        }
    model = SelfPlayHrm(config).to(device)
    if initial_checkpoint is not None:
        model.load_state_dict(initial_checkpoint["model"])
    if args.search_head_only:
        for parameter in model.parameters():
            parameter.requires_grad_(False)
        for parameter in model.output[-1].parameters():
            parameter.requires_grad_(True)
    trainable_parameters = [
        parameter for parameter in model.parameters() if parameter.requires_grad
    ]
    optimizer = torch.optim.AdamW(
        trainable_parameters,
        lr=args.learning_rate,
        weight_decay=args.weight_decay,
        fused=device.type == "cuda",
    )
    amp_dtype = (
        torch.bfloat16
        if device.type == "cuda" and torch.cuda.is_bf16_supported()
        else torch.float16
    )
    scaler = torch.amp.GradScaler(
        "cuda", enabled=device.type == "cuda" and amp_dtype == torch.float16
    )
    parameters = sum(parameter.numel() for parameter in model.parameters())
    trainable = sum(parameter.numel() for parameter in trainable_parameters)
    log(
        f"training teacher-free SelfPlayHrm on {device}: parameters={parameters:,}, "
        f"trainable={trainable:,}, "
        f"train={len(train_set)}, validation={len(validation_set)}, seconds={args.seconds}"
    )
    started = time.monotonic()
    updates = 0
    epochs = 0
    recent_loss = 0.0
    model.train()
    while time.monotonic() - started < args.seconds:
        epochs += 1
        for state, action, numeric, history, target, mask in train_loader:
            if time.monotonic() - started >= args.seconds:
                break
            state = state.to(device=device, dtype=torch.long, non_blocking=True)
            action = action.to(device=device, dtype=torch.long, non_blocking=True)
            numeric = numeric.to(device=device, non_blocking=True)
            history = history.to(device=device, dtype=torch.long, non_blocking=True)
            target = target.to(device=device, non_blocking=True)
            mask = mask.to(device=device, non_blocking=True)
            optimizer.zero_grad(set_to_none=True)
            with torch.autocast(
                device_type=device.type,
                dtype=amp_dtype,
                enabled=device.type == "cuda",
            ):
                prediction = model(state, action, numeric, history)
                loss = masked_loss(prediction, target, mask)
            if not torch.isfinite(loss):
                raise RuntimeError(
                    f"non-finite loss at epoch={epochs} update={updates}; checkpoint rejected"
                )
            scaler.scale(loss).backward()
            scaler.unscale_(optimizer)
            gradient_norm = torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
            if not torch.isfinite(gradient_norm):
                raise RuntimeError(
                    f"non-finite gradient at epoch={epochs} update={updates}; checkpoint rejected"
                )
            scaler.step(optimizer)
            scaler.update()
            updates += 1
            recent_loss = float(loss.detach())
        if epochs == 1 or epochs % 5 == 0:
            log(
                f"epoch={epochs} updates={updates} loss={recent_loss:.5f} "
                f"elapsed={time.monotonic() - started:.1f}s"
            )

    metrics = evaluate(model, validation_loader, device)
    elapsed = time.monotonic() - started
    args.output.parent.mkdir(parents=True, exist_ok=True)
    checkpoint = {
        "format": "sts-selfplay-hrm-v1",
        "model": model.state_dict(),
        "config": config,
        "feature_buckets": FEATURE_BUCKETS,
        "max_state_features": MAX_STATE_FEATURES,
        "max_action_features": MAX_ACTION_FEATURES,
        "max_history_steps": MAX_HISTORY_STEPS,
        "measurement_specs": MEASUREMENT_SPECS,
        "target_names": TARGET_NAMES,
        "target_scales": TARGET_SCALES,
        "dataset_signature": prepared["signature"],
        "teacher": None,
        "initialized_from": (
            str(args.init_checkpoint) if args.init_checkpoint is not None else None
        ),
        "search_head_only": args.search_head_only,
        "updates": updates,
        "epochs": epochs,
    }
    torch.save(checkpoint, args.output)
    metrics_path = args.output.with_suffix(".metrics.json")
    metrics_path.write_text(
        json.dumps(
            {
                "checkpoint": str(args.output),
                "device": str(device),
                "amp_dtype": str(amp_dtype),
                "parameters": parameters,
                "trainable_parameters": trainable,
                "seconds": elapsed,
                "updates": updates,
                "epochs": epochs,
                "episodes": prepared["episodes"],
                "branch_decisions": prepared["branch_counts"],
                "decisions": prepared["counts"],
                "teacher": None,
                "initialized_from": (
                    str(args.init_checkpoint) if args.init_checkpoint is not None else None
                ),
                "search_head_only": args.search_head_only,
                "architecture": args.architecture,
                "replay_priority": "self_discovered_floor_frontier_v1",
                "train_priority_mean": float(train_tensors["weight"].mean()),
                "frontier_priority_scale": args.frontier_priority_scale,
                "validation": metrics,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    log(f"saved {args.output} and {metrics_path}")
    for name, values in metrics.items():
        log(f"validation {name}: rmse={values['rmse']:.4f} mae={values['mae']:.4f}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--dataset",
        type=Path,
        action="append",
        help="trajectory JSONL/XZ; repeat to mix self-play generations",
    )
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-random-prepared.pt"),
    )
    parser.add_argument(
        "--branch-dataset",
        type=Path,
        action="append",
        help="teacher-free exact branch-value JSONL/XZ; repeat to mix searches",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-selfplay-hrm.pt"),
    )
    parser.add_argument("--seconds", type=float, default=DEFAULTS["seconds"])
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--hidden-size", type=int, default=DEFAULTS["hidden_size"])
    parser.add_argument(
        "--architecture", choices=("hrm", "hrm_ssm"), default="hrm_ssm"
    )
    parser.add_argument("--batch-size", type=int, default=DEFAULTS["batch_size"])
    parser.add_argument("--learning-rate", type=float, default=DEFAULTS["learning_rate"])
    parser.add_argument("--weight-decay", type=float, default=DEFAULTS["weight_decay"])
    parser.add_argument("--frontier-priority-scale", type=float, default=1.0)
    parser.add_argument("--init-checkpoint", type=Path)
    parser.add_argument(
        "--search-head-only",
        action="store_true",
        help="train only the search-value output row on branch records",
    )
    parser.add_argument("--seed", type=int, default=20260826)
    args = parser.parse_args()
    if args.dataset is None:
        args.dataset = [Path("artifacts/selfplay/defect-a0-random-traces-1000.jsonl.xz")]
    if args.branch_dataset is None:
        args.branch_dataset = []
    if args.search_head_only and (args.init_checkpoint is None or not args.branch_dataset):
        parser.error("search-head-only requires an initial checkpoint and branch data")
    if (
        args.seconds <= 0
        or args.hidden_size <= 0
        or args.batch_size <= 0
        or args.frontier_priority_scale < 0
    ):
        parser.error("sizes must be positive and frontier priority cannot be negative")
    if args.device == "auto":
        args.device = "cuda" if torch.cuda.is_available() else "cpu"
    return args


if __name__ == "__main__":
    train(parse_args())
