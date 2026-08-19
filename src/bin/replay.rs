use sts_engine::{load_commands, replay_seed, Unlocks};
use std::env;
use std::time::Instant;

fn main() {
    let mut args = env::args().skip(1);
    let seed: i64 = args
        .next()
        .unwrap_or_else(|| "2".into())
        .parse()
        .expect("seed");
    let commands_path = args
        .next()
        .unwrap_or_else(|| "../exact-text-sim/runtime/act1-seed2.commands.jsonl".into());
    let commands = load_commands(&commands_path).expect("commands");
    let start = Instant::now();
    let game = replay_seed(seed, &commands, Unlocks::fixture());
    let elapsed = start.elapsed();
    println!(
        "seed={} commands={} floor={} act={:?} hp={}/{} gold={} deck={} relics={:?} done={} {:?}",
        seed,
        commands.len(),
        game.dungeon.floor,
        game.dungeon.act,
        game.player.hp,
        game.player.max_hp,
        game.player.gold,
        game.player.deck.len(),
        game.player
            .relics
            .iter()
            .map(|r| r.id.sts_id())
            .collect::<Vec<_>>(),
        game.done,
        elapsed
    );
}
