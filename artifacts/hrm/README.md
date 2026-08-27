# Combat HRM artifacts

Run the standard experiment from the repository root with no training
hyperparameters:

```sh
cargo run --release --bin sts-hrm-train
```

The frontend expands and verifies the checked-in 500-puzzle Defect A0 Act 3
boss fixture, caches the prepared tensors, trains for 600 wall-clock seconds,
evaluates puzzle-level train/validation/test splits, and exports an FP16 ONNX
policy plus its Rust runtime manifest. Large generated `.jsonl`, `.pt`, and
`.onnx` files are ignored by Git. Small metrics and summary JSON files are
retained as machine-readable records.

The promoted 26 August 2026 default run used an RTX 5060 and produced:

- 37,804 decision examples from 500 exact winning trajectories;
- a deterministic 400/50/50 boss-stratified puzzle split;
- 49,814 optimizer updates in 600.00 seconds;
- 44.47% held-out action accuracy versus 20.56% legal-frequency and 25.14%
  expected uniform-legal baselines;
- 16.53 HP held-out progress mean absolute error.

RNG words are encoded by byte position and high nibble. This reduces the
training vocabulary from 27,595 to 6,307 tokens and held-out unknowns from
roughly 26,000 per split to 1,959 validation / 2,379 test without increasing
sequence length. The saved embedding budget is invested in a 192-wide shared
model: 2,521,153 parameters rather than sparse seed-specific lookup rows.

Run closed-loop evaluation with:

```sh
cargo run --release --bin sts-hrm-eval
```

The native Rust/ONNX measured rollout produced 190 wins, 304 deaths, and six
detected Grid cycles: 38.00% overall wins and 34.00% on the held-out 50-puzzle
test split. There were no unknown-action fallbacks. The complete 500-puzzle run
took 29.64 seconds on the RTX 5060. Cycles remain visible as caps rather than
being counted as losses. The previous ten-minute model won 175/500 overall and
25/100 validation-plus-test puzzles; the promoted model wins 190/500 overall
and 35/100 validation-plus-test puzzles.

See `combat-hrm-10m-metrics.json` for offline imitation metrics,
`combat-hrm-10m-rollout-summary.json` for aggregate closed-loop results, and
`combat-hrm-10m-rollouts.tsv` or `.jsonl` for every seed. The research design is
in `docs/research/hrm-combat-training-reference.html`.

## Outcome-aware branch experiment

The native branch scorer forced every legal opening action in the 400 training
split puzzles and let the unchanged five-minute policy complete each fight. It
produced 5,384 counterfactual continuations: 1,692 wins, 3,606 losses, and 86
caps. All 400 states had distinct action outcomes, including 144 states where
no candidate won and monster-HP progress still supplied a ranking signal.

A conservative ten-second trial trained only the 12,672-parameter action head,
mixing soft branch preferences with the original imitation objective. Branch
best-action accuracy improved from 16.00% to 19.75%, but closed-loop wins fell
from 173 to 142. The checkpoint was rejected. A single global action-head update
also changes all later actions, while these counterfactual labels assume the old
policy for every continuation; this off-policy mismatch overwhelms the opening
improvement. The next experiment should collect branches throughout current
policy trajectories and refresh them between policy updates, or learn a
separate action-value/ranking head instead of directly replacing policy logits.

## Teacher-free full-run choice critic and exact lookahead

The full-run Defect A0 work changes the selection objective from boss-only
imitation accuracy to paired mean final floor. Boss reaches and wins remain
diagnostics, but do not override a positive or negative paired floor delta.
`tools/compare_selfplay_mean_floor.py` enforces identical seed cohorts and
reports the mean delta, its 95% confidence interval, and the improved,
unchanged, and regressed seed counts.

### Choice representation

Every action is scored in the context of the complete visible run state:

- current and maximum HP are independent numeric state channels, alongside
  floor, gold, energy, deck statistics, combat state, and enemy state;
- every owned card copy contributes both a base-card identity and an exact
  identity containing upgrade and mutable card fields; relics and occupied
  potions contribute identities in the same namespace;
- card and relic candidates use that same identity namespace. Candidate-to-
  inventory attention, elementwise interaction features, and explicit match
  counts let the critic associate a proposed choice with all owned cards and
  relics rather than learn a global value for the choice;
- Skip receives an explicit no-choice identity whenever it competes with an
  inventory choice, so it is evaluated against the same deck and relic state;
