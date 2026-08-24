//! Print HTN combats as compact turn-by-turn tables.
//!
//! ```sh
//! cargo run --bin sts-combat-trace -- 1200928818479558666
//! cargo run --bin sts-combat-trace -- 1200928818479558666 --floors 1-3
//! cargo run --bin sts-combat-trace -- --seed 7 --character DEFECT --ascension 20
//! ```

use std::env;
use sts_engine::card::Card;
use sts_engine::creature::{Intent, Monster, Player};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{CardId, Character, MonsterId, PowerId, RelicId};
use sts_engine::{seed_from_string, Action, Game, Screen, Unlocks};

const DEFAULT_SEED: &str = "1200928818479558666";
const DEFAULT_MAX_STEPS: usize = 10_000;

#[derive(Clone, Debug)]
struct Config {
    seed: i64,
    seed_label: String,
    character: Character,
    ascension: i32,
    max_steps: usize,
    floors: Option<FloorRange>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            seed: parse_seed(DEFAULT_SEED),
            seed_label: DEFAULT_SEED.to_string(),
            character: Character::Defect,
            ascension: 20,
            max_steps: DEFAULT_MAX_STEPS,
            floors: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FloorRange {
    start: i32,
    end: i32,
}

impl FloorRange {
    fn new(start: i32, end: i32) -> Result<Self, String> {
        if start < 1 || end < 1 {
            return Err("floor numbers must be positive".to_string());
        }
        if start > end {
            return Err(format!("start floor {start} is after end floor {end}"));
        }
        Ok(Self { start, end })
    }

