use crate::action::Action;
use crate::game::{Game, Screen};
use crate::ids::{Act, Character, PotionId, RoomType};
use crate::unlocks::Unlocks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Why an episode stopped. Act 3 boss victory is deliberately distinct from
/// the game's optional Act 4 / credits flow: it is the target used by the
/// from-scratch A0 experiments.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Running,
    PlayerDeath,
    Act3BossVictory,
    StepLimit,
}

impl RunOutcome {
    pub fn done(self) -> bool {
        self != Self::Running
    }
}

/// Named auxiliary prediction targets; these are not blended into the reward.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct RunMeasurements {
    pub act: i32,
    pub floor: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub gold: i32,
    pub energy: i32,
    pub energy_master: i32,
    pub deck_size: usize,
    pub upgraded_cards: usize,
    pub distinct_cards: usize,
    pub deck_base_damage: i32,
    pub deck_base_block: i32,
    pub deck_base_magic: i32,
    pub relics: usize,
    pub potions: usize,
    pub player_power_amount: i32,
    pub hand_size: usize,
    pub draw_size: usize,
    pub discard_size: usize,
    pub exhaust_size: usize,
    pub playable_cards: usize,
    pub zero_cost_cards: usize,
    pub orb_slots: i32,
    pub filled_orbs: usize,
    pub dark_evoke: i32,
    pub combat_turn: i32,
    pub cards_played_this_turn: i32,
    pub living_enemies: usize,
    pub enemy_hp: i32,
    pub enemy_max_hp: i32,
    pub enemy_block: i32,
    pub enemy_power_amount: i32,
    pub incoming_attack: i32,
    pub legal_actions: usize,
}

impl RunMeasurements {
    pub fn from_game(game: &Game) -> Self {
        let distinct_cards = game
            .player
            .deck
            .iter()
            .map(|card| format!("{:?}", card.id))
            .collect::<BTreeSet<_>>()
            .len();
        let legal = game.legal_actions();
        let playable_cards = legal
            .iter()
            .filter(|action| matches!(action, Action::Play { .. }))
            .count();
        let zero_cost_cards = game
            .player
            .hand
            .iter()
            .filter(|card| card.cost_for_turn == 0 || card.free_to_play_once)
            .count();
        let potions = game
            .player
            .potions
            .iter()
            .filter(|potion| potion.id != PotionId::Slot)
            .count();
        let player_power_amount = game
            .player
            .powers
            .iter()
            .map(|power| power.amount.abs())
            .sum();
        let dark_evoke = game.player.orbs.iter().map(|orb| orb.evoke.max(0)).sum();

        let mut measurements = Self {
            act: game.dungeon.act as i32,
            floor: game.dungeon.floor,
            hp: game.player.hp,
            max_hp: game.player.max_hp,
            block: game.player.block,
            gold: game.player.gold,
            energy: game.player.energy,
            energy_master: game.player.energy_master,
            deck_size: game.player.deck.len(),
            upgraded_cards: game.player.deck.iter().filter(|card| card.upgraded).count(),
            distinct_cards,
            deck_base_damage: game
                .player
                .deck
                .iter()
                .map(|card| i32::from(card.base_damage.max(0)))
                .sum(),
            deck_base_block: game
                .player
                .deck
                .iter()
                .map(|card| i32::from(card.base_block.max(0)))
                .sum(),
            deck_base_magic: game
                .player
                .deck
                .iter()
                .map(|card| i32::from(card.base_magic.max(0)))
                .sum(),
            relics: game.player.relics.len(),
            potions,
            player_power_amount,
            hand_size: game.player.hand.len(),
            draw_size: game.player.draw.len(),
            discard_size: game.player.discard.len(),
            exhaust_size: game.player.exhaust.len(),
            playable_cards,
            zero_cost_cards,
            orb_slots: game.player.max_orbs,
            filled_orbs: game.player.orbs.len(),
            dark_evoke,
            legal_actions: legal.len(),
            ..Self::default()
        };

        if let Some(combat) = &game.combat {
            measurements.combat_turn = combat.turn;
            measurements.cards_played_this_turn = combat.cards_played_this_turn;
            for monster in combat
                .monsters
                .iter()
                .filter(|monster| !monster.dead && !monster.escaped)
            {
                measurements.living_enemies += 1;
                measurements.enemy_hp += monster.hp.max(0);
                measurements.enemy_max_hp += monster.max_hp.max(0);
                measurements.enemy_block += monster.block.max(0);
                measurements.enemy_power_amount += monster
                    .powers
                    .iter()
                    .map(|power| power.amount.abs())
                    .sum::<i32>();
                measurements.incoming_attack +=
                    monster.intent_damage.max(0) * monster.intent_hits.max(1);
            }
        }
        measurements
    }
}

