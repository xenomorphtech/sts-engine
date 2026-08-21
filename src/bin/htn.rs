//! Run the HTN autoplayer on the rust engine.
//!
//! ```sh
//! cargo run --release --bin sts-htn -- --character DEFECT --seed 7 --ascension 0
//! cargo run --release --bin sts-htn -- --seed 0 --count 100 --concurrent 6 --a0
//! ```

use std::collections::HashSet;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Character, PowerId, RoomType};
use sts_engine::rng::StsRandom;
use sts_engine::Unlocks;

#[derive(Clone, Copy)]
enum Outcome {
    Win,
    Loss,
    Capped,
    Stopped,
}

impl Outcome {
    fn label(self) -> &'static str {
        match self {
            Outcome::Win => "win",
            Outcome::Loss => "loss",
            Outcome::Capped => "capped",
            Outcome::Stopped => "stopped",
        }
    }
}

struct RunResult {
    game: Game,
    steps: usize,
    outcome: Outcome,
    diagnostics: RunDiagnostics,
}

#[derive(Clone, Debug, Default)]
struct RunDiagnostics {
    monsters: usize,
    elites: usize,
    rests: usize,
    events: usize,
    shops: usize,
    treasures: usize,
    bosses: usize,
    boss_entry_hp: Vec<String>,
    rested: usize,
    smithed: usize,
    recalled: usize,
    paths: [String; 4],
}

struct SeedDetail {
    seed: i64,
    outcome: Outcome,
    steps: usize,
    floor_achieved: i32,
    monsters_with_hp: String,
    diagnostics: RunDiagnostics,
    final_focus: i32,
    final_orbs: String,
    final_relics: String,
    final_deck: String,
}

