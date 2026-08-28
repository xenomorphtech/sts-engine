"""Focused tests for the clean mean-progress training objective."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

import torch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from train_mean_progress import (
    final_entry,
    masked_cross_entropy,
    prepare,
    progress_target,
    selection_score,
)


def episode(floor: int, entry_hp: int, max_hp: int) -> dict:
    before = {"floor": floor - 1, "hp": max_hp, "max_hp": max_hp}
    entry = {"floor": floor, "hp": entry_hp, "max_hp": max_hp}
    terminal = {"floor": floor, "hp": 0, "max_hp": max_hp}
    return {
        "result": {
            "seed": floor,
            "max_floor": floor,
            "outcome": "player_death",
            "terminal": terminal,
        },
        "transitions": [
            {"before": before, "after": entry},
            {"before": entry, "after": terminal},
        ],
    }


def test_final_entry_uses_health_carried_into_the_furthest_floor() -> None:
    assert final_entry(episode(10, 37, 70)) == (10, 37, 70)


def test_progress_target_keeps_floor_primary_and_hp_dense() -> None:
    farther = progress_target(episode(10, 1, 70))[0]
    healthier_but_earlier = progress_target(episode(9, 100, 100))[0]
    same_floor_healthier = progress_target(episode(10, 40, 70))[0]

    assert farther > healthier_but_earlier
    assert same_floor_healthier > farther


def test_existing_implicit_cache_is_a_frozen_generation(tmp_path: Path) -> None:
    cache = tmp_path / "generation.pt"
    expected = {"format": "sts-mean-progress-data-v2", "sentinel": 17}
    torch.save(expected, cache)

    loaded = prepare(
        argparse.Namespace(
            cache=cache,
            rebuild_cache=False,
            trajectory=None,
            imitation=None,
        )
    )

    assert loaded == expected


def test_checkpoint_selection_tracks_progress_and_planner_imitation() -> None:
    reference = {"progress_mae": 0.12, "imitation_top_accuracy": 0.55}
    better_progress = {"progress_mae": 0.11, "imitation_top_accuracy": 0.55}
    better_imitation = {"progress_mae": 0.12, "imitation_top_accuracy": 0.60}

    assert selection_score(better_progress) < selection_score(reference)
    assert selection_score(better_imitation) < selection_score(reference)


def test_label_smoothing_ignores_padded_actions() -> None:
    logits = torch.tensor([[2.0, 1.0, -torch.inf]])
    mask = torch.tensor([[True, True, False]])

    loss = masked_cross_entropy(logits, torch.tensor([0]), mask, 0.05)

    assert torch.isfinite(loss)