- deterministic non-combat choices expose typed numeric effects: HP, maximum
  HP, gold, deck size, upgraded-card count, relic count, and potion count
  deltas. Derived channels include HP loss as a fraction of current HP,
  maximum-HP change as a fraction of maximum HP, gold change as a fraction of
  current gold, and a lethal-HP-cost flag;
- the Rust engine obtains these effects by cloning the current state and
  applying only supported deterministic menu actions. Unsupported or combat
  actions set `known=false` instead of pretending that an unknown effect is a
  real zero.

The isolated `CounterfactualChoiceCritic` owns its embeddings and state memory,
and trains on complete legal-action menus with a seed-level train/validation
split. The parameterized run contained 131,137 training menus and 13,354 held
out by seed. Numeric action features can enter through an additive projection,
a zero-initialized gated menu residual, or both. A zero residual scale exactly
reproduces the underlying checkpoint, and older checkpoints load with zero
action-numeric channels.

### Training and checkpoint selection

`tools/train_selfplay_branch_rank.py` can create the isolated critic, add the
numeric action channels, fit exact branch-return menus, rank repeated rollouts
of the same seed by achieved floor, or imitate each seed's best teacher-free
rollout. The main trainer can also mix branch datasets and initialize from an
existing checkpoint.

Compare two closed-loop cohorts with:

```sh
python3 tools/compare_selfplay_mean_floor.py \
  artifacts/selfplay/defect-a0-generation15-seed-elite-cem-w0p6-heldout1000.jsonl \
  artifacts/selfplay/defect-a0-generation17-parameterized-w0p1-heldout1000.jsonl
```

On this fixed 1,000-seed cohort, adding the parameterized critic raised mean
floor from 15.182 to 15.718: a paired gain of 0.536 floors with 95% confidence
interval `[0.156, 0.916]`. It improved 350 seeds, left 363 unchanged, and
regressed 287. Later replay and residual-adapter checkpoints scored 15.679,
15.713, and 15.682 on the same gate and were rejected.

### Exact model-based policy improvement

`tools/eval_selfplay_hrm.py` can clone the Rust engine at a legal-action menu,
roll out several candidate actions, and use the learned policy/value model for
the continuation. Search horizons can differ by decision class: inventory
menus need enough depth to reveal deck-building consequences, while expensive
combat search is restricted to high-HP encounters. This remains teacher-free:
neither root choices nor continuations use the HTN policy.

The promoted combined configuration uses depth 64 for card/relic inventory
menus and depth 16 for combats whose visible maximum enemy HP is at least 100:

```sh
python3 tools/eval_selfplay_hrm.py \
  --checkpoint artifacts/selfplay/defect-a0-generation17-parameterized-seed-elite-cem-180s.pt \
  --count 100 \
  --seed-source 20261012 \
  --counterfactual-search-weight 0.1 \
  --counterfactual-outside-weight 0.1 \
  --lookahead-depth 16 \
  --lookahead-min-enemy-hp 100 \
  --lookahead-include-identity-choices \
  --lookahead-identity-depth 64 \
  --output artifacts/selfplay/defect-a0-generation27-combined-seed20261012.jsonl
```

On the paired development cohort, menu depth 64 scored 16.86 versus 15.75,
high-HP combat depth 16 scored 17.13 versus 14.83, and the combined policy
scored 20.39 versus the direct policy's 16.33. A separate depth-128 boss search
did not improve mean floor and worsened the inspected Donu/Deca suffix, so it
was rejected.

Across four fresh 100-seed cohorts (seed sources 20261011 through 20261014),
the combined policy averaged 19.005 floors. Twenty-four runs reached floor 33
or later: 19 reached 33, one reached 40, one reached 41, two reached 48, and one
won on floor 50. The winning seed was `-3542596578145849789`; it completed the
Act 3 boss after 962 decisions with 37/66 HP. This is evidence that the
teacher-free critic plus exact search can solve a complete run, not evidence of
a high win rate: the fresh-cohort result is 1/400 (0.25%).

The main remaining constraint is search cost. The winning 100-seed cohort made
10,917 lookahead decisions and simulated 1,041,572 branch steps in about 20.7
minutes. Distilling the deep-search choices into the small gated residual did
not preserve the closed-loop gain, so exact heterogeneous-horizon search
remains part of the promoted policy rather than merely a data generator.

## Defect A20 continuation

Teacher-free self-play now accepts `--ascension 0..20`. Ascension is preserved
by reset, exported in every measurement row, encoded as a semantic state token,
and appended to the numeric measurement schema so old checkpoints retain their
original numeric prefix. `tools/eval_selfplay_hrm.py --ascension 20` passes the
level to the Rust server and records it in the evaluation summary.

