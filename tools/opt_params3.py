#!/usr/bin/env python3
"""Phase-3 ES. Fixes phase-2's stalls and cohort adaptation:

- pure (mu/mu, lambda) recombination each generation (no sticky incumbent),
- mirrored (antithetic) sampling,
- a different random 500-seed cohort every generation, so fitness targets the
  true win rate instead of one memorized cohort,
- adds search width/depth and potion thresholds to the vector.

Start mean comes from tools/params_default.json (phase-2 bake).
"""

import json
import os
import random
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BIN = os.path.join(ROOT, "target", "release", "sts-htn")
STATE = os.path.join(ROOT, "tools", "opt_state3.json")
VAL_SOURCES = ["1787336748186657062", "1787329866870376876"]
FLOOR_WEIGHT = 2.0

BAKED = json.load(open(os.path.join(ROOT, "tools", "params_default.json")))

SPEC = {}


def add(name, v, sigma, lo, hi):
    SPEC[name] = (float(v), float(sigma), float(lo), float(hi))


for name, v in BAKED.items():
    if name in ("pick", "upgrade", "boss_relic"):
        prefix = {"pick": "pick.", "upgrade": "upgrade.", "boss_relic": "bossrelic."}[name]
        sig = {"pick.": 14.0, "upgrade.": 12.0, "bossrelic.": 8.0}[prefix]
        hi = {"pick.": 360.0, "upgrade.": 350.0, "bossrelic.": 160.0}[prefix]
        for card, cv in v.items():
            add(prefix + card, cv, sig, 0.0, hi)
    elif 0.0 <= v <= 1.0:
        add(name, v, max(abs(v) * 0.10, 0.02), 0.0, 1.0)
    else:
        lo, hi = min(v * 0.3, v * 2.2), max(v * 0.3, v * 2.2)
        add(name, v, max(abs(v) * 0.10, 0.03), lo, hi)

add("search_width", 8.0, 0.8, 5.0, 14.0)
add("search_depth", 6.0, 0.6, 4.0, 9.0)
add("potion_desperate_hp_div", 8.0, 0.8, 3.0, 16.0)
add("potion_defense_hp_div", 3.0, 0.3, 1.5, 8.0)
add("potion_heal_hp_frac", 0.5, 0.05, 0.15, 0.9)
add("potion_block_min", 12.0, 1.5, 3.0, 30.0)
add("potion_block_hp_div", 4.0, 0.4, 1.5, 10.0)

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


def evaluate(theta, count, source, tag, concurrent=12):
    path = os.path.join("/tmp", f"htn_p3_{os.getpid()}_{tag}.json")
    with open(path, "w") as fh:
        json.dump(to_json(theta), fh)
    env = dict(os.environ, STS_HTN_PARAMS=path)
    out = subprocess.run(
        [BIN, "--character", "DEFECT", "--a0", "--count", str(count),
         "--concurrent", str(concurrent), "--seed-source", str(source)],
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
    generations = int(sys.argv[1]) if len(sys.argv) > 1 else 80
    lam = 12  # mirrored: 6 pairs
    mu = 4
    if os.path.exists(STATE):
        st = json.load(open(STATE))
    else:
        st = {"mean": [SPEC[n][0] for n in NAMES], "sigma_mult": 0.6,
              "gen": 0, "history": [], "best": None}
    rng = random.Random(4242 + st["gen"] * 313)
    for _ in range(generations):
        gen_source = 1787000000000000000 + (st["gen"] * 7919) % 999999937
        cands = []
        for _ in range(lam // 2):
            eps = [rng.gauss(0, 1) for _ in NAMES]
            for sign in (1.0, -1.0):
                theta = [
                    clamp(n, m + sign * st["sigma_mult"] * SPEC[n][1] * e)
                    for n, m, e in zip(NAMES, st["mean"], eps)
                ]
                fit, w, f = evaluate(theta, 500, gen_source, "c")
                cands.append((fit, theta, w, f))
        cands.sort(key=lambda c: -c[0])
        elite = cands[:mu]
        weights = [mu - i for i in range(mu)]
        tot = sum(weights)
        st["mean"] = [
            sum(wt * c[1][i] for wt, c in zip(weights, elite)) / tot
            for i in range(len(NAMES))
        ]
        st["gen"] += 1
        st["history"].append({
            "gen": st["gen"], "top": cands[0][0], "top_wins500": cands[0][2],
            "median": cands[lam // 2][0], "sigma": st["sigma_mult"],
        })
        print(f"gen {st['gen']} top={cands[0][0]:.1f} median={cands[lam//2][0]:.1f} "
              f"sigma={st['sigma_mult']:.2f}", flush=True)
        if st["gen"] % 6 == 0:
            vals = []
            for i, src in enumerate(VAL_SOURCES):
                _, w1, f1 = evaluate(st["mean"], 1000, src, f"v{i}")
                vals.append([w1, f1])
            st["history"][-1]["val"] = vals
            print(f"  validate {vals}", flush=True)
            score = sum(v[0] for v in vals)
            if st["best"] is None or score > st["best"]["score"]:
                st["best"] = {"score": score, "mean": st["mean"], "vals": vals}
        json.dump(st, open(STATE, "w"), indent=1)
    print("done", flush=True)


if __name__ == "__main__":
    main()