    fn contains(self, floor: i32) -> bool {
        (self.start..=self.end).contains(&floor)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TurnRow {
    turn: i32,
    plays: Vec<String>,
    unplayed: Vec<String>,
    monster_hp: i32,
    incoming_and_block: String,
    player_hp: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FightTrace {
    floor: i32,
    monster_name: String,
    rows: Vec<TurnRow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FloorSummary {
    floor: i32,
    entry_hp: i32,
    room_type: String,
    mobs: Vec<String>,
    finish_hp: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TraceRun {
    fights: Vec<FightTrace>,
    floors: Vec<FloorSummary>,
    steps: usize,
}

fn parse_seed(raw: &str) -> i64 {
    raw.parse::<i64>().unwrap_or_else(|_| seed_from_string(raw))
}

fn parse_floor(raw: &str, label: &str) -> Result<i32, String> {
    raw.parse().map_err(|_| format!("invalid {label} {raw:?}"))
}

fn parse_floor_range(raw: &str) -> Result<Option<FloorRange>, String> {
    let pieces = raw
        .split_once("..")
        .or_else(|| raw.split_once('-'))
        .or_else(|| raw.split_once(':'));
    let Some((start, end)) = pieces else {
        return Ok(None);
    };
    FloorRange::new(
        parse_floor(start, "start floor")?,
        parse_floor(end, "end floor")?,
    )
    .map(Some)
}

fn required_value(args: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    args.get(*index)
        .cloned()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_args() -> Result<Option<Config>, String> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut config = Config::default();
    let mut positional_seed = false;
    let mut start_floor = None;
    let mut end_floor = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "-h" | "--help" => return Ok(None),
            "--seed" => {
                let raw = required_value(&args, &mut index, "--seed")?;
                config.seed = parse_seed(&raw);
                config.seed_label = raw;
                positional_seed = true;
            }
            "--character" => {
                let raw = required_value(&args, &mut index, "--character")?;
                config.character = Character::from_cli(&raw)
                    .ok_or_else(|| format!("unsupported character {raw:?}"))?;
            }
            "--ascension" => {
                let raw = required_value(&args, &mut index, "--ascension")?;
                config.ascension = raw
                    .parse()
                    .map_err(|_| format!("invalid ascension {raw:?}"))?;
            }
            "--max-steps" => {
                let raw = required_value(&args, &mut index, "--max-steps")?;
                config.max_steps = raw
                    .parse()
                    .map_err(|_| format!("invalid max step count {raw:?}"))?;
            }
            "--floors" => {
                if config.floors.is_some() || start_floor.is_some() || end_floor.is_some() {
                    return Err("floor range specified more than once".to_string());
                }
                let raw = required_value(&args, &mut index, "--floors")?;
                config.floors = if let Some(range) = parse_floor_range(&raw)? {
                    Some(range)
                } else {
                    let end = required_value(&args, &mut index, "--floors")?;
                    Some(FloorRange::new(
                        parse_floor(&raw, "start floor")?,
                        parse_floor(&end, "end floor")?,
                    )?)
                };
            }
            "--from-floor" | "--start-floor" => {
                if config.floors.is_some() || start_floor.is_some() {
                    return Err("start floor specified more than once".to_string());
                }
                let raw = required_value(&args, &mut index, "--from-floor")?;
                start_floor = Some(parse_floor(&raw, "start floor")?);
            }
            "--to-floor" | "--end-floor" => {
                if config.floors.is_some() || end_floor.is_some() {
                    return Err("end floor specified more than once".to_string());
                }
                let raw = required_value(&args, &mut index, "--to-floor")?;
                end_floor = Some(parse_floor(&raw, "end floor")?);
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag:?}")),
            raw if !positional_seed => {
                config.seed = parse_seed(raw);
                config.seed_label = raw.to_string();
                positional_seed = true;
            }
            raw => return Err(format!("unexpected argument {raw:?}")),
        }
        index += 1;
    }
    if !(0..=20).contains(&config.ascension) {
        return Err("ascension must be between 0 and 20".to_string());
    }
    match (start_floor, end_floor) {
        (Some(start), Some(end)) => config.floors = Some(FloorRange::new(start, end)?),
        (None, None) => {}
        _ => return Err("both start and end floor must be specified".to_string()),
    }
    Ok(Some(config))
}

fn usage() -> &'static str {
    "Usage: sts-combat-trace [SEED] [--seed SEED] [--character CHARACTER] \
[--ascension 0..20] [--floors START-END] [--max-steps N]\n\
\n\
With no floor range, prints entry HP, room/mobs, and finish HP for every floor.\n\
With a range, prints every detailed fight on those inclusive floors. The range\n\
may also be written as\n\
--from-floor START --to-floor END.\n\
Defaults: seed 1200928818479558666, character DEFECT, ascension 20."
}

fn character_name(character: Character) -> &'static str {
    match character {
        Character::Ironclad => "Ironclad",
        Character::Silent => "Silent",
        Character::Defect => "Defect",
        Character::Watcher => "Watcher",
    }
}

fn identifier_words(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len() + 4);
    let mut previous: Option<char> = None;
    for character in raw.chars() {
        if character == '_' {
            if !result.ends_with(' ') {
                result.push(' ');
            }
        } else {
            if character.is_ascii_uppercase()
                && previous.is_some_and(|before| before.is_ascii_lowercase())
            {
                result.push(' ');
            }
            result.push(character);
        }
        previous = Some(character);
    }
    result
}

fn card_name(card: &Card) -> String {
    let name = match card.id {
        CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P => {
            "Strike".to_string()
        }
        CardId::Defend_R | CardId::Defend_G | CardId::Defend_B | CardId::Defend_P => {
            "Defend".to_string()
        }
        CardId::AscendersBane => "Ascender's Bane".to_string(),
        _ => identifier_words(card.sts_id()),
    };
    if card.upgraded {
        format!("{name}+")
    } else {
        name
    }
}

fn hand_names(game: &Game) -> Vec<String> {
    game.player.hand.iter().map(card_name).collect()
}

fn compress_names(names: &[String]) -> String {
    if names.is_empty() {
        return "—".to_string();
    }
    let mut groups: Vec<(&str, usize)> = Vec::new();
    for name in names {
        if let Some((last, count)) = groups.last_mut() {
            if *last == name {
                *count += 1;
                continue;
            }
        }
        groups.push((name, 1));
    }
    groups
        .into_iter()
        .map(|(name, count)| {
            if count == 1 {
                name.to_string()
            } else {
                format!("{name} ×{count}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn projected_attack(player: &Player, monster: &Monster) -> i32 {
    if monster.intent_damage <= 0 {
        return 0;
    }
    let mut damage = (monster.intent_damage + monster.power_amount(PowerId::Strength)) as f32;
    if monster.power_amount(PowerId::Weak) > 0 {
        damage *= 0.75;
    }
    if player.power_amount(PowerId::Vulnerable) > 0 {
        damage *= if player.has_relic(RelicId::Odd_Mushroom) {
            1.25
        } else {
            1.5
        };
    }
    let mut damage = damage.floor().max(0.0) as i32;
    if player.power_amount(PowerId::Intangible) > 0 && damage > 1 {
        damage = 1;
    }
    damage * monster.intent_hits.max(1)
}

fn incoming_and_block(game: &Game) -> String {
    let block = game.player.block;
    let Some(combat) = game.combat.as_ref() else {
        return format!("Lethal / {block}");
    };
    let living: Vec<_> = combat
        .monsters
        .iter()
        .filter(|monster| monster.alive())
        .collect();
    let incoming: i32 = living
        .iter()
        .map(|monster| projected_attack(&game.player, monster))
        .sum();
    let ritual = !living.is_empty()
        && living
            .iter()
            .all(|monster| monster.id == MonsterId::Cultist && monster.intent == Intent::Buff);
    if ritual {
        format!("Ritual / {block} wasted")
    } else if incoming == 0 {
        format!("No attack / {block} wasted")
    } else {
        format!("{incoming} / {block}")
    }
}

fn monster_hp(game: &Game) -> i32 {
    game.combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| !monster.escaped)
                .map(|monster| monster.hp.max(0))
                .sum()
        })
        .unwrap_or(0)
}

fn first_monster_name(game: &Game) -> String {
    game.combat
        .as_ref()
        .and_then(|combat| combat.monsters.first())
        .map(|monster| identifier_words(monster.id.sts_id()))
        .unwrap_or_else(|| "Monster".to_string())
}

fn current_mob_names(game: &Game) -> Vec<String> {
    game.combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .map(|monster| identifier_words(monster.id.sts_id()))
                .collect()
        })
        .unwrap_or_default()
}

