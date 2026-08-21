# HTN A0 experiments

All reported runs use Defect, 100 seeds, `--max-steps 100000`, and the release
build. A thesis is first replayed against the fixed cohort where its failure was
observed; if it improves that diagnosed behavior, it is then evaluated on a
newly randomized cohort. The 15% target is judged only on a fresh randomized
cohort. Seed sources make randomized cohorts exactly replayable without making
the final evaluation reuse a tuning cohort.

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
