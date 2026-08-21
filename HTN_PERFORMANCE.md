# HTN simulation performance

## Exact validation boundary

The representation gate is the 10,000-seed Defect A0 fixture at
`fixtures/htn/defect-a0-seeds-0-9999.jsonl`. Each line contains one compact
final state, including every gameplay RNG stream. Representation changes must
match all 10,000 lines exactly. HTN policy changes instead require a freshly
generated fixture plus paired win-rate and progression evaluation.

Action capture is optional and buffered in memory as `seed -> [actions]`.
Replaying those actions bypasses HTN and provides an engine-only measurement.

## Representation benchmark

Both measurements used the release build and eight workers on the same host.
The table reports the mean of two warmed full-cohort runs. The baseline binary
was rebuilt from pre-port `main` with only the fixture CLI added; the optimized
binary then compared equal for all 10,000 final states. This paired benchmark
used the pre-loop-fix policy snapshot so both representations executed the
same 48,062,848 decisions.

| Representation | 10k time | Change | Exact states |
|---|---:|---:|---:|
| Deep-cloned dungeon and unlock state | 32.540s | baseline | 10,000/10,000 |
| COW dungeon state plus shared unlock profile | 8.438s | **-74.1%** | 10,000/10,000 |

Throughput increased from about 1.48 million to 5.70 million decisions/s
(3.86x). This branch uses a smaller HTN policy than the source performance
branch, so its absolute timings and speedup are intentionally reported
separately from commit `b94cad2`.

## Current fixture

The committed fixture contains 1,812,196 decisions across 10,000 terminal
losses, with zero runs reaching the 5,000-step cap. Runs average 181.22
decisions and end at an average floor of 14.61; the longest run takes 657
decisions and reaches floor 44. Two warmed exact comparisons with eight workers
took 6.063s and 8.414s (7.239s mean).

The cap cleanup fixed two distinct problems without changing the frozen oracle
set: completed campfires were still publishing their original Rest/Smith
buttons, and the HTN always selected the first entry of persistent multi-card
grids. The latter is policy state, not an engine-index defect: Java oracle
commands retain the original compact grid indices, so the HTN now rotates
among least-recently chosen grid entries. The frozen A0 gate remains 982 GREEN
out of 1,000 with no accepted GREEN regression.

## Integrated representation changes

- The dungeon map and immutable card pools use `Arc` snapshots.
- encounter, elite, event, shrine, and one-time-event lists use a small COW
  vector wrapper, detaching only on mutation.
- relic pools store typed `RelicId` values behind `Arc`, avoiding cloned
  strings and repeated ID parsing in search branches.
- each cloned `Game` shares its immutable unlock profile.
- the HTN anti-stall window uses structured keys in a `VecDeque`, avoiding
  formatted strings and front-removal shifts.

These changes preserve the flattened authoritative `Game` interface. A future
HTN-local checkpoint/undo or shallow-delta layer could target hot combat state
without introducing search-specific rollback semantics into ordinary gameplay.