The A0 generation-17 checkpoint transferred poorly without adaptation. On the
fixed 100-seed cohort from seed source `20262001`, the direct A20 policy reached
a mean floor of 9.06. Every run died in Act 1: 60 died by floor 8 and 17 reached
the floor-16 boss.

Depth-64 planning over card and relic menus was the first positive A20 signal.
It improved a 20-seed paired cohort from 8.75 to 10.35, while combat-only
single-policy rollouts improved only to 9.15 at depth 16 and regressed to 8.45
at depth 64. This falsified the assumption that a longer continuation of the
same weak combat policy is useful planning.

The Rust JSONL protocol therefore gained nested `branch_fork` requests, and the
evaluator gained an opt-in combat beam. Each retained exact engine state forks
several of the model's top legal actions at the next decision; pruning is
performed per live environment, and values are backed up from the deepest
available leaves while preserving earlier exact combat wins. Width one keeps
the previous evaluator behavior.

The first promoted A20 configuration combines:

- depth-64 single-continuation search for inventory-identity menus;
- an exact combat beam of width 8, expansion 4, and depth 12;
- combat search only when visible enemy maximum HP is at least 140, concentrating
  the extra branching on bosses rather than perturbing routine fights.

Reproduce the 100-seed gate with:

```sh
uv run --with torch --with numpy python tools/eval_selfplay_hrm.py \
  --checkpoint artifacts/selfplay/defect-a0-generation17-parameterized-seed-elite-cem-180s.pt \
  --ascension 20 \
  --count 100 \
  --seed-source 20262001 \
  --counterfactual-search-weight 0.1 \
  --counterfactual-outside-weight 0.1 \
  --lookahead-depth 12 \
  --lookahead-min-enemy-hp 140 \
  --lookahead-beam-width 8 \
  --lookahead-beam-expansion 4 \
  --lookahead-include-identity-choices \
  --lookahead-identity-depth 64 \
  --output artifacts/selfplay/defect-a20-generation8-bossbeam8x4d12-menu64-heldout100.jsonl
```

This raises mean floor from 9.06 to 10.16 on identical seeds: paired gain
`+1.10`, 95% confidence interval `[0.370, 1.830]`, with 41 improved, 46
unchanged, and 13 regressed seeds. Four runs enter Act 2 (floors 20, 20, 21,
and 21), versus none in the direct baseline. The evaluator simulated 476,075
branch steps in 175.9 seconds. No run won, so this is an A20 mean-floor
breakthrough rather than completion of the first-win milestone.

Two compression attempts were rejected. Retraining the entire critic on 1,081
exact menu groups achieved 39.5% held-out top-1 menu accuracy but reduced
closed-loop mean floor to 6.59; even a 0.01 critic weight reached only 8.58.
Training only the zero-initialized menu residual preserved the baseline exactly
at scale zero, but its best tested scale reached only 9.35. These results point
to compounding off-policy error: logged menu accuracy is not yet a substitute
for online exact planning.

### A20 counterfactual combat adapter

Updating the whole actor on mixed A0/A20 trajectories was also rejected. Its
closed-loop A20 mean floor fell to 5.60 even though its offline losses improved.
The replacement is an append-only checkpoint migration plus an isolated
counterfactual adapter:

- the legacy numeric projection remains an unchanged prefix, while appended
  deck and ascension measurements enter through a zero-initialized projection;
- named legacy actor rows and the complete choice critic are transplanted;
- newly added survival heads are excluded from policy utility until a checkpoint
  explicitly marks them supported, so random rows cannot alter old decisions;
- the actor and choice critic are frozen, and a 203,543-parameter residual sees
  actor context, full inventory identities, candidate identities, appended
  numeric measurements, and parameterized action deltas;
- exact actions from the same `(seed, step)` menu become winner/loser pairs.
  Training optimizes their ordering rather than the rollout's arbitrary shared
  state-value offset. Branch-score gaps below one point are ignored.

Use `--expand-init-schema --counterfactual-adapter-only` to train this mode.
For example:

```sh
uv run --with torch --with numpy python tools/train_selfplay_hrm.py \
  --init-checkpoint artifacts/selfplay/defect-a20-generation4-menu-residual-60s.pt \
  --expand-init-schema \
  --counterfactual-adapter-only \
  --branch-dataset artifacts/selfplay/a20-combat-branches-1.jsonl \
  --branch-dataset artifacts/selfplay/a20-combat-branches-2.jsonl \
  --cache artifacts/selfplay/a20-counterfactual-prepared.pt \
  --output artifacts/selfplay/a20-counterfactual-adapter.pt
```

