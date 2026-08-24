//! Run the HTN autoplayer on the rust engine.
//!
//! ```sh
//! cargo run --release --bin sts-htn -- --character DEFECT --seed 7 --ascension 0
//! cargo run --release --bin sts-htn -- --seed 0 --count 100 --concurrent 6 --a0
//! ```

#[path = "htn/learned_deck.rs"]
mod learned_deck;

use learned_deck::LearnedDeckPolicy;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Character, PowerId, RoomType};
use sts_engine::rng::StsRandom;
use sts_engine::{Action, Unlocks};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeckPolicyMode {
    Learned,
    Htn,
}

impl DeckPolicyMode {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "rl" | "rled" | "learned" | "weights" => Some(Self::Learned),
            "htn" | "pure-htn" | "pure_htn" => Some(Self::Htn),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum Outcome {
    Win,
    Loss,
    Capped,
    Stopped,
}

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
    learned_policy: Option<&LearnedDeckPolicy>,
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
                            seed,
                            character,
                            ascension,
                            max_steps,
                            unlocks,
                            learned_policy,
                            false,
                            false,
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
    learned_policy: Option<&LearnedDeckPolicy>,
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
                        seed,
                        character,
                        ascension,
                        max_steps,
                        unlocks,
                        learned_policy,
                        false,
                        true,
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

fn replay_action_prefix(
    log: &ActionLog,
    character: Character,
    ascension: i32,
    unlocks: &Unlocks,
) -> Game {
    let mut game = Game::new(log.seed, character, ascension, unlocks.clone());
    for action in &log.actions {
        game.step(action);
    }
    game
}

fn at_a20_second_boss_start(game: &Game, ascension: i32) -> bool {
    ascension == 20
        && game.screen == Screen::Combat
        && game.current_room == RoomType::Boss
        && game.dungeon.act as i32 == 3
        && game.dungeon.floor >= 51
        && game.combat.is_some()
        && game.player.hp > 0
}

fn second_boss_prefixes(
    logs: &[ActionLog],
    character: Character,
    ascension: i32,
    unlocks: &Unlocks,
) -> Vec<ActionLog> {
    let mut prefixes = Vec::new();
    for log in logs {
        let mut game = Game::new(log.seed, character, ascension, unlocks.clone());
        for (index, action) in log.actions.iter().enumerate() {
            game.step(action);
            if at_a20_second_boss_start(&game, ascension) {
                prefixes.push(ActionLog {
                    seed: log.seed,
                    outcome: None,
                    actions: log.actions[..=index].to_vec(),
                });
                break;
            }
        }
    }
    prefixes
}

fn run_boss_gauntlet(
    log: &ActionLog,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> RunResult {
    let mut game = replay_action_prefix(log, character, ascension, unlocks);
    let mut agent = HtnAgent::new();
    let mut steps = 0usize;
    let mut diagnostics = RunDiagnostics::default();
    diagnostics.a20_second_boss_entries = 1;
    diagnostics.a20_second_boss_entry_hp_fraction =
        f64::from(game.player.hp) / f64::from(game.player.max_hp.max(1));
    let mut combat_progress = None;
    let mut combat_stalemate = false;
    let mut cleared = false;

    while game.combat.is_some()
        && game.player.hp > 0
        && game.screen != Screen::Terminal
        && steps < max_steps
    {
        if combat_has_stalled(&game, &mut combat_progress) {
            combat_stalemate = true;
            diagnostics.combat_stalemates = 1;
            break;
        }
        let action = agent.decide(&game);
        if matches!(action, Action::Quit) {
            break;
        }
        let screen_before = game.screen;
        game.step(&action);
        steps += 1;
        if screen_before == Screen::Combat
            && game.screen == Screen::CombatReward
            && game.current_room == RoomType::Boss
            && game.dungeon.act as i32 == 3
            && game.dungeon.floor >= 51
            && game.player.hp > 0
        {
            diagnostics.a20_second_boss_clears = 1;
            cleared = true;
            break;
        }
    }

    let outcome = if cleared {
        Outcome::Win
    } else {
        completed_outcome(&game).unwrap_or_else(|| {
            if combat_stalemate {
                Outcome::Loss
            } else if steps >= max_steps {
                Outcome::Capped
            } else {
                Outcome::Stopped
            }
        })
    };
    record_late_boss_failure(&game, &mut diagnostics, outcome);
    RunResult {
        game,
        steps,
        outcome,
        diagnostics,
        actions: Vec::new(),
    }
}

fn run_boss_gauntlet_batch(
    logs: &[ActionLog],
    concurrent: usize,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
) -> Vec<SeedDetail> {
    let next_offset = AtomicUsize::new(0);
    let mut details = thread::scope(|scope| {
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
                    let run = run_boss_gauntlet(log, character, ascension, max_steps, unlocks);
                    local.push(SeedDetail::from_run(log.seed, &run));
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
                    let outcome = completed_outcome(&game)
                        .unwrap_or_else(|| log.outcome.unwrap_or(Outcome::Stopped));
                    let run = RunResult {
                        game,
                        steps,
                        outcome,
                        diagnostics: RunDiagnostics::default(),
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

fn completed_outcome(game: &Game) -> Option<Outcome> {
    if game.done && game.player.hp > 0 {
        Some(Outcome::Win)
    } else if game.player.hp <= 0 {
        Some(Outcome::Loss)
    } else {
        None
    }
}

struct RunResult {
    game: Game,
    steps: usize,
    outcome: Outcome,
    diagnostics: RunDiagnostics,
    actions: Vec<Action>,
}

/// Batch-wide orb telemetry, tallied only from executed actions (never from
/// HTN search clones). Indexed by Lightning, Frost, Dark, Plasma.
#[derive(Default)]
struct OrbStats {
    channels: [AtomicUsize; 4],
    evokes: [AtomicUsize; 4],
    dark_evoke_damage: AtomicUsize,
    dark_evoke_max: AtomicUsize,
    plays: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<&'static str, usize>>>,
    events: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, usize>>>,
}

static ORB_STATS: OrbStats = OrbStats {
    channels: [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
    evokes: [
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
        AtomicUsize::new(0),
    ],
    dark_evoke_damage: AtomicUsize::new(0),
    dark_evoke_max: AtomicUsize::new(0),
    plays: std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new())),
    events: std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new())),
};

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
fn tally_orb_step(before: &[(usize, i32)], after: &[(usize, i32)]) {
    let mut split = None;
    for j in 0..=before.len() {
        let kept = &before[j..];
        if after.len() >= kept.len()
            && kept
                .iter()
                .map(|o| o.0)
                .eq(after[..kept.len()].iter().map(|o| o.0))
        {
            split = Some(j);
            break;
        }
    }
    let Some(j) = split else { return };
    for &(kind, evoke) in &before[..j] {
        ORB_STATS.evokes[kind].fetch_add(1, Ordering::Relaxed);
        if kind == 2 {
            ORB_STATS
                .dark_evoke_damage
                .fetch_add(evoke.max(0) as usize, Ordering::Relaxed);
            ORB_STATS
                .dark_evoke_max
                .fetch_max(evoke.max(0) as usize, Ordering::Relaxed);
        }
    }
    for &(kind, _) in &after[before.len() - j..] {
        ORB_STATS.channels[kind].fetch_add(1, Ordering::Relaxed);
    }
}

fn print_orb_stats() {
    let names = ["lightning", "frost", "dark", "plasma"];
    let mut parts = Vec::new();
    for i in 0..4 {
        parts.push(format!(
            "{}={}ch/{}ev",
            names[i],
            ORB_STATS.channels[i].load(Ordering::Relaxed),
            ORB_STATS.evokes[i].load(Ordering::Relaxed)
        ));
    }
    println!(
        "orb_stats {} dark_evoke_damage={} dark_evoke_max={}",
        parts.join(" "),
        ORB_STATS.dark_evoke_damage.load(Ordering::Relaxed),
        ORB_STATS.dark_evoke_max.load(Ordering::Relaxed)
    );
    let plays = ORB_STATS.plays.lock().unwrap();
    let mut rows: Vec<_> = plays.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1));
    let joined: Vec<String> = rows.iter().map(|(id, n)| format!("{id}={n}")).collect();
    println!("card_plays {}", joined.join(" "));
    let events = ORB_STATS.events.lock().unwrap();
    let mut rows: Vec<_> = events.iter().collect();
    rows.sort();
    for (key, n) in rows {
        println!("event_choice\t{key}\t{n}");
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
    boss_entry_hp: Vec<String>,
    a20_second_boss_entries: usize,
    a20_second_boss_entry_hp_fraction: f64,
    a20_second_boss_clears: usize,
    final_boss_entries: usize,
    final_boss_entry_hp_fraction: f64,
    last_boss_fights: usize,
    last_boss_remaining_hp: i64,
    last_boss_damage_fraction: f64,
    combat_stalemates: usize,
    rested: usize,
    smithed: usize,
    recalled: usize,
    paths: [String; 4],
}

struct SeedDetail {
    seed: i64,
    outcome: Outcome,
    steps: usize,
    act_achieved: i32,
    floor_achieved: i32,
    player_died: bool,
    death_room: RoomType,
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
            act_achieved: run.game.dungeon.act as i32,
            floor_achieved: run.game.dungeon.floor,
            player_died: run.game.player.hp <= 0,
            death_room: run.game.current_room,
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
    a20_second_boss_entries: usize,
    a20_second_boss_entry_hp_fraction_sum: f64,
    a20_second_boss_clears: usize,
    final_boss_entries: usize,
    final_boss_entry_hp_fraction_sum: f64,
    last_boss_fights: usize,
    last_boss_remaining_hp_sum: i64,
    last_boss_damage_fraction_sum: f64,
    combat_stalemates: usize,
    deaths_by_act: [usize; 4],
    deaths_by_act_and_room: [[usize; 4]; 4],
}

impl BatchStats {
    fn record(&mut self, detail: &SeedDetail) {
        self.steps += detail.steps;
        self.max_floor_achieved = self.max_floor_achieved.max(detail.floor_achieved);
        self.floor_achieved_sum += i64::from(detail.floor_achieved);
        self.a20_second_boss_entries += detail.diagnostics.a20_second_boss_entries;
        self.a20_second_boss_entry_hp_fraction_sum +=
            detail.diagnostics.a20_second_boss_entry_hp_fraction;
        self.a20_second_boss_clears += detail.diagnostics.a20_second_boss_clears;
        self.final_boss_entries += detail.diagnostics.final_boss_entries;
        self.final_boss_entry_hp_fraction_sum += detail.diagnostics.final_boss_entry_hp_fraction;
        self.last_boss_fights += detail.diagnostics.last_boss_fights;
        self.last_boss_remaining_hp_sum += detail.diagnostics.last_boss_remaining_hp;
        self.last_boss_damage_fraction_sum += detail.diagnostics.last_boss_damage_fraction;
        self.combat_stalemates += detail.diagnostics.combat_stalemates;
        if let Some(index) =
            death_act_index(detail.outcome, detail.player_died, detail.act_achieved)
        {
            self.deaths_by_act[index] += 1;
            self.deaths_by_act_and_room[index][death_room_index(detail.death_room)] += 1;
        }
        match detail.outcome {
            Outcome::Win => self.wins += 1,
            Outcome::Loss => self.losses += 1,
            Outcome::Capped => self.capped += 1,
            Outcome::Stopped => self.stopped += 1,
        }
    }
}

fn death_act_index(outcome: Outcome, player_died: bool, act: i32) -> Option<usize> {
    if outcome != Outcome::Loss || !player_died || !(1..=4).contains(&act) {
        return None;
    }
    Some((act - 1) as usize)
}

fn death_room_index(room: RoomType) -> usize {
    match room {
        RoomType::Monster => 0,
        RoomType::Elite => 1,
        RoomType::Boss => 2,
        _ => 3,
    }
}

fn print_death_layer(label: &str, total: usize, rooms: [usize; 4]) {
    let [normal, elite, boss, other] = rooms;
    if other == 0 {
        println!("{label}: {total} deaths (normal={normal} elite={elite} boss={boss})");
    } else {
        println!(
            "{label}: {total} deaths (normal={normal} elite={elite} boss={boss} other={other})"
        );
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
    learned_policy: Option<&LearnedDeckPolicy>,
    collect_diagnostics: bool,
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
                    learned_policy,
                    collect_diagnostics,
                    false,
                );
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
                    let run = run_seed(
                        seed,
                        character,
                        ascension,
                        max_steps,
                        unlocks,
                        learned_policy,
                        collect_diagnostics,
                        false,
                    );
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

const MAX_TURNS_WITHOUT_ENEMY_HP_PROGRESS: i32 = 60;

fn combat_has_stalled(game: &Game, progress: &mut Option<(i32, i32)>) -> bool {
    let Some(combat) = &game.combat else {
        *progress = None;
        return false;
    };
    let remaining_hp: i32 = combat
        .monsters
        .iter()
        .filter(|monster| monster.alive() && !monster.half_dead)
        .map(|monster| monster.hp.max(0))
        .sum();
    match *progress {
        Some((_last_progress_turn, best_remaining_hp)) if remaining_hp < best_remaining_hp => {
            *progress = Some((combat.turn, remaining_hp));
            false
        }
        Some((last_progress_turn, _)) => {
            combat.turn.saturating_sub(last_progress_turn) >= MAX_TURNS_WITHOUT_ENEMY_HP_PROGRESS
        }
        None => {
            *progress = Some((combat.turn, remaining_hp));
            false
        }
    }
}

fn record_late_boss_failure(game: &Game, diagnostics: &mut RunDiagnostics, outcome: Outcome) {
    let is_late_boss = game.current_room == RoomType::Boss
        && ((game.dungeon.act as i32 == 3 && game.dungeon.floor >= 51)
            || game.dungeon.act as i32 == 4);
    if outcome == Outcome::Win || !is_late_boss {
        return;
    }
    if let Some(combat) = &game.combat {
        let remaining_hp: i64 = combat
            .monsters
            .iter()
            .filter(|monster| monster.hp > 0 && !monster.dead && !monster.escaped)
            .map(|monster| i64::from(monster.hp))
            .sum();
        let total_max_hp: i64 = combat
            .monsters
            .iter()
            .filter(|monster| !monster.escaped)
            .map(|monster| i64::from(monster.max_hp.max(0)))
            .sum();
        diagnostics.last_boss_fights = 1;
        diagnostics.last_boss_remaining_hp = remaining_hp;
        diagnostics.last_boss_damage_fraction = if total_max_hp > 0 {
            (1.0 - remaining_hp as f64 / total_max_hp as f64).clamp(0.0, 1.0)
        } else {
            0.0
        };
    }
}

fn run_seed(
    seed: i64,
    character: Character,
    ascension: i32,
    max_steps: usize,
    unlocks: &Unlocks,
    learned_policy: Option<&LearnedDeckPolicy>,
    collect_diagnostics: bool,
    collect_actions: bool,
) -> RunResult {
    let mut game = Game::new(seed, character, ascension, unlocks.clone());
    let mut agent = HtnAgent::new();
    let mut learned_run = learned_policy.map(LearnedDeckPolicy::start_run);
    let mut steps = 0usize;
    let mut diagnostics = RunDiagnostics::default();
    let mut actions = Vec::new();
    let mut combat_progress: Option<(i32, i32)> = None;
    let mut combat_stalemate = false;

    while !game.done && game.player.hp > 0 && game.screen != Screen::Terminal && steps < max_steps {
        if combat_has_stalled(&game, &mut combat_progress) {
            combat_stalemate = true;
            diagnostics.combat_stalemates = 1;
            break;
        }
        // Always advance the HTN agent's internal bookkeeping. On supported
        // deck-building screens the learned policy replaces only its proposed
        // action; combat, routing, events, and unsupported grids stay pure HTN.
        let htn_action = agent.decide(&game);
        let action = learned_run
            .as_mut()
            .and_then(|policy| policy.decide(&game))
            .unwrap_or(htn_action);
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
                if room == RoomType::Boss {
                    diagnostics.bosses += 1;
                    diagnostics.boss_entry_hp.push(format!(
                        "{}:{}/{}",
                        game.dungeon.act as i32, game.player.hp, game.player.max_hp
                    ));
                    if game.dungeon.act as i32 == 4 {
                        diagnostics.final_boss_entries += 1;
                        diagnostics.final_boss_entry_hp_fraction +=
                            f64::from(game.player.hp) / f64::from(game.player.max_hp.max(1));
                    }
                }
                if collect_diagnostics {
                    match room {
                        RoomType::Monster => diagnostics.monsters += 1,
                        RoomType::Elite => diagnostics.elites += 1,
                        RoomType::Rest => diagnostics.rests += 1,
                        RoomType::Event => diagnostics.events += 1,
                        RoomType::Shop => diagnostics.shops += 1,
                        RoomType::Treasure | RoomType::BossTreasure => diagnostics.treasures += 1,
                        RoomType::Boss => {}
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
            }
        } else if collect_diagnostics && screen_before == Screen::Rest {
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
        if screen_before == Screen::Event {
            if let (Some(event), sts_engine::Action::Choose { label, index, .. }) =
                (game.event.as_ref(), &action)
            {
                let choice = label.clone().unwrap_or_else(|| format!("#{index}"));
                let mut key = format!("{}|{}", event.id, choice);
                key.truncate(80);
                *ORB_STATS.events.lock().unwrap().entry(key).or_insert(0) += 1;
            }
        }
        let orbs_before: Option<Vec<(usize, i32)>> = if screen_before == Screen::Combat {
            if let sts_engine::Action::Play { hand_index, .. } = &action {
                if let Some(card) = game.player.hand.get(*hand_index) {
                    *ORB_STATS
                        .plays
                        .lock()
                        .unwrap()
                        .entry(card.sts_id())
                        .or_insert(0) += 1;
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
        if ascension == 20
            && screen_before == Screen::CombatReward
            && game.screen == Screen::Combat
            && game.current_room == RoomType::Boss
            && game.dungeon.act as i32 == 3
            && game.dungeon.floor >= 51
        {
            diagnostics.a20_second_boss_entries += 1;
            diagnostics.a20_second_boss_entry_hp_fraction +=
                f64::from(game.player.hp) / f64::from(game.player.max_hp.max(1));
        }
        if ascension == 20
            && screen_before == Screen::Combat
            && game.screen == Screen::CombatReward
            && game.current_room == RoomType::Boss
            && game.dungeon.act as i32 == 3
            && game.dungeon.floor >= 51
            && game.player.hp > 0
        {
            diagnostics.a20_second_boss_clears += 1;
        }
        if let Some(before) = orbs_before {
            let after: Vec<(usize, i32)> = game
                .player
                .orbs
                .iter()
                .map(|o| (orb_kind_index(o.kind), o.evoke))
                .collect();
            tally_orb_step(&before, &after);
        }
        if collect_actions {
            actions.push(action);
        }
        steps += 1;
    }

    let outcome = completed_outcome(&game).unwrap_or_else(|| {
        if combat_stalemate {
            Outcome::Loss
        } else if steps >= max_steps {
            Outcome::Capped
        } else {
            Outcome::Stopped
        }
    });
    record_late_boss_failure(&game, &mut diagnostics, outcome);

    RunResult {
        game,
        steps,
        outcome,
        diagnostics,
        actions,
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
    let mean_a20_second_boss_entry_hp_fraction =
        stats.a20_second_boss_entry_hp_fraction_sum / count as f64;
    let mean_final_boss_entry_hp_fraction = stats.final_boss_entry_hp_fraction_sum / count as f64;
    let mean_last_boss_damage_fraction = stats.last_boss_damage_fraction_sum / count as f64;
    let cohort = if let Some(source) = random_source {
        format!("cohort=random seed_source={source}")
    } else {
        format!(
            "cohort=consecutive range={}..={}",
            seeds.first().copied().unwrap_or(0),
            seeds.last().copied().unwrap_or(0)
        )
    };
    println!("WR: {:.2}% ({}/{})", win_rate, stats.wins, count);
    println!("mean_floor_achieved = {:.2}", mean_floor_achieved);
    print_death_layer(
        "act 1",
        stats.deaths_by_act[0],
        stats.deaths_by_act_and_room[0],
    );
    print_death_layer(
        "act 2",
        stats.deaths_by_act[1],
        stats.deaths_by_act_and_room[1],
    );
    print_death_layer(
        "act 3",
        stats.deaths_by_act[2],
        stats.deaths_by_act_and_room[2],
    );
    print_death_layer(
        "heart",
        stats.deaths_by_act[3],
        stats.deaths_by_act_and_room[3],
    );
    println!(
        "character={:?} asc={} seeds={} concurrent={} {} wins={} losses={} capped={} stopped={} combat_stalemates={} win_rate={:.2}% max_floor_achieved={} mean_floor_achieved={:.2} a20_second_boss_entries={} mean_a20_second_boss_entry_hp_fraction={:.4} a20_second_boss_clears={} final_boss_entries={} mean_final_boss_entry_hp_fraction={:.4} last_boss_fights={} last_boss_remaining_hp_sum={} mean_last_boss_damage_fraction={:.4} steps={} max_steps={} elapsed={:.6}s seeds/s={:.1} steps/s={:.0}",
        character,
        ascension,
        count,
        concurrent.min(count),
        cohort,
        stats.wins,
        stats.losses,
        stats.capped,
        stats.stopped,
        stats.combat_stalemates,
        win_rate,
        stats.max_floor_achieved,
        mean_floor_achieved,
        stats.a20_second_boss_entries,
        mean_a20_second_boss_entry_hp_fraction,
        stats.a20_second_boss_clears,
        stats.final_boss_entries,
        mean_final_boss_entry_hp_fraction,
        stats.last_boss_fights,
        stats.last_boss_remaining_hp_sum,
        mean_last_boss_damage_fraction,
        stats.steps,
        max_steps,
        seconds,
        seeds_per_second,
        steps_per_second,
    );
    if !diagnostics {
        return;
    }
    print_orb_stats();
    println!("seed\toutcome\tfloor_achieved\tmonsters_with_hp_remaining\tnormals\telites\trests\tevents\tshops\ttreasures\tbosses\tboss_entry_hp\trested\tsmithed\trecalled\tact1_path\tact2_path\tact3_path\tact4_path\tfinal_focus\tfinal_orbs\tfinal_relics\tfinal_deck");
    for detail in details {
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
    let mut fixture_jsonl: Option<PathBuf> = None;
    let mut compare_jsonl: Option<PathBuf> = None;
    let mut actions_jsonl: Option<PathBuf> = None;
    let mut replay_actions_jsonl: Option<PathBuf> = None;
    let mut boss_prefix_jsonl: Option<PathBuf> = None;
    let mut boss_gauntlet_jsonl: Option<PathBuf> = None;
    let mut deck_policy_mode = DeckPolicyMode::Learned;
    let mut deck_policy_path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/draft_policy_synergy_a20.json");
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
            "--fixture-jsonl" => fixture_jsonl = args.next().map(PathBuf::from),
            "--compare-jsonl" => compare_jsonl = args.next().map(PathBuf::from),
            "--actions-jsonl" => actions_jsonl = args.next().map(PathBuf::from),
            "--replay-actions-jsonl" => replay_actions_jsonl = args.next().map(PathBuf::from),
            "--boss-prefix-jsonl" => boss_prefix_jsonl = args.next().map(PathBuf::from),
            "--boss-gauntlet-jsonl" => boss_gauntlet_jsonl = args.next().map(PathBuf::from),
            "--deck-policy" => {
                let Some(value) = args.next() else {
                    eprintln!("--deck-policy requires rl or htn");
                    std::process::exit(2);
                };
                let Some(mode) = DeckPolicyMode::parse(&value) else {
                    eprintln!("invalid --deck-policy {value:?}; expected rl or htn");
                    std::process::exit(2);
                };
                deck_policy_mode = mode;
            }
            "--pure-htn" => deck_policy_mode = DeckPolicyMode::Htn,
            "--deck-policy-path" => {
                let Some(path) = args.next() else {
                    eprintln!("--deck-policy-path requires a checkpoint path");
                    std::process::exit(2);
                };
                deck_policy_path = PathBuf::from(path);
            }
            "--random-seeds" => randomize = true,
            "--seed-source" => {
                random_source = args.next().and_then(|s| s.parse().ok());
                randomize = true;
            }
            "--help" | "-h" => {
                println!(
                    "Usage: sts-htn [--character CHARACTER] [--seed FIRST_SEED] [--count N] [--concurrent N] [--random-seeds] [--seed-source N] [--ascension 0|20] [--max-steps N] [--diagnostics] [--deck-policy rl|htn] [--pure-htn] [--deck-policy-path PATH] [--fixture-jsonl PATH | --compare-jsonl PATH | --actions-jsonl PATH | --boss-prefix-jsonl PATH] [--replay-actions-jsonl PATH | --boss-gauntlet-jsonl PATH]\n\nThe default --deck-policy rl uses the learned checkpoint for deck-building decisions and HTN for fights, routing, events, and unsupported selections. --deck-policy htn (or --pure-htn) uses HTN for every decision. --deck-policy-path selects another learned checkpoint. Batch mode runs seeds in one process and prints aggregate throughput, win rate, and per-seed results. --fixture-jsonl writes one compact final engine state per seed; --compare-jsonl reruns the cohort and requires exact equality. --actions-jsonl writes complete policy action logs. --boss-prefix-jsonl writes replayable prefixes ending at exact A20 second-boss entry. --boss-gauntlet-jsonl replays those prefixes and evaluates only the second-boss fight, reporting a gauntlet clear as a win. --replay-actions-jsonl bypasses policy selection and replays a complete action log; combine it with --compare-jsonl for an engine-only exact gate. --random-seeds generates a fresh cohort; --seed-source makes that cohort reproducible. Runs cap at 5000 steps by default; long combats with 60 turns of no enemy HP progress are scored as stalemate losses."
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
    let write_modes = usize::from(fixture_jsonl.is_some())
        + usize::from(actions_jsonl.is_some())
        + usize::from(boss_prefix_jsonl.is_some());
    if write_modes > 1
        || (compare_jsonl.is_some() && write_modes > 0)
        || (replay_actions_jsonl.is_some() && write_modes > 0)
        || (boss_gauntlet_jsonl.is_some() && write_modes > 0)
        || (boss_gauntlet_jsonl.is_some() && replay_actions_jsonl.is_some())
    {
        eprintln!("choose only one fixture, action-log, boss-prefix, replay, or gauntlet mode");
        std::process::exit(2);
    }

    // Load the profile-backed unlock data once, then clone the in-memory value
    // into each fresh game. No assets or profile files are reloaded per seed.
    let unlocks = Unlocks::fixture();
    if let Some(gauntlet_path) = boss_gauntlet_jsonl {
        let logs = match load_action_log(&gauntlet_path) {
            Ok(logs) if !logs.is_empty() => logs,
            Ok(_) => {
                eprintln!("{} contains no boss prefixes", gauntlet_path.display());
                std::process::exit(2);
            }
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(2);
            }
        };
        if let Some(invalid) = logs.iter().find(|log| {
            !at_a20_second_boss_start(
                &replay_action_prefix(log, character, ascension, &unlocks),
                ascension,
            )
        }) {
            eprintln!(
                "{} seed {} does not end at an A20 second-boss entry",
                gauntlet_path.display(),
                invalid.seed
            );
            std::process::exit(2);
        }
        let seeds: Vec<i64> = logs.iter().map(|log| log.seed).collect();
        let start = Instant::now();
        let details =
            run_boss_gauntlet_batch(&logs, concurrent, character, ascension, max_steps, &unlocks);
        let elapsed = start.elapsed();
        let mut stats = BatchStats::default();
        for detail in &details {
            stats.record(detail);
        }
        print_batch(
            character,
            &seeds,
            None,
            concurrent,
            ascension,
            max_steps,
            &stats,
            &details,
            elapsed,
            diagnostics,
        );
        return;
    }
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
    let learned_policy = match deck_policy_mode {
        DeckPolicyMode::Htn => {
            eprintln!("deck_policy=htn");
            None
        }
        DeckPolicyMode::Learned => {
            if character != Character::Defect {
                eprintln!(
                    "--deck-policy rl currently supports DEFECT only; use --deck-policy htn for {:?}",
                    character
                );
                std::process::exit(2);
            }
            let policy = match LearnedDeckPolicy::load(&deck_policy_path) {
                Ok(policy) => policy,
                Err(message) => {
                    eprintln!("{message}");
                    std::process::exit(2);
                }
            };
            eprintln!(
                "deck_policy=rl generation={} source={} checkpoint={}",
                policy.generation(),
                policy.weight_source(),
                deck_policy_path.display()
            );
            Some(policy)
        }
    };
    let learned_policy = learned_policy.as_ref();
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
    if actions_jsonl.is_some() || boss_prefix_jsonl.is_some() {
        let start = Instant::now();
        let logs = run_action_batch(
            &seeds,
            concurrent,
            character,
            ascension,
            max_steps,
            &unlocks,
            learned_policy,
        );
        let (path, logs, label) = if let Some(path) = boss_prefix_jsonl {
            let prefixes = second_boss_prefixes(&logs, character, ascension, &unlocks);
            (path, prefixes, "A20 second-boss prefixes")
        } else {
            (actions_jsonl.unwrap(), logs, "action logs")
        };
        match write_action_log(&path, &logs) {
            Ok(()) => eprintln!(
                "wrote {} {} to {} in {:.3}s",
                logs.len(),
                label,
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
            &seeds,
            concurrent,
            character,
            ascension,
            max_steps,
            &unlocks,
            learned_policy,
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
            learned_policy,
            diagnostics,
            false,
        );
        print_single(actual_seed, character, ascension, &run);
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
        learned_policy,
        diagnostics,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_state_with_live_player_is_a_completed_win() {
        let mut game = Game::new(0, Character::Defect, 20, Unlocks::all());
        game.done = true;
        game.screen = Screen::Terminal;
        game.player.hp = 1;

        assert_eq!(completed_outcome(&game), Some(Outcome::Win));
    }

    #[test]
    fn dead_player_is_a_completed_loss() {
        let mut game = Game::new(0, Character::Defect, 20, Unlocks::all());
        game.done = true;
        game.screen = Screen::Terminal;
        game.player.hp = 0;

        assert_eq!(completed_outcome(&game), Some(Outcome::Loss));
    }

    #[test]
    fn death_layers_count_only_actual_player_deaths() {
        assert_eq!(death_act_index(Outcome::Loss, true, 1), Some(0));
        assert_eq!(death_act_index(Outcome::Loss, true, 4), Some(3));
        assert_eq!(death_act_index(Outcome::Loss, false, 3), None);
        assert_eq!(death_act_index(Outcome::Capped, true, 2), None);
        assert_eq!(death_act_index(Outcome::Loss, true, 5), None);
        assert_eq!(death_room_index(RoomType::Monster), 0);
        assert_eq!(death_room_index(RoomType::Elite), 1);
        assert_eq!(death_room_index(RoomType::Boss), 2);
        assert_eq!(death_room_index(RoomType::Event), 3);
    }
}
