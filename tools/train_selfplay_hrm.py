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
MAX_INVENTORY_IDENTITIES = 192
MAX_CANDIDATE_IDENTITIES = 16
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
    ("deck_total_cost", 100.0),
    ("deck_attack_cards", 50.0),
    ("deck_skill_cards", 50.0),
    ("deck_power_cards", 20.0),
    ("deck_exhaust_cards", 20.0),
    ("deck_orb_cards", 30.0),
    ("deck_card_access", 20.0),
    ("deck_energy_cards", 20.0),
    ("deck_focus_cards", 10.0),
    # Append-only: old checkpoints retain their original numeric prefix.
    ("ascension", 20.0),
)
ACTION_PARAMETER_SPECS = (
    ("known", 1.0),
    ("hp_delta", 100.0),
    ("max_hp_delta", 100.0),
    ("gold_delta", 500.0),
    ("deck_size_delta", 10.0),
    ("upgraded_cards_delta", 10.0),
    ("relic_delta", 5.0),
    ("potion_delta", 5.0),
    ("hp_delta_current_fraction", 1.0),
    ("max_hp_delta_fraction", 1.0),
    ("gold_delta_current_fraction", 1.0),
    ("lethal_hp_loss", 1.0),
)
DECK_AUXILIARY_TARGET_SPECS = (
    ("deck_cost_delta_128", "deck_total_cost", 100.0),
    ("deck_attack_delta_128", "deck_attack_cards", 50.0),
    ("deck_skill_delta_128", "deck_skill_cards", 50.0),
    ("deck_power_delta_128", "deck_power_cards", 20.0),
    ("deck_exhaust_delta_128", "deck_exhaust_cards", 20.0),
    ("deck_orb_delta_128", "deck_orb_cards", 30.0),
    ("deck_access_delta_128", "deck_card_access", 20.0),
    ("deck_energy_delta_128", "deck_energy_cards", 20.0),
    ("deck_focus_delta_128", "deck_focus_cards", 10.0),
)
MAX_RECORDED_FLOOR = 50
FLOOR_SURVIVAL_NAMES = tuple(
    f"reach_floor_{floor}" for floor in range(1, MAX_RECORDED_FLOOR + 1)
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
    *(target for target, _, _ in DECK_AUXILIARY_TARGET_SPECS),
    *FLOOR_SURVIVAL_NAMES,
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
    *(scale for _, _, scale in DECK_AUXILIARY_TARGET_SPECS),
    *(1.0 for _ in FLOOR_SURVIVAL_NAMES),
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


def measurement_vector(
    measurements: dict[str, Any], limit: int | None = None
) -> list[float]:
    specs = MEASUREMENT_SPECS if limit is None else MEASUREMENT_SPECS[:limit]
    return [
        # New measurements remain distinguishable from a real zero when old
        # teacher-free generations predate that field.
        symlog_scaled(float(measurements.get(name, -1.0)), scale)
        for name, scale in specs
    ]


def action_parameter_vector(
    action: dict[str, Any],
    measurements: dict[str, Any],
    limit: int | None = None,
) -> list[float]:
    """Factor deterministic choice effects into reusable numeric channels."""
    parameters = action.get("parameters") or {}
    known = bool(parameters.get("known", False))
    values = {
        "known": float(known),
        "hp_delta": float(parameters.get("hp_delta", 0.0)),
        "max_hp_delta": float(parameters.get("max_hp_delta", 0.0)),
        "gold_delta": float(parameters.get("gold_delta", 0.0)),
        "deck_size_delta": float(parameters.get("deck_size_delta", 0.0)),
        "upgraded_cards_delta": float(
            parameters.get("upgraded_cards_delta", 0.0)
        ),
        "relic_delta": float(parameters.get("relic_delta", 0.0)),
        "potion_delta": float(parameters.get("potion_delta", 0.0)),
    }
    if known:
        hp = max(float(measurements.get("hp", 0.0)), 1.0)
        max_hp = max(float(measurements.get("max_hp", 0.0)), 1.0)
        gold = max(float(measurements.get("gold", 0.0)), 1.0)
        values["hp_delta_current_fraction"] = max(
            -2.0, min(2.0, values["hp_delta"] / hp)
        )
        values["max_hp_delta_fraction"] = max(
            -2.0, min(2.0, values["max_hp_delta"] / max_hp)
        )
        values["gold_delta_current_fraction"] = max(
            -2.0, min(2.0, values["gold_delta"] / gold)
        )
        values["lethal_hp_loss"] = float(
            values["hp_delta"] < 0.0
            and float(measurements.get("hp", 0.0)) + values["hp_delta"] <= 0.0
        )
    else:
        values.update(
            hp_delta_current_fraction=0.0,
            max_hp_delta_fraction=0.0,
            gold_delta_current_fraction=0.0,
            lethal_hp_loss=0.0,
        )
    specs = (
        ACTION_PARAMETER_SPECS
        if limit is None
        else ACTION_PARAMETER_SPECS[:limit]
    )
    return [symlog_scaled(values[name], scale) for name, scale in specs]


def decision_signature(observation: dict[str, Any], action_index: int) -> int:
    """Fold one visible state/action pair into a stable nonzero history token."""
    value = 0xCBF29CE484222325
    action = observation["actions"][action_index]
    for feature in observation["state_features"][:4] + action["features"]:
        value ^= int(feature)
        value = (value * 0x100000001B3) & ((1 << 64) - 1)
    return value % FEATURE_BUCKETS + 1


def feature_token_id(token: str) -> int:
    """Match the engine's stable FNV-1a feature hashing."""
    value = 0xCBF29CE484222325
    for byte in token.encode():
        value ^= byte
        value = (value * 0x100000001B3) & ((1 << 64) - 1)
    return value % FEATURE_BUCKETS + 1


CHOICE_NONE_ID = feature_token_id("IDENTITY:CHOICE_NONE")


def candidate_identity_features(
    observation: dict[str, Any], selected: dict[str, Any], in_combat: bool
) -> list[int]:
    """Give every action in an inventory-choice menu a critic identity.

    Older traces predate the engine-side marker on Skip/leave actions. Adding
    the stable marker while preprocessing keeps those alternatives explicitly
    conditioned on the current deck and relics.
    """
    if in_combat:
        return []
    identities = list(selected.get("candidate_identities", []))
    if not identities and any(
        action.get("candidate_identities") for action in observation["actions"]
    ):
        identities.append(CHOICE_NONE_ID)
    return identities


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
            *(
                float(h128[measurement] - before[measurement])
                if measurement in before and measurement in h128
                else 0.0
                for _, measurement, _ in DECK_AUXILIARY_TARGET_SPECS
            ),
            *(float(max_floor >= floor) for floor in range(1, MAX_RECORDED_FLOOR + 1)),
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
                *(
                    measurement in before and measurement in h128
                    for _, measurement, _ in DECK_AUXILIARY_TARGET_SPECS
                ),
                *(True for _ in FLOOR_SURVIVAL_NAMES),
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
        "max_inventory_identities": MAX_INVENTORY_IDENTITIES,
        "max_candidate_identities": MAX_CANDIDATE_IDENTITIES,
        "max_history_steps": MAX_HISTORY_STEPS,
        "measurement_specs": MEASUREMENT_SPECS,
        "action_parameter_specs": ACTION_PARAMETER_SPECS,
        "targets": TARGET_NAMES,
        "branch_only": branch_only,
        "preprocess_version": 8,
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
            "inventory": torch.zeros(
                (count, MAX_INVENTORY_IDENTITIES), dtype=torch.int32
            ),
            "candidate": torch.zeros(
                (count, MAX_CANDIDATE_IDENTITIES), dtype=torch.int32
            ),
            "numeric": torch.zeros((count, len(MEASUREMENT_SPECS)), dtype=torch.float32),
            "action_numeric": torch.zeros(
                (count, len(ACTION_PARAMETER_SPECS)), dtype=torch.float32
            ),
            "history": torch.zeros((count, MAX_HISTORY_STEPS), dtype=torch.int32),
            "target": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.float32),
            "mask": torch.zeros((count, len(TARGET_NAMES)), dtype=torch.bool),
            "weight": torch.zeros(count, dtype=torch.float32),
        }

    offsets = {"train": 0, "validation": 0}
    state_truncated = 0
    action_truncated = 0
    inventory_truncated = 0
    candidate_truncated = 0
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
                inventory = observation.get("inventory_identities", [])
                candidate = candidate_identity_features(
                    observation,
                    selected,
                    transition["before"]["enemy_max_hp"] > 0,
                )
                if selected["index"] != transition["action_index"]:
                    raise ValueError("legal action index mismatch in trace")
                state_truncated += len(state) > MAX_STATE_FEATURES
                action_truncated += len(action) > MAX_ACTION_FEATURES
                inventory_truncated += len(inventory) > MAX_INVENTORY_IDENTITIES
                candidate_truncated += len(candidate) > MAX_CANDIDATE_IDENTITIES
                state = state[:MAX_STATE_FEATURES]
                action = action[:MAX_ACTION_FEATURES]
                inventory = inventory[:MAX_INVENTORY_IDENTITIES]
                candidate = candidate[:MAX_CANDIDATE_IDENTITIES]
                target_tensors["state"][offset, : len(state)] = torch.tensor(state)
                target_tensors["action"][offset, : len(action)] = torch.tensor(action)
                target_tensors["inventory"][offset, : len(inventory)] = torch.tensor(
                    inventory
                )
                target_tensors["candidate"][offset, : len(candidate)] = torch.tensor(
                    candidate
                )
                target_tensors["numeric"][offset] = torch.tensor(
                    measurement_vector(transition["before"])
                )
                target_tensors["action_numeric"][offset] = torch.tensor(
                    action_parameter_vector(selected, transition["before"])
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
        inventory = observation.get("inventory_identities", [])
        candidate = candidate_identity_features(
            observation,
            selected,
            row["before"]["enemy_max_hp"] > 0,
        )
        inventory = inventory[:MAX_INVENTORY_IDENTITIES]
        candidate = candidate[:MAX_CANDIDATE_IDENTITIES]
        if selected["index"] != row["action_index"]:
            raise ValueError("legal action index mismatch in branch record")
        target_tensors["state"][offset, : len(state)] = torch.tensor(state)
        target_tensors["action"][offset, : len(action)] = torch.tensor(action)
        target_tensors["inventory"][offset, : len(inventory)] = torch.tensor(inventory)
        target_tensors["candidate"][offset, : len(candidate)] = torch.tensor(candidate)
        target_tensors["numeric"][offset] = torch.tensor(
            measurement_vector(row["before"])
        )
        target_tensors["action_numeric"][offset] = torch.tensor(
            action_parameter_vector(selected, row["before"])
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
        "inventory_truncated": inventory_truncated,
        "candidate_truncated": candidate_truncated,
    }
    cache.parent.mkdir(parents=True, exist_ok=True)
    torch.save(prepared, cache)
    log(
        f"prepared {sum(counts.values())} decisions from {sum(episodes.values())} "
        f"teacher-free episodes in {time.monotonic() - started:.1f}s; "
        f"split={counts}; state_truncated={state_truncated}; "
        f"action_truncated={action_truncated}; "
        f"inventory_truncated={inventory_truncated}; "
        f"candidate_truncated={candidate_truncated}; branches={branch_counts}"
    )
    return prepared


def branch_preference_pairs(
    paths: list[Path], minimum_gap: float = 1.0
) -> dict[str, torch.Tensor]:
    """Build within-menu winner/loser indices for counterfactual ranking."""
    offsets = {"train": 0, "validation": 0}
    menus: dict[str, dict[tuple[int, int, int], list[tuple[int, float]]]] = {
        "train": {},
        "validation": {},
    }
    for source, path in enumerate(paths):
        for row in iter_branch_rows([path]):
            split = split_for_seed(int(row["seed"]))
            index = offsets[split]
            offsets[split] += 1
            key = (source, int(row["seed"]), int(row["step"]))
            menus[split].setdefault(key, []).append(
                (index, float(row["branch_score"]))
            )
    result: dict[str, torch.Tensor] = {}
    for split, grouped in menus.items():
        pairs: list[tuple[int, int]] = []
        for actions in grouped.values():
            for left in range(len(actions)):
                for right in range(left + 1, len(actions)):
                    left_index, left_score = actions[left]
                    right_index, right_score = actions[right]
                    if abs(left_score - right_score) < minimum_gap:
                        continue
                    if left_score > right_score:
                        pairs.append((left_index, right_index))
                    else:
                        pairs.append((right_index, left_index))
        result[split] = torch.tensor(pairs, dtype=torch.long).reshape(-1, 2)
    return result


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
        # Span short and long recurrent time scales at initialization. A
        # uniform zero decay forgets almost every early token in a 256-token
        # state before learning has a chance to specialize the channels.
        self.log_decay = nn.Parameter(torch.linspace(-4.0, 0.0, hidden_size))
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

        # Scan h_t = a_t h_(t-1) + b_t in bounded blocks. A single cumprod
        # across 256 state tokens underflows during early training; 32-token
        # blocks retain parallelism without dividing by vanishing prefixes.
        state = torch.zeros_like(update[:, 0])
        for start in range(0, embedded.shape[1], 32):
            block_decay = decay[:, start : start + 32]
            block_update = update[:, start : start + 32]
            prefix = torch.cumprod(block_decay, dim=1).clamp_min(1e-20)
            block_state = prefix * (
                state.unsqueeze(1) + torch.cumsum(block_update / prefix, dim=1)
            )
            state = block_state[:, -1]
        positions = torch.arange(mask.shape[1], device=mask.device).unsqueeze(0)
        last_visible = positions.masked_fill(~mask, 0).max(dim=1).values
        last_gate = gate.gather(
            1,
            last_visible[:, None, None].expand(-1, 1, gate.shape[-1]),
        ).squeeze(1)
        memory = state * torch.sigmoid(last_gate)
        return self.output(memory.to(embedded.dtype))


class HierarchicalStateSsmMemory(nn.Module):
    """Compress local token groups before the selective state-space scan."""

    def __init__(self, hidden_size: int, group_size: int = 8):
        super().__init__()
        self.group_size = group_size
        self.memory = SelectiveSsmMemory(hidden_size)

    def forward(self, embedded: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        remainder = embedded.shape[1] % self.group_size
        if remainder:
            padding = self.group_size - remainder
            embedded = F.pad(embedded, (0, 0, 0, padding))
            mask = F.pad(mask, (0, padding))
        batch, tokens, hidden = embedded.shape
        grouped_mask = mask.view(batch, tokens // self.group_size, self.group_size)
        grouped = embedded.view(
            batch, tokens // self.group_size, self.group_size, hidden
        )
        grouped = (grouped * grouped_mask.unsqueeze(-1)).sum(2) / grouped_mask.sum(
            2
        ).unsqueeze(-1).clamp_min(1)
        return self.memory(grouped, grouped_mask.any(2))


class ActionConditionedStateAttention(nn.Module):
    """One-query cross-attention from the candidate action into visible state tokens."""

    def __init__(self, hidden_size: int, heads: int = 4):
        super().__init__()
        if hidden_size % heads:
            raise ValueError("attention hidden size must be divisible by its head count")
        self.heads = heads
        self.head_size = hidden_size // heads
        self.state_norm = nn.RMSNorm(hidden_size)
        self.action_norm = nn.RMSNorm(hidden_size)
        self.query = nn.Linear(hidden_size, hidden_size, bias=False)
        self.key = nn.Linear(hidden_size, hidden_size, bias=False)
        self.value = nn.Linear(hidden_size, hidden_size, bias=False)
        self.output = nn.Linear(hidden_size, hidden_size, bias=False)

    def forward(
        self,
        state_embedded: torch.Tensor,
        state_mask: torch.Tensor,
        action: torch.Tensor,
    ) -> torch.Tensor:
        batch, tokens, hidden = state_embedded.shape
        query = self.query(self.action_norm(action)).view(
            batch, self.heads, self.head_size
        )
        normalized = self.state_norm(state_embedded)
        key = self.key(normalized).view(
            batch, tokens, self.heads, self.head_size
        )
        value = self.value(normalized).view(
            batch, tokens, self.heads, self.head_size
        )
        scores = torch.einsum("bhd,bthd->bht", query, key) / math.sqrt(
            self.head_size
        )
        scores = scores.masked_fill(~state_mask.unsqueeze(1), -torch.inf)
        weights = F.softmax(scores.float(), dim=-1).to(value.dtype)
        attended = torch.einsum("bht,bthd->bhd", weights, value)
        return self.output(attended.reshape(batch, hidden))


class CandidateInventoryMemory(nn.Module):
    """A permutation-invariant join between a candidate and owned identities.

    Candidate and inventory IDs share one embedding namespace. Tied cosine
    attention therefore starts with an exact-identity anchor while remaining
    free to learn associations between different cards and relics. Explicit
    overlap statistics preserve copy-count information that softmax attention
    would otherwise discard.
    """

    def __init__(self, hidden_size: int):
        super().__init__()
        self.norm = nn.RMSNorm(hidden_size)
        self.match_projection = nn.Sequential(
            nn.Linear(2, hidden_size),
            nn.SiLU(),
        )
        self.output = nn.Sequential(
            nn.RMSNorm(hidden_size * 5),
            nn.Linear(hidden_size * 5, hidden_size, bias=False),
        )

    def forward(
        self,
        inventory_ids: torch.Tensor,
        candidate_ids: torch.Tensor,
        embedding: nn.Embedding,
    ) -> torch.Tensor:
        batch = inventory_ids.shape[0]
        active = candidate_ids.ne(0).any(1)
        if not bool(active.any()):
            return embedding.weight.new_zeros((batch, embedding.embedding_dim))
        active_indices = active.nonzero(as_tuple=False).squeeze(1)
        inventory_ids = inventory_ids.index_select(0, active_indices)
        candidate_ids = candidate_ids.index_select(0, active_indices)
        inventory_mask = inventory_ids.ne(0)
        candidate_mask = candidate_ids.ne(0)
        inventory = embedding(inventory_ids)
        candidate = embedding(candidate_ids)
        normalized_inventory = F.normalize(self.norm(inventory).float(), dim=-1)
        normalized_candidate = F.normalize(self.norm(candidate).float(), dim=-1)
        scores = 8.0 * torch.einsum(
            "bch,bih->bci", normalized_candidate, normalized_inventory
        )
        visible_pairs = candidate_mask.unsqueeze(2) & inventory_mask.unsqueeze(1)
        weights = F.softmax(scores.masked_fill(~visible_pairs, -1e4), dim=-1)
        weights = weights * visible_pairs
        weights = weights / weights.sum(-1, keepdim=True).clamp_min(1e-6)
        attended_per_candidate = torch.einsum(
            "bci,bih->bch", weights.to(inventory.dtype), inventory
        )
        candidate_count = candidate_mask.sum(1, keepdim=True).clamp_min(1)
        candidate_summary = (
            candidate * candidate_mask.unsqueeze(-1)
        ).sum(1) / candidate_count
        attended_summary = (
            attended_per_candidate * candidate_mask.unsqueeze(-1)
        ).sum(1) / candidate_count
        inventory_count = inventory_mask.sum(1, keepdim=True).clamp_min(1)
        inventory_summary = (
            inventory * inventory_mask.unsqueeze(-1)
        ).sum(1) / inventory_count

        exact = visible_pairs & candidate_ids.unsqueeze(2).eq(
            inventory_ids.unsqueeze(1)
        )
        exact_count = exact.sum((1, 2)).float().unsqueeze(1)
        match_statistics = torch.cat(
            (
                torch.log1p(exact_count) / math.log(51.0),
                exact_count / inventory_count.float(),
            ),
            dim=1,
        )
        match = self.match_projection(match_statistics).to(candidate.dtype)
        relation = self.output(
            torch.cat(
                (
                    candidate_summary,
                    attended_summary,
                    candidate_summary * attended_summary,
                    inventory_summary,
                    match,
                ),
                dim=-1,
            )
        )
        return relation.new_zeros((batch, relation.shape[-1])).index_copy(
            0, active_indices, relation
        )


class CounterfactualChoiceCritic(nn.Module):
    """An isolated value model for exact action-branch outcomes.

    The policy trunk never receives gradients from the sparse branch labels.
    This critic owns its embeddings and state memory, sees the complete owned
    inventory for every action (including Skip), and adds a tied candidate-to-
    inventory attention join when the action offers a card or relic.
    """

    def __init__(
        self,
        hidden_size: int,
        numeric_size: int,
        action_numeric_size: int = 0,
        action_numeric_mode: str = "additive",
    ):
        super().__init__()
        if action_numeric_mode not in (
            "additive",
            "gated_residual",
            "additive_gated_residual",
        ):
            raise ValueError(f"unsupported action numeric mode {action_numeric_mode!r}")
        self.action_numeric_mode = action_numeric_mode
        self.menu_residual_scale = 1.0
        self.embedding = nn.Embedding(
            FEATURE_BUCKETS + 1, hidden_size, padding_idx=0
        )
        self.state_projection = nn.Sequential(
            nn.RMSNorm(hidden_size),
            nn.Linear(hidden_size, hidden_size, bias=False),
        )
        self.action_projection = nn.Sequential(
            nn.RMSNorm(hidden_size),
            nn.Linear(hidden_size, hidden_size, bias=False),
        )
        self.numeric_projection = nn.Sequential(
            nn.RMSNorm(numeric_size),
            nn.Linear(numeric_size, hidden_size),
            nn.SiLU(),
            nn.Linear(hidden_size, hidden_size, bias=False),
        )
        self.action_numeric_projection = (
            nn.Sequential(
                nn.RMSNorm(action_numeric_size),
                nn.Linear(action_numeric_size, hidden_size),
                nn.SiLU(),
                nn.Linear(hidden_size, hidden_size, bias=False),
            )
            if action_numeric_size
            and action_numeric_mode in ("additive", "additive_gated_residual")
            else None
        )
        self.state_memory = SelectiveSsmMemory(hidden_size)
        self.identity_norm = nn.RMSNorm(hidden_size)
        self.match_projection = nn.Sequential(
            nn.Linear(2, hidden_size),
            nn.SiLU(),
        )
        self.output = nn.Sequential(
            nn.RMSNorm(hidden_size * 8),
            nn.Linear(hidden_size * 8, hidden_size * 2),
            nn.SiLU(),
            nn.Linear(hidden_size * 2, 1),
        )
        self.menu_residual = (
            nn.Sequential(
                nn.RMSNorm(hidden_size * 8 + action_numeric_size),
                nn.Linear(hidden_size * 8 + action_numeric_size, hidden_size * 2),
                nn.SiLU(),
                nn.Linear(hidden_size * 2, 1),
            )
            if action_numeric_size
            and action_numeric_mode in (
                "gated_residual",
                "additive_gated_residual",
            )
            else None
        )
        if self.menu_residual is not None:
            nn.init.zeros_(self.menu_residual[-1].weight)
            nn.init.zeros_(self.menu_residual[-1].bias)

    @staticmethod
    def pool(embedded: torch.Tensor, mask: torch.Tensor) -> torch.Tensor:
        return (embedded * mask.unsqueeze(-1)).sum(1) / mask.sum(
            1, keepdim=True
        ).clamp_min(1)

    def forward(
        self,
        state_ids: torch.Tensor,
        action_ids: torch.Tensor,
        numeric: torch.Tensor,
        inventory_ids: torch.Tensor,
        candidate_ids: torch.Tensor,
        action_numeric: torch.Tensor | None = None,
    ) -> torch.Tensor:
        state_mask = state_ids.ne(0)
        action_mask = action_ids.ne(0)
        inventory_mask = inventory_ids.ne(0)
        candidate_mask = candidate_ids.ne(0)
        state_embedded = self.embedding(state_ids)
        action_embedded = self.embedding(action_ids)
        inventory = self.embedding(inventory_ids)
        candidate = self.embedding(candidate_ids)

        state = self.state_projection(self.pool(state_embedded, state_mask))
        state = state + self.numeric_projection(numeric)
        action = self.action_projection(self.pool(action_embedded, action_mask))
        if self.action_numeric_projection is not None:
            if action_numeric is None:
                raise ValueError(
                    "action parameters are required by this choice critic"
                )
            action = action + self.action_numeric_projection(action_numeric)
        state_memory = self.state_memory(state_embedded, state_mask)
        inventory_summary = self.pool(inventory, inventory_mask)
        candidate_summary = self.pool(candidate, candidate_mask)

        normalized_inventory = F.normalize(
            self.identity_norm(inventory).float(), dim=-1
        )
        normalized_candidate = F.normalize(
            self.identity_norm(candidate).float(), dim=-1
        )
        visible_pairs = candidate_mask.unsqueeze(2) & inventory_mask.unsqueeze(1)
        scores = 8.0 * torch.einsum(
            "bch,bih->bci", normalized_candidate, normalized_inventory
        )
        weights = F.softmax(scores.masked_fill(~visible_pairs, -1e4), dim=-1)
        weights = weights * visible_pairs
        weights = weights / weights.sum(-1, keepdim=True).clamp_min(1e-6)
        attended_per_candidate = torch.einsum(
            "bci,bih->bch", weights.to(inventory.dtype), inventory
        )
        attended = self.pool(attended_per_candidate, candidate_mask)

        exact = visible_pairs & candidate_ids.unsqueeze(2).eq(
            inventory_ids.unsqueeze(1)
        )
        exact_count = exact.sum((1, 2)).float().unsqueeze(1)
        inventory_count = inventory_mask.sum(1, keepdim=True).clamp_min(1)
        match = self.match_projection(
            torch.cat(
                (
                    torch.log1p(exact_count) / math.log(51.0),
                    exact_count / inventory_count.float(),
                ),
                dim=1,
            )
        ).to(state.dtype)
        context = torch.cat(
            (
                state,
                action,
                state_memory,
                inventory_summary,
                candidate_summary,
                attended,
                candidate_summary * attended,
                match,
            ),
            dim=-1,
        )
        value = self.output(context)
        if self.menu_residual is not None:
            if action_numeric is None:
                raise ValueError(
                    "action parameters are required by this menu residual"
                )
            known = action_numeric[:, :1].clamp(0.0, 1.0)
            value = value + self.menu_residual_scale * known * self.menu_residual(
                torch.cat((context, action_numeric), dim=-1)
            )
        return value.squeeze(1)


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
        self.numeric_prefix_size = int(
            config.get("numeric_prefix_measurements", self.numeric_size)
        )
        if not 0 <= self.numeric_prefix_size <= self.numeric_size:
            raise ValueError("numeric prefix must fit inside numeric measurements")
        self.action_numeric_size = int(
            config.get("action_numeric_measurements", 0)
        )
        self.choice_numeric_size = int(
            config.get("choice_numeric_measurements", self.numeric_size)
        )
        self.action_numeric_mode = str(
            config.get("action_numeric_mode", "additive")
        )
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
                nn.RMSNorm(self.numeric_prefix_size),
                nn.Linear(self.numeric_prefix_size, hidden),
                nn.SiLU(),
                nn.Linear(hidden, hidden, bias=False),
            )
            if self.numeric_prefix_size
            else None
        )
        extra_numeric_size = self.numeric_size - self.numeric_prefix_size
        self.extra_numeric_projection = (
            nn.Sequential(
                nn.RMSNorm(extra_numeric_size),
                nn.Linear(extra_numeric_size, hidden),
                nn.SiLU(),
                nn.Linear(hidden, hidden, bias=False),
            )
            if extra_numeric_size
            else None
        )
        if self.extra_numeric_projection is not None:
            nn.init.zeros_(self.extra_numeric_projection[-1].weight)
        self.history_memory = (
            SelectiveSsmMemory(hidden)
            if self.architecture in ("hrm_ssm", "hrm_dual_ssm")
            else None
        )
        self.state_memory = (
            SelectiveSsmMemory(hidden)
            if self.architecture
            in (
                "hrm_state_ssm",
                "hrm_dual_ssm",
                "hrm_relational_ssm",
                "hrm_choice_critic_ssm",
            )
            else HierarchicalStateSsmMemory(hidden)
            if self.architecture == "hrm_hierarchical_ssm"
            else None
        )
        self.state_attention = (
            ActionConditionedStateAttention(hidden)
            if self.architecture == "hrm_attention"
            else None
        )
        self.inventory_memory = (
            CandidateInventoryMemory(hidden)
            if self.architecture in ("hrm_relational", "hrm_relational_ssm")
            else None
        )
        target_names = tuple(config.get("target_names", TARGET_NAMES))
        actor_target_names = tuple(config.get("actor_target_names", target_names))
        self.target_names = target_names
        self.actor_target_names = actor_target_names
        self.choice_value_index = (
            target_names.index("choice_value")
            if "choice_value" in target_names
            else None
        )
        self.choice_critic = (
            CounterfactualChoiceCritic(
                hidden,
                self.choice_numeric_size,
                self.action_numeric_size,
                self.action_numeric_mode,
            )
            if self.architecture == "hrm_choice_critic_ssm"
            and self.choice_value_index is not None
            and self.numeric_size
            else None
        )
        if self.choice_critic is not None:
            self.choice_critic.menu_residual_scale = float(
                config.get("menu_residual_scale", 1.0)
            )
        self.low = GatedBlock(hidden, expansion)
        self.high = GatedBlock(hidden, expansion)
        output_count = len(actor_target_names)
        extra_widths = sum(
            module is not None
            for module in (
                self.history_memory,
                self.state_memory,
                self.state_attention,
            )
        )
        output_width = hidden * (3 + extra_widths)
        self.output = nn.Sequential(
            nn.RMSNorm(output_width),
            nn.Linear(output_width, hidden * 2),
            nn.SiLU(),
            nn.Linear(hidden * 2, output_count),
        )
        adapter_input_width = (
            output_width
            + hidden * 2
            + extra_numeric_size
            + self.action_numeric_size
        )
        self.counterfactual_adapter_scale = float(
            config.get("counterfactual_adapter_scale", 1.0)
        )
        self.counterfactual_adapter_min_enemy_hp = float(
            config.get("counterfactual_adapter_min_enemy_hp", 0.0)
        )
        self.counterfactual_value_adapter = (
            nn.Sequential(
                nn.RMSNorm(adapter_input_width),
                nn.Linear(adapter_input_width, hidden * 2),
                nn.SiLU(),
                nn.Linear(hidden * 2, 1),
            )
            if config.get("counterfactual_value_adapter", False)
            and self.choice_value_index is not None
            else None
        )
        if self.counterfactual_value_adapter is not None:
            nn.init.zeros_(self.counterfactual_value_adapter[-1].weight)
            nn.init.zeros_(self.counterfactual_value_adapter[-1].bias)

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
        inventory_ids: torch.Tensor | None = None,
        candidate_identity_ids: torch.Tensor | None = None,
        action_numeric: torch.Tensor | None = None,
    ) -> torch.Tensor:
        state_embedded = self.embedding(state_ids)
        state_mask = state_ids.ne(0)
        state = self.state_projection(
            (state_embedded * state_mask.unsqueeze(-1)).sum(1)
            / state_mask.sum(1).unsqueeze(-1).clamp_min(1)
        )
        action = self.action_projection(self.pool(action_ids))
        if self.numeric_projection is not None:
            if numeric is None:
                raise ValueError("numeric measurements are required by this checkpoint")
            state = state + self.numeric_projection(
                numeric[:, : self.numeric_prefix_size]
            )
        if self.extra_numeric_projection is not None:
            if numeric is None:
                raise ValueError("numeric measurements are required by this checkpoint")
            state = state + self.extra_numeric_projection(
                numeric[:, self.numeric_prefix_size :]
            )
        context_parts: list[torch.Tensor] = []
        problem = state + action
        if self.history_memory is not None:
            if history_ids is None:
                raise ValueError("history IDs are required by the HRM-SSM checkpoint")
            history_memory = self.history_memory(
                self.embedding(history_ids), history_ids.ne(0)
            )
            problem = problem + history_memory
            context_parts.append(history_memory)
        if self.state_memory is not None:
            state_memory = self.state_memory(state_embedded, state_mask)
            problem = problem + state_memory
            context_parts.append(state_memory)
        if self.state_attention is not None:
            state_attention = self.state_attention(state_embedded, state_mask, action)
            problem = problem + state_attention
            context_parts.append(state_attention)
        if self.inventory_memory is not None:
            if inventory_ids is None or candidate_identity_ids is None:
                raise ValueError(
                    "inventory and candidate identity IDs are required by the "
                    "relational checkpoint"
                )
            inventory_memory = self.inventory_memory(
                inventory_ids, candidate_identity_ids, self.embedding
            )
            problem = problem + inventory_memory
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
        actor_context = torch.cat((high, state, action, *context_parts), dim=-1)
        prediction = self.output(actor_context)
        counterfactual_correction = None
        if self.counterfactual_value_adapter is not None:
            if inventory_ids is None or candidate_identity_ids is None:
                raise ValueError(
                    "inventory and candidate identity IDs are required by the "
                    "counterfactual-value adapter"
                )
            adapter_parts = [
                actor_context,
                self.pool(inventory_ids),
                self.pool(candidate_identity_ids),
            ]
            if self.numeric_size > self.numeric_prefix_size:
                if numeric is None:
                    raise ValueError(
                        "numeric measurements are required by the "
                        "counterfactual-value adapter"
                    )
                adapter_parts.append(numeric[:, self.numeric_prefix_size :])
            if self.action_numeric_size:
                if action_numeric is None:
                    raise ValueError(
                        "action parameters are required by the "
                        "counterfactual-value adapter"
                    )
                adapter_parts.append(action_numeric)
            counterfactual_correction = self.counterfactual_value_adapter(
                torch.cat(adapter_parts, dim=-1)
            ).squeeze(1)
        if self.choice_critic is not None:
            if (
                numeric is None
                or inventory_ids is None
                or candidate_identity_ids is None
            ):
                raise ValueError(
                    "numeric, inventory, and candidate identity inputs are required "
                    "by the choice-critic checkpoint"
                )
            choice_value = self.choice_critic(
                state_ids,
                action_ids,
                numeric[:, : self.choice_numeric_size],
                inventory_ids,
                candidate_identity_ids,
                action_numeric,
            ).unsqueeze(1)
            index = self.choice_value_index
            assert index is not None
            if index != prediction.shape[1]:
                raise ValueError("choice_value must follow every actor target")
            prediction = torch.cat(
                (prediction, choice_value),
                dim=1,
            )
        if counterfactual_correction is not None:
            index = self.choice_value_index
            assert index is not None
            if self.counterfactual_adapter_min_enemy_hp > 0.0:
                assert numeric is not None
                measurement_index = next(
                    index
                    for index, (name, _) in enumerate(MEASUREMENT_SPECS)
                    if name == "enemy_max_hp"
                )
                enemy_hp_scale = MEASUREMENT_SPECS[measurement_index][1]
                gate = (
                    numeric[:, measurement_index] * enemy_hp_scale
                    >= self.counterfactual_adapter_min_enemy_hp
                ).to(counterfactual_correction.dtype)
                counterfactual_correction = counterfactual_correction * gate
            prediction = prediction.clone()
            prediction[:, index] = (
                prediction[:, index]
                + self.counterfactual_adapter_scale * counterfactual_correction
            )
        return prediction


