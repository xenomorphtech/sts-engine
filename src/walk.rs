//! Lockstep ExactTextSim oracle walk. Shared by tests and `sts-parity`.

use crate::action::Action;
use crate::game::{Game, RewardKind, Screen};
use crate::ids::Character;
use crate::replay::load_commands;
use crate::rng::RngSnapshot;
use crate::Unlocks;
use serde::Deserialize;
use serde_json::Value;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct WalkConfig {
    pub name: String,
    pub states: PathBuf,
    pub commands: PathBuf,
    pub character: Character,
    pub unlocks: Unlocks,
    pub ascension: i32,
}

#[derive(Clone, Debug)]
pub struct WalkOk {
    pub last_ok: usize,
    pub snaps: usize,
    pub seed: i64,
}

#[derive(Clone, Debug)]
pub struct WalkFail {
    pub name: String,
    pub seed: i64,
    pub last_ok: usize,
    pub seq: usize,
    pub command_index: usize,
    pub boundary: String,
    pub mismatched: Vec<&'static str>,
    pub rust: Side,
    pub java: Side,
    pub cmds_around: Vec<(usize, Action)>,
    pub last_cmd: Option<Action>,
}

#[derive(Clone, Debug, Default)]
pub struct Side {
    pub screen: String,
    pub room: String,
    pub act: i32,
    pub floor: i32,
    pub hp: i32,
    pub gold: i32,
    pub block: i32,
    pub deck: Vec<String>,
    pub relics: Vec<String>,
    pub potions: Vec<String>,
    pub powers: Vec<String>,
    pub mons: Vec<(String, i32)>,
    pub hand: Vec<String>,
    pub event: String,
    pub options: Vec<String>,
    pub rewards: Vec<String>,
    pub card_reward: Vec<String>,
    pub pending: Vec<String>,
    pub overlay: String,
    pub rng: Vec<(String, String)>,
}

impl Display for WalkFail {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "{} first mismatch at seq {} {}  last_ok {}  cmd_index {}  seed {}",
            self.name, self.seq, self.boundary, self.last_ok, self.command_index, self.seed
        )?;
        writeln!(f, "mismatched: {}", self.mismatched.join(", "))?;
        if let Some(cmd) = &self.last_cmd {
            writeln!(f, "last command [{:?}]: {cmd:?}", self.command_index.saturating_sub(1))?;
        }
        if !self.cmds_around.is_empty() {
            writeln!(f, "commands around fail:")?;
            for (i, cmd) in &self.cmds_around {
                writeln!(f, "  [{i}] {cmd:?}")?;
            }
        }
        writeln!(f)?;
        writeln!(f, "--- rust ---")?;
        write_side(f, &self.rust)?;
        writeln!(f)?;
        writeln!(f, "--- java ---")?;
        write_side(f, &self.java)?;
        Ok(())
    }
}

