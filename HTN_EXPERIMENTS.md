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
| Extend known-boss acquisition to Act 1 | fixed source `1787315208666760241` | 8.80% (44/500) | 30.78 | 9.80% (49/500) | 30.91 | 0 | advanced after improving wins and depth |
| Extend known-boss acquisition to Act 1 | replay source `1787318856394675018` | 9.00% (45/500) | 30.35 | 9.80% (49/500) | 30.88 | 0 | advanced after independent confirmation |
| Extend known-boss acquisition to Act 1 | fresh source `1787322961599519915` | 7.80% (39/500) | 29.91 | 8.80% (44/500) | 30.50 | 0 | accepted; improved all three paired cohorts |
| Extend known-boss acquisition to Act 3 | fixed source `1787315208666760241` | 9.80% (49/500) | 30.91 | 9.80% (49/500) | 30.91 | 0 | rejected before fresh testing; 23 decks changed, but one win was gained and one lost |
| Demote Busted Crown below neutral boss relics | fixed source `1787315208666760241` | 9.80% (49/500) | 30.91 | 9.80% (49/500) | 30.95 | 0 | advanced after Busted Crown picks fell 9→1 and mean depth improved |
| Demote Busted Crown below neutral boss relics | replay source `1787318856394675018` | 9.80% (49/500) | 30.88 | 9.80% (49/500) | 30.89 | 0 | advanced after a second neutral-positive cohort |
| Demote Busted Crown below neutral boss relics | fresh source `1787323430036114759` | 8.40% (42/500) | 30.56 | 8.60% (43/500) | 30.64 | 1 | accepted; one more win and higher depth, while the same Spheric Guardian cap occurred before and after this relic-only change |
| Value all enemy block removal at 55% of HP damage | fixed source `1787315208666760241` | 9.80% (49/500) | 30.95 | 9.20% (46/500) | 31.05 | 0 | rejected: fixed depth improved but three wins were lost |
| Value block removal for every Barricade enemy | fixed source `1787315208666760241` | 9.80% (49/500) | 30.95 | 10.20% (51/500) | 31.12 | 0 | refined after a strong first-cohort result |
| Value block removal for every Barricade enemy | replay source `1787318856394675018` | 9.80% (49/500) | 30.89 | 9.60% (48/500) | 30.84 | 0 | refined to block-wall states after losing a win and depth |
| Value retained block only once it walls off remaining HP | fixed source `1787315208666760241` | 9.80% (49/500) | 30.95 | 10.40% (52/500) | 31.11 | 0 | advanced after three more wins and higher depth |
| Value retained block only once it walls off remaining HP | replay source `1787318856394675018` | 9.80% (49/500) | 30.89 | 9.60% (48/500) | 30.89 | 0 | one-win regression with neutral depth; advanced because it fixes a confirmed cap |
| Value retained block only once it walls off remaining HP | fresh source `1787323430036114759` | 8.60% (43/500) | 30.64 | 9.40% (47/500) | 30.60 | 0 | accepted; four more wins and capped seed `1792856469989672380` now terminates normally in 980 steps |
| Hold Biased Cognition for Champ's second phase | fixed source `1787315208666760241` | 10.40% (52/500) | 31.11 | 11.00% (55/500) | 31.38 | 0 | advanced after three more wins and higher depth |
| Hold Biased Cognition for Champ's second phase | replay source `1787318856394675018` | 9.60% (48/500) | 30.89 | 10.00% (50/500) | 31.05 | 0 | advanced after independent confirmation |
| Hold Biased Cognition for Champ's second phase | fresh source `1787324441138756002` | 9.00% (45/500) | 30.74 | 10.20% (51/500) | 31.07 | 0 | accepted; improved all three paired cohorts |
| Hold Biased Cognition through Automaton's first two turns | fixed source `1787315208666760241` | 11.00% (55/500) | 31.38 | 11.00% (55/500) | 31.38 | 0 | rejected before fresh testing; aggregate results were identical and only nine steps changed |
| End combat when only Minion-power summons remain | fixed source `1787315208666760241` | 11.00% (55/500) | 31.38 | 11.00% (55/500) | 31.39 | 0 | accepted for engine correctness; seed `8051295551183191776` advanced from floor 27 to the Act 2 boss |
| End combat when only Minion-power summons remain | replay source `1787318856394675018` | 10.00% (50/500) | 31.05 | 10.00% (50/500) | 31.06 | 0 | accepted for engine correctness; seed `8629714848339533862` advanced from floor 22 to floor 30 |
| End combat when only Minion-power summons remain | fresh source `1787325008170000000` | 7.40% (37/500) | 31.07 | 7.40% (37/500) | 31.07 | 0 | accepted for correctness with neutral aggregate results; one Act 3 win changed in each direction |

