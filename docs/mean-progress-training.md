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

## Evidence available on 2026-08-28

The active prepared A20 corpus now contains:

- 165 trajectory files and 23,480 eligible seed-balanced episodes;
- 9,830 unique seeds and 375,672 sampled-decision candidates;
- 160,000 retained terminal-progress decisions;
- 500 generation-4 planner episodes containing 49,366 nontrivial legal-action
  menus for policy distillation;
- maximum observed floor 33;
- no A20 wins yet;
- retained exact-branch and failed-policy trajectories for future generations.

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

Training separates two kinds of evidence:

1. A calibrated progress critic regresses Monte-Carlo outcomes from random and
   policy play, including failed runs.
2. A policy head distills the action selected by the deployed generation-4
   exact planner from the complete legal-action menu.

The policy head reads a detached progress representation, so imitation cannot
move the shared critic away from its terminal-progress scale. There is no win
threshold, boss-only objective, external expert, or checkpoint initialization.

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

The 3,979,280-parameter model has one shared 32,769-by-96 token embedding,
selective state/history memories, action-conditioned state attention, and an
explicit candidate-to-inventory join. It emits:

- `policy_logit` for live, root, and continuation action ranking;
- `progress_value` for calibrated exact-rollout leaf value;
- `final_floor` as an auxiliary target;
- `entry_hp_fraction` as an auxiliary target.

The evaluator obtains policy and progress scores from one forward pass. Legacy
three-output generation-4 checkpoints remain load-compatible.

## Commands

Train from scratch for ten minutes using the frozen A20 evidence:

```bash
uv run --with torch --with numpy python tools/train_mean_progress.py
```

An existing cache is a frozen data generation: newly written evaluation traces
do not silently change later runs. Use `--rebuild-cache` when intentionally
starting a new self-play generation from all currently available evidence.
During the run, held-out seed validation selects the exported checkpoint rather
than blindly using the last optimizer step. `--snapshot-dir PATH` additionally
retains time-separated checkpoints for closed-loop mean-floor selection.

Evaluate 100 fresh A20 seeds with the default depth-12, width-8 exact planner:

```bash
uv run --with torch --with numpy python tools/eval_selfplay_hrm.py
```

Use `--lookahead-depth 0` to measure the learned policy without search.

## Procedural combat curriculum

The frozen full-run corpus remains useful evidence, but generation 11 showed
that it is not a sound repeated-training distribution. A 30-minute scratch run
selected step 2,386 after about three minutes; by step 23,271, held-out planner
agreement had fallen from 56.63% to 46.27% and progress MAE had risen from
0.0463 to 0.0566. The selected checkpoint was also flat against generation 10
in closed-loop play: +0.095 mean floor over 200 paired seeds with 95% interval
`[-0.539, 0.729]`.

`TrainEnv::procedural_defect_combat` and the self-play protocol's
`reset_combat` operation provide an unbounded alternative. Each seed creates a
new combination of:

- Act 1, 2, or 3 and an elite or boss room, stratified across all 18 encounters;
- a stage-sized Defect deck sampled from the real card pools;
- a latent mechanism bias (orbs, draw/access, energy, focus, zero-cost, powers,
  or balanced) mixed with off-theme cards;
- starter removals, per-card upgrades, safe acquired relics, and realistic
  stateful relic counters;
- maximum HP and current HP over a broad stage-dependent range;
- the ordinary seeded combat shuffle, monsters, intents, and relic hooks.

The native `sts-hrm-train` learner does not build a data cache or repeat epochs.
For every optimizer batch it:

1. requests never-before-used procedural seeds;
2. advances each fight by a random number of HTN actions so roots cover
   later turns as well as opening hands;
3. forks the HTN-preferred action and random legal alternatives from the same
   root state;
4. continues each branch to the fight's terminal state in the exact engine;
5. ranks the actions by surviving player HP fraction on a win or negative
   remaining monster HP fraction on a loss;
6. takes one optimizer step and discards the scenarios.