impl SeedDetail {
    fn from_run(seed: i64, run: &RunResult) -> Self {
        let monsters_with_hp = run
            .game
            .combat
            .as_ref()
            .map(|combat| {
                combat
                    .monsters
                    .iter()
                    .filter(|monster| monster.hp > 0 && !monster.dead && !monster.escaped)
                    .map(|monster| format!("{:?}={}/{}", monster.id, monster.hp, monster.max_hp))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .filter(|monsters| !monsters.is_empty())
            .unwrap_or_else(|| "-".to_string());

        Self {
            seed,
            outcome: run.outcome,
            steps: run.steps,
            floor_achieved: run.game.dungeon.floor,
            monsters_with_hp,
            diagnostics: run.diagnostics.clone(),
            final_focus: run.game.player.power_amount(PowerId::Focus),
            final_orbs: run
                .game
                .player
                .orbs
                .iter()
                .map(|orb| format!("{:?}:{}", orb.kind, orb.evoke))
                .collect::<Vec<_>>()
                .join(","),
            final_relics: run
                .game
                .player
                .relics
                .iter()
                .map(|relic| relic.id.sts_id())
                .collect::<Vec<_>>()
                .join(","),
            final_deck: run
                .game
                .player
                .deck
                .iter()
                .map(|card| format!("{}{}", card.sts_id(), if card.upgraded { "+" } else { "" }))
                .collect::<Vec<_>>()
                .join(","),
        }
    }
}

#[derive(Default)]
struct BatchStats {
    wins: usize,
    losses: usize,
    capped: usize,
    stopped: usize,
    steps: usize,
    max_floor_achieved: i32,
    floor_achieved_sum: i64,
}

impl BatchStats {
    fn record(&mut self, detail: &SeedDetail) {
        self.steps += detail.steps;
        self.max_floor_achieved = self.max_floor_achieved.max(detail.floor_achieved);
        self.floor_achieved_sum += i64::from(detail.floor_achieved);
        match detail.outcome {
            Outcome::Win => self.wins += 1,
            Outcome::Loss => self.losses += 1,
            Outcome::Capped => self.capped += 1,
            Outcome::Stopped => self.stopped += 1,
        }
    }
}

fn consecutive_seeds(first_seed: i64, count: usize) -> Vec<i64> {
    (0..count)
        .map(|offset| {
            let offset = i64::try_from(offset).unwrap_or(i64::MAX);
            first_seed.saturating_add(offset)
        })
        .collect()
}

fn randomized_seeds(source: i64, count: usize) -> Vec<i64> {
    let mut rng = StsRandom::from_seed(source);
    let mut seen = HashSet::with_capacity(count);
    let mut seeds = Vec::with_capacity(count);
    while seeds.len() < count {
        let seed = rng.random_long() & i64::MAX;
        if seen.insert(seed) {
            seeds.push(seed);
        }
    }
    seeds
}

fn run_batch(
    seeds: &[i64],
    concurrent: usize,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> Vec<SeedDetail> {
    let count = seeds.len();
    let worker_count = concurrent.min(count);
    if worker_count == 1 {
        return seeds
            .iter()
            .map(|&seed| {
                let run = run_seed(seed, character, ascension, max_steps, unlocks);
                SeedDetail::from_run(seed, &run)
            })
            .collect();
    }

    let next_offset = AtomicUsize::new(0);
    let mut details = thread::scope(|scope| {
        let mut workers = Vec::with_capacity(worker_count);
        for _ in 0..worker_count {
            let next_offset = &next_offset;
            workers.push(scope.spawn(move || {
                let mut local = Vec::new();
                loop {
                    let offset = next_offset.fetch_add(1, Ordering::Relaxed);
                    if offset >= count {
                        break;
                    }
                    let seed = seeds[offset];
                    let run = run_seed(seed, character, ascension, max_steps, unlocks);
                    local.push(SeedDetail::from_run(seed, &run));
                }
                local
            }));
        }
        workers
            .into_iter()
            .flat_map(|worker| worker.join().expect("HTN worker panicked"))
            .collect::<Vec<_>>()
    });
    details.sort_unstable_by_key(|detail| detail.seed);
    details
}

fn run_seed(
    seed: i64,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> RunResult {
    let mut game = Game::new(seed, character, ascension, unlocks.clone());
    let mut agent = HtnAgent::new();
    let mut steps = 0usize;
    let mut diagnostics = RunDiagnostics::default();

    while !game.done && game.player.hp > 0 && game.screen != Screen::Terminal && steps < max_steps {
        let action = agent.decide(&game);
        if matches!(action, sts_engine::Action::Quit) {
            break;
        }
        let screen_before = game.screen;
        if screen_before == Screen::Map {
            if let sts_engine::Action::Choose {
                room: Some(room), ..
            } = &action
            {
                let room = room_type(room);
                match room {
                    RoomType::Monster => diagnostics.monsters += 1,
                    RoomType::Elite => diagnostics.elites += 1,
                    RoomType::Rest => diagnostics.rests += 1,
                    RoomType::Event => diagnostics.events += 1,
                    RoomType::Shop => diagnostics.shops += 1,
                    RoomType::Treasure | RoomType::BossTreasure => diagnostics.treasures += 1,
                    RoomType::Boss => {
                        diagnostics.bosses += 1;
                        diagnostics.boss_entry_hp.push(format!(
                            "{}:{}/{}",
                            game.dungeon.act as i32, game.player.hp, game.player.max_hp
                        ));
                    }
                    _ => {}
                }
                let symbol = match room {
                    RoomType::Monster => 'M',
                    RoomType::Elite => 'E',
                    RoomType::Rest => 'R',
                    RoomType::Event => '?',
                    RoomType::Shop => '$',
                    RoomType::Treasure | RoomType::BossTreasure => 'T',
                    RoomType::Boss => 'B',
                    _ => '-',
                };
                let act_index = (game.dungeon.act as usize).saturating_sub(1).min(3);
                diagnostics.paths[act_index].push(symbol);
            }
        } else if screen_before == Screen::Rest {
            if let sts_engine::Action::Choose {
                label: Some(label), ..
            } = &action
            {
                if label.eq_ignore_ascii_case("rest") {
                    diagnostics.rested += 1;
                } else if label.eq_ignore_ascii_case("smith") {
                    diagnostics.smithed += 1;
                } else if label.eq_ignore_ascii_case("recall") {
                    diagnostics.recalled += 1;
                }
            }
        }
        game.step(&action);
        steps += 1;
    }

    let outcome = if game.done && game.player.hp > 0 && game.screen != Screen::Terminal {
        Outcome::Win
    } else if game.player.hp <= 0 {
        Outcome::Loss
    } else if steps >= max_steps {
        Outcome::Capped
    } else {
        Outcome::Stopped
    };

    RunResult {
        game,
        steps,
        outcome,
        diagnostics,
    }
}

fn room_type(java_class: &str) -> RoomType {
    if java_class.contains("MonsterRoomElite") {
        RoomType::Elite
    } else if java_class.contains("MonsterRoomBoss") {
        RoomType::Boss
    } else if java_class.contains("MonsterRoom") {
        RoomType::Monster
    } else if java_class.contains("RestRoom") {
        RoomType::Rest
    } else if java_class.contains("ShopRoom") {
        RoomType::Shop
    } else if java_class.contains("TreasureRoomBoss") {
        RoomType::BossTreasure
    } else if java_class.contains("TreasureRoom") {
        RoomType::Treasure
    } else if java_class.contains("EventRoom") {
        RoomType::Event
    } else {
        RoomType::Empty
    }
}

fn print_single(seed: i64, character: Character, ascension: i32, run: &RunResult) {
    let game = &run.game;
    println!(
        "character={:?} seed={} asc={} steps={} floor={} act={:?} screen={:?} hp={}/{} gold={} deck={} relics={} done={}",
        character,
        seed,
        ascension,
        run.steps,
        game.dungeon.floor,
        game.dungeon.act,
        game.screen,
        game.player.hp,
        game.player.max_hp,
        game.player.gold,
        game.player.deck.len(),
        game.player.relics.len(),
        game.done || game.player.hp <= 0,
    );
    if let Some(event) = &game.event {
        println!(
            "event={} event_screen={} options={:?}",
            event.id, event.screen, event.options
        );
    }
    if let Some(combat) = &game.combat {
        println!(
            "combat_turn={} energy={} block={} powers={:?} hand={:?} draw_top={:?} discard={} monsters={:?}",
            combat.turn,
            game.player.energy,
            game.player.block,
            game.player
                .powers
                .iter()
                .map(|power| format!("{:?}={}", power.id, power.amount))
                .collect::<Vec<_>>(),
            game.player
                .hand
                .iter()
                .map(|card| card.sts_id())
                .collect::<Vec<_>>(),
            game.player.draw.last().map(|card| card.sts_id()),
            game.player.discard.len(),
            combat
                .monsters
                .iter()
                .filter(|monster| monster.alive())
                .map(|monster| format!(
                    "{}={}/{} intent={:?} {}x{} powers={:?}",
                    monster.id.sts_id(),
                    monster.hp,
                    monster.max_hp,
                    monster.intent,
                    monster.intent_damage,
                    monster.intent_hits,
                    monster
                        .powers
                        .iter()
                        .map(|power| format!("{:?}={}", power.id, power.amount))
                        .collect::<Vec<_>>()
                ))
                .collect::<Vec<_>>()
        );
        let legal = game.legal_actions();
        let mut agent = HtnAgent::new();
        println!(
            "next_legal={legal:?} next_decision={:?}",
            agent.decide(game)
        );
    }
    println!(
        "deck={}",
        game.player
            .deck
            .iter()
            .map(|card| format!("{}{}", card.sts_id(), if card.upgraded { "+" } else { "" }))
            .collect::<Vec<_>>()
            .join(",")
    );
}

fn print_batch(
    character: Character,
    seeds: &[i64],
    random_source: Option<i64>,
    concurrent: usize,
    ascension: i32,
    max_steps: usize,
    stats: &BatchStats,
    details: &[SeedDetail],
    elapsed: Duration,
    diagnostics: bool,
) {
    let count = seeds.len();
    let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
    let seeds_per_second = count as f64 / seconds;
    let steps_per_second = stats.steps as f64 / seconds;
    let win_rate = stats.wins as f64 * 100.0 / count as f64;
    let mean_floor_achieved = stats.floor_achieved_sum as f64 / count as f64;
    let cohort = if let Some(source) = random_source {
        format!("cohort=random seed_source={source}")
    } else {
        format!(
            "cohort=consecutive range={}..={}",
            seeds.first().copied().unwrap_or(0),
            seeds.last().copied().unwrap_or(0)
        )
    };
    println!(
        "character={:?} asc={} seeds={} concurrent={} {} wins={} losses={} capped={} stopped={} win_rate={:.2}% max_floor_achieved={} mean_floor_achieved={:.2} steps={} max_steps={} elapsed={:.6}s seeds/s={:.1} steps/s={:.0}",
        character,
        ascension,
        count,
        concurrent.min(count),
        cohort,
        stats.wins,
        stats.losses,
        stats.capped,
        stats.stopped,
        win_rate,
        stats.max_floor_achieved,
        mean_floor_achieved,
        stats.steps,
        max_steps,
        seconds,
        seeds_per_second,
        steps_per_second,
    );
    if diagnostics {
        println!("seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining\tnormals\telites\trests\tevents\tshops\ttreasures\tbosses\tboss_entry_hp\trested\tsmithed\trecalled\tact1_path\tact2_path\tact3_path\tact4_path\tfinal_focus\tfinal_orbs\tfinal_relics\tfinal_deck");
    } else {
        println!("seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining");
    }
    for detail in details {
        if diagnostics {
            println!(
                "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                detail.seed,
                detail.outcome.label(),
                detail.floor_achieved,
                detail.monsters_with_hp,
                detail.diagnostics.monsters,
                detail.diagnostics.elites,
                detail.diagnostics.rests,
                detail.diagnostics.events,
                detail.diagnostics.shops,
                detail.diagnostics.treasures,
                detail.diagnostics.bosses,
                detail.diagnostics.boss_entry_hp.join(","),
                detail.diagnostics.rested,
                detail.diagnostics.smithed,
                detail.diagnostics.recalled,
                detail.diagnostics.paths[0],
                detail.diagnostics.paths[1],
                detail.diagnostics.paths[2],
                detail.diagnostics.paths[3],
                detail.final_focus,
                detail.final_orbs,
                detail.final_relics,
                detail.final_deck,
            );
        } else {
            println!(
                "{}\t{}\t{}\t{}",
                detail.seed,
                detail.outcome.label(),
                detail.floor_achieved,
                detail.monsters_with_hp
            );
        }
    }
}

fn main() {
    let mut character = Character::Defect;
    let mut seed: i64 = 2;
    let mut count: usize = 1;
    let mut concurrent: usize = 1;
    let mut ascension: i32 = 0;
    let mut max_steps: usize = 5000;
    let mut diagnostics = false;
    let mut randomize = false;
    let mut random_source: Option<i64> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--character" | "-c" => {
                let v = args.next().unwrap_or_else(|| "DEFECT".into());
                character = match v.to_ascii_uppercase().as_str() {
                    "DEFECT" => Character::Defect,
                    "SILENT" | "THE_SILENT" => Character::Silent,
                    "WATCHER" => Character::Watcher,
                    "IRONCLAD" | "IRON_CLAD" => Character::Ironclad,
                    _ => Character::Defect,
                };
            }
            "--seed" | "-s" => seed = args.next().and_then(|s| s.parse().ok()).unwrap_or(2),
            "--count" | "--seeds" | "-n" => {
                count = args.next().and_then(|s| s.parse().ok()).unwrap_or(1)
            }
            "--concurrent" | "-j" => {
                concurrent = args.next().and_then(|s| s.parse().ok()).unwrap_or(1)
            }
            "--ascension" | "-a" => {
                ascension = args.next().and_then(|s| s.parse().ok()).unwrap_or(0)
            }
            "--a0" => ascension = 0,
            "--a20" => ascension = 20,
            "--max-steps" => max_steps = args.next().and_then(|s| s.parse().ok()).unwrap_or(5000),
            "--diagnostics" => diagnostics = true,
            "--random-seeds" => randomize = true,
            "--seed-source" => {
                random_source = args.next().and_then(|s| s.parse().ok());
                randomize = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: sts-htn [--character CHARACTER] [--seed FIRST_SEED] [--count N] [--concurrent N] [--random-seeds] [--seed-source N] [--ascension 0|20] [--max-steps N] [--diagnostics]\n\nBatch mode runs seeds in one process and prints aggregate throughput, win rate, and per-seed results. --random-seeds generates a fresh cohort; --seed-source makes that cohort reproducible. Runs cap at 5000 steps by default; any capped seed should be treated as a loop bug."
                );
                return;
            }
            other => {
                if let Ok(n) = other.parse::<i64>() {
                    seed = n;
                }
            }
        }
    }