fn merge_mobs(known: &mut Vec<String>, observed: &[String]) {
    for (index, mob) in observed.iter().enumerate() {
        let occurrence = observed[..=index]
            .iter()
            .filter(|candidate| *candidate == mob)
            .count();
        if known.iter().filter(|candidate| *candidate == mob).count() < occurrence {
            known.push(mob.clone());
        }
    }
}

fn observe_floor_state(game: &Game, floors: &mut Vec<FloorSummary>, entry_hp: i32) {
    let floor = game.dungeon.floor;
    if floor < 1 {
        return;
    }
    let room_type = identifier_words(&format!("{:?}", game.current_room));
    let mobs = current_mob_names(game);
    if let Some(summary) = floors.last_mut().filter(|summary| summary.floor == floor) {
        summary.finish_hp = game.player.hp;
        merge_mobs(&mut summary.mobs, &mobs);
    } else {
        floors.push(FloorSummary {
            floor,
            entry_hp,
            room_type,
            mobs,
            finish_hp: game.player.hp,
        });
    }
}

fn step_and_observe(game: &mut Game, action: &Action, floors: &mut Vec<FloorSummary>) {
    let floor_before = game.dungeon.floor;
    let hp_before = game.player.hp;
    game.step(action);
    let entry_hp = if game.dungeon.floor != floor_before {
        hp_before
    } else {
        game.player.hp
    };
    observe_floor_state(game, floors, entry_hp);
}

fn terminal(game: &Game) -> bool {
    game.done || game.player.hp <= 0 || game.screen == Screen::Terminal
}

