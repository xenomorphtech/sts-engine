//! Seeded, fight-free deck formation followed by isolated late-boss tests.
//!
//! The environment keeps the original reward, relic, shop, Neow, and combat
//! implementations. It only replaces map traversal and non-boss combats with
//! a short, seeded schedule of the opportunities those rooms would have
//! produced.

use crate::action::Action;
use crate::game::{Game, Screen};
use crate::htn::HtnAgent;
use crate::ids::{Act, Character, EncounterId, RelicId, RelicTier, RoomType};
use crate::java_util::shuffle_java;
use crate::rng::StsRandom;
use crate::unlocks::Unlocks;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CountRange {
    pub min: usize,
    pub max: usize,
}

impl CountRange {
    pub const fn fixed(value: usize) -> Self {
        Self {
            min: value,
            max: value,
        }
    }

    fn validate(self, name: &str) -> Result<(), String> {
        if self.min > self.max {
            Err(format!("{name}.min must not exceed {name}.max"))
        } else {
            Ok(())
        }
    }

    fn sample(self, rng: &mut StsRandom) -> usize {
        if self.min == self.max {
            self.min
        } else {
            rng.random_range(self.min as i32, self.max as i32) as usize
        }
    }

    pub fn mean(self) -> f32 {
        (self.min + self.max) as f32 * 0.5
    }
}

/// Opportunity-count distribution for a complete three-act build.
///
/// Defaults deliberately describe totals, not per-act maxima: 18 ordinary
/// card rewards, 6 elite bundles, 3 shops, 3 chests, and 7 upgrades on
/// average. The two Act 1/2 boss card and boss-relic rewards are always added.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DraftConfig {
    pub ascension: i32,
    pub normal_card_rewards: CountRange,
    pub elite_opportunities: CountRange,
    pub shops: CountRange,
    pub shop_purchases_per_visit: CountRange,
    pub treasure_relics: CountRange,
    pub upgrades: CountRange,
    pub full_heal_for_each_boss: bool,
}

impl Default for DraftConfig {
    fn default() -> Self {
        Self {
            ascension: 20,
            normal_card_rewards: CountRange { min: 15, max: 21 },
            elite_opportunities: CountRange { min: 4, max: 8 },
            shops: CountRange { min: 2, max: 4 },
            shop_purchases_per_visit: CountRange { min: 1, max: 2 },
            treasure_relics: CountRange::fixed(3),
            upgrades: CountRange { min: 5, max: 9 },
            full_heal_for_each_boss: true,
        }
    }
}

