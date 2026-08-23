//! Run the HTN autoplayer on the rust engine.
//!
//! ```sh
//! cargo run --release --bin sts-htn -- --character DEFECT --seed 7 --ascension 0
//! cargo run --release --bin sts-htn -- --seed 0 --count 100 --concurrent 6 --a0
//! ```

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sts_engine::creature::OrbKind;
use sts_engine::game::{CampfireOption, EventOption, Game, Screen};
use sts_engine::htn::{HtnAgent, SearchStats};
use sts_engine::ids::{Act, CardId, Character, EventId, MonsterId, PowerId, RelicId, RoomType};
use sts_engine::rng::StsRandom;
use sts_engine::{Action, Unlocks};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Win,
    Loss,
    Capped,
    Stopped,
}

sts_engine::string_id_map! {
    Outcome,
    WIRE_NAMES,
    label,
    from_label {
        Outcome::Win => "win",
        Outcome::Loss => "loss",
        Outcome::Capped => "capped",
        Outcome::Stopped => "stopped",
    }
}

sts_engine::string_id_serde!(Outcome, label, from_label);

/// Stable, compact engine checkpoint. There is deliberately one record per
/// seed: this is a regression oracle, not a trace or an HTN diagnostics log.
#[derive(Debug, Deserialize, PartialEq, Eq, Serialize)]
struct FinalState {
    seed: i64,
    outcome: Outcome,
    steps: usize,
    floor: i32,
    act: i32,
    screen: String,
    room: String,
    hp: i32,
    max_hp: i32,
    block: i32,
    gold: i32,
    energy: i32,
    energy_master: i32,
    deck: Vec<String>,
    relics: Vec<String>,
    potions: Vec<String>,
    powers: Vec<String>,
    orbs: Vec<String>,
    combat: Option<String>,
    rng: sts_engine::rng::RngSetSnapshot,
}

#[derive(Deserialize, Serialize)]
struct ActionLog {
    seed: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    outcome: Option<Outcome>,
    actions: Vec<Action>,
}

fn load_action_log(path: &Path) -> Result<Vec<ActionLog>, String> {
    let input =
        BufReader::new(File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?);
    input
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let line = line.map_err(|e| e.to_string())?;
            serde_json::from_str(&line)
                .map_err(|e| format!("{} line {}: {e}", path.display(), i + 1))
        })
        .collect()
}

impl FinalState {
    fn from_run(seed: i64, run: &RunResult) -> Self {
        let game = &run.game;
        Self {
            seed,
            outcome: run.outcome,
            steps: run.steps,
            floor: game.dungeon.floor,
            act: game.dungeon.act as i32,
            screen: format!("{:?}", game.screen),
            room: format!("{:?}", game.current_room),
            hp: game.player.hp,
            max_hp: game.player.max_hp,
            block: game.player.block,
            gold: game.player.gold,
            energy: game.player.energy,
            energy_master: game.player.energy_master,
            deck: game.player.deck.iter().map(compact_card).collect(),
            relics: game
                .player
                .relics
                .iter()
                .map(|r| format!("{}:{}:{}", r.id.sts_id(), r.counter, u8::from(r.used_up)))
                .collect(),
            potions: game
                .player
                .potions
                .iter()
                .map(|p| format!("{}:{}", p.id.sts_id(), p.slot))
                .collect(),
            powers: game
                .player
                .powers
                .iter()
                .map(|p| {
                    format!(
                        "{:?}:{}:{}:{}:{}",
                        p.id,
                        p.amount,
                        u8::from(p.just_applied),
                        u8::from(p.skip_first),
                        p.misc
                    )
                })
                .collect(),
            orbs: game
                .player
                .orbs
                .iter()
                .map(|o| format!("{:?}:{}", o.kind, o.evoke))
                .collect(),
            combat: game.combat.as_ref().map(|c| {
                let monsters = c
                    .monsters
                    .iter()
                    .map(|m| {
                        format!(
                            "{}:{}/{}:{:?}:{}x{}:{}:{}",
                            m.id.sts_id(),
                            m.hp,
                            m.max_hp,
                            m.intent,
                            m.intent_damage,
                            m.intent_hits,
                            u8::from(m.dead),
                            u8::from(m.escaped)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{:?}:{}:[{}]", c.encounter, c.turn, monsters)
            }),
            rng: game.rng.snapshot(),
        }
    }
}

fn compact_card(card: &sts_engine::card::Card) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        card.sts_id(),
        card.times_upgraded,
        card.cost,
        card.cost_for_turn,
        card.base_damage,
        card.base_block,
        card.base_magic,
        card.misc,
        u8::from(card.free_to_play_once),
        u8::from(card.exhaust),
        u8::from(card.ethereal),
        u8::from(card.retain),
        u8::from(card.innate),
        u8::from(card.in_bottle),
        u8::from(card.upgraded),
    )
}

fn write_fixture(path: &Path, states: &[FinalState]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut out = BufWriter::new(file);
    for state in states {
        serde_json::to_writer(&mut out, state).map_err(|e| e.to_string())?;
        writeln!(out).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}

fn write_action_log(path: &Path, logs: &[ActionLog]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("create {}: {e}", path.display()))?;
    let mut out = BufWriter::new(file);
    for log in logs {
        serde_json::to_writer(&mut out, log).map_err(|e| e.to_string())?;
        writeln!(out).map_err(|e| e.to_string())?;
    }
    out.flush().map_err(|e| e.to_string())
}

fn run_final_batch(
    seeds: &[i64],
    concurrent: usize,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> Vec<FinalState> {
    let next_offset = AtomicUsize::new(0);
    let mut states = thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrent.min(seeds.len()));
        for _ in 0..concurrent.min(seeds.len()) {
            workers.push(scope.spawn(|| {
                let mut local = Vec::new();
                loop {
                    let offset = next_offset.fetch_add(1, Ordering::Relaxed);
                    if offset >= seeds.len() {
                        break;
                    }
                    let seed = seeds[offset];
                    local.push(FinalState::from_run(
                        seed,
                        &run_seed(
                            seed, character, ascension, max_steps, unlocks, false, false, false,
                        ),
                    ));
                }
                local
            }));
        }
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("HTN worker panicked"))
            .collect::<Vec<_>>()
    });
    states.sort_unstable_by_key(|state| state.seed);
    states
}