Because action labels are compared within one state, recognizing that a deck is
generally strong cannot explain which action is better. Training and validation
have independent seed streams. Validation scenarios are also one-use, and the
final unseen cohort is evaluated once against both the trained network and its
saved random initialization. This makes memorizing individual training menus
useless. It does not make distribution shift impossible: the deck generator
and continuation policy still define what the learner can observe.

The procedural model is separate from the run-progress critic. It emits a menu
policy logit and combat margin; both outputs train a shared state/action/deck/
relic representation. Isolated fight margins are not written into whole-run
floor targets.

Run the default ten-minute online curriculum on CUDA:

```bash
cargo run --release --features native-training-cuda --bin sts-hrm-train
```

Use `--features native-training -- --device cpu` for a CPU-only build. The
trainer defaults the experiment parameters; `--seconds` is the ordinary
override. It writes model weights as safetensors and a neighboring metrics JSON.

`BatchedTrainEnv` is called directly. There is no subprocess, JSONL, ctypes,
MessagePack, Python, PyTorch, or ONNX operation in the training path. Reset,
Step, Fork, BranchFork, BranchStep, and observation construction run independent
environments on Rayon while retaining request order exactly. Candle supplies
Rust autograd, AdamW, CPU/CUDA tensors, and safetensors serialization. A captured
six-request JSON protocol fixture covering every batch operation remains
byte-identical to the earlier serial adapter.

The fresh native critic has 3,467,522 parameters at the default width of 96.
One shared 32,769-row hashed embedding represents state, legal action, full
deck/relic inventory, candidate identity, and action history. Pooled state ×
action and inventory × candidate products make the two important joins
explicit. The model also receives all 45 run/combat measurements and 31
candidate-specific resource parameters, including current/max HP and lethal
HP/enemy-damage indicators. Nine context vectors feed a two-output MLP.

Before native training, the Python/PyTorch path on the 6-core/12-thread Ryzen 5
7500F host measured:

| Rayon threads | Scenarios/batch | Scenarios/s | Branch steps/s |
| ---: | ---: | ---: | ---: |
| 1 | 12 | 19.7 | 1,286 |
| 12 | 12 | 20.3 | 1,332 |
| 1 | 96 | 23.2 | 1,547 |
| 12 | 48 | 25.8 | 1,740 |
| 12 | 96 | 26.5 | 1,722 |
| 12 | 128 | 22.4 | 1,520 |
| 12 | 192 | 19.5 | 1,402 |
| 12 | 384 | 23.7 | 1,623 |

Batch 96 remains the default. Hundreds or thousands are not automatically
better: terminal branches thin unevenly, so very large synchronous batches wait
on stragglers and perform fewer optimizer updates. Set `RAYON_NUM_THREADS` to
bound engine CPU use when sharing a host.

With the same 96 scenarios, four root actions, 16-action burn-in, and 12 Rayon
threads, the native implementation measured:

| Trainer/device | Scenarios/s | Branch steps/s | Optimizer time |
| --- | ---: | ---: | ---: |
| Python/PyTorch CUDA (previous best) | 26.5 | 1,722 | not isolated |
| Native Rust/Candle CPU | 26.3 | 2,190 | 0.86 s / 6 batches |
| Native Rust/Candle CUDA | 26.6 | 2,222 | 0.63 s / 6 batches |

Scenario rates are continuation-policy dependent, so exact branch steps per
second are the better comparison. Native CUDA improved that rate by about 29%
in the controlled 20-second run. The branch hot path returns only outcome and
terminal margin: it skips post-step run/deck measurements, the next legal menu,
and full neural observations while the HTN continuation reads the environment
directly. Compact and full transitions have exact-state and terminal-score
equivalence tests. The result also confirms that neither serialization nor
tensor optimization is the main bottleneck: exact rollout collection still
dominates, and CUDA is only about 1.5% faster than CPU at this batch size.

