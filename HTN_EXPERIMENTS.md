# HTN A0 experiments

The concise strategy ledger, including discarded ideas and genuine synergy
groups, is maintained in [`HTN_STRATEGY_LOG.html`](HTN_STRATEGY_LOG.html).

The historical runs in the first table use Defect, 100 seeds,
`--max-steps 100000`, and the release build. New optimization runs use 500
seeds and a 5,000-step cap: a normal run needs hundreds of decisions, so every
capped seed is a loop bug to diagnose rather than an outcome to hide behind a
large limit. A thesis is first replayed against the fixed cohort where its
failure was observed; if it improves that diagnosed behavior, it is then
evaluated on a newly randomized cohort. Seed sources make randomized cohorts
exactly replayable without making the final evaluation reuse a tuning cohort.

The runs through source `1787318422655767662` predate the boss-relic
acquisition fix and are retained as historical strategy comparisons only. The
engine exposed the same boss reward after it was taken, so HTN selected it
twice; energy relics could therefore grant two energy and every final relic
list contained duplicate boss relics. Current milestones start from the
single-acquisition cohorts below and are not numerically comparable to the old
15% milestone.

| Thesis | Historical win rate | Historical mean floor | Fresh cohort | Fresh win rate | Fresh mean floor | Decision |
|---|---:|---:|---|---:|---:|---|
| Baseline | 8% | 27.95 | source `1787312088712518379` | 5% | 28.28 | reference |
| Pathing: value inaccessible shops at -25 | 6% | 28.73 | not run | — | — | rejected: historical wins regressed |
| Pathing: stronger campfire preference (50/25, pre-boss 70) | 9% | 27.88 | source `1787312242685583510` | 5% | 27.23 | rejected: fresh mean floor regressed |
| Pathing: expose real shop stock and buy scored items | 10% | 29.23 | source `1787312389845730400` | 7% | 30.68 | accepted |
| Orb management: charge Biased Cognition for triangular Focus decay | 10% | 29.79 | source `1787312470469267103` | 14% | 30.63 | accepted; seed 13 improved floor 31→47 |
| Rebound: exact power lifecycle and score the next card | 10% | 29.79 | source `1787312975814781466` | 5% | 30.28 | accepted for correctness; no sampled run acquired Rebound |
| Events: reject lethal HP loss; specialize Winding Halls/Forgotten Altar | 12% | 29.94 | source `1787313234548553304` | 8% | 30.51 | accepted; seed 32 improved floor 44→46 |
| Combat: score Strength/Weak/Vulnerable/Intangible and enemy Strength gain | 14% | 30.72 | source `1787313410802255679` | 17% | 32.94 | promising, but cohort had one capped seed |
| Combat legality: Medical Kit/Blue Candle and zero-progress loop guard | 13% | 30.32 | replay source `1787313410802255679` | 20% | 32.98 | accepted; eliminated cap (seed `8204511003183509530`) |
| Validation after loop fix | — | — | source `1787314071256080154` | 12% | 30.51 | fresh reference for target diagnosis; zero caps |
| Targeting: prioritize Donu, Cultists, Bronze Orbs, and Torch Heads | replay source `1787314071256080154`: 14% | 30.57 | source `1787314207414780189` | **15%** | 30.01 | accepted; zero caps and three diagnosed Donu losses became wins |

## 500-seed optimization runs