fn run_action_batch(
    seeds: &[i64],
    concurrent: usize,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> Vec<ActionLog> {
    let next_offset = AtomicUsize::new(0);
    let mut logs = thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrent.min(seeds.len()));
        for _ in 0..concurrent.min(seeds.len()) {
            workers.push(scope.spawn(|| {
                let mut local = Vec::new();
                loop {
                    let offset = next_offset.fetch_add(1, Ordering::Relaxed);
                    if offset >= seeds.len() {
                        break;
                    }
                    let seed = seeds[offset];
                    let run = run_seed(
                        seed, character, ascension, max_steps, unlocks, false, false, true,
                    );
                    local.push(ActionLog {
                        seed,
                        outcome: Some(run.outcome),
                        actions: run.actions,
                    });
                }
                local
            }));
        }
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("HTN worker panicked"))
            .collect::<Vec<_>>()
    });
    logs.sort_unstable_by_key(|log| log.seed);
    logs
}

fn replay_action_batch(
    logs: &[ActionLog],
    concurrent: usize,
    character: Character,
    ascension: i32,
    unlocks: &Unlocks,
) -> Vec<FinalState> {
    let next_offset = AtomicUsize::new(0);
    let mut states = thread::scope(|scope| {
        let mut workers = Vec::with_capacity(concurrent.min(logs.len()));
        for _ in 0..concurrent.min(logs.len()) {
            workers.push(scope.spawn(|| {
                let mut local = Vec::new();
                loop {
                    let offset = next_offset.fetch_add(1, Ordering::Relaxed);
                    if offset >= logs.len() {
                        break;
                    }
                    let log = &logs[offset];
                    let mut game = Game::new(log.seed, character, ascension, unlocks.clone());
                    let mut steps = 0;
                    for action in &log.actions {
                        game.step(action);
                        steps += 1;
                    }
                    let outcome = if game.done && game.player.hp > 0 && game.screen != Screen::Terminal {
                            Outcome::Win
                        } else if game.player.hp <= 0 {
                            Outcome::Loss
                        } else {
                            log.outcome.unwrap_or(Outcome::Stopped)
                        };
                    let run = RunResult {
                        game,
                        steps,
                        outcome,
                        diagnostics: RunDiagnostics::default(),
                        search: SearchStats::default(),
                        telemetry: OrbStats::default(),
                        actions: Vec::new(),
                    };
                    local.push(FinalState::from_run(log.seed, &run));
                }
                local
            }));
        }
        workers
            .into_iter()
            .flat_map(|w| w.join().expect("replay worker panicked"))
            .collect::<Vec<_>>()
    });
    states.sort_unstable_by_key(|state| state.seed);
    states
}

