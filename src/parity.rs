use crate::game::Game;
use crate::ids::{CardId, EncounterId, RelicId};
use crate::rng::RngSnapshot;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct JavaEnvelope {
    pub sequence: u64,
    pub boundary: String,
    pub state_sha256: String,
    pub state: JavaState,
}

#[derive(Debug, Deserialize)]
pub struct JavaState {
    pub dungeon: JavaDungeon,
    pub player: JavaPlayer,
    pub rng: JavaRngSet,
}

#[derive(Debug, Deserialize)]
pub struct JavaDungeon {
    pub act: i32,
    pub floor: i32,
    pub boss: EncounterId,
    pub monster_list: Vec<EncounterId>,
    pub elite_monster_list: Vec<EncounterId>,
    pub common_relic_pool: Vec<RelicId>,
    pub uncommon_relic_pool: Vec<RelicId>,
    pub rare_relic_pool: Vec<RelicId>,
    pub shop_relic_pool: Vec<RelicId>,
    pub boss_relic_pool: Vec<RelicId>,
}

#[derive(Debug, Deserialize)]
pub struct JavaPlayer {
    pub current_hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub relics: Vec<JavaNamed<RelicId>>,
    pub master_deck: Vec<JavaNamed<CardId>>,
}

#[derive(Debug, Deserialize)]
pub struct JavaNamed<T> {
    pub id: T,
}

#[derive(Debug, Deserialize)]
pub struct JavaRngSet {
    pub monster: JavaRng,
    pub map: JavaRng,
    pub event: JavaRng,
    pub relic: JavaRng,
    pub card: JavaRng,
    pub misc: JavaRng,
}

#[derive(Debug, Deserialize)]
pub struct JavaRng {
    pub counter: i32,
    pub state0: i64,
    pub state1: i64,
}

pub fn load_first_snapshot(path: impl AsRef<Path>) -> std::io::Result<JavaEnvelope> {
    let file = File::open(path)?;
    let mut lines = BufReader::new(file).lines();
    let line = lines
        .next()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "empty jsonl"))??;
    serde_json::from_str(&line).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

#[derive(Debug, Default)]
pub struct ParityReport {
    pub mismatches: Vec<String>,
}

impl ParityReport {
    pub fn ok(&self) -> bool {
        self.mismatches.is_empty()
    }
}

pub fn compare_generation(game: &Game, java: &JavaEnvelope) -> ParityReport {
    let mut report = ParityReport::default();
    check(
        &mut report,
        "boss",
        game.dungeon.boss,
        java.state.dungeon.boss,
    );
    check(
        &mut report,
        "monster_list",
        game.dungeon.monster_list.as_ref(),
        &java.state.dungeon.monster_list,
    );
    check(
        &mut report,
        "elite_list",
        game.dungeon.elite_list.as_ref(),
        &java.state.dungeon.elite_monster_list,
    );
    check(
        &mut report,
        "common_relics",
        game.dungeon.common_relics.as_ref(),
        &java.state.dungeon.common_relic_pool,
    );
    check(
        &mut report,
        "uncommon_relics",
        game.dungeon.uncommon_relics.as_ref(),
        &java.state.dungeon.uncommon_relic_pool,
    );
    check_rng(&mut report, "monster", game.rng.monster.snapshot(), &java.state.rng.monster);
    check_rng(&mut report, "map", game.rng.map.snapshot(), &java.state.rng.map);
    check_rng(&mut report, "relic", game.rng.relic.snapshot(), &java.state.rng.relic);
    check_rng(&mut report, "misc", game.rng.misc.snapshot(), &java.state.rng.misc);
    check(&mut report, "hp", game.player.hp, java.state.player.current_hp);
    check(&mut report, "gold", game.player.gold, java.state.player.gold);
    report
}

fn check<T: PartialEq + std::fmt::Debug>(report: &mut ParityReport, name: &str, got: T, expected: T) {
    if got != expected {
        report
            .mismatches
            .push(format!("{name}: got {got:?} expected {expected:?}"));
    }
}

fn check_rng(report: &mut ParityReport, name: &str, got: RngSnapshot, expected: &JavaRng) {
    if got.counter != expected.counter || got.state0 != expected.state0 || got.state1 != expected.state1 {
        report.mismatches.push(format!(
            "{name} rng: got {}/{}/{} expected {}/{}/{}",
            got.counter, got.state0, got.state1, expected.counter, expected.state0, expected.state1
        ));
    }
}