impl DraftConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=20).contains(&self.ascension) {
            return Err("ascension must be between 0 and 20".into());
        }
        self.normal_card_rewards.validate("normal_card_rewards")?;
        self.elite_opportunities.validate("elite_opportunities")?;
        self.shops.validate("shops")?;
        self.shop_purchases_per_visit
            .validate("shop_purchases_per_visit")?;
        self.treasure_relics.validate("treasure_relics")?;
        self.upgrades.validate("upgrades")?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCounts {
    pub normal_card_rewards: usize,
    pub elite_opportunities: usize,
    pub shops: usize,
    pub shop_purchase_slots: usize,
    pub treasure_relics: usize,
    pub upgrades: usize,
    pub boss_card_rewards: usize,
    pub boss_relic_choices: usize,
    pub normal_card_rewards_by_act: [usize; 3],
    pub elites_by_act: [usize; 3],
    pub shops_by_act: [usize; 3],
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct DraftMetrics {
    pub decision_steps: usize,
    pub automated_opportunities: usize,
    pub card_rewards_seen: usize,
    pub cards_added: usize,
    pub cards_removed: usize,
    pub relics_gained: usize,
    pub elite_bundles_resolved: usize,
    pub shops_visited: usize,
    pub shop_purchases: usize,
    pub upgrades_taken: usize,
    pub gold_gained: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FormationAction {
    Game { action: Action },
    Upgrade { deck_index: usize, card: String },
    SkipUpgrade,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftOffer {
    pub action_index: usize,
    pub action: FormationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<i32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCardObservation {
    pub id: String,
    pub upgraded: bool,
    pub times_upgraded: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftRelicObservation {
    pub id: String,
    pub counter: i32,
    pub used_up: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftObservation {
    pub seed: i64,
    pub ascension: i32,
    pub character: String,
    pub phase: String,
    pub engine_screen: String,
    pub source: Option<String>,
    pub ready_for_bosses: bool,
    pub act: i32,
    pub floor: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub gold: i32,
    pub energy_master: i32,
    pub sampled_counts: DraftCounts,
    pub opportunities_remaining: usize,
    pub metrics: DraftMetrics,
    pub deck: Vec<DraftCardObservation>,
    pub relics: Vec<DraftRelicObservation>,
    pub offers: Vec<DraftOffer>,
    pub shop_purchase_slots_remaining: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpportunityKind {
    NormalCard,
    Elite,
    Shop,
    Treasure,
    Upgrade,
    BossReward,
}

impl OpportunityKind {
    fn label(self) -> &'static str {
        match self {
            OpportunityKind::NormalCard => "normal_card_reward",
            OpportunityKind::Elite => "elite_bundle",
            OpportunityKind::Shop => "shop",
            OpportunityKind::Treasure => "treasure_relic",
            OpportunityKind::Upgrade => "upgrade",
            OpportunityKind::BossReward => "boss_reward",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Opportunity {
    act: Act,
    floor: i32,
    kind: OpportunityKind,
    shop_purchase_slots: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Continuation {
    Done,
    EliteCard,
    EliteRelics { remaining: usize, noncamp: bool },
    BossRelic,
    Shop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Neow,
    Start,
    Card {
        remaining: usize,
        continuation: Continuation,
    },
    ResolveRelic {
        continuation: Continuation,
    },
    Shop,
    Upgrade,
    BossRelic,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BuildStage {
    Complete,
    Act1,
}

#[derive(Clone, Copy, Debug)]
struct BossSpec {
    name: &'static str,
    encounter: EncounterId,
    act: Act,
    floor: i32,
    build_stage: BuildStage,
}

// Keep the original late-boss indices stable so an existing joint-combat
// checkpoint can continue learning. The new Act 1 fights occupy indices 4-6.
const BOSS_SPECS: [BossSpec; 7] = [
    BossSpec {
        name: "Awakened One",
        encounter: EncounterId::AwakenedOne,
        act: Act::Beyond,
        floor: 50,
        build_stage: BuildStage::Complete,
    },
    BossSpec {
        name: "Time Eater",
        encounter: EncounterId::TimeEater,
        act: Act::Beyond,
        floor: 50,
        build_stage: BuildStage::Complete,
    },
    BossSpec {
        name: "Donu and Deca",
        encounter: EncounterId::DonuAndDeca,
        act: Act::Beyond,
        floor: 50,
        build_stage: BuildStage::Complete,
    },
    BossSpec {
        name: "Corrupt Heart",
        encounter: EncounterId::CorruptHeart,
        act: Act::Ending,
        floor: 56,
        build_stage: BuildStage::Complete,
    },
    BossSpec {
        name: "Slime Boss",
        encounter: EncounterId::SlimeBoss,
        act: Act::Exordium,
        floor: 16,
        build_stage: BuildStage::Act1,
    },
    BossSpec {
        name: "The Guardian",
        encounter: EncounterId::TheGuardian,
        act: Act::Exordium,
        floor: 16,
        build_stage: BuildStage::Act1,
    },
    BossSpec {
        name: "Hexaghost",
        encounter: EncounterId::Hexaghost,
        act: Act::Exordium,
        floor: 16,
        build_stage: BuildStage::Act1,
    },
];

const ACT1_BOSS_STARTING_HP: i32 = 60;
const ACT1_BOSS_MAX_HP: i32 = 75;

#[derive(Clone, Debug)]
pub struct BossDraftEnv {
    pub game: Game,
    pub config: DraftConfig,
    pub counts: DraftCounts,
    pub metrics: DraftMetrics,
    schedule: Vec<Opportunity>,
    cursor: usize,
    active: Option<Opportunity>,
    stage: Stage,
    shop_purchase_slots_remaining: usize,
    /// Exact build state after Act 1 formation and before its boss rewards.
    act1_boss_snapshot: Option<Game>,
}

impl BossDraftEnv {
    pub fn new(
        seed: i64,
        character: Character,
        config: DraftConfig,
        unlocks: Unlocks,
    ) -> Result<Self, String> {
        config.validate()?;
        // Route composition has its own deterministic stream so changing the
        // opportunity-count model does not silently perturb card/relic offers.
        let mut route_rng =
            StsRandom::from_seed(seed.wrapping_add(0x4452_4146_545f_524c_u64 as i64));
        let normal = config.normal_card_rewards.sample(&mut route_rng);
        let elites = config.elite_opportunities.sample(&mut route_rng);
        let shops = config.shops.sample(&mut route_rng);
        let treasures = config.treasure_relics.sample(&mut route_rng);
        let upgrades = config.upgrades.sample(&mut route_rng);
        let normal_by_act = distribute(normal, normal.min(3), &mut route_rng);
        let elites_by_act = distribute(elites, elites.min(3), &mut route_rng);
        let shops_by_act = distribute(shops, 0, &mut route_rng);
        let treasures_by_act = distribute(treasures, treasures.min(3), &mut route_rng);
        let upgrades_by_act = distribute(upgrades, upgrades.min(3), &mut route_rng);

        let schedule = build_schedule(
            normal_by_act,
            elites_by_act,
            shops_by_act,
            treasures_by_act,
            upgrades_by_act,
            config.shop_purchases_per_visit,
            &mut route_rng,
        );
        let counts = DraftCounts {
            normal_card_rewards: normal,
            elite_opportunities: elites,
            shops,
            shop_purchase_slots: schedule.iter().map(|event| event.shop_purchase_slots).sum(),
            treasure_relics: treasures,
            upgrades,
            boss_card_rewards: 2,
            boss_relic_choices: 2,
            normal_card_rewards_by_act: normal_by_act,
            elites_by_act,
            shops_by_act,
        };
        let mut env = Self {
            game: Game::new(seed, character, config.ascension, unlocks),
            config,
            counts,
            metrics: DraftMetrics::default(),
            schedule,
            cursor: 0,
            active: None,
            stage: Stage::Neow,
            shop_purchase_slots_remaining: 0,
            act1_boss_snapshot: None,
        };
        env.advance_automatic()?;
        Ok(env)
    }

    pub fn fixture(seed: i64, character: Character, config: DraftConfig) -> Result<Self, String> {
        Self::new(seed, character, config, Unlocks::fixture())
    }

    pub fn ready_for_bosses(&self) -> bool {
        self.stage == Stage::Ready
    }

    pub fn legal_actions(&self) -> Vec<FormationAction> {
        match self.stage {
            Stage::Ready | Stage::Start => Vec::new(),
            Stage::Upgrade => {
                let mut actions = self
                    .game
                    .player
                    .deck
                    .iter()
                    .enumerate()
                    .filter(|(_, card)| card.can_upgrade())
                    .map(|(deck_index, card)| FormationAction::Upgrade {
                        deck_index,
                        card: card_label(card),
                    })
                    .collect::<Vec<_>>();
                actions.push(FormationAction::SkipUpgrade);
                actions
            }
            _ => self
                .game
                .legal_actions()
                .into_iter()
                .filter(|action| !matches!(action, Action::Potion { .. } | Action::Quit))
                .map(|action| FormationAction::Game { action })
                .collect(),
        }
    }

    /// Reference formation policy using the existing HTN card/shop/relic
    /// decisions. This is a reproducible baseline for RL comparisons, not an
    /// action forced by the environment.
    pub fn htn_baseline_action_index(&self, agent: &mut HtnAgent) -> Option<usize> {
        let legal = self.legal_actions();
        if self.stage == Stage::Upgrade {
            return legal
                .iter()
                .enumerate()
                .filter_map(|(index, action)| {
                    let FormationAction::Upgrade { deck_index, .. } = action else {
                        return None;
                    };
                    self.game
                        .player
                        .deck
                        .get(*deck_index)
                        .map(|card| (index, crate::htn::strategy::upgrade_score(card.id)))
                })
                .max_by_key(|(_, score)| *score)
                .map(|(index, _)| index)
                .or_else(|| (!legal.is_empty()).then_some(legal.len() - 1));
        }

        let decision = agent.decide(&self.game);
        legal
            .iter()
            .position(|candidate| {
                matches!(candidate, FormationAction::Game { action } if action == &decision)
            })
            .or_else(|| {
                legal.iter().position(|candidate| {
                    matches!(
                        candidate,
                        FormationAction::Game {
                            action: Action::Skip | Action::Proceed
                        } | FormationAction::SkipUpgrade
                    )
                })
            })
            .or_else(|| (!legal.is_empty()).then_some(0))
    }

    pub fn observation(&self) -> DraftObservation {
        let actions = self.legal_actions();
        let offers = actions
            .into_iter()
            .enumerate()
            .map(|(action_index, action)| {
                let cost = match &action {
                    FormationAction::Game { action } if self.game.screen == Screen::Shop => {
                        self.game.draft_shop_action_price(action)
                    }
                    _ => None,
                };
                DraftOffer {
                    action_index,
                    action,
                    cost,
                }
            })
            .collect();
        DraftObservation {
            seed: self.game.seed,
            ascension: self.game.ascension,
            character: self.game.character.sts_name().to_string(),
            phase: self.phase_name().to_string(),
            engine_screen: format!("{:?}", self.game.screen),
            source: self.active.map(|event| event.kind.label().to_string()),
            ready_for_bosses: self.ready_for_bosses(),
            act: self.game.dungeon.act as i32,
            floor: self.game.dungeon.floor,
            hp: self.game.player.hp,
            max_hp: self.game.player.max_hp,
            gold: self.game.player.gold,
            energy_master: self.game.player.energy_master,
            sampled_counts: self.counts.clone(),
            opportunities_remaining: self.schedule.len().saturating_sub(self.cursor)
                + usize::from(self.active.is_some()),
            metrics: self.metrics.clone(),
            deck: self
                .game
                .player
                .deck
                .iter()
                .map(|card| DraftCardObservation {
                    id: card.sts_id().to_string(),
                    upgraded: card.upgraded,
                    times_upgraded: card.times_upgraded,
                })
                .collect(),
            relics: self
                .game
                .player
                .relics
                .iter()
                .map(|relic| DraftRelicObservation {
                    id: relic.id.sts_id().to_string(),
                    counter: relic.counter,
                    used_up: relic.used_up,
                })
                .collect(),
            offers,
            shop_purchase_slots_remaining: self.shop_purchase_slots_remaining,
        }
    }

    /// Apply an index into `observation().offers` and return the next stable
    /// decision boundary. Automatic gold/relic/act transitions do not consume
    /// RL steps.
    pub fn step(&mut self, action_index: usize) -> Result<DraftObservation, String> {
        let legal = self.legal_actions();
        let selected = legal.get(action_index).cloned().ok_or_else(|| {
            format!(
                "invalid action index {action_index}; {} legal actions",
                legal.len()
            )
        })?;
        let deck_before = self.game.player.deck.len();
        let price = match &selected {
            FormationAction::Game { action } if self.game.screen == Screen::Shop => {
                self.game.draft_shop_action_price(action)
            }
            _ => None,
        };
        self.metrics.decision_steps += 1;

        match selected {
            FormationAction::Game { action } => {
                let closes_shop =
                    self.stage == Stage::Shop && matches!(action, Action::Skip | Action::Proceed);
                self.game.step(&action);
                if price.is_some() {
                    self.metrics.shop_purchases += 1;
                    self.shop_purchase_slots_remaining =
                        self.shop_purchase_slots_remaining.saturating_sub(1);
                }
                if closes_shop {
                    self.finish_event();
                }
            }
            FormationAction::Upgrade { deck_index, .. } => {
                let card = self
                    .game
                    .player
                    .deck
                    .get_mut(deck_index)
                    .ok_or_else(|| format!("deck index {deck_index} is no longer valid"))?;
                if !card.can_upgrade() {
                    return Err(format!("{} cannot be upgraded", card.sts_id()));
                }
                card.upgrade();
                self.metrics.upgrades_taken += 1;
                self.finish_event();
            }
            FormationAction::SkipUpgrade => self.finish_event(),
        }

        let deck_after = self.game.player.deck.len();
        if deck_after > deck_before {
            self.metrics.cards_added += deck_after - deck_before;
        } else {
            self.metrics.cards_removed += deck_before - deck_after;
        }
        self.advance_automatic()?;
        Ok(self.observation())
    }

    pub fn evaluate_htn(&self, max_steps_per_boss: usize) -> BossSuiteResult {
        let fights = BOSS_SPECS
            .into_iter()
            .map(|boss| {
                let mut game = prepare_boss_game(self, boss);
                let initial_boss_hp = living_enemy_hp(&game);
                let mut previous_boss_hp = initial_boss_hp;
                let mut boss_damage_dealt = 0i32;
                let mut agent = HtnAgent::new();
                let mut steps = 0usize;
                while game.player.hp > 0 && game.combat.is_some() && steps < max_steps_per_boss {
                    let action = agent.decide(&game);
                    if matches!(action, Action::Quit) {
                        break;
                    }
                    game.step(&action);
                    let current_boss_hp = living_enemy_hp(&game);
                    boss_damage_dealt += (previous_boss_hp - current_boss_hp).max(0);
                    previous_boss_hp = current_boss_hp;
                    steps += 1;
                }
                let boss_hp_remaining = living_enemy_hp(&game);
                let won = game.player.hp > 0 && game.combat.is_none();
                BossFightResult {
                    boss: boss.name.to_string(),
                    fought: initial_boss_hp > 0 && steps > 0,
                    won,
                    timed_out: !won && game.player.hp > 0 && game.combat.is_some(),
                    combat_steps: steps,
                    player_hp_remaining: game.player.hp.max(0),
                    initial_boss_hp,
                    boss_hp_remaining,
                    boss_damage_dealt,
                }
            })
            .collect::<Vec<_>>();
        BossSuiteResult::from_fights(fights)
    }

    fn phase_name(&self) -> &'static str {
        match self.stage {
            Stage::Neow => "neow",
            Stage::Start => "automatic",
            Stage::Card { .. } => match self.active.map(|event| event.kind) {
                Some(OpportunityKind::Elite) => "elite_card_reward",
                Some(OpportunityKind::BossReward) => "boss_card_reward",
                _ => "card_reward",
            },
            Stage::ResolveRelic { .. } => "relic_resolution",
            Stage::Shop => "shop",
            Stage::Upgrade => "upgrade",
            Stage::BossRelic => "boss_relic",
            Stage::Ready => "ready_for_bosses",
        }
    }

    fn finish_event(&mut self) {
        self.active = None;
        self.shop_purchase_slots_remaining = 0;
        self.stage = Stage::Start;
    }

    fn continue_with(&mut self, continuation: Continuation) {
        match continuation {
            Continuation::Done => self.finish_event(),
            Continuation::EliteCard => {
                let event = self.active.expect("elite event remains active");
                self.open_card_reward(event, RoomType::Elite, 1, Continuation::Done);
            }
            Continuation::EliteRelics { remaining, noncamp } => {
                self.stage = if remaining == 0 {
                    Stage::Card {
                        remaining: 0,
                        continuation: Continuation::EliteCard,
                    }
                } else {
                    self.grant_elite_relic(remaining, noncamp)
                };
            }
            Continuation::BossRelic => {
                let event = self.active.expect("boss event remains active");
                self.game.draft_open_boss_relics(event.act, event.floor);
                self.stage = Stage::BossRelic;
            }
            Continuation::Shop => {
                if self.game.screen == Screen::Shop && self.shop_purchase_slots_remaining > 0 {
                    self.stage = Stage::Shop;
                } else {
                    self.finish_event();
                }
            }
        }
    }

    fn advance_automatic(&mut self) -> Result<(), String> {
        for _ in 0..10_000 {
            match self.stage {
                Stage::Ready => return Ok(()),
                Stage::Neow => {
                    if self.game.screen == Screen::Neow {
                        if self.game.neow_screen == 3 {
                            return Ok(());
                        }
                        let action = self
                            .game
                            .legal_actions()
                            .into_iter()
                            .find(|action| !matches!(action, Action::Potion { .. } | Action::Quit))
                            .ok_or_else(|| "Neow has no action to advance".to_string())?;
                        self.game.step(&action);
                        continue;
                    }
                    if self.game.screen == Screen::Map {
                        self.stage = Stage::Start;
                        continue;
                    }
                    if auxiliary_decision_pending(&self.game) {
                        return Ok(());
                    }
                    self.stage = Stage::Start;
                }
                Stage::Start => {
                    let Some(event) = self.schedule.get(self.cursor).copied() else {
                        self.active = None;
                        self.stage = Stage::Ready;
                        return Ok(());
                    };
                    if event.act == Act::Exordium
                        && event.kind == OpportunityKind::BossReward
                        && self.act1_boss_snapshot.is_none()
                    {
                        self.act1_boss_snapshot = Some(self.game.clone());
                    }
                    self.cursor += 1;
                    self.active = Some(event);
                    self.metrics.automated_opportunities += 1;
                    self.start_event(event);
                }
                Stage::Card {
                    remaining,
                    continuation,
                } => {
                    if remaining == 0 {
                        self.continue_with(continuation);
                    } else if self.game.screen == Screen::CardReward {
                        return Ok(());
                    } else if remaining > 1 {
                        let event = self.active.expect("card event remains active");
                        let room = card_room(event.kind);
                        self.open_card_reward(event, room, remaining - 1, continuation);
                        return Ok(());
                    } else {
                        self.continue_with(continuation);
                    }
                }
                Stage::ResolveRelic { continuation } => {
                    if auxiliary_decision_pending(&self.game) {
                        return Ok(());
                    }
                    self.continue_with(continuation);
                }
                Stage::Shop => {
                    if self.game.screen == Screen::Shop {
                        if self.shop_purchase_slots_remaining == 0 {
                            self.game.step(&Action::Skip);
                            self.finish_event();
                            continue;
                        }
                        return Ok(());
                    }
                    if auxiliary_decision_pending(&self.game) {
                        self.stage = Stage::ResolveRelic {
                            continuation: Continuation::Shop,
                        };
                        return Ok(());
                    }
                    self.finish_event();
                }
                Stage::Upgrade => return Ok(()),
                Stage::BossRelic => {
                    if !self.game.boss_relics.is_empty() {
                        return Ok(());
                    }
                    if auxiliary_decision_pending(&self.game) {
                        self.stage = Stage::ResolveRelic {
                            continuation: Continuation::Done,
                        };
                        return Ok(());
                    }
                    self.finish_event();
                }
            }
        }
        Err("draft automatic transition guard exceeded".into())
    }

    fn start_event(&mut self, event: Opportunity) {
        self.game.draft_prepare_act(event.act);
        self.game.dungeon.floor = event.floor;
        match event.kind {
            OpportunityKind::NormalCard => {
                self.game.current_room = RoomType::Monster;
                let gold = crate::rewards::roll_monster_gold(
                    &mut self.game.rng,
                    false,
                    false,
                    self.game.ascension,
                );
                self.metrics.gold_gained += self.game.draft_gain_gold(gold);
                let rewards = 1 + usize::from(self.game.player.has_relic(RelicId::Prayer_Wheel));
                self.open_card_reward(event, RoomType::Monster, rewards, Continuation::Done);
            }
            OpportunityKind::Elite => {
                self.game.current_room = RoomType::Elite;
                self.metrics.elite_bundles_resolved += 1;
                let gold = crate::rewards::roll_monster_gold(
                    &mut self.game.rng,
                    false,
                    true,
                    self.game.ascension,
                );
                self.metrics.gold_gained += self.game.draft_gain_gold(gold);
                let relics = 1 + usize::from(self.game.player.has_relic(RelicId::Black_Star));
                self.stage = self.grant_elite_relic(relics, false);
            }
            OpportunityKind::Shop => {
                self.game.draft_open_shop(event.act, event.floor);
                self.metrics.shops_visited += 1;
                self.shop_purchase_slots_remaining = event.shop_purchase_slots;
                self.stage = Stage::Shop;
            }
            OpportunityKind::Treasure => {
                self.game.current_room = RoomType::Treasure;
                self.game.screen = Screen::CombatReward;
                self.game.rewards.clear();
                let roll = self.game.rng.treasure.random_range(0, 99);
                let tier = if roll < 50 {
                    RelicTier::COMMON
                } else if roll < 83 {
                    RelicTier::UNCOMMON
                } else {
                    RelicTier::RARE
                };
                if self.game.rng.treasure.random_boolean_chance(0.5) {
                    let gold = self.game.rng.treasure.random_range(20, 35);
                    self.metrics.gold_gained += self.game.draft_gain_gold(gold);
                }
                if self.game.draft_gain_relic(tier, false).is_some() {
                    self.metrics.relics_gained += 1;
                }
                self.stage = Stage::ResolveRelic {
                    continuation: Continuation::Done,
                };
            }
            OpportunityKind::Upgrade => self.stage = Stage::Upgrade,
            OpportunityKind::BossReward => {
                self.game.current_room = RoomType::Boss;
                let gold = crate::rewards::roll_monster_gold(
                    &mut self.game.rng,
                    true,
                    false,
                    self.game.ascension,
                );
                self.metrics.gold_gained += self.game.draft_gain_gold(gold);
                self.open_card_reward(event, RoomType::Boss, 1, Continuation::BossRelic);
            }
        }
    }

    fn grant_elite_relic(&mut self, remaining: usize, noncamp: bool) -> Stage {
        let event = self.active.expect("elite event remains active");
        self.game.draft_prepare_act(event.act);
        self.game.dungeon.floor = event.floor;
        self.game.current_room = RoomType::Elite;
        self.game.screen = Screen::CombatReward;
        self.game.rewards.clear();
        let roll = self.game.rng.relic.random_range(0, 99);
        let tier = if roll < 50 {
            RelicTier::COMMON
        } else if roll > 82 {
            RelicTier::RARE
        } else {
            RelicTier::UNCOMMON
        };
        if self.game.draft_gain_relic(tier, noncamp).is_some() {
            self.metrics.relics_gained += 1;
        }
        Stage::ResolveRelic {
            continuation: Continuation::EliteRelics {
                remaining: remaining.saturating_sub(1),
                noncamp: true,
            },
        }
    }

    fn open_card_reward(
        &mut self,
        event: Opportunity,
        room: RoomType,
        remaining: usize,
        continuation: Continuation,
    ) {
        self.game
            .draft_open_card_reward(event.act, event.floor, room);
        self.metrics.card_rewards_seen += 1;
        self.stage = Stage::Card {
            remaining,
            continuation,
        };
    }
}

/// Synchronous vector wrapper for Monte Carlo training over many independent
/// seeds. Every index is stable across reset, step, observation, and boss
/// evaluation so a learner can keep recurrent state per seed.
#[derive(Clone, Debug)]
pub struct BossDraftBatch {
    pub envs: Vec<BossDraftEnv>,
}

impl BossDraftBatch {
    pub fn new(
        seeds: &[i64],
        character: Character,
        config: DraftConfig,
        unlocks: Unlocks,
    ) -> Result<Self, String> {
        if seeds.is_empty() {
            return Err("batch must contain at least one seed".into());
        }
        let envs = seeds
            .iter()
            .map(|&seed| BossDraftEnv::new(seed, character, config.clone(), unlocks.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self { envs })
    }

    pub fn fixture(
        seeds: &[i64],
        character: Character,
        config: DraftConfig,
    ) -> Result<Self, String> {
        Self::new(seeds, character, config, Unlocks::fixture())
    }

    pub fn len(&self) -> usize {
        self.envs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.envs.is_empty()
    }

    pub fn ready_count(&self) -> usize {
        self.envs
            .iter()
            .filter(|environment| environment.ready_for_bosses())
            .count()
    }

    pub fn observations(&self) -> Vec<DraftObservation> {
        self.envs.iter().map(BossDraftEnv::observation).collect()
    }

    /// Step every active environment once. A ready environment requires
    /// `None`; an active environment requires `Some(index)`.
    pub fn step(
        &mut self,
        action_indices: &[Option<usize>],
    ) -> Result<Vec<DraftObservation>, String> {
        if action_indices.len() != self.envs.len() {
            return Err(format!(
                "batch has {} environments but received {} actions",
                self.envs.len(),
                action_indices.len()
            ));
        }
        for (index, (environment, action)) in self
            .envs
            .iter_mut()
            .zip(action_indices.iter().copied())
            .enumerate()
        {
            match (environment.ready_for_bosses(), action) {
                (true, None) => {}
                (true, Some(_)) => {
                    return Err(format!("batch environment {index} is already ready"));
                }
                (false, Some(action_index)) => {
                    environment
                        .step(action_index)
                        .map_err(|error| format!("batch environment {index}: {error}"))?;
                }
                (false, None) => {
                    return Err(format!(
                        "batch environment {index} still needs a formation action"
                    ));
                }
            }
        }
        Ok(self.observations())
    }

    /// Complete every randomized formation with the existing HTN policy as a
    /// reproducible baseline. The returned builds are still seed-specific;
    /// this only supplies choices where a training policy would normally act.
    pub fn complete_with_htn_baseline(
        &mut self,
        max_decisions: usize,
    ) -> Result<Vec<DraftObservation>, String> {
        let mut agents = (0..self.len()).map(|_| HtnAgent::new()).collect::<Vec<_>>();
        for _ in 0..max_decisions {
            if self.ready_count() == self.len() {
                return Ok(self.observations());
            }
            let actions = self
                .envs
                .iter()
                .zip(agents.iter_mut())
                .enumerate()
                .map(|(index, (environment, agent))| {
                    if environment.ready_for_bosses() {
                        Ok(None)
                    } else {
                        environment
                            .htn_baseline_action_index(agent)
                            .map(Some)
                            .ok_or_else(|| {
                                format!("batch environment {index} has no legal baseline action")
                            })
                    }
                })
                .collect::<Result<Vec<_>, String>>()?;
            self.step(&actions)?;
        }
        Err(format!(
            "baseline reached the {max_decisions}-decision cap with only {}/{} environments ready",
            self.ready_count(),
            self.len()
        ))
    }

    pub fn evaluate_htn(&self, max_steps_per_boss: usize) -> Result<Vec<BossSuiteResult>, String> {
        if self.ready_count() != self.len() {
            return Err(format!(
                "only {}/{} batch environments are ready for bosses",
                self.ready_count(),
                self.len()
            ));
        }
        Ok(self
            .envs
            .iter()
            .map(|environment| environment.evaluate_htn(max_steps_per_boss))
            .collect())
    }

    /// Start the three Act 1 fights from the pre-reward Act 1 snapshot and the
    /// three Act 3 bosses plus Heart from the completed build, exposing every
    /// combat decision to an external policy.
    pub fn start_boss_combats(
        &self,
        max_steps_per_fight: usize,
    ) -> Result<BossCombatBatch, String> {
        if self.ready_count() != self.len() {
            return Err(format!(
                "only {}/{} batch environments are ready for bosses",
                self.ready_count(),
                self.len()
            ));
        }
        if max_steps_per_fight == 0 {
            return Err("max_steps_per_fight must be positive".into());
        }
        let mut fights = Vec::with_capacity(self.len() * BOSS_SPECS.len());
        for (build_index, environment) in self.envs.iter().enumerate() {
            for (boss_index, boss) in BOSS_SPECS.iter().copied().enumerate() {
                let game = prepare_boss_game(environment, boss);
                let initial_boss_hp = living_enemy_hp(&game);
                fights.push(BossCombatEnv {
                    game,
                    build_index,
                    boss_index,
                    boss: boss.name.to_string(),
                    initial_boss_hp,
                    previous_boss_hp: initial_boss_hp,
                    boss_damage_dealt: 0,
                    steps: 0,
                    max_steps: max_steps_per_fight,
                    baseline: HtnAgent::new(),
                });
            }
        }
        Ok(BossCombatBatch {
            fights,
            build_count: self.len(),
        })
    }
}

pub struct BossCombatBatch {
    pub fights: Vec<BossCombatEnv>,
    pub build_count: usize,
}

pub struct BossCombatEnv {
    pub game: Game,
    pub build_index: usize,
    pub boss_index: usize,
    pub boss: String,
    initial_boss_hp: i32,
    previous_boss_hp: i32,
    boss_damage_dealt: i32,
    steps: usize,
    max_steps: usize,
    baseline: HtnAgent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatCardObservation {
    pub id: String,
    pub upgraded: bool,
    pub times_upgraded: u8,
    pub cost: i16,
    pub cost_for_turn: i16,
    pub damage: i16,
    pub block: i16,
    pub magic: i16,
    pub free_to_play_once: bool,
    pub exhaust: bool,
    pub ethereal: bool,
    pub retain: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatPowerObservation {
    pub id: String,
    pub amount: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatOrbObservation {
    pub kind: String,
    pub evoke: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatMonsterObservation {
    pub index: usize,
    pub id: String,
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub dead: bool,
    pub escaped: bool,
    pub intent: String,
    /// Expected raw damage from one hit of the published intent.
    pub intent_damage_per_hit: i32,
    pub intent_hits: i32,
    /// Raw intent damage before player block, Weak, or Intangible resolution.
    pub intent_total_damage: i32,
    pub powers: Vec<DraftCombatPowerObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatOffer {
    pub action_index: usize,
    pub label: String,
    pub action: Action,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DraftCombatObservation {
    pub build_index: usize,
    pub boss_index: usize,
    pub boss: String,
    pub seed: i64,
    pub done: bool,
    pub won: bool,
    pub timed_out: bool,
    pub steps: usize,
    pub max_steps: usize,
    pub screen: String,
    pub turn: i32,
    pub cards_played_this_turn: i32,
    pub player_hp: i32,
    pub player_max_hp: i32,
    pub player_block: i32,
    pub energy: i32,
    pub energy_master: i32,
    pub hand: Vec<DraftCombatCardObservation>,
    pub draw: Vec<DraftCombatCardObservation>,
    pub discard: Vec<DraftCombatCardObservation>,
    pub exhaust: Vec<DraftCombatCardObservation>,
    pub relics: Vec<DraftRelicObservation>,
    pub potions: Vec<String>,
    pub powers: Vec<DraftCombatPowerObservation>,
    pub orbs: Vec<DraftCombatOrbObservation>,
    pub monsters: Vec<DraftCombatMonsterObservation>,
    pub boss_hp_remaining: i32,
    pub boss_damage_dealt: i32,
    pub offers: Vec<DraftCombatOffer>,
}

impl BossCombatEnv {
    pub fn active(&self) -> bool {
        self.game.player.hp > 0 && self.game.combat.is_some() && self.steps < self.max_steps
    }

    pub fn legal_actions(&self) -> Vec<Action> {
        if !self.active() {
            return Vec::new();
        }
        self.game
            .legal_actions()
            .into_iter()
            .filter(|action| !matches!(action, Action::Quit))
            .collect()
    }

    pub fn observation(&self) -> DraftCombatObservation {
        let actions = self.legal_actions();
        let combat = self.game.combat.as_ref();
        DraftCombatObservation {
            build_index: self.build_index,
            boss_index: self.boss_index,
            boss: self.boss.clone(),
            seed: self.game.seed,
            done: !self.active(),
            won: self.game.player.hp > 0 && self.game.combat.is_none(),
            timed_out: self.game.player.hp > 0
                && self.game.combat.is_some()
                && self.steps >= self.max_steps,
            steps: self.steps,
            max_steps: self.max_steps,
            screen: format!("{:?}", self.game.screen),
            turn: combat.map(|state| state.turn).unwrap_or_default(),
            cards_played_this_turn: combat
                .map(|state| state.cards_played_this_turn)
                .unwrap_or_default(),
            player_hp: self.game.player.hp,
            player_max_hp: self.game.player.max_hp,
            player_block: self.game.player.block,
            energy: self.game.player.energy,
            energy_master: self.game.player.energy_master,
            hand: combat_cards(&self.game.player.hand),
            draw: combat_cards(&self.game.player.draw),
            discard: combat_cards(&self.game.player.discard),
            exhaust: combat_cards(&self.game.player.exhaust),
            relics: self
                .game
                .player
                .relics
                .iter()
                .map(|relic| DraftRelicObservation {
                    id: relic.id.sts_id().to_string(),
                    counter: relic.counter,
                    used_up: relic.used_up,
                })
                .collect(),
            potions: self
                .game
                .player
                .potions
                .iter()
                .map(|potion| potion.id.sts_id().to_string())
                .collect(),
            powers: combat_powers(&self.game.player.powers),
            orbs: self
                .game
                .player
                .orbs
                .iter()
                .map(|orb| DraftCombatOrbObservation {
                    kind: format!("{:?}", orb.kind),
                    evoke: orb.evoke,
                })
                .collect(),
            monsters: combat
                .map(|state| {
                    state
                        .monsters
                        .iter()
                        .enumerate()
                        .map(|(index, monster)| DraftCombatMonsterObservation {
                            index,
                            id: monster.id.sts_id().to_string(),
                            hp: monster.hp,
                            max_hp: monster.max_hp,
                            block: monster.block,
                            dead: monster.dead,
                            escaped: monster.escaped,
                            intent: format!("{:?}", monster.intent),
                            intent_damage_per_hit: monster.intent_damage,
                            intent_hits: monster.intent_hits,
                            intent_total_damage: monster
                                .intent_damage
                                .saturating_mul(monster.intent_hits.max(0)),
                            powers: combat_powers(&monster.powers),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            boss_hp_remaining: living_enemy_hp(&self.game),
            boss_damage_dealt: self.boss_damage_dealt,
            offers: actions
                .into_iter()
                .enumerate()
                .map(|(action_index, action)| DraftCombatOffer {
                    action_index,
                    label: combat_action_label(&self.game, &action),
                    action,
                })
                .collect(),
        }
    }

    pub fn step(&mut self, action_index: usize) -> Result<DraftCombatObservation, String> {
        let actions = self.legal_actions();
        let action = actions.get(action_index).cloned().ok_or_else(|| {
            format!(
                "invalid combat action index {action_index}; {} legal actions",
                actions.len()
            )
        })?;
        self.game.step(&action);
        let current_boss_hp = living_enemy_hp(&self.game);
        self.boss_damage_dealt += (self.previous_boss_hp - current_boss_hp).max(0);
        self.previous_boss_hp = current_boss_hp;
        self.steps += 1;
        Ok(self.observation())
    }

    pub fn baseline_action_index(&mut self) -> Option<usize> {
        if !self.active() {
            return None;
        }
        let actions = self.legal_actions();
        let decision = self.baseline.decide(&self.game);
        actions
            .iter()
            .position(|action| action == &decision)
            .or_else(|| {
                actions
                    .iter()
                    .position(|action| matches!(action, Action::EndTurn))
            })
            .or_else(|| (!actions.is_empty()).then_some(0))
    }

    pub fn result(&self) -> BossFightResult {
        let won = self.game.player.hp > 0 && self.game.combat.is_none();
        BossFightResult {
            boss: self.boss.clone(),
            fought: self.initial_boss_hp > 0 && self.steps > 0,
            won,
            timed_out: !won && self.game.player.hp > 0 && self.game.combat.is_some(),
            combat_steps: self.steps,
            player_hp_remaining: self.game.player.hp.max(0),
            initial_boss_hp: self.initial_boss_hp,
            boss_hp_remaining: living_enemy_hp(&self.game),
            boss_damage_dealt: self.boss_damage_dealt,
        }
    }
}

impl BossCombatBatch {
    pub fn ready_count(&self) -> usize {
        self.fights.iter().filter(|fight| !fight.active()).count()
    }

    pub fn observations(&self) -> Vec<DraftCombatObservation> {
        self.fights.iter().map(BossCombatEnv::observation).collect()
    }

    pub fn baseline_action_indices(&mut self) -> Vec<Option<usize>> {
        self.fights
            .iter_mut()
            .map(BossCombatEnv::baseline_action_index)
            .collect()
    }

    pub fn step(
        &mut self,
        action_indices: &[Option<usize>],
    ) -> Result<Vec<DraftCombatObservation>, String> {
        if action_indices.len() != self.fights.len() {
            return Err(format!(
                "combat batch has {} fights but received {} actions",
                self.fights.len(),
                action_indices.len()
            ));
        }
        for (index, (fight, action)) in self
            .fights
            .iter_mut()
            .zip(action_indices.iter().copied())
            .enumerate()
        {
            match (fight.active(), action) {
                (false, None) => {}
                (false, Some(_)) => return Err(format!("combat fight {index} is already done")),
                (true, Some(action_index)) => {
                    fight
                        .step(action_index)
                        .map_err(|error| format!("combat fight {index}: {error}"))?;
                }
                (true, None) => {
                    return Err(format!("combat fight {index} still needs an action"));
                }
            }
        }
        Ok(self.observations())
    }

    pub fn results(&self) -> Result<Vec<BossSuiteResult>, String> {
        if self.ready_count() != self.fights.len() {
            return Err(format!(
                "only {}/{} boss fights are finished",
                self.ready_count(),
                self.fights.len()
            ));
        }
        Ok(self
            .fights
            .chunks(BOSS_SPECS.len())
            .map(|fights| {
                BossSuiteResult::from_fights(
                    fights.iter().map(BossCombatEnv::result).collect::<Vec<_>>(),
                )
            })
            .collect())
    }
}

fn combat_cards(cards: &[crate::card::Card]) -> Vec<DraftCombatCardObservation> {
    cards
        .iter()
        .map(|card| DraftCombatCardObservation {
            id: card.id.sts_id().to_string(),
            upgraded: card.upgraded,
            times_upgraded: card.times_upgraded,
            cost: card.cost,
            cost_for_turn: card.cost_for_turn,
            damage: card.base_damage,
            block: card.base_block,
            magic: card.base_magic,
            free_to_play_once: card.free_to_play_once,
            exhaust: card.exhaust,
            ethereal: card.ethereal,
            retain: card.retain,
        })
        .collect()
}

fn combat_powers(powers: &[crate::creature::Power]) -> Vec<DraftCombatPowerObservation> {
    powers
        .iter()
        .map(|power| DraftCombatPowerObservation {
            id: format!("{:?}", power.id),
            amount: power.amount,
        })
        .collect()
}

fn combat_action_label(game: &Game, action: &Action) -> String {
    match action {
        Action::Play {
            hand_index,
            target_index,
        } => {
            let card = game
                .player
                .hand
                .get(*hand_index)
                .map(|card| card.sts_id().to_string())
                .unwrap_or_else(|| format!("hand-{hand_index}"));
            let target = target_index
                .and_then(|index| game.combat.as_ref()?.monsters.get(index))
                .map(|monster| monster.id.sts_id().to_string())
                .unwrap_or_else(|| "none".into());
            format!("play:{card}:target:{target}")
        }
        Action::Potion {
            action,
            slot,
            target_index,
        } => {
            let potion = game
                .player
                .potions
                .get(*slot)
                .map(|potion| potion.id.sts_id().to_string())
                .unwrap_or_else(|| format!("slot-{slot}"));
            format!("potion:{potion}:{action:?}:target:{target_index:?}")
        }
        Action::Choose { label, index, .. } => {
            label.clone().unwrap_or_else(|| format!("choose:{index}"))
        }
        Action::EndTurn => "end_turn".into(),
        Action::Proceed => "proceed".into(),
        Action::Skip => "skip".into(),
        Action::Quit => "quit".into(),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BossFightResult {
    pub boss: String,
    pub fought: bool,
    pub won: bool,
    pub timed_out: bool,
    pub combat_steps: usize,
    pub player_hp_remaining: i32,
    pub initial_boss_hp: i32,
    /// Sum over every still-living enemy in the boss encounter.
    pub boss_hp_remaining: i32,
    /// Cumulative positive HP reductions, including multi-phase bosses.
    pub boss_damage_dealt: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BossSuiteResult {
    pub fights: Vec<BossFightResult>,
    pub fights_started: usize,
    pub wins: usize,
    pub losses: usize,
    pub timeouts: usize,
    pub act1_fights_started: usize,
    pub act1_wins: usize,
    /// The required early-build objective: all three Act 1 bosses were beaten.
    pub act1_all_won: bool,
    pub player_hp_remaining_sum: i32,
    pub initial_boss_hp_sum: i32,
    pub boss_hp_remaining_sum: i32,
    /// A dense positive reach signal: more damage always increases this value.
    pub boss_damage_dealt_sum: i32,
}

impl BossSuiteResult {
    fn from_fights(fights: Vec<BossFightResult>) -> Self {
        let fights_started = fights.iter().filter(|fight| fight.fought).count();
        let wins = fights.iter().filter(|fight| fight.won).count();
        let timeouts = fights.iter().filter(|fight| fight.timed_out).count();
        let losses = fights_started.saturating_sub(wins + timeouts);
        let act1_fights = fights
            .iter()
            .filter(|fight| is_act1_boss(&fight.boss))
            .collect::<Vec<_>>();
        let act1_fights_started = act1_fights.iter().filter(|fight| fight.fought).count();
        let act1_wins = act1_fights.iter().filter(|fight| fight.won).count();
        let act1_all_won = act1_fights_started == 3 && act1_wins == 3;
        let player_hp_remaining_sum = fights.iter().map(|fight| fight.player_hp_remaining).sum();
        let initial_boss_hp_sum = fights
            .iter()
            .filter(|fight| fight.fought)
            .map(|fight| fight.initial_boss_hp)
            .sum();
        let boss_hp_remaining_sum = fights
            .iter()
            .filter(|fight| fight.fought)
            .map(|fight| fight.boss_hp_remaining)
            .sum();
        let boss_damage_dealt_sum = fights
            .iter()
            .filter(|fight| fight.fought)
            .map(|fight| fight.boss_damage_dealt)
            .sum();
        Self {
            fights,
            fights_started,
            wins,
            losses,
            timeouts,
            act1_fights_started,
            act1_wins,
            act1_all_won,
            player_hp_remaining_sum,
            initial_boss_hp_sum,
            boss_hp_remaining_sum,
            boss_damage_dealt_sum,
        }
    }
}

fn prepare_boss_game(environment: &BossDraftEnv, boss: BossSpec) -> Game {
    let source = match boss.build_stage {
        BuildStage::Complete => &environment.game,
        BuildStage::Act1 => environment
            .act1_boss_snapshot
            .as_ref()
            .expect("Act 1 build snapshot must exist before boss evaluation"),
    };
    let mut game = source.clone();
    match boss.build_stage {
        BuildStage::Act1 => {
            game.player.max_hp = ACT1_BOSS_MAX_HP;
            game.player.hp = ACT1_BOSS_STARTING_HP;
        }
        BuildStage::Complete if environment.config.full_heal_for_each_boss => {
            game.player.hp = game.player.max_hp;
        }
        BuildStage::Complete => {}
    }
    game.player.block = 0;
    game.player.powers.clear();
    game.player.draw.clear();
    game.player.hand.clear();
    game.player.discard.clear();
    game.player.exhaust.clear();
    game.draft_start_boss(boss.encounter, boss.act, boss.floor);
    game
}

fn is_act1_boss(name: &str) -> bool {
    matches!(name, "Slime Boss" | "The Guardian" | "Hexaghost")
}

fn distribute(total: usize, minimum_slots: usize, rng: &mut StsRandom) -> [usize; 3] {
    let base = minimum_slots.min(3).min(total);
    let mut out = [0usize; 3];
    for slot in out.iter_mut().take(base) {
        *slot = 1;
    }
    for _ in base..total {
        out[rng.random_range(0, 2) as usize] += 1;
    }
    out
}

fn build_schedule(
    normal: [usize; 3],
    elites: [usize; 3],
    shops: [usize; 3],
    treasures: [usize; 3],
    upgrades: [usize; 3],
    shop_purchases_per_visit: CountRange,
    rng: &mut StsRandom,
) -> Vec<Opportunity> {
    let acts = [Act::Exordium, Act::City, Act::Beyond];
    let bases = [0, 17, 34];
    let bosses = [16, 33, 50];
    let mut schedule = Vec::new();
    for act_index in 0..3 {
        let mut kinds = Vec::new();
        kinds.extend(std::iter::repeat_n(
            OpportunityKind::NormalCard,
            normal[act_index],
        ));
        kinds.extend(std::iter::repeat_n(
            OpportunityKind::Elite,
            elites[act_index],
        ));
        kinds.extend(std::iter::repeat_n(OpportunityKind::Shop, shops[act_index]));
        kinds.extend(std::iter::repeat_n(
            OpportunityKind::Treasure,
            treasures[act_index],
        ));
        kinds.extend(std::iter::repeat_n(
            OpportunityKind::Upgrade,
            upgrades[act_index],
        ));
        shuffle_java(&mut kinds, rng.random_long());
        let count = kinds.len();
        for (index, kind) in kinds.into_iter().enumerate() {
            let offset = (((index + 1) * 15) / (count + 1)).max(1) as i32;
            schedule.push(Opportunity {
                act: acts[act_index],
                floor: bases[act_index] + offset,
                kind,
                shop_purchase_slots: if kind == OpportunityKind::Shop {
                    shop_purchases_per_visit.sample(rng)
                } else {
                    0
                },
            });
        }
        if act_index < 2 {
            schedule.push(Opportunity {
                act: acts[act_index],
                floor: bosses[act_index],
                kind: OpportunityKind::BossReward,
                shop_purchase_slots: 0,
            });
        }
    }
    schedule
}

fn card_room(kind: OpportunityKind) -> RoomType {
    match kind {
        OpportunityKind::Elite => RoomType::Elite,
        OpportunityKind::BossReward => RoomType::Boss,
        _ => RoomType::Monster,
    }
}

fn auxiliary_decision_pending(game: &Game) -> bool {
    match game.screen {
        Screen::Grid | Screen::CardReward => true,
        Screen::CombatReward => game.rewards.iter().any(|reward| !reward.taken),
        _ => false,
    }
}

fn card_label(card: &crate::card::Card) -> String {
    format!("{}{}", card.sts_id(), if card.upgraded { "+" } else { "" })
}

fn living_enemy_hp(game: &Game) -> i32 {
    game.combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| !monster.dead && !monster.escaped)
                .map(|monster| monster.hp.max(0))
                .sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy_action(observation: &DraftObservation) -> usize {
        let choose = observation
            .offers
            .iter()
            .enumerate()
            .filter_map(|(index, offer)| {
                matches!(
                    offer.action,
                    FormationAction::Game {
                        action: Action::Choose { .. }
                    }
                )
                .then_some(index)
            })
            .collect::<Vec<_>>();
        if let Some(index) = observation.offers.iter().position(|offer| {
            matches!(
                offer.action,
                FormationAction::Game {
                    action: Action::Proceed
                }
            )
        }) {
            if observation.engine_screen == "Grid" {
                if !choose.is_empty() {
                    return choose[observation.metrics.decision_steps % choose.len()];
                }
            }
            return index;
        }
        if observation.engine_screen == "Grid" && !choose.is_empty() {
            return choose[observation.metrics.decision_steps % choose.len()];
        }
        observation
            .offers
            .iter()
            .position(|offer| {
                matches!(
                    offer.action,
                    FormationAction::Game {
                        action: Action::Skip
                    } | FormationAction::SkipUpgrade
                )
            })
            .unwrap_or(0)
    }

    fn finish(seed: i64) -> BossDraftEnv {
        let mut env =
            BossDraftEnv::fixture(seed, Character::Defect, DraftConfig::default()).unwrap();
        for _ in 0..200 {
            if env.ready_for_bosses() {
                return env;
            }
            let observation = env.observation();
            assert!(
                !observation.offers.is_empty(),
                "stalled at {}",
                observation.phase
            );
            env.step(policy_action(&observation)).unwrap();
        }
        panic!(
            "seed {seed} did not finish within 200 decisions: {:?}",
            env.observation()
        )
    }

    #[test]
    fn same_seed_has_same_counts_and_initial_offers() {
        let a = BossDraftEnv::fixture(77, Character::Defect, DraftConfig::default()).unwrap();
        let b = BossDraftEnv::fixture(77, Character::Defect, DraftConfig::default()).unwrap();
        assert_eq!(a.counts, b.counts);
        assert_eq!(a.observation(), b.observation());
        assert_eq!(a.observation().offers.len(), 4);
    }

    #[test]
    fn batch_monte_carlo_keeps_seed_slots_stable_through_boss_evaluation() {
        let seeds = [0, 1, 2, 3, 4, 5, 6, 7];
        let mut batch =
            BossDraftBatch::fixture(&seeds, Character::Defect, DraftConfig::default()).unwrap();
        let initial = batch.observations();
        assert_eq!(initial.len(), seeds.len());
        assert!(initial
            .iter()
            .all(|observation| observation.phase == "neow"));
        assert!(initial[1..].iter().any(|observation| {
            observation.offers != initial[0].offers
                || observation.sampled_counts != initial[0].sampled_counts
        }));

        batch.complete_with_htn_baseline(200).unwrap();
        assert_eq!(batch.ready_count(), seeds.len());

        let evaluations = batch.evaluate_htn(1).unwrap();
        assert_eq!(evaluations.len(), seeds.len());
        assert!(evaluations
            .iter()
            .all(|result| result.fights.len() == 7 && result.fights_started == 7));
    }

    #[test]
    fn external_boss_batch_exposes_intent_damage_and_accepts_indexed_actions() {
        let mut builds =
            BossDraftBatch::fixture(&[11], Character::Defect, DraftConfig::default()).unwrap();
        builds.complete_with_htn_baseline(200).unwrap();
        let mut fights = builds.start_boss_combats(1).unwrap();
        let observations = fights.observations();
        assert_eq!(observations.len(), 7);
        assert!(observations[4..]
            .iter()
            .all(|observation| observation.player_hp == 60 && observation.player_max_hp == 75));
        assert!(observations.iter().all(|observation| {
            !observation.done
                && !observation.offers.is_empty()
                && observation.monsters.iter().all(|monster| {
                    monster.intent_total_damage
                        == monster
                            .intent_damage_per_hit
                            .saturating_mul(monster.intent_hits.max(0))
                })
        }));

        let actions = fights.baseline_action_indices();
        assert!(actions.iter().all(Option::is_some));
        fights.step(&actions).unwrap();
        assert_eq!(fights.ready_count(), 7);
        let suites = fights.results().unwrap();
        assert_eq!(suites.len(), 1);
        assert_eq!(suites[0].fights_started, 7);
        assert_eq!(suites[0].timeouts, 7);
    }

    #[test]
    fn eight_fights_with_seven_losses_and_no_timeouts_has_one_win() {
        let fights = (0..8)
            .map(|index| BossFightResult {
                boss: format!("boss-{index}"),
                fought: true,
                won: index == 0,
                timed_out: false,
                combat_steps: 1,
                player_hp_remaining: i32::from(index == 0),
                initial_boss_hp: 100,
                boss_hp_remaining: if index == 0 { 0 } else { 50 },
                boss_damage_dealt: 50 + 50 * i32::from(index == 0),
            })
            .collect();
        let result = BossSuiteResult::from_fights(fights);
        assert_eq!(result.fights_started, 8);
        assert_eq!(result.losses, 7);
        assert_eq!(result.wins, 1);
        assert_eq!(result.timeouts, 0);
        assert_eq!(result.boss_hp_remaining_sum, 350);
        assert_eq!(result.boss_damage_dealt_sum, 450);
    }

    #[test]
    fn sampled_route_reports_shops_and_elites_and_finishes_compactly() {
        let env = finish(19);
        assert!((4..=8).contains(&env.counts.elite_opportunities));
        assert!((2..=4).contains(&env.counts.shops));
        assert!(env.counts.shop_purchase_slots >= env.counts.shops);
        assert!(env.counts.shop_purchase_slots <= env.counts.shops * 2);
        assert_eq!(
            env.metrics.elite_bundles_resolved,
            env.counts.elite_opportunities
        );
        assert_eq!(env.metrics.shops_visited, env.counts.shops);
        assert!(
            env.metrics.decision_steps < 100,
            "{} steps",
            env.metrics.decision_steps
        );
    }

    #[test]
    fn hundred_seed_average_stays_compact() {
        let mut decisions = 0usize;
        let mut normal = 0usize;
        let mut elites = 0usize;
        let mut shops = 0usize;
        let mut shop_slots = 0usize;
        for seed in 0..100 {
            let env = finish(seed);
            decisions += env.metrics.decision_steps;
            normal += env.counts.normal_card_rewards;
            elites += env.counts.elite_opportunities;
            shops += env.counts.shops;
            shop_slots += env.counts.shop_purchase_slots;
        }
        let mean_decisions = decisions as f32 / 100.0;
        eprintln!(
            "100-seed means: decisions={mean_decisions:.2} normal_cards={:.2} elites={:.2} shops={:.2} shop_slots={:.2}",
            normal as f32 / 100.0,
            elites as f32 / 100.0,
            shops as f32 / 100.0,
            shop_slots as f32 / 100.0
        );
        assert!(mean_decisions < 65.0, "mean decisions {mean_decisions}");
        assert!((16.0..=20.0).contains(&(normal as f32 / 100.0)));
        assert!((5.0..=7.0).contains(&(elites as f32 / 100.0)));
        assert!((2.5..=3.5).contains(&(shops as f32 / 100.0)));
        assert!((4.0..=5.5).contains(&(shop_slots as f32 / 100.0)));
    }

    #[test]
    fn suite_uses_act1_snapshot_for_all_seven_bosses() {
        let env = finish(3);
        let act1 = env
            .act1_boss_snapshot
            .as_ref()
            .expect("Act 1 snapshot should be captured");
        assert_eq!(act1.dungeon.act, Act::Exordium);
        assert!(act1.dungeon.floor < 16);
        let result = env.evaluate_htn(2_000);
        assert_eq!(result.fights.len(), 7);
        assert_eq!(result.fights_started, 7);
        assert_eq!(result.timeouts, 0);
        assert_eq!(
            result.wins + result.losses + result.timeouts,
            result.fights_started
        );
        assert_eq!(
            result
                .fights
                .iter()
                .map(|fight| fight.boss.as_str())
                .collect::<Vec<_>>(),
            [
                "Awakened One",
                "Time Eater",
                "Donu and Deca",
                "Corrupt Heart",
                "Slime Boss",
                "The Guardian",
                "Hexaghost"
            ]
        );
        assert_eq!(result.act1_fights_started, 3);
        assert_eq!(
            result.act1_wins,
            usize::from(result.fights[4].won)
                + usize::from(result.fights[5].won)
                + usize::from(result.fights[6].won)
        );
        assert_eq!(result.act1_all_won, result.act1_wins == 3);
        assert_eq!(
            result.boss_hp_remaining_sum,
            result
                .fights
                .iter()
                .map(|fight| fight.boss_hp_remaining)
                .sum::<i32>()
        );
        assert_eq!(
            result.boss_damage_dealt_sum,
            result
                .fights
                .iter()
                .map(|fight| fight.boss_damage_dealt)
                .sum::<i32>()
        );
        assert!(result.boss_damage_dealt_sum > 0);
    }
}