def masked_loss(
    prediction: torch.Tensor, target: torch.Tensor, mask: torch.Tensor
) -> torch.Tensor:
    """Mix regression heads with an ordinal final-floor survival objective."""
    prediction = prediction[:, : len(TARGET_NAMES)]
    survival_start = len(TARGET_NAMES) - len(FLOOR_SURVIVAL_NAMES)
    regression = F.smooth_l1_loss(
        prediction[:, :survival_start],
        target[:, :survival_start],
        reduction="none",
    )
    survival = F.binary_cross_entropy_with_logits(
        prediction[:, survival_start:],
        target[:, survival_start:],
        reduction="none",
    )
    component = torch.cat((regression, survival), dim=1)
    data_loss = (component * mask).sum() / mask.sum().clamp_min(1)

    # Survival curves must be non-increasing: reaching floor k+1 implies
    # reaching floor k. The soft constraint shares statistical strength across
    # sparse late floors without assigning them a separate policy objective.
    probabilities = prediction[:, survival_start:].sigmoid()
    monotonic = F.relu(probabilities[:, 1:] - probabilities[:, :-1])
    return data_loss + 0.05 * monotonic.mean()


def policy_training_prediction(
    model: nn.Module, prediction: torch.Tensor
) -> torch.Tensor:
    """Map an isolated counterfactual adapter onto existing branch labels."""
    full_prediction = prediction
    prediction = full_prediction[:, : len(TARGET_NAMES)].clone()
    adapter = getattr(model, "counterfactual_value_adapter", None)
    choice_index = getattr(model, "choice_value_index", None)
    if adapter is not None and choice_index is not None:
        search_index = TARGET_NAMES.index("search_value")
        prediction[:, search_index] = full_prediction[:, choice_index]
    return prediction