/// Gym-style wrapper. `step` takes an index into `legal_actions()` and reset
/// retains the configured character, ascension, and episode horizon.
#[derive(Clone, Debug)]
pub struct TrainEnv {
    pub game: Game,
    character: Character,
    ascension: i32,
    steps: usize,
    max_steps: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepInfo {
    /// Sparse objective reward: +1 for the Act 3 boss, -1 for death.
    pub reward: f32,
    pub done: bool,
    pub outcome: RunOutcome,
    /// Surviving player HP on a win, negative living-monster HP on a loss.
    pub terminal_score: Option<i32>,
    pub measurements: RunMeasurements,
    pub legal: Vec<Action>,
}

/// Compact, vocabulary-free input for whole-run policy experiments. Feature
/// strings are mapped through a stable FNV-1a hash; the trainer can choose the
/// embedding table size without rebuilding engine data.
#[derive(Clone, Debug, Serialize)]
pub struct TrainingObservation {
    pub state_features: Vec<u16>,
    pub actions: Vec<TrainingAction>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrainingAction {
    pub index: usize,
    pub action: Action,
    pub features: Vec<u16>,
}

pub const TRAINING_FEATURE_BUCKETS: u64 = 32_768;

fn feature_id(token: &str) -> u16 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in token.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    // Zero remains padding in both PyTorch and the eventual native runtime.
    (hash % TRAINING_FEATURE_BUCKETS + 1) as u16
}

fn push_feature(features: &mut Vec<u16>, token: impl AsRef<str>) {
    features.push(feature_id(token.as_ref()));
}

fn bucket(value: i32) -> i32 {
    if (-64..=128).contains(&value) {
        value
    } else {
        (value / 8) * 8
    }
}

fn push_scalar(features: &mut Vec<u16>, name: &str, value: i32) {
    push_feature(features, format!("{name}={}", bucket(value)));
}

fn card_identity(card: &crate::card::Card) -> String {
    format!(
        "{:?}:u{}:c{}:d{}:b{}:m{}:x{}",
        card.id,
        u8::from(card.upgraded),
        card.cost_for_turn,
        card.base_damage,
        card.base_block,
        card.base_magic,
        card.misc,
    )
}

fn add_card_multiset(
    features: &mut Vec<u16>,
    zone: &str,
    cards: impl Iterator<Item = crate::card::Card>,
) {
    let mut counts = BTreeMap::<String, usize>::new();
    for card in cards {
        *counts.entry(card_identity(&card)).or_default() += 1;
    }
    for (card, count) in counts {
        push_feature(features, format!("CARD:{zone}:{card}:n{count}"));
    }
}

fn inventory_counts(game: &Game) -> BTreeMap<String, i32> {
    let mut counts = BTreeMap::new();
    for card in &game.player.deck {
        *counts
            .entry(format!("CARD:MASTER:{}", card_identity(card)))
            .or_default() += 1;
    }
    for relic in &game.player.relics {
        *counts.entry(format!("RELIC:{:?}", relic.id)).or_default() += 1;
    }
    for potion in game
        .player
        .potions
        .iter()
        .filter(|potion| potion.id != PotionId::Slot)
    {
        *counts.entry(format!("POTION:{:?}", potion.id)).or_default() += 1;
    }
    counts
}

fn state_features(game: &Game) -> Vec<u16> {
    let measurements = RunMeasurements::from_game(game);
    let mut features = Vec::with_capacity(256);
    push_feature(&mut features, "[STATE]");
    push_feature(&mut features, format!("SCREEN:{:?}", game.screen));
    push_feature(&mut features, format!("ROOM:{:?}", game.current_room));
    push_feature(&mut features, format!("ACT:{:?}", game.dungeon.act));
    push_feature(&mut features, format!("BOSS:{:?}", game.dungeon.boss));
    push_scalar(&mut features, "FLOOR", measurements.floor);
    push_scalar(&mut features, "HP", measurements.hp);
    push_scalar(&mut features, "MAX_HP", measurements.max_hp);
    push_scalar(&mut features, "BLOCK", measurements.block);
    push_scalar(&mut features, "GOLD", measurements.gold);
    push_scalar(&mut features, "ENERGY", measurements.energy);
    push_scalar(&mut features, "ENERGY_MASTER", measurements.energy_master);
    push_scalar(&mut features, "DECK_SIZE", measurements.deck_size as i32);
    push_scalar(
        &mut features,
        "UPGRADED_CARDS",
        measurements.upgraded_cards as i32,
    );
    push_scalar(
        &mut features,
        "DISTINCT_CARDS",
        measurements.distinct_cards as i32,
    );
    push_scalar(&mut features, "RELICS", measurements.relics as i32);
    push_scalar(&mut features, "POTIONS", measurements.potions as i32);
    push_scalar(&mut features, "HAND_SIZE", measurements.hand_size as i32);
    push_scalar(&mut features, "DRAW_SIZE", measurements.draw_size as i32);
    push_scalar(
        &mut features,
        "DISCARD_SIZE",
        measurements.discard_size as i32,
    );
    push_scalar(
        &mut features,
        "EXHAUST_SIZE",
        measurements.exhaust_size as i32,
    );
    push_scalar(&mut features, "ORB_SLOTS", measurements.orb_slots);
    push_scalar(&mut features, "DARK_EVOKE", measurements.dark_evoke);
    push_scalar(&mut features, "COMBAT_TURN", measurements.combat_turn);
    push_scalar(
        &mut features,
        "CARDS_THIS_TURN",
        measurements.cards_played_this_turn,
    );
    push_scalar(&mut features, "ENEMY_HP", measurements.enemy_hp);
    push_scalar(&mut features, "ENEMY_MAX_HP", measurements.enemy_max_hp);
    push_scalar(
        &mut features,
        "INCOMING_ATTACK",
        measurements.incoming_attack,
    );

    for (index, card) in game.player.hand.iter().enumerate() {
        push_feature(
            &mut features,
            format!("CARD:HAND:{index}:{}", card_identity(card)),
        );
    }
    add_card_multiset(&mut features, "MASTER", game.player.deck.iter().cloned());
    add_card_multiset(&mut features, "DRAW", game.player.draw.iter().cloned());
    add_card_multiset(
        &mut features,
        "DISCARD",
        game.player.discard.iter().cloned(),
    );
    add_card_multiset(
        &mut features,
        "EXHAUST",
        game.player.exhaust.iter().cloned(),
    );
    for relic in &game.player.relics {
        push_feature(&mut features, format!("RELIC:{:?}", relic.id));
        push_scalar(
            &mut features,
            &format!("RELIC:{:?}:COUNTER", relic.id),
            relic.counter,
        );
    }
    for (slot, potion) in game.player.potions.iter().enumerate() {
        if potion.id != PotionId::Slot {
            push_feature(&mut features, format!("POTION:{slot}:{:?}", potion.id));
        }
    }
    for power in &game.player.powers {
        push_feature(&mut features, format!("POWER:PLAYER:{:?}", power.id));
        push_scalar(
            &mut features,
            &format!("POWER:PLAYER:{:?}:AMOUNT", power.id),
            power.amount,
        );
    }
    for (index, orb) in game.player.orbs.iter().enumerate() {
        push_feature(&mut features, format!("ORB:{index}:{:?}", orb.kind));
        push_scalar(
            &mut features,
            &format!("ORB:{index}:{:?}:EVOKE", orb.kind),
            orb.evoke,
        );
    }

    if let Some(combat) = &game.combat {
        push_feature(&mut features, format!("ENCOUNTER:{:?}", combat.encounter));
        for (index, monster) in combat.monsters.iter().enumerate() {
            let owner = format!("MONSTER:{index}:{:?}", monster.id);
            push_feature(
                &mut features,
                format!(
                    "{owner}:INTENT:{:?}:DEAD{}:ESCAPED{}",
                    monster.intent,
                    u8::from(monster.dead),
                    u8::from(monster.escaped),
                ),
            );
            for (name, value) in [
                ("HP", monster.hp),
                ("MAX_HP", monster.max_hp),
                ("BLOCK", monster.block),
                ("INTENT_DAMAGE", monster.intent_damage),
                ("INTENT_HITS", monster.intent_hits),
                ("NEXT_MOVE", monster.next_move),
            ] {
                push_scalar(&mut features, &format!("{owner}:{name}"), value);
            }
            for power in &monster.powers {
                push_feature(&mut features, format!("POWER:{owner}:{:?}", power.id));
                push_scalar(
                    &mut features,
                    &format!("POWER:{owner}:{:?}:AMOUNT", power.id),
                    power.amount,
                );
            }
        }
    }

    if game.screen == Screen::Map {
        push_feature(
            &mut features,
            format!("MAP_CURRENT:{}:{}", game.current_x, game.current_y),
        );
        for row in &game.dungeon.map.nodes {
            for node in row {
                if let Some(room) = node.room {
                    push_feature(
                        &mut features,
                        format!(
                            "MAP_NODE:{}:{}:{room:?}:KEY{}:TAKEN{}",
                            node.x,
                            node.y,
                            u8::from(node.emerald_key),
                            u8::from(node.taken),
                        ),
                    );
                    for edge in &node.edges {
                        push_feature(
                            &mut features,
                            format!(
                                "MAP_EDGE:{}:{}:{}:{}",
                                edge.src_x, edge.src_y, edge.dst_x, edge.dst_y
                            ),
                        );
                    }
                }
            }
        }
    }
    if let Some(event) = &game.event {
        push_feature(
            &mut features,
            format!("EVENT:{:?}:S{}", event.id, event.screen),
        );
        for (index, option) in event.options.iter().enumerate() {
            push_feature(&mut features, format!("EVENT_OPTION:{index}:{option:?}"));
        }
    }
    for (index, option) in game.neow_options.iter().enumerate() {
        push_feature(
            &mut features,
            format!("NEOW_OPTION:{index}:{:?}", option.kind),
        );
    }
    for (index, reward) in game.rewards.iter().enumerate() {
        push_feature(
            &mut features,
            format!(
                "REWARD:{index}:{:?}:TAKEN{}",
                reward.kind,
                u8::from(reward.taken)
            ),
        );
    }
    for (index, card) in game.card_reward.iter().enumerate() {
        push_feature(
            &mut features,
            format!("CARD_REWARD:{index}:{}", card_identity(card)),
        );
    }
    for (index, relic) in game.boss_relics.iter().enumerate() {
        push_feature(&mut features, format!("BOSS_RELIC:{index}:{relic:?}"));
    }
    features
}

fn add_inventory_delta(features: &mut Vec<u16>, before: &Game, after: &Game) {
    let before_inventory = inventory_counts(before);
    let after_inventory = inventory_counts(after);
    let identities: BTreeSet<_> = before_inventory
        .keys()
        .chain(after_inventory.keys())
        .cloned()
        .collect();
    for identity in identities {
        let delta = after_inventory.get(&identity).copied().unwrap_or(0)
            - before_inventory.get(&identity).copied().unwrap_or(0);
        if delta != 0 {
            push_feature(features, format!("RESULT:{identity}:DELTA{delta}"));
        }
    }
    for (name, delta) in [
        ("HP", after.player.hp - before.player.hp),
        ("MAX_HP", after.player.max_hp - before.player.max_hp),
        ("GOLD", after.player.gold - before.player.gold),
    ] {
        if delta != 0 {
            push_scalar(features, &format!("RESULT:{name}:DELTA"), delta);
        }
    }
}

fn action_features(game: &Game, action: &Action) -> Vec<u16> {
    let mut features = Vec::with_capacity(16);
    push_feature(&mut features, "[ACTION]");
    push_feature(&mut features, format!("ACTION_SCREEN:{:?}", game.screen));
    match action {
        Action::Play {
            hand_index,
            target_index,
        } => {
            push_feature(&mut features, "ACTION:PLAY");
            if let Some(card) = game.player.hand.get(*hand_index) {
                push_feature(
                    &mut features,
                    format!("ACTION_CARD:{}", card_identity(card)),
                );
            }
            if let Some(target) = target_index.and_then(|index| {
                game.combat
                    .as_ref()
                    .and_then(|combat| combat.monsters.get(index))
            }) {
                push_feature(
                    &mut features,
                    format!("ACTION_TARGET:{:?}:HP{}", target.id, bucket(target.hp)),
                );
            }
        }
        Action::Choose { index, x, y, room } => {
            push_feature(&mut features, "ACTION:CHOOSE");
            push_feature(
                &mut features,
                format!("ACTION_CHOICE:{index}:X{x:?}:Y{y:?}:ROOM{room:?}"),
            );
            match game.screen {
                Screen::Neow => {
                    if let Some(option) = game.neow_options.get(*index) {
                        push_feature(&mut features, format!("CHOOSE_NEOW:{:?}", option.kind));
                    }
                }
                Screen::Event => {
                    if let Some(option) = game
                        .event
                        .as_ref()
                        .and_then(|event| event.options.get(*index))
                    {
                        push_feature(&mut features, format!("CHOOSE_EVENT:{option:?}"));
                    }
                }
                Screen::CardReward => {
                    if let Some(card) = game.card_reward.get(*index) {
                        push_feature(
                            &mut features,
                            format!("CHOOSE_CARD:{}", card_identity(card)),
                        );
                    }
                }
                Screen::BossRelic => {
                    if let Some(relic) = game.boss_relics.get(*index) {
                        push_feature(&mut features, format!("CHOOSE_RELIC:{relic:?}"));
                    }
                }
                Screen::CombatReward => {
                    if let Some(reward) = game
                        .rewards
                        .iter()
                        .filter(|reward| !reward.taken)
                        .nth(*index)
                    {
                        push_feature(&mut features, format!("CHOOSE_REWARD:{:?}", reward.kind));
                    }
                }
                Screen::HandSelect => {
                    if let Some(card) = game.player.hand.get(*index) {
                        push_feature(
                            &mut features,
                            format!("CHOOSE_HAND:{}", card_identity(card)),
                        );
                    }
                }
                Screen::Shop | Screen::Rest | Screen::Grid | Screen::Treasure => {
                    let mut after = game.clone();
                    after.step(action);
                    add_inventory_delta(&mut features, game, &after);
                }
                _ => {}
            }
        }
        Action::Potion {
            action,
            slot,
            target_index,
        } => {
            push_feature(&mut features, format!("ACTION:POTION:{action:?}"));
            if let Some(potion) = game.player.potions.get(*slot) {
                push_feature(&mut features, format!("ACTION_POTION:{:?}", potion.id));
            }
            if let Some(target) = target_index {
                push_feature(&mut features, format!("ACTION_TARGET_INDEX:{target}"));
            }
        }
        Action::EndTurn => push_feature(&mut features, "ACTION:END_TURN"),
        Action::Proceed => push_feature(&mut features, "ACTION:PROCEED"),
        Action::Skip => push_feature(&mut features, "ACTION:SKIP"),
        Action::Quit => push_feature(&mut features, "ACTION:QUIT"),
    }
    features
}

impl TrainEnv {
    pub const DEFAULT_MAX_STEPS: usize = 5_000;