`--counterfactual-adapter-scale 0` is an exact policy-control setting. A
positive scale adds the learned correction, and
`--counterfactual-adapter-min-enemy-hp` can optionally gate it to harder
combats. The evaluator records both values in its summary.

The multi-cohort experiment collected 132,489 exact A20 actions from four
depth-8 combat-rollout cohorts plus one depth-12 boss-beam cohort. These formed
69,798 training and 9,293 seed-held-out preference pairs; held-out pair accuracy
was 61.18%. With the existing menu residual fixed at 0.6 and the combat adapter
fixed at 0.2, three untouched 100-seed cohorts averaged floor 9.40 versus 9.18
at adapter scale zero. The paired gain was `+0.217` floors: 48 improved, 214
unchanged, and 38 regressed, with bootstrap 95% interval `[-0.067, 0.513]`.
This is a consistent training signal, but not yet a statistically conclusive
promotion and not an A20 win.

Frontier exploration did find one floor-33 Act 2 boss entry. The deck entered
with 55/88 HP, 25 cards, only three upgrades, and one focus card; even depth-32,
width-16 suffix search left 276--287 of 388 enemy HP. That failure is evidence
that the next training generation must improve earlier deck construction as
well as combat ordering rather than spending more search on the terminal boss.

### A20 mean-floor and retained-HP objective

The population objective is lexicographic at the trajectory level: maximize
mean reached floor first, then prefer more HP when two runs enter the same
reached floor. A complete floor is worth more than any possible Defect HP
difference. Maximum HP and terminal combat margin are lower-order tie-breaks.
The trajectory trainers stop positive elite imitation after first entering the
run's maximum floor, so actions from the subsequent terminal loss are not
mistaken for causes of progress.

Recalibrating the generation-18 counterfactual adapter from scale 0.2 to 0.1
added 126 total floors across 2,400 fresh A20 seeds, or `+0.0525` mean floor.
It was slightly worse in same-floor entry HP on the traced 500-seed cohort, so
0.1 is the working mean-floor incumbent rather than evidence that the learned
adapter has solved health preservation.

A fresh on-policy common-random cohort used 200 seeds with 16 exploratory
copies each. Its raw mean was 8.490 floors, while selecting the best sampled
run per seed produced mean floor 13.200 and mean entry HP 32.04. Floor outcomes
varied for 189/200 seeds, and 98 seeds also varied in HP among copies tied at
their best floor. This supplied dense positive and negative evidence for
seed-centered policy-gradient, elite-prefix, mean-return, and relational
candidate/inventory experiments. None survived independent closed-loop gates.
Full-trunk A20 value fine-tuning also regressed, including conservative weight
interpolation. These checkpoints remain rejected; offline menu accuracy is not
used as a promotion criterion.

The retained-HP insight did transfer directly to exact combat planning. The
legacy nonterminal rollout leaf score weighted damage progress by 50 but HP
fraction by only 20. `--lookahead-combat-hp-weight` now exposes the latter and
defaults to 100 for A20 while preserving 20 for lower ascensions. On the first
paired 50-seed cohort, weight 100 raised mean floor from 9.04 to 9.68 and mean
entry HP from 32.34 to 35.16. On 100 untouched seeds it raised mean floor from
9.41 to 9.78; among the 68 same-floor pairs it carried 3.24 additional HP, and
the cohort maximum rose from floor 16 to floor 23. Combined, the change added
69 floors over 150 seeds. Weights 150, 200, and 300 were worse on the selection
cohort, showing that health preservation must not dominate necessary damage.

Reproduce the confirmed comparison with:

```sh
python3 tools/eval_selfplay_hrm.py \
  --checkpoint artifacts/selfplay/defect-a20-generation18-multicohort-menu-combat-pairwise-120s.pt \
  --ascension 20 \
  --count 100 \
  --seed-source 20262069 \
  --counterfactual-adapter-scale 0.1 \
  --menu-residual-scale 0.6 \
  --counterfactual-search-weight 0.1 \
  --counterfactual-outside-weight 0.1 \
  --lookahead-depth 8 \
  --lookahead-candidates 4 \
  --lookahead-min-enemy-hp 0 \
  --lookahead-combat-hp-weight 100 \
  --output artifacts/selfplay/a20-hp-aware-lookahead.jsonl
```

