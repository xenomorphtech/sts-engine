# sts-engine

Graphics-free Slay the Spire engine for AI training research.

The desktop JAR remains the logic authority. This crate ports the gameplay RNG
and rules so a seeded ExactTextSim command transcript can be replayed without
LibGDX, a GPU, or the 60 Hz action queue. Instant resolution is the point:
training loops should step hundreds of thousands of times per second, not
10 seconds per act.

## Measured speed (release, this machine)

| Workload | Rate |
| --- | --- |
| Seed-2 command transcript | **11,100 acts/s** (~90 µs each) |
| Random-policy `TrainEnv::step` | **578,000 steps/s** |
| Java ExactTextSim pruned Act 1 | ~0.1 acts/s warm / ~0.04 cold |

That is about **100,000×** faster than the pruned Java frame loop for act replay.

## What already matches the Java headless oracle

Checked against `exact-text-sim/runtime/act1-seed2-final-pruned.jsonl` and
seeds 1/2/3 for determinism:

- libGDX `RandomXS128` (2016 desktop) and STS `Random` counters
- `java.util.Random` + `Collections.shuffle` for relics, bosses, and draw piles
- Java 8 `HashMap` iteration order for relic and card libraries
- Act 1 Ironclad generation: monster/elite/boss lists, relic pools, map graph,
  Neow, first-combat starter shuffle (hand + draw pile)

`Unlocks::fixture()` loads the Java ExactTextSim profile (`runtime/profile-fixture`,
or `STS_PROFILE_FIXTURE`): `STSSeenBosses`, `STSUnlocks` card/relic locks, and
`Settings.isFinalActAvailable` from the three base-character WIN flags. That is
the same prefs the headless hunts copy into each instance. `--unlocks all` is the
rust cheat that also hard-unlocks every card and relic.

## Parity status (seed 2 vs ExactTextSim JSONL)

The living oracle is `tests/act1_parity.rs`. It walks every snapshot and
stops at the first HP / gold / floor / deck / monster / hand mismatch.

**Currently locked, in order, from snapshot 0 through snapshot 89:**

- Neow, map, Cultist (full fight + Burning Blood + gold/potion/Hemokinesis)
- Jaw Worm (exact `getMove` including extra `aiRng` booleans)
- Woman in Blue (two-step leave)
- Event-room `EventHelper.roll` (this seed converts the next `?` into Blue Slaver)
- Small Slimes composition (`miscRng` acid/spike swap), Slimed, frail, Second Wind
- Blue Slaver through the weakened Strike (seq 85 slaver 25/46)

Next break is seq 90 (Slaver rake vs stab, 4 HP). Remaining Act 1 content:
Large Slime, Looter, Lots of Slimes, Scrap Ooze, chests, rest sites,
Hexaghost divider/inferno, boss relics, Act 2 transition.

Act 2/3/4 dungeon generation (monster/elite/boss lists, map seed offsets
`act*100/200/300`) is scaffolded in `Dungeon::generate_act` so later acts
can be walked the same way once Act 1 is green.

ExactTextSim drops VFX-gated obtains (Neow rare, card-reward insert until
Proceed). The engine matches that headless timing.

## Build

```sh
cargo test --manifest-path sts-engine/Cargo.toml
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-replay -- 2
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-bench
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-htn -- --seed 0 --count 100 --concurrent 6 --a0
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-parity -- --character DEFECT --seed 338612
```

HTN engine optimizations are gated by a compact final-state fixture. It has one
JSON object per seed (no turn trace), including progression, player/combat
state, and every gameplay RNG stream. Generate or verify the fixed 10,000-seed
Defect A0 cohort with:

```sh
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-htn -- --seed 0 --count 10000 --concurrent 8 --a0 --fixture-jsonl sts-engine/fixtures/htn/defect-a0-seeds-0-9999.jsonl
cargo run --release --manifest-path sts-engine/Cargo.toml --bin sts-htn -- --seed 0 --count 10000 --concurrent 8 --a0 --compare-jsonl sts-engine/fixtures/htn/defect-a0-seeds-0-9999.jsonl
```

The comparison exits nonzero at the first exact mismatch. Policy changes may
legitimately alter the fixture, so use this gate for engine/representation
changes while holding HTN decisions fixed. Measured results and the snapshot
design notes are in [`HTN_PERFORMANCE.md`](HTN_PERFORMANCE.md). The checked-in
cohort has no 5,000-step caps; any newly capped seed is a loop regression.

Action capture is opt-in and buffered per seed. It can isolate engine stepping
from HTN search:

```sh
sts-htn --seed 0 --count 10000 --concurrent 8 --a0 --actions-jsonl /tmp/defect-a0-actions.jsonl
sts-htn --character DEFECT --a0 --concurrent 8 --replay-actions-jsonl /tmp/defect-a0-actions.jsonl --compare-jsonl sts-engine/fixtures/htn/defect-a0-seeds-0-9999.jsonl
```

