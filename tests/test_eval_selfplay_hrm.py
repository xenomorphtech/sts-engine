import argparse
import sys
from pathlib import Path

import torch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from eval_selfplay_hrm import (
    apply_lookahead_defaults,
    branch_score,
    decision_signature,
    observation_key,
    replay_policy_state,
    utilities,
)
from mean_progress_model import (
    ActionConditionedStateAttention,
    MeanProgressModel,
)
from training_schema import (
    ACTION_PARAMETER_SPECS,
    action_parameter_vector,
)


def test_action_attention_handles_padded_candidates() -> None:
    attention = ActionConditionedStateAttention(hidden_size=4).eval()
    output = attention(
        torch.zeros((1, 3, 4)),
        torch.zeros((1, 3), dtype=torch.bool),
        torch.zeros((1, 4)),
    )
    assert torch.isfinite(output).all()
    assert torch.equal(output, torch.zeros_like(output))


def test_a20_lookahead_defaults_join_noncombat_choices_to_next_room() -> None:
    args = argparse.Namespace(
        ascension=20,
        lookahead_depth=12,
        lookahead_candidates=None,
        lookahead_combat_hp_weight=None,
        lookahead_noncombat_only=None,
        lookahead_noncombat_depth=None,
        lookahead_identity_choices_only=False,
        lookahead_include_identity_choices=False,
    )

    apply_lookahead_defaults(args)

    assert args.lookahead_candidates == 3
    assert args.lookahead_combat_hp_weight == 100.0
    assert args.lookahead_noncombat_only is True
    assert args.lookahead_noncombat_depth == 64


def test_noncombat_lookahead_default_preserves_lower_ascensions_and_opt_out() -> None:
    lower = argparse.Namespace(
        ascension=0,
        lookahead_depth=12,
        lookahead_candidates=None,
        lookahead_combat_hp_weight=None,
        lookahead_noncombat_only=None,
        lookahead_noncombat_depth=None,
        lookahead_identity_choices_only=False,
        lookahead_include_identity_choices=False,
    )
    disabled = argparse.Namespace(
        ascension=20,
        lookahead_depth=12,
        lookahead_candidates=None,
        lookahead_combat_hp_weight=None,
        lookahead_noncombat_only=False,
        lookahead_noncombat_depth=None,
        lookahead_identity_choices_only=False,
        lookahead_include_identity_choices=False,
    )

    apply_lookahead_defaults(lower)
    apply_lookahead_defaults(disabled)

    assert lower.lookahead_noncombat_only is False
    assert lower.lookahead_noncombat_depth is None
    assert disabled.lookahead_noncombat_only is False
    assert disabled.lookahead_noncombat_depth is None


def test_a20_noncombat_default_does_not_override_identity_ablation() -> None:
    args = argparse.Namespace(
        ascension=20,
        lookahead_depth=12,
        lookahead_candidates=None,
        lookahead_combat_hp_weight=None,
        lookahead_noncombat_only=None,
        lookahead_noncombat_depth=None,
        lookahead_identity_choices_only=True,
        lookahead_include_identity_choices=False,
    )

    apply_lookahead_defaults(args)

    assert args.lookahead_noncombat_only is False
    assert args.lookahead_noncombat_depth is None


def test_combat_branch_hp_weight_prefers_healthier_leaf() -> None:
    root = {
        "floor": 5,
        "hp": 70,
        "enemy_max_hp": 100,
        "enemy_hp": 100,
        "combat_turn": 1,
        "energy": 3,
        "hand_size": 5,
        "draw_size": 5,
        "discard_size": 0,
    }
    low_hp = {
        "outcome": "running",
        "measurements": {
            "floor": 5,
            "hp": 20,
            "max_hp": 70,
            "enemy_max_hp": 100,
            "enemy_hp": 50,
            "incoming_attack": 0,
            "combat_turn": 2,
            "energy": 0,
            "hand_size": 3,
            "draw_size": 5,
            "discard_size": 2,
        },
    }
    high_hp = {
        "outcome": "running",
        "measurements": {
            **low_hp["measurements"],
            "hp": 60,
            "enemy_hp": 70,
        },
    }
    low_weight_margin = branch_score(root, high_hp, 0.0, 20.0) - branch_score(
        root, low_hp, 0.0, 20.0
    )
    high_weight_margin = branch_score(root, high_hp, 0.0, 100.0) - branch_score(
        root, low_hp, 0.0, 100.0
    )
    assert high_weight_margin > low_weight_margin