| Thesis | Cohort | Before win rate | Before mean floor | After win rate | After mean floor | Caps | Decision |
|---|---|---:|---:|---:|---:|---:|---|
| Delayed Self Repair healing is invisible to the shallow turn search | fixed source `1787315208666760241` | 11.60% (58/500) | 29.76 | 12.40% (62/500) | 29.99 | 0 | accepted; seed `6156007182303515596` changed from floor-31 loss to win, seed `1883842478669890359` improved floor 31→42 |
| Delayed Self Repair healing is invisible to the shallow turn search | fresh source `1787316226187148540` | 8.40% (42/500) | 29.12 | 11.00% (55/500) | 29.66 | 0 | accepted; paired milestone binary against changed binary |
| Shop strategy: prioritize Strike purge at value 330 | fixed source `1787315208666760241` | 12.40% (62/500) | 29.99 | 14.20% (71/500) | 30.17 | 0 | rejected after fresh cohort did not generalize |
| Shop strategy: prioritize Strike purge at value 330 | fresh source `1787316573696228393` | 13.40% (67/500) | 30.79 | 13.40% (67/500) | 30.41 | 0 | rejected: same wins, lower mean floor |
| Shop strategy: moderate Strike purge value 240 | fixed source `1787315208666760241` | 12.40% (62/500) | 29.99 | 14.80% (74/500) | 30.47 | 0 | rejected on the previously observed fresh cohort |
| Shop strategy: moderate Strike purge value 240 | replay source `1787316573696228393` | 13.40% (67/500) | 30.79 | 13.00% (65/500) | 30.55 | 0 | rejected; no new random cohort warranted |
| Delayed Buffer prevention | fixed source `1787315208666760241` | 12.40% (62/500) | 29.99 | 12.80% (64/500) | 29.81 | 0 | tested fresh because Automaton seed `1883842478669890359` improved floor 42→47 |
| Delayed Buffer prevention | fresh source `1787316759813919465` | 10.40% (52/500) | 29.81 | 9.80% (49/500) | 29.68 | 0 | rejected |
| Delayed Loop output | fixed source `1787315208666760241` | 12.40% (62/500) | 29.99 | 11.60% (58/500) | 29.65 | 0 | rejected before fresh cohort |
| Delayed Machine Learning draw | fixed source `1787315208666760241` | 12.40% (62/500) | 29.99 | 12.40% (62/500) | 30.09 | 0 | accepted provisionally; three wins gained and three lost, with higher mean floor |
| Delayed Machine Learning draw | fresh source `1787316864833023929` | 11.40% (57/500) | 29.92 | 11.60% (58/500) | 30.03 | 0 | accepted; small consistent paired improvement |
| Draft synergy: require two Frost sources before taking Blizzard | fixed source `1787315208666760241` | 12.40% (62/500) | 30.09 | 13.80% (69/500) | 30.62 | 0 | tested fresh after a clear fixed-cohort gain |
| Draft synergy: require two Frost sources before taking Blizzard | fresh source `1787317200265831039` | 14.40% (72/500) | 30.68 | 14.00% (70/500) | 30.77 | 0 | rejected: two fewer wins despite slightly higher mean floor |
| Broad card/shop/boss package planner v1 | fixed source `1787315208666760241` | 12.40% (62/500) | 30.09 | 11.80% (59/500) | 30.04 | 0 | rejected as a bundle and decomposed by acquisition context |
| Deck-aware boss relic priorities v1 | fixed source `1787315208666760241` | 12.40% (62/500) | 30.09 | 9.60% (48/500) | 28.98 | 0 | rejected: package bonuses over-selected Inserter instead of reliable energy relics |
| Deck-aware shop relic value | fixed source `1787315208666760241` | 12.40% (62/500) | 30.09 | 12.80% (64/500) | 30.09 | 0 | advanced to independent cohorts |
| Deck-aware shop relic value | replay source `1787317200265831039` | 14.40% (72/500) | 30.68 | 14.40% (72/500) | 30.67 | 0 | neutral confirmation; advanced to a new random cohort |
| Deck-aware shop relic value | fresh source `1787318292151234368` | 11.40% (57/500) | 29.99 | 11.80% (59/500) | 30.16 | 0 | accepted |
| Card-package HTN on top of deck-aware shops | fixed source `1787315208666760241` | 12.80% (64/500) | 30.09 | 14.40% (72/500) | 31.21 | 0 | advanced to independent cohorts |
| Card-package HTN on top of deck-aware shops | replay source `1787317200265831039` | 14.40% (72/500) | 30.67 | 15.40% (77/500) | 31.20 | 0 | crossed 15% on an observed independent source |
| Card-package HTN on top of deck-aware shops | fresh source `1787318422655767662` | 11.40% (57/500) | 30.01 | 11.60% (58/500) | 30.31 | 0 | accepted; smaller but positive paired generalization |

