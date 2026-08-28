use crate::action::Action;
use crate::card::Card;
use crate::game::{Game, Screen};
use crate::ids::{Act, CardId, CardType, Character, EncounterId, PotionId, RelicId, RoomType};
use crate::unlocks::Unlocks;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Why an episode stopped. A procedural fight ends at `CombatVictory`, while
/// a full run reserves `Act3BossVictory` for the external mean-floor target.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RunOutcome {
    Running,
    PlayerDeath,
    CombatVictory,
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
#[serde(default)]
pub struct RunMeasurements {
    pub act: i32,
    pub ascension: i32,
    pub floor: i32,
    /// Dispatch-only room class. This is deliberately not part of the numeric
    /// model vector, so adding it cannot change existing checkpoint inputs.
    pub elite_or_boss_combat: bool,
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
    pub deck_total_cost: i32,
    pub deck_attack_cards: usize,
    pub deck_skill_cards: usize,
    pub deck_power_cards: usize,
    pub deck_exhaust_cards: usize,
    pub deck_orb_cards: usize,
    pub deck_card_access: usize,
    pub deck_energy_cards: usize,
    pub deck_focus_cards: usize,
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

fn card_channels_or_manipulates_orbs(id: CardId) -> bool {
    matches!(
        id,
        CardId::Ball_Lightning
            | CardId::Barrage
            | CardId::Capacitor
            | CardId::Chaos
            | CardId::Chill
            | CardId::Cold_Snap
            | CardId::Compile_Driver
            | CardId::Consume
            | CardId::Coolheaded
            | CardId::Darkness
            | CardId::Doom_and_Gloom
            | CardId::Dualcast
            | CardId::Electrodynamics
            | CardId::Fission
            | CardId::Fusion
            | CardId::Glacier
            | CardId::Loop
            | CardId::Meteor_Strike
            | CardId::Multi_Cast
            | CardId::Rainbow
            | CardId::Redo
            | CardId::Static_Discharge
            | CardId::Storm
            | CardId::Tempest
            | CardId::Zap
    )
}

fn card_accesses_more_cards(id: CardId) -> bool {
    matches!(
        id,
        CardId::All_For_One
            | CardId::Compile_Driver
            | CardId::Coolheaded
            | CardId::FTL
            | CardId::Heatsinks
            | CardId::Hologram
            | CardId::Machine_Learning
            | CardId::Reboot
            | CardId::Rebound
            | CardId::Scrape
            | CardId::Seek
            | CardId::Skim
            | CardId::Steam_Power
            | CardId::Sweeping_Beam
    )
}

fn card_generates_energy(id: CardId) -> bool {
    matches!(
        id,
        CardId::Aggregate
            | CardId::Conserve_Battery
            | CardId::Double_Energy
            | CardId::Fission
            | CardId::Fusion
            | CardId::Meteor_Strike
            | CardId::Recycle
            | CardId::Turbo
    )
}

fn card_changes_focus(id: CardId) -> bool {
    matches!(
        id,
        CardId::Biased_Cognition | CardId::Consume | CardId::Defragment
    )
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
            ascension: game.ascension,
            floor: game.dungeon.floor,
            elite_or_boss_combat: game.combat.is_some()
                && matches!(game.current_room, RoomType::Elite | RoomType::Boss),
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
            deck_total_cost: game
                .player
                .deck
                .iter()
                .map(|card| i32::from(card.cost.max(0)))
                .sum(),
            deck_attack_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card.card_type() == CardType::ATTACK)
                .count(),
            deck_skill_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card.card_type() == CardType::SKILL)
                .count(),
            deck_power_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card.card_type() == CardType::POWER)
                .count(),
            deck_exhaust_cards: game.player.deck.iter().filter(|card| card.exhaust).count(),
            deck_orb_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card_channels_or_manipulates_orbs(card.id))
                .count(),
            deck_card_access: game
                .player
                .deck
                .iter()
                .filter(|card| card_accesses_more_cards(card.id))
                .count(),
            deck_energy_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card_generates_energy(card.id))
                .count(),
            deck_focus_cards: game
                .player
                .deck
                .iter()
                .filter(|card| card_changes_focus(card.id))
                .count(),
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
    goal: TrainingGoal,
    scenario: Option<ProceduralCombatScenario>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TrainingGoal {
    FullRun,
    CurrentCombat,
}

/// Which room class a procedural combat request should sample.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProceduralCombatKind {
    Elite,
    Boss,
    #[default]
    Mixed,
}

