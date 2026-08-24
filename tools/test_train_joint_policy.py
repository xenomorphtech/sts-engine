import copy
import unittest

from tools.train_joint_policy import (
    CombatSoftmaxPolicy,
    default_deck_weights,
    fight_reward,
)


def combat_observation() -> dict:
    return {
        "boss_index": 3,
        "boss": "CorruptHeart",
        "screen": "Combat",
        "turn": 2,
        "player_hp": 48,
        "player_max_hp": 75,
        "player_block": 0,
        "energy": 3,
        "cards_played_this_turn": 0,
        "hand": [
            {
                "id": "Defend_B",
                "upgraded": False,
                "cost": 1,
                "cost_for_turn": 1,
            }
        ],
        "draw": [],
        "discard": [],
        "exhaust": [],
        "relics": [{"id": "Anchor", "counter": -1}],
        "potions": [],
        "powers": [],
        "orbs": [{"kind": "Lightning", "passive": 3, "evoke": 8}],
        "monsters": [
            {
                "index": 0,
                "id": "CorruptHeart",
                "hp": 750,
                "max_hp": 800,
                "block": 0,
                "dead": False,
                "escaped": False,
                "intent": "Attack",
                "intent_damage_per_hit": 15,
                "intent_hits": 3,
                "intent_total_damage": 45,
                "powers": [],
            }
        ],
        "offers": [
            {
                "label": "play:Defend_B:target:none",
                "action": {"PlayCard": {"hand_index": 0, "target": None}},
            },
            {"label": "end_turn", "action": "EndTurn"},
        ],
    }


class CombatPolicyFeatureTests(unittest.TestCase):
    def test_enemy_intent_and_exact_damage_change_candidate_features(self) -> None:
        policy = CombatSoftmaxPolicy(1 << 16, 0.01)
        attacking = combat_observation()
        buffing = copy.deepcopy(attacking)
        buffing["monsters"][0].update(
            {
                "intent": "Buff",
                "intent_damage_per_hit": 0,
                "intent_hits": 0,
                "intent_total_damage": 0,
            }
        )

        attack_features = policy.features(attacking, attacking["offers"][0])
        buff_features = policy.features(buffing, buffing["offers"][0])
        intent_index = policy._index(
            "combat_cross:label=play:Defend_B:target:none|"
            "enemy=0:CorruptHeart:intent=Attack"
        )
        damage_index = policy._index(
            "combat_cross:label=play:Defend_B:target:none|"
            "enemy=0:CorruptHeart:total_damage=45"
        )

        self.assertIn(intent_index, attack_features)
        self.assertIn(damage_index, attack_features)
        self.assertNotEqual(attack_features, buff_features)

    def test_intent_feature_can_change_relative_action_probability(self) -> None:
        policy = CombatSoftmaxPolicy(1 << 16, 0.01)
        observation = combat_observation()
        index = policy._index(
            "combat_cross:label=play:Defend_B:target:none|"
            "enemy=0:CorruptHeart:intent=Attack"
        )
        policy.weights[index] = 3.0

        attacking_probability = policy.distribution(observation)[1][0]
        observation["monsters"][0]["intent"] = "Buff"
        buffing_probability = policy.distribution(observation)[1][0]

        self.assertGreater(attacking_probability, buffing_probability)


class FightRewardTests(unittest.TestCase):
    def test_win_and_boss_damage_are_positive_signals(self) -> None:
        base = {
            "won": False,
            "timed_out": False,
            "boss_damage_dealt": 100,
            "boss_hp_remaining": 400,
            "player_hp_remaining": 10,
        }
        more_damage = {**base, "boss_damage_dealt": 200, "boss_hp_remaining": 300}
        win = {**more_damage, "won": True, "boss_hp_remaining": 0}

        self.assertGreater(fight_reward(more_damage), fight_reward(base))
        self.assertGreater(fight_reward(win), fight_reward(more_damage))


class DeckCheckpointSelectionTests(unittest.TestCase):
    def test_current_weights_are_the_default(self) -> None:
        checkpoint = {
            "dimensions": 2,
            "weights": [1.0, 2.0],
            "best_weights": [3.0, 4.0],
        }
        self.assertEqual(default_deck_weights(checkpoint), [1.0, 2.0])

    def test_explicit_legacy_best_remains_supported(self) -> None:
        checkpoint = {
            "dimensions": 2,
            "default_weight_source": "best_weights",
            "weights": [1.0, 2.0],
            "best_weights": [3.0, 4.0],
        }
        self.assertEqual(default_deck_weights(checkpoint), [3.0, 4.0])


if __name__ == "__main__":
    unittest.main()
