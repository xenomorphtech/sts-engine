#!/usr/bin/env python3
"""Phase-2 ES: policy scalars + drafting tables (pick/upgrade/boss-relic
overrides, deck-shape bonuses, fight lengths). Fresh search cohort to avoid
the phase-1-adapted one. Nested keys: pick.X / upgrade.X / bossrelic.X."""

import json
import os
import random
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "sts-htn")
STATE = os.path.join(ROOT, "tools", "opt_state2.json")
SEARCH_SOURCE = "1787332612345678901"
HOLDOUT_SOURCE = "1787315208666760241"
FLOOR_WEIGHT = 2.0

BAKED = json.load(open(os.path.join(ROOT, "tools", "params_best.json")))

SPEC = {}
for name, v in BAKED.items():
    lo, hi = (0.0, 1.0) if 0 <= v <= 1 else (min(v * 0.3, v * 2.2), max(v * 0.3, v * 2.2))
    sigma = max(abs(v) * 0.12, 0.03)
    SPEC[name] = (v, sigma, lo, hi)

DRAFT_SCALARS = {
    "upgraded_pick_bonus": 25, "copies_full_penalty": 250, "copies_near_penalty": 40,
    "aoe_bonus": 45, "block_bonus": 40, "channel_bonus": 35, "scaling_bonus": 40,
    "focus_bonus": 45, "act1_attack_bonus": 55, "act1_big_damage_bonus": 25,
    "act2_damage_bonus": 45, "act2_finisher_bonus": 30, "act1_late_block_bonus": 35,
    "size_full_penalty": 160, "size_near_penalty": 60,
    "target_size_act1": 22, "target_size_act2": 26, "target_size_act3": 28,
}
for name, v in DRAFT_SCALARS.items():
    if name.startswith("target_size"):
        SPEC[name] = (float(v), 1.2, 14.0, 40.0)
    else:
        SPEC[name] = (float(v), max(v * 0.15, 4.0), 0.0, v * 3.0 + 40.0)

FIGHT_LENGTHS = {
    "fl_a1_normal": 3.3, "fl_a1_elite": 5.3, "fl_a1_boss": 9.5,
    "fl_a2_normal": 5.0, "fl_a2_elite": 4.5, "fl_a2_boss": 8.0,
    "fl_a3_normal": 5.5, "fl_a3_elite": 6.0, "fl_a3_boss": 10.0,
    "fl_a4_normal": 5.0, "fl_a4_elite": 6.0, "fl_a4_boss": 12.0,
}
for name, v in FIGHT_LENGTHS.items():
    SPEC[name] = (v, 0.6, 1.0, 20.0)

PICK = {
    "Defragment": 290, "Echo Form": 280, "Electrodynamics": 240, "Glacier": 240,
    "Coolheaded": 190, "Cold Snap": 160, "Ball Lightning": 180, "Loop": 170,
    "Conserve Battery": 160, "Skim": 200, "BootSequence": 150, "Biased Cognition": 150,
    "Capacitor": 140, "Buffer": 160, "Self Repair": 260, "Machine Learning": 170,
    "Compile Driver": 140, "Sweeping Beam": 155, "Doom and Gloom": 170, "Blizzard": 170,
    "Melter": 120, "Rip and Tear": 110, "Sunder": 140, "Hyperbeam": 130,
    "Core Surge": 140, "FTL": 130, "Streamline": 100, "Barrage": 60,
    "Thunder Strike": 90, "All For One": 100, "Stack": 80, "Go for the Eyes": 115,
    "Auto Shields": 120, "Reinforced Body": 110, "Force Field": 70, "Chill": 90,
    "Chaos": 70, "Turbo": 90, "Fusion": 70, "Double Energy": 90, "Consume": 90,
    "Heatsinks": 90, "Static Discharge": 60, "Storm": 110, "Creative AI": 90,
    "Seek": 150, "Reprogram": 30, "White Noise": 60, "Rainbow": 60, "Tempest": 90,
    "Meteor Strike": 60, "Zap": 40, "Dualcast": 40, "Leap": 90, "Rebound": 70,
    "Scrape": 60, "Beam Cell": 70, "Genetic Algorithm": 40, "Hologram": 60,
    "Recycle": 40, "Darkness": 50, "Gash": 40, "Strike_B": 20, "Defend_B": 20,
    "Aggregate": 40, "Hello World": 40, "Multi-Cast": 40, "Amplify": 40,
    "Reboot": 40, "Fission": 40, "Steam Power": 40, "Redo": 40,
}
for name, v in PICK.items():
    SPEC[f"pick.{name}"] = (float(v), 16.0, 0.0, 360.0)

UPGRADE = {
    "Defragment": 270, "Echo Form": 260, "Glacier": 240, "Electrodynamics": 230,
    "Skim": 210, "Coolheaded": 200, "Loop": 200, "Self Repair": 190,
    "Ball Lightning": 180, "Blizzard": 180, "Buffer": 170, "Doom and Gloom": 170,
    "Defend_B": 40, "Strike_B": 30,
}
for name, v in UPGRADE.items():
    SPEC[f"upgrade.{name}"] = (float(v), 14.0, 0.0, 350.0)

