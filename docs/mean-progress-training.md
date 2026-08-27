# Mean-progress combat training

## Current state

The active training path is deliberately small and checkpoint-independent:

- `tools/training_schema.py` defines the complete state/action contract.
- `tools/mean_progress_model.py` defines the policy network.
- `tools/train_mean_progress.py` prepares the canonical A20 dataset and trains
  from random initialization.
- `tools/eval_selfplay_hrm.py` runs the checkpoint and exact lookahead together.

The historical 78-head trainer and its sequence of checkpoint-migration,
choice-critic, menu-residual, and population-adapter modes were removed. Git
history and `artifacts/hrm/README.md` retain the experiment record, but they are
not part of the active training interface.

## Evidence available on 2026-08-27

The local A20 corpus contains:

- 27,347 complete trajectories and 3,560,274 decisions;
- 7,280 unique seeds;
- mean observed final floor 9.866 and maximum floor 33;
- no A20 wins yet;
- 283,108 exact evaluated branches grouped into 98,372 distinct action menus;
- 169,217 combat branches and 113,891 noncombat branches.

Many seeds have repeated random or policy-driven attempts. Preparation caps
each seed at four trajectories and samples across the whole run so repeated CRN
cohorts cannot dominate the population mean.

## Objective

For a trajectory that reaches floor `f` with `hp` on first entering that floor,
the scalar policy target is:

```text
(f + hp / 200) / 50
```

One full floor is therefore worth more than any normal Defect HP difference,
while higher HP remains dense continuation value among runs reaching comparable
floors. Separate final-floor and entry-HP-fraction heads prevent the HP signal
from disappearing inside the scalar target.

Training combines two kinds of evidence:

1. Monte-Carlo regression from random and policy play, including failed runs.
2. Pairwise ranking from exact cloned action branches.

There is no win threshold, boss-only objective, expert imitation target, or
checkpoint initialization.

## Inputs and model

Every legal action sees:

- hashed state and action tokens;
- the full owned card/relic multiset, with one identity token per copy;
- candidate card/relic identities in the same embedding namespace;
- 45 named state measurements, including current and maximum HP;
- 31 deterministic action-transition measurements;
- up to 64 prior decision signatures.

The transition vector uses one schema in and out of combat. It includes player
and enemy HP, block, energy, hand/draw/discard/exhaust sizes, deck changes,
relics, potions, orb capacity/occupancy/evoke value, incoming attack, living
enemies, turn/card counters, and power totals.

The 3,673,519-parameter model has one shared 32,769-by-96 token embedding,
selective state/history memories, action-conditioned state attention, and an
explicit candidate-to-inventory join. It emits only:

- `progress_value` for action ranking;
- `final_floor` as an auxiliary target;
- `entry_hp_fraction` as an auxiliary target.

## Commands

Train from scratch using all locally available A20 evidence:

```bash
uv run --with torch --with numpy python tools/train_mean_progress.py
```

An existing cache is a frozen data generation: newly written evaluation traces
do not silently change later runs. Use `--rebuild-cache` when intentionally
starting a new self-play generation from all currently available evidence.

Evaluate 100 fresh A20 seeds with the default depth-12, width-8 exact planner:

```bash
uv run --with torch --with numpy python tools/eval_selfplay_hrm.py
```

Use `--lookahead-depth 0` to measure the learned policy without search.

## First scratch round

The first 180-second run retained 160,000 seed-balanced trajectory decisions
and 100,000 exact preference pairs. Held-out-by-seed results were:

- exact-pair accuracy: 57.88%;
- final-floor MAE: 2.46 floors;
- entry-HP-fraction MAE: 0.198.

Closed loop on fresh seeds:

- planned, 50 seeds (`20262092`): mean floor 10.68;
- direct, 100 seeds (`20262093`): mean floor 7.51;
- A20 wins: 0.

This is the clean baseline, not a promotion over the former generation-104
system. Its main current limitation is the learned long-horizon policy; exact
lookahead contributes more than three floors of mean progress.

## Artifact cleanup

Sixteen reproducible prepared caches (3.54 GiB), 111 obsolete self-play
checkpoints/caches (3.38 GiB), and seven superseded combat-model exports
(roughly 105 MiB) were moved to the desktop trash. Raw trajectory and exact
branch evidence was retained. The active local artifacts are:

- `artifacts/selfplay/defect-a20-mean-progress-v1-data.pt`;
- `artifacts/selfplay/defect-a20-mean-progress-v1.pt`.
