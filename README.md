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