A 60-second native CUDA run before the compact-row optimization processed
1,440 unique scenarios, 1,177 usable
menus, 4,341 root branches, and 125,290 exact branch steps in 15 updates. It
reached 2,071.6 branch steps/s. Collection consumed 58.96 seconds, optimizer
work 1.52 seconds, and checkpoint serialization 0.019 seconds.

On a final 120-scenario stream that was never trained or used for selection, 95
usable menus completed. Against the saved random initialization, total loss
fell from 2.0912 to 1.5267, margin MAE from 0.9604 to 0.4897, mean action regret
from 0.1261 to 0.1057, and optimal-action accuracy rose from 34.7% to 44.2%.
The checkpoint is
`artifacts/selfplay/defect-a20-procedural-combat-v2-60s.safetensors`. This is a
successful learning and throughput check, not yet a mean-floor promotion.

The superseded `.pt` checkpoints were deleted. The former Python procedural
trainer and its FFI bridge were removed rather than kept as a second training
implementation. Full A20 mean floor remains the only promotion gate once the
native policy is wired into closed-loop run evaluation.

## Promoted generation 10

Generation 10 replaces raw exact-branch score fitting with planner
distillation. Four retained generation-4 planned cohorts supply 49,366 full
legal-action menus. The progress critic is trained from all 23,480 eligible
episodes, while the detached policy adapter learns only which menu action the
planner selected. The 601.7-second run started from random initialization and
produced ten temporal snapshots; closed-loop screening selected step 2,394 at
180.0 seconds rather than the offline-selected step 3,191.

Held-out-by-seed validation for the promoted snapshot is:

- planner top-action agreement: 54.51%;
- progress MAE: 0.04439 normalized, or about 2.22 floor units;
- explicit final-floor MAE: 2.23 floors;
- entry-HP-fraction MAE: 0.199.

The standalone distilled policy did not beat generation 4 on its 100-seed
screen (9.21 versus 9.39 mean floor). The intended policy-plus-planner system
did. Across two independent 100-seed cohorts (`20262116` and `20262117`), the
default exact planner improved from mean floor 11.635 to 13.445. The paired
gain is +1.81 floors with 95% confidence interval `[1.057, 2.563]`: 95 seeds
improved, 63 tied, and 42 regressed. Neither system won an A20 run; both have a
maximum observed floor of 33.

Separating policy logits from calibrated leaf values initially required two
frontier inference calls. Fusing both outputs into one model pass produced
byte-identical terminal results on a fixed 10-seed cohort and reduced runtime
from 57.4 to 45.7 seconds.

## Historical generation 4

Generation 4 deliberately rebuilt the cache after collecting direct and exact-
planner trajectories from several policies on common seeds. The resulting
corpus contained 20,580 seed-balanced episodes from 8,330 unique seeds. A
ten-minute random-initialization run produced ten one-minute snapshots.

The unbounded pair objective showed a sharp temporal phase change: direct mean
floor peaked at step 1,305 and then fell monotonically as exact-pair fitting
continued. Selecting the early snapshot by closed-loop play raised direct mean
floor from 7.57 to 9.42 on 200 unseen seeds, a paired gain of 1.85 floors with
95% confidence interval `[1.23, 2.47]`.

Across two separate cohorts totaling 300 seeds with the default exact planner,
mean floor rose from 11.32 to 12.117. The paired estimate was +0.797 floors with
95% confidence interval `[0.268, 1.326]`: 118 seeds improved, 102 were unchanged,
and 80 regressed. Neither checkpoint won an A20 run yet.

The key evidence was that extra optimizer steps are harmful without new data.
Ten-minute experiments should retain temporal snapshots and select by actual
closed-loop mean floor; the final training step is not a valid default.

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

After generation 10 was promoted, its ten temporary snapshots and the
superseded generation-4 checkpoint/cache were also moved to trash. The raw
generation-4 planner trajectories remain because they are the reproducible
distillation source.

- `artifacts/selfplay/defect-a20-mean-progress-v10-distill-data.pt`;
- `artifacts/selfplay/defect-a20-mean-progress-v10-distill-selected-10m.pt`.