fn trace_current_fight(
    game: &mut Game,
    agent: &mut HtnAgent,
    steps: &mut usize,
    max_steps: usize,
    floors: &mut Vec<FloorSummary>,
) -> Result<FightTrace, String> {
    let floor = game.dungeon.floor;
    let monster_name = first_monster_name(&game);
    if game.combat.is_none() {
        return Err("cannot trace a fight outside combat".to_string());
    }
    let mut rows = Vec::new();
    while game.combat.is_some() && game.player.hp > 0 && *steps < max_steps {
        let turn = game.combat.as_ref().expect("combat disappeared").turn;
        let mut plays = Vec::new();
        let mut completed_row = None;
        loop {
            let current_turn = game.combat.as_ref().map(|combat| combat.turn);
            if current_turn != Some(turn) || game.player.hp <= 0 || *steps >= max_steps {
                break;
            }
            let action = agent.decide(&game);
            if action == Action::Quit {
                return Err(format!(
                    "HTN agent quit during combat on floor {floor}, turn {turn}"
                ));
            }
            let mut fallback_unplayed = hand_names(game);
            let fallback_incoming = incoming_and_block(game);
            match &action {
                Action::Play { hand_index, .. } => {
                    let card = game.player.hand.get(*hand_index).ok_or_else(|| {
                        format!("HTN chose missing hand index {hand_index} on turn {turn}")
                    })?;
                    let card_id = card.id;
                    let mut name = card_name(card);
                    let block_before = game.player.block;
                    let mut remaining = hand_names(&game);
                    remaining.remove(*hand_index);
                    fallback_unplayed = remaining.clone();
                    step_and_observe(game, &action, floors);
                    *steps += 1;
                    if card_id == CardId::Stack {
                        name = format!("Stack for {}", (game.player.block - block_before).max(0));
                    }
                    plays.push(name);
                    if game.combat.is_none() {
                        completed_row = Some(TurnRow {
                            turn,
                            plays: plays.clone(),
                            unplayed: remaining,
                            monster_hp: 0,
                            incoming_and_block: format!("Lethal / {}", game.player.block),
                            player_hp: game.player.hp,
                        });
                    }
                }
                Action::EndTurn => {
                    let unplayed = hand_names(&game);
                    let incoming_and_block = incoming_and_block(&game);
                    step_and_observe(game, &action, floors);
                    *steps += 1;
                    completed_row = Some(TurnRow {
                        turn,
                        plays: plays.clone(),
                        unplayed,
                        monster_hp: monster_hp(&game),
                        incoming_and_block,
                        player_hp: game.player.hp,
                    });
                }
                Action::Potion {
                    slot,
                    action: operation,
                    ..
                } => {
                    let potion = game
                        .player
                        .potions
                        .get(*slot)
                        .map(|potion| identifier_words(potion.id.sts_id()))
                        .unwrap_or_else(|| format!("slot {slot}"));
                    plays.push(format!("{operation:?} {potion}"));
                    step_and_observe(game, &action, floors);
                    *steps += 1;
                }
                _ => {
                    step_and_observe(game, &action, floors);
                    *steps += 1;
                }
            }
            if completed_row.is_none()
                && (game.combat.is_none()
                    || game.player.hp <= 0
                    || game.combat.as_ref().map(|combat| combat.turn) != Some(turn))
            {
                completed_row = Some(TurnRow {
                    turn,
                    plays: plays.clone(),
                    unplayed: fallback_unplayed,
                    monster_hp: monster_hp(game),
                    incoming_and_block: fallback_incoming,
                    player_hp: game.player.hp,
                });
            }
            if completed_row.is_some() {
                break;
            }
        }
        if let Some(row) = completed_row {
            rows.push(row);
        }
        if game.combat.is_none() || game.player.hp <= 0 {
            break;
        }
    }
    if *steps >= max_steps && game.player.hp > 0 && game.combat.is_some() {
        return Err(format!(
            "reached --max-steps {max_steps} during combat on floor {floor}"
        ));
    }
    Ok(FightTrace {
        floor,
        monster_name,
        rows,
    })
}

