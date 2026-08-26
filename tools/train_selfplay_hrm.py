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
TARGET_NAMES = (
    "act3_win",
    "max_floor",
    "terminal_margin",
    "combat_margin",
    "hp_delta_1",
    "enemy_hp_delta_1",
    "hp_delta_8",
    "enemy_hp_delta_8",
    "floor_delta_32",
    "gold_delta_32",
    "relic_delta_128",
    "upgrade_delta_128",
)
TARGET_SCALES = (1.0, 52.0, 300.0, 300.0, 100.0, 300.0, 100.0, 300.0, 32.0, 500.0, 10.0, 10.0)
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


def iter_episodes(path: Path) -> Iterable[dict[str, Any]]:
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


def symlog_scaled(value: float, scale: float) -> float:
    if scale == 1.0:
        return value
    return math.copysign(math.log1p(abs(value)), value) / math.log1p(scale)


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
            float(result["max_floor"]),
            float(result["terminal_score"]),
            combat[index][0],
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
                combat[index][1],
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


def prepare(path: Path, cache: Path) -> dict[str, Any]:
    signature = {
        "path": str(path.resolve()),
        "size": path.stat().st_size,
        "mtime_ns": path.stat().st_mtime_ns,
        "feature_buckets": FEATURE_BUCKETS,
        "max_state_features": MAX_STATE_FEATURES,
        "max_action_features": MAX_ACTION_FEATURES,
        "targets": TARGET_NAMES,
        "preprocess_version": 1,
    }
    if cache.exists():
        prepared = torch.load(cache, map_location="cpu", weights_only=False)
        if prepared.get("signature") == signature:
            log(f"loaded prepared tensors from {cache}")
            return prepared

    started = time.monotonic()
    counts = {"train": 0, "validation": 0}
    episodes = {"train": 0, "validation": 0}
    for episode in iter_episodes(path):
        split = split_for_seed(int(episode["result"]["seed"]))
        episodes[split] += 1
        counts[split] += len(episode["transitions"])
    tensors: dict[str, dict[str, torch.Tensor]] = {}
    for split, count in counts.items():
        tensors[split] = {
            "state": torch.zeros((count, MAX_STATE_FEATURES), dtype=torch.int32),
            "action": torch.zeros((count, MAX_ACTION_FEATURES), dtype=torch.int32),
            "target": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.float32),
            "mask": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.bool),
        }

    offsets = {"train": 0, "validation": 0}
    state_truncated = 0
    action_truncated = 0
    for episode in iter_episodes(path):
        split = split_for_seed(int(episode["result"]["seed"]))
        targets, masks = target_rows(episode)
        target_tensors = tensors[split]
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
            target_tensors["target"][offset] = torch.tensor(target)
            target_tensors["mask"][offset] = torch.tensor(mask)

    prepared = {
        "signature": signature,
        "tensors": tensors,
        "episodes": episodes,
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
        f"action_truncated={action_truncated}"
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


class SelfPlayHrm(nn.Module):
    """Action-conditioned HRM core with multi-horizon world-value heads."""

    def __init__(self, config: dict[str, Any]):
        super().__init__()
        hidden = int(config["hidden_size"])
        expansion = int(config["expansion"])
        self.h_cycles = int(config["h_cycles"])
        self.l_cycles = int(config["l_cycles"])
        self.segments = int(config["segments"])
        self.embedding = nn.Embedding(FEATURE_BUCKETS + 1, hidden, padding_idx=0)
        self.state_projection = nn.Sequential(
            nn.RMSNorm(hidden),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.action_projection = nn.Sequential(
            nn.RMSNorm(hidden),
            nn.Linear(hidden, hidden, bias=False),
        )
        self.low = GatedBlock(hidden, expansion)
        self.high = GatedBlock(hidden, expansion)
        self.output = nn.Sequential(
            nn.RMSNorm(hidden * 3),
            nn.Linear(hidden * 3, hidden * 2),
            nn.SiLU(),
            nn.Linear(hidden * 2, len(TARGET_NAMES)),
        )

    def pool(self, ids: torch.Tensor) -> torch.Tensor:
        embedded = self.embedding(ids)
        mask = ids.ne(0).unsqueeze(-1)
        return (embedded * mask).sum(1) / mask.sum(1).clamp_min(1)

    def forward(self, state_ids: torch.Tensor, action_ids: torch.Tensor) -> torch.Tensor:
        state = self.state_projection(self.pool(state_ids))
        action = self.action_projection(self.pool(action_ids))
        problem = state + action
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
        return self.output(torch.cat((high, state, action), dim=-1))


def masked_loss(prediction: torch.Tensor, target: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
    component = F.smooth_l1_loss(prediction, target, reduction="none")
    return (component * mask).sum() / mask.sum().clamp_min(1)


@torch.inference_mode()
def evaluate(model: nn.Module, loader: DataLoader, device: torch.device) -> dict[str, Any]:
    model.eval()
    squared = torch.zeros(len(TARGET_NAMES), device=device)
    absolute = torch.zeros(len(TARGET_NAMES), device=device)
    counts = torch.zeros(len(TARGET_NAMES), device=device)
    for state, action, target, mask in loader:
        state = state.to(device=device, dtype=torch.long, non_blocking=True)
        action = action.to(device=device, dtype=torch.long, non_blocking=True)
        target = target.to(device=device, non_blocking=True)
        mask = mask.to(device=device, non_blocking=True)
        error = model(state, action) - target
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
    prepared = prepare(args.dataset, args.cache)
    train_tensors = prepared["tensors"]["train"]
    validation_tensors = prepared["tensors"]["validation"]
    train_set = TensorDataset(
        train_tensors["state"],
        train_tensors["action"],
        train_tensors["target"],
        train_tensors["mask"],
    )
    validation_set = TensorDataset(
        validation_tensors["state"],
        validation_tensors["action"],
        validation_tensors["target"],
        validation_tensors["mask"],
    )
    train_loader = DataLoader(
        train_set,
        batch_size=args.batch_size,
        shuffle=True,
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
    config = {
        **DEFAULTS,
        "hidden_size": args.hidden_size,
        "batch_size": args.batch_size,
    }
    model = SelfPlayHrm(config).to(device)
    optimizer = torch.optim.AdamW(
        model.parameters(),
        lr=args.learning_rate,
        weight_decay=args.weight_decay,
        fused=device.type == "cuda",
    )
    scaler = torch.amp.GradScaler("cuda", enabled=device.type == "cuda")
    parameters = sum(parameter.numel() for parameter in model.parameters())
    log(
        f"training teacher-free SelfPlayHrm on {device}: parameters={parameters:,}, "
        f"train={len(train_set)}, validation={len(validation_set)}, seconds={args.seconds}"
    )
    started = time.monotonic()
    updates = 0
    epochs = 0
    recent_loss = 0.0
    model.train()
    while time.monotonic() - started < args.seconds:
        epochs += 1
        for state, action, target, mask in train_loader:
            if time.monotonic() - started >= args.seconds:
                break
            state = state.to(device=device, dtype=torch.long, non_blocking=True)
            action = action.to(device=device, dtype=torch.long, non_blocking=True)
            target = target.to(device=device, non_blocking=True)
            mask = mask.to(device=device, non_blocking=True)
            optimizer.zero_grad(set_to_none=True)
            with torch.autocast(device_type=device.type, dtype=torch.float16, enabled=device.type == "cuda"):
                prediction = model(state, action)
                loss = masked_loss(prediction, target, mask)
            scaler.scale(loss).backward()
            scaler.unscale_(optimizer)
            torch.nn.utils.clip_grad_norm_(model.parameters(), 1.0)
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
        "target_names": TARGET_NAMES,
        "target_scales": TARGET_SCALES,
        "dataset_signature": prepared["signature"],
        "teacher": None,
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
                "parameters": parameters,
                "seconds": elapsed,
                "updates": updates,
                "epochs": epochs,
                "episodes": prepared["episodes"],
                "decisions": prepared["counts"],
                "teacher": None,
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
        default=Path("artifacts/selfplay/defect-a0-random-traces-1000.jsonl.xz"),
    )
    parser.add_argument(
        "--cache",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-random-prepared.pt"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("artifacts/selfplay/defect-a0-selfplay-hrm.pt"),
    )
    parser.add_argument("--seconds", type=float, default=DEFAULTS["seconds"])
    parser.add_argument("--device", choices=("auto", "cuda", "cpu"), default="auto")
    parser.add_argument("--hidden-size", type=int, default=DEFAULTS["hidden_size"])
    parser.add_argument("--batch-size", type=int, default=DEFAULTS["batch_size"])
    parser.add_argument("--learning-rate", type=float, default=DEFAULTS["learning_rate"])
    parser.add_argument("--weight-decay", type=float, default=DEFAULTS["weight_decay"])
    parser.add_argument("--seed", type=int, default=20260826)
    args = parser.parse_args()
    if args.seconds <= 0 or args.hidden_size <= 0 or args.batch_size <= 0:
        parser.error("seconds, hidden size, and batch size must be positive")
    if args.device == "auto":
        args.device = "cuda" if torch.cuda.is_available() else "cpu"
    return args


if __name__ == "__main__":
    train(parse_args())