    pub fn new(seed: i64) -> Self {
        Self::new_with_config(seed, Character::Ironclad, 0, Self::DEFAULT_MAX_STEPS)
    }

    pub fn new_character(seed: i64, character: Character) -> Self {
        Self::new_with_config(seed, character, 0, Self::DEFAULT_MAX_STEPS)
    }

    pub fn defect_a0(seed: i64) -> Self {
        Self::new_with_config(seed, Character::Defect, 0, Self::DEFAULT_MAX_STEPS)
    }

    pub fn new_with_config(
        seed: i64,
        character: Character,
        ascension: i32,
        max_steps: usize,
    ) -> Self {
        assert!(
            max_steps > 0,
            "training episode step limit must be positive"
        );
        Self {
            game: Game::new(seed, character, ascension, Unlocks::fixture()),
            character,
            ascension,
            steps: 0,
            max_steps,
        }
    }

    pub fn reset(&mut self, seed: i64) -> Vec<Action> {
        self.game = Game::new(seed, self.character, self.ascension, Unlocks::fixture());
        self.steps = 0;
        self.game.legal_actions()
    }

    pub fn steps(&self) -> usize {
        self.steps
    }

    pub fn max_steps(&self) -> usize {
        self.max_steps
    }

    pub fn measurements(&self) -> RunMeasurements {
        RunMeasurements::from_game(&self.game)
    }

