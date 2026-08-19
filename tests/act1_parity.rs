//! Walk the seed-2 Act 1 transcript and stop at the first HP / pile / monster mismatch.

use serde::Deserialize;
use sts_engine::action::Action;
use sts_engine::game::{Game, Screen};
use sts_engine::ids::Character;
use sts_engine::Unlocks;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[derive(Deserialize)]
struct Envelope {
    sequence: usize,
    boundary: String,
    state: State,
}

#[derive(Deserialize)]
struct State {
    dungeon: Dungeon,
    player: Player,
    combat: Option<Combat>,
}

#[derive(Deserialize)]
struct Dungeon {
    floor: i32,
    act: i32,
}

#[derive(Deserialize)]
struct Player {
    current_hp: i32,
    block: i32,
    gold: i32,
    master_deck: Vec<Named>,
    relics: Vec<Named>,
}

#[derive(Deserialize)]
struct Named {
    id: String,
}

#[derive(Deserialize)]
struct Combat {
    hand: Vec<Named>,
    monsters: Vec<Mon>,
}

#[derive(Deserialize)]
struct Mon {
    id: String,
    current_hp: i32,
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn seed2_act1_walk() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime");
    let snaps: Vec<Envelope> = BufReader::new(File::open(root.join("act1-seed2-final-pruned.jsonl")).unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .collect();
    let cmds = sts_engine::load_commands(root.join("act1-seed2.commands.jsonl")).unwrap();
    let mut game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    let mut last_ok = 0;
    for snap in &snaps {
        let seq = snap.sequence;
        if seq > 0 {
            if seq - 1 < cmds.len() {
                game.step(&cmds[seq - 1]);
            }
        }
        let deck: Vec<_> = game.player.deck.iter().map(|c| c.sts_id().to_string()).collect();
        let jdeck: Vec<_> = snap.state.player.master_deck.iter().map(|c| c.id.clone()).collect();
        let rust_mons: Vec<(String, i32)> = game
            .combat
            .as_ref()
            .map(|c| {
                c.monsters
                    .iter()
                    .map(|m| (m.id.sts_id().to_string(), m.hp))
                    .collect()
            })
            .unwrap_or_default();
        let java_mons: Vec<(String, i32)> = snap
            .state
            .combat
            .as_ref()
            .map(|c| {
                c.monsters
                    .iter()
                    .map(|m| (m.id.clone(), m.current_hp))
                    .collect()
            })
            .unwrap_or_default();
        let rust_hand: Vec<_> = game.player.hand.iter().map(|c| c.sts_id().to_string()).collect();
        let java_hand: Vec<_> = snap
            .state
            .combat
            .as_ref()
            .map(|c| c.hand.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        if game.player.hp != snap.state.player.current_hp
            || game.player.gold != snap.state.player.gold
            || game.dungeon.floor != snap.state.dungeon.floor
            || game.dungeon.act as i32 != snap.state.dungeon.act
            || deck != jdeck
            || rust_mons != java_mons
            || rust_hand != java_hand
        {
            panic!(
                "first mismatch at seq {seq} {}\n  rust act={} floor={} hp={} gold={} screen={:?} deck={:?}\n  java act={} floor={} hp={} gold={} deck={:?}\n  rust mons={:?} hand={:?}\n  java mons={:?} hand={:?}\n  last ok {last_ok}",
                snap.boundary,
                game.dungeon.act as i32,
                game.dungeon.floor,
                game.player.hp,
                game.player.gold,
                game.screen,
                deck,
                snap.state.dungeon.act,
                snap.state.dungeon.floor,
                snap.state.player.current_hp,
                snap.state.player.gold,
                jdeck,
                rust_mons,
                rust_hand,
                java_mons,
                java_hand,
            );
        }
        last_ok = seq;
    }
    assert_eq!(last_ok, snaps.len() - 1);
}