BOSS_RELIC = {
    "SlaversCollar": 95, "Velvet Choker": 88, "Cursed Key": 82, "FrozenCore": 72,
    "Nuclear Battery": 70, "Runic Pyramid": 76, "Coffee Dripper": 75,
    "Fusion Hammer": 72, "Tiny House": 60, "Busted Crown": 20, "Snecko Eye": 15,
    "Runic Dome": 5, "Calling Bell": 8, "Astrolabe": 55, "Pandora's Box": 55,
    "Empty Cage": 55, "Sacred Bark": 55, "Ectoplasm": 55, "Sozu": 55,
}
for name, v in BOSS_RELIC.items():
    SPEC[f"bossrelic.{name}"] = (float(v), 9.0, 0.0, 160.0)

NAMES = list(SPEC)
SUMMARY = re.compile(r"wins=(\d+).*win_rate=([\d.]+)%.*mean_floor_achieved=([\d.]+)")


def to_json(theta):
    out = {"pick": {}, "upgrade": {}, "boss_relic": {}}
    for name, v in zip(NAMES, theta):
        if name.startswith("pick."):
            out["pick"][name[5:]] = v
        elif name.startswith("upgrade."):
            out["upgrade"][name[8:]] = v
        elif name.startswith("bossrelic."):
            out["boss_relic"][name[10:]] = v
        else:
            out[name] = v
    return out


def evaluate(theta, count, source, concurrent=12):
    path = os.path.join("/tmp", f"htn_params2_{os.getpid()}.json")
    with open(path, "w") as fh:
        json.dump(to_json(theta), fh)
    env = dict(os.environ, STS_HTN_PARAMS=path)
    out = subprocess.run(
        [BIN, "--character", "DEFECT", "--a0", "--count", str(count),
         "--concurrent", str(concurrent), "--seed-source", source],
        env=env, capture_output=True, text=True, timeout=1800,
    ).stdout
    m = SUMMARY.search(out)
    if not m:
        raise RuntimeError(f"no summary: {out[:400]}")
    wins, floor = int(m.group(1)), float(m.group(3))
    return wins * (1000 / count) + FLOOR_WEIGHT * floor, wins, floor


def clamp(name, v):
    _, _, lo, hi = SPEC[name]
    return min(max(v, lo), hi)


def main():
    generations = int(sys.argv[1]) if len(sys.argv) > 1 else 120
    lam, mu = 12, 3
    if os.path.exists(STATE):
        st = json.load(open(STATE))
    else:
        st = {"mean": [SPEC[n][0] for n in NAMES], "sigma_mult": 0.9,
              "gen": 0, "history": [], "best": None}
    rng = random.Random(777 + st["gen"] * 131)
    base_fit, w, f = evaluate(st["mean"], 500, SEARCH_SOURCE)
    print(f"start gen {st['gen']} fit={base_fit:.1f} wins500={w} floor={f:.2f}", flush=True)
    for _ in range(generations):
        cands = []
        for _ in range(lam):
            theta = [
                clamp(n, m + st["sigma_mult"] * SPEC[n][1] * rng.gauss(0, 1))
                for n, m in zip(NAMES, st["mean"])
            ]
            fit, w, f = evaluate(theta, 500, SEARCH_SOURCE)
            cands.append((fit, theta, w, f))
        cands.sort(key=lambda c: -c[0])
        elite = cands[:mu]
        new_mean = [sum(c[1][i] for c in elite) / mu for i in range(len(NAMES))]
        new_fit, w, f = evaluate(new_mean, 500, SEARCH_SOURCE)
        if new_fit > base_fit:
            st["mean"], base_fit = new_mean, new_fit
            st["sigma_mult"] = min(st["sigma_mult"] * 1.12, 2.0)
            kept = True
        else:
            st["sigma_mult"] = max(st["sigma_mult"] * 0.87, 0.2)
            kept = False
        st["gen"] += 1
        st["history"].append({"gen": st["gen"], "fit": new_fit, "wins500": w,
                              "floor": f, "kept": kept, "sigma": st["sigma_mult"]})
        print(f"gen {st['gen']} top={cands[0][0]:.1f} mean={new_fit:.1f} kept={kept} "
              f"sigma={st['sigma_mult']:.2f}", flush=True)
        if st["gen"] % 5 == 0:
            _, w1, f1 = evaluate(st["mean"], 1000, SEARCH_SOURCE)
            _, wh, fh = evaluate(st["mean"], 1000, HOLDOUT_SOURCE)
            st["history"][-1]["val_search_1k"] = [w1, f1]
            st["history"][-1]["val_holdout_1k"] = [wh, fh]
            print(f"  validate search1k={w1}/{f1:.2f} holdout1k={wh}/{fh:.2f}", flush=True)
            if st["best"] is None or w1 + wh > st["best"]["score"]:
                st["best"] = {"score": w1 + wh, "mean": st["mean"],
                              "search": [w1, f1], "holdout": [wh, fh]}
        json.dump(st, open(STATE, "w"), indent=1)
    print("done", flush=True)


if __name__ == "__main__":
    main()
