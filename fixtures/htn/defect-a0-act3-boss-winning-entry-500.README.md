# Defect A0 Act 3 boss-entry winning checkpoints

`defect-a0-act3-boss-winning-entry-500.jsonl.xz` contains 500 unique Defect A0
states at the opening of the Act 3 boss combat. It expands to JSONL; every
selected seed eventually wins under the current HTN policy.

Each schema-v2 JSONL record contains:

- `state`: the structured game, dungeon/map, player, combat/monster, card-pile,
  orb, relic, potion, power, legal-action, and RNG state used as model input;
- `action_prefix`: the authoritative resume checkpoint. Replay these actions
  from `Game::new(seed, Character::Defect, 0, Unlocks::fixture())` to recreate
  the complete engine state, including private bookkeeping;
- `winning_suffix`: the exact winning continuation, usable as an audit trail or
  expert demonstration;
- `entry_step`, `final_step`, and `boss`: trajectory metadata.

The corpus was selected in deterministic source order from 900 unique random
seeds generated with seed source `20260826`:

```sh
target/release/sts-htn --a0 --count 900 --concurrent 12 \
  --seed-source 20260826 --target-states 500 \
  --winning-boss-entry-jsonl \
  fixtures/htn/defect-a0-act3-boss-winning-entry-500.jsonl
xz -T0 -9 fixtures/htn/defect-a0-act3-boss-winning-entry-500.jsonl
```

Verify every resume snapshot and winning suffix in a fresh process with:

```sh
checkpoint_path=$(mktemp)
xz -dc fixtures/htn/defect-a0-act3-boss-winning-entry-500.jsonl.xz \
  > "$checkpoint_path"
target/release/sts-htn --a0 --verify-winning-boss-entry-jsonl \
  "$checkpoint_path"
```

Decompressed corpus SHA-256:
`deccb556b77edc52a66ccef5c768e50e0b7300bd592825bad638c04ed83f9dba`.

Compressed artifact SHA-256:
`b128f23848b4fe4f6c43920a6cdeb20d0479f890a07ad4c35918550a7037a2aa`.