## Corrected single-boss-relic baseline

| Thesis | Cohort | Before win rate | Before mean floor | After win rate | After mean floor | Caps | Decision |
|---|---|---:|---:|---:|---:|---:|---|
| Card-package HTN plus deck-aware shop relics | fixed source `1787315208666760241` | 3.00% (15/500) | 27.58 | 5.40% (27/500) | 28.54 | 0 | accepted after boss relics were limited to one acquisition |
| Card-package HTN plus deck-aware shop relics | fresh source `1787318856394675018` | 1.80% (9/500) | 27.65 | 3.80% (19/500) | 28.21 | 0 | accepted; paired generalization on the corrected engine |
| Conservative energy-first boss relic ordering | fixed source `1787315208666760241` | 5.40% (27/500) | 28.54 | 7.20% (36/500) | 29.19 | 0 | advanced after a strong observed gain |
| Conservative energy-first boss relic ordering | replay source `1787318856394675018` | 3.80% (19/500) | 28.21 | 4.60% (23/500) | 28.81 | 0 | advanced after a second observed gain |
| Conservative energy-first boss relic ordering | fresh source `1787319073388804697` | 4.80% (24/500) | 28.22 | 4.40% (22/500) | 28.38 | 0 | rejected: two fewer wins despite slightly higher mean floor |
| Require enabling packages before Claw, Tempest, and Fusion | fixed source `1787315208666760241` | 5.40% (27/500) | 28.54 | 5.60% (28/500) | 28.74 | 0 | advanced |
| Require enabling packages before Claw, Tempest, and Fusion | replay source `1787318856394675018` | 3.80% (19/500) | 28.21 | 4.60% (23/500) | 28.38 | 0 | advanced |
| Require enabling packages before Claw, Tempest, and Fusion | replay source `1787319073388804697` | 4.80% (24/500) | 28.22 | 4.80% (24/500) | 28.49 | 0 | neutral wins with higher mean floor |
| Require enabling packages before Claw, Tempest, and Fusion | fresh source `1787319499337259007` | 4.40% (22/500) | 28.55 | 4.80% (24/500) | 28.42 | 0 | accepted: two more wins with a small depth tradeoff |
| Delay Strike purge until replacement damage and engine support exist | fixed source `1787315208666760241` | 5.60% (28/500) | 28.74 | 6.40% (32/500) | 28.81 | 0 | advanced |
| Delay Strike purge until replacement damage and engine support exist | replay source `1787318856394675018` | 4.60% (23/500) | 28.38 | 4.60% (23/500) | 28.46 | 0 | neutral wins with higher mean floor |
| Delay Strike purge until replacement damage and engine support exist | replay source `1787319073388804697` | 4.80% (24/500) | 28.49 | 5.00% (25/500) | 28.50 | 0 | improved |
| Delay Strike purge until replacement damage and engine support exist | replay source `1787319499337259007` | 4.80% (24/500) | 28.42 | 4.60% (23/500) | 28.39 | 0 | one-cohort regression; advanced to a new random cohort because prior evidence was positive |
| Delay Strike purge until replacement damage and engine support exist | fresh source `1787319767048101927` | 4.20% (21/500) | 28.83 | 4.40% (22/500) | 28.99 | 0 | accepted |
| Lower the global elite-strength gate while retaining matchup checks | fixed source `1787315208666760241` | 6.40% (32/500) | 28.81 | 4.40% (22/500) | 27.80 | 0 | rejected before fresh testing |
| Retest delayed Buffer prevention after single-boss-relic correction | fixed source `1787315208666760241` | 6.40% (32/500) | 28.81 | 6.20% (31/500) | 28.74 | 0 | rejected before fresh testing |
| Bounded turn beam search (width 8, depth 6) | fixed source `1787315208666760241` | 6.40% (32/500) | 28.81 | 7.60% (38/500) | 30.39 | 0 | advanced after improving wins and depth |
| Bounded turn beam search (width 8, depth 6) | replay source `1787318856394675018` | 4.60% (23/500) | 28.46 | 7.80% (39/500) | 30.04 | 0 | advanced after independent confirmation |
| Bounded turn beam search (width 8, depth 6) | fresh source `1787320322822582653` | 5.80% (29/500) | 29.52 | 7.60% (38/500) | 30.79 | 0 | accepted; paired fresh validation, with runtime increasing from 9s to 41s per 500 seeds |
| Delay Champ's half-HP transition until a deep burst | fixed source `1787315208666760241` | 7.60% (38/500) | 30.39 | 7.60% (38/500) | 30.46 | 0 | advanced after Champ deaths fell 74→71 and three diagnosed Champ losses became wins |
| Delay Champ's half-HP transition until a deep burst | replay source `1787318856394675018` | 7.80% (39/500) | 30.04 | 8.00% (40/500) | 30.19 | 0 | advanced after a small independent gain |
| Delay Champ's half-HP transition until a deep burst | fresh source `1787320907338841119` | 6.20% (31/500) | 30.58 | 6.00% (30/500) | 30.57 | 0 | rejected: one fewer win and slightly lower mean floor |
| Spend long-duration potions at the start of boss fights | fixed source `1787315208666760241` | 7.60% (38/500) | 30.39 | 8.60% (43/500) | 30.58 | 0 | advanced after improving wins and depth |
| Spend long-duration potions at the start of boss fights | replay source `1787318856394675018` | 7.80% (39/500) | 30.04 | 9.00% (45/500) | 30.36 | 0 | advanced after independent confirmation |
| Spend long-duration potions at the start of boss fights | fresh source `1787321283377521703` | 5.60% (28/500) | 29.24 | 6.20% (31/500) | 29.28 | 0 | accepted; improved all three paired cohorts |
| Require X-cost support before taking Double Energy | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 8.00% (40/500) | 30.48 | 0 | rejected before fresh testing; raw card/outcome correlation did not identify a beneficial gate |
| Spend long-duration potions at elite openings as well as bosses | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 7.80% (39/500) | 30.68 | 0 | rejected before fresh testing; improved depth did not offset four fewer wins |
| Extend bounded turn beam depth from 6 to 8 | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 8.60% (43/500) | 30.58 | 0 | rejected before fresh testing; five loss floors changed in mixed directions and no win changed |
| Broad engine-critical Defect upgrade tier | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 8.20% (41/500) | 30.21 | 0 | rejected as a bundle; new priorities displaced proven Defragment, Glacier, and Self Repair upgrades |
| Upgrade Fission and Biased Cognition below the proven core | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 8.80% (44/500) | 30.59 | 0 | advanced after a small paired gain |
| Upgrade Fission and Biased Cognition below the proven core | replay source `1787318856394675018` | 9.00% (45/500) | 30.36 | 9.60% (48/500) | 30.42 | 0 | advanced after independent improvement |
| Upgrade Fission and Biased Cognition below the proven core | fresh source `1787322079969279673` | 6.40% (32/500) | 30.15 | 6.20% (31/500) | 30.00 | 0 | rejected: one fewer win and lower mean floor |
| Raise Act 2 rest thresholds to 88%, and 90% near its boss | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 7.80% (39/500) | 30.49 | 0 | rejected before fresh testing; extra health did not offset displaced upgrades |
| Add a known-Act-2-boss acquisition task | fixed source `1787315208666760241` | 8.60% (43/500) | 30.58 | 8.80% (44/500) | 30.78 | 0 | advanced after improving wins and depth |
| Add a known-Act-2-boss acquisition task | replay source `1787318856394675018` | 9.00% (45/500) | 30.36 | 9.00% (45/500) | 30.35 | 0 | neutral wins with a 0.01 mean-floor tradeoff |
| Add a known-Act-2-boss acquisition task | fresh source `1787322574873510016` | 7.20% (36/500) | 30.81 | 7.40% (37/500) | 30.74 | 0 | accepted; one more win with a 0.07 mean-floor tradeoff |
