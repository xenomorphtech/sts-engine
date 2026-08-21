#!/usr/bin/env python3
"""Black-box evolution-strategy optimizer for HTN policy parameters.

Evaluates candidates with common random numbers (fixed --seed-source) on the
release sts-htn binary via STS_HTN_PARAMS. Fitness = wins + floor_weight *
mean_floor. Search runs on a 500-seed cohort; the incumbent mean is validated
periodically on the full 1k cohort plus a holdout source. State is resumable.
"""

import json
import math
import os
import random
import re
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "sts-htn")
STATE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "opt_state.json")
SEARCH_SOURCE = "1787315208666760241"
HOLDOUT_SOURCE = "1787329866870376876"
FLOOR_WEIGHT = 2.0

# name: (default, sigma, lo, hi)
SPEC = {
    "dmg_base": (6.0, 1.0, 1.0, 20.0),
    "dmg_per_turn": (4.5, 0.8, 0.5, 15.0),
    "danger_base": (20.0, 3.0, 2.0, 80.0),
    "danger_scale": (90.0, 12.0, 10.0, 300.0),
    "kill_bonus": (900.0, 120.0, 0.0, 3000.0),
    "strip_block_mult": (0.55, 0.1, 0.0, 2.0),
    "lethal_discount": (0.55, 0.08, 0.1, 1.0),
    "overblock_penalty": (0.8, 0.15, 0.0, 4.0),
    "energy_value": (1.5, 0.3, 0.0, 8.0),
    "strength_weight": (4.0, 0.7, 0.0, 15.0),
    "dexterity_weight": (3.0, 0.6, 0.0, 15.0),
    "focus_weight": (4.0, 0.7, 0.0, 15.0),
    "enemy_strength_penalty": (20.0, 3.0, 0.0, 80.0),
    "bias_decay_weight": (4.0, 0.7, 0.0, 15.0),
    "orb_horizon": (4.0, 0.5, 1.0, 10.0),
    "orb_lightning_mult": (0.8, 0.12, 0.0, 3.0),
    "orb_frost_mult": (1.0, 0.15, 0.0, 3.0),
    "orb_dark_stored": (0.45, 0.08, 0.0, 2.0),
    "orb_dark_growth": (0.0, 0.08, 0.0, 2.0),
    "orb_plasma": (12.0, 2.0, 0.0, 40.0),
    "elite_afford_hp": (0.65, 0.04, 0.3, 0.95),
    "elite_strength_base": (3.0, 0.6, -4.0, 10.0),
    "elite_strength_slope": (2.0, 0.4, 0.0, 6.0),
    "elite_value": (40.0, 6.0, 0.0, 150.0),
    "elite_penalty": (-150.0, 20.0, -400.0, 0.0),
    "elite_hp_floor": (0.4, 0.03, 0.1, 0.7),
    "rest_low_value": (35.0, 5.0, 0.0, 120.0),
    "rest_high_value": (18.0, 3.0, 0.0, 80.0),
    "rest_preboss_value": (50.0, 7.0, 0.0, 150.0),
    "shop_gold_div": (5.0, 0.8, 1.0, 20.0),
    "treasure_value": (25.0, 4.0, 0.0, 100.0),
    "event_value": (14.0, 2.5, 0.0, 80.0),
    "monster_ok_value": (15.0, 2.5, -20.0, 60.0),
    "monster_low_value": (-15.0, 3.0, -80.0, 20.0),
    "rest_hp_act1": (0.7, 0.04, 0.3, 0.95),
    "rest_hp_later": (0.78, 0.04, 0.3, 0.97),
    "rest_hp_preboss": (0.85, 0.04, 0.4, 0.99),
    "pick_threshold": (85.0, 8.0, 30.0, 200.0),
}
NAMES = list(SPEC)

SUMMARY = re.compile(r"wins=(\d+).*win_rate=([\d.]+)%.*mean_floor_achieved=([\d.]+)")