@torch.inference_mode()
def evaluate_preferences(
    model: SelfPlayHrm,
    tensors: dict[str, torch.Tensor],
    pairs: torch.Tensor,
    device: torch.device,
    batch_size: int,
) -> dict[str, float | int]:
    """Measure held-out exact-branch ordering rather than absolute offsets."""
    model.eval()
    correct = 0
    margin_sum = 0.0
    loss_sum = 0.0
    for start in range(0, len(pairs), batch_size):
        batch = pairs[start : start + batch_size]
        indices = torch.cat((batch[:, 0], batch[:, 1]), dim=0)
        inputs = [
            tensors[name].index_select(0, indices).to(
                device=device,
                dtype=torch.long if name in {
                    "state", "action", "inventory", "candidate", "history"
                } else torch.float32,
                non_blocking=True,
            )
            for name in (
                "state",
                "action",
                "inventory",
                "candidate",
                "numeric",
                "action_numeric",
                "history",
            )
        ]
        state, action, inventory, candidate, numeric, action_numeric, history = inputs
        prediction = model(
            state,
            action,
            numeric,
            history,
            inventory,
            candidate,
            action_numeric,
        )
        index = model.choice_value_index
        assert index is not None
        winner, loser = prediction[:, index].float().chunk(2)
        margin = winner - loser
        correct += int(margin.gt(0).sum())
        margin_sum += float(margin.sum())
        loss_sum += float(F.softplus(-margin / 0.1).sum())
    count = len(pairs)
    return {
        "count": count,
        "accuracy": correct / max(count, 1),
        "mean_margin": margin_sum / max(count, 1),
        "pairwise_loss": loss_sum / max(count, 1),
    }