/// Deterministic request for a freshly generated Defect combat puzzle.
///
/// Omitting `act` samples uniformly from Acts 1 through 3. `kind = mixed`
/// independently samples an elite or boss. The seed controls the encounter,
/// deck, upgrades, relics, HP, relic counters, and ordinary combat RNG.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProceduralCombatSpec {
    pub seed: i64,
    #[serde(default)]
    pub act: Option<i32>,
    #[serde(default)]
    pub kind: ProceduralCombatKind,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProceduralCombatScenario {
    pub seed: i64,
    pub act: i32,
    pub floor: i32,
    pub kind: ProceduralCombatKind,
    pub encounter: EncounterId,
    pub starting_hp: i32,
    pub max_hp: i32,
    pub deck_size: usize,
    pub upgraded_cards: usize,
    pub relics: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct StepInfo {
    /// Sparse objective reward: +1 for the configured victory, -1 for failure.
    pub reward: f32,
    pub done: bool,
    pub outcome: RunOutcome,
    /// Surviving player HP on a win, negative living-monster HP on a loss.
    pub terminal_score: Option<i32>,
    pub measurements: RunMeasurements,
    pub legal: Vec<Action>,
}

/// Minimal transition result for native rollout policies that inspect the
/// environment directly. Unlike [`StepInfo`], this does not scan the deck,
/// construct measurements, or enumerate the next action menu.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
pub struct CompactStepInfo {
    pub outcome: RunOutcome,
    /// Surviving player HP on a win, negative living-monster HP on a loss.
    pub terminal_score: Option<i32>,
}

/// Compact, vocabulary-free input for whole-run policy experiments. Feature
/// strings are mapped through a stable FNV-1a hash; the trainer can choose the
/// embedding table size without rebuilding engine data.
#[derive(Clone, Debug, Serialize)]
pub struct TrainingObservation {
    pub state_features: Vec<u16>,
    /// Shared card/relic identity tokens, repeated once per owned copy. These
    /// are kept separate from state_features so older checkpoints retain
    /// bit-exact inputs while relational models can join inventory to a
    /// candidate action without reversing the feature hash.
    pub inventory_identities: Vec<u16>,
    pub actions: Vec<TrainingAction>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrainingAction {
    pub index: usize,
    pub action: Action,
    pub features: Vec<u16>,
    /// Identity tokens drawn from the same namespace as inventory_identities.
    pub candidate_identities: Vec<u16>,
    /// Deterministic, candidate-specific resource changes. These remain raw
    /// numeric parameters so adjacent costs (for example 10 versus 11 HP)
    /// share structure instead of becoming unrelated hashed feature IDs.
    pub parameters: ActionParameters,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct ActionParameters {
    /// False only when cloning the action would cross an intentionally opaque
    /// room transition.
    pub known: bool,
    pub hp_delta: i32,
    pub max_hp_delta: i32,
    pub enemy_hp_delta: i32,
    pub block_delta: i32,
    pub enemy_block_delta: i32,
    pub energy_delta: i32,
    pub gold_delta: i32,
    pub hand_delta: i32,
    pub draw_delta: i32,
    pub discard_delta: i32,
    pub exhaust_delta: i32,
    pub deck_size_delta: i32,
    pub upgraded_cards_delta: i32,
    pub relic_delta: i32,
    pub potion_delta: i32,
    pub orb_slots_delta: i32,
    pub filled_orbs_delta: i32,
    pub orb_evoke_delta: i32,
    pub incoming_attack_delta: i32,
    pub living_enemies_delta: i32,
    pub turn_delta: i32,
    pub cards_played_delta: i32,
    pub player_power_delta: i32,
    pub enemy_power_delta: i32,
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

fn card_identity_tokens(card: &crate::card::Card) -> [String; 2] {
    [
        format!("IDENTITY:CARD:{:?}", card.id),
        format!("IDENTITY:CARD_EXACT:{}", card_identity(card)),
    ]
}

fn inventory_identity_tokens(game: &Game) -> Vec<String> {
    let mut tokens = Vec::with_capacity(game.player.deck.len() * 2 + game.player.relics.len());
    for card in &game.player.deck {
        tokens.extend(card_identity_tokens(card));
    }
    for relic in &game.player.relics {
        tokens.push(format!("IDENTITY:RELIC:{:?}", relic.id));
    }
    for potion in game
        .player
        .potions
        .iter()
        .filter(|potion| potion.id != PotionId::Slot)
    {
        tokens.push(format!("IDENTITY:POTION:{:?}", potion.id));
    }
    tokens
}

fn inventory_identity_features(game: &Game) -> Vec<u16> {
    inventory_identity_tokens(game)
        .into_iter()
        .map(|token| feature_id(&token))
        .collect()
}

fn add_card_identities(features: &mut Vec<u16>, card: &crate::card::Card) {
    for token in card_identity_tokens(card) {
        push_feature(features, token);
    }
}

fn add_identity_delta(features: &mut Vec<u16>, before: &Game, after: &Game) {
    let mut before_counts = BTreeMap::<String, i32>::new();
    let mut after_counts = BTreeMap::<String, i32>::new();
    for token in inventory_identity_tokens(before) {
        *before_counts.entry(token).or_default() += 1;
    }
    for token in inventory_identity_tokens(after) {
        *after_counts.entry(token).or_default() += 1;
    }
    let identities: BTreeSet<_> = before_counts
        .keys()
        .chain(after_counts.keys())
        .cloned()
        .collect();
    for identity in identities {
        let delta = after_counts.get(&identity).copied().unwrap_or(0)
            - before_counts.get(&identity).copied().unwrap_or(0);
        for _ in 0..delta.unsigned_abs() {
            push_feature(features, &identity);
        }
    }
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
    push_scalar(&mut features, "ASCENSION", measurements.ascension);
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

fn occupied_potion_slots(game: &Game) -> i32 {
    game.player
        .potions
        .iter()
        .filter(|potion| potion.id != PotionId::Slot)
        .count() as i32
}

#[derive(Clone, Copy)]
struct ActionSnapshot {
    hp: i32,
    max_hp: i32,
    block: i32,
    energy: i32,
    gold: i32,
    hand: i32,
    draw: i32,
    discard: i32,
    exhaust: i32,
    deck_size: i32,
    upgraded_cards: i32,
    relics: i32,
    potions: i32,
    orb_slots: i32,
    filled_orbs: i32,
    dark_evoke: i32,
    player_power: i32,
    turn: i32,
    cards_played: i32,
    living_enemies: i32,
    enemy_hp: i32,
    enemy_block: i32,
    enemy_power: i32,
    incoming_attack: i32,
}

impl ActionSnapshot {
    fn from_game(game: &Game) -> Self {
        let mut snapshot = Self {
            hp: game.player.hp,
            max_hp: game.player.max_hp,
            block: game.player.block,
            energy: game.player.energy,
            gold: game.player.gold,
            hand: game.player.hand.len() as i32,
            draw: game.player.draw.len() as i32,
            discard: game.player.discard.len() as i32,
            exhaust: game.player.exhaust.len() as i32,
            deck_size: game.player.deck.len() as i32,
            upgraded_cards: game.player.deck.iter().filter(|card| card.upgraded).count() as i32,
            relics: game.player.relics.len() as i32,
            potions: occupied_potion_slots(game),
            orb_slots: game.player.max_orbs,
            filled_orbs: game.player.orbs.len() as i32,
            dark_evoke: game.player.orbs.iter().map(|orb| orb.evoke.max(0)).sum(),
            player_power: game
                .player
                .powers
                .iter()
                .map(|power| power.amount.abs())
                .sum(),
            turn: 0,
            cards_played: 0,
            living_enemies: 0,
            enemy_hp: 0,
            enemy_block: 0,
            enemy_power: 0,
            incoming_attack: 0,
        };
        if let Some(combat) = &game.combat {
            snapshot.turn = combat.turn;
            snapshot.cards_played = combat.cards_played_this_turn;
            for monster in combat
                .monsters
                .iter()
                .filter(|monster| !monster.dead && !monster.escaped)
            {
                snapshot.living_enemies += 1;
                snapshot.enemy_hp += monster.hp.max(0);
                snapshot.enemy_block += monster.block.max(0);
                snapshot.enemy_power += monster
                    .powers
                    .iter()
                    .map(|power| power.amount.abs())
                    .sum::<i32>();
                snapshot.incoming_attack +=
                    monster.intent_damage.max(0) * monster.intent_hits.max(1);
            }
        }
        snapshot
    }
}

fn action_parameters(game: &Game, action: &Action) -> ActionParameters {
    let parameterized_screen = matches!(
        game.screen,
        Screen::Neow
            | Screen::CombatReward
            | Screen::CardReward
            | Screen::Rest
            | Screen::Treasure
            | Screen::BossRelic
            | Screen::Event
            | Screen::Shop
            | Screen::Grid
    );
    let combat_action = game.combat.is_some() && !matches!(action, Action::Quit);
    let noncombat_action = parameterized_screen
        && matches!(
            action,
            Action::Choose { .. } | Action::Skip | Action::Potion { .. }
        );
    if !combat_action && !noncombat_action {
        return ActionParameters::default();
    }

    let before = ActionSnapshot::from_game(game);
    let mut after_game = game.clone();
    after_game.step(action);
    let after = ActionSnapshot::from_game(&after_game);
    ActionParameters {
        known: true,
        hp_delta: after.hp - before.hp,
        max_hp_delta: after.max_hp - before.max_hp,
        enemy_hp_delta: after.enemy_hp - before.enemy_hp,
        block_delta: after.block - before.block,
        enemy_block_delta: after.enemy_block - before.enemy_block,
        energy_delta: after.energy - before.energy,
        gold_delta: after.gold - before.gold,
        hand_delta: after.hand - before.hand,
        draw_delta: after.draw - before.draw,
        discard_delta: after.discard - before.discard,
        exhaust_delta: after.exhaust - before.exhaust,
        deck_size_delta: after.deck_size - before.deck_size,
        upgraded_cards_delta: after.upgraded_cards - before.upgraded_cards,
        relic_delta: after.relics - before.relics,
        potion_delta: after.potions - before.potions,
        orb_slots_delta: after.orb_slots - before.orb_slots,
        filled_orbs_delta: after.filled_orbs - before.filled_orbs,
        orb_evoke_delta: after.dark_evoke - before.dark_evoke,
        incoming_attack_delta: after.incoming_attack - before.incoming_attack,
        living_enemies_delta: after.living_enemies - before.living_enemies,
        turn_delta: after.turn - before.turn,
        cards_played_delta: after.cards_played - before.cards_played,
        player_power_delta: after.player_power - before.player_power,
        enemy_power_delta: after.enemy_power - before.enemy_power,
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

fn action_identity_features(game: &Game, action: &Action) -> Vec<u16> {
    let mut features = Vec::with_capacity(8);
    match action {
        Action::Play { hand_index, .. } => {
            if let Some(card) = game.player.hand.get(*hand_index) {
                add_card_identities(&mut features, card);
            }
        }
        Action::Choose { index, .. } => match game.screen {
            Screen::CardReward => {
                if let Some(card) = game.card_reward.get(*index) {
                    add_card_identities(&mut features, card);
                }
            }
            Screen::BossRelic => {
                if let Some(relic) = game.boss_relics.get(*index) {
                    push_feature(&mut features, format!("IDENTITY:RELIC:{relic:?}"));
                }
            }
            Screen::HandSelect => {
                if let Some(card) = game.player.hand.get(*index) {
                    add_card_identities(&mut features, card);
                }
            }
            Screen::Grid => {
                let mut after = game.clone();
                after.step(action);
                let before = features.len();
                add_identity_delta(&mut features, game, &after);
                if features.len() == before {
                    if let Some((_, cards)) = game.grid_view() {
                        if let Some((_, card)) =
                            cards.into_iter().find(|(choice, _)| choice == index)
                        {
                            add_card_identities(&mut features, card);
                        }
                    }
                }
            }
            Screen::Rest if game.rest_is_smithing() => {
                if let Some(card) = game
                    .player
                    .deck
                    .iter()
                    .filter(|card| card.can_upgrade())
                    .nth(*index)
                {
                    add_card_identities(&mut features, card);
                }
            }
            Screen::CombatReward | Screen::Shop | Screen::Rest | Screen::Treasure => {
                let mut after = game.clone();
                after.step(action);
                add_identity_delta(&mut features, game, &after);
            }
            _ => {}
        },
        Action::Potion { slot, .. } => {
            if let Some(potion) = game.player.potions.get(*slot) {
                push_feature(&mut features, format!("IDENTITY:POTION:{:?}", potion.id));
            }
        }
        Action::Proceed if matches!(game.screen, Screen::Grid | Screen::Rest) => {
            let mut after = game.clone();
            after.step(action);
            add_identity_delta(&mut features, game, &after);
        }
        Action::EndTurn | Action::Proceed | Action::Skip | Action::Quit => {}
    }
    features
}

#[derive(Clone, Copy, Debug)]
struct ProceduralRng(u64);

impl ProceduralRng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D049BB133111EB);
        value ^ (value >> 31)
    }

    fn index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0);
        (self.next() as usize) % len
    }

    fn inclusive(&mut self, low: usize, high: usize) -> usize {
        debug_assert!(low <= high);
        low + self.index(high - low + 1)
    }

    fn chance(&mut self, numerator: usize, denominator: usize) -> bool {
        self.index(denominator) < numerator
    }
}

#[derive(Clone, Copy, Debug)]
enum DeckTheme {
    Balanced,
    Orbs,
    CardAccess,
    Energy,
    Focus,
    ZeroCost,
    Powers,
}

fn card_matches_theme(id: CardId, theme: DeckTheme) -> bool {
    match theme {
        DeckTheme::Balanced => true,
        DeckTheme::Orbs => card_channels_or_manipulates_orbs(id),
        DeckTheme::CardAccess => card_accesses_more_cards(id),
        DeckTheme::Energy => card_generates_energy(id),
        DeckTheme::Focus => card_changes_focus(id) || card_channels_or_manipulates_orbs(id),
        DeckTheme::ZeroCost => Card::new(id).cost == 0,
        DeckTheme::Powers => Card::new(id).card_type() == CardType::POWER,
    }
}

fn procedural_encounters(act: i32, kind: ProceduralCombatKind) -> &'static [EncounterId] {
    match (act, kind) {
        (1, ProceduralCombatKind::Elite) => &[
            EncounterId::GremlinNob,
            EncounterId::Lagavulin,
            EncounterId::ThreeSentries,
        ],
        (1, ProceduralCombatKind::Boss) => &[
            EncounterId::Hexaghost,
            EncounterId::TheGuardian,
            EncounterId::SlimeBoss,
        ],
        (2, ProceduralCombatKind::Elite) => &[
            EncounterId::BookOfStabbing,
            EncounterId::Slavers,
            EncounterId::GremlinLeader,
        ],
        (2, ProceduralCombatKind::Boss) => &[
            EncounterId::Automaton,
            EncounterId::Champ,
            EncounterId::Collector,
        ],
        (3, ProceduralCombatKind::Elite) => &[
            EncounterId::GiantHead,
            EncounterId::Nemesis,
            EncounterId::Reptomancer,
        ],
        (3, ProceduralCombatKind::Boss) => &[
            EncounterId::DonuAndDeca,
            EncounterId::AwakenedOne,
            EncounterId::TimeEater,
        ],
        _ => unreachable!("procedural act and room kind are normalized"),
    }
}

fn procedural_floor(act: i32, kind: ProceduralCombatKind, rng: &mut ProceduralRng) -> i32 {
    match (act, kind) {
        (1, ProceduralCombatKind::Elite) => rng.inclusive(6, 14) as i32,
        (1, ProceduralCombatKind::Boss) => 16,
        (2, ProceduralCombatKind::Elite) => rng.inclusive(23, 31) as i32,
        (2, ProceduralCombatKind::Boss) => 33,
        (3, ProceduralCombatKind::Elite) => rng.inclusive(40, 48) as i32,
        (3, ProceduralCombatKind::Boss) => 50,
        _ => unreachable!("procedural act and room kind are normalized"),
    }
}

fn added_card_range(act: i32, kind: ProceduralCombatKind) -> (usize, usize) {
    match (act, kind) {
        (1, ProceduralCombatKind::Elite) => (2, 8),
        (1, ProceduralCombatKind::Boss) => (5, 12),
        (2, ProceduralCombatKind::Elite) => (8, 16),
        (2, ProceduralCombatKind::Boss) => (11, 21),
        (3, ProceduralCombatKind::Elite) => (14, 25),
        (3, ProceduralCombatKind::Boss) => (18, 30),
        _ => unreachable!("procedural act and room kind are normalized"),
    }
}

fn relic_range(act: i32, kind: ProceduralCombatKind) -> (usize, usize) {
    match (act, kind) {
        (1, ProceduralCombatKind::Elite) => (1, 4),
        (1, ProceduralCombatKind::Boss) => (2, 6),
        (2, ProceduralCombatKind::Elite) => (4, 9),
        (2, ProceduralCombatKind::Boss) => (6, 11),
        (3, ProceduralCombatKind::Elite) => (8, 14),
        (3, ProceduralCombatKind::Boss) => (10, 17),
        _ => unreachable!("procedural act and room kind are normalized"),
    }
}

fn acquisition_safe_training_relic(id: RelicId) -> bool {
    !matches!(
        id,
        RelicId::Astrolabe
            | RelicId::Bottled_Flame
            | RelicId::Bottled_Lightning
            | RelicId::Bottled_Tornado
            | RelicId::Calling_Bell
            | RelicId::Cauldron
            | RelicId::DollysMirror
            | RelicId::Empty_Cage
            | RelicId::Orrery
            | RelicId::Pandoras_Box
            | RelicId::Tiny_House
    )
}

fn randomize_training_relic_counters(game: &mut Game, rng: &mut ProceduralRng) {
    for relic in &mut game.player.relics {
        relic.counter = match relic.id {
            RelicId::Happy_Flower => rng.inclusive(0, 2) as i32,
            RelicId::Pen_Nib | RelicId::InkBottle | RelicId::Nunchaku => rng.inclusive(0, 9) as i32,
            RelicId::Incense_Burner => rng.inclusive(0, 5) as i32,
            RelicId::Sundial => rng.inclusive(0, 2) as i32,
            RelicId::Inserter => rng.inclusive(0, 1) as i32,
            RelicId::Girya => rng.inclusive(0, 3) as i32,
            _ => relic.counter,
        };
    }
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
            goal: TrainingGoal::FullRun,
            scenario: None,
        }
    }

    /// Build a deterministic but effectively unbounded combat curriculum row.
    ///
    /// The generator samples only from real Defect card/relic pools and starts
    /// the ordinary combat engine, so subsequent actions and RNG semantics are
    /// identical to full-run play. It intentionally varies both coherent deck
    /// mechanisms and off-theme cards instead of assigning standalone card
    /// values.
    pub fn procedural_defect_combat(
        spec: ProceduralCombatSpec,
        ascension: i32,
        max_steps: usize,
    ) -> Result<Self, String> {
        if !(0..=20).contains(&ascension) {
            return Err("procedural combat ascension must be between 0 and 20".to_string());
        }
        if max_steps == 0 {
            return Err("procedural combat step limit must be positive".to_string());
        }
        if spec.act.is_some_and(|act| !(1..=3).contains(&act)) {
            return Err("procedural combat act must be 1, 2, or 3".to_string());
        }

        let mut scenario_rng = ProceduralRng(spec.seed as u64 ^ 0xC04B_A771_5EED_5EED);
        let act_number = spec
            .act
            .unwrap_or_else(|| scenario_rng.inclusive(1, 3) as i32);
        let kind = match spec.kind {
            ProceduralCombatKind::Mixed => {
                if scenario_rng.chance(1, 2) {
                    ProceduralCombatKind::Elite
                } else {
                    ProceduralCombatKind::Boss
                }
            }
            kind => kind,
        };
        let encounter_pool = procedural_encounters(act_number, kind);
        let encounter = encounter_pool[scenario_rng.index(encounter_pool.len())];
        let floor = procedural_floor(act_number, kind, &mut scenario_rng);
        let act = match act_number {
            1 => Act::Exordium,
            2 => Act::City,
            3 => Act::Beyond,
            _ => unreachable!("act was validated"),
        };

        let mut game = Game::new(spec.seed, Character::Defect, ascension, Unlocks::fixture());
        game.dungeon.act = act;
        game.dungeon.floor = floor;
        game.dungeon.boss = encounter;
        game.current_room = match kind {
            ProceduralCombatKind::Elite => RoomType::Elite,
            ProceduralCombatKind::Boss => RoomType::Boss,
            ProceduralCombatKind::Mixed => unreachable!("room kind was normalized"),
        };

        let starter_removals = scenario_rng.inclusive(0, (act_number as usize) * 2 + 1);
        for _ in 0..starter_removals {
            let removable = game
                .player
                .deck
                .iter()
                .enumerate()
                .filter_map(|(index, card)| {
                    matches!(card.id, CardId::Strike_B | CardId::Defend_B).then_some(index)
                })
                .collect::<Vec<_>>();
            if removable.is_empty() {
                break;
            }
            let index = removable[scenario_rng.index(removable.len())];
            game.player.deck.remove(index);
        }

        let themes = [
            DeckTheme::Balanced,
            DeckTheme::Orbs,
            DeckTheme::CardAccess,
            DeckTheme::Energy,
            DeckTheme::Focus,
            DeckTheme::ZeroCost,
            DeckTheme::Powers,
        ];
        let theme = themes[scenario_rng.index(themes.len())];
        let all_cards = game
            .dungeon
            .common_cards
            .iter()
            .chain(game.dungeon.uncommon_cards.iter())
            .chain(game.dungeon.rare_cards.iter())
            .copied()
            .collect::<Vec<_>>();
        let themed_cards = all_cards
            .iter()
            .copied()
            .filter(|id| card_matches_theme(*id, theme))
            .collect::<Vec<_>>();
        let (minimum_cards, maximum_cards) = added_card_range(act_number, kind);
        let added_cards = scenario_rng.inclusive(minimum_cards, maximum_cards);
        let upgrade_chance = match act_number {
            1 => 12,
            2 => 24,
            3 => 36,
            _ => unreachable!(),
        };
        for _ in 0..added_cards {
            let rarity_roll = scenario_rng.index(100);
            let rarity_pool = if rarity_roll < 54 {
                game.dungeon.common_cards.as_ref()
            } else if rarity_roll < 89 {
                game.dungeon.uncommon_cards.as_ref()
            } else {
                game.dungeon.rare_cards.as_ref()
            };
            let themed_rarity = rarity_pool
                .iter()
                .copied()
                .filter(|id| card_matches_theme(*id, theme))
                .collect::<Vec<_>>();
            let pool = if scenario_rng.chance(2, 3) && !themed_rarity.is_empty() {
                &themed_rarity
            } else if scenario_rng.chance(1, 5) && !themed_cards.is_empty() {
                &themed_cards
            } else {
                rarity_pool
            };
            let mut id = pool[scenario_rng.index(pool.len())];
            for _ in 0..8 {
                if game.player.deck.iter().filter(|card| card.id == id).count() < 4 {
                    break;
                }
                id = pool[scenario_rng.index(pool.len())];
            }
            let mut card = Card::new(id);
            if card.can_upgrade() && scenario_rng.chance(upgrade_chance, 100) {
                card.upgrade();
            }
            game.player.deck.push(card);
        }

        let starter_upgrade_chance = match act_number {
            1 => 5,
            2 => 12,
            3 => 20,
            _ => unreachable!(),
        };
        for card in &mut game.player.deck {
            if card.can_upgrade() && scenario_rng.chance(starter_upgrade_chance, 100) {
                card.upgrade();
            }
        }

        let ordinary_relics = game
            .dungeon
            .common_relics
            .iter()
            .chain(game.dungeon.uncommon_relics.iter())
            .chain(game.dungeon.rare_relics.iter())
            .chain(game.dungeon.shop_relics.iter())
            .copied()
            .filter(|id| acquisition_safe_training_relic(*id))
            .collect::<Vec<_>>();
        let boss_relics = game
            .dungeon
            .boss_relics
            .iter()
            .copied()
            .filter(|id| acquisition_safe_training_relic(*id))
            .collect::<Vec<_>>();
        for _ in 0..act_number.saturating_sub(1) {
            for _ in 0..16 {
                let id = boss_relics[scenario_rng.index(boss_relics.len())];
                if !game.player.has_relic(id) {
                    game.gain_training_relic(id);
                    break;
                }
            }
        }
        let (minimum_relics, maximum_relics) = relic_range(act_number, kind);
        let target_relics = scenario_rng.inclusive(minimum_relics, maximum_relics);
        while game.player.relics.len() < target_relics {
            let id = ordinary_relics[scenario_rng.index(ordinary_relics.len())];
            if !game.player.has_relic(id) {
                game.gain_training_relic(id);
            }
        }
        randomize_training_relic_counters(&mut game, &mut scenario_rng);

        let (minimum_max_hp, maximum_max_hp) = match act_number {
            1 => (55, 82),
            2 => (48, 90),
            3 => (42, 96),
            _ => unreachable!(),
        };
        game.player.max_hp = scenario_rng.inclusive(minimum_max_hp, maximum_max_hp) as i32;
        game.player.hp = scenario_rng.inclusive(
            (game.player.max_hp as usize / 4).max(1),
            game.player.max_hp as usize,
        ) as i32;
        game.player.gold = scenario_rng.inclusive(0, 450) as i32;
        game.start_training_combat(encounter);

        let scenario = ProceduralCombatScenario {
            seed: spec.seed,
            act: act_number,
            floor,
            kind,
            encounter,
            starting_hp: game.player.hp,
            max_hp: game.player.max_hp,
            deck_size: game.player.deck.len(),
            upgraded_cards: game.player.deck.iter().filter(|card| card.upgraded).count(),
            relics: game.player.relics.len(),
        };
        Ok(Self {
            game,
            character: Character::Defect,
            ascension,
            steps: 0,
            max_steps,
            goal: TrainingGoal::CurrentCombat,
            scenario: Some(scenario),
        })
    }

    pub fn reset(&mut self, seed: i64) -> Vec<Action> {
        self.game = Game::new(seed, self.character, self.ascension, Unlocks::fixture());
        self.steps = 0;
        self.goal = TrainingGoal::FullRun;
        self.scenario = None;
        self.game.legal_actions()
    }

    pub fn scenario(&self) -> Option<&ProceduralCombatScenario> {
        self.scenario.as_ref()
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
        let mut actions: Vec<_> = legal
            .into_iter()
            .enumerate()
            .map(|(index, action)| TrainingAction {
                index,
                candidate_identities: action_identity_features(&self.game, &action),
                features: action_features(&self.game, &action),
                parameters: action_parameters(&self.game, &action),
                action,
            })
            .collect();
        if self.game.combat.is_none()
            && actions
                .iter()
                .any(|action| !action.candidate_identities.is_empty())
        {
            for action in &mut actions {
                if action.candidate_identities.is_empty() {
                    push_feature(&mut action.candidate_identities, "IDENTITY:CHOICE_NONE");
                }
            }
        }
        TrainingObservation {
            state_features: state_features(&self.game),
            inventory_identities: inventory_identity_features(&self.game),
            actions,
        }
    }

    pub fn outcome(&self) -> RunOutcome {
        if self.game.player.hp <= 0 {
            return RunOutcome::PlayerDeath;
        }
        if self.goal == TrainingGoal::CurrentCombat && self.game.combat.is_none() {
            return RunOutcome::CombatVictory;
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

    /// Apply one action without constructing the observation data required by
    /// protocol clients. This is the branch-rollout hot path for the in-process
    /// Rust trainer and has the same state-transition semantics as [`Self::step`].
    pub fn step_compact(&mut self, action_index: usize) -> CompactStepInfo {
        if !self.outcome().done() {
            let legal = self.game.legal_actions();
            if let Some(action) = legal.get(action_index) {
                self.game.step(action);
                self.steps += 1;
            }
        }
        let outcome = self.outcome();
        let terminal_score = self.terminal_score(outcome);
        CompactStepInfo {
            outcome,
            terminal_score,
        }
    }

    fn step_info(&self) -> StepInfo {
        let outcome = self.outcome();
        let measurements = self.measurements();
        let reward = match outcome {
            RunOutcome::Running => 0.0,
            RunOutcome::CombatVictory | RunOutcome::Act3BossVictory => 1.0,
            RunOutcome::PlayerDeath | RunOutcome::StepLimit => -1.0,
        };
        let terminal_score = self.terminal_score(outcome);
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

    fn terminal_score(&self, outcome: RunOutcome) -> Option<i32> {
        match outcome {
            RunOutcome::Running => None,
            RunOutcome::CombatVictory | RunOutcome::Act3BossVictory => {
                Some(self.game.player.hp.max(0))
            }
            RunOutcome::PlayerDeath | RunOutcome::StepLimit => {
                let enemy_hp = self
                    .game
                    .combat
                    .as_ref()
                    .map(|combat| {
                        combat
                            .monsters
                            .iter()
                            .filter(|monster| !monster.dead && !monster.escaped)
                            .map(|monster| monster.hp.max(0))
                            .sum::<i32>()
                    })
                    .unwrap_or(0);
                Some(-enemy_hp.max(1))
            }
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
    use crate::card::Card;

    #[test]
    fn reset_preserves_character_and_ascension() {
        let mut env = TrainEnv::new_with_config(1, Character::Defect, 7, 123);
        env.reset(2);
        assert_eq!(env.game.character, Character::Defect);
        assert_eq!(env.game.ascension, 7);
        assert_eq!(env.measurements().ascension, 7);
        assert_eq!(env.max_steps(), 123);
        assert_eq!(env.steps(), 0);
    }

    #[test]
    fn procedural_combat_is_deterministic_and_uses_real_combat_setup() {
        let spec = ProceduralCombatSpec {
            seed: 0x1234_5678,
            act: Some(3),
            kind: ProceduralCombatKind::Boss,
        };
        let first = TrainEnv::procedural_defect_combat(spec, 20, 500).unwrap();
        let second = TrainEnv::procedural_defect_combat(spec, 20, 500).unwrap();
        assert_eq!(first.scenario(), second.scenario());
        assert_eq!(
            first.training_observation().state_features,
            second.training_observation().state_features
        );
        assert_eq!(
            first.training_observation().inventory_identities,
            second.training_observation().inventory_identities
        );
        assert_eq!(first.game.current_room, RoomType::Boss);
        assert_eq!(first.game.dungeon.act, Act::Beyond);
        assert_eq!(first.game.dungeon.floor, 50);
        assert!(first.game.combat.is_some());
        assert!(first.game.player.deck.len() >= 20);
        assert!(first.game.player.relics.len() >= 10);
        assert_eq!(first.outcome(), RunOutcome::Running);
    }

    #[test]
    fn procedural_curriculum_covers_every_elite_and_boss() {
        for act in 1..=3 {
            for kind in [ProceduralCombatKind::Elite, ProceduralCombatKind::Boss] {
                let encounters = (0..128)
                    .map(|seed| {
                        let env = TrainEnv::procedural_defect_combat(
                            ProceduralCombatSpec {
                                seed,
                                act: Some(act),
                                kind,
                            },
                            20,
                            500,
                        )
                        .unwrap();
                        format!("{:?}", env.scenario().unwrap().encounter)
                    })
                    .collect::<BTreeSet<_>>();
                assert_eq!(encounters.len(), 3, "act {act} {kind:?}");
            }
        }
    }

    #[test]
    fn procedural_combat_victory_uses_surviving_hp_margin() {
        let mut env = TrainEnv::procedural_defect_combat(
            ProceduralCombatSpec {
                seed: 77,
                act: Some(1),
                kind: ProceduralCombatKind::Elite,
            },
            20,
            500,
        )
        .unwrap();
        env.game.combat = None;
        env.game.screen = Screen::CombatReward;
        let info = env.step_info();
        let compact = env.step_compact(0);
        assert_eq!(info.outcome, RunOutcome::CombatVictory);
        assert_eq!(info.reward, 1.0);
        assert_eq!(info.terminal_score, Some(env.game.player.hp));
        assert!(info.done);
        assert!(info.legal.is_empty());
        assert_eq!(compact.outcome, info.outcome);
        assert_eq!(compact.terminal_score, info.terminal_score);
    }

    #[test]
    fn full_run_reset_leaves_the_procedural_goal() {
        let mut env = TrainEnv::procedural_defect_combat(
            ProceduralCombatSpec {
                seed: 9,
                act: None,
                kind: ProceduralCombatKind::Mixed,
            },
            20,
            500,
        )
        .unwrap();
        assert!(env.scenario().is_some());
        env.reset(10);
        assert!(env.scenario().is_none());
        assert_eq!(env.game.screen, Screen::Neow);
        assert_eq!(env.outcome(), RunOutcome::Running);
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
        let mut compact_env = env.clone();
        let info = env.step(0);
        let compact = compact_env.step_compact(0);
        assert!(info.done);
        assert_eq!(info.reward, -1.0);
        assert_eq!(info.outcome, RunOutcome::StepLimit);
        assert!(info.legal.is_empty());
        assert_eq!(compact.outcome, info.outcome);
        assert_eq!(compact.terminal_score, info.terminal_score);
        assert_eq!(format!("{:?}", compact_env.game), format!("{:?}", env.game));
    }

    #[test]
    fn training_observation_has_one_semantic_row_per_legal_action() {
        let env = TrainEnv::defect_a0(1);
        let observation = env.training_observation();
        assert!(!observation.state_features.is_empty());
        assert!(!observation.inventory_identities.is_empty());
        assert_eq!(observation.actions.len(), env.game.legal_actions().len());
        assert!(observation
            .actions
            .iter()
            .all(|action| !action.features.is_empty()));
    }

    #[test]
    fn combat_actions_expose_their_immediate_transition() {
        let mut env = TrainEnv::defect_a0(1);
        for _ in 0..64 {
            if env.game.combat.is_some() {
                break;
            }
            env.step(0);
        }
        assert!(
            env.game.combat.is_some(),
            "seed did not reach its first combat"
        );

        let observation = env.training_observation();
        let play = observation
            .actions
            .iter()
            .find(|action| matches!(action.action, Action::Play { .. }))
            .expect("playable opening card");
        assert!(play.parameters.known);
        assert!(play.parameters.cards_played_delta >= 1);
        assert!(
            play.parameters.energy_delta != 0
                || play.parameters.enemy_hp_delta != 0
                || play.parameters.block_delta != 0
                || play.parameters.filled_orbs_delta != 0
        );
    }

    #[test]
    fn reward_candidate_joins_to_every_owned_copy_by_shared_identity() {
        let mut env = TrainEnv::defect_a0(1);
        env.game.player.deck = vec![
            Card::new(CardId::Ball_Lightning),
            Card::new(CardId::Ball_Lightning),
            Card::new(CardId::Skim),
        ]
        .into();
        env.game.screen = Screen::CardReward;
        env.game.card_reward = vec![Card::new(CardId::Ball_Lightning)];

        let observation = env.training_observation();
        let candidate = observation
            .actions
            .iter()
            .find(|action| matches!(&action.action, Action::Choose { index: 0, .. }))
            .expect("card reward action");
        assert_eq!(candidate.candidate_identities.len(), 2);
        assert!(candidate.parameters.known);
        assert_eq!(candidate.parameters.deck_size_delta, 1);
        assert!(candidate.candidate_identities.iter().all(|identity| {
            observation
                .inventory_identities
                .iter()
                .filter(|owned| *owned == identity)
                .count()
                == 2
        }));
    }

    #[test]
    fn deck_measurements_expose_mechanisms_without_policy_preferences() {
        let mut env = TrainEnv::defect_a0(1);
        env.game.player.deck = vec![
            Card::new(CardId::Ball_Lightning),
            Card::new(CardId::Skim),
            Card::new(CardId::Turbo),
            Card::new(CardId::Fission),
            Card::new(CardId::Defragment),
        ]
        .into();

        let measurements = env.measurements();
        assert_eq!(measurements.deck_total_cost, 3);
        assert_eq!(measurements.deck_attack_cards, 1);
        assert_eq!(measurements.deck_skill_cards, 3);
        assert_eq!(measurements.deck_power_cards, 1);
        assert_eq!(measurements.deck_exhaust_cards, 1);
        assert_eq!(measurements.deck_orb_cards, 2);
        assert_eq!(measurements.deck_card_access, 1);
        assert_eq!(measurements.deck_energy_cards, 2);
        assert_eq!(measurements.deck_focus_cards, 1);
    }
}