    if count == 0 {
        eprintln!("--count must be greater than zero");
        std::process::exit(2);
    }
    if concurrent == 0 {
        eprintln!("--concurrent must be greater than zero");
        std::process::exit(2);
    }

    // Load the profile-backed unlock data once, then clone the in-memory value
    // into each fresh game. No assets or profile files are reloaded per seed.
    let unlocks = Unlocks::fixture();
    let random_source = if randomize {
        Some(random_source.unwrap_or_else(|| {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            (nanos as u64 & i64::MAX as u64) as i64
        }))
    } else {
        None
    };
    let seeds = if let Some(source) = random_source {
        randomized_seeds(source, count)
    } else {
        consecutive_seeds(seed, count)
    };
    if count == 1 {
        let actual_seed = seeds[0];
        let run = run_seed(actual_seed, character, ascension, max_steps, &unlocks);
        print_single(actual_seed, character, ascension, &run);
        return;
    }

    let start = Instant::now();
    let details = run_batch(
        &seeds, concurrent, character, ascension, max_steps, &unlocks,
    );
    let elapsed = start.elapsed();
    let mut stats = BatchStats::default();
    for detail in &details {
        stats.record(detail);
    }
    print_batch(
        character,
        &seeds,
        random_source,
        concurrent,
        ascension,
        max_steps,
        &stats,
        &details,
        elapsed,
        diagnostics,
    );
}