fn compare_fixture(path: &Path, actual: &[FinalState]) -> Result<(), String> {
    let input =
        BufReader::new(File::open(path).map_err(|e| format!("open {}: {e}", path.display()))?);
    let expected: Vec<FinalState> = input
        .lines()
        .enumerate()
        .map(|(i, line)| {
            let line = line.map_err(|e| e.to_string())?;
            serde_json::from_str(&line)
                .map_err(|e| format!("{} line {}: {e}", path.display(), i + 1))
        })
        .collect::<Result<_, _>>()?;
    if expected.len() != actual.len() {
        return Err(format!(
            "fixture count differs: expected {}, got {}",
            expected.len(),
            actual.len()
        ));
    }
    for (expected, actual) in expected.iter().zip(actual) {
        if expected != actual {
            return Err(format!(
                "seed {} final state differs\nexpected: {}\nactual:   {}",
                actual.seed,
                serde_json::to_string(expected).unwrap(),
                serde_json::to_string(actual).unwrap()
            ));
        }
    }
    Ok(())
}

struct RunResult {
    game: Game,
    steps: usize,
    outcome: Outcome,
    diagnostics: RunDiagnostics,
    search: SearchStats,
    telemetry: OrbStats,
    actions: Vec<Action>,
}

/// Optional per-seed orb telemetry. It stays local to one worker and is merged
/// after the run, so enabling it never introduces locks into gameplay.
#[derive(Clone, Debug, Default)]
struct OrbStats {
    channels: [usize; 4],
    evokes: [usize; 4],
    dark_evoke_damage: usize,
    dark_evoke_max: usize,
    plays: HashMap<CardId, usize>,
    events: HashMap<(EventId, EventOption), usize>,
}

impl OrbStats {
    fn merge(&mut self, other: &Self) {
        for i in 0..4 {
            self.channels[i] += other.channels[i];
            self.evokes[i] += other.evokes[i];
        }
        self.dark_evoke_damage += other.dark_evoke_damage;
        self.dark_evoke_max = self.dark_evoke_max.max(other.dark_evoke_max);
        for (&id, &count) in &other.plays {
            *self.plays.entry(id).or_insert(0) += count;
        }
        for (&event, &count) in &other.events {
            *self.events.entry(event).or_insert(0) += count;
        }
    }
}

fn orb_kind_index(kind: sts_engine::creature::OrbKind) -> usize {
    use sts_engine::creature::OrbKind;
    match kind {
        OrbKind::Lightning => 0,
        OrbKind::Frost => 1,
        OrbKind::Dark => 2,
        OrbKind::Plasma => 3,
    }
}

/// Align the orb rows before/after one executed step. Orbs channel by
/// appending on the right and evoke by leaving from the left, so the after
/// row is `before[j..] ++ channeled` for the smallest feasible j.
fn tally_orb_step(stats: &mut OrbStats, before: &[(usize, i32)], after: &[(usize, i32)]) {
    let mut split = None;
    for j in 0..=before.len() {
        let kept = &before[j..];
        if after.len() >= kept.len() && kept.iter().map(|o| o.0).eq(after[..kept.len()].iter().map(|o| o.0)) {
            split = Some(j);
            break;
        }
    }
    let Some(j) = split else { return };
    for &(kind, evoke) in &before[..j] {
        stats.evokes[kind] += 1;
        if kind == 2 {
            stats.dark_evoke_damage += evoke.max(0) as usize;
            stats.dark_evoke_max = stats.dark_evoke_max.max(evoke.max(0) as usize);
        }
    }
    for &(kind, _) in &after[before.len() - j..] {
        stats.channels[kind] += 1;
    }
}

