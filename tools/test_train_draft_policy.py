import unittest

from tools import train_draft_policy as draft


def observation() -> dict:
    return {
        "phase": "card_reward",
        "engine_screen": "card_reward",
        "source": "normal_card_reward",
        "act": 1,
        "hp": 60,
        "max_hp": 75,
        "gold": 100,
        "energy_master": 3,
        "opportunities_remaining": 20,
        "deck": [
            {"id": "Strike_B", "upgraded": False},
            {"id": "Zap", "upgraded": True},
        ],
        "relics": [{"id": "Cracked Core"}],
        "shop_purchase_slots_remaining": 0,
        "offers": [
            {
                "action_index": 0,
                "action": {
                    "kind": "game",
                    "action": {"op": "choose", "index": 0, "label": "Ball Lightning"},
                },
            },
            {
                "action_index": 1,
                "action": {
                    "kind": "game",
                    "action": {"op": "choose", "index": 1, "label": "Claw"},
                },
            },
        ],
    }


class DraftPolicyTests(unittest.TestCase):
    def test_offer_features_ignore_position_but_keep_identity(self) -> None:
        left = list(
            draft.flatten_offer(
                {"action_index": 7, "action": {"index": 2, "label": "Claw"}}
            )
        )
        right = list(
            draft.flatten_offer(
                {"action_index": 0, "action": {"index": 9, "label": "Claw"}}
            )
        )
        self.assertEqual(left, right)
        self.assertIn("action.label=Claw", left)

    def test_reach_and_wins_are_positive_reward_signals(self) -> None:
        base = {
            "wins": 0,
            "timeouts": 0,
            "boss_damage_dealt_sum": 100,
            "boss_hp_remaining_sum": 500,
            "player_hp_remaining_sum": 0,
        }
        farther = {**base, "boss_damage_dealt_sum": 200, "boss_hp_remaining_sum": 400}
        win = {**farther, "wins": 1}
        self.assertGreater(draft.suite_reward(farther), draft.suite_reward(base))
        self.assertGreater(draft.suite_reward(win), draft.suite_reward(farther))

    def test_all_three_act1_bosses_are_a_required_reward_gate(self) -> None:
        base = {
            "wins": 2,
            "timeouts": 0,
            "boss_damage_dealt_sum": 500,
            "boss_hp_remaining_sum": 100,
            "player_hp_remaining_sum": 20,
            "act1_wins": 2,
            "act1_all_won": False,
        }
        full_clear = {
            **base,
            "wins": 3,
            "act1_wins": 3,
            "act1_all_won": True,
        }
        self.assertGreater(
            draft.suite_reward(full_clear) - draft.suite_reward(base),
            1000.0,
        )

    def test_positive_advantage_increases_chosen_probability(self) -> None:
        policy = draft.HashedSoftmaxPolicy(256, 0.02)
        obs = observation()
        features, probabilities = policy.distribution(obs)
        self.assertAlmostEqual(probabilities[0], 0.5)
        trajectories = [
            [(features, probabilities, 0)],
            [(features, probabilities, 1)],
        ]
        policy.update(trajectories, [1.0, -1.0])
        _, updated = policy.distribution(obs)
        self.assertGreater(updated[0], 0.5)

    def test_parallel_gradient_statistics_match_direct_update(self) -> None:
        direct = draft.HashedSoftmaxPolicy(256, 0.02)
        aggregated = draft.HashedSoftmaxPolicy(256, 0.02)
        features, probabilities = direct.distribution(observation())
        trajectories = [
            [(features, probabilities, 0)],
            [(features, probabilities, 1)],
            [(features, probabilities, 0)],
        ]
        rewards = [3.0, -2.0, 1.0]

        direct_report = direct.update(trajectories, rewards)
        left = draft.policy_gradient_statistics(
            trajectories[:2], rewards[:2], direct.dimensions
        )
        right = draft.policy_gradient_statistics(
            trajectories[2:], rewards[2:], direct.dimensions
        )
        statistics = draft.merge_gradient_statistics([left, right])
        aggregated_report = aggregated.update_from_statistics(statistics)

        self.assertAlmostEqual(
            direct_report["gradient_norm"], aggregated_report["gradient_norm"]
        )
        for direct_weight, aggregated_weight in zip(
            direct.weights, aggregated.weights
        ):
            self.assertAlmostEqual(direct_weight, aggregated_weight)

    def test_parallel_summary_keeps_act1_gate_metrics(self) -> None:
        parts = [
            {
                "episodes": 2,
                "mean_reward": 10.0,
                "fights": 14,
                "wins": 5,
                "losses": 9,
                "timeouts": 0,
                "act1_wins": 4,
                "act1_full_clears": 1,
                "boss_hp_remaining_sum": 100,
                "boss_damage_dealt_sum": 200,
                "mean_decisions": 40.0,
            },
            {
                "episodes": 1,
                "mean_reward": 20.0,
                "fights": 7,
                "wins": 3,
                "losses": 4,
                "timeouts": 0,
                "act1_wins": 3,
                "act1_full_clears": 1,
                "boss_hp_remaining_sum": 50,
                "boss_damage_dealt_sum": 100,
                "mean_decisions": 43.0,
            },
        ]
        summary = draft.merge_summaries(parts)
        self.assertEqual(summary["act1_wins"], 7)
        self.assertEqual(summary["act1_full_clears"], 2)
        self.assertAlmostEqual(summary["act1_full_clear_rate"], 2 / 3)

    def test_imitation_increases_teacher_action_probability(self) -> None:
        policy = draft.HashedSoftmaxPolicy(256, 0.02)
        features, probabilities = policy.distribution(observation())
        report = policy.imitate([(features, probabilities, 1)])
        _, updated = policy.distribution(observation())
        self.assertEqual(report["imitation_accuracy"], 0.0)
        self.assertGreater(updated[1], 0.5)

    def test_repeated_noop_can_be_masked(self) -> None:
        policy = draft.HashedSoftmaxPolicy(256, 0.02)
        chosen, (_, probabilities, _) = policy.choose(
            observation(),
            draft.random.Random(1),
            greedy=True,
            forbidden={0},
        )
        self.assertEqual(chosen, 1)
        self.assertEqual(probabilities, [0.0, 1.0])

    def test_owned_card_and_its_count_change_relative_offer_score(self) -> None:
        policy = draft.HashedSoftmaxPolicy(32768, 0.02)
        target = policy._index("offer_deck:action.label=Claw|Claw")
        policy.weights[target] = 1.0
        no_claw = observation()
        one_claw = observation()
        one_claw["deck"].append({"id": "Claw", "upgraded": False})
        four_claws = observation()
        four_claws["deck"].extend(
            {"id": "Claw", "upgraded": False} for _ in range(4)
        )

        _, no_claw_probabilities = policy.distribution(no_claw)
        _, one_claw_probabilities = policy.distribution(one_claw)
        _, four_claw_probabilities = policy.distribution(four_claws)
        self.assertGreater(one_claw_probabilities[1], no_claw_probabilities[1])
        self.assertGreater(four_claw_probabilities[1], one_claw_probabilities[1])

    def test_owned_relic_changes_relative_offer_score(self) -> None:
        policy = draft.HashedSoftmaxPolicy(32768, 0.02)
        target = policy._index("offer_relic:action.label=Claw|Kunai")
        policy.weights[target] = 1.0
        without_relic = observation()
        with_relic = observation()
        with_relic["relics"].append({"id": "Kunai"})

        _, without_probabilities = policy.distribution(without_relic)
        _, with_probabilities = policy.distribution(with_relic)
        self.assertGreater(with_probabilities[1], without_probabilities[1])

    def test_seed_sets_are_reproducible_and_namespaced(self) -> None:
        first = draft.derived_seeds(5, "train", 3, 8)
        self.assertEqual(first, draft.derived_seeds(5, "train", 3, 8))
        self.assertNotEqual(first, draft.derived_seeds(5, "validation", 3, 8))


if __name__ == "__main__":
    unittest.main()