## 1,000-seed cohort runs

The goal moved to 1,000-seed cohorts (same paired methodology: fixed source
`1787315208666760241` for observation, fresh sources for acceptance).

| Thesis | Cohort | Before win rate | Before mean floor | After win rate | After mean floor | Caps | Decision |
|---|---|---:|---:|---:|---:|---:|---|
| Kind-aware grid policy (purge worst, upgrade best, retrieve best; search resolves in-combat grids) | fixed source `1787315208666760241` | 10.10% | 31.38 | 11.10% | 32.00 | 0 | advanced |
| Kind-aware grid policy | fresh source `1787327776218180251` | 10.60% | 31.72 | 11.20% | 32.42 | 0 | accepted |
| Dark orb growth term in orb_value | fixed source `1787315208666760241` | 11.10% | 32.00 | 11.10% | 31.90 | 0 | rejected: dark orbs too rare for the term to matter; drafting is the bottleneck |
| A20H tier-list pick recalibration (bundle) | fixed source `1787315208666760241` | 11.10% | 32.00 | 10.60% | 31.56 | 0 | rejected; demotions-only variant 10.40%/31.82 and raises-only variant 9.60%/31.13 both also regressed — the hill-climbed A0 table beats A20H tier lists |
| More smithing (rest thresholds 0.6/0.68) | fixed source `1787315208666760241` | 11.10% | 32.00 | 10.40% | 31.40 | 0 | rejected: HP matters more than upgrades here |
| More resting (rest thresholds 0.75/0.82) + ConvertDark deck task | fresh source `1787328951980394648` | 10.30% | 31.55 | 9.50% | 31.52 | 0 | rejected on fresh cohort after +3 fixed-cohort wins; overfit |
| Two-turn beam lookahead (also stratified variant) | fixed source `1787315208666760241` | 11.10% | 32.00 | 8.90% | 30.97 | 0 | rejected: single-turn eval heuristics beat noisy cross-turn rollouts; 5x slower |
| Rank Neow blessings (RemoveTwo/TransformTwo/rare relics over first option) | fixed source `1787315208666760241` | 11.10% | 32.00 | 12.00% | 31.94 | 0 | advanced |
| Rank Neow blessings | fresh source `1787329344052076934` | 11.10% | 32.04 | 12.00% | 32.25 | 0 | accepted |
| Event policy: free shrines, Library read, Cleric purify, Shining Light, fight Masked Bandits | fixed source `1787315208666760241` | 12.00% | 31.94 | 13.00% | 32.15 | 0 | advanced; Golden Idol (121) and Scrap Ooze (127) variants regressed and stay skipped |
| Event policy | fresh source `1787329866870376876` | 12.40% | 32.28 | 13.40% | 32.24 | 0 | accepted |

## Evolution-strategy parameter optimization

Policy constants moved behind `STS_HTN_PARAMS` (see tools/opt_params*.py);
each phase's best mean is baked into `tools/params_default.json`. Honest
numbers are brand-new cohorts never touched during search or validation.

| Phase | Space | Search result | Honest fresh 1k | Decision |
|---|---|---|---:|---|
| 1: 38 policy scalars, (3,10)-ES, fixed 500-seed cohort | combat eval, orb values, map/rest/elite gates, pick threshold | 34.10%/38.32 on its search cohort | 31.20%/38.32 (`1787332551475131966`) | baked; damage-chasing fell ~70%, smith-earlier rests, draft threshold 85→65 |
| 1b: energy boss relic priority (+40 until the run has one) | hand-written on top of phase 1 | +1.0pp fixed | +0.7pp | accepted; 99% of wins vs 76% of losses carried an energy boss relic |
| 2: +117 dims (per-card pick/upgrade/boss-relic tables, deck shape, fight lengths), fresh search cohort | drafting tables | 40.20%/39.91 search, 40.10%/39.71 phase-1 cohort | 35.20%/39.11 (`1787336748186657062`) | baked; ~5pp cohort adaptation observed, and the sticky-incumbent acceptance gate stalled after gen 15 |
| 3: per-generation random cohorts, mirrored sampling, pure (μ/μ,λ), + search width/depth and potion thresholds | all of the above | running | — | — |