fn print_orb_stats(stats: &OrbStats) {
    let names = ["lightning", "frost", "dark", "plasma"];
    let mut parts = Vec::new();
    for i in 0..4 {
        parts.push(format!(
            "{}={}ch/{}ev",
            names[i],
            stats.channels[i],
            stats.evokes[i]
        ));
    }
    println!(
        "orb_stats {} dark_evoke_damage={} dark_evoke_max={}",
        parts.join(" "),
        stats.dark_evoke_damage,
        stats.dark_evoke_max
    );
    let mut rows: Vec<_> = stats.plays.iter().collect();
    rows.sort_by_key(|(id, count)| {
        (
            std::cmp::Reverse(**count),
            sts_engine::generated::card_name_order::card_id_order(**id),
        )
    });
    let joined: Vec<String> = rows
        .iter()
        .map(|(id, n)| format!("{}={n}", id.sts_id()))
        .collect();
    println!("card_plays {}", joined.join(" "));
    let mut rows: Vec<_> = stats.events.iter().collect();
    rows.sort_by_key(|(key, _)| **key);
    for ((event, option), n) in rows {
        println!("event_choice\t{}|{option:?}\t{n}", event.sts_id());
    }
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
    boss_entry_hp: Vec<(Act, i32, i32)>,
    rested: usize,
    smithed: usize,
    recalled: usize,
    paths: [Vec<RoomType>; 4],
}

struct SeedDetail {
    seed: i64,
    outcome: Outcome,
    steps: usize,
    floor_achieved: i32,
    monsters_with_hp: Vec<(MonsterId, i32, i32)>,
    diagnostics: RunDiagnostics,
    search: SearchStats,
    telemetry: OrbStats,
    final_focus: i32,
    final_orbs: Option<Vec<(OrbKind, i32)>>,
    final_relics: Option<Vec<RelicId>>,
    final_deck: Option<Vec<(CardId, bool)>>,
}

