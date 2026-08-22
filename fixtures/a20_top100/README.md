# Defect A20 Rust Top-100 Cohort

This is a frozen compatibility cohort selected entirely by the Rust HTN before
any Java replay or parity result was inspected.

## Source cohort

- Engine commit: `ca5729a`
- Character: Defect
- Ascension: 20
- Unlock profile: `fixture`
- Random seed source: `20260822`
- Candidate count: 1,000
- Maximum actions per run: 5,000
- Concurrent Rust workers: 12

The source command was:

```sh
target/release/sts-htn \
  --character DEFECT \
  --a20 \
  --count 1000 \
  --concurrent 12 \
  --seed-source 20260822 \
  --max-steps 5000 \
  --fixture-jsonl /tmp/sts-a20-1000-20260822-ca5729a.jsonl
```

The run completed with 995 losses and zero capped runs. Five runs reached a
terminal `Victory` room on floor 52; the current batch runner labels those
terminal states `stopped`. The complete 1,000-state fixture JSONL had SHA-256
`640de70b48b368b9b460f71e36d9007510aab33fc7be693899d9ac704dfd7904`.

## Frozen ranking

The deterministic rank is:

1. wins before non-wins;
2. greater `floor_achieved` first;
3. smaller signed numeric seed first.

The selected cohort contains all five terminal Victory/stopped runs at floor
52, all 24 losses at floor 51, all 59 losses at floor 50, all three losses at
floor 48, the floor-47 loss, all seven floor-46 losses, and the first floor-45
loss. `selection.tsv` records the rank, raw Rust outcome, floor, and final live
monsters. `seed_list.txt` is the authoritative replay order.

Do not replace members based on Java results. A seed remains in the cohort even
when its first Java replay fails early.

## Compatibility workflow

For every seed:

1. create live Rust HTN and ExactTextSim2 RPC sessions;
2. transform the complete Rust legal-action set into Java command vocabulary;
3. require a one-to-one match with Java's complete executor-valid action set;
4. compare gameplay state, then apply exactly one HTN decision to both engines;
5. stop immediately at the first legal-action or gameplay-state mismatch;
6. fix Java-authoritative behavior in Rust, add a regression test, and replay
   from the seed start.

Rust actions and states are streamed through RPC and are not written as a
per-seed result corpus. Only a fully matched Java transcript is stored as an
oracle.

ExactTextSim2's `SnapshotWriter` deliberately omits potion-discard commands
from its published `legal_actions`, although `CommandExecutor.legalActions()`
includes them and its command endpoint accepts them. The lockstep driver
reconstructs only those hidden commands from the serialized ordered potion
slots, applying Java's `WeMeetAgain` discard prohibition. This keeps the
bijection against Java's executor-valid set rather than its filtered display
subset.

The generated gzip JSONL oracle corpus is stored outside Git under:

```text
../exact-text-sim/runtime/oracles/defect/a20-rust-top100-from1k-20260822/
```

Run `sts-htn-rpc`, then use `tools/lockstep_exactsim.py` to inspect the first
mismatch or write fully matched Java oracles.
