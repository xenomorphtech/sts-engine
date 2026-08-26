# Combat HRM artifacts

Run the standard experiment from the repository root with no training
hyperparameters:

```sh
cargo run --release --bin sts-hrm-train
```

The frontend expands and verifies the checked-in 500-puzzle Defect A0 Act 3
boss fixture, caches the prepared tensors, trains for 600 wall-clock seconds,
evaluates puzzle-level train/validation/test splits, and exports an FP16 ONNX
policy plus its Rust runtime manifest. Large generated `.jsonl`, `.pt`, and
`.onnx` files are ignored by Git. Small metrics and summary JSON files are
retained as machine-readable records.

The promoted 26 August 2026 default run used an RTX 5060 and produced:

- 37,804 decision examples from 500 exact winning trajectories;
- a deterministic 400/50/50 boss-stratified puzzle split;
- 49,814 optimizer updates in 600.00 seconds;
- 44.47% held-out action accuracy versus 20.56% legal-frequency and 25.14%
  expected uniform-legal baselines;
- 16.53 HP held-out progress mean absolute error.

RNG words are encoded by byte position and high nibble. This reduces the
training vocabulary from 27,595 to 6,307 tokens and held-out unknowns from
roughly 26,000 per split to 1,959 validation / 2,379 test without increasing
sequence length. The saved embedding budget is invested in a 192-wide shared
model: 2,521,153 parameters rather than sparse seed-specific lookup rows.

Run closed-loop evaluation with:

```sh
cargo run --release --bin sts-hrm-eval
```

The native Rust/ONNX measured rollout produced 190 wins, 304 deaths, and six
detected Grid cycles: 38.00% overall wins and 34.00% on the held-out 50-puzzle
test split. There were no unknown-action fallbacks. The complete 500-puzzle run
took 29.64 seconds on the RTX 5060. Cycles remain visible as caps rather than
being counted as losses. The previous ten-minute model won 175/500 overall and
25/100 validation-plus-test puzzles; the promoted model wins 190/500 overall
and 35/100 validation-plus-test puzzles.

See `combat-hrm-10m-metrics.json` for offline imitation metrics,
`combat-hrm-10m-rollout-summary.json` for aggregate closed-loop results, and
`combat-hrm-10m-rollouts.tsv` or `.jsonl` for every seed. The research design is
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