def evaluate(theta, count, source, concurrent=12):
    path = os.path.join("/tmp", f"htn_params_{os.getpid()}.json")
    with open(path, "w") as fh:
        json.dump(dict(zip(NAMES, theta)), fh)
    env = dict(os.environ, STS_HTN_PARAMS=path)
    out = subprocess.run(
        [BIN, "--character", "DEFECT", "--a0", "--count", str(count),
         "--concurrent", str(concurrent), "--seed-source", source],
        env=env, capture_output=True, text=True, timeout=1200,
    ).stdout
    m = SUMMARY.search(out)
    if not m:
        raise RuntimeError(f"no summary in output: {out[:500]}")
    wins, rate, floor = int(m.group(1)), float(m.group(2)), float(m.group(3))
    scale = 1000 / count
    return wins * scale + FLOOR_WEIGHT * floor, wins, rate, floor


def clamp(name, v):
    _, _, lo, hi = SPEC[name]
    return min(max(v, lo), hi)


def main():
    generations = int(sys.argv[1]) if len(sys.argv) > 1 else 40
    lam, mu = 10, 3
    if os.path.exists(STATE):
        st = json.load(open(STATE))
    else:
        st = {
            "mean": [SPEC[n][0] for n in NAMES],
            "sigma_mult": 1.0,
            "gen": 0,
            "history": [],
            "best": None,
        }
    rng = random.Random(12345 + st["gen"] * 977)
    base_fit, w, r, f = evaluate(st["mean"], 500, SEARCH_SOURCE)
    print(f"gen {st['gen']} incumbent fit={base_fit:.1f} wins500={w} floor={f:.2f}", flush=True)
    for g in range(generations):
        cands = []
        for _ in range(lam):
            theta = [
                clamp(n, m + st["sigma_mult"] * SPEC[n][1] * rng.gauss(0, 1))
                for n, m in zip(NAMES, st["mean"])
            ]
            fit, w, r, f = evaluate(theta, 500, SEARCH_SOURCE)
            cands.append((fit, theta, w, f))
        cands.sort(key=lambda c: -c[0])
        elite = cands[:mu]
        new_mean = [sum(c[1][i] for c in elite) / mu for i in range(len(NAMES))]
        new_fit, w, r, f = evaluate(new_mean, 500, SEARCH_SOURCE)
        improved = new_fit > base_fit
        if improved:
            st["mean"], base_fit = new_mean, new_fit
            st["sigma_mult"] = min(st["sigma_mult"] * 1.15, 2.5)
        else:
            st["sigma_mult"] = max(st["sigma_mult"] * 0.85, 0.25)
        st["gen"] += 1
        st["history"].append(
            {"gen": st["gen"], "fit": new_fit, "wins500": w, "floor": f,
             "kept": improved, "sigma": st["sigma_mult"],
             "top": cands[0][0]}
        )
        print(
            f"gen {st['gen']} top={cands[0][0]:.1f} mean_fit={new_fit:.1f} "
            f"kept={improved} sigma={st['sigma_mult']:.2f}", flush=True,
        )
        if st["gen"] % 5 == 0:
            fit1k, w1, r1, f1 = evaluate(st["mean"], 1000, SEARCH_SOURCE)
            fith, wh, rh, fh = evaluate(st["mean"], 1000, HOLDOUT_SOURCE)
            st["history"][-1]["val_search_1k"] = [w1, f1]
            st["history"][-1]["val_holdout_1k"] = [wh, fh]
            print(
                f"  validate: search1k {w1} wins {f1:.2f} floor | "
                f"holdout1k {wh} wins {fh:.2f} floor", flush=True,
            )
            if st["best"] is None or w1 + wh > st["best"]["score"]:
                st["best"] = {"score": w1 + wh, "mean": st["mean"],
                              "search": [w1, f1], "holdout": [wh, fh]}
        json.dump(st, open(STATE, "w"), indent=1)
    print("done", flush=True)


if __name__ == "__main__":
    main()
