# HTN simulation performance

## Fixed validation boundary

The engine gate is the 10,000-seed Defect A0 fixture at
`fixtures/htn/defect-a0-seeds-0-9999.jsonl`. Each line contains only the final
state for one seed. Engine representation changes must match all lines exactly;
HTN policy experiments instead use paired win rate and mean floor.

Action capture is optional and buffered in memory as `seed -> [actions]`.
Replaying those actions bypasses HTN and provides an engine-only measurement.

## Measurements

All full-cohort measurements used release mode and eight workers on the same
8-core host.

| Representation | 10k time | Change from baseline | Exact states |
|---|---:|---:|---:|
| Deep-cloned dungeon state | 431.161s | baseline | 10,000/10,000 |
| COW dungeon map | 277.113s | -35.7% | 10,000/10,000 |
| COW map and card pools; typed/COW relic pools | 166.025s | -61.5% | 10,000/10,000 |
| All above plus COW encounter/event tables | 108.597s | **-74.8%** | 10,000/10,000 |
| All above plus shared immutable unlock profile | 42.200s | **-90.2%** | 10,000/10,000 |

The baseline fixture contains 4,417,950 decisions, 439 wins, two capped runs,
and mean floor 28.4151. Action-only engine replay measured roughly 2.75 million
actions/s, while the original HTN batch managed roughly 10,239 decisions/s.
This establishes branch construction/lookahead as the dominant cost rather than
ordinary engine stepping or JSON serialization.

The final representation processes roughly 104,691 decisions/s on this cohort,
about 10.2 times the original throughput. The unlock profile was the single
largest remaining clone sink because its string hash sets had been deep-cloned
for every HTN search branch despite being immutable after game construction.

## Rejected experiments

- Reusing full `Game` allocations with derived `clone_from` was 13.6% slower
  (15.96s versus 14.05s on the same 300 seeds). Element-wise reuse cost more
  than fresh allocation/copy for this state shape.
- Reducing greedy combat depth from eight to six was effectively timing-neutral
  and reduced a fixed 1,000-seed cohort from 45 to 44 wins, so it was reverted.
- Replacing formatted anti-stall keys with structured keys removed allocations
  but was approximately timing-neutral; it remains because it is simpler and
  avoids unnecessary strings.

## ECS and layered-state conclusion

A conventional mutable ECS does not itself make search snapshots cheap. The
results support an ECS only if its component tables have stable IDs and cheap
snapshots: typed immutable tables, copy-on-write pages, or persistent overlays.

The recommended next experiment is an HTN-local checkpoint/undo or shallow
delta layer for hot combat components (player piles, monsters, powers, orbs,
and RNG streams). The ordinary engine should continue to expose a flattened
authoritative state. This keeps search-specific lifetime and rollback machinery
outside gameplay semantics and leaves the 10k exact engine gate meaningful.
