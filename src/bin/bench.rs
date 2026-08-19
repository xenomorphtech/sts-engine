use sts_engine::{load_commands, replay_seed, TrainEnv, Unlocks};
use std::time::Instant;

fn main() {
    let path = "../exact-text-sim/runtime/act1-seed2.commands.jsonl";
    let commands = load_commands(path).expect("commands");
    let warmup = replay_seed(2, &commands, Unlocks::fixture());
    println!(
        "warmup floor={} hp={} gold={}",
        warmup.dungeon.floor, warmup.player.hp, warmup.player.gold
    );

    let iters = 200usize;
    let start = Instant::now();
    for _ in 0..iters {
        let _ = replay_seed(2, &commands, Unlocks::fixture());
    }
    let elapsed = start.elapsed();
    let per = elapsed / iters as u32;
    println!(
        "replayed {iters} Act 1 transcripts in {elapsed:?} ({per:?} each, {:.1} acts/s)",
        iters as f64 / elapsed.as_secs_f64()
    );

    let mut env = TrainEnv::new(2);
    let start = Instant::now();
    let mut steps = 0u64;
    for seed in 0..256i64 {
        env.reset(seed);
        for _ in 0..64 {
            let n = env.game.legal_actions().len();
            if n == 0 {
                break;
            }
            let info = env.step(0);
            steps += 1;
            if info.done {
                break;
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "random-policy {steps} steps in {elapsed:?} ({:.0} steps/s)",
        steps as f64 / elapsed.as_secs_f64()
    );
}