@torch.inference_mode()
def evaluate(model: nn.Module, loader: DataLoader, device: torch.device) -> dict[str, Any]:
    model.eval()
    squared = torch.zeros(len(TARGET_NAMES), device=device)
    absolute = torch.zeros(len(TARGET_NAMES), device=device)
    counts = torch.zeros(len(TARGET_NAMES), device=device)
    for (
        state,
        action,
        inventory,
        candidate,
        numeric,
        action_numeric,
        history,
        target,
        mask,
    ) in loader:
        state = state.to(device=device, dtype=torch.long, non_blocking=True)
        action = action.to(device=device, dtype=torch.long, non_blocking=True)
        inventory = inventory.to(device=device, dtype=torch.long, non_blocking=True)
        candidate = candidate.to(device=device, dtype=torch.long, non_blocking=True)
        numeric = numeric.to(device=device, non_blocking=True)
        action_numeric = action_numeric.to(device=device, non_blocking=True)
        history = history.to(device=device, dtype=torch.long, non_blocking=True)
        target = target.to(device=device, non_blocking=True)
        mask = mask.to(device=device, non_blocking=True)
        prediction = model(
            state,
            action,
            numeric,
            history,
            inventory,
            candidate,
            action_numeric,
        )
        prediction = policy_training_prediction(model, prediction)
        survival_start = len(TARGET_NAMES) - len(FLOOR_SURVIVAL_NAMES)
        prediction = torch.cat(
            (
                prediction[:, :survival_start],
                prediction[:, survival_start:].sigmoid(),
            ),
            dim=1,
        )
        error = prediction - target
        squared += (error.square() * mask).sum(0)
        absolute += (error.abs() * mask).sum(0)
        counts += mask.sum(0)
    rmse = (squared / counts.clamp_min(1)).sqrt().cpu().tolist()
    mae = (absolute / counts.clamp_min(1)).cpu().tolist()
    return {
        name: {"rmse": rmse[index], "mae": mae[index], "count": int(counts[index])}
        for index, name in enumerate(TARGET_NAMES)
    }


