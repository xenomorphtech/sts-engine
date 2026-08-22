import sys
from pathlib import Path
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from lockstep_exactsim import (
    enriched_java_actions,
    neow_label_matches,
    terminally_aligned,
    transform_legal_actions,
)


class ActionTransformerTests(unittest.TestCase):
    def test_three_enemy_kill_accepts_java_spelled_number(self):
        self.assertTrue(
            neow_label_matches(
                "ThreeEnemyKill",
                "Enemies in your next #gthree combats have #g1 HP.",
            )
        )

    def test_internal_sts_ids_match_java_shop_display_names(self):
        aliases = {
            "Boot": "The Boot",
            "Conserve Battery": "Charge Battery",
            "FairyPotion": "Fairy in a Bottle",
            "Frozen Egg 2": "Frozen Egg",
            "Gash": "Claw",
            "Lockon": "Bullseye",
            "Molten Egg 2": "Molten Egg",
            "Redo": "Recursion",
            "Sling": "Sling of Courage",
            "Steam": "Steam Barrier",
            "Steam Power": "Overclock",
            "SteroidPotion": "Flex Potion",
            "Toxic Egg 2": "Toxic Egg",
        }
        for rust_label, java_label in aliases.items():
            with self.subTest(rust_label=rust_label, java_label=java_label):
                mapping, error = transform_legal_actions(
                    [{"op": "choose", "index": 2, "label": rust_label}],
                    [{"op": "choose", "index": 2, "label": java_label}],
                )
                self.assertIsNone(error)
                self.assertEqual(mapping[0]["reason"], "sts-id-display-name")

        mapping, error = transform_legal_actions(
            [{"op": "choose", "index": 5, "label": "Conserve Battery"}],
            [{"op": "choose", "index": 5, "label": "Charge Battery+"}],
        )
        self.assertIsNone(error)
        self.assertEqual(mapping[0]["reason"], "sts-id-display-name")

    def test_java_upgrade_marker_is_display_only_for_shop_actions(self):
        mapping, error = transform_legal_actions(
            [{"op": "choose", "index": 1, "label": "Heatsinks"}],
            [{"op": "choose", "index": 1, "label": "Heatsinks+"}],
        )

        self.assertIsNone(error)
        self.assertEqual(mapping[0]["reason"], "upgrade-display-name")

    def test_proceed_and_skip_keep_their_exact_semantics(self):
        mapping, error = transform_legal_actions(
            [{"op": "proceed"}, {"op": "skip"}],
            [{"op": "proceed"}, {"op": "skip"}],
        )

        self.assertIsNone(error)
        self.assertEqual([entry["java"]["op"] for entry in mapping], ["proceed", "skip"])

    def test_disjoint_proceed_skip_vocabulary_is_a_mismatch(self):
        mapping, error = transform_legal_actions([{"op": "proceed"}], [{"op": "skip"}])

        self.assertIsNone(mapping)
        self.assertEqual(error["reason"], "legal_action_bijection")

    def test_extra_action_stops_the_bijection(self):
        mapping, error = transform_legal_actions(
            [{"op": "proceed"}],
            [{"op": "proceed"}, {"op": "skip"}],
        )

        self.assertIsNone(mapping)
        self.assertEqual(error["reason"], "legal_action_count")

    def test_snapshot_filtered_discards_are_restored(self):
        observation = {
            "legal_actions": [{"op": "proceed"}],
            "state": {
                "player": {
                    "potions": [
                        {"id": "EssenceOfDarkness"},
                        {"id": "Potion Slot"},
                    ]
                },
                "room": {"event": {}},
            },
        }

        self.assertEqual(
            enriched_java_actions(observation)[1],
            {
                "op": "potion",
                "action": "discard",
                "slot": 0,
                "potion_id": "EssenceOfDarkness",
                "_snapshot_filtered": True,
            },
        )

    def test_we_meet_again_does_not_restore_discards(self):
        observation = {
            "legal_actions": [{"op": "proceed"}],
            "state": {
                "player": {"potions": [{"id": "EssenceOfDarkness"}]},
                "room": {
                    "event": {
                        "class": "com.megacrit.cardcrawl.events.shrines.WeMeetAgain"
                    }
                },
            },
        }

        self.assertEqual(enriched_java_actions(observation), [{"op": "proceed"}])

    def test_matching_death_boundaries_do_not_require_a_quit_decision(self):
        java_death = {
            "boundary": "death",
            "legal_actions": [{"op": "quit"}],
            "state": {
                "player": {"potions": [{"id": "EssenceOfSteel"}]},
                "room": {"event": {}},
            },
        }
        self.assertTrue(
            terminally_aligned(
                {"done": True, "decision": None, "legal_actions": [{"op": "quit"}]},
                java_death,
            )
        )
        self.assertEqual(len(enriched_java_actions(java_death)), 2)
        self.assertFalse(
            terminally_aligned(
                {"done": True, "decision": None},
                {"boundary": "combat", "legal_actions": [{"op": "end_turn"}]},
            )
        )


if __name__ == "__main__":
    unittest.main()