    pub fn training_observation(&self) -> TrainingObservation {
        let legal = self.game.legal_actions();
        TrainingObservation {
            state_features: state_features(&self.game),
            actions: legal
                .into_iter()
                .enumerate()
                .map(|(index, action)| TrainingAction {
                    index,
                    features: action_features(&self.game, &action),
                    action,
                })
                .collect(),
        }
    }

    pub fn outcome(&self) -> RunOutcome {
        if self.game.player.hp <= 0 {
            return RunOutcome::PlayerDeath;
        }
        let act_three_boss_cleared = self.game.dungeon.act == Act::Beyond
            && ((self.game.current_room == RoomType::Boss
                && self.game.combat.is_none()
                && self.game.screen == Screen::CombatReward)
                || self.game.current_room == RoomType::Victory);
        if act_three_boss_cleared {
            return RunOutcome::Act3BossVictory;
        }
        if self.steps >= self.max_steps {
            return RunOutcome::StepLimit;
        }
        if self.game.done || self.game.screen == Screen::Terminal {
            return RunOutcome::PlayerDeath;
        }
        RunOutcome::Running
    }

    pub fn step(&mut self, action_index: usize) -> StepInfo {
        if self.outcome().done() {
            return self.step_info();
        }
        let legal = self.game.legal_actions();
        if let Some(action) = legal.get(action_index) {
            self.game.step(action);
            self.steps += 1;
        }
        self.step_info()
    }

