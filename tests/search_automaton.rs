//! Search a winning Bronze Automaton line from a recorded prefix.

use std::path::PathBuf;
use sts_engine::action::Action;
use sts_engine::game::{Game, Screen};
use sts_engine::ids::{Character, MonsterId};
use sts_engine::Unlocks;

fn auto_hp(game: &Game) -> i32 {
    game.combat
        .as_ref()
        .and_then(|c| {
            c.monsters
                .iter()
                .find(|m| m.id == MonsterId::BronzeAutomaton)
        })
        .map(|m| m.hp)
        .unwrap_or(0)
}

fn player_dead(game: &Game) -> bool {
    game.player.hp <= 0
}

fn auto_dead(game: &Game) -> bool {
    game.combat.as_ref().is_none_or(|c| {
        c.monsters
            .iter()
            .filter(|m| m.id == MonsterId::BronzeAutomaton)
            .all(|m| !m.alive())
    })
}

fn score(game: &Game) -> i64 {
    if player_dead(game) {
        return -1_000_000;
    }
    if auto_dead(game) {
        return 10_000_000 + i64::from(game.player.hp);
    }
    i64::from(game.player.hp) * 100 - i64::from(auto_hp(game))
}

#[test]
fn search_seed2_automaton() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime");
    let cmds_path = root.join("hunt2-s2.commands.jsonl");
    let snaps_path = root.join("hunt2-s2.jsonl");
    if !cmds_path.exists() {
        eprintln!("skip search: missing {}", cmds_path.display());
        return;
    }
    let cmds = sts_engine::load_commands(&cmds_path).unwrap();
    let mut game = Game::new(2, Character::Ironclad, 0, Unlocks::guardian_champ());
    if snaps_path.exists() {
        let snaps: Vec<serde_json::Value> = std::io::BufRead::lines(std::io::BufReader::new(
            std::fs::File::open(&snaps_path).unwrap(),
        ))
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
        for snap in &snaps {
            let seq = snap["sequence"].as_u64().unwrap() as usize;
            if seq > 0 && seq - 1 < cmds.len() {
                game.step(&cmds[seq - 1]);
            }
            let jhp = snap["state"]["player"]["current_hp"].as_i64().unwrap() as i32;
            let jfloor = snap["state"]["dungeon"]["floor"].as_i64().unwrap() as i32;
            if game.player.hp != jhp || game.dungeon.floor != jfloor {
                eprintln!(
                    "desync seq {seq} rust hp={} floor={} java hp={} floor={}",
                    game.player.hp, game.dungeon.floor, jhp, jfloor
                );
                break;
            }
        }
        game = Game::new(2, Character::Ironclad, 0, Unlocks::guardian_champ());
    }
    let mut start = 0;
    for (i, cmd) in cmds.iter().enumerate() {
        game.step(cmd);
        if game.dungeon.floor == 33 && game.screen == Screen::Combat {
            start = i + 1;
            break;
        }
        if game.player.hp <= 0 {
            eprintln!("rust died at floor {} after cmd {i}", game.dungeon.floor);
            return;
        }
    }
    assert_eq!(game.dungeon.floor, 33, "did not reach floor 33");
    eprintln!(
        "search start after cmd {start} hp={} auto={} hand={}",
        game.player.hp,
        auto_hp(&game),
        game.player.hand.len()
    );

    let mut extra: Vec<Action> = Vec::new();
    let mut guard = 0;
    while !auto_dead(&game) && !player_dead(&game) && guard < 80 {
        guard += 1;
        let legal = game.legal_actions();
        let mut best: Option<(i64, Action, Game)> = None;
        for action in legal {
            if matches!(action, Action::Quit | Action::Skip) {
                continue;
            }
            let mut next = game.clone();
            next.step(&action);
            let s = score(&next);
            if best.as_ref().is_none_or(|(bs, _, _)| s > *bs) {
                best = Some((s, action, next));
            }
        }
        let Some((_, action, next)) = best else {
            eprintln!("no legal actions screen={:?}", game.screen);
            break;
        };
        extra.push(action.clone());
        eprintln!(
            "  play {:?} -> hp={} auto={} screen={:?}",
            action,
            next.player.hp,
            auto_hp(&next),
            next.screen
        );
        game = next;
    }
    eprintln!(
        "search end hp={} auto={} extra={} auto_dead={} player_dead={}",
        game.player.hp,
        auto_hp(&game),
        extra.len(),
        auto_dead(&game),
        player_dead(&game)
    );
    if auto_dead(&game) && !player_dead(&game) {
        let out =
            PathBuf::from("/tmp/grok-goal-bb496ccd9352/implementer/automaton-win.commands.jsonl");
        let mut lines: Vec<String> = cmds[..start]
            .iter()
            .map(|a| serde_json::to_string(a).unwrap())
            .collect();
        lines.extend(extra.iter().map(|a| serde_json::to_string(a).unwrap()));
        std::fs::write(&out, lines.join("\n") + "\n").unwrap();
        eprintln!("wrote {}", out.display());
    }
}
