import sys
from pathlib import Path
import unittest


sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "tools"))

from lockstep_exactsim import enriched_java_actions, transform_legal_actions


class ActionTransformerTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