def test_replay_policy_state_restores_tried_actions() -> None:
    observation = {
        "state_features": [1, 2],
        "inventory_identities": [3],
        "actions": [
            {"index": 0, "features": [4], "candidate_identities": [5]},
            {"index": 1, "features": [6], "candidate_identities": [7]},
        ],
    }
    prefix = [
        {"observation": observation, "action_index": 0},
        {"observation": observation, "action_index": 1},
    ]

    history, tried = replay_policy_state(prefix)

    assert history == [
        decision_signature(observation, 0),
        decision_signature(observation, 1),
    ]
    assert tried == {observation_key(observation): {0, 1}}


def test_combat_action_parameters_share_one_transition_schema() -> None:
    action = {
        "parameters": {
            "known": True,
            "hp_delta": -7,
            "enemy_hp_delta": -12,
            "energy_delta": -1,
        }
    }
    measurements = {"hp": 30, "max_hp": 70, "enemy_hp": 40, "gold": 99}

    encoded = action_parameter_vector(action, measurements)

    assert len(encoded) == len(ACTION_PARAMETER_SPECS)
    assert encoded[0] == 1.0
    assert encoded[1] < 0.0
    assert encoded[3] < 0.0
    assert encoded[6] < 0.0


def test_mean_progress_model_has_one_shared_embedding_and_three_outputs() -> None:
    model = MeanProgressModel(
        {
            "hidden_size": 8,
            "numeric_measurements": 45,
            "action_numeric_measurements": len(ACTION_PARAMETER_SPECS),
            "target_names": (
                "progress_value",
                "final_floor",
                "entry_hp_fraction",
            ),
        }
    ).eval()

    inputs = {
        "state_ids": torch.tensor([[1, 2]], dtype=torch.long),
        "action_ids": torch.tensor([[3]], dtype=torch.long),
        "numeric": torch.zeros((1, 45)),
        "history_ids": torch.zeros((1, 1), dtype=torch.long),
        "inventory_ids": torch.tensor([[4]], dtype=torch.long),
        "candidate_identity_ids": torch.tensor([[5]], dtype=torch.long),
        "action_numeric": torch.zeros((1, len(ACTION_PARAMETER_SPECS))),
    }
    prediction = model(**inputs)

    assert prediction.shape == (1, 3)
    assert (
        sum(isinstance(module, torch.nn.Embedding) for module in model.modules()) == 1
    )


def test_distilled_policy_head_is_separate_from_progress_representation() -> None:
    target_names = (
        "policy_logit",
        "progress_value",
        "final_floor",
        "entry_hp_fraction",
    )
    model = MeanProgressModel(
        {
            "hidden_size": 8,
            "numeric_measurements": 45,
            "action_numeric_measurements": len(ACTION_PARAMETER_SPECS),
            "target_names": target_names,
        }
    )
    prediction = model(
        state_ids=torch.tensor([[1, 2]]),
        action_ids=torch.tensor([[3]]),
        numeric=torch.zeros((1, 45)),
        history_ids=torch.zeros((1, 1), dtype=torch.long),
        inventory_ids=torch.tensor([[4]]),
        candidate_identity_ids=torch.tensor([[5]]),
        action_numeric=torch.zeros((1, len(ACTION_PARAMETER_SPECS))),
    )
    prediction[:, 0].sum().backward()

    assert prediction.shape == (1, 4)
    assert model.embedding.weight.grad is not None
    assert torch.count_nonzero(model.embedding.weight.grad) == 0
    assert model.policy_output[-1].weight.grad is not None


def test_policy_and_leaf_utility_select_distinct_heads() -> None:
    prediction = torch.tensor([[4.0, 0.25, 0.2, 0.8]])
    target_names = (
        "policy_logit",
        "progress_value",
        "final_floor",
        "entry_hp_fraction",
    )
    args = (
        prediction,
        torch.tensor([False]),
        torch.tensor([True]),
        torch.tensor([True]),
        "mean_progress",
        target_names,
        frozenset(target_names),
        0.0,
        0.0,
        0.0,
        0.0,
    )

    assert utilities(*args).item() == 4.0
    assert utilities(*args, "progress_value").item() == 0.25