## A20 improvement-plan execution

Plan items are evaluated on reproducible 1,000-seed random samples. The first
sample uses `--random-seeds --seed-source 20260822`; every before/after binary
receives the same generated seed list.

| Thesis | Cohort | Before win rate | Before mean floor | After win rate | After mean floor | Caps | Decision |
|---|---|---:|---:|---:|---:|---:|---|
| D1: effective end-of-turn block in mid-turn scoring (Frost, Frozen Core, Cables, Metallicize, Plated Armor, Orichalcum) | random source `20260822` | 7.80% (78/1000) | 23.88 | 8.80% (88/1000) | 23.94 | 0 | advanced |
| D2: deduplicate equivalent beam-frontier states, on top of D1 | random source `20260822` | 8.80% (88/1000) | 23.94 | 9.40% (94/1000) | 24.26 | 0 | accepted as the D1+D2 bundle; baseline→bundle had 30 loss→win and 14 win→loss flips (exact McNemar p=0.022629), with floors improving on 82 seeds, worsening on 47, and unchanged on 871 |
| E1: score next-turn exposure at weight 0.05, on top of D1+D2 | random source `20260822` | 9.40% (94/1000) | 24.26 | 10.70% (107/1000) | 24.65 | 0 | advanced; 38 loss→win and 25 win→loss flips (exact McNemar p=0.129918) |
| E1: score next-turn exposure at weight 0.05 | fresh random source `20260823` | 12.00% (120/1000) | 24.64 | 12.20% (122/1000) | 24.93 | 0 | accepted; small fresh win gain, +0.29 mean floor, 35 loss→win and 33 win→loss flips |
| A11 fidelity: reduce base potion capacity from 3 to 2 | random source `20260822` | 10.70% (107/1000) | 24.65 | 9.60% (96/1000) | 23.89 | 0 | accepted for engine correctness; expected measured regression after removing an extra potion slot |
| A20 fidelity: fight the second Beyond boss with a real room transition and no heal | random source `20260822` | 9.60% (96/1000) | 23.89 | 6.10% (61/1000) | 23.95 | 0 | accepted for engine correctness; 35 apparent wins failed the newly required second boss, while the extra room slightly raised mean floor |
| A20 fidelity: replace the Time Eater→Hexaghost fallback with the real A20 fight | random source `20260822` | 6.10% (61/1000) | 23.95 | 1.10% (11/1000) | 23.86 | 0 | accepted for engine correctness; exact decompiled HP/moves/Haste/Time Warp expose how strongly the prior trivial proxy inflated results |
| Gate ruby/emerald/sapphire key chasing while Act 4 is unreachable | random source `20260822` | 1.10% (11/1000) | 23.86 | 1.90% (19/1000) | 24.05 | 0 | advanced; avoids costs that cannot affect the current Act-3 win condition |
| Gate unreachable Act-4 keys | fresh random source `20260823` | 1.20% (12/1000) | 24.27 | 1.30% (13/1000) | 24.68 | 0 | accepted; fresh win gain is marginal but the +0.41 mean-floor gain agrees with the fixed cohort and the removed objective is provably dead |
| Exclude unplayable curses/statuses from actionable deck size | random source `20260822` | 1.90% (19/1000) | 24.05 | 1.90% (19/1000) | 24.22 | 0 | retained as metric correctness; win-neutral, modest fixed-cohort floor gain |
| Exclude unplayable curses/statuses from actionable deck size | fresh random source `20260823` | 1.30% (13/1000) | 24.68 | 1.30% (13/1000) | 24.67 | 0 | neutral confirmation; no win change and effectively flat mean floor |
| D3: exact same-turn lethal search over Attack plays | random source `20260822` | 1.90% (19/1000) | 24.22 | 2.10% (21/1000) | 24.30 | 0 | advanced; two more wins and higher mean floor |
| D3: exact same-turn lethal search over Attack plays | fresh random source `20260823` | 1.30% (13/1000) | 24.67 | 1.70% (17/1000) | 24.61 | 0 | accepted; 6 loss→win and 2 win→loss flips, with 24 floor gains, 24 regressions, and 952 unchanged; throughput 44.1→43.9 seeds/s |
| A20 fidelity: replace the Spire Growth→Cultist fallback with the real fight | random source `20260822` | 2.10% (21/1000) | 24.30 | 2.10% (21/1000) | 24.24 | 0 | accepted for engine correctness; exact 190 HP, A17 forced Constrict 12, move recursion, and persistent end-turn damage; eight losses now end at Spire Growth |
| A20 fidelity: replace the Nemesis→Gremlin Nob fallback with the real fight | random source `20260822` | 2.10% (21/1000) | 24.24 | 2.10% (21/1000) | 24.22 | 0 | accepted for engine correctness; exact 200 HP, A18 five-Burn debuff, 45-damage Scythe cooldown, and alternating monster Intangible; one loss now ends at Nemesis |
| A20 fidelity: replace the Reptomancer→Gremlin Nob fallback and classify all Act-3 elites correctly | random source `20260822` | 2.10% (21/1000) | 24.22 | 1.60% (16/1000) | 24.12 | 0 | accepted for engine correctness; exact 190–200 HP, A18 two-dagger summons, dagger Wound/suicide lifecycle, and minion cleanup; ten losses now end at Reptomancer, with five win→loss flips |
| A20 fidelity: replace the Writhing Mass→Cultist fallback with the real fight | random source `20260822` | 1.60% (16/1000) | 24.12 | 1.10% (11/1000) | 24.07 | 0 | accepted for engine correctness; exact 175 HP, five-move recursion, Malleable, Compulsive rerolls, and permanent Parasite implant; five win→loss flips surface downstream after the harder hallway |
| A20 fidelity: Bronze Automaton A4/A9/A19 tiers | random source `20260822` | 1.10% (11/1000) | 24.07 | 0.40% (4/1000) | 23.28 | 0 | accepted for engine correctness; 320 HP, 8×2 Flail, 50-damage HYPER BEAM, and the A19 post-beam Boost pattern cause seven win→loss flips, five directly at Automaton |
| A20 fidelity: Awakened One A9/A19 tiers and two-form lifecycle | random source `20260822` | 0.40% (4/1000) | 23.28 | 0.30% (3/1000) | 23.26 | 0 | accepted for engine correctness; two 320-HP forms, Regen 15, Curiosity 2, exact rebirth cleanup, and Dark Echo opening cause two direct win→loss flips and one downstream loss→win flip |
| A20 fidelity: remaining A7/A8/A9 monster HP tiers | random source `20260822` | 0.30% (3/1000) | 23.26 | 0.10% (1/1000) | 23.26 | 0 | accepted for engine correctness; audited normal/elite/boss endpoint HP ranges cause two win→loss flips while mean floor changes by less than 0.01 |
| A20 fidelity: remaining late-monster A2/A3/A4/A17/A18/A20 move tiers | random source `20260822` | 0.10% (1/1000) | 23.26 | 0.10% (1/1000) | 21.77 | 0 | accepted for engine correctness; no win flips, but exact Chosen through Giant Head move values and recursions make 189 runs die earlier and only 25 advance farther |
| Scripted boss-spike table and pre-spike overblock suppression, weight 10 / horizon 2 | random source `20260822` | 0.10% (1/1000) | 21.77 | 0.10% (1/1000) | 21.81 | 0 | advanced to an independent cohort after three floor gains and no regressions |
| Scripted boss-spike table, weight 10 / horizon 2 | fresh random source `20260823` | 0.00% (0/1000) | 22.05 | 0.00% (0/1000) | 22.00 | 0 | rejected as an active default; two floor gains versus five regressions did not generalize |
| Scripted boss-spike magnitude sweep, weights 2 and 5 / horizon 2 | replay sources `20260822` + `20260823` | combined 0.05% (1/2000) | 21.91 | combined 0.05% (1/2000) | 21.91 / 21.92 | 0 | no robust magnitude: weight 2 was aggregate-neutral and weight 5 had four floor gains/four regressions; schedule infrastructure retained with zero default for future ES |
| Context-aware target priorities: stasis orbs, Gremlin Leader inversion, Taskmaster, and Sentry intent | random source `20260822` | 0.10% (1/1000) | 21.77 | 0.10% (1/1000) | 21.53 | 0 | rejected before fresh testing; 16 floor gains versus 34 regressions, so the prior validated static priorities remain active |
| Penalize newly gained combat Status cards at 12 | random source `20260822` | 0.10% (1/1000) | 21.77 | 0.10% (1/1000) | 21.84 | 0 | advanced; 26 floor gains versus 15 regressions, concentrated around status-producing fights |
| Penalize newly gained combat Status cards at 12 | fresh random source `20260823` | 0.00% (0/1000) | 22.05 | 0.10% (1/1000) | 22.14 | 0 | accepted; one loss→win, 22 floor gains versus 16 regressions, and +0.09 mean floor |
| Lagavulin early-wake guard, penalty 150 unless remaining HP is within 3× the turn's damage | random source `20260822` | 0.10% (1/1000) | 21.84 | 0.20% (2/1000) | 22.02 | 0 | advanced; one loss→win and 19 floor gains versus 9 regressions |
| Lagavulin early-wake guard | fresh random source `20260823` | 0.10% (1/1000) | 22.14 | 0.10% (1/1000) | 22.31 | 0 | accepted; win-neutral confirmation with 27 floor gains versus 13 regressions and +0.17 mean floor |
| Hexaghost pre-boss rest/smith rule using post-Divider effective healing | random source `20260822` | 0.20% (2/1000) | 22.02 | 0.20% (2/1000) | 22.02 | 0 | neutral: no decision changed in the sampled cohort; breakpoint-aware helper retained for optimizer control |
| Never spend Weak/Fear potions into Artifact | random source `20260822` | 0.20% (2/1000) | 22.02 | 0.20% (2/1000) | 22.01 | 0 | rejected; zero gains and one regression (seed `6041573289961355560`, floor 24→15), because stripping Artifact can enable later debuffs |
| P2: replace the weakest non-Fairy/non-Entropic potion when a reward/shop potion clears a 30-point margin | random source `20260822` | 0.20% (2/1000) | 22.02 | 0.20% (2/1000) | 22.09 | 0 | advanced; one loss→win and one win→loss, with 82 floor gains versus 61 regressions |
| P2: full-slot potion replacement at margin 30 | fresh random source `20260823` | 0.10% (1/1000) | 22.31 | 0.20% (2/1000) | 22.26 | 0 | accepted provisionally; two loss→win and one win→loss, with 81 floor gains versus 79 regressions; across both cohorts the bundle is +1 net win and +0.01 mean floor |
| P4: dump offensive potions in bosses from turn 3 at ≤120 total enemy HP | random source `20260822` | 0.20% (2/1000) | 22.09 | 0.20% (2/1000) | 22.12 | 0 | advanced; no win flips, 9 floor gains versus 8 regressions, +0.02 mean floor |
| P4: late-boss offensive potion dump | fresh random source `20260823` | 0.20% (2/1000) | 22.26 | 0.20% (2/1000) | 22.27 | 0 | accepted; no win flips, 12 floor gains versus 11 regressions, and a second small positive mean-floor result |
| P5: hold Weak/Fear potions until total incoming damage is at least 20 | random source `20260822` | 0.20% (2/1000) | 22.12 | 0.20% (2/1000) | 22.06 | 0 | rejected before fresh testing; 4 floor gains versus 12 regressions and −0.05 mean floor, because Fear's early Vulnerable value is not represented by incoming damage |
| P6: drink Entropic Brew with one open slot in a pre-boss normal fight | random source `20260822` | 0.20% (2/1000) | 22.12 | 0.20% (2/1000) | 22.09 | 0 | rejected before fresh testing; no gains and two floor regressions (−0.03 mean); the tunable general minimum remains at its behavior-preserving value of 2 |
| P3: on Normal-fight desperate turns, allow offense only when potion + exact Attack search proves lethal | random source `20260822` | 0.20% (2/1000) | 22.12 | 0.20% (2/1000) | 22.09 | 0 | rejected before fresh testing; one floor gain versus four regressions (−0.02 mean), so the existing emergency offense fallback remains active |
| Event-plan rows 1–7 as one bundle (Golden Idol, Drug Dealer, Woman in Blue, Goop, Falling, Scrap Ooze, Match-and-Keep) | random source `20260822` | 0.20% (2/1000) | 22.12 | 0.20% (2/1000) | 21.52 | 0 | rejected as a bundle; 115 floor gains versus 159 regressions and −0.60 mean floor show that the HP/gold gambles do not transfer safely to this agent |
| Safe event subset: Drug Dealer Study/Inject, removal-aware Falling, memory/curse-aware Match-and-Keep | random source `20260822` | 0.20% (2/1000) | 22.12 | 0.20% (2/1000) | 22.29 | 0 | advanced; 44 floor gains versus 29 regressions and +0.18 mean floor |
| Safe three-event subset | fresh random source `20260823` | 0.20% (2/1000) | 22.27 | 0.50% (5/1000) | 22.43 | 0 | accepted; three loss→win flips, no win→loss flips, 43 floor gains versus 28 regressions, and +0.17 mean floor |
| Event rows 8–9: Big Fish Donut at high HP + Designer removal-first | random source `20260822` | 0.20% (2/1000) | 22.29 | 0.30% (3/1000) | 22.13 | 0 | mixed: one loss→win but 11 floor gains versus 28 regressions and −0.16 mean floor |
| Event rows 8–9 bundle | fresh random source `20260823` | 0.50% (5/1000) | 22.43 | 0.40% (4/1000) | 22.34 | 0 | rejected; one win→loss, 19 floor gains versus 28 regressions, and −0.10 mean floor |
| Designer Full Service→Clean Up→Adjust ordering, isolated | sources `20260822` + `20260823` | combined 0.35% (7/2000) | 22.36 | combined 0.35% (7/2000) | 22.35 | 0 | rejected; one loss→win and one win→loss across cohorts, slightly negative floor on both |
| Big Fish Donut at ≥70% HP, isolated by paired difference from Designer-only outputs | sources `20260822` + `20260823` | combined 0.35% (7/2000) | 22.35 | combined 0.35% (7/2000) | 22.24 | 0 | rejected; no win flips and mean floor fell on both cohorts (−0.15/−0.08) |
| Static Deca-first + Taskmaster + Reptomancer-dagger target priorities | random source `20260822` | 0.20% (2/1000) | 22.29 | 0.20% (2/1000) | 22.27 | 0 | rejected before fresh testing; no win flips, 9 floor gains versus 8 regressions, and −0.02 mean floor; prior static targets remain active |
| Map elite matchups: frost-aware Book, AoE-aware Slavers/Reptomancer, scaling-aware Giant Head, big-hit-aware Nemesis | random source `20260822` | 0.20% (2/1000) | 22.29 | 0.20% (2/1000) | 22.33 | 0 | advanced; no win flips, 5 floor gains versus 2 regressions, +0.04 mean floor |
| Map elite-matchup bundle | fresh random source `20260823` | 0.50% (5/1000) | 22.43 | 0.50% (5/1000) | 22.44 | 0 | accepted; no win flips, 4 floor gains versus 5 regressions, but positive floor magnitude on a second cohort |
| Final engine-fidelity corrections: fixed-range HP RNG, Gremlin Leader slots/draw X, reactive Stasis, non-Attack Intangible, Hex/Pellets, and Letter Opener action order | random source `20260822` | 0.20% (2/1000) | 22.33 | 0.20% (2/1000) | 22.28 | 0 | accepted for correctness; exact A0 oracle regressions were cleared through registry seed 119, with the aggregate walk next stopping at a separate Orichalcum block mismatch on seed 135 |
| Final corrected-engine validation | fresh random source `20260823` | 0.50% (5/1000) | 22.44 | 0.40% (4/1000) | 22.46 | 0 | accepted for correctness; one fewer win but +0.02 mean floor, with no caps in either final 1,000-seed cohort |

The combined bundle increased the sample win rate by 1.6 percentage points and
mean floor by 0.38. At four workers it reduced throughput from 24.4 to 19.1
seeds/s, so subsequent search work should preserve the score gain while
recovering some of the frontier-hashing cost.

The E1 sweep initially exposed an evaluation-hygiene bug: a partial
`STS_HTN_PARAMS` JSON fell back to hand defaults for omitted fields instead of
overlaying the baked policy. The loader now recursively merges overrides onto
`params_default.json`, with nested-map coverage, so one-parameter sweeps test
the intended policy. Results collected before that fix were discarded.