`sts-htn --count N` runs `N` consecutive seeds in one process, starting at
`--seed`, and reports aggregate win rate, floors reached, remaining-monster HP,
and simulation throughput. `--concurrent N` selects the worker count; `--a0`
and `--a20` are shortcuts for `--ascension 0` and `--ascension 20`. The
character defaults to Defect, and the unlock profile is loaded once per batch.
Pass `--random-seeds` to generate a fresh unique cohort; the reported
`seed_source` can be supplied later with `--seed-source N` for an exact replay.
Batch output is aggregate-only by default; `--diagnostics` adds orb/event stats
and one detailed row per seed. The aggregate begins with compact win rate,
mean-floor, and Act 1/2/3/Heart death-layer lines, followed by the existing
full metrics line. Each death layer splits its total into normal, elite, and
boss fights (plus `other` when an unusual combat room is present). Death layers
count only runs where player HP reached zero; caps, stops, and live-player
combat stalemates retain their separate totals.

Defect runs use the current learned deck-building weights from
`tools/draft_policy_synergy_a20.json` by default, while HTN continues to control
combat, routing, events, and unsupported card-selection screens. Select the
policy explicitly with:

```sh
# Learned deck building plus HTN for the rest (the default).
sts-htn --deck-policy rl --count 1000 --concurrent 12 --a20 --random-seeds

# Original HTN decisions everywhere.
sts-htn --deck-policy htn --count 1000 --concurrent 12 --a20 --random-seeds
# Equivalent shorthand:
sts-htn --pure-htn --count 1000 --concurrent 12 --a20 --random-seeds
```

Use `--deck-policy-path PATH` to load a different compatible learned
checkpoint. Existing HTN-only regression fixtures should be generated and
checked with `--pure-htn`.

`sts-parity` lockstep-replays an ExactTextSim oracle and prints the first
mismatch with screen, event options, rewards, card-reward list, pending cards,
RNG counters, and the commands around the fail. Agents should use that instead
of re-parsing `states.jsonl`. Default oracle root is
`exact-text-sim/runtime/oracles/<character>/a<asc>/<seed>/` (`STS_RUNTIME` overrides).

## Training API

```rust
use sts_engine::TrainEnv;

let mut env = TrainEnv::new(2);
let legal = env.reset(2);
let info = env.step(0);
let obs = env.compact_obs();
```

`TrainEnv::step` indexes into the current `legal_actions()` list, the same
discrete interface the Java headless sim advertises.

### Compressed late-boss curriculum

`BossDraftEnv` removes map walking and every non-boss combat while retaining
seeded Neow options, card rewards, relic pools/side effects, shops/prices,
upgrades, and the real boss combat engine. The environment controls what is
offered; the learner controls only the indexed pick/skip/buy actions.

The default A20 three-act build samples 15–21 normal card rewards (mean 18),
4–8 elite bundles (mean 6), 2–4 shops (mean 3), three chest relics, and 5–9
upgrades. Each shop permits a seeded 1–2 purchases, approximating an average
shop visit without letting fight-free gold buy the entire inventory. Elite
card rewards use the stronger elite rarity roll and always include a relic;
Black Star adds its normal second independent non-campfire relic. Act 1 and 2
boss card and boss-relic rewards are included automatically in the schedule.
Question Card, Busted Crown, Prayer Wheel, eggs, shop discounts, bottles, and
other reward-time relic effects still go through the engine's regular paths.

```rust
use sts_engine::{BossDraftEnv, Character, DraftConfig};

let mut env = BossDraftEnv::fixture(7, Character::Defect, DraftConfig::default())?;
while !env.ready_for_bosses() {
    let observation = env.observation();
    let action_index = policy(&observation); // index into observation.offers
    env.step(action_index)?;
}
let result = env.evaluate_htn(2_000);
# Ok::<(), String>(())
```

Evaluation captures an exact Act 1 build snapshot after the first act's
formation opportunities and before its boss card/relic rewards. That snapshot
is independently tested against Slime Boss, The Guardian, and Hexaghost with a
base starting health of 60/75 for every fight. Formation then continues, and
the completed build is independently tested at full health against Awakened
One, Time Eater, Donu/Deca, and Corrupt Heart. This prevents one boss from
consuming resources or HP before another identity is tested while ensuring the
Act 1 bosses never see cards or relics obtained later in the route.

The result reports wins/losses/timeouts, the required `act1_all_won` objective,
remaining player HP, and the sum of remaining boss HP (for Donu/Deca, both
living enemies are summed). It also reports cumulative
`boss_damage_dealt_sum` as a dense positive signal, so a loss that reaches
farther is better than an earlier loss. Completed fights obey
`fights_started = wins + losses + timeouts`.

For Python or another external trainer, `sts-draft` is a persistent JSONL
stdio bridge:

```sh
cargo run --release --bin sts-draft
{"op":"reset","seed":7,"character":"DEFECT"}
{"op":"step","action_index":0}
{"op":"evaluate","max_steps_per_boss":2000}
```

Monte Carlo training can keep many seeds live in the same process. Seed and
observation indices remain stable; use `null` once an individual environment
has reached `ready_for_bosses`:

```json
{"op":"batch_reset","seeds":[100,101,102],"character":"DEFECT"}
{"op":"batch_step","action_indices":[0,2,1]}
{"op":"batch_step","action_indices":[null,0,3]}
{"op":"batch_evaluate","max_steps_per_boss":2000}
```

For a reproducible comparison policy, `batch_baseline` uses the current HTN
to finish every formation before the same boss evaluation. This is also a
convenient end-to-end smoke test; training normally replaces these baseline
choices with the RL action indices:

```json
{"op":"batch_reset","seeds":[100,101,102],"character":"DEFECT"}
{"op":"batch_baseline","max_decisions":200}
{"op":"batch_evaluate","max_steps_per_boss":2000}
```

Every observation includes the sampled total and per-act counts, shops and
remaining purchase slots, elite count, current deck/relic state, metrics, and
the legal indexed offers. A separate deterministic route RNG makes count/order
sampling seed-dependent without perturbing the engine's card/relic offer RNGs.

`tools/train_draft_policy.py` provides a dependency-free episodic policy
gradient learner over that batch protocol. A generation is one terminal-reward
update over `--batch-size` newly seeded builds. It checkpoints after every
generation and selects the best greedy policy on a fixed validation set:

```sh
python3 tools/train_draft_policy.py \
  --generations 25 --batch-size 256 --workers 10 \
  --validation-size 12 --test-size 16 \
  --state tools/draft_policy_synergy_a20.json
```

Parallel workers run independent seed shards and return compact gradient
statistics. The trainer combines those statistics into one policy-gradient
update over all 256 builds, rather than applying a separate update per shard.
The latest `weights`/`generation` pair is the default for evaluation and for
starting joint training. `best_weights` remains available as the winner of the
legacy fixed validation set, but is not selected implicitly.

The scalar reward makes wins dominant while using cumulative boss damage,
remaining boss HP, surviving player HP, and timeouts as dense reach signals.
Clearing all three Act 1 bosses adds a large requirement bonus; each missing
Act 1 win is penalized, and validation ranks full Act 1 clears first.
Every candidate is explicitly crossed with owned card identities and counts,
upgraded cards, relics, deck/relic shape, gold, act, and formation progress;
these candidate-conditioned features let the policy learn synergies rather
than only global card preferences.
The final untouched test report compares the learned policy with both the
generation-zero policy and the HTN formation baseline.

Deck formation and combat can also be trained as one episodic problem. After
`batch_reset`/`batch_step` finishes every build, the same batch remains live for
the seven independent boss fights (three from the Act 1 snapshot and four from
the completed build):

```json
{"op":"batch_fight_reset","max_steps_per_fight":500}
{"op":"batch_fight_observe"}
{"op":"batch_fight_step","action_indices":[0,2,null,1]}
{"op":"batch_fight_results"}
```

Each active fight observation contains the complete hand, draw, discard and
exhaust piles; owned relics and counters; potions; player powers and orbs; and
all living enemies. Enemy intent includes its type, damage per hit, hit count,
and total raw damage. Legal card/target, potion, grid, and end-turn actions are
indexed in `offers`. Finished fights use `null` in later batched steps.
`batch_fight_baseline_actions` exposes the HTN action indices for imitation
warm-up without hiding the state from the learned policy.

`tools/train_joint_policy.py` starts with the best synergy-aware deck weights,
warms the combat policy from HTN decisions, then applies boss outcomes to both
the deck-building and combat trajectories. Training generations use new,
deterministically derived seeds, so card/relic/shop/elite offers change from
batch to batch; validation and test seeds stay fixed for comparable results.

```sh
python3 tools/train_joint_policy.py
```

The joint checkpoint stores current and best deck/combat policies separately
and reports four held-out combinations: learned/learned, learned deck/HTN
fight, HTN deck/learned fight, and HTN/HTN.

## How this was developed

The engine was not written from the wiki and then tested. It was grown against
ExactTextSim transcripts.

A Python HTN drives the pruned Java headless sim and records every
`ready_for_command` boundary (combat turn, map, event, rewards, shop, rest,
grids) plus the command it sent. Those JSONL files are the oracle: gameplay
fields and the 13 named STS RNG streams, not a SHA-256 of the Java object
graph. `sts-parity` replays the same commands on rust and stops at the first
mismatch (hp, gold, block, floor, act, deck, monsters, hand, relics; optionally
`--rng`).

Most of the card, relic, potion, and monster side effects that were missing
showed up as leftover reds on that walk — a Lagavulin siphon that dropped
negative Strength, a ChannelAction that still ran after the last monster died,
Discovery never rolling the card-random stream, and so on. The loop is: hunt
thousands of seeds, harvest the JSONL, scan for the earliest mismatch cluster,
port that Java `Action` / `Power` / relic trigger, retest the green registry
so a fix does not silently drop a seed that used to match.

Defect A20 has a registry of more than a thousand seeds that walk GREEN under
`Unlocks::fixture()` against their oracles. That is lockstep of those
transcripts, not “Defect is done” and not a claim that HTN wins Act 4.

## Seeds and unlocks

- `Unlocks::fixture()` — captured ExactTextSim profile
- `Unlocks::all()` — research default; every card and relic is in the pool so
  a seed alone determines the run