fn write_side(f: &mut Formatter<'_>, s: &Side) -> std::fmt::Result {
    writeln!(
        f,
        "  screen={} room={} act={} floor={} hp={} gold={} block={}",
        s.screen, s.room, s.act, s.floor, s.hp, s.gold, s.block
    )?;
    writeln!(f, "  deck={:?}", s.deck)?;
    writeln!(f, "  relics={:?} pots={:?} powers={:?}", s.relics, s.potions, s.powers)?;
    writeln!(f, "  mons={:?} hand={:?}", s.mons, s.hand)?;
    if !s.event.is_empty() {
        writeln!(f, "  event={}", s.event)?;
    }
    if !s.options.is_empty() {
        writeln!(f, "  options={:?}", s.options)?;
    }
    if !s.rewards.is_empty() {
        writeln!(f, "  rewards={:?}", s.rewards)?;
    }
    if !s.card_reward.is_empty() {
        writeln!(f, "  card_reward={:?}", s.card_reward)?;
    }
    if !s.pending.is_empty() {
        writeln!(f, "  pending_cards={:?}", s.pending)?;
    }
    if !s.overlay.is_empty() {
        writeln!(f, "  overlay={}", s.overlay)?;
    }
    if !s.rng.is_empty() {
        writeln!(f, "  rng:")?;
        for (name, dump) in &s.rng {
            writeln!(f, "    {name}: {dump}")?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct Envelope {
    sequence: usize,
    #[serde(default)]
    command_index: Option<usize>,
    boundary: String,
    state: State,
}

#[derive(Deserialize)]
struct State {
    #[serde(default)]
    game: Option<GameMeta>,
    dungeon: DungeonSnap,
    player: PlayerSnap,
    #[serde(default)]
    combat: Option<CombatSnap>,
    #[serde(default)]
    screen: Value,
    #[serde(default)]
    room: Value,
    #[serde(default)]
    rng: Value,
}

#[derive(Deserialize)]
struct GameMeta {
    #[serde(default)]
    seed: Option<i64>,
}

#[derive(Deserialize)]
struct DungeonSnap {
    floor: i32,
    act: i32,
}

#[derive(Deserialize)]
struct PlayerSnap {
    current_hp: i32,
    #[serde(default)]
    gold: i32,
    #[serde(default)]
    block: i32,
    #[serde(default)]
    master_deck: Vec<Named>,
    #[serde(default)]
    relics: Vec<Named>,
    #[serde(default)]
    potions: Vec<Named>,
    #[serde(default)]
    powers: Vec<PowerAmt>,
}

#[derive(Deserialize)]
struct PowerAmt {
    id: String,
    #[serde(default)]
    amount: i32,
}

#[derive(Deserialize)]
struct Named {
    id: String,
}

#[derive(Deserialize)]
struct CombatSnap {
    #[serde(default)]
    hand: Vec<Named>,
    #[serde(default)]
    monsters: Vec<Mon>,
}

#[derive(Deserialize)]
struct Mon {
    id: String,
    current_hp: i32,
}

fn parse_envelope(line: &str) -> Result<Option<Envelope>, serde_json::Error> {
    let value: Value = serde_json::from_str(line)?;
    if value.get("boundary").is_none() {
        return Ok(None);
    }
    serde_json::from_value(value).map(Some)
}

fn dummy_fail(name: &str, msg: &str) -> WalkFail {
    WalkFail {
        name: name.to_string(),
        seed: 0,
        last_ok: 0,
        seq: 0,
        command_index: 0,
        boundary: msg.to_string(),
        mismatched: vec!["io"],
        rust: Side::default(),
        java: Side::default(),
        cmds_around: Vec::new(),
        last_cmd: None,
    }
}

pub fn walk_oracle(cfg: &WalkConfig) -> Result<WalkOk, WalkFail> {
    if !cfg.states.exists() || !cfg.commands.exists() {
        return Err(dummy_fail(
            &cfg.name,
            &format!("missing {} or {}", cfg.states.display(), cfg.commands.display()),
        ));
    }
    let cmds = load_commands(&cfg.commands).map_err(|e| dummy_fail(&cfg.name, &e.to_string()))?;
    let file = File::open(&cfg.states).map_err(|e| dummy_fail(&cfg.name, &e.to_string()))?;
    let mut lines = BufReader::new(file).lines();
    let first_line = lines
        .next()
        .and_then(|r| r.ok())
        .ok_or_else(|| dummy_fail(&cfg.name, "empty states jsonl"))?;
    let first: Envelope = match parse_envelope(&first_line) {
        Ok(Some(e)) => e,
        Ok(None) => {
            return Err(dummy_fail(&cfg.name, "first state line is stall_diag / missing boundary"));
        }
        Err(e) => {
            let head = first_line.chars().take(180).collect::<String>();
            return Err(dummy_fail(
                &cfg.name,
                &format!("{e} (line_len={} head={head:?})", first_line.len()),
            ));
        }
    };
    let seed = first.state.game.as_ref().and_then(|g| g.seed).unwrap_or(2);
    let mut game = Game::new(seed, cfg.character, cfg.ascension, cfg.unlocks.clone());
    let mut last_ok = 0usize;
    let mut applied = 0usize;
    let mut snaps = 0usize;

    let mut check = |snap: Envelope| -> Result<(), WalkFail> {
        snaps += 1;
        let seq = snap.sequence;
        let target = snap.command_index.unwrap_or(seq);
        while applied < target && applied < cmds.len() {
            game.step(&cmds[applied]);
            applied += 1;
        }
        let rust = rust_side(&game);
        let java = java_side(&snap);
        let mut mismatched = Vec::new();
        if rust.hp != java.hp {
            mismatched.push("hp");
        }
        if rust.gold != java.gold {
            mismatched.push("gold");
        }
        if rust.block != java.block {
            mismatched.push("block");
        }
        if rust.floor != java.floor {
            mismatched.push("floor");
        }
        if rust.act != java.act {
            mismatched.push("act");
        }
        if rust.deck != java.deck {
            // Combat-reward picks flush immediately (FastCardObtainEffect is
            // done before the next stable boundary). Other obtains (Neow
            // transform, some relics) still sit in pending_cards until the
            // matching VFX completes; ExactTextSim waits, so java.deck may
            // already include them.
            let mut landed = rust.deck.clone();
            landed.extend(rust.pending.iter().cloned());
            if landed != java.deck {
                mismatched.push("deck");
            }
        }
        if rust.mons != java.mons {
            mismatched.push("mons");
        }
        if rust.hand != java.hand {
            mismatched.push("hand");
        }
        if rust.relics != java.relics {
            mismatched.push("relics");
        }
        if !mismatched.is_empty() {
            let start = applied.saturating_sub(4);
            let cmds_around = (start..applied.min(cmds.len()))
                .map(|i| (i, cmds[i].clone()))
                .collect();
            let last_cmd = applied.checked_sub(1).and_then(|i| cmds.get(i).cloned());
            return Err(WalkFail {
                name: cfg.name.clone(),
                seed,
                last_ok,
                seq,
                command_index: target,
                boundary: snap.boundary.clone(),
                mismatched,
                rust,
                java,
                cmds_around,
                last_cmd,
            });
        }
        last_ok = seq;
        Ok(())
    };

    check(first)?;
    for line in lines {
        let line = line.map_err(|e| dummy_fail(&cfg.name, &e.to_string()))?;
        if line.trim().is_empty() {
            continue;
        }
        match parse_envelope(&line) {
            Ok(Some(snap)) => check(snap)?,
            Ok(None) => continue, // stall_diag and other non-state lines
            Err(e) => {
                return Err(dummy_fail(
                    &cfg.name,
                    &format!("{e} after last_ok={last_ok} (line_len={})", line.len()),
                ));
            }
        }
    }
    Ok(WalkOk {
        last_ok,
        snaps,
        seed,
    })
}

fn rust_side(game: &Game) -> Side {
    let mut overlay = String::new();
    if let Some(g) = game.grid_summary() {
        overlay = format!("grid {g}");
    }
    Side {
        screen: format!("{:?}", game.screen),
        room: format!("{:?}", game.current_room),
        act: game.dungeon.act as i32,
        floor: game.dungeon.floor,
        hp: game.player.hp,
        gold: game.player.gold,
        block: game.player.block,
        deck: game.player.deck.iter().map(|c| c.sts_id().to_string()).collect(),
        relics: game.player.relics.iter().map(|r| r.id.sts_id().to_string()).collect(),
        potions: game
            .player
            .potions
            .iter()
            .filter(|p| p.id != crate::ids::PotionId::Slot)
            .map(|p| p.id.sts_id().to_string())
            .collect(),
        powers: game
            .player
            .powers
            .iter()
            .map(|p| format!("{:?}:{}", p.id, p.amount))
            .collect(),
        mons: game
            .combat
            .as_ref()
            .map(|c| c.monsters.iter().map(|m| (m.id.sts_id().to_string(), m.hp)).collect())
            .unwrap_or_default(),
        hand: game.player.hand.iter().map(|c| c.sts_id().to_string()).collect(),
        event: game.event.as_ref().map(|e| e.id.clone()).unwrap_or_default(),
        options: match game.screen {
            Screen::Event => game.event.as_ref().map(|e| e.options.clone()).unwrap_or_default(),
            Screen::Neow => game.neow_options.iter().map(|o| o.label.clone()).collect(),
            _ => Vec::new(),
        },
        rewards: game
            .rewards
            .iter()
            .map(|r| {
                let taken = if r.taken { " taken" } else { "" };
                match &r.kind {
                    RewardKind::Gold(g) => format!("GOLD({g}){taken}"),
                    RewardKind::StolenGold(g) => format!("STOLEN_GOLD({g}){taken}"),
                    RewardKind::Potion(p) => format!("POTION({}){taken}", p.sts_id()),
                    RewardKind::Relic(id) => format!("RELIC({}){taken}", id.sts_id()),
                    RewardKind::Card => format!("CARD{taken}"),
                }
            })
            .collect(),
        card_reward: game.card_reward.iter().map(|c| c.sts_id().to_string()).collect(),
        pending: game.pending_cards.iter().map(|c| c.sts_id().to_string()).collect(),
        overlay,
        rng: rust_rng(game),
    }
}

fn rust_rng(game: &Game) -> Vec<(String, String)> {
    let s = game.rng.snapshot();
    let pairs = [
        ("monster", s.monster),
        ("map", s.map),
        ("event", s.event),
        ("merchant", s.merchant),
        ("card", s.card),
        ("treasure", s.treasure),
        ("relic", s.relic),
        ("potion", s.potion),
        ("monster_hp", s.monster_hp),
        ("ai", s.ai),
        ("shuffle", s.shuffle),
        ("card_random", s.card_random),
        ("misc", s.misc),
    ];
    pairs
        .into_iter()
        .map(|(n, r)| (n.to_string(), fmt_rng(&r)))
        .collect()
}

fn fmt_rng(r: &RngSnapshot) -> String {
    format!("counter={} s0={} s1={}", r.counter, r.state0, r.state1)
}

fn java_side(snap: &Envelope) -> Side {
    let st = &snap.state;
    let screen_name = java_screen_name(&st.screen);
    let event = java_event(&st.room);
    let (rewards, card_from_screen) = java_rewards(&st.room, &st.screen);
    let mut card_reward = java_cards(&st.screen);
    if card_reward.is_empty() {
        card_reward = card_from_screen;
    }
    Side {
        screen: screen_name,
        room: st
            .room
            .get("class")
            .and_then(Value::as_str)
            .unwrap_or("")
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_string(),
        act: st.dungeon.act,
        floor: st.dungeon.floor,
        hp: st.player.current_hp,
        gold: st.player.gold,
        block: st.player.block,
        deck: st.player.master_deck.iter().map(|c| c.id.clone()).collect(),
        relics: st.player.relics.iter().map(|c| c.id.clone()).collect(),
        potions: st
            .player
            .potions
            .iter()
            .filter(|p| p.id != "Potion Slot")
            .map(|p| p.id.clone())
            .collect(),
        powers: st
            .player
            .powers
            .iter()
            .map(|p| format!("{}:{}", p.id, p.amount))
            .collect(),
        mons: st
            .combat
            .as_ref()
            .map(|c| c.monsters.iter().map(|m| (m.id.clone(), m.current_hp)).collect())
            .unwrap_or_default(),
        hand: st
            .combat
            .as_ref()
            .map(|c| c.hand.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default(),
        event: event.0,
        options: event.1,
        rewards,
        card_reward,
        pending: Vec::new(),
        overlay: java_grid(&st.screen),
        rng: java_rng(&st.rng),
    }
}

fn java_screen_name(v: &Value) -> String {
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    v.get("name")
        .and_then(Value::as_str)
        .unwrap_or("NONE")
        .to_string()
}

fn java_event(room: &Value) -> (String, Vec<String>) {
    let ev = room.get("event").unwrap_or(&Value::Null);
    let class = ev
        .get("class")
        .and_then(Value::as_str)
        .unwrap_or("")
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_string();
    let mut opts = Vec::new();
    if let Some(arr) = ev.get("options").and_then(Value::as_array) {
        for o in arr {
            let slot = o.get("slot").and_then(Value::as_i64).unwrap_or(-1);
            let text = o.get("text").and_then(Value::as_str).unwrap_or("");
            let dis = o.get("disabled").and_then(Value::as_bool).unwrap_or(false);
            opts.push(format!("{slot}{} {text}", if dis { " [locked]" } else { "" }));
        }
    }
    (class, opts)
}

fn java_cards(screen: &Value) -> Vec<String> {
    screen
        .get("cards")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

fn java_rewards(room: &Value, screen: &Value) -> (Vec<String>, Vec<String>) {
    let mut out = Vec::new();
    let mut cards = Vec::new();
    let lists = [
        screen.get("rewards"),
        room.get("rewards"),
    ];
    for list in lists.into_iter().flatten() {
        if let Some(arr) = list.as_array() {
            for r in arr {
                let ty = r.get("type").and_then(Value::as_str).unwrap_or("?");
                let done = r.get("done").and_then(Value::as_bool).unwrap_or(false);
                let taken = if done { " taken" } else { "" };
                match ty {
                    "GOLD" | "STOLEN_GOLD" => {
                        let g = r.get("gold").and_then(Value::as_i64).unwrap_or(0);
                        out.push(format!("{ty}({g}){taken}"));
                    }
                    "POTION" => {
                        let id = r
                            .get("potion")
                            .and_then(|p| p.get("id").or(p.get("ID")))
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        out.push(format!("POTION({id}){taken}"));
                    }
                    "RELIC" => {
                        let id = r
                            .get("relic")
                            .and_then(|p| p.get("id").or(p.get("relicId")))
                            .and_then(Value::as_str)
                            .unwrap_or("?");
                        out.push(format!("RELIC({id}){taken}"));
                    }
                    "CARD" => {
                        out.push(format!("CARD{taken}"));
                        if let Some(arr) = r.get("cards").and_then(Value::as_array) {
                            cards = arr
                                .iter()
                                .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_string))
                                .collect();
                        }
                    }
                    other => out.push(format!("{other}{taken}")),
                }
            }
            if !out.is_empty() {
                break;
            }
        }
    }
    (out, cards)
}

fn java_grid(screen: &Value) -> String {
    let name = java_screen_name(screen);
    if name != "GRID" {
        return String::new();
    }
    format!(
        "GRID purge={} upgrade={} transform={} confirm={} num={}",
        screen.get("for_purge").and_then(Value::as_bool).unwrap_or(false),
        screen.get("for_upgrade").and_then(Value::as_bool).unwrap_or(false),
        screen.get("for_transform").and_then(Value::as_bool).unwrap_or(false),
        screen.get("confirm_screen_up").and_then(Value::as_bool).unwrap_or(false),
        screen.get("num_cards").and_then(Value::as_i64).unwrap_or(0)
    )
}

fn java_rng(v: &Value) -> Vec<(String, String)> {
    let names = [
        "monster",
        "map",
        "event",
        "merchant",
        "card",
        "treasure",
        "relic",
        "potion",
        "monster_hp",
        "ai",
        "shuffle",
        "card_random",
        "misc",
    ];
    names
        .into_iter()
        .filter_map(|n| {
            let s = v.get(n)?;
            let c = s.get("counter").and_then(Value::as_i64).unwrap_or(0);
            let s0 = s.get("state0").and_then(Value::as_i64).unwrap_or(0);
            let s1 = s.get("state1").and_then(Value::as_i64).unwrap_or(0);
            Some((n.to_string(), format!("counter={c} s0={s0} s1={s1}")))
        })
        .collect()
}

pub fn runtime_root() -> PathBuf {
    if let Ok(p) = std::env::var("STS_RUNTIME") {
        return PathBuf::from(p);
    }
    let crate_rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime");
    for cand in [
        PathBuf::from("exact-text-sim/runtime"),
        PathBuf::from("../exact-text-sim/runtime"),
        crate_rel.clone(),
    ] {
        if cand.exists() {
            return cand;
        }
    }
    crate_rel
}

pub fn oracle_paths(character: Character, seed: &str, ascension: i32) -> (PathBuf, PathBuf) {
    let root = runtime_root();
    let preferred = root
        .join("oracles")
        .join(character.oracle_dir())
        .join(format!("a{ascension}"))
        .join(seed);
    if preferred.join("states.jsonl").exists() {
        return (preferred.join("states.jsonl"), preferred.join("commands.jsonl"));
    }
    // Harvest sometimes files A0 hunts under a20 (summary default). Try both.
    for asc in [ascension, 0, 20] {
        let dir = root
            .join("oracles")
            .join(character.oracle_dir())
            .join(format!("a{asc}"))
            .join(seed);
        if dir.join("states.jsonl").exists() {
            return (dir.join("states.jsonl"), dir.join("commands.jsonl"));
        }
    }
    (preferred.join("states.jsonl"), preferred.join("commands.jsonl"))
}

pub fn default_config(character: Character, seed: &str, unlocks: Unlocks, ascension: i32) -> WalkConfig {
    let (states, commands) = oracle_paths(character, seed, ascension);
    WalkConfig {
        name: format!("{}-s{seed}", character.oracle_dir()),
        states,
        commands,
        character,
        unlocks,
        ascension,
    }
}

pub fn walk_from_runtime(
    name: &str,
    states_rel: impl AsRef<Path>,
    commands_rel: impl AsRef<Path>,
    character: Character,
    unlocks: Unlocks,
) -> Result<WalkOk, WalkFail> {
    let root = runtime_root();
    let cfg = WalkConfig {
        name: name.to_string(),
        states: root.join(states_rel),
        commands: root.join(commands_rel),
        character,
        unlocks,
        ascension: 0,
    };
    walk_oracle(&cfg)
}