fn run_trace(config: &Config) -> Result<TraceRun, String> {
    let mut game = Game::new(
        config.seed,
        config.character,
        config.ascension,
        Unlocks::fixture(),
    );
    let mut agent = HtnAgent::new();
    let mut steps = 0;
    let mut fights = Vec::new();
    let mut floors = Vec::new();
    observe_floor_state(&game, &mut floors, game.player.hp);

    loop {
        if terminal(&game) {
            break;
        }
        if config
            .floors
            .is_some_and(|range| game.dungeon.floor > range.end)
        {
            break;
        }
        if game.combat.is_some() {
            let fight = trace_current_fight(
                &mut game,
                &mut agent,
                &mut steps,
                config.max_steps,
                &mut floors,
            )?;
            if config
                .floors
                .is_some_and(|range| range.contains(fight.floor))
            {
                fights.push(fight);
            }
            continue;
        }
        if steps >= config.max_steps {
            return Err(format!(
                "reached --max-steps {} on floor {}",
                config.max_steps, game.dungeon.floor
            ));
        }
        let action = agent.decide(&game);
        if action == Action::Quit {
            break;
        }
        step_and_observe(&mut game, &action, &mut floors);
        steps += 1;
    }

    if let Some(range) = config.floors {
        if fights.is_empty() {
            return Err(format!(
                "no fights found from floor {} through {}",
                range.start, range.end
            ));
        }
    } else if floors.is_empty() {
        return Err("run ended before the first floor".to_string());
    }
    Ok(TraceRun {
        fights,
        floors,
        steps,
    })
}

fn append_cell(line: &mut String, value: &str, width: usize, right: bool) {
    let padding = width.saturating_sub(value.chars().count());
    if right {
        for _ in 0..padding {
            line.push(' ');
        }
        line.push_str(value);
    } else {
        line.push_str(value);
        for _ in 0..padding {
            line.push(' ');
        }
    }
}

fn render_table(rows: &[TurnRow], monster_name: &str, player_name: &str) -> String {
    let monster_header = format!("{monster_name} HP");
    let player_header = format!("{player_name} HP");
    let unplayed: Vec<String> = rows
        .iter()
        .map(|row| format!("Not played: {}", compress_names(&row.unplayed)))
        .collect();
    let plays: Vec<String> = rows.iter().map(|row| compress_names(&row.plays)).collect();
    let widths = [
        6,
        plays
            .iter()
            .chain(unplayed.iter())
            .map(|value| value.chars().count())
            .max()
            .unwrap_or(0)
            .max("HTN plays".len())
            .max(26),
        rows.iter()
            .map(|row| row.monster_hp.to_string().len())
            .max()
            .unwrap_or(0)
            .max(monster_header.chars().count())
            .max(12),
        rows.iter()
            .map(|row| row.incoming_and_block.chars().count())
            .max()
            .unwrap_or(0)
            .max("Incoming / block".len())
            .max(20),
        rows.iter()
            .map(|row| row.player_hp.to_string().len())
            .max()
            .unwrap_or(0)
            .max(player_header.chars().count())
            .max(11),
    ];
    let headers = [
        "Turn".to_string(),
        "HTN plays".to_string(),
        monster_header,
        "Incoming / block".to_string(),
        player_header,
    ];
    let mut output = String::new();
    output.push_str("  ");
    for (column, header) in headers.iter().enumerate() {
        append_cell(&mut output, header, widths[column], column >= 2);
        if column + 1 < headers.len() {
            output.push_str("  ");
        }
    }
    output.push('\n');
    output.push_str("  ");
    for (column, width) in widths.iter().enumerate() {
        for _ in 0..*width {
            output.push('━');
        }
        if column + 1 < widths.len() {
            output.push_str("  ");
        }
    }
    output.push('\n');
    for (index, row) in rows.iter().enumerate() {
        output.push_str("  ");
        append_cell(&mut output, &row.turn.to_string(), widths[0], true);
        output.push_str("  ");
        append_cell(&mut output, &plays[index], widths[1], false);
        output.push_str("  ");
        append_cell(&mut output, &row.monster_hp.to_string(), widths[2], true);
        output.push_str("  ");
        append_cell(&mut output, &row.incoming_and_block, widths[3], true);
        output.push_str("  ");
        append_cell(&mut output, &row.player_hp.to_string(), widths[4], true);
        output.push('\n');

        output.push_str("  ");
        append_cell(&mut output, "", widths[0], false);
        output.push_str("  ");
        append_cell(&mut output, &unplayed[index], widths[1], false);
        for width in &widths[2..] {
            output.push_str("  ");
            append_cell(&mut output, "", *width, false);
        }
        output.push('\n');

        if index + 1 < rows.len() {
            output.push_str("  ");
            for (column, width) in widths.iter().enumerate() {
                for _ in 0..*width {
                    output.push('─');
                }
                if column + 1 < widths.len() {
                    output.push_str("  ");
                }
            }
            output.push('\n');
        }
    }
    output
}