No A20 run has won yet. This is a confirmed improvement to the population
objective and health carried forward, not completion of the first-win goal.

The HP-aware score also composes with the exact combat beam. On 50 paired
seeds, depth 12 and width 8 improved from mean floor 11.86 at HP weight 20 to
12.22 at weight 100; same-floor entries carried 9.6 more HP. Increasing depth
to 16 regressed to 10.70 on the selection cohort. Increasing width to 16 first
appeared positive but failed its independent gate, scoring 10.02 versus 11.60
at width 8 while nearly doubling branch work. Adding depth-64 card/relic menu
planning also regressed mean floor, even when it improved same-floor HP, and
was rejected under the mean-first objective.

Root pruning has a smaller confirmed efficiency gain. Three searched root
actions versus four added six total floors across 80 paired seeds while using
fewer branch steps. On the 50-seed confirmation it scored 10.84 versus 10.74,
carried 0.42 more HP on same-floor pairs, reached floor 24 rather than 23, and
used 839,063 rather than 882,744 branch steps. Searching two roots or six to
eight roots regressed. A20 therefore defaults to three lookahead candidates;
lower ascensions retain the previous default of four.

Noncombat choices need a longer causal horizon than combat actions. Planning
only card and relic identity menus at depth 64 had regressed, but applying the
same horizon to every route, reward, rest, shop, and event choice lets the
planner observe the downstream fight and HP carried into the next floor. With
the depth-12, width-8 combat beam unchanged, this raised mean floor from 11.30
to 14.80 on 30 paired selection seeds (`+3.50`, 95% CI `+1.04` to `+5.96`).
On 50 untouched seeds it raised mean floor from 10.64 to 12.78 (`+2.14`, 95%
CI `+0.11` to `+4.17`), improving 29 seeds and regressing 13. Among the eight
equal-floor pairs it entered the floor with 7.88 more HP on average. Across
both cohorts the population gain was 212 floors over 80 seeds, or `+2.65`
mean floor. Branch work increased from 861,298 to 1,266,701 steps on the
confirmation cohort, a worthwhile cost for the measured progress gain.

Active A20 lookahead now enables this noncombat horizon at depth 64 by default.
Lower ascensions retain their previous behavior, and
`--no-lookahead-noncombat` provides an explicit ablation. The combat beam still
uses `--lookahead-depth`; only the noncombat root continuation uses the longer
horizon. No A20 run has won yet.

### Current-source A20 noncombat branch residual

A clean release rebuild established a new current-source control before the
next training round. On seed sources `20262077`, `20262078`, and `20262079`,
the depth-12/width-8/noncombat-64 policy scored mean floors 11.80, 11.50, and
11.08. These runs exported 22,753 exact branch records; source `20262072`
contributed another 4,770. The older 14.80 selection result above came from a
previous release artifact and is historical evidence, not the current-source
promotion baseline.

An isolated counterfactual residual was fine-tuned on the four current branch
sets, restricted to noncombat menus. The seed split contained 4,278 training
menus and 338 validation menus. Offline accuracy was modest (53.18% pairwise,
46.45% top-1), so the residual was applied at only 0.01 weight and judged by
fresh closed-loop runs:

| Seed source | Episodes | Control mean | Residual mean | Delta |
| --- | ---: | ---: | ---: | ---: |
| `20262080` | 50 | 11.12 | 11.82 | +0.70 |
| `20262081` | 100 | 10.43 | 11.35 | +0.92 |
| Combined | 150 | 10.66 | 11.51 | **+0.847** |

The combined paired 95% confidence interval is `[+0.078, +1.616]`: 61 seeds
improved, 43 regressed, and 46 tied. The 100-seed confirmation's 29 same-floor
pairs entered their frontier with 0.97 less HP on average, so this promotion is
supported by converted floors, not by a diagnostic HP improvement. Mean final
floor remains the selection objective; HP remains a within-frontier
continuation signal.

The local checkpoint is
`artifacts/selfplay/defect-a20-generation104-current-multicohort-noncombat-60s.pt`
(SHA-256 `5bff4da594b2da6efbe3a543baae2ac31f3c31b01fcd9917e6d0e1a0eae45938`).
It is intentionally ignored with the other generated self-play artifacts; the
tracked experiment manifest records its inputs and gate. Evaluate it with
`--counterfactual-outside-weight 0.01`. Scale zero exactly recovers the
incumbent checkpoint. No A20 run has won yet.
