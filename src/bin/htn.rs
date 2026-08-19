//! Run the HTN autoplayer on the rust engine.
//!
//! ```sh
//! cargo run --release --bin sts-htn -- --character DEFECT --seed 7 --ascension 0
//! ```

use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::Character;
use sts_engine::Unlocks;
use std::env;

fn main() {
    let mut character = Character::Ironclad;
    let mut seed: i64 = 2;
    let mut ascension: i32 = 0;
    let mut max_steps: usize = 4000;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--character" | "-c" => {
                let v = args.next().unwrap_or_else(|| "IRONCLAD".into());
                character = match v.to_ascii_uppercase().as_str() {
                    "DEFECT" => Character::Defect,
                    "SILENT" | "THE_SILENT" => Character::Silent,
                    "WATCHER" => Character::Watcher,
                    _ => Character::Ironclad,
                };
            }
            "--seed" | "-s" => seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(2),
            "--ascension" | "-a" => ascension = args.next().and_then(|s| s.parse().ok()).unwrap_or(0),
            "--max-steps" => max_steps = args.next().and_then(|s| s.parse().ok()).unwrap_or(4000),
            other => {
                if let Ok(n) = other.parse::<i64>() {
                    seed = n;
                }
            }
        }
    }

    let mut game = Game::new(seed, character, ascension, Unlocks::fixture());
    let mut agent = HtnAgent::new();
    let mut steps = 0usize;
    while !game.done && game.player.hp > 0 && game.screen != Screen::Terminal && steps < max_steps {
        let action = agent.decide(&game);
        if matches!(action, sts_engine::Action::Quit) {
            break;
        }
        game.step(&action);
        steps += 1;
    }
    println!(
        "character={:?} seed={} asc={} steps={} floor={} act={:?} hp={}/{} gold={} deck={} relics={} done={}",
        character,
        seed,
        ascension,
        steps,
        game.dungeon.floor,
        game.dungeon.act,
        game.player.hp,
        game.player.max_hp,
        game.player.gold,
        game.player.deck.len(),
        game.player.relics.len(),
        game.done || game.player.hp <= 0,
    );
}