def transplant_expanded_checkpoint(
    model: SelfPlayHrm, checkpoint: dict[str, Any]
) -> dict[str, int]:
    """Migrate a teacher-free actor into the current append-only schema.

    Shared tensors copy exactly. Newly appended numeric channels initially
    have zero projection weight, and actor output rows are joined by target
    name rather than position. Old predictions are therefore preserved while
    the ascension channel and newer auxiliary heads become trainable.
    """
    source = checkpoint["model"]
    destination = model.state_dict()
    exact_tensors = 0
    for name, value in source.items():
        if name in destination and destination[name].shape == value.shape:
            destination[name] = value
            exact_tensors += 1

    old_numeric = int(checkpoint["config"].get("numeric_measurements", 0))
    new_numeric = model.numeric_size
    expanded_numeric_tensors = 0
    if 0 < old_numeric < new_numeric:
        if model.numeric_prefix_size != old_numeric:
            raise ValueError(
                "expanded migration must preserve the legacy numeric prefix"
            )
        expanded_numeric_tensors = sum(
            name.startswith("extra_numeric_projection.") for name in destination
        )

    old_actor_names = tuple(
        checkpoint["config"].get(
            "actor_target_names",
            tuple(
                name
                for name in checkpoint["target_names"]
                if name != "choice_value"
            ),
        )
    )
    new_indices = {name: index for index, name in enumerate(TARGET_NAMES)}
    copied_output_rows = 0
    for parameter_name in ("output.3.weight", "output.3.bias"):
        if parameter_name not in source or parameter_name not in destination:
            continue
        for old_index, target_name in enumerate(old_actor_names):
            new_index = new_indices.get(target_name)
            if new_index is None or old_index >= source[parameter_name].shape[0]:
                continue
            destination[parameter_name][new_index] = source[parameter_name][old_index]
            if parameter_name.endswith("bias"):
                copied_output_rows += 1
    model.load_state_dict(destination)
    return {
        "exact_tensors": exact_tensors,
        "expanded_numeric_tensors": expanded_numeric_tensors,
        "copied_output_rows": copied_output_rows,
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
        branch_only=args.search_head_only or args.counterfactual_adapter_only,
    )
    train_tensors = prepared["tensors"]["train"]
    validation_tensors = prepared["tensors"]["validation"]
    train_set = TensorDataset(
        train_tensors["state"],
        train_tensors["action"],
        train_tensors["inventory"],
        train_tensors["candidate"],
        train_tensors["numeric"],
        train_tensors["action_numeric"],
        train_tensors["history"],
        train_tensors["target"],
        train_tensors["mask"],
    )
    validation_set = TensorDataset(
        validation_tensors["state"],
        validation_tensors["action"],
        validation_tensors["inventory"],
        validation_tensors["candidate"],
        validation_tensors["numeric"],
        validation_tensors["action_numeric"],
        validation_tensors["history"],
        validation_tensors["target"],
        validation_tensors["mask"],
    )
    preference_pairs = None
    if args.counterfactual_adapter_only:
        preference_pairs = branch_preference_pairs(args.branch_dataset)
        if not len(preference_pairs["train"]):
            raise ValueError("counterfactual branch data contains no ranked pairs")
        train_loader = DataLoader(
            TensorDataset(preference_pairs["train"]),
            batch_size=args.batch_size,
            shuffle=True,
            num_workers=2,
            pin_memory=device.type == "cuda",
            persistent_workers=True,
        )
        log(
            "counterfactual preference pairs: "
            f"train={len(preference_pairs['train'])}, "
            f"validation={len(preference_pairs['validation'])}"
        )
    else:
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
        config = dict(initial_checkpoint["config"])
        if args.expand_init_schema:
            old_numeric = int(config.get("numeric_measurements", 0))
            old_target_names = tuple(initial_checkpoint["target_names"])
            old_actor_names = tuple(
                config.get(
                    "actor_target_names",
                    tuple(name for name in old_target_names if name != "choice_value"),
                )
            )
            auxiliary_names = tuple(
                name
                for name in old_target_names
                if name not in old_actor_names and name not in TARGET_NAMES
            )
            config["numeric_measurements"] = len(MEASUREMENT_SPECS)
            config["numeric_prefix_measurements"] = old_numeric
            config["choice_numeric_measurements"] = old_numeric
            config["action_numeric_measurements"] = len(ACTION_PARAMETER_SPECS)
            config["target_names"] = (*TARGET_NAMES, *auxiliary_names)
            config["actor_target_names"] = TARGET_NAMES
            config["counterfactual_value_adapter"] = (
                args.counterfactual_adapter_only
            )
        else:
            if tuple(initial_checkpoint["target_names"]) != TARGET_NAMES:
                raise ValueError(
                    "initial checkpoint target heads do not match; "
                    "use --expand-init-schema for an append-only migration"
                )
            config["target_names"] = TARGET_NAMES
    else:
        config = {
            **DEFAULTS,
            "hidden_size": args.hidden_size,
            "batch_size": args.batch_size,
            "target_names": TARGET_NAMES,
            "numeric_measurements": len(MEASUREMENT_SPECS),
            "action_numeric_measurements": len(ACTION_PARAMETER_SPECS),
            "architecture": args.architecture,
        }
    model = SelfPlayHrm(config).to(device)
    if initial_checkpoint is not None:
        if args.expand_init_schema:
            migration = transplant_expanded_checkpoint(model, initial_checkpoint)
            log(f"expanded initial checkpoint: {migration}")
        else:
            model.load_state_dict(initial_checkpoint["model"])
    if args.search_head_only:
        for parameter in model.parameters():
            parameter.requires_grad_(False)
        search_module = (
            model.choice_critic
            if model.choice_critic is not None
            else model.output[-1]
        )
        for parameter in search_module.parameters():
            parameter.requires_grad_(True)
    if args.counterfactual_adapter_only:
        for parameter in model.parameters():
            parameter.requires_grad_(False)
        if model.counterfactual_value_adapter is None:
            raise ValueError(
                "counterfactual-adapter-only requires an enabled adapter and critic"
            )
        for parameter in model.counterfactual_value_adapter.parameters():
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
        for batch in train_loader:
            if time.monotonic() - started >= args.seconds:
                break
            if args.counterfactual_adapter_only:
                pair_indices = batch[0]
                indices = torch.cat(
                    (pair_indices[:, 0], pair_indices[:, 1]), dim=0
                )
                state = train_tensors["state"].index_select(0, indices)
                action = train_tensors["action"].index_select(0, indices)
                inventory = train_tensors["inventory"].index_select(0, indices)
                candidate = train_tensors["candidate"].index_select(0, indices)
                numeric = train_tensors["numeric"].index_select(0, indices)
                action_numeric = train_tensors["action_numeric"].index_select(
                    0, indices
                )
                history = train_tensors["history"].index_select(0, indices)
                target = None
                mask = None
            else:
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
            state = state.to(device=device, dtype=torch.long, non_blocking=True)
            action = action.to(device=device, dtype=torch.long, non_blocking=True)
            inventory = inventory.to(
                device=device, dtype=torch.long, non_blocking=True
            )
            candidate = candidate.to(
                device=device, dtype=torch.long, non_blocking=True
            )
            numeric = numeric.to(device=device, non_blocking=True)
            action_numeric = action_numeric.to(device=device, non_blocking=True)
            history = history.to(device=device, dtype=torch.long, non_blocking=True)
            if target is not None:
                target = target.to(device=device, non_blocking=True)
            if mask is not None:
                mask = mask.to(device=device, non_blocking=True)
            optimizer.zero_grad(set_to_none=True)
            with torch.autocast(
                device_type=device.type,
                dtype=amp_dtype,
                enabled=device.type == "cuda",
            ):
                prediction = model(
                    state,
                    action,
                    numeric,
                    history,
                    inventory,
                    candidate,
                    action_numeric,
                )
                if args.counterfactual_adapter_only:
                    index = model.choice_value_index
                    assert index is not None
                    winner, loser = prediction[:, index].chunk(2)
                    loss = F.softplus(-(winner - loser) / 0.1).mean()
                else:
                    assert target is not None and mask is not None
                    loss = masked_loss(
                        policy_training_prediction(model, prediction), target, mask
                    )
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
    preference_metrics = (
        evaluate_preferences(
            model,
            validation_tensors,
            preference_pairs["validation"],
            device,
            args.batch_size,
        )
        if preference_pairs is not None
        else None
    )
    elapsed = time.monotonic() - started
    args.output.parent.mkdir(parents=True, exist_ok=True)
    checkpoint_target_names = tuple(config["target_names"])
    if (
        initial_checkpoint is not None
        and args.expand_init_schema
        and args.counterfactual_adapter_only
    ):
        policy_supported_targets = tuple(
            initial_checkpoint.get(
                "policy_supported_targets", initial_checkpoint["target_names"]
            )
        )
    else:
        policy_supported_targets = checkpoint_target_names
    checkpoint = {
        "format": "sts-selfplay-hrm-v1",
        "model": model.state_dict(),
        "config": config,
        "feature_buckets": FEATURE_BUCKETS,
        "max_state_features": MAX_STATE_FEATURES,
        "max_action_features": MAX_ACTION_FEATURES,
        "max_inventory_identities": MAX_INVENTORY_IDENTITIES,
        "max_candidate_identities": MAX_CANDIDATE_IDENTITIES,
        "max_history_steps": MAX_HISTORY_STEPS,
        "measurement_specs": MEASUREMENT_SPECS,
        "action_parameter_specs": ACTION_PARAMETER_SPECS,
        "target_names": checkpoint_target_names,
        "target_scales": TARGET_SCALES,
        "dataset_signature": prepared["signature"],
        "teacher": None,
        "initialized_from": (
            str(args.init_checkpoint) if args.init_checkpoint is not None else None
        ),
        "expanded_init_schema": args.expand_init_schema,
        "search_head_only": args.search_head_only,
        "counterfactual_adapter_only": args.counterfactual_adapter_only,
        "search_value_supported": bool(args.branch_dataset),
        "search_value_min_floor": 16,
        "policy_supported_targets": policy_supported_targets,
        "preference_validation": preference_metrics,
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
                "expanded_init_schema": args.expand_init_schema,
                "search_head_only": args.search_head_only,
                "counterfactual_adapter_only": args.counterfactual_adapter_only,
                "architecture": config["architecture"],
                "replay_priority": (
                    "uniform_expected_final_floor_v1"
                    if args.frontier_priority_scale == 0.0
                    else "self_discovered_floor_frontier_v1"
                ),
                "train_priority_mean": float(train_tensors["weight"].mean()),
                "frontier_priority_scale": args.frontier_priority_scale,
                "validation": metrics,
                "preference_validation": preference_metrics,
            },
            indent=2,
            sort_keys=True,
        )
        + "\n"
    )
    log(f"saved {args.output} and {metrics_path}")
    for name, values in metrics.items():
        log(f"validation {name}: rmse={values['rmse']:.4f} mae={values['mae']:.4f}")
    if preference_metrics is not None:
        log(f"validation preferences: {preference_metrics}")


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
        "--architecture",
        choices=(
            "hrm",
            "hrm_ssm",
            "hrm_state_ssm",
            "hrm_hierarchical_ssm",
            "hrm_dual_ssm",
            "hrm_attention",
            "hrm_relational",
            "hrm_relational_ssm",
        ),
        default="hrm_state_ssm",
    )
    parser.add_argument("--batch-size", type=int, default=DEFAULTS["batch_size"])
    parser.add_argument("--learning-rate", type=float, default=DEFAULTS["learning_rate"])
    parser.add_argument("--weight-decay", type=float, default=DEFAULTS["weight_decay"])
    parser.add_argument(
        "--frontier-priority-scale",
        type=float,
        default=0.0,
        help=(
            "optional high-floor replay oversampling; zero keeps the training "
            "distribution aligned with mean final floor"
        ),
    )
    parser.add_argument("--init-checkpoint", type=Path)
    parser.add_argument(
        "--expand-init-schema",
        action="store_true",
        help=(
            "migrate a teacher-free actor into the current numeric/target schema; "
            "shared tensors and named output rows are preserved"
        ),
    )
    parser.add_argument(
        "--search-head-only",
        action="store_true",
        help="train only the search-value output row on branch records",
    )
    parser.add_argument(
        "--counterfactual-adapter-only",
        action="store_true",
        help=(
            "freeze the migrated policy and train a zero-initialized A20 "
            "counterfactual-value residual from exact branch records"
        ),
    )
    parser.add_argument("--seed", type=int, default=20260826)
    args = parser.parse_args()
    if args.dataset is None:
        args.dataset = [Path("artifacts/selfplay/defect-a0-random-traces-1000.jsonl.xz")]
    if args.branch_dataset is None:
        args.branch_dataset = []
    if (args.search_head_only or args.counterfactual_adapter_only) and (
        args.init_checkpoint is None or not args.branch_dataset
    ):
        parser.error("search-only training requires an initial checkpoint and branch data")
    if args.expand_init_schema and args.init_checkpoint is None:
        parser.error("expand-init-schema requires an initial checkpoint")
    if args.counterfactual_adapter_only and not args.expand_init_schema:
        parser.error("counterfactual-adapter-only requires expand-init-schema")
    if args.counterfactual_adapter_only and args.search_head_only:
        parser.error("counterfactual adapter and search head modes are mutually exclusive")
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
