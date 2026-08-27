"""Stable engine-to-model schema shared by training and closed-loop evaluation."""

from __future__ import annotations

import hashlib
import json
import lzma
import math
from collections.abc import Iterable
from pathlib import Path
from typing import Any

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
    ("ascension", 20.0),
)
ACTION_PARAMETER_SPECS = (
    ("known", 1.0),
    ("hp_delta", 100.0),
    ("max_hp_delta", 100.0),
    ("enemy_hp_delta", 500.0),
    ("block_delta", 100.0),
    ("enemy_block_delta", 500.0),
    ("energy_delta", 10.0),
    ("gold_delta", 500.0),
    ("hand_delta", 10.0),
    ("draw_delta", 10.0),
    ("discard_delta", 10.0),
    ("exhaust_delta", 10.0),
    ("deck_size_delta", 10.0),
    ("upgraded_cards_delta", 10.0),
    ("relic_delta", 5.0),
    ("potion_delta", 5.0),
    ("orb_slots_delta", 10.0),
    ("filled_orbs_delta", 10.0),
    ("orb_evoke_delta", 500.0),
    ("incoming_attack_delta", 500.0),
    ("living_enemies_delta", 5.0),
    ("turn_delta", 10.0),
    ("cards_played_delta", 20.0),
    ("player_power_delta", 100.0),
    ("enemy_power_delta", 200.0),
    ("hp_delta_current_fraction", 1.0),
    ("max_hp_delta_fraction", 1.0),
    ("enemy_hp_delta_current_fraction", 1.0),
    ("gold_delta_current_fraction", 1.0),
    ("lethal_hp_loss", 1.0),
    ("lethal_enemy_damage", 1.0),
)
RAW_ACTION_PARAMETER_COUNT = 25


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
        symlog_scaled(float(measurements.get(name, -1.0)), scale)
        for name, scale in MEASUREMENT_SPECS
    ]


def action_parameter_vector(
    action: dict[str, Any], measurements: dict[str, Any]
) -> list[float]:
    parameters = action.get("parameters") or {}
    known = bool(parameters.get("known", False))
    values = {
        name: float(known) if name == "known" else float(parameters.get(name, 0.0))
        for name, _ in ACTION_PARAMETER_SPECS[:RAW_ACTION_PARAMETER_COUNT]
    }
    if known:
        denominators = {
            "hp_delta_current_fraction": max(float(measurements.get("hp", 0)), 1.0),
            "max_hp_delta_fraction": max(float(measurements.get("max_hp", 0)), 1.0),
            "enemy_hp_delta_current_fraction": max(
                float(measurements.get("enemy_hp", 0)), 1.0
            ),
            "gold_delta_current_fraction": max(float(measurements.get("gold", 0)), 1.0),
        }
        numerators = {
            "hp_delta_current_fraction": values["hp_delta"],
            "max_hp_delta_fraction": values["max_hp_delta"],
            "enemy_hp_delta_current_fraction": values["enemy_hp_delta"],
            "gold_delta_current_fraction": values["gold_delta"],
        }
        for name, denominator in denominators.items():
            values[name] = max(-2.0, min(2.0, numerators[name] / denominator))
        values["lethal_hp_loss"] = float(
            values["hp_delta"] < 0
            and float(measurements.get("hp", 0)) + values["hp_delta"] <= 0
        )
        values["lethal_enemy_damage"] = float(
            values["enemy_hp_delta"] < 0
            and float(measurements.get("enemy_hp", 0)) + values["enemy_hp_delta"] <= 0
        )
    else:
        for name, _ in ACTION_PARAMETER_SPECS[RAW_ACTION_PARAMETER_COUNT:]:
            values[name] = 0.0
    return [
        symlog_scaled(values[name], scale) for name, scale in ACTION_PARAMETER_SPECS
    ]


def decision_signature(observation: dict[str, Any], action_index: int) -> int:
    value = 0xCBF29CE484222325
    action = next(
        action
        for action in observation["actions"]
        if int(action["index"]) == action_index
    )
    for feature in observation["state_features"][:4] + action["features"]:
        value ^= int(feature)
        value = (value * 0x100000001B3) & ((1 << 64) - 1)
    return value % FEATURE_BUCKETS + 1


def feature_token_id(token: str) -> int:
    value = 0xCBF29CE484222325
    for byte in token.encode():
        value ^= byte
        value = (value * 0x100000001B3) & ((1 << 64) - 1)
    return value % FEATURE_BUCKETS + 1


CHOICE_NONE_ID = feature_token_id("IDENTITY:CHOICE_NONE")


def candidate_identity_features(
    observation: dict[str, Any], selected: dict[str, Any], in_combat: bool
) -> list[int]:
    if in_combat:
        return list(selected.get("candidate_identities", []))
    identities = list(selected.get("candidate_identities", []))
    if not identities and any(
        action.get("candidate_identities") for action in observation["actions"]
    ):
        identities.append(CHOICE_NONE_ID)
    return identities


def split_for_seed(seed: int) -> str:
    digest = hashlib.sha256(f"selfplay-eval-v1:{seed}".encode()).digest()
    return "validation" if int.from_bytes(digest[:8], "little") % 10 == 0 else "train"