    fn step_info(&self) -> StepInfo {
        let outcome = self.outcome();
        let measurements = self.measurements();
        let (reward, terminal_score) = match outcome {
            RunOutcome::Running => (0.0, None),
            RunOutcome::Act3BossVictory => (1.0, Some(self.game.player.hp.max(0))),
            RunOutcome::PlayerDeath | RunOutcome::StepLimit => {
                (-1.0, Some(-measurements.enemy_hp.max(1)))
            }
        };
        StepInfo {
            reward,
            done: outcome.done(),
            outcome,
            terminal_score,
            measurements,
            legal: if outcome.done() {
                Vec::new()
            } else {
                self.game.legal_actions()
            },
        }
    }

    /// Kept for small integrations; learners should prefer named measurements.
    pub fn compact_obs(&self) -> Vec<f32> {
        let m = self.measurements();
        vec![
            m.hp as f32,
            m.max_hp as f32,
            m.gold as f32,
            m.energy as f32,
            m.floor as f32,
            m.act as f32,
            m.deck_size as f32,
            m.hand_size as f32,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_preserves_character_and_ascension() {
        let mut env = TrainEnv::new_with_config(1, Character::Defect, 7, 123);
        env.reset(2);
        assert_eq!(env.game.character, Character::Defect);
        assert_eq!(env.game.ascension, 7);
        assert_eq!(env.max_steps(), 123);
        assert_eq!(env.steps(), 0);
    }

    #[test]
    fn defect_a0_starts_at_the_first_neow_choice() {
        let env = TrainEnv::defect_a0(1);
        assert_eq!(env.game.character, Character::Defect);
        assert_eq!(env.game.ascension, 0);
        assert_eq!(env.game.screen, Screen::Neow);
        assert!(!env.game.legal_actions().is_empty());
        assert_eq!(env.outcome(), RunOutcome::Running);
    }

    #[test]
    fn sparse_reward_does_not_claim_act_two_is_a_win() {
        let mut env = TrainEnv::defect_a0(1);
        env.game.dungeon.act = Act::City;
        let info = env.step_info();
        assert_eq!(info.reward, 0.0);
        assert!(!info.done);
        assert_eq!(info.outcome, RunOutcome::Running);
    }

    #[test]
    fn step_limit_is_terminal_without_becoming_a_victory() {
        let mut env = TrainEnv::new_with_config(1, Character::Defect, 0, 1);
        let info = env.step(0);
        assert!(info.done);
        assert_eq!(info.reward, -1.0);
        assert_eq!(info.outcome, RunOutcome::StepLimit);
        assert!(info.legal.is_empty());
    }

    #[test]
    fn training_observation_has_one_semantic_row_per_legal_action() {
        let env = TrainEnv::defect_a0(1);
        let observation = env.training_observation();
        assert!(!observation.state_features.is_empty());
        assert_eq!(observation.actions.len(), env.game.legal_actions().len());
        assert!(observation
            .actions
            .iter()
            .all(|action| !action.features.is_empty()));
    }
}