impl SeedDetail {
    fn from_run(seed: i64, run: RunResult, include_diagnostics: bool) -> Self {
        let monsters_with_hp = run
            .game
            .combat
            .as_ref()
            .map(|combat| {
                combat
                    .monsters
                    .iter()
                    .filter(|monster| monster.hp > 0 && !monster.dead && !monster.escaped)
                    .map(|monster| (monster.id, monster.hp, monster.max_hp))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        Self {
            seed,
            outcome: run.outcome,
            steps: run.steps,
            floor_achieved: run.game.dungeon.floor,
            monsters_with_hp,
            diagnostics: run.diagnostics,
            search: run.search,
            telemetry: run.telemetry,
            final_focus: run.game.player.power_amount(PowerId::Focus),
            final_orbs: include_diagnostics.then(|| {
                run.game
                    .player
                    .orbs
                    .iter()
                    .map(|orb| (orb.kind, orb.evoke))
                    .collect()
            }),
            final_relics: include_diagnostics.then(|| {
                run.game
                    .player
                    .relics
                    .iter()
                    .map(|relic| relic.id)
                    .collect()
            }),
            final_deck: include_diagnostics.then(|| {
                run.game
                    .player
                    .deck
                    .iter()
                    .map(|card| (card.id, card.upgraded))
                    .collect()
            }),
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
    search: SearchStats,
    telemetry: OrbStats,
}

impl BatchStats {
    fn record(&mut self, detail: &SeedDetail) {
        self.steps += detail.steps;
        self.max_floor_achieved = self.max_floor_achieved.max(detail.floor_achieved);
        self.floor_achieved_sum += i64::from(detail.floor_achieved);
        self.search += detail.search;
        self.telemetry.merge(&detail.telemetry);
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
    collect_diagnostics: bool,
    collect_telemetry: bool,
) -> Vec<SeedDetail> {
    let count = seeds.len();
    let worker_count = concurrent.min(count);
    if worker_count == 1 {
        return seeds
            .iter()
            .map(|&seed| {
                let run = run_seed(
                    seed,
                    character,
                    ascension,
                    max_steps,
                    unlocks,
                    collect_diagnostics,
                    collect_telemetry,
                    false,
                );
                SeedDetail::from_run(seed, run, collect_diagnostics)
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
                    let run = run_seed(
                        seed,
                        character,
                        ascension,
                        max_steps,
                        unlocks,
                        collect_diagnostics,
                        collect_telemetry,
                        false,
                    );
                    local.push(SeedDetail::from_run(seed, run, collect_diagnostics));
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
    collect_diagnostics: bool,
    collect_telemetry: bool,
    collect_actions: bool,
) -> RunResult {
    let mut game = Game::new(seed, character, ascension, unlocks.clone());
    let mut agent = HtnAgent::new();
    let mut steps = 0usize;
    let mut diagnostics = RunDiagnostics::default();
    let mut search = SearchStats::default();
    let mut telemetry = OrbStats::default();
    let mut actions = Vec::new();

    while !game.done && game.player.hp > 0 && game.screen != Screen::Terminal && steps < max_steps {
        let (action, decision_search) = agent.decide_with_stats(&game);
        search += decision_search;
        if matches!(action, sts_engine::Action::Quit) {
            break;
        }
        let screen_before = game.screen;
        if collect_diagnostics && screen_before == Screen::Map {
            if let sts_engine::Action::Choose {
                room: Some(room), ..
            } = &action
            {
                let room = *room;
                match room {
                    RoomType::Monster => diagnostics.monsters += 1,
                    RoomType::Elite => diagnostics.elites += 1,
                    RoomType::Rest => diagnostics.rests += 1,
                    RoomType::Event => diagnostics.events += 1,
                    RoomType::Shop => diagnostics.shops += 1,
                    RoomType::Treasure | RoomType::BossTreasure => diagnostics.treasures += 1,
                    RoomType::Boss => {
                        diagnostics.bosses += 1;
                        diagnostics.boss_entry_hp.push((
                            game.dungeon.act,
                            game.player.hp,
                            game.player.max_hp,
                        ));
                    }
                    _ => {}
                }
                let act_index = (game.dungeon.act as usize).saturating_sub(1).min(3);
                diagnostics.paths[act_index].push(room);
            }
        } else if collect_diagnostics && screen_before == Screen::Rest {
            if let sts_engine::Action::Choose { index, .. } = &action {
                match game.campfire_options().get(*index) {
                    Some(CampfireOption::Rest) => diagnostics.rested += 1,
                    Some(CampfireOption::Smith) => diagnostics.smithed += 1,
                    Some(CampfireOption::Recall) => diagnostics.recalled += 1,
                    _ => {}
                }
            }
        }
        if collect_telemetry && screen_before == Screen::Event {
            if let (Some(event), sts_engine::Action::Choose { index, .. }) =
                (game.event.as_ref(), &action)
            {
                if let Some(&option) = event.options.get(*index) {
                    *telemetry.events.entry((event.id, option)).or_insert(0) += 1;
                }
            }
        }
        let orbs_before: Option<Vec<(usize, i32)>> = if collect_telemetry
            && screen_before == Screen::Combat
        {
            if let sts_engine::Action::Play { hand_index, .. } = &action {
                if let Some(card) = game.player.hand.get(*hand_index) {
                    *telemetry.plays.entry(card.id).or_insert(0) += 1;
                }
            }
            Some(
                game.player
                    .orbs
                    .iter()
                    .map(|o| (orb_kind_index(o.kind), o.evoke))
                    .collect(),
            )
        } else {
            None
        };
        game.step(&action);
        if let Some(before) = orbs_before {
            let after: Vec<(usize, i32)> = game
                .player
                .orbs
                .iter()
                .map(|o| (orb_kind_index(o.kind), o.evoke))
                .collect();
            tally_orb_step(&mut telemetry, &before, &after);
        }
        if collect_actions {
            actions.push(action);
        }
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
        search,
        telemetry,
        actions,
    }
}

fn print_single(
    seed: i64,
    character: Character,
    ascension: i32,
    run: &RunResult,
    telemetry: bool,
) {
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
            event.id.sts_id(), event.screen, event.options
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
    println!(
        "search plan_calls={} emulation_cycles={} emulation_cycles/decision={:.1} expanded_nodes={} score_evaluations={} lethal_expansions={} dedup_hits={}",
        run.search.plan_calls,
        run.search.simulated_steps,
        run.search.simulated_steps as f64 / run.steps.max(1) as f64,
        run.search.expanded_nodes,
        run.search.score_evaluations,
        run.search.lethal_expansions,
        run.search.dedup_hits,
    );
    if telemetry {
        print_orb_stats(&run.telemetry);
    }
}

fn format_remaining_monsters(monsters: &[(MonsterId, i32, i32)]) -> String {
    if monsters.is_empty() {
        return "-".into();
    }
    monsters
        .iter()
        .map(|(id, hp, max_hp)| format!("{id:?}={hp}/{max_hp}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_boss_entries(entries: &[(Act, i32, i32)]) -> String {
    entries
        .iter()
        .map(|(act, hp, max_hp)| format!("{}:{hp}/{max_hp}", *act as i32))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_path(path: &[RoomType]) -> String {
    path.iter()
        .map(|room| match room {
            RoomType::Monster => 'M',
            RoomType::Elite => 'E',
            RoomType::Rest => 'R',
            RoomType::Event => '?',
            RoomType::Shop => '$',
            RoomType::Treasure | RoomType::BossTreasure => 'T',
            RoomType::Boss => 'B',
            _ => '-',
        })
        .collect()
}

fn format_final_orbs(orbs: Option<&[(OrbKind, i32)]>) -> String {
    orbs.unwrap_or_default()
        .iter()
        .map(|(kind, evoke)| format!("{kind:?}:{evoke}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn format_final_relics(relics: Option<&[RelicId]>) -> String {
    relics
        .unwrap_or_default()
        .iter()
        .map(|id| id.sts_id())
        .collect::<Vec<_>>()
        .join(",")
}

fn format_final_deck(deck: Option<&[(CardId, bool)]>) -> String {
    deck.unwrap_or_default()
        .iter()
        .map(|(id, upgraded)| format!("{}{}", id.sts_id(), if *upgraded { "+" } else { "" }))
        .collect::<Vec<_>>()
        .join(",")
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
    telemetry: bool,
) {
    let count = seeds.len();
    let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
    let seeds_per_second = count as f64 / seconds;
    let steps_per_second = stats.steps as f64 / seconds;
    let simulations_per_second = stats.search.simulated_steps as f64 / seconds;
    let simulations_per_decision = stats.search.simulated_steps as f64 / stats.steps.max(1) as f64;
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
        "character={:?} asc={} seeds={} concurrent={} {} wins={} losses={} capped={} stopped={} win_rate={:.2}% max_floor_achieved={} mean_floor_achieved={:.2} decisions={} max_steps={} elapsed={:.6}s seeds/s={:.1} decisions/s={:.0} emulation_cycles={} emulation_cycles/s={:.0} emulation_cycles/decision={:.1} plan_calls={} expanded_nodes={} score_evaluations={} lethal_expansions={} dedup_hits={}",
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
        stats.search.simulated_steps,
        simulations_per_second,
        simulations_per_decision,
        stats.search.plan_calls,
        stats.search.expanded_nodes,
        stats.search.score_evaluations,
        stats.search.lethal_expansions,
        stats.search.dedup_hits,
    );
    if telemetry {
        print_orb_stats(&stats.telemetry);
    }
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
                format_remaining_monsters(&detail.monsters_with_hp),
                detail.diagnostics.monsters,
                detail.diagnostics.elites,
                detail.diagnostics.rests,
                detail.diagnostics.events,
                detail.diagnostics.shops,
                detail.diagnostics.treasures,
                detail.diagnostics.bosses,
                format_boss_entries(&detail.diagnostics.boss_entry_hp),
                detail.diagnostics.rested,
                detail.diagnostics.smithed,
                detail.diagnostics.recalled,
                format_path(&detail.diagnostics.paths[0]),
                format_path(&detail.diagnostics.paths[1]),
                format_path(&detail.diagnostics.paths[2]),
                format_path(&detail.diagnostics.paths[3]),
                detail.final_focus,
                format_final_orbs(detail.final_orbs.as_deref()),
                format_final_relics(detail.final_relics.as_deref()),
                format_final_deck(detail.final_deck.as_deref()),
            );
        } else {
            println!(
                "{}\t{}\t{}\t{}",
                detail.seed,
                detail.outcome.label(),
                detail.floor_achieved,
                format_remaining_monsters(&detail.monsters_with_hp)
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
    let mut telemetry = false;
    let mut fixture_jsonl: Option<PathBuf> = None;
    let mut compare_jsonl: Option<PathBuf> = None;
    let mut actions_jsonl: Option<PathBuf> = None;
    let mut replay_actions_jsonl: Option<PathBuf> = None;
    let mut randomize = false;
    let mut random_source: Option<i64> = None;
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--character" | "-c" => {
                let v = args.next().unwrap_or_else(|| "DEFECT".into());
                character = Character::from_cli(&v).unwrap_or(Character::Defect);
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
            "--telemetry" => telemetry = true,
            "--fixture-jsonl" => fixture_jsonl = args.next().map(PathBuf::from),
            "--compare-jsonl" => compare_jsonl = args.next().map(PathBuf::from),
            "--actions-jsonl" => actions_jsonl = args.next().map(PathBuf::from),
            "--replay-actions-jsonl" => replay_actions_jsonl = args.next().map(PathBuf::from),
            "--random-seeds" => randomize = true,
            "--seed-source" => {
                random_source = args.next().and_then(|s| s.parse().ok());
                randomize = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: sts-htn [--character CHARACTER] [--seed FIRST_SEED] [--count N] [--concurrent N] [--random-seeds] [--seed-source N] [--ascension 0|20] [--max-steps N] [--diagnostics] [--telemetry] [--fixture-jsonl PATH | --compare-jsonl PATH | --actions-jsonl PATH] [--replay-actions-jsonl PATH]\n\nBatch mode runs seeds in one process and prints aggregate decision and HTN-emulation throughput, win rate, and per-seed results. --telemetry opts into orb/card/event telemetry; --diagnostics adds per-seed policy diagnostics. --fixture-jsonl writes one compact final engine state per seed; --compare-jsonl reruns the cohort and requires exact equality. --actions-jsonl optionally accumulates only each seed's actions in memory and writes them once after the batch. --replay-actions-jsonl bypasses HTN and replays that action log; combine it with --compare-jsonl for an engine-only exact gate. --random-seeds generates a fresh cohort; --seed-source makes that cohort reproducible. Runs cap at 5000 steps by default; any capped seed should be treated as a loop bug."
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
    let write_modes = usize::from(fixture_jsonl.is_some()) + usize::from(actions_jsonl.is_some());
    if write_modes > 1
        || (compare_jsonl.is_some() && write_modes > 0)
        || (replay_actions_jsonl.is_some() && write_modes > 0)
    {
        eprintln!("choose only one of --fixture-jsonl, --compare-jsonl, and --actions-jsonl");
        std::process::exit(2);
    }

    // Load the profile-backed unlock data once, then clone the in-memory value
    // into each fresh game. No assets or profile files are reloaded per seed.
    let unlocks = Unlocks::fixture();
    if let Some(actions_path) = replay_actions_jsonl {
        let logs = match load_action_log(&actions_path) {
            Ok(logs) if !logs.is_empty() => logs,
            Ok(_) => {
                eprintln!("{} contains no action logs", actions_path.display());
                std::process::exit(2);
            }
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(2);
            }
        };
        let start = Instant::now();
        let states = replay_action_batch(&logs, concurrent, character, ascension, &unlocks);
        let elapsed = start.elapsed();
        if let Some(path) = compare_jsonl {
            if let Err(message) = compare_fixture(&path, &states) {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        let steps: usize = states.iter().map(|state| state.steps).sum();
        eprintln!(
            "replayed {} seeds / {} actions in {:.3}s ({:.0} actions/s)",
            states.len(),
            steps,
            elapsed.as_secs_f64(),
            steps as f64 / elapsed.as_secs_f64().max(f64::EPSILON)
        );
        return;
    }
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
    if let Some(path) = actions_jsonl {
        let start = Instant::now();
        let logs = run_action_batch(
            &seeds, concurrent, character, ascension, max_steps, &unlocks,
        );
        match write_action_log(&path, &logs) {
            Ok(()) => eprintln!(
                "wrote {} action logs to {} in {:.3}s",
                logs.len(),
                path.display(),
                start.elapsed().as_secs_f64()
            ),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }
    if fixture_jsonl.is_some() || compare_jsonl.is_some() {
        let start = Instant::now();
        let states = run_final_batch(
            &seeds, concurrent, character, ascension, max_steps, &unlocks,
        );
        let result = if let Some(path) = fixture_jsonl {
            write_fixture(&path, &states)
                .map(|_| format!("wrote {} states to {}", states.len(), path.display()))
        } else {
            let path = compare_jsonl.unwrap();
            compare_fixture(&path, &states)
                .map(|_| format!("matched {} states in {}", states.len(), path.display()))
        };
        match result {
            Ok(message) => eprintln!("{} in {:.3}s", message, start.elapsed().as_secs_f64()),
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
        return;
    }
    if count == 1 {
        let actual_seed = seeds[0];
        let run = run_seed(
            actual_seed,
            character,
            ascension,
            max_steps,
            &unlocks,
            diagnostics,
            telemetry,
            false,
        );
        print_single(actual_seed, character, ascension, &run, telemetry);
        return;
    }

    let start = Instant::now();
    let details = run_batch(
        &seeds,
        concurrent,
        character,
        ascension,
        max_steps,
        &unlocks,
        diagnostics,
        telemetry,
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
        telemetry,
    );
}
