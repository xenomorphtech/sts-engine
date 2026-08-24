#!/usr/bin/env python3

import json
import math
from pathlib import Path
import tempfile
import unittest

import opt_params4 as es


class EsV4Tests(unittest.TestCase):
    def test_rank_utilities_are_signed_ordered_and_zero_sum(self):
        utilities = es.rank_utilities([4.0, 1.0, 3.0, 2.0])
        self.assertGreater(utilities[0], utilities[2])
        self.assertGreater(utilities[2], utilities[3])
        self.assertGreaterEqual(utilities[3], utilities[1])
        self.assertGreater(utilities[0], 0.0)
        self.assertLess(utilities[1], 0.0)
        self.assertAlmostEqual(sum(utilities), 0.0)

    def test_rank_utilities_do_not_break_ties_arbitrarily(self):
        utilities = es.rank_utilities([1.0, 5.0, 5.0, 0.0])
        self.assertAlmostEqual(utilities[1], utilities[2])

    def test_snes_moves_mean_toward_better_mirror(self):
        space = es.ParamSpace({"weight": 10.0})
        utilities = es.rank_utilities([2.0, 1.0, 0.0, -1.0])
        mean, sigmas = es.snes_update(
            space,
            [10.0],
            [1.0],
            [[1.0], [-1.0], [0.5], [-0.5]],
            utilities,
            [0],
            global_scale=1.0,
            mean_rate=1.0,
            sigma_rate=0.06,
        )
        self.assertGreater(mean[0], 10.0)
        self.assertTrue(math.isfinite(sigmas[0]))

    def test_snes_leaves_inactive_dimensions_frozen(self):
        space = es.ParamSpace({"a": 10.0, "task": {"b": 10.0}})
        mean, sigmas = es.snes_update(
            space,
            [10.0, 10.0],
            [1.0, 1.0],
            [[1.0, 1.0], [-1.0, -1.0]],
            [0.5, -0.5],
            [0],
            global_scale=1.0,
            mean_rate=1.0,
            sigma_rate=0.06,
        )
        self.assertGreater(mean[0], 10.0)
        self.assertEqual(mean[1], 10.0)
        self.assertEqual(sigmas[1], 1.0)

    def test_tail_average_never_argmaxes(self):
        self.assertEqual(es.tail_average([[0.0, 3.0], [2.0, 1.0], [4.0, 2.0]], 2), [3.0, 1.5])

    def test_stages_activate_patterns_cumulatively(self):
        schedule = [
            {"generation": 0, "patterns": [r"^base$"]},
            {"generation": 10, "patterns": [r"^task\."]},
        ]
        names = ["base", "task.frost", "boss.hex"]
        self.assertEqual(es.active_dimensions(names, 0, schedule), [0])
        self.assertEqual(es.active_dimensions(names, 10, schedule), [0, 1])

    def test_global_schedule_waits_a_full_window_after_each_step(self):
        state = {
            "ascension": 20,
            "generation": 30,
            "global_levels": [1.0, 0.6, 0.45],
            "global_level": 0,
            "last_global_change": 0,
            "validations": [
                {"generation": 18, "win_rate": 0.04},
                {"generation": 30, "win_rate": 0.04},
            ],
        }
        self.assertTrue(es.maybe_anneal_global(state, 30, 12, 0.004))
        state["generation"] = 36
        state["validations"].append({"generation": 36, "win_rate": 0.04})
        self.assertFalse(es.maybe_anneal_global(state, 30, 12, 0.004))

    def test_global_schedule_holds_while_reach_improves(self):
        state = {
            "ascension": 20,
            "generation": 30,
            "global_levels": [1.0, 0.6],
            "global_level": 0,
            "last_global_change": 0,
            "validations": [
                {"generation": 18, "win_rate": 0.01, "anneal_score": 0.02},
                {"generation": 30, "win_rate": 0.01, "anneal_score": 0.025},
            ],
        }
        self.assertFalse(es.maybe_anneal_global(state, 30, 12, 0.004))

    def test_batch_output_and_paired_mcnemar(self):
        candidate = es.parse_batch_output(
            "character=Defect asc=20 seeds=3 concurrent=1 cohort=random seed_source=1 "
            "wins=2 losses=1 capped=0 stopped=0 win_rate=66.67% max_floor_achieved=57 "
            "mean_floor_achieved=50.00 a20_second_boss_entries=1 "
            "mean_a20_second_boss_entry_hp_fraction=0.1000 a20_second_boss_clears=1 "
            "final_boss_entries=1 "
            "mean_final_boss_entry_hp_fraction=0.2500 last_boss_fights=1 "
            "last_boss_remaining_hp_sum=42 mean_last_boss_damage_fraction=0.2000 "
            "steps=3 max_steps=5000 "
            "elapsed=1s seeds/s=3 steps/s=3\n"
            "seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining\n"
            "1\twin\t57\t-\n2\tloss\t56\t-\n3\twin\t57\t-\n"
        )
        baseline = es.parse_batch_output(
            "character=Defect asc=20 seeds=3 concurrent=1 cohort=random seed_source=1 "
            "wins=1 losses=2 capped=0 stopped=0 win_rate=33.33% max_floor_achieved=57 "
            "mean_floor_achieved=49.00 steps=3 max_steps=5000 elapsed=1s seeds/s=3 steps/s=3\n"
            "seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining\n"
            "1\twin\t57\t-\n2\tloss\t56\t-\n3\tloss\t56\t-\n"
        )
        report = es.mcnemar(candidate, baseline)
        self.assertEqual(candidate.wins, 2)
        self.assertAlmostEqual(candidate.floor_weight, 2.0)
        self.assertEqual(candidate.final_boss_entries, 1)
        self.assertAlmostEqual(candidate.mean_final_boss_entry_hp_fraction, 0.25)
        self.assertEqual(candidate.a20_second_boss_entries, 1)
        self.assertEqual(candidate.a20_second_boss_clears, 1)
        self.assertEqual(candidate.last_boss_remaining_hp_sum, 42)
        self.assertAlmostEqual(candidate.fitness, 2 / 3 * 1000 + 100 + 30 + 100 + 75 + 60)
        self.assertEqual(report["candidate_only_wins"], 1)
        self.assertEqual(report["baseline_only_wins"], 0)

    def test_mcnemar_handles_large_discordant_cohorts(self):
        candidate = es.Evaluation(
            1500, 900, 0, 0.0, {seed: seed < 900 for seed in range(1500)}
        )
        baseline = es.Evaluation(
            1500, 600, 0, 0.0, {seed: seed >= 900 for seed in range(1500)}
        )
        report = es.mcnemar(candidate, baseline)
        self.assertEqual(report["discordant"], 1500)
        self.assertGreater(report["exact_two_sided_p"], 0.0)
        self.assertLess(report["exact_two_sided_p"], 1.0)

    def test_capped_output_reports_the_exact_seed(self):
        output = (
            "character=Defect asc=20 seeds=2 concurrent=1 cohort=random seed_source=1 "
            "wins=0 losses=1 capped=1 stopped=0 win_rate=0.00% max_floor_achieved=12 "
            "mean_floor_achieved=10.00\n"
            "seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining\n"
            "42\tcapped\t12\t-\n43\tloss\t8\t-\n"
        )
        with self.assertRaisesRegex(RuntimeError, r"\[42\]"):
            es.parse_batch_output(output)

    def test_gauntlet_fitness_adds_clear_and_damage_signal(self):
        evaluation = es.Evaluation(
            count=10,
            wins=0,
            capped=0,
            mean_floor=20.0,
            outcomes={},
            gauntlet_count=10,
            gauntlet_wins=4,
            gauntlet_mean_damage_fraction=0.5,
            gauntlet_weight=0.35,
        )
        self.assertAlmostEqual(evaluation.fitness, 120.0 + 0.35 * (400.0 + 150.0))

    def test_second_boss_reach_is_a_positive_fitness_signal(self):
        evaluation = es.Evaluation(
            count=10,
            wins=0,
            capped=0,
            mean_floor=20.0,
            outcomes={},
            a20_second_boss_entries=2,
            reach_weight=1500.0,
        )
        self.assertAlmostEqual(evaluation.fitness, 120.0 + 300.0)

    def test_state_growth_appends_new_dimensions(self):
        with tempfile.TemporaryDirectory() as directory:
            initial_path = Path(directory) / "initial.json"
            grown_path = Path(directory) / "grown.json"
            initial_path.write_text(json.dumps({"base": 10.0}))
            grown_path.write_text(json.dumps({"base": 10.0, "task": {"frost": 2.0}}))
            initial = es.ParamSpace.load(initial_path)

            class Args:
                ascension = 20
                master_seed = 1
                tail_width = 8
                start_params = initial_path

            state = es.initial_state(initial, Args, [])
            state["tail_means"] = [[11.0]]
            aligned = es.align_state(state, es.ParamSpace.load(grown_path), 20)
            self.assertEqual(aligned["names"], ["base", "task.frost"])
            self.assertEqual(aligned["mean"], [10.0, 2.0])
            self.assertEqual(aligned["tail_means"], [[11.0, 2.0]])


if __name__ == "__main__":
    unittest.main()
