#!/usr/bin/env python3
"""Drive Rust HTN and ExactTextSim2 RPC sessions in strict live lockstep."""

from __future__ import annotations

import argparse
import gzip
import json
import os
from pathlib import Path
import re
import sys
import urllib.error
import urllib.request


SEED_CHARS = "0123456789ABCDEFGHIJKLMNPQRSTUVWXYZ"


def sts_seed_string(seed: int) -> str:
    if seed < 0:
        raise ValueError(f"negative STS seed is unsupported: {seed}")
    if seed == 0:
        return "0"
    chars: list[str] = []
    while seed:
        seed, digit = divmod(seed, len(SEED_CHARS))
        chars.append(SEED_CHARS[digit])
    return "".join(reversed(chars))


def request(base: str, timeout: float, method: str, path: str, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        base + path,
        data=data,
        method=method,
        headers={"content-type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        payload = error.read().decode(errors="replace")
        raise RuntimeError(f"{method} {path} returned HTTP {error.code}: {payload}") from error


def normalized_label(label) -> str:
    label = re.sub(r"#[a-zA-Z]", "", label or "")
    return " ".join(label.lower().split())


def compact_label(label) -> str:
    return "".join(character for character in normalized_label(label) if character.isalnum() or character == "+")


def bracket_verb(label) -> str | None:
    match = re.match(r"^\[\s*([^]]+)]", normalized_label(label))
    return match.group(1).strip() if match else None


def neow_label_matches(kind: str, java_label) -> bool:
    label = normalized_label(java_label)
    required = {
        "ThreeCards": ("choose", "card"),
        "RandomRareCard": ("random", "rare", "card"),
        "RemoveCard": ("remove", "card"),
        "UpgradeCard": ("upgrade", "card"),
        "TransformCard": ("transform", "card"),
        "RandomColorless": ("colorless", "card"),
        "ThreePotions": ("potion",),
        "RandomCommonRelic": ("common", "relic"),
        "TenHp": ("max", "hp"),
        "ThreeEnemyKill": ("3", "enemy", "1", "hp"),
        "HundredGold": ("100", "gold"),
        "RandomColorless2": ("rare", "colorless", "card"),
        "RemoveTwo": ("remove", "2", "card"),
        "RareRelic": ("rare", "relic"),
        "ThreeRareCards": ("rare", "card"),
        "TwoFiftyGold": ("250", "gold"),
        "TransformTwo": ("transform", "2", "card"),
        "TwentyHp": ("max", "hp"),
        "Boss Relic": ("starting", "relic", "boss", "relic"),
    }.get(kind)
    return required is not None and all(term in label for term in required)


def action_match_reason(rust: dict, java: dict) -> str | None:
    rust_op = rust.get("op")
    java_op = java.get("op")
    if rust_op != java_op:
        return None

    if rust_op in ("end_turn", "proceed", "skip"):
        return "operation"
    if rust_op == "play":
        keys = ("hand_index", "target_index")
        return "play-indices" if all(rust.get(key) == java.get(key) for key in keys) else None
    if rust_op == "potion":
        keys = ("action", "slot", "target_index")
        return "potion-indices" if all(rust.get(key) == java.get(key) for key in keys) else None
    if rust_op != "choose":
        return "exact" if rust == java else None

    rust_x, rust_y = rust.get("x"), rust.get("y")
    if rust_x is not None and rust_y is not None:
        if java.get("x") == rust_x and java.get("y") == rust_y:
            return "map-coordinate"
        if rust_x == -1 and rust_y == 15 and normalized_label(java.get("label")) == "boss":
            return "boss-coordinate"
        return None

    rust_label = rust.get("label")
    java_label = java.get("label")
    same_index = rust.get("index") == java.get("index")
    if normalized_label(rust_label) == normalized_label(java_label) and same_index:
        return "normalized-label"
    if compact_label(rust_label) == compact_label(java_label) and same_index:
        return "compact-label"
    java_card_id = java.get("_card_id") or java.get("card_id")
    if same_index and java_card_id and compact_label(rust_label) == compact_label(java_card_id):
        return "card-id"
    if isinstance(rust_label, str) and same_index and neow_label_matches(rust_label, java_label):
        return "neow-kind-label"

    rust_verb = bracket_verb(rust_label)
    java_verb = bracket_verb(java_label)
    if rust_verb and rust_verb == java_verb and same_index:
        return "bracket-verb"
    verb_aliases = {
        "ingest": "test j.a.x.",
        "study": "become test subject",
        "inject": "ingest mutagens",
    }
    if rust_verb and verb_aliases.get(rust_verb) == java_verb and same_index:
        return "event-verb-alias"

    if rust_label is None and same_index:
        return "unlabeled-index"
    return None


def transform_legal_actions(rust_legal: list[dict], java_legal: list[dict]):
    """Return a one-to-one Rust-index -> Java-action map, or a strict failure."""
    if len(rust_legal) != len(java_legal):
        return None, {
            "reason": "legal_action_count",
            "rust_count": len(rust_legal),
            "java_count": len(java_legal),
        }

    candidates: list[list[tuple[int, str]]] = []
    for rust in rust_legal:
        options = []
        for java_index, java in enumerate(java_legal):
            reason = action_match_reason(rust, java)
            if reason is not None:
                options.append((java_index, reason))
        candidates.append(options)

    order = sorted(range(len(rust_legal)), key=lambda index: (len(candidates[index]), index))
    java_owner: dict[int, int] = {}
    reasons: dict[int, str] = {}

    def assign(rust_index: int, seen: set[int]) -> bool:
        for java_index, reason in candidates[rust_index]:
            if java_index in seen:
                continue
            seen.add(java_index)
            owner = java_owner.get(java_index)
            if owner is None or assign(owner, seen):
                java_owner[java_index] = rust_index
                reasons[rust_index] = reason
                return True
        return False

    for rust_index in order:
        if not assign(rust_index, set()):
            return None, {
                "reason": "legal_action_bijection",
                "unmatched_rust_index": rust_index,
                "candidate_java_indices": [index for index, _ in candidates[rust_index]],
            }

    mapping = [None] * len(rust_legal)
    for java_index, rust_index in java_owner.items():
        mapping[rust_index] = {
            "java": {
                key: value
                for key, value in java_legal[java_index].items()
                if not key.startswith("_")
            },
            "reason": reasons[rust_index],
        }
    return mapping, None


def enriched_java_actions(observation: dict) -> list[dict]:
    """Reconstruct CommandExecutor.legalActions() from its published subset."""
    actions = [dict(action) for action in observation.get("legal_actions", [])]
    state = observation.get("state") or {}
    screen = state.get("screen") or {}
    cards = screen.get("cards", []) if isinstance(screen, dict) else []
    for action in actions:
        index = action.get("index")
        if action.get("op") == "choose" and isinstance(index, int) and 0 <= index < len(cards):
            card = cards[index]
            if isinstance(card, dict) and card.get("id"):
                action["_card_id"] = card["id"]

    # SnapshotWriter deliberately filters potion-discard commands even though
    # CommandExecutor.legalActions() emits them. Restore that exact executor
    # vocabulary from StateSnapshot's ordered potion list. AbstractPotion only
    # forbids discarding while WeMeetAgain is the current room event.
    room = state.get("room") or {}
    event = room.get("event") or {}
    event_class = event.get("class", "") if isinstance(event, dict) else ""
    if not event_class.endswith(".WeMeetAgain"):
        player = state.get("player") or {}
        for slot, potion in enumerate(player.get("potions", [])):
            if not isinstance(potion, dict) or potion.get("id") == "Potion Slot":
                continue
            actions.append(
                {
                    "op": "potion",
                    "action": "discard",
                    "slot": slot,
                    "potion_id": potion.get("id"),
                    "_snapshot_filtered": True,
                }
            )
    return actions


def load_seeds(path: Path) -> list[int]:
    seeds: list[int] = []
    for raw in path.read_text().splitlines():
        raw = raw.partition("#")[0].strip()
        if raw:
            seeds.append(int(raw))
    if len(seeds) != len(set(seeds)):
        raise ValueError(f"duplicate seed in {path}")
    return seeds


def write_oracle(root: Path, seed: int, states: list[dict], commands: list[dict]) -> None:
    seed_dir = root / str(seed)
    seed_dir.mkdir(parents=True, exist_ok=True)
    with gzip.open(seed_dir / "states.jsonl.gz", "wt", encoding="utf-8") as output:
        for state in states:
            output.write(json.dumps(state, separators=(",", ":")) + "\n")
    with gzip.open(seed_dir / "commands.jsonl.gz", "wt", encoding="utf-8") as output:
        for command in commands:
            output.write(json.dumps(command, separators=(",", ":")) + "\n")


def compact_java_observation(observation: dict) -> dict:
    state = observation.get("state") or {}
    player = state.get("player") or {}
    dungeon = state.get("dungeon") or {}
    combat = state.get("combat") or {}
    return {
        "sequence": observation.get("sequence"),
        "command_index": observation.get("command_index"),
        "boundary": observation.get("boundary"),
        "legal_actions": observation.get("legal_actions", []),
        "act": dungeon.get("act"),
        "floor": dungeon.get("floor"),
        "hp": player.get("current_hp"),
        "gold": player.get("gold"),
        "block": player.get("block"),
        "deck": [card.get("id") for card in player.get("master_deck", [])],
        "relics": [relic.get("id") for relic in player.get("relics", [])],
        "hand": [card.get("id") for card in combat.get("hand", [])],
        "monsters": [
            [monster.get("id"), monster.get("current_hp")]
            for monster in combat.get("monsters", [])
        ],
    }


def compact_rust_observation(observation: dict) -> dict:
    return {
        key: observation.get(key)
        for key in ("seed", "steps", "done", "screen", "room", "decision", "legal_actions", "state")
    }


def strict_failure(seed: int, step: int, kind: str, rust: dict, java: dict, detail: dict) -> dict:
    return {
        "status": "mismatch",
        "kind": kind,
        "seed": seed,
        "java_seed": sts_seed_string(seed),
        "step": step,
        "detail": detail,
        "rust": compact_rust_observation(rust),
        "java": compact_java_observation(java),
    }


def run_seed(args, seed: int) -> tuple[bool, dict]:
    rust_session = None
    java_session = None
    java_states: list[dict] = []
    java_commands: list[dict] = []
    try:
        rust_body = request(
            args.rust_rpc,
            args.timeout,
            "POST",
            "/v1/sessions",
            {"seed": seed, "ascension": 20, "character": "DEFECT"},
        )
        rust_session = rust_body["session_id"]
        rust = rust_body["observation"]

        java_body = request(
            args.java_rpc,
            args.timeout,
            "POST",
            "/v1/sessions",
            {"seed": sts_seed_string(seed), "ascension": 20, "character": "DEFECT"},
        )
        java_session = java_body["session_id"]
        java = java_body["observation"]
        java_states.append(java)

        for step in range(args.max_actions + 1):
            java_legal = enriched_java_actions(java)
            mapping, legal_error = transform_legal_actions(rust.get("legal_actions", []), java_legal)
            if legal_error is not None:
                legal_error["java_executor_actions"] = java_legal
                return False, strict_failure(seed, step, "legal_actions", rust, java, legal_error)

            comparison = request(
                args.rust_rpc,
                args.timeout,
                "POST",
                f"/v1/sessions/{rust_session}/compare",
                {"observation": java},
            )
            if not comparison["matched"]:
                return False, strict_failure(
                    seed,
                    step,
                    "state",
                    rust,
                    java,
                    {
                        "mismatched": comparison["mismatched"],
                        "rust_state": comparison.get("rust"),
                        "java_state": comparison.get("java"),
                    },
                )

            decision = rust.get("decision")
            if decision is None:
                if rust.get("done") and not java.get("legal_actions"):
                    if args.oracle_dir is not None:
                        write_oracle(args.oracle_dir, seed, java_states, java_commands)
                    return True, {
                        "status": "complete",
                        "seed": seed,
                        "steps": step,
                        "boundary": java.get("boundary"),
                    }
                return False, strict_failure(
                    seed,
                    step,
                    "termination",
                    rust,
                    java,
                    {"reason": "Rust has no HTN decision before both sessions are terminal"},
                )

            try:
                decision_index = rust["legal_actions"].index(decision)
            except ValueError:
                return False, strict_failure(
                    seed,
                    step,
                    "rust_rpc",
                    rust,
                    java,
                    {"reason": "HTN decision is absent from Rust legal actions", "decision": decision},
                )
            java_action = mapping[decision_index]["java"]

            java = request(
                args.java_rpc,
                args.timeout,
                "POST",
                f"/v1/sessions/{java_session}/step",
                java_action,
            )["observation"]
            java_commands.append(java_action)
            java_states.append(java)
            rust = request(
                args.rust_rpc,
                args.timeout,
                "POST",
                f"/v1/sessions/{rust_session}/step",
                {"action": decision},
            )

        return False, strict_failure(
            seed,
            args.max_actions,
            "action_cap",
            rust,
            java,
            {"reason": "maximum action count reached"},
        )
    finally:
        if rust_session is not None:
            try:
                request(args.rust_rpc, args.timeout, "DELETE", f"/v1/sessions/{rust_session}")
            except Exception:
                pass
        if java_session is not None:
            try:
                request(args.java_rpc, args.timeout, "DELETE", f"/v1/sessions/{java_session}")
            except Exception:
                pass


def parse_args():
    parser = argparse.ArgumentParser(description=__doc__)
    seeds = parser.add_mutually_exclusive_group(required=True)
    seeds.add_argument("--seed", type=int)
    seeds.add_argument("--seed-list", type=Path)
    parser.add_argument("--rust-rpc", default=os.environ.get("STS_HTN_RPC", "http://127.0.0.1:18082"))
    parser.add_argument("--java-rpc", default=os.environ.get("EXACTSIM_RPC", "http://127.0.0.1:18081"))
    parser.add_argument("--oracle-dir", type=Path)
    parser.add_argument("--timeout", type=float, default=120.0)
    parser.add_argument("--max-actions", type=int, default=5000)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    seeds = [args.seed] if args.seed is not None else load_seeds(args.seed_list)
    for index, seed in enumerate(seeds, 1):
        try:
            matched, result = run_seed(args, seed)
        except Exception as error:
            result = {
                "status": "error",
                "seed": seed,
                "error": f"{type(error).__name__}: {error}",
            }
            matched = False
        if not matched:
            json.dump(result, sys.stdout, indent=2, sort_keys=True)
            sys.stdout.write("\n")
            return 1
        print(
            f"[{index}/{len(seeds)}] seed={seed} exact steps={result['steps']} "
            f"boundary={result.get('boundary')}"
        )
    print(f"exact={len(seeds)} total={len(seeds)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
