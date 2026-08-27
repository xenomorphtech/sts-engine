#!/usr/bin/env python3
"""Focused regression tests for trajectory-level self-play ranking."""

from __future__ import annotations

import sys
from pathlib import Path

import torch


ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "tools"))

from train_selfplay_branch_rank import (  # noqa: E402
    aggregate_action_rollouts,
    centered_policy_advantages,
    episode_progress_key,
    episode_progress_return,
    policy_gradient_loss,
    progress_prefix,
)


def episode(floor: int, entry_hp: int, max_hp: int, terminal_score: float):
    state = {"floor": floor, "hp": entry_hp, "max_hp": max_hp}
    return {
        "result": {
            "max_floor": floor,
            "terminal_score": terminal_score,
            "terminal": state,
        },
        "transitions": [{"before": state, "after": state}],
    }


def test_progress_key_prefers_floor_before_health() -> None:
    farther = episode(10, 1, 70, -200)
    healthier = episode(9, 70, 70, -1)
    assert episode_progress_key(farther) > episode_progress_key(healthier)


def test_progress_key_prefers_entry_health_on_same_floor() -> None:
    healthier = episode(10, 40, 70, -200)
    lower_enemy_hp = episode(10, 39, 70, -1)
    assert episode_progress_key(healthier) > episode_progress_key(lower_enemy_hp)


def test_progress_prefix_stops_after_entering_best_floor() -> None:
    states = [
        {"floor": 9, "hp": 40, "max_hp": 70},
        {"floor": 10, "hp": 35, "max_hp": 70},
        {"floor": 10, "hp": 10, "max_hp": 70},
        {"floor": 10, "hp": 0, "max_hp": 70},
    ]
    value = {
        "result": {
            "max_floor": 10,
            "terminal_score": -100,
            "terminal": states[-1],
        },
        "transitions": [
            {"before": before, "after": after}
            for before, after in zip(states, states[1:])
        ],
    }
    assert progress_prefix(value) == [value["transitions"][0]]


def test_progress_return_keeps_floor_primary_and_hp_dense() -> None:
    assert episode_progress_return(episode(10, 1, 70, -200)) > (
        episode_progress_return(episode(9, 70, 90, 90))
    )
    assert episode_progress_return(episode(10, 40, 70, -200)) > (
        episode_progress_return(episode(10, 39, 70, -1))
    )


def test_max_return_aggregation_backs_up_best_continuation() -> None:
    rows = [
        {"action_index": 0, "branch_score": 8.0},
        {"action_index": 0, "branch_score": 12.0},
        {"action_index": 1, "branch_score": 10.0},
    ]
    backed_up = aggregate_action_rollouts(rows, 0.0, "max")
    assert [row["branch_score"] for row in backed_up] == [12.0, 10.0]


def test_policy_advantages_use_bad_runs_as_negative_evidence() -> None:
    advantages = centered_policy_advantages([8.0, 10.0, 12.0])
    assert advantages[0] < 0.0 < advantages[-1]
    assert abs(sum(advantages)) < 1e-12
    assert centered_policy_advantages([10.0, 10.0]) == [0.0, 0.0]


def test_policy_gradient_raises_good_action_and_lowers_bad_action() -> None:
    good_logits = torch.zeros((1, 2), requires_grad=True)
    policy_gradient_loss(
        good_logits,
        torch.tensor([[1.0, 0.0]]),
        torch.tensor([[True, True]]),
    ).backward()
    assert good_logits.grad[0, 0] < 0 < good_logits.grad[0, 1]

    bad_logits = torch.zeros((1, 2), requires_grad=True)
    policy_gradient_loss(
        bad_logits,
        torch.tensor([[-1.0, 0.0]]),
        torch.tensor([[True, True]]),
    ).backward()
    assert bad_logits.grad[0, 0] > 0 > bad_logits.grad[0, 1]


if __name__ == "__main__":
    test_progress_key_prefers_floor_before_health()
    test_progress_key_prefers_entry_health_on_same_floor()
    test_progress_prefix_stops_after_entering_best_floor()
    test_progress_return_keeps_floor_primary_and_hp_dense()
    test_max_return_aggregation_backs_up_best_continuation()
    test_policy_advantages_use_bad_runs_as_negative_evidence()
    test_policy_gradient_raises_good_action_and_lowers_bad_action()
    print("7 trajectory-ranking regression tests passed")
