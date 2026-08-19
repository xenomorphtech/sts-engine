//! Snapshot-level parity for seed 2's opening Cultist fight.

use serde::Deserialize;
use sts_engine::action::Action;
use sts_engine::game::Game;
use sts_engine::ids::Character;
use sts_engine::Unlocks;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

#[derive(Deserialize)]
struct Envelope {
    sequence: u64,
    boundary: String,
    state: State,
}

#[derive(Deserialize)]
struct State {
    player: Player,
    combat: Option<Combat>,
}

#[derive(Deserialize)]
struct Player {
    current_hp: i32,
    block: i32,
}

#[derive(Deserialize)]
struct Combat {
    turn: i32,
    hand: Vec<Card>,
    draw_pile: Vec<Card>,
    discard_pile: Vec<Card>,
    monsters: Vec<Monster>,
}

#[derive(Deserialize)]
struct Card {
    id: String,
}

#[derive(Deserialize)]
struct Monster {
    id: String,
    current_hp: i32,
    powers: Vec<Power>,
}

#[derive(Deserialize)]
struct Power {
    id: String,
    amount: i32,
}

fn load_java() -> (Vec<Envelope>, Vec<Action>) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime");
    let snaps: Vec<Envelope> = BufReader::new(File::open(root.join("act1-seed2-final-pruned.jsonl")).unwrap())
        .lines()
        .map(|l| serde_json::from_str(&l.unwrap()).unwrap())
        .take(29)
        .collect();
    let cmds = sts_engine::load_commands(root.join("act1-seed2.commands.jsonl")).unwrap();
    (snaps, cmds)
}

#[test]
fn seed2_cultist_fight_matches_java() {
    let (snaps, cmds) = load_java();
    let mut game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    for i in 0..4 {
        game.step(&cmds[i]);
    }
    for seq in 4..=27 {
        let java = &snaps[seq];
        assert_eq!(java.boundary, "combat_turn");
        let combat = game.combat.as_ref().expect("in combat");
        let jcombat = java.state.combat.as_ref().unwrap();
        let hand: Vec<_> = game.player.hand.iter().map(|c| c.sts_id()).collect();
        let jhand: Vec<_> = jcombat.hand.iter().map(|c| c.id.as_str()).collect();
        let mhp = combat.monsters[0].hp;
        let jhp = jcombat.monsters[0].current_hp;
        if seq == 7 || seq == 9 {
            eprintln!(
                "seq{seq} rust disc={:?} draw={:?} hand={:?}",
                game.player.discard.iter().map(|c| c.sts_id()).collect::<Vec<_>>(),
                game.player.draw.iter().map(|c| c.sts_id()).collect::<Vec<_>>(),
                hand
            );
        }
        if hand != jhand || mhp != jhp || game.player.hp != java.state.player.current_hp {
            panic!(
                "seq {seq} mismatch\n  rust hp={} block={} mon={} hand={:?} turn={}\n  java hp={} block={} mon={} hand={:?} turn={}\n  powers rust={:?} java={:?}",
                game.player.hp,
                game.player.block,
                mhp,
                hand,
                combat.turn,
                java.state.player.current_hp,
                java.state.player.block,
                jhp,
                jhand,
                jcombat.turn,
                combat.monsters[0]
                    .powers
                    .iter()
                    .map(|p| format!("{:?}={}", p.id, p.amount))
                    .collect::<Vec<_>>(),
                jcombat.monsters[0]
                    .powers
                    .iter()
                    .map(|p| format!("{}={}", p.id, p.amount))
                    .collect::<Vec<_>>(),
            );
        }
        game.step(&cmds[seq]);
    }
    game.step(&cmds[27]);
    assert!(game.combat.is_none() || game.combat.as_ref().unwrap().all_dead());
    assert_eq!(game.player.hp, 68, "burning blood after cultist");
}

#[test]
fn turn3_reshuffle_permutation() {
    use sts_engine::java_util::shuffle_java;
    use sts_engine::rng::StsRandom;
    let mut rng = StsRandom::from_seed(2 + 1);
    let _ = rng.random_long(); // combat-start shuffle
    let seed = rng.random_long();
    let mut pile = vec!["Strike_R","Strike_R","Defend_R","Defend_R","Defend_R","Defend_R","Bash","Strike_R","Strike_R","Strike_R"];
    shuffle_java(&mut pile, seed);
    eprintln!("shuffled {:?}", pile);
    let hand: Vec<_> = pile.iter().rev().take(5).cloned().collect();
    let draw: Vec<_> = pile.iter().rev().skip(5).cloned().collect();
    eprintln!("drawn-from-end hand {:?} draw {:?}", hand, draw);
    assert_eq!(hand, vec!["Strike_R","Defend_R","Strike_R","Strike_R","Bash"]);
}