fn render_floor_summary(floors: &[FloorSummary]) -> String {
    let room_and_mobs: Vec<String> = floors
        .iter()
        .map(|floor| format!("{} | {}", floor.room_type, compress_names(&floor.mobs)))
        .collect();
    let headers = ["Floor", "Entry HP", "Room type | mobs", "Finish HP"];
    let widths = [
        floors
            .iter()
            .map(|floor| floor.floor.to_string().len())
            .max()
            .unwrap_or(0)
            .max(headers[0].len()),
        floors
            .iter()
            .map(|floor| floor.entry_hp.to_string().len())
            .max()
            .unwrap_or(0)
            .max(headers[1].len()),
        room_and_mobs
            .iter()
            .map(|value| value.chars().count())
            .max()
            .unwrap_or(0)
            .max(headers[2].len())
            .max(24),
        floors
            .iter()
            .map(|floor| floor.finish_hp.to_string().len())
            .max()
            .unwrap_or(0)
            .max(headers[3].len()),
    ];
    let mut output = String::new();
    output.push_str("  ");
    for (column, header) in headers.iter().enumerate() {
        append_cell(&mut output, header, widths[column], column != 2);
        if column + 1 < headers.len() {
            output.push_str("  ");
        }
    }
    output.push('\n');
    output.push_str("  ");
    for (column, width) in widths.iter().enumerate() {
        for _ in 0..*width {
            output.push('━');
        }
        if column + 1 < widths.len() {
            output.push_str("  ");
        }
    }
    output.push('\n');
    for (index, floor) in floors.iter().enumerate() {
        output.push_str("  ");
        append_cell(&mut output, &floor.floor.to_string(), widths[0], true);
        output.push_str("  ");
        append_cell(&mut output, &floor.entry_hp.to_string(), widths[1], true);
        output.push_str("  ");
        append_cell(&mut output, &room_and_mobs[index], widths[2], false);
        output.push_str("  ");
        append_cell(&mut output, &floor.finish_hp.to_string(), widths[3], true);
        output.push('\n');
    }
    output
}

fn real_main() -> Result<(), String> {
    let Some(config) = parse_args()? else {
        println!("{}", usage());
        return Ok(());
    };
    let trace = run_trace(&config)?;
    let scope = config.floors.map_or_else(
        || "floor summary".to_string(),
        |range| format!("floors {}–{}", range.start, range.end),
    );
    println!(
        "Seed {} · {} A{} · {} · {} engine steps",
        config.seed_label,
        character_name(config.character),
        config.ascension,
        scope,
        trace.steps
    );
    if config.floors.is_none() {
        println!();
        print!("{}", render_floor_summary(&trace.floors));
        return Ok(());
    }
    for (index, fight) in trace.fights.iter().enumerate() {
        println!("\nFloor {} · {}\n", fight.floor, fight.monster_name);
        print!(
            "{}",
            render_table(
                &fight.rows,
                &fight.monster_name,
                character_name(config.character)
            )
        );
        if index + 1 < trace.fights.len() {
            println!();
        }
    }
    Ok(())
}

