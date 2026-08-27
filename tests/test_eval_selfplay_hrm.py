import argparse
from pathlib import Path
import sys

import torch


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from eval_selfplay_hrm import (  # noqa: E402
    apply_lookahead_defaults,
    branch_score,
    decision_signature,
    observation_key,
    replay_policy_state,
)
from train_selfplay_hrm import (  # noqa: E402
    ActionConditionedStateAttention,
    CounterfactualChoiceCritic,
    ENEMY_MAX_HP_MEASUREMENT_INDEX,
    SelfPlayHrm,
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
            {"features": [4], "candidate_identities": [5]},
            {"features": [6], "candidate_identities": [7]},
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


def test_combat_menu_residual_is_enemy_hp_gated() -> None:
    critic = CounterfactualChoiceCritic(
        hidden_size=4,
        numeric_size=35,
        action_numeric_size=12,
        combat_menu_residual=True,
    ).eval()
    assert critic.combat_menu_residual is not None
    with torch.no_grad():
        critic.combat_menu_residual[-1].bias.fill_(1.0)

    state = torch.ones((2, 2), dtype=torch.long)
    action = torch.ones((2, 1), dtype=torch.long)
    numeric = torch.zeros((2, 35))
    numeric[1, ENEMY_MAX_HP_MEASUREMENT_INDEX] = 168.0 / 500.0
    inventory = torch.zeros((2, 1), dtype=torch.long)
    candidate = torch.zeros((2, 1), dtype=torch.long)
    action_numeric = torch.zeros((2, 12))

    critic.combat_menu_residual_scale = 0.0
    baseline = critic(
        state, action, numeric, inventory, candidate, action_numeric
    )
    critic.combat_menu_residual_scale = 1.0
    corrected = critic(
        state, action, numeric, inventory, candidate, action_numeric
    )

    assert torch.allclose(corrected[0], baseline[0])
    assert torch.allclose(corrected[1], baseline[1] + 1.0)


def test_relational_population_adapter_is_exactly_disabled_at_zero_scale() -> None:
    model = SelfPlayHrm(
        {
            "hidden_size": 8,
            "expansion": 2,
            "h_cycles": 1,
            "l_cycles": 1,
            "segments": 1,
            "architecture": "hrm_choice_critic_ssm",
            "numeric_measurements": 45,
            "numeric_prefix_measurements": 35,
            "choice_numeric_measurements": 35,
            "action_numeric_measurements": 12,
            "action_numeric_mode": "additive_gated_residual",
            "target_names": ("max_floor", "choice_value"),
            "actor_target_names": ("max_floor",),
            "counterfactual_value_adapter": True,
            "population_value_adapter": True,
            "population_relational_inventory": True,
            "population_action_attention": True,
            "population_adapter_combat_only": True,
        }
    ).eval()
    assert model.population_inventory_memory is not None
    assert model.population_state_attention is not None
    assert model.population_value_adapter is not None
    with torch.no_grad():
        model.population_value_adapter[-1].bias.fill_(1.0)

    inputs = {
        "state_ids": torch.tensor([[1, 2]], dtype=torch.long),
        "action_ids": torch.tensor([[3]], dtype=torch.long),
        "numeric": torch.zeros((1, 45)),
        "history_ids": torch.zeros((1, 1), dtype=torch.long),
        "inventory_ids": torch.tensor([[4, 5]], dtype=torch.long),
        "candidate_identity_ids": torch.tensor([[6]], dtype=torch.long),
        "action_numeric": torch.zeros((1, 12)),
    }
    model.population_adapter_scale = 0.0
    baseline = model(**inputs)
    model.population_adapter_scale = 1.0
    noncombat = model(**inputs)

    assert torch.equal(noncombat, baseline)

    inputs["numeric"][:, ENEMY_MAX_HP_MEASUREMENT_INDEX] = 1.0
    model.population_adapter_scale = 0.0
    combat_baseline = model(**inputs)
    model.population_adapter_scale = 1.0
    corrected = model(**inputs)

    assert torch.equal(corrected[:, :-1], combat_baseline[:, :-1])
    assert torch.allclose(corrected[:, -1], combat_baseline[:, -1] + 1.0)
