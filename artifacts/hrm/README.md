# Combat HRM artifacts

Run the standard experiment from the repository root with no training
hyperparameters:

```sh
cargo run --release --bin sts-hrm-train
```

The frontend expands and verifies the checked-in 500-puzzle Defect A0 Act 3
boss fixture, caches the prepared tensors, trains for 300 wall-clock seconds,
evaluates puzzle-level train/validation/test splits, and exports an FP16 ONNX
policy plus its Rust runtime manifest. Large generated `.jsonl`, `.pt`, and
`.onnx` files are ignored by Git. Small metrics and summary JSON files are
retained as machine-readable records.

The 26 August 2026 default run used an RTX 5060 and produced:

- 37,804 decision examples from 500 exact winning trajectories;
- a deterministic 400/50/50 boss-stratified puzzle split;
- 35,951 optimizer updates in 300.00 seconds;
- 40.18% held-out action accuracy versus 20.56% legal-frequency and 25.14%
  expected uniform-legal baselines;
- 16.38 HP held-out progress mean absolute error.

Run closed-loop evaluation with:

```sh
cargo run --release --bin sts-hrm-eval
```

The native Rust/ONNX measured rollout produced 173 wins, 322 deaths, and five
detected Grid cycles: 34.60% overall wins and 32.00% on the held-out 50-puzzle
test split. There were no unknown-action fallbacks. The complete 500-puzzle run
took 21.62 seconds on the RTX 5060, down from 51.03 seconds for the former
Python-bridged evaluator. Cycles remain visible as caps rather than being
counted as losses.

See `combat-hrm-5m-metrics.json` for offline imitation metrics,
`combat-hrm-5m-rollout-summary.json` for aggregate closed-loop results, and
`combat-hrm-5m-rollouts.tsv` or `.jsonl` for every seed. The research design is
in `docs/research/hrm-combat-training-reference.html`.

## Outcome-aware branch experiment

The native branch scorer forced every legal opening action in the 400 training
split puzzles and let the unchanged five-minute policy complete each fight. It
produced 5,384 counterfactual continuations: 1,692 wins, 3,606 losses, and 86
caps. All 400 states had distinct action outcomes, including 144 states where
no candidate won and monster-HP progress still supplied a ranking signal.

A conservative ten-second trial trained only the 12,672-parameter action head,
mixing soft branch preferences with the original imitation objective. Branch
best-action accuracy improved from 16.00% to 19.75%, but closed-loop wins fell
from 173 to 142. The checkpoint was rejected. A single global action-head update
also changes all later actions, while these counterfactual labels assume the old
policy for every continuation; this off-policy mismatch overwhelms the opening
improvement. The next experiment should collect branches throughout current
policy trajectories and refresh them between policy updates, or learn a
separate action-value/ranking head instead of directly replacing policy logits.