fn main() {
    if let Err(message) = real_main() {
        eprintln!("error: {message}\n\n{}", usage());
        std::process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_equal_plays_are_compacted() {
        assert_eq!(
            compress_names(&[
                "Defend".to_string(),
                "Strike".to_string(),
                "Strike".to_string(),
            ]),
            "Defend, Strike ×2"
        );
    }

    #[test]
    fn table_places_unplayed_cards_on_the_following_line() {
        let table = render_table(
            &[TurnRow {
                turn: 1,
                plays: vec!["Zap".to_string()],
                unplayed: vec!["Defend".to_string(), "Defend".to_string()],
                monster_hp: 3,
                incoming_and_block: "6 / 0".to_string(),
                player_hp: 40,
            }],
            "Cultist",
            "Defect",
        );
        assert!(table.contains("Zap"));
        assert!(table.contains("Not played: Defend ×2"));
    }

    #[test]
    fn diagnosed_seed_keeps_turn_two_lightning_and_wins() {
        let mut config = Config::default();
        config.floors = Some(FloorRange::new(1, 1).unwrap());
        let trace = run_trace(&config).expect("diagnosed seed should run");
        assert_eq!(trace.fights.len(), 1);
        let fight = &trace.fights[0];
        assert_eq!(fight.floor, 1);
        let turn_two = fight
            .rows
            .iter()
            .find(|row| row.turn == 2)
            .expect("turn two");
        assert_eq!(turn_two.plays, ["Defend", "Strike", "Zap"]);
        let final_turn = fight.rows.last().expect("at least one combat turn");
        assert_eq!(final_turn.monster_hp, 0);
        assert_eq!(final_turn.player_hp, 33);
    }

    #[test]
    fn time_eater_counter_eleven_does_not_trap_the_htn_in_empty_turns() {
        let seed = "1051551053542457741";
        let mut config = Config::default();
        config.seed = parse_seed(seed);
        config.seed_label = seed.to_string();
        config.ascension = 0;
        config.floors = Some(FloorRange::new(50, 50).unwrap());

        let trace = run_trace(&config).expect("Time Eater regression seed should run");
        let fight = trace.fights.first().expect("floor 50 fight");
        let final_turn = fight.rows.last().expect("at least one combat turn");

        assert_eq!(fight.monster_name, "Time Eater");
        assert_eq!(final_turn.monster_hp, 0);
        assert!(final_turn.player_hp > 0);
    }

    #[test]
    fn late_biased_cognition_converts_the_hexaghost_race() {
        let seed = "7697160996050744976";
        let mut config = Config::default();
        config.seed = parse_seed(seed);
        config.seed_label = seed.to_string();
        config.ascension = 20;
        config.floors = Some(FloorRange::new(16, 16).unwrap());

        let trace = run_trace(&config).expect("Hexaghost regression seed should run");
        let fight = trace.fights.first().expect("floor 16 fight");
        let final_turn = fight.rows.last().expect("at least one combat turn");

        assert_eq!(fight.monster_name, "Hexaghost");
        assert!(fight
            .rows
            .iter()
            .any(|row| row.plays.iter().any(|card| card == "Biased Cognition")));
        assert_eq!(final_turn.monster_hp, 0);
        assert!(final_turn.player_hp > 0);
    }

    #[test]
    fn death_with_live_combat_terminates_the_trace() {
        let seed = "12992569554709756";
        let mut config = Config::default();
        config.seed = parse_seed(seed);
        config.seed_label = seed.to_string();
        config.max_steps = 10_000;

        let trace = run_trace(&config).expect("fatal combat should terminate the trace");
        let last_floor = trace
            .floors
            .last()
            .expect("trace should include fatal floor");

        assert!(trace.steps < config.max_steps);
        // The policy may improve or regress to a different fatal fight; this
        // test protects termination while combat is still populated, not a
        // particular route or reward sequence.
        assert!(last_floor.floor > 0);
        assert_eq!(last_floor.finish_hp, 0);
    }

    #[test]
    fn floor_range_accepts_common_spellings() {
        let expected = Some(FloorRange { start: 3, end: 7 });
        assert_eq!(parse_floor_range("3-7").unwrap(), expected);
        assert_eq!(parse_floor_range("3..7").unwrap(), expected);
        assert_eq!(parse_floor_range("3:7").unwrap(), expected);
        assert_eq!(parse_floor_range("3").unwrap(), None);
    }

    #[test]
    fn floor_summary_shows_entry_room_mobs_and_finish_hp() {
        let table = render_floor_summary(&[FloorSummary {
            floor: 1,
            entry_hp: 46,
            room_type: "Monster".to_string(),
            mobs: vec!["Cultist".to_string()],
            finish_hp: 33,
        }]);
        assert!(table.contains("Entry HP"));
        assert!(table.contains("Room type | mobs"));
        assert!(table.contains("Monster | Cultist"));
        assert!(table.contains("Finish HP"));
    }
}
