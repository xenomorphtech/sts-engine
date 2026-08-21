#!/usr/bin/env python3
"""Rewrite Params::default() in src/htn/params.rs from the ES state's best
validated mean (fallback: current mean), and write tools/params_best.json."""

import json
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "tools"))
from opt_params import NAMES  # noqa: E402

st = json.load(open(os.path.join(ROOT, "tools", "opt_state.json")))
mean = (st.get("best") or {}).get("mean") or st["mean"]
vals = dict(zip(NAMES, mean))
json.dump(vals, open(os.path.join(ROOT, "tools", "params_best.json"), "w"), indent=1)

path = os.path.join(ROOT, "src", "htn", "params.rs")
src = open(path).read()
for name, v in vals.items():
    pat = re.compile(rf"(\b{name}: )(-?[0-9.]+)(,)")
    m = pat.search(src)
    assert m, name
    src = pat.sub(rf"\g<1>{v:.4f}\g<3>", src, count=1)
open(path, "w").write(src)
print(f"baked {len(vals)} params into {path}")
