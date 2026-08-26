#!/usr/bin/env python3
"""Train a compact HRM policy on replay-expanded Act 3 boss puzzles.

The user-facing entry point is the sts-hrm-train Rust binary. This module keeps
model and preprocessing details behind stable defaults so ordinary runs do not
need a wall of hyperparameters.
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import lzma
import math
from pathlib import Path
import random
import time
from typing import Any, Iterable, Iterator

try:
    import torch
    from torch import nn
    from torch.nn import functional as F
    from torch.utils.data import DataLoader, TensorDataset
except ImportError as exc:
    raise SystemExit(
        "PyTorch is required. Run this through sts-hrm-train, which provisions "
        "the default CUDA environment with uv."
    ) from exc


PREPROCESS_VERSION = 3
MODEL_DEFAULTS = {
    "hidden_size": 192,
    "heads": 4,
    "expansion": 4,
    "h_layers": 1,
    "l_layers": 1,
    "h_cycles": 2,
    "l_cycles": 2,
    "deep_supervision_segments": 2,
    "max_tokens": 384,
    "max_vocab": 32768,
    "batch_size": 24,
    "learning_rate": 3e-4,
    "weight_decay": 0.1,
    "warmup_updates": 100,
    "progress_loss_weight": 0.20,
    "grad_clip": 1.0,
}
SPECIAL_TOKENS = ("[PAD]", "[UNK]", "[CLS]")
BOSS_NAMES = ("AwakenedOne", "DonuAndDeca", "TimeEater")


def log(message: str) -> None:
    print(message, flush=True)


def open_jsonl(path: Path) -> Iterable[str]:
    if path.suffix == ".xz":
        return lzma.open(path, "rt", encoding="utf-8")
    return path.open("r", encoding="utf-8")


def iter_puzzles(path: Path) -> Iterator[dict[str, Any]]:
    with open_jsonl(path) as source:
        for line_number, line in enumerate(source, 1):
            if not line.strip():
                continue
            puzzle = json.loads(line)
            if puzzle.get("schema_version") != 1:
                raise ValueError(
                    f"{path} line {line_number}: unsupported expanded schema "
                    f"{puzzle.get('schema_version')!r}"
                )
            yield puzzle


def action_key(action: dict[str, Any]) -> str:
    return json.dumps(action, sort_keys=True, separators=(",", ":"))


def bucket_number(value: Any) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return "none"
    if isinstance(value, float):
        value = int(round(value * 100))
        return f"pct:{max(-1000, min(1000, value))}"
    if not isinstance(value, int):
        return str(value)
    if -32 <= value <= 128:
        return str(value)
    magnitude = abs(value)
    if magnitude <= 1024:
        rounded = int(round(value / 8) * 8)
        return f"q8:{rounded}"
    sign = "n" if value < 0 else "p"
    return f"{sign}log2:{magnitude.bit_length() - 1}"


def scalar_token(prefix: str, value: Any) -> str:
    if isinstance(value, str):
        return f"{prefix}={value}"
    return f"{prefix}={bucket_number(value)}"


def card_token(zone: str, card: dict[str, Any]) -> str:
    flags = "".join(
        key[0]
        for key in (
            "free_to_play_once",
            "exhaust",
            "ethereal",
            "retain",
            "innate",
            "in_bottle",
        )
        if card.get(key)
    ) or "-"
    return (
        f"CARD:{zone}:{card.get('id','?')}:u{int(bool(card.get('upgraded')))}:"
        f"tu{bucket_number(card.get('times_upgraded', 0))}:"
        f"c{bucket_number(card.get('cost', -1))}:"
        f"ct{bucket_number(card.get('cost_for_turn', -1))}:"
        f"m{bucket_number(card.get('misc', 0))}:f{flags}"
    )


def add_power_tokens(tokens: list[str], owner: str, powers: list[dict[str, Any]]) -> None:
    for power in powers:
        tokens.append(f"POWER:{owner}:{power.get('id','?')}")
        for key in ("amount", "misc"):
            tokens.append(
                scalar_token(
                    f"POWER:{owner}:{power.get('id','?')}:{key}",
                    power.get(key, 0),
                )
            )
        if power.get("just_applied"):
            tokens.append(f"POWER:{owner}:{power.get('id','?')}:just")
        if power.get("skip_first"):
            tokens.append(f"POWER:{owner}:{power.get('id','?')}:skip")


def rng_byte_tokens(rng: dict[str, Any]) -> Iterator[str]:
    # These six streams can alter combat decisions. Coarse byte tokenization
    # preserves each byte's position and high nibble while avoiding a sparse
    # position/value vocabulary that memorizes individual training seeds and
    # maps unseen held-out byte values to [UNK].
    for stream in ("ai", "card", "card_random", "misc", "monster", "shuffle"):
        state = rng.get(stream)
        if not isinstance(state, dict):
            continue
        yield scalar_token(f"RNG:{stream}:counter", state.get("counter", 0))
        for field in ("state0", "state1"):
            raw = int(state.get(field, 0)) & ((1 << 64) - 1)
            for byte_index in range(8):
                byte = (raw >> (byte_index * 8)) & 0xFF
                yield f"RNG:{stream}:{field}:b{byte_index}:hi={byte >> 4}"


def state_tokens(puzzle: dict[str, Any], decision: dict[str, Any]) -> list[str]:
    state = decision["state"]
    game = state.get("game") or {}
    player = state.get("player") or {}
    combat = state.get("combat") or {}
    tokens = ["[CLS]", f"BOSS={puzzle['boss']}"]

    for key in (
        "screen",
        "current_room",
        "ascension",
        "character",
        "card_blizz",
        "potion_blizzard",
    ):
        if key in game:
            tokens.append(scalar_token(f"GAME:{key}", game[key]))
    for key, value in sorted((game.get("keys") or {}).items()):
        tokens.append(scalar_token(f"KEY:{key}", value))
    for card in game.get("pending_cards") or []:
        tokens.append(card_token("pending", card))
    if game.get("grid") is not None:
        grid = game["grid"]
        if isinstance(grid, dict):
            tokens.append(scalar_token("GRID:kind", grid.get("kind", "?")))
            for key in ("confirm", "can_cancel"):
                if key in grid:
                    tokens.append(scalar_token(f"GRID:{key}", grid[key]))
        else:
            tokens.append(scalar_token("GRID", grid))
    if game.get("hand_select"):
        tokens.append(f"HAND_SELECT={len(game['hand_select'])}")

    for key in (
        "hp",
        "max_hp",
        "block",
        "energy",
        "energy_master",
        "gold",
        "max_orbs",
        "master_max_orbs",
        "potion_slots",
        "pending_evoke_dark",
        "pending_evoke_frost",
        "pending_evoke_lightning",
        "pending_static",
    ):
        if key in player:
            tokens.append(scalar_token(f"PLAYER:{key}", player[key]))

    for key in (
        "encounter",
        "turn",
        "cards_played_this_turn",
        "echo_cards_duplicated_this_turn",
        "skills_this_turn",
        "attacks_this_turn",
        "orange_pellets_mask",
        "draw_after_exhaust",
        "pending_dark_embrace",
        "pending_ink_bottle",
        "pending_letter_opener",
        "pending_hex_after_seek",
        "energy_on_use",
        "force_end_turn",
        "need_exhaust_select",
        "need_put_on_deck",
        "need_discard_to_hand",
        "need_draw_to_hand",
        "need_discovery",
        "need_forethought",
        "need_skill_from_deck",
        "pending_rebound",
    ):
        if key in combat:
            tokens.append(scalar_token(f"COMBAT:{key}", combat[key]))

    for monster_index, monster in enumerate(combat.get("monsters") or []):
        owner = f"M{monster_index}:{monster.get('id','?')}"
        tokens.append(
            f"MONSTER:{owner}:intent={monster.get('intent','?')}:"
            f"dead={int(bool(monster.get('dead')))}:"
            f"half={int(bool(monster.get('half_dead')))}"
        )
        for key in (
            "hp",
            "max_hp",
            "block",
            "intent_damage",
            "intent_base_damage",
            "intent_hits",
            "next_move",
            "extra",
            "pending_reactive",
            "pending_curl",
            "pending_hand_drill",
        ):
            tokens.append(scalar_token(f"MONSTER:{owner}:{key}", monster.get(key, 0)))
        history = monster.get("move_history") or []
        for offset, move in enumerate(history[-4:]):
            tokens.append(scalar_token(f"MONSTER:{owner}:history:{offset}", move))
        add_power_tokens(tokens, owner, monster.get("powers") or [])

    tokens.append("ZONE:hand")
    tokens.extend(card_token("hand", card) for card in player.get("hand") or [])

    for orb_index, orb in enumerate(player.get("orbs") or []):
        kind = orb.get("kind", "?")
        tokens.append(f"ORB:{orb_index}:{kind}")
        for key, value in sorted(orb.items()):
            if key != "kind":
                tokens.append(scalar_token(f"ORB:{orb_index}:{kind}:{key}", value))

    add_power_tokens(tokens, "PLAYER", player.get("powers") or [])
    for relic in player.get("relics") or []:
        tokens.append(f"RELIC:{relic.get('id','?')}")
        tokens.append(
            scalar_token(
                f"RELIC:{relic.get('id','?')}:counter",
                relic.get("counter", -1),
            )
        )
        if relic.get("used_up"):
            tokens.append(f"RELIC:{relic.get('id','?')}:used")
    for potion in player.get("potions") or []:
        tokens.append(f"POTION:{potion.get('slot','?')}:{potion.get('id','?')}")

    for legal in decision.get("legal_actions") or []:
        tokens.append("LEGAL:" + action_key(legal))

    for zone in ("draw", "discard", "exhaust"):
        tokens.append(f"ZONE:{zone}")
        tokens.extend(card_token(zone, card) for card in player.get(zone) or [])

    deck_counts: collections.Counter[tuple[str, bool]] = collections.Counter(
        (card.get("id", "?"), bool(card.get("upgraded")))
        for card in player.get("deck") or []
    )
    for (card_id, upgraded), count in sorted(deck_counts.items()):
        tokens.append(f"DECK:{card_id}:u{int(upgraded)}:n{bucket_number(count)}")

    tokens.extend(rng_byte_tokens(state.get("rng") or {}))
    return tokens


def proportional_quota(counts: dict[str, int], total: int) -> dict[str, int]:
    raw = {boss: total * count / sum(counts.values()) for boss, count in counts.items()}
    quota = {boss: int(math.floor(value)) for boss, value in raw.items()}
    remaining = total - sum(quota.values())
    order = sorted(
        counts,
        key=lambda boss: (raw[boss] - quota[boss], boss),
        reverse=True,
    )
    for boss in order[:remaining]:
        quota[boss] += 1
    return quota


def stable_rank(seed: int, split_seed: int) -> str:
    return hashlib.sha256(f"{split_seed}:{seed}".encode()).hexdigest()


def build_split(metadata: list[dict[str, Any]], split_seed: int) -> dict[int, str]:
    by_boss: dict[str, list[dict[str, Any]]] = collections.defaultdict(list)
    for row in metadata:
        by_boss[row["boss"]].append(row)
    counts = {boss: len(rows) for boss, rows in by_boss.items()}
    test_quota = proportional_quota(counts, 50)
    val_quota = proportional_quota(counts, 50)
    split: dict[int, str] = {}
    for boss, rows in by_boss.items():
        rows.sort(key=lambda row: stable_rank(row["seed"], split_seed))
        test_end = test_quota[boss]
        val_end = test_end + val_quota[boss]
        for row in rows[:test_end]:
            split[row["puzzle_index"]] = "test"
        for row in rows[test_end:val_end]:
            split[row["puzzle_index"]] = "val"
        for row in rows[val_end:]:
            split[row["puzzle_index"]] = "train"
    return split


def source_signature(path: Path) -> dict[str, Any]:
    stat = path.stat()
    return {
        "path": str(path.resolve()),
        "size": stat.st_size,
        "mtime_ns": stat.st_mtime_ns,
        "preprocess_version": PREPROCESS_VERSION,
        "model_defaults": MODEL_DEFAULTS,
    }


def prepare_dataset(dataset_path: Path, cache_path: Path, split_seed: int) -> dict[str, Any]:
    signature = source_signature(dataset_path)
    if cache_path.exists():
        cached = torch.load(cache_path, map_location="cpu", weights_only=False)
        if (
            cached.get("source_signature") == signature
            and cached.get("split_seed") == split_seed
        ):
            log(f"loaded prepared tensors from {cache_path}")
            return cached

    started = time.monotonic()
    metadata = []
    for puzzle in iter_puzzles(dataset_path):
        metadata.append(
            {
                "puzzle_index": int(puzzle["puzzle_index"]),
                "seed": int(puzzle["seed"]),
                "boss": puzzle["boss"],
                "decisions": len(puzzle["decisions"]),
            }
        )
    if len(metadata) != 500:
        raise ValueError(f"expected 500 puzzles, found {len(metadata)}")
    split_map = build_split(metadata, split_seed)
    split_puzzle_counts = collections.Counter(split_map.values())
    if split_puzzle_counts != {"train": 400, "val": 50, "test": 50}:
        raise AssertionError(f"unexpected puzzle split {split_puzzle_counts}")

    token_counts: collections.Counter[str] = collections.Counter()
    action_keys: set[str] = set()
    for puzzle in iter_puzzles(dataset_path):
        split = split_map[int(puzzle["puzzle_index"])]
        for decision in puzzle["decisions"]:
            action_keys.add(action_key(decision["expert_action"]))
            action_keys.update(action_key(action) for action in decision["legal_actions"])
            if split == "train":
                token_counts.update(state_tokens(puzzle, decision))

    action_list = sorted(action_keys)
    action_to_id = {key: index for index, key in enumerate(action_list)}
    vocabulary = list(SPECIAL_TOKENS)
    vocabulary.extend(
        token
        for token, _ in sorted(
            token_counts.items(), key=lambda item: (-item[1], item[0])
        )
        if token not in SPECIAL_TOKENS
    )
    vocabulary = vocabulary[: MODEL_DEFAULTS["max_vocab"]]
    token_to_id = {token: index for index, token in enumerate(vocabulary)}

    example_counts = collections.Counter()
    for row in metadata:
        example_counts[split_map[row["puzzle_index"]]] += row["decisions"]

    tensors: dict[str, dict[str, torch.Tensor]] = {}
    for split, count in example_counts.items():
        tensors[split] = {
            "input_ids": torch.zeros(
                (count, MODEL_DEFAULTS["max_tokens"]), dtype=torch.int32
            ),
            "legal_mask": torch.zeros((count, len(action_list)), dtype=torch.bool),
            "target": torch.empty(count, dtype=torch.int64),
            "score": torch.empty(count, dtype=torch.float32),
            "puzzle_index": torch.empty(count, dtype=torch.int32),
            "boss": torch.empty(count, dtype=torch.int8),
        }

    offsets = collections.Counter()
    lengths = []
    truncated = 0
    unknown_tokens = collections.Counter()
    boss_to_id = {boss: index for index, boss in enumerate(BOSS_NAMES)}
    for puzzle in iter_puzzles(dataset_path):
        split = split_map[int(puzzle["puzzle_index"])]
        score = float(puzzle["score"]) / 100.0
        for decision in puzzle["decisions"]:
            index = offsets[split]
            offsets[split] += 1
            tokens = state_tokens(puzzle, decision)
            lengths.append(len(tokens))
            if len(tokens) > MODEL_DEFAULTS["max_tokens"]:
                truncated += 1
            token_ids = []
            for token in tokens[: MODEL_DEFAULTS["max_tokens"]]:
                token_id = token_to_id.get(token, 1)
                if token_id == 1:
                    unknown_tokens[split] += 1
                token_ids.append(token_id)
            tensors[split]["input_ids"][index, : len(token_ids)] = torch.tensor(
                token_ids, dtype=torch.int32
            )
            for legal in decision["legal_actions"]:
                tensors[split]["legal_mask"][index, action_to_id[action_key(legal)]] = True
            target = action_to_id[action_key(decision["expert_action"])]
            if not tensors[split]["legal_mask"][index, target]:
                raise AssertionError(
                    f"puzzle {puzzle['puzzle_index']} decision "
                    f"{decision['decision_index']}: target is not legal"
                )
            tensors[split]["target"][index] = target
            tensors[split]["score"][index] = score
            tensors[split]["puzzle_index"][index] = int(puzzle["puzzle_index"])
            tensors[split]["boss"][index] = boss_to_id[puzzle["boss"]]

    prepared = {
        "source_signature": signature,
        "split_seed": split_seed,
        "split_map": split_map,
        "metadata": metadata,
        "vocabulary": vocabulary,
        "action_list": action_list,
        "tensors": tensors,
        "stats": {
            "puzzles": dict(split_puzzle_counts),
            "examples": dict(example_counts),
            "token_length_min": min(lengths),
            "token_length_mean": sum(lengths) / len(lengths),
            "token_length_max": max(lengths),
            "truncated_examples": truncated,
            "unknown_tokens": dict(unknown_tokens),
        },
    }
    cache_path.parent.mkdir(parents=True, exist_ok=True)
    torch.save(prepared, cache_path)
    log(
        f"prepared {sum(example_counts.values())} decisions from 500 puzzles in "
        f"{time.monotonic() - started:.1f}s; cache={cache_path}"
    )
    return prepared


class PostNormBlock(nn.Module):
    def __init__(self, hidden_size: int, heads: int, expansion: int):
        super().__init__()
        self.attention = nn.MultiheadAttention(
            hidden_size,
            heads,
            dropout=0.0,
            bias=False,
            batch_first=True,
        )
        inner = hidden_size * expansion
        self.gate = nn.Linear(hidden_size, inner, bias=False)
        self.value = nn.Linear(hidden_size, inner, bias=False)
        self.down = nn.Linear(inner, hidden_size, bias=False)
        self.attention_norm = nn.RMSNorm(hidden_size)
        self.mlp_norm = nn.RMSNorm(hidden_size)

    def forward(
        self,
        hidden: torch.Tensor,
        padding_mask: torch.Tensor,
    ) -> torch.Tensor:
        attended = self.attention(
            hidden,
            hidden,
            hidden,
            key_padding_mask=padding_mask,
            need_weights=False,
        )[0]
        hidden = self.attention_norm(hidden + attended)
        mlp = self.down(F.silu(self.gate(hidden)) * self.value(hidden))
        return self.mlp_norm(hidden + mlp)


class RecurrentModule(nn.Module):
    def __init__(
        self,
        hidden_size: int,
        heads: int,
        expansion: int,
        layers: int,
    ):
        super().__init__()
        self.layers = nn.ModuleList(
            PostNormBlock(hidden_size, heads, expansion) for _ in range(layers)
        )

    def forward(
        self,
        hidden: torch.Tensor,
        padding_mask: torch.Tensor,
    ) -> torch.Tensor:
        for layer in self.layers:
            hidden = layer(hidden, padding_mask)
        return hidden


class CombatHrm(nn.Module):
    def __init__(self, vocabulary_size: int, action_count: int):
        super().__init__()
        hidden = MODEL_DEFAULTS["hidden_size"]
        self.token_embedding = nn.Embedding(
            vocabulary_size,
            hidden,
            padding_idx=0,
        )
        self.position_embedding = nn.Embedding(
            MODEL_DEFAULTS["max_tokens"],
            hidden,
        )
        self.low = RecurrentModule(
            hidden,
            MODEL_DEFAULTS["heads"],
            MODEL_DEFAULTS["expansion"],
            MODEL_DEFAULTS["l_layers"],
        )
        self.high = RecurrentModule(
            hidden,
            MODEL_DEFAULTS["heads"],
            MODEL_DEFAULTS["expansion"],
            MODEL_DEFAULTS["h_layers"],
        )
        self.action_head = nn.Linear(hidden, action_count, bias=False)
        self.progress_head = nn.Sequential(
            nn.Linear(hidden, hidden, bias=False),
            nn.SiLU(),
            nn.Linear(hidden, 1, bias=True),
        )
        self.register_buffer(
            "high_init",
            torch.empty(1, MODEL_DEFAULTS["max_tokens"], hidden),
        )
        self.register_buffer(
            "low_init",
            torch.empty(1, MODEL_DEFAULTS["max_tokens"], hidden),
        )
        self.reset_parameters()

    def reset_parameters(self) -> None:
        nn.init.normal_(self.token_embedding.weight, std=0.02)
        with torch.no_grad():
            self.token_embedding.weight[0].zero_()
        nn.init.trunc_normal_(self.high_init, std=1.0, a=-2.0, b=2.0)
        nn.init.trunc_normal_(self.low_init, std=1.0, a=-2.0, b=2.0)

    def segment(
        self,
        input_ids: torch.Tensor,
        carry: tuple[torch.Tensor, torch.Tensor] | None,
    ) -> tuple[tuple[torch.Tensor, torch.Tensor], torch.Tensor, torch.Tensor]:
        padding_mask = input_ids.eq(0)
        positions = torch.arange(input_ids.shape[1], device=input_ids.device)
        embedded = (
            self.token_embedding(input_ids)
            + self.position_embedding(positions)[None, :, :]
        )
        if carry is None:
            high = self.high_init.expand(input_ids.shape[0], -1, -1)
            low = self.low_init.expand(input_ids.shape[0], -1, -1)
        else:
            high, low = carry

        # Faithful one-step-gradient schedule: all recurrent work except the
        # final low and high updates is detached from autograd.
        with torch.no_grad():
            for high_cycle in range(MODEL_DEFAULTS["h_cycles"]):
                for low_cycle in range(MODEL_DEFAULTS["l_cycles"]):
                    final_low = (
                        high_cycle == MODEL_DEFAULTS["h_cycles"] - 1
                        and low_cycle == MODEL_DEFAULTS["l_cycles"] - 1
                    )
                    if final_low:
                        break
                    low = self.low(low + high + embedded, padding_mask)
                if high_cycle == MODEL_DEFAULTS["h_cycles"] - 1:
                    break
                high = self.high(high + low, padding_mask)

        low = self.low(low + high + embedded, padding_mask)
        high = self.high(high + low, padding_mask)
        pooled = high[:, 0]
        carry_out = (high.detach(), low.detach())
        return (
            carry_out,
            self.action_head(pooled),
            self.progress_head(pooled).squeeze(-1),
        )


def make_loader(
    split_tensors: dict[str, torch.Tensor],
    batch_size: int,
    shuffle: bool,
    seed: int,
) -> DataLoader:
    dataset = TensorDataset(
        split_tensors["input_ids"],
        split_tensors["legal_mask"],
        split_tensors["target"],
        split_tensors["score"],
        split_tensors["boss"],
    )
    generator = torch.Generator().manual_seed(seed)
    return DataLoader(
        dataset,
        batch_size=batch_size,
        shuffle=shuffle,
        num_workers=0,
        pin_memory=torch.cuda.is_available(),
        generator=generator,
        drop_last=shuffle,
    )


def masked_logits(
    logits: torch.Tensor,
    legal_mask: torch.Tensor,
) -> torch.Tensor:
    return logits.masked_fill(
        ~legal_mask,
        torch.finfo(logits.dtype).min,
    )


def evaluate(
    model: CombatHrm,
    split_tensors: dict[str, torch.Tensor],
    device: torch.device,
    max_examples: int | None = None,
) -> dict[str, Any]:
    if max_examples is not None:
        split_tensors = {
            key: value[:max_examples]
            for key, value in split_tensors.items()
        }
    loader = make_loader(
        split_tensors,
        MODEL_DEFAULTS["batch_size"] * 2,
        False,
        0,
    )
    model.eval()
    total = 0
    correct = 0
    cross_entropy = 0.0
    absolute_score_error = 0.0
    by_boss = {
        index: {"correct": 0, "total": 0}
        for index in range(len(BOSS_NAMES))
    }
    with torch.inference_mode():
        for input_ids, legal, target, score, boss in loader:
            input_ids = input_ids.to(device, non_blocking=True).long()
            legal = legal.to(device, non_blocking=True)
            target = target.to(device, non_blocking=True)
            score = score.to(device, non_blocking=True)
            boss = boss.to(device, non_blocking=True)
            carry = None
            with torch.autocast(
                device_type=device.type,
                dtype=torch.bfloat16,
                enabled=device.type == "cuda",
            ):
                for _ in range(MODEL_DEFAULTS["deep_supervision_segments"]):
                    carry, action_logits, progress = model.segment(
                        input_ids,
                        carry,
                    )
                action_logits = masked_logits(action_logits, legal)
                loss = F.cross_entropy(
                    action_logits.float(),
                    target,
                    reduction="sum",
                )
            prediction = action_logits.argmax(dim=-1)
            matches = prediction.eq(target)
            batch_size = target.numel()
            total += batch_size
            correct += int(matches.sum())
            cross_entropy += float(loss)
            absolute_score_error += float(
                (progress.float() - score).abs().sum()
            )
            for boss_index in range(len(BOSS_NAMES)):
                selected = boss.eq(boss_index)
                by_boss[boss_index]["total"] += int(selected.sum())
                by_boss[boss_index]["correct"] += int(
                    (matches & selected).sum()
                )

    return {
        "examples": total,
        "decision_accuracy": correct / max(1, total),
        "action_cross_entropy": cross_entropy / max(1, total),
        "progress_mae_hp": (
            100.0 * absolute_score_error / max(1, total)
        ),
        "accuracy_by_boss": {
            BOSS_NAMES[index]: (
                values["correct"] / max(1, values["total"])
            )
            for index, values in by_boss.items()
        },
    }


def frequency_baseline(
    train: dict[str, torch.Tensor],
    evaluation: dict[str, torch.Tensor],
) -> dict[str, float]:
    action_count = train["legal_mask"].shape[1]
    frequencies = torch.bincount(
        train["target"],
        minlength=action_count,
    ).float()
    scores = frequencies[None, :].expand(
        evaluation["legal_mask"].shape[0],
        -1,
    ).clone()
    scores.masked_fill_(~evaluation["legal_mask"], -1)
    prediction = scores.argmax(dim=-1)
    accuracy = prediction.eq(evaluation["target"]).float().mean().item()
    random_legal = (
        1.0 / evaluation["legal_mask"].sum(dim=1).float()
    ).mean().item()
    return {
        "legal_frequency_accuracy": accuracy,
        "uniform_legal_expected_accuracy": random_legal,
    }


def choose_device(raw: str) -> torch.device:
    if raw == "auto":
        raw = "cuda" if torch.cuda.is_available() else "cpu"
    device = torch.device(raw)
    if device.type == "cuda" and not torch.cuda.is_available():
        raise RuntimeError(
            "CUDA was requested but torch.cuda.is_available() is false"
        )
    return device


def train(args: argparse.Namespace) -> dict[str, Any]:
    random.seed(args.seed)
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)
        torch.backends.cuda.matmul.allow_tf32 = True
        torch.set_float32_matmul_precision("high")

    dataset_path = Path(args.dataset)
    output_dir = Path(args.output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    cache_path = output_dir / "prepared-combat-hrm-v3.pt"
    prepared = prepare_dataset(dataset_path, cache_path, args.seed)
    tensors = prepared["tensors"]
    device = choose_device(args.device)
    device_name = (
        torch.cuda.get_device_name(device)
        if device.type == "cuda"
        else "CPU"
    )
    log(
        f"device={device} ({device_name}); "
        f"vocab={len(prepared['vocabulary'])}; "
        f"actions={len(prepared['action_list'])}; "
        f"split={prepared['stats']['puzzles']}"
    )
    log(
        "dataset stats: "
        + json.dumps(prepared["stats"], sort_keys=True)
    )

    model = CombatHrm(
        len(prepared["vocabulary"]),
        len(prepared["action_list"]),
    ).to(device)
    parameter_count = sum(
        parameter.numel()
        for parameter in model.parameters()
    )
    optimizer = torch.optim.AdamW(
        model.parameters(),
        lr=MODEL_DEFAULTS["learning_rate"],
        betas=(0.9, 0.95),
        weight_decay=MODEL_DEFAULTS["weight_decay"],
    )
    train_loader = make_loader(
        tensors["train"],
        MODEL_DEFAULTS["batch_size"],
        True,
        args.seed,
    )
    baseline = frequency_baseline(
        tensors["train"],
        tensors["test"],
    )
    initial_validation = evaluate(
        model,
        tensors["val"],
        device,
        max_examples=min(1024, len(tensors["val"]["target"])),
    )
    log(
        f"untrained val accuracy="
        f"{initial_validation['decision_accuracy']:.4f}; "
        f"frequency baseline test accuracy="
        f"{baseline['legal_frequency_accuracy']:.4f}"
    )

    model.train()
    started = time.monotonic()
    deadline = started + args.train_seconds
    next_report = started + min(30.0, args.train_seconds / 4)
    updates = 0
    examples_seen = 0
    loss_ema = None
    loader_iterator = iter(train_loader)

    while time.monotonic() < deadline:
        try:
            batch = next(loader_iterator)
        except StopIteration:
            loader_iterator = iter(train_loader)
            batch = next(loader_iterator)
        input_ids, legal, target, score, _boss = batch
        input_ids = input_ids.to(device, non_blocking=True).long()
        legal = legal.to(device, non_blocking=True)
        target = target.to(device, non_blocking=True)
        score = score.to(device, non_blocking=True)
        carry = None

        for _segment in range(
            MODEL_DEFAULTS["deep_supervision_segments"]
        ):
            if time.monotonic() >= deadline and updates > 0:
                break
            optimizer.zero_grad(set_to_none=True)
            warmup = min(
                1.0,
                (updates + 1) / MODEL_DEFAULTS["warmup_updates"],
            )
            for group in optimizer.param_groups:
                group["lr"] = (
                    MODEL_DEFAULTS["learning_rate"] * warmup
                )
            with torch.autocast(
                device_type=device.type,
                dtype=torch.bfloat16,
                enabled=device.type == "cuda",
            ):
                carry, logits, progress = model.segment(
                    input_ids,
                    carry,
                )
                logits = masked_logits(logits, legal)
                policy_loss = F.cross_entropy(
                    logits.float(),
                    target,
                )
                progress_loss = F.smooth_l1_loss(
                    progress.float(),
                    score,
                )
                loss = (
                    policy_loss
                    + MODEL_DEFAULTS["progress_loss_weight"]
                    * progress_loss
                )
            loss.backward()
            nn.utils.clip_grad_norm_(
                model.parameters(),
                MODEL_DEFAULTS["grad_clip"],
            )
            optimizer.step()
            updates += 1
            examples_seen += target.numel()
            value = float(loss.detach())
            loss_ema = (
                value
                if loss_ema is None
                else 0.98 * loss_ema + 0.02 * value
            )

        now = time.monotonic()
        if now >= next_report:
            elapsed = now - started
            log(
                f"training {elapsed:.1f}/{args.train_seconds:.1f}s; "
                f"updates={updates}; examples={examples_seen}; "
                f"loss_ema={loss_ema:.4f}"
            )
            next_report += 30.0

    if device.type == "cuda":
        torch.cuda.synchronize()
    training_elapsed = time.monotonic() - started
    log(
        f"optimization complete: {training_elapsed:.2f}s, "
        f"{updates} updates, {examples_seen} segment-examples"
    )

    validation = evaluate(model, tensors["val"], device)
    test = evaluate(model, tensors["test"], device)
    train_sample = evaluate(
        model,
        tensors["train"],
        device,
        max_examples=2048,
    )

    duration_label = (
        f"{int(args.train_seconds // 60)}m"
        if args.train_seconds % 60 == 0
        else f"{int(args.train_seconds)}s"
    )
    checkpoint_path = (
        output_dir / f"combat-hrm-{duration_label}.pt"
    )
    metrics_path = (
        output_dir / f"combat-hrm-{duration_label}-metrics.json"
    )
    metrics = {
        "source_fixture": str(Path(args.source_fixture).resolve()),
        "expanded_dataset": str(dataset_path.resolve()),
        "preprocess_version": PREPROCESS_VERSION,
        "seed": args.seed,
        "device": str(device),
        "device_name": device_name,
        "torch_version": torch.__version__,
        "model_defaults": MODEL_DEFAULTS,
        "parameter_count": parameter_count,
        "requested_training_seconds": args.train_seconds,
        "actual_training_seconds": training_elapsed,
        "optimizer_updates": updates,
        "segment_examples_seen": examples_seen,
        "final_loss_ema": loss_ema,
        "dataset": prepared["stats"],
        "baseline": baseline,
        "untrained_validation_sample": initial_validation,
        "train_sample": train_sample,
        "validation": validation,
        "test": test,
    }
    torch.save(
        {
            "model_state": model.state_dict(),
            "vocabulary": prepared["vocabulary"],
            "action_list": prepared["action_list"],
            "model_defaults": MODEL_DEFAULTS,
            "split_map": prepared["split_map"],
            "metrics": metrics,
        },
        checkpoint_path,
    )
    metrics_path.write_text(
        json.dumps(metrics, indent=2, sort_keys=True) + "\n"
    )
    log(f"checkpoint={checkpoint_path}")
    log(f"metrics={metrics_path}")
    # Emit the trainer-neutral artifact consumed by Rust inference while this
    # process still has the export dependencies available.
    from export_hrm_onnx import export_checkpoint

    export_checkpoint(
        checkpoint_path,
        checkpoint_path.with_suffix(".onnx"),
        checkpoint_path.with_suffix(".runtime.json"),
        "float16",
    )
    log("RESULT " + json.dumps(metrics, sort_keys=True))
    return metrics


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Train the default combat HRM. Prefer the zero-argument "
            "sts-hrm-train frontend."
        )
    )
    parser.add_argument(
        "--dataset",
        default=(
            "artifacts/hrm/"
            "defect-a0-act3-boss-hrm-puzzles.jsonl"
        ),
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--source-fixture",
        default=(
            "fixtures/htn/"
            "defect-a0-act3-boss-winning-entry-500.jsonl.xz"
        ),
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--output-dir",
        default="artifacts/hrm",
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--train-seconds",
        type=float,
        default=600.0,
    )
    parser.add_argument(
        "--device",
        default="auto",
        choices=("auto", "cuda", "cpu"),
    )
    parser.add_argument(
        "--seed",
        type=int,
        default=20260826,
        help=argparse.SUPPRESS,
    )
    args = parser.parse_args()
    if args.train_seconds <= 0:
        parser.error("--train-seconds must be positive")
    return args


if __name__ == "__main__":
    try:
        train(parse_args())
    except KeyboardInterrupt:
        raise SystemExit(130)
