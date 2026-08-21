use crate::action::{Action, PotionOp};
use crate::card::Card;
use crate::combat::{self, after_combat_relics, Combat};
use crate::creature::{Player, PotionInstance, RelicInstance};
use crate::dungeon::Dungeon;
use crate::ids::{CardId, CardRarity, CardType, Character, EncounterId, PotionId, RelicId, RelicTier, RoomType};
use crate::java_util::shuffle_java;
use crate::rng::{RngSet, StsRandom};
use crate::unlocks::Unlocks;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Neow,
    Map,
    Combat,
    CombatReward,
    CardReward,
    Rest,
    Treasure,
    BossRelic,
    Event,
    Shop,
    HandSelect,
    Grid,
    DoorUnlock,
    ActTransition,
    Terminal,
}

#[derive(Clone, Debug)]
pub struct Reward {
    pub kind: RewardKind,
    pub taken: bool,
    /// Bidirectional `RewardItem.relicLink` (chest relic ↔ sapphire key).
    relic_link: Option<usize>,
    /// RewardItem.cards, populated eagerly for multi-card rewards such as
    /// Orrery so every CARD item keeps its own roll.
    card_options: Option<Vec<Card>>,
}

impl Reward {
    fn new(kind: RewardKind) -> Self {
        Self {
            kind,
            taken: false,
            relic_link: None,
            card_options: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum RewardKind {
    Gold(i32),
    StolenGold(i32),
    Potion(PotionId),
    Relic(RelicId),
    Card,
    EmeraldKey,
    SapphireKey,
}

#[derive(Clone, Debug)]
pub struct EventState {
    pub id: String,
    pub screen: i32,
    pub options: Vec<String>,
    /// Event-specific deck indices or counters (Falling: skill/power/attack).
    pub data: Vec<i32>,
    /// The Library Read grid (20 unique cards).
    library_cards: Vec<Card>,
    /// GremlinMatchGame cards in shuffled table order.
    match_cards: Vec<MatchCard>,
    match_chosen: Option<usize>,
    match_attempts: i32,
}

#[derive(Clone, Copy, Debug)]
struct MatchCard {
    id: CardId,
    /// `AbstractCard.isFlipped`: face-down and still in the clickable set.
    flipped: bool,
    /// Revealed at least once; ExactTextSim labels these by `cardID`.
    revealed: bool,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub seed: i64,
    pub ascension: i32,
    pub character: Character,
    pub unlocks: Arc<Unlocks>,
    pub rng: RngSet,
    pub player: Player,
    pub dungeon: Dungeon,
    pub screen: Screen,
    pub combat: Option<Combat>,
    pub rewards: Vec<Reward>,
    pub card_reward: Vec<Card>,
    active_card_reward: Option<usize>,
    pub event: Option<EventState>,
    pub neow_options: Vec<NeowOption>,
    pub neow_screen: i32,
    pub neow_rng: StsRandom,
    pub boss_relics: Vec<RelicId>,
    pub current_room: RoomType,
    pub current_x: i32,
    pub current_y: i32,
    pub hand_select: Vec<usize>,
    pub done: bool,
    pub potion_blizzard: i32,
    pub card_blizz: i32,
    pub pending_cards: Vec<Card>,
    pending_gold: i32,
    pending_rest_heal: i32,
    pending_equip: Vec<RelicId>,
    pub event_elite_chance: f32,
    pub event_monster_chance: f32,
    pub event_shop_chance: f32,
    pub event_treasure_chance: f32,
    chest_gold: bool,
    chest_gold_amt: i32,
    chest_tier: RelicTier,
    hand_held: Vec<Card>,
    pending_room: Option<(i32, i32, RoomType)>,
    shop: ShopState,
    rest_smithing: bool,
    rest_smith_picked: bool,
    rest_selected: bool,
    has_ruby_key: bool,
    has_emerald_key: bool,
    has_sapphire_key: bool,
    final_act_available: bool,
    grid: Option<GridSelect>,
    exhaust_select: bool,
    put_on_deck_select: bool,
    gambling_select: bool,
    memories_select: bool,
    pending_shop_purge: Option<usize>,
    discovery_combat: bool,
    discovery_typ: Option<crate::ids::CardType>,
    discovery_colorless: bool,
    discovery_copies: usize,
    toolbox_reward: bool,
    /// ToyOrnithopter HealAction is addToBot after DiscoveryAction, so the
    /// CARD_REWARD snapshot is still pre-heal (seed 45).
    pending_ornithopter_heal: bool,
}

#[derive(Clone, Debug, Default)]
struct ShopState {
    open: bool,
    cards: Vec<crate::rewards::ShopOffer<Card>>,
    relics: Vec<crate::rewards::ShopOffer<RelicId>>,
    potions: Vec<crate::rewards::ShopOffer<crate::ids::PotionId>>,
    purge_cost: i32,
    purge_available: bool,
}

#[derive(Clone, Copy, Debug)]
enum ShopKind {
    Purge,
    Card(usize),
    Relic(usize),
    Potion(usize),
}

fn match_play_options(cards: &[MatchCard]) -> Vec<String> {
    cards
        .iter()
        .filter(|c| c.flipped)
        .map(|c| {
            if c.revealed {
                c.id.sts_id().to_string()
            } else {
                "hidden card".into()
            }
        })
        .collect()
}

fn purgeable_card(c: &Card) -> bool {
    !matches!(
        c.id,
        CardId::Necronomicurse | CardId::CurseOfTheBell | CardId::AscendersBane
    )
}

fn shop_card_matches(card: &Card, label: &str) -> bool {
    let id = card.sts_id();
    label == id
        || label == id.replace('_', " ")
        || (card.upgraded && (label == format!("{id}+") || label == format!("{}+", id.replace('_', " "))))
}

fn shop_relic_matches(id: RelicId, label: &str) -> bool {
    let sts = id.sts_id();
    label == sts || label.replace(' ', "") == sts.replace(' ', "")
}

fn shop_potion_matches(id: crate::ids::PotionId, label: &str) -> bool {
    let sts = id.sts_id();
    let name = potion_shop_name(id);
    label == sts || label == name || label.replace(' ', "") == sts.replace(' ', "")
}

fn potion_shop_name(id: crate::ids::PotionId) -> &'static str {
    match id {
        crate::ids::PotionId::Fairy => "Fairy in a Bottle",
        crate::ids::PotionId::EntropicBrew => "Entropic Brew",
        crate::ids::PotionId::Blood => "Blood Potion",
        crate::ids::PotionId::FruitJuice => "Fruit Juice",
        crate::ids::PotionId::HeartOfIron => "Heart of Iron",
        crate::ids::PotionId::Elixir => "Elixir",
        crate::ids::PotionId::LiquidBronze => "Liquid Bronze",
        crate::ids::PotionId::Duplication => "Duplication Potion",
        crate::ids::PotionId::GamblersBrew => "Gambler's Brew",
        crate::ids::PotionId::EssenceOfSteel => "Essence of Steel",
        crate::ids::PotionId::DistilledChaos => "Distilled Chaos",
        crate::ids::PotionId::LiquidMemories => "Liquid Memories",
        crate::ids::PotionId::Cultist => "Cultist Potion",
        crate::ids::PotionId::SneckoOil => "Snecko Oil",
        crate::ids::PotionId::SmokeBomb => "Smoke Bomb",
        crate::ids::PotionId::BlessingOfTheForge => "Blessing of the Forge",
        crate::ids::PotionId::Attack => "Attack Potion",
        crate::ids::PotionId::Skill => "Skill Potion",
        crate::ids::PotionId::Power => "Power Potion",
        crate::ids::PotionId::Colorless => "Colorless Potion",
        crate::ids::PotionId::Steroid => "Flex Potion",
        crate::ids::PotionId::Speed => "Speed Potion",
        crate::ids::PotionId::Fear => "Fear Potion",
        _ => id.sts_id(),
    }
}

#[derive(Clone, Debug)]
pub struct NeowOption {
    pub label: String,
    pub kind: NeowKind,
}

#[derive(Clone, Copy, Debug)]
pub enum NeowKind {
    ThreeCards,
    RandomRareCard,
    RemoveCard,
    UpgradeCard,
    TransformCard,
    RandomColorless,
    ThreePotions,
    RandomCommonRelic,
    TenHp,
    ThreeEnemyKill,
    HundredGold,
    RandomColorless2,
    RemoveTwo,
    RareRelic,
    ThreeRareCards,
    TwoFiftyGold,
    TransformTwo,
    TwentyHp,
    BossRelic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridKind {
    Purge,
    Upgrade,
    Transform,
    /// Dolly's Mirror: copy one master-deck card without bottle flags.
    Copy,
    /// Combat CardGroup select over the discard pile (Hologram).
    DiscardToHand,
    /// Combat CardGroup select over the draw pile (Seek).
    DrawPileToHand,
    /// SkillFromDeckToHandAction (Secret Technique): skills only.
    SkillFromDeck,
    /// Bottled Flame / Lightning / Tornado: one purgeable card of this type.
    Bottle(CardType),
    /// The Library Read: 20 unique cards, obtain one.
    Library,
}

#[derive(Clone, Debug)]
struct GridSelect {
    kind: GridKind,
    needed: usize,
    confirm: bool,
    hovered: Option<usize>,
    picked: Vec<usize>,
    return_event: bool,
    return_shop: bool,
    return_screen: Option<Screen>,
    /// ChoiceDriver.chooseGrid: forUpgrade/forTransform/forPurge wait for
    /// confirm; otherwise closeCurrentScreen after the click (BackToBasics).
    immediate: bool,
}

impl Game {
    pub fn new(seed: i64, character: Character, ascension: i32, unlocks: Unlocks) -> Self {
        let mut rng = RngSet::generate_seeds(seed);
        // MainMusic.getSong("Exordium") consumes miscRng.random(1)
        let _ = rng.misc.random_int(1);
        let dungeon = Dungeon::generate_exordium(seed, &mut rng, &unlocks, character, ascension);
        let final_act_available = unlocks.final_act_available;
        let mut game = Self {
            seed,
            ascension,
            character,
            unlocks: Arc::new(unlocks),
            rng,
            player: {
                let mut p = Player::for_character(character);
                p.apply_ascension(character, ascension);
                p
            },
            dungeon,
            screen: Screen::Neow,
            combat: None,
            rewards: Vec::new(),
            card_reward: Vec::new(),
            active_card_reward: None,
            event: None,
            neow_options: Vec::new(),
            neow_screen: 0,
            neow_rng: StsRandom::from_seed(seed),
            boss_relics: Vec::new(),
            current_room: RoomType::Neow,
            current_x: 0,
            current_y: -1,
            hand_select: Vec::new(),
            done: false,
            potion_blizzard: 0,
            card_blizz: 5,
            pending_cards: Vec::new(),
            pending_gold: 0,
            pending_rest_heal: 0,
            pending_equip: Vec::new(),
            event_elite_chance: 0.1,
            event_monster_chance: 0.1,
            event_shop_chance: 0.03,
            event_treasure_chance: 0.02,
            chest_gold: false,
            chest_gold_amt: 25,
            chest_tier: RelicTier::COMMON,
            hand_held: Vec::new(),
            pending_room: None,
            shop: ShopState {
                purge_cost: 75,
                purge_available: true,
                ..ShopState::default()
            },
            rest_smithing: false,
            rest_smith_picked: false,
            rest_selected: false,
            has_ruby_key: false,
            has_emerald_key: false,
            has_sapphire_key: false,
            final_act_available,
            grid: None,
            exhaust_select: false,
            put_on_deck_select: false,
            gambling_select: false,
            memories_select: false,
            pending_shop_purge: None,
            discovery_combat: false,
            discovery_typ: None,
            discovery_colorless: false,
            discovery_copies: 1,
            toolbox_reward: false,
            pending_ornithopter_heal: false,
        };
        game.neow_options = vec![NeowOption {
            label: "[Talk]".into(),
            kind: NeowKind::ThreeCards,
        }];
        game
    }

    pub fn has_ruby_key(&self) -> bool {
        self.has_ruby_key
    }

    pub fn has_emerald_key(&self) -> bool {
        self.has_emerald_key
    }

    pub fn has_sapphire_key(&self) -> bool {
        self.has_sapphire_key
    }

    pub fn final_act_available(&self) -> bool {
        self.final_act_available
    }

    pub fn legal_actions(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        match self.screen {
            Screen::Combat => {
                if let Some(combat) = &self.combat {
                    let velvet_choker_full = self.player.relics.iter().any(|r| {
                        r.id == RelicId::Velvet_Choker && r.counter >= 6
                    });
                    for (i, card) in self.player.hand.iter().enumerate() {
                        if velvet_choker_full
                            || crate::combat::status_or_curse_unplayable(card, &self.player)
                        {
                            continue;
                        }
                        if card.cost_for_turn > self.player.energy as i16 && card.cost_for_turn >= 0 {
                            continue;
                        }
                        if card.card_type() == crate::ids::CardType::ATTACK
                            && self.player.power_amount(crate::ids::PowerId::Entangled) > 0
                        {
                            continue;
                        }
                        if card.needs_target() {
                            for (t, _) in combat.living() {
                                actions.push(Action::Play {
                                    hand_index: i,
                                    target_index: Some(t),
                                });
                            }
                        } else {
                            actions.push(Action::Play {
                                hand_index: i,
                                target_index: None,
                            });
                        }
                    }
                    actions.push(Action::EndTurn);
                    for (slot, pot) in self.player.potions.iter().enumerate() {
                        if pot.id != PotionId::Slot {
                            if pot.id == PotionId::Fire
                                || pot.id == PotionId::Explosive
                                || pot.id == PotionId::Fear
                                || pot.id == PotionId::Weak
                            {
                                for (t, _) in combat.living() {
                                    actions.push(Action::Potion {
                                        action: PotionOp::Use,
                                        slot,
                                        target_index: Some(t),
                                    });
                                }
                            } else {
                                actions.push(Action::Potion {
                                    action: PotionOp::Use,
                                    slot,
                                    target_index: None,
                                });
                            }
                            actions.push(Action::Potion {
                                action: PotionOp::Discard,
                                slot,
                                target_index: None,
                            });
                        }
                    }
                }
            }
            Screen::Map => {
                for (idx, (x, y, room)) in self.map_choices().into_iter().enumerate() {
                    actions.push(Action::Choose {
                        index: idx,
                        label: Some("map node".into()),
                        x: Some(x),
                        y: Some(y),
                        room: Some(room.java_class().into()),
                    });
                }
            }
            Screen::Rest => {
                if self.rest_smithing {
                    if self.rest_smith_picked {
                        actions.push(Action::Proceed);
                    } else {
                        for (i, card) in self
                            .player
                            .deck
                            .iter()
                            .filter(|card| card.can_upgrade())
                            .enumerate()
                        {
                            actions.push(Action::Choose {
                                index: i,
                                label: Some(card.sts_id().to_string()),
                                x: None,
                                y: None,
                                room: None,
                            });
                        }
                        if actions.is_empty() {
                            actions.push(Action::Proceed);
                        }
                    }
                } else if self.rest_selected {
                    actions.push(Action::Proceed);
                } else {
                    for (i, kind) in self.campfire_options().into_iter().enumerate() {
                        actions.push(Action::Choose {
                            index: i,
                            label: Some(kind.into()),
                            x: None,
                            y: None,
                            room: None,
                        });
                    }
                }
            }
            Screen::Shop => {
                if !self.shop.open {
                    actions.push(Action::Choose {
                        index: 0,
                        label: Some("shop".into()),
                        x: None,
                        y: None,
                        room: None,
                    });
                } else {
                    for (index, kind) in self.shop_affordable().into_iter().enumerate() {
                        let label = match kind {
                            ShopKind::Purge => Some("purge".to_string()),
                            ShopKind::Card(i) => self
                                .shop
                                .cards
                                .get(i)
                                .map(|offer| offer.item.sts_id().to_string()),
                            ShopKind::Relic(i) => self
                                .shop
                                .relics
                                .get(i)
                                .map(|offer| offer.item.sts_id().to_string()),
                            ShopKind::Potion(i) => self
                                .shop
                                .potions
                                .get(i)
                                .map(|offer| offer.item.sts_id().to_string()),
                        };
                        actions.push(Action::Choose {
                            index,
                            label,
                            x: None,
                            y: None,
                            room: None,
                        });
                    }
                }
                actions.push(Action::Proceed);
            }
            Screen::Neow | Screen::Event | Screen::Treasure | Screen::BossRelic => {
                let n = match self.screen {
                    Screen::Neow => self.neow_options.len(),
                    Screen::Event => self.event.as_ref().map(|e| e.options.len()).unwrap_or(0),
                    Screen::Treasure => 1,
                    Screen::BossRelic => self.boss_relics.len() + 1,
                    _ => 0,
                };
                for i in 0..n {
                    let label = match self.screen {
                        Screen::Neow => self.neow_options.get(i).map(|o| o.label.clone()),
                        Screen::Event => self.event.as_ref().and_then(|e| e.options.get(i).cloned()),
                        Screen::Treasure => Some("open".into()),
                        Screen::BossRelic => self.boss_relics.get(i).map(|r| r.sts_id().to_string()),
                        _ => None,
                    };
                    actions.push(Action::Choose {
                        index: i,
                        label,
                        x: None,
                        y: None,
                        room: None,
                    });
                }
                if self.screen == Screen::BossRelic {
                    actions.push(Action::Proceed);
                }
            }
            Screen::Grid => {
                if let Some(grid) = &self.grid {
                    if grid.confirm {
                        actions.push(Action::Proceed);
                        actions.push(Action::Skip);
                    } else {
                        let cards = self.grid_card_indices(grid.kind);
                        for (i, &pile_i) in cards.iter().enumerate() {
                            if grid.picked.contains(&pile_i) {
                                continue;
                            }
                            let label = match grid.kind {
                                GridKind::DiscardToHand => {
                                    self.player.discard.get(pile_i).map(|c| c.sts_id().to_string())
                                }
                                _ => self.player.deck.get(pile_i).map(|c| c.sts_id().to_string()),
                            };
                            actions.push(Action::Choose {
                                index: i,
                                label,
                                x: None,
                                y: None,
                                room: None,
                            });
                        }
                    }
                }
            }
            Screen::CombatReward => {
                let mut compact = 0usize;
                for reward in self.rewards.iter() {
                    if !reward.taken {
                        let label = match reward.kind {
                            RewardKind::Gold(_) => "GOLD",
                            RewardKind::StolenGold(_) => "STOLEN_GOLD",
                            RewardKind::Potion(_) => "POTION",
                            RewardKind::Relic(_) => "RELIC",
                            RewardKind::Card => "CARD",
                            RewardKind::EmeraldKey => "EMERALD_KEY",
                            RewardKind::SapphireKey => "SAPPHIRE_KEY",
                        };
                        actions.push(Action::Choose {
                            index: compact,
                            label: Some(label.into()),
                            x: None,
                            y: None,
                            room: None,
                        });
                        compact += 1;
                    }
                }
                actions.push(Action::Proceed);
            }
            Screen::CardReward => {
                for (i, card) in self.card_reward.iter().enumerate() {
                    actions.push(Action::Choose {
                        index: i,
                        label: Some(card.sts_id().to_string()),
                        x: None,
                        y: None,
                        room: None,
                    });
                }
                if !self.toolbox_reward {
                    actions.push(Action::Skip);
                }
            }
            Screen::HandSelect => {
                for (i, card) in self.player.hand.iter().enumerate() {
                    actions.push(Action::Choose {
                        index: i,
                        label: Some(card.sts_id().to_string()),
                        x: None,
                        y: None,
                        room: None,
                    });
                }
                actions.push(Action::Proceed);
            }
            Screen::DoorUnlock | Screen::ActTransition | Screen::Terminal => {
                actions.push(Action::Proceed);
            }
        }
        actions
    }

    pub fn step(&mut self, action: &Action) {
        if matches!(action, Action::Quit) {
            self.done = true;
            self.screen = Screen::Terminal;
            return;
        }
        if let Some(dest) = self.pending_room.take() {
            let stay = matches!(
                action,
                Action::Choose {
                    label: Some(label),
                    ..
                } if label == "map node" || label == "boss"
            ) || matches!(action, Action::Potion { .. });
            if stay {
                self.pending_room = Some(dest);
            } else {
                self.enter_room(dest.0, dest.1, dest.2);
            }
        }
        if let Action::Choose { label: Some(label), .. } = action {
            if label.contains("Apparition") && label.contains("Accept") {
                let loss = ((self.player.max_hp as f32) * 0.5).ceil() as i32;
                let loss = loss.min(self.player.max_hp - 1).max(0);
                self.player.max_hp -= loss;
                if self.player.hp > self.player.max_hp {
                    self.player.hp = self.player.max_hp;
                }
            }
            match label.as_str() {
                "map node" | "boss" => {
                    if self.screen == Screen::Map {
                        self.step_map(action);
                        return;
                    }
                }
                "open" => {
                    self.step_treasure(action);
                    return;
                }
                "Rest" => {
                    self.step_rest(action);
                    return;
                }
                "shop" => {
                    self.step_shop(action);
                    return;
                }
                "GOLD" | "POTION" | "CARD" | "RELIC" | "STOLEN_GOLD" => {
                    if matches!(self.screen, Screen::CombatReward | Screen::Treasure) {
                        self.step_reward(action);
                        return;
                    }
                }
                _ => {}
            }
        }
        if let Action::Potion {
            action: op,
            slot,
            target_index,
        } = action
        {
            self.use_potion(*op, *slot, *target_index);
            if let Some(dest) = self.pending_room.take() {
                self.enter_room(dest.0, dest.1, dest.2);
            }
            return;
        }
        match self.screen {
            Screen::Neow => self.step_neow(action),
            Screen::Map => self.step_map(action),
            Screen::Combat => self.step_combat(action),
            Screen::CombatReward => self.step_reward(action),
            Screen::CardReward => self.step_card_reward(action),
            Screen::Rest => self.step_rest(action),
            Screen::Treasure => self.step_treasure(action),
            Screen::BossRelic => self.step_boss_relic(action),
            Screen::Event => self.step_event(action),
            Screen::Shop => self.step_shop(action),
            Screen::HandSelect => self.step_hand_select(action),
            Screen::Grid => self.step_grid(action),
            Screen::DoorUnlock => {
                if matches!(action, Action::Proceed) {
                    self.begin_next_act();
                }
            }
            Screen::ActTransition => {
                self.done = true;
                self.screen = Screen::Terminal;
            }
            Screen::Terminal => self.done = true,
        }
        if matches!(action, Action::Quit) {
            self.done = true;
            self.screen = Screen::Terminal;
        }
    }

    fn step_neow(&mut self, action: &Action) {
        let Action::Choose { index, .. } = action else {
            return;
        };
        if self.neow_screen == 0 {
            self.blessing();
            self.neow_screen = 3;
            return;
        }
        if self.neow_screen == 3 {
            if let Some(opt) = self.neow_options.get(*index).cloned() {
                self.apply_neow(opt.kind);
            }
            if matches!(
                self.screen,
                Screen::CombatReward | Screen::CardReward | Screen::Grid
            ) {
                self.neow_screen = 99;
                return;
            }
            self.present_neow_leave();
            return;
        }
        self.open_map();
    }

    /// ExactTextSim waits for ShowCardAndObtainEffect / FastCardObtainEffect
    /// before publishing Neow Leave, so pending obtains are already in
    /// masterDeck at that snapshot.
    fn present_neow_leave(&mut self) {
        self.flush_pending_cards();
        self.neow_options = vec![NeowOption {
            label: "[Leave]".into(),
            kind: NeowKind::ThreeCards,
        }];
        self.neow_screen = 99;
        self.screen = Screen::Neow;
    }

    fn blessing(&mut self) {
        self.neow_rng = StsRandom::from_seed(self.seed);
        let cat0 = [
            NeowKind::ThreeCards,
            NeowKind::RandomRareCard,
            NeowKind::RemoveCard,
            NeowKind::UpgradeCard,
            NeowKind::TransformCard,
            NeowKind::RandomColorless,
        ];
        let cat1 = [
            NeowKind::ThreePotions,
            NeowKind::RandomCommonRelic,
            NeowKind::TenHp,
            NeowKind::ThreeEnemyKill,
            NeowKind::HundredGold,
        ];
        let pick = |rng: &mut StsRandom, opts: &[NeowKind]| opts[rng.random_range(0, opts.len() as i32 - 1) as usize];
        let a = pick(&mut self.neow_rng, &cat0);
        let b = pick(&mut self.neow_rng, &cat1);
        let drawback = self.neow_rng.random_range(0, 3);
        // NeowReward.getRewardOptions(2) order, then NeowReward(3) still rolls
        // rng.random(0, 0) even though the boss-relic list has one entry.
        let mut cat2 = vec![NeowKind::RandomColorless2];
        if drawback != 2 {
            cat2.push(NeowKind::RemoveTwo);
        }
        cat2.push(NeowKind::RareRelic);
        cat2.push(NeowKind::ThreeRareCards);
        if drawback != 1 {
            cat2.push(NeowKind::TwoFiftyGold);
        }
        cat2.push(NeowKind::TransformTwo);
        if drawback != 0 {
            cat2.push(NeowKind::TwentyHp);
        }
        let c = pick(&mut self.neow_rng, &cat2);
        let _ = self.neow_rng.random_range(0, 0);
        self.neow_options = vec![
            NeowOption {
                label: format!("{a:?}"),
                kind: a,
            },
            NeowOption {
                label: format!("{b:?}"),
                kind: b,
            },
            NeowOption {
                label: format!("{c:?}"),
                kind: c,
            },
            NeowOption {
                label: "Boss Relic".into(),
                kind: NeowKind::BossRelic,
            },
        ];
    }

    fn apply_neow(&mut self, kind: NeowKind) {
        match kind {
            NeowKind::RandomRareCard => {
                if let Some(id) = self.random_card(CardRarity::RARE, true) {
                    self.pending_cards.push(Card::new(id));
                }
            }
            NeowKind::HundredGold => self.player.gold += 100,
            NeowKind::TenHp => {
                let bonus = self.player.max_hp / 10;
                self.player.max_hp += bonus;
                self.player.hp += bonus;
            }
            NeowKind::ThreePotions => {
                // CombatRewardScreen.open() always rolls a CARD reward, then
                // Neow removes that item. The 9 cardRng calls and blizzard
                // mutation still happen.
                self.generate_card_reward();
                self.card_reward.clear();
                self.rewards.clear();
                for _ in 0..3 {
                    let p = crate::rewards::get_random_potion_for(&mut self.rng, self.character);
                    self.rewards.push(Reward::new(RewardKind::Potion(p)));
                }
                self.screen = Screen::CombatReward;
            }
            NeowKind::ThreeEnemyKill => {
                self.player.relics.push(RelicInstance {
                    id: RelicId::NeowsBlessing,
                    counter: 3,
                    used_up: false,
                });
            }
            NeowKind::RandomCommonRelic => {
                if let Some(id) = self.take_relic(RelicTier::COMMON) {
                    self.gain_relic(id);
                }
            }
            NeowKind::RareRelic => {
                if let Some(id) = self.take_relic(RelicTier::RARE) {
                    self.gain_relic(id);
                }
            }
            NeowKind::BossRelic => {
                if let Some(id) = self.take_relic(RelicTier::BOSS) {
                    if !self.player.relics.is_empty() {
                        self.player.relics.remove(0);
                    }
                    self.gain_relic(id);
                }
            }
            NeowKind::RandomColorless | NeowKind::RandomColorless2 => {
                let rare_only = matches!(kind, NeowKind::RandomColorless2);
                self.card_reward = crate::rewards::neow_colorless_cards(&self.dungeon, &mut self.rng, 3, rare_only);
                self.screen = Screen::CardReward;
            }
            NeowKind::RemoveCard => self.open_grid(GridKind::Purge, 1, false),
            NeowKind::UpgradeCard => self.open_grid(GridKind::Upgrade, 1, false),
            NeowKind::TransformCard => self.open_grid(GridKind::Transform, 1, false),
            NeowKind::RemoveTwo => self.open_grid(GridKind::Purge, 2, false),
            NeowKind::TransformTwo => self.open_grid(GridKind::Transform, 2, false),
            NeowKind::ThreeCards => {
                self.card_reward = self.neow_colored_cards(3, false);
                self.screen = Screen::CardReward;
            }
            NeowKind::ThreeRareCards => {
                self.card_reward = self.neow_colored_cards(3, true);
                self.screen = Screen::CardReward;
            }
            _ => {}
        }
    }

    fn open_library_grid(&mut self) {
        if std::env::var_os("STS_WALK_INK").is_some() {
            eprintln!(
                "library_pre blizz={} card_rng={} commons={:?}",
                self.card_blizz,
                self.rng.card.counter,
                self.dungeon
                    .common_cards
                    .iter()
                    .map(|id| id.sts_id())
                    .collect::<Vec<_>>()
            );
        }
        let cards = self.generate_library_cards();
        if std::env::var_os("STS_WALK_INK").is_some() {
            eprintln!(
                "library_blizz={} card_rng={} commons={} uncommons={} rares={} library_cards={:?}",
                self.card_blizz,
                self.rng.card.counter,
                self.dungeon.common_cards.len(),
                self.dungeon.uncommon_cards.len(),
                self.dungeon.rare_cards.len(),
                cards.iter().map(|c| c.sts_id()).collect::<Vec<_>>()
            );
        }
        if let Some(event) = self.event.as_mut() {
            event.library_cards = cards;
            event.screen = 1;
            event.options = vec!["[Leave]".into()];
        }
        self.grid = Some(GridSelect {
            kind: GridKind::Library,
            needed: 1,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: true,
            return_shop: false,
            return_screen: None,
            immediate: true,
        });
        self.screen = Screen::Grid;
    }

    /// The Library Read: `getCard(rollRarity())` until 20 unique cardIDs.
    /// `getCard(rarity)` uses `getRandomCard(true)` = cardRng, not MathUtils.
    fn generate_library_cards(&mut self) -> Vec<Card> {
        let mut out = Vec::new();
        for _ in 0..20 {
            let mut card = self.roll_library_card();
            let mut guard = 0;
            while out.iter().any(|c: &Card| c.id == card.id) && guard < 40 {
                card = self.roll_library_card();
                guard += 1;
            }
            crate::rewards::preview_obtain(&self.player, &mut card);
            // CardGroup.addToBottom inserts at index 0.
            out.insert(0, card);
        }
        out
    }

    fn roll_library_card(&mut self) -> Card {
        // AbstractDungeon.rollRarity: cardRng.random(99) + cardBlizzRandomizer.
        // Unlike getRewardCards, Library does not mutate cardBlizzRandomizer.
        let mut roll = self.rng.card.random_int(99);
        roll += self.card_blizz;
        let rarity = if roll < 3 {
            CardRarity::RARE
        } else if roll < 40 {
            CardRarity::UNCOMMON
        } else {
            CardRarity::COMMON
        };
        let pool = match rarity {
            CardRarity::RARE => &self.dungeon.rare_cards,
            CardRarity::UNCOMMON => &self.dungeon.uncommon_cards,
            _ => &self.dungeon.common_cards,
        };
        if pool.is_empty() {
            return Card::new(CardId::Zap);
        }
        Card::new(pool[self.rng.card.random_int(pool.len() as i32 - 1) as usize])
    }

    fn open_grid(&mut self, kind: GridKind, needed: usize, return_event: bool) {
        self.grid = Some(GridSelect {
            kind,
            needed,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event,
            return_shop: false,
            return_screen: None,
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    /// EmptyCage.onEquip: GRID of 2 purgeable master-deck cards. size<=2
    /// deletes them immediately (no overlay). Keep the previous screen so
    /// BossRelic Proceed still runs the act transition (seed 723 doubled Cage).
    /// TinyHouse.onEquip: upgrade 1 random upgradeable (miscRng), +5 max HP,
    /// addGoldToRewards(50), addPotionToRewards(getRandomPotion(miscRng)),
    /// then CombatRewardScreen.open which copies room.rewards and adds CARD
    /// (TreasureRoomBoss is not TreasureRoom). Seed 906 hp 36 vs 41.
    fn on_equip_tiny_house(&mut self) {
        let seed = self.rng.misc.random_long();
        let mut idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| c.can_upgrade())
            .map(|(i, _)| i)
            .collect();
        crate::java_util::shuffle_java(&mut idxs, seed);
        if let Some(&idx) = idxs.first() {
            if let Some(card) = self.player.deck.get_mut(idx) {
                card.upgrade();
            }
        }
        self.increase_max_hp(5);
        self.rewards.clear();
        self.add_gold_to_rewards(50);
        let potion = crate::rewards::get_random_potion_misc(&mut self.rng, self.character);
        self.rewards.push(Reward::new(RewardKind::Potion(potion)));
        self.rewards.push(Reward {
            kind: RewardKind::Card,
            taken: false,
            relic_link: None,
            card_options: None,
        });
        self.generate_card_reward();
        self.screen = Screen::CombatReward;
    }

    /// Astrolabe.onEquip: GRID of 3 purgeable cards, then transformCard(c, true, miscRng).
    fn open_astrolabe_grid(&mut self) {
        let idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| purgeable_card(c))
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            return;
        }
        if idxs.len() <= 3 {
            self.apply_astrolabe_transforms(&idxs);
            return;
        }
        let prev = self.screen;
        self.open_grid(GridKind::Transform, 3, false);
        if let Some(grid) = self.grid.as_mut() {
            grid.return_screen = Some(prev);
        }
    }

    fn apply_astrolabe_transforms(&mut self, idxs: &[usize]) {
        // Astrolabe.giveCards iterates GridSelectScreen.selectedCards in click
        // order. Preserve that order for the miscRng transforms: the source
        // card is temporarily excluded from each roll's pool.
        let mut selected = Vec::new();
        for &i in idxs {
            if !selected.iter().any(|(selected_i, _)| *selected_i == i) {
                if let Some(card) = self.player.deck.get(i) {
                    selected.push((i, card.id));
                }
            }
        }
        let mut remove_indices: Vec<usize> = selected.iter().map(|(i, _)| *i).collect();
        remove_indices.sort_unstable();
        for i in remove_indices.into_iter().rev() {
            self.remove_master_deck_card(i);
        }
        for (_, old) in selected {
            if let Some(id) = self.misc_transform_roll(old) {
                let mut card = Card::new(id);
                if card.can_upgrade() {
                    card.upgrade();
                }
                self.player.deck.push(card);
            }
        }
    }

    /// AbstractDungeon.returnTrulyRandomCardFromAvailable via miscRng.
    fn misc_transform_roll(&mut self, avoid: CardId) -> Option<CardId> {
        if avoid.def().color == crate::ids::CardColor::COLORLESS {
            // transformCard routes colorless sources through
            // returnTrulyRandomColorlessCardFromAvailable, whose list is the
            // addToBottom copy held in srcColorlessCardPool.
            let mut pool = self.dungeon.src_colorless_cards.as_ref().clone();
            pool.retain(|id| *id != avoid);
            if pool.is_empty() {
                return None;
            }
            let idx = self.rng.misc.random_int(pool.len() as i32 - 1) as usize;
            return Some(pool[idx]);
        }
        let mut pool: Vec<CardId> = self.dungeon.common_cards.as_ref().clone();
        let mut uncommons = self.dungeon.uncommon_cards.as_ref().clone();
        uncommons.reverse();
        let mut rares = self.dungeon.rare_cards.as_ref().clone();
        rares.reverse();
        pool.extend(uncommons);
        pool.extend(rares);
        pool.retain(|id| *id != avoid);
        if pool.is_empty() {
            return None;
        }
        let idx = self.rng.misc.random_int(pool.len() as i32 - 1) as usize;
        Some(pool[idx])
    }

    fn open_empty_cage_grid(&mut self) {
        let idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| purgeable_card(c) && !c.in_bottle)
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            return;
        }
        if idxs.len() <= 2 {
            for i in idxs.into_iter().rev() {
                self.remove_master_deck_card(i);
            }
            return;
        }
        let prev = self.screen;
        self.open_grid(GridKind::Purge, 2, false);
        if let Some(grid) = self.grid.as_mut() {
            grid.return_screen = Some(prev);
        }
    }

    fn open_bottle_grid(&mut self, typ: CardType) {
        let any = self
            .player
            .deck
            .iter()
            .any(|c| purgeable_card(c) && c.card_type() == typ);
        if !any {
            return;
        }
        let prev = self.screen;
        self.grid = Some(GridSelect {
            kind: GridKind::Bottle(typ),
            needed: 1,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: prev == Screen::Shop,
            return_screen: Some(prev),
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    fn open_dollys_mirror_grid(&mut self) {
        if self.player.deck.is_empty() {
            return;
        }
        let prev = self.screen;
        self.grid = Some(GridSelect {
            kind: GridKind::Copy,
            needed: 1,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: prev == Screen::Shop,
            return_screen: Some(prev),
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    /// NeowReward.getRewardCards: rarity via neowRng.randomBoolean(0.33) uncommon else common.
    fn neow_colored_cards(&mut self, n: usize, rare_only: bool) -> Vec<Card> {
        let mut out = Vec::new();
        for _ in 0..n {
            // rollRarity always consumes neowRng.randomBoolean(0.33), even when rareOnly.
            let rolled = if self.neow_rng.random_boolean_chance(0.33) {
                CardRarity::UNCOMMON
            } else {
                CardRarity::COMMON
            };
            let rarity = if rare_only {
                CardRarity::RARE
            } else {
                rolled
            };
            let pool: &[CardId] = match rarity {
                CardRarity::RARE => &self.dungeon.rare_cards,
                CardRarity::UNCOMMON => &self.dungeon.uncommon_cards,
                _ => &self.dungeon.common_cards,
            };
            if pool.is_empty() {
                continue;
            }
            let mut chosen = pool[self.neow_rng.random_int(pool.len() as i32 - 1) as usize];
            let mut guard = 0;
            while out.iter().any(|c: &Card| c.id == chosen) && guard < 20 {
                chosen = pool[self.neow_rng.random_int(pool.len() as i32 - 1) as usize];
                guard += 1;
            }
            out.push(Card::new(chosen));
        }
        out
    }

    fn grid_card_indices(&self, kind: GridKind) -> Vec<usize> {
        if kind == GridKind::DiscardToHand {
            return (0..self.player.discard.len()).collect();
        }
        if kind == GridKind::DrawPileToHand {
            return seek_draw_grid_indices(&self.player.draw);
        }
        if kind == GridKind::SkillFromDeck {
            return self
                .combat
                .as_ref()
                .map(|c| c.skill_from_deck.clone())
                .unwrap_or_default();
        }
        if kind == GridKind::Library {
            let n = self
                .event
                .as_ref()
                .map(|e| e.library_cards.len())
                .unwrap_or(0);
            return (0..n).collect();
        }
        if let GridKind::Bottle(typ) = kind {
            // CardGroup.getCardsOfType uses addToBottom (insert at 0), reversing
            // master-deck order. getPurgeableCards then getSkills/Attacks/Powers.
            let mut idxs: Vec<usize> = self
                .player
                .deck
                .iter()
                .enumerate()
                .filter(|(_, c)| purgeable_card(c) && c.card_type() == typ)
                .map(|(i, _)| i)
                .collect();
            idxs.reverse();
            return idxs;
        }
        self.player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| match kind {
                GridKind::Upgrade => c.can_upgrade(),
                GridKind::Purge => purgeable_card(c) && !c.in_bottle,
                GridKind::Transform => purgeable_card(c),
                GridKind::DiscardToHand
                | GridKind::DrawPileToHand
                | GridKind::SkillFromDeck
                | GridKind::Bottle(_)
                | GridKind::Library
                | GridKind::Copy => true,
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn step_grid(&mut self, action: &Action) {
        let Some(grid) = self.grid.as_ref() else {
            self.screen = Screen::Neow;
            return;
        };
        let kind = grid.kind;
        let needed = grid.needed;
        let confirm = grid.confirm;
        match action {
            Action::Choose { index, .. } if !confirm => {
                let cards = self.grid_card_indices(kind);
                let Some(&pile_i) = cards.get(*index) else {
                    return;
                };
                // ChoiceDriver.chooseGrid: forUpgrade/forTransform/forPurge wait
                // for confirm; otherwise closeCurrentScreen after the click.
                // BetterDrawPileToHandAction(Seek+) opens with numCards=2;
                // selectedCards stay until the count is met (seed 96 GRID
                // still open after Choose 3, Melter is the second pick).
                let combat_multi = matches!(
                    kind,
                    GridKind::DiscardToHand
                        | GridKind::DrawPileToHand
                        | GridKind::SkillFromDeck
                ) && needed > 1;
                if !combat_multi
                    && (matches!(
                        kind,
                        GridKind::DiscardToHand
                            | GridKind::DrawPileToHand
                            | GridKind::SkillFromDeck
                            | GridKind::Bottle(_)
                            | GridKind::Library
                            | GridKind::Copy
                    ) || self.grid.as_ref().is_some_and(|g| g.immediate))
                {
                    self.apply_grid(kind, &[pile_i]);
                    return;
                }
                if let Some(grid) = self.grid.as_mut() {
                    if needed == 1 {
                        grid.hovered = Some(pile_i);
                        grid.confirm = true;
                    } else {
                        if !grid.picked.contains(&pile_i) {
                            grid.picked.push(pile_i);
                        }
                        if grid.picked.len() >= needed {
                            let picked = grid.picked.clone();
                            self.apply_grid(kind, &picked);
                        }
                    }
                }
            }
            Action::Proceed if confirm => {
                let shop_purge = self.grid.as_ref().is_some_and(|g| g.return_shop);
                if shop_purge {
                    // Grid confirm queues the selected card. Some ExactTextSim
                    // walks consume it immediately; others only on shop skip.
                    // Apply now so Proceed matches Java updatePurge.
                    self.pending_shop_purge = self.grid.as_ref().and_then(|g| g.hovered);
                    self.apply_pending_shop_purge();
                    self.finish_grid();
                    return;
                }
                let hovered = self.grid.as_ref().and_then(|g| g.hovered);
                if let Some(i) = hovered {
                    self.apply_grid(kind, &[i]);
                } else {
                    self.finish_grid();
                }
            }
            Action::Skip => self.finish_grid(),
            _ => {}
        }
    }

    fn apply_grid(&mut self, kind: GridKind, indices: &[usize]) {
        let mut selection_order = Vec::new();
        for &i in indices {
            if !selection_order.contains(&i) {
                selection_order.push(i);
            }
        }
        let mut idxs = selection_order.clone();
        idxs.sort_unstable();
        match kind {
            GridKind::Purge => {
                let shop_purge = self.grid.as_ref().is_some_and(|g| g.return_shop);
                if shop_purge {
                    if self.player.gold < self.shop.purge_cost || !self.shop.purge_available {
                        self.finish_grid();
                        return;
                    }
                    self.spend_shop_gold(self.shop.purge_cost);
                    self.shop.purge_available = false;
                    self.shop.purge_cost += 25;
                }
                let bonfire = self.grid.as_ref().is_some_and(|g| g.return_event)
                    && self.event.as_ref().is_some_and(|e| e.id == "Bonfire Elementals");
                let designer_full = self.grid.as_ref().is_some_and(|g| g.return_event)
                    && self.event.as_ref().is_some_and(|e| {
                        e.id == "Designer" && e.data.get(6).copied().unwrap_or(0) != 0
                    });
                for i in idxs.into_iter().rev() {
                    if i < self.player.deck.len() {
                        if bonfire {
                            self.apply_bonfire_offer(self.player.deck[i].rarity());
                        }
                        self.remove_master_deck_card(i);
                    }
                }
                if designer_full {
                    // Designer REMOVE_AND_UPGRADE: shuffle remaining upgradables
                    // with miscRng.randomLong and upgrade the first.
                    let seed = self.rng.misc.random_long();
                    let mut up: Vec<usize> = self
                        .player
                        .deck
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| c.can_upgrade())
                        .map(|(i, _)| i)
                        .collect();
                    shuffle_java(&mut up, seed);
                    if let Some(&i) = up.first() {
                        if let Some(c) = self.player.deck.get_mut(i) {
                            c.upgrade();
                        }
                    }
                }
            }
            GridKind::Upgrade => {
                for i in idxs {
                    if let Some(c) = self.player.deck.get_mut(i) {
                        c.upgrade();
                    }
                }
            }
            GridKind::Transform => {
                let astrolabe = self.grid.as_ref().is_some_and(|g| {
                    g.needed == 3 && g.return_screen == Some(Screen::BossRelic)
                });
                if astrolabe {
                    // Astrolabe.giveCards: transformCard(c, true, miscRng) and
                    // obtain immediately (seed 133 Gash+/White Noise+/Steam+).
                    self.apply_astrolabe_transforms(&selection_order);
                } else {
                    // Java NeowReward.update TRANSFORM_*: transformCard via
                    // NeowEvent.rng, remove immediately, then queue
                    // ShowCardAndObtainEffect. ExactTextSim waits for that VFX
                    // before publishing Leave, so the replacement is flushed there.
                    for i in idxs.into_iter().rev() {
                        if i < self.player.deck.len() {
                            let old = self.player.deck[i].id;
                            let rolled = self.neow_transform_roll(old);
                            self.remove_master_deck_card(i);
                            if let Some(id) = rolled {
                                self.pending_cards.push(Card::new(id));
                            }
                        }
                    }
                }
            }
            GridKind::Copy => {
                for i in idxs {
                    if let Some(mut card) = self.player.deck.get(i).cloned() {
                        // DollysMirror.update uses makeStatEquivalentCopy, then
                        // explicitly clears all three bottle flags before the
                        // CardObtainTransition adds it to the master deck.
                        card.in_bottle = false;
                        self.player.deck.push(card);
                    }
                }
            }
            GridKind::DiscardToHand => {
                if self.memories_select {
                    // BetterDiscardPileToHandAction iterates selectedCards in
                    // click order and sets each moved card's cost for turn.
                    let room = 10usize.saturating_sub(self.player.hand.len());
                    let chosen: Vec<Card> = selection_order
                        .iter()
                        .take(room)
                        .filter_map(|&i| self.player.discard.get(i).cloned())
                        .collect();
                    let mut remove: Vec<usize> = selection_order
                        .iter()
                        .take(room)
                        .copied()
                        .filter(|&i| i < self.player.discard.len())
                        .collect();
                    remove.sort_unstable();
                    for i in remove.into_iter().rev() {
                        self.player.discard.remove(i);
                    }
                    for mut card in chosen {
                        card.cost_for_turn = 0;
                        self.player.hand.push(card);
                    }
                } else {
                    for i in idxs.into_iter().rev() {
                        combat::discard_pile_to_hand(&mut self.player, i);
                    }
                }
            }
            GridKind::DrawPileToHand | GridKind::SkillFromDeck => {
                for i in idxs.into_iter().rev() {
                    combat::draw_pile_to_hand(&mut self.player, i);
                }
            }
            GridKind::Bottle(_) => {
                for i in idxs {
                    if let Some(c) = self.player.deck.get_mut(i) {
                        c.in_bottle = true;
                    }
                }
            }
            GridKind::Library => {
                let cards = self
                    .event
                    .as_ref()
                    .map(|e| e.library_cards.clone())
                    .unwrap_or_default();
                for i in idxs {
                    if let Some(card) = cards.get(i).cloned() {
                        self.pending_cards.push(card);
                    }
                }
            }
        }
        self.finish_grid();
    }

    fn neow_transform_roll(&mut self, avoid: CardId) -> Option<CardId> {
        // AbstractDungeon.returnTrulyRandomCardFromAvailable (colored):
        // commonCardPool (running, addToTop=append) then srcUncommonCardPool
        // and srcRareCardPool. src pools are copied with addToBottom, which
        // reverses each rarity relative to the running pools.
        let mut pool: Vec<CardId> = self.dungeon.common_cards.as_ref().clone();
        let mut uncommons = self.dungeon.uncommon_cards.as_ref().clone();
        uncommons.reverse();
        let mut rares = self.dungeon.rare_cards.as_ref().clone();
        rares.reverse();
        pool.extend(uncommons);
        pool.extend(rares);
        pool.retain(|id| *id != avoid);
        if pool.is_empty() {
            return None;
        }
        let idx = self.neow_rng.random_int(pool.len() as i32 - 1) as usize;
        Some(pool[idx])
    }

    fn apply_bonfire_offer(&mut self, rarity: crate::ids::CardRarity) {
        match rarity {
            crate::ids::CardRarity::CURSE => {
                if !self.player.has_relic(RelicId::Spirit_Poop) {
                    self.gain_relic(RelicId::Spirit_Poop);
                }
            }
            crate::ids::CardRarity::BASIC => {}
            crate::ids::CardRarity::COMMON | crate::ids::CardRarity::SPECIAL => {
                self.player.hp = (self.player.hp + 5).min(self.player.max_hp);
            }
            crate::ids::CardRarity::UNCOMMON => {
                self.player.hp = self.player.max_hp;
            }
            crate::ids::CardRarity::RARE => {
                self.player.max_hp += 10;
                self.player.hp = self.player.max_hp;
            }
        }
        if let Some(event) = self.event.as_mut() {
            event.screen = 2;
            event.options = vec!["[Leave]".into()];
        }
    }

    fn finish_grid(&mut self) {
        let memories = self.memories_select;
        let back_to_combat = self
            .grid
            .as_ref()
            .is_some_and(|g| {
                matches!(
                    g.kind,
                    GridKind::DiscardToHand | GridKind::DrawPileToHand | GridKind::SkillFromDeck
                )
            });
        let back_to_event = self.grid.as_ref().is_some_and(|g| g.return_event);
        let back_to_shop = self.grid.as_ref().is_some_and(|g| g.return_shop);
        let return_screen = self.grid.as_ref().and_then(|g| g.return_screen);
        self.grid = None;
        if memories {
            self.memories_select = false;
            self.screen = Screen::Combat;
            self.apply_pending_ornithopter_heal();
            return;
        }
        if back_to_combat {
            self.finish_discard_to_hand();
            return;
        }
        if let Some(screen) = return_screen {
            if screen != Screen::Grid {
                self.screen = screen;
                return;
            }
        }
        if back_to_shop {
            self.screen = Screen::Shop;
            return;
        }
        if back_to_event {
            self.flush_pending_cards();
            if self.event.as_ref().is_some_and(|e| e.id == "Bonfire Elementals") {
                self.screen = Screen::Event;
                return;
            }
            if let Some(event) = self.event.as_mut() {
                // Wheel applyResult already moved to LEAVE before opening GRID.
                event.screen = match event.id.as_str() {
                    "Wheel of Change" => 3,
                    // Designer.buttonEffect sets curScreen=DONE before opening
                    // its upgrade/remove/transform grid.
                    "Designer" => 2,
                    _ => 1,
                };
                event.options = vec!["[Leave]".into()];
            }
            self.screen = Screen::Event;
        } else {
            self.present_neow_leave();
        }
    }

    fn random_card(&mut self, rarity: CardRarity, use_neow_rng: bool) -> Option<CardId> {
        let pool = match rarity {
            CardRarity::COMMON => &self.dungeon.common_cards,
            CardRarity::UNCOMMON => &self.dungeon.uncommon_cards,
            CardRarity::RARE => &self.dungeon.rare_cards,
            _ => &self.dungeon.colorless_cards,
        };
        if pool.is_empty() {
            return None;
        }
        let idx = if use_neow_rng {
            self.neow_rng.random_int(pool.len() as i32 - 1) as usize
        } else {
            self.rng.card.random_int(pool.len() as i32 - 1) as usize
        };
        Some(pool[idx])
    }

    fn open_map(&mut self) {
        self.flush_pending_cards();
        self.flush_pending_equip();
        self.event = None;
        self.screen = Screen::Map;
        // Keep current_room. EventHelper.roll runs before setCurrMapNode, so a
        // ShopRoom still current zeros shopSize for the next ? node.
    }

    fn map_choices(&self) -> Vec<(i32, i32, RoomType)> {
        let mut out = Vec::new();
        if !self.dungeon.first_room_chosen {
            for node in &self.dungeon.map.nodes[0] {
                if node.has_edges() {
                    out.push((node.x, node.y, node.room.unwrap_or(RoomType::Monster)));
                }
            }
            return out;
        }
        if self.current_y < 0 {
            return out;
        }
        if self.current_y >= 13 {
            out.push((-1, 15, RoomType::Boss));
            return out;
        }
        let node = self.dungeon.map.node(self.current_x, self.current_y);
        for edge in &node.edges {
            let dest = self.dungeon.map.node(edge.dst_x, edge.dst_y);
            out.push((dest.x, dest.y, dest.room.unwrap_or(RoomType::Monster)));
        }
        out
    }

    fn step_map(&mut self, action: &Action) {
        let Action::Choose { index, x, y, .. } = action else {
            return;
        };
        let choices = self.map_choices();
        if matches!(action, Action::Choose { label: Some(label), .. } if label == "boss") {
            self.enter_room(-1, 15, RoomType::Boss);
            return;
        }
        let (mx, my, room) = if let (Some(x), Some(y)) = (*x, *y) {
            choices
                .into_iter()
                .find(|c| c.0 == x && c.1 == y)
                .unwrap_or_else(|| {
                    let room = if y >= 0 && (y as usize) < self.dungeon.map.height() && x >= 0 {
                        self.dungeon
                            .map
                            .node(x, y)
                            .room
                            .unwrap_or(RoomType::Monster)
                    } else {
                        RoomType::Monster
                    };
                    (x, y, room)
                })
        } else {
            match choices.get(*index).copied() {
                Some(choice) => choice,
                None => return,
            }
        };
        // Map node click always enters. Fruit Juice is usable on the map as a
        // Potion action before this choose; deferring entry left rust on Map
        // while Java was already in the first hallway (191892).
        self.enter_room(mx, my, room);
    }

    fn reset_player_between_rooms(&mut self) {
        // AbstractDungeon.resetPlayer: lose leftover combat block/powers/piles.
        self.player.block = 0;
        self.player.powers.clear();
        self.player.hand.clear();
        self.player.draw.clear();
        self.player.discard.clear();
        self.player.exhaust.clear();
    }

    fn enter_room(&mut self, x: i32, y: i32, room: RoomType) {
        // EventHelper.roll runs before setCurrMapNode, so getCurrRoom() is still
        // the previous room (ShopRoom zeros shopSize).
        let prev_room = self.current_room;
        self.event = None;
        if self.pending_gold != 0 {
            self.player.gold += self.gold_with_idol(self.pending_gold);
            self.pending_gold = 0;
        }
        if self.pending_rest_heal != 0 {
            self.player.hp = (self.player.hp + self.pending_rest_heal).min(self.player.max_hp);
            self.pending_rest_heal = 0;
        }
        self.reset_player_between_rooms();
        self.dungeon.first_room_chosen = true;
        self.current_x = x;
        self.current_y = y;
        self.current_room = room;
        self.dungeon.path_x.push(x);
        self.dungeon.path_y.push(y);
        self.dungeon.floor += 1;
        self.rng.reset_floor_streams(self.seed, self.dungeon.floor);
        self.maw_bank_on_enter_room();
        if y >= 0 && y < self.dungeon.map.height() as i32 && x >= 0 {
            std::sync::Arc::make_mut(&mut self.dungeon.map).node_mut(x, y).taken = true;
        }
        match room {
            RoomType::Monster | RoomType::Elite | RoomType::Boss => self.start_combat_in_current_room(),
            RoomType::Rest => {
                self.rest_smithing = false;
                self.rest_smith_picked = false;
                self.rest_selected = false;
                // RestRoom.onPlayerEntry: every relic.onEnterRestRoom.
                // AncientTeaSet sets counter = -2 (armed for the next fight).
                if let Some(r) = self
                    .player
                    .relics
                    .iter_mut()
                    .find(|r| r.id == RelicId::Ancient_Tea_Set)
                {
                    r.counter = -2;
                }
                // EternalFeather.onEnterRoom RestRoom: heal (masterDeck/5)*3.
                if self.player.has_relic(RelicId::Eternal_Feather) {
                    let heal = (self.player.deck.len() as i32 / 5) * 3;
                    self.heal_player(heal);
                }
                self.screen = Screen::Rest;
            }
            RoomType::Treasure => {
                self.generate_chest();
                self.screen = Screen::Treasure;
            }
            RoomType::BossTreasure => self.screen = Screen::Treasure,
            RoomType::Event => match self.roll_event_room(prev_room) {
                Some(converted) => {
                    self.current_room = converted;
                    match converted {
                        RoomType::Monster | RoomType::Elite => self.start_combat_in_current_room(),
                        RoomType::Treasure => {
                            self.generate_chest();
                            self.screen = Screen::Treasure;
                        }
                        RoomType::Shop => self.open_shop(),
                        _ => self.start_event(),
                    }
                }
                None => self.start_event(),
            },
            RoomType::Shop => self.open_shop(),
            _ => self.screen = Screen::Map,
        }
    }

    fn step_combat(&mut self, action: &Action) {
        match action {
            Action::Play {
                hand_index,
                target_index,
            } => {
                if let Some(combat) = self.combat.as_mut() {
                    let select = combat::play_card(
                        &mut self.player,
                        combat,
                        *hand_index,
                        *target_index,
                        &mut self.rng,
                        Some(&self.dungeon),
                    );
                    if combat.force_end_turn && self.player.hp > 0 && !combat.all_dead() {
                        combat.force_end_turn = false;
                        combat::end_turn(
                            &mut self.player,
                            combat,
                            &mut self.rng,
                            Some(&self.dungeon),
                        );
                    }
                    // Player death wins a simultaneous-death race. A card's
                    // queued hits can finish the enemy after reactive damage
                    // has already killed the player (seed 760 Rip and Tear
                    // into Guardian's Sharp Hide).
                    if self.player.hp <= 0 {
                        self.screen = Screen::Terminal;
                        self.done = true;
                    } else if combat.all_dead() {
                        self.finish_combat();
                    } else if select {
                        if combat.need_put_on_deck {
                            self.begin_put_on_deck_select();
                        } else if combat.need_exhaust_select {
                            self.begin_exhaust_select();
                        } else if combat.need_discard_to_hand {
                            self.begin_discard_to_hand_select();
                        } else if combat.need_draw_to_hand {
                            self.begin_draw_to_hand_select();
                        } else if combat.need_discovery {
                            self.begin_discovery(None, false);
                        } else if combat.need_skill_from_deck {
                            self.begin_skill_from_deck_select();
                        } else {
                            self.begin_armaments_select();
                        }
                    }
                }
            }
            Action::EndTurn => {
                if let Some(combat) = self.combat.as_mut() {
                    combat::end_turn(&mut self.player, combat, &mut self.rng, Some(&self.dungeon));
                    if self.player.hp <= 0 {
                        self.screen = Screen::Terminal;
                        self.done = true;
                    } else if combat.all_dead() {
                        self.finish_combat();
                    }
                }
            }
            Action::Potion {
                action,
                slot,
                target_index,
            } => self.use_potion(*action, *slot, *target_index),
            _ => {}
        }
    }

    fn use_potion(&mut self, op: PotionOp, slot: usize, target: Option<usize>) {
        if slot >= self.player.potions.len() {
            return;
        }
        let id = self.player.potions[slot].id;
        if id == PotionId::Slot {
            return;
        }
        if op == PotionOp::Use {
            match id {
                PotionId::Strength => {
                    if self.combat.is_some() {
                        let potency = if self.player.has_relic(RelicId::SacredBark) {
                            4
                        } else {
                            2
                        };
                        self.player
                            .add_power(crate::ids::PowerId::Strength, potency);
                    } else {
                        return;
                    }
                }
                PotionId::Dexterity => {
                    if self.combat.is_some() {
                        let potency = if self.player.has_relic(RelicId::SacredBark) {
                            4
                        } else {
                            2
                        };
                        self.player
                            .add_power(crate::ids::PowerId::Dexterity, potency);
                    } else {
                        return;
                    }
                }
                PotionId::Speed => {
                    // SpeedPotion.use / AbstractPotion.canUse: COMBAT only.
                    // Using it on Rest/Map consumes the slot in rust and
                    // Dexterity is wiped on enter_room (seed 357 Glacier+ 10 vs 15).
                    if self.combat.is_none() {
                        return;
                    }
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        10
                    } else {
                        5
                    };
                    self.player
                        .add_power(crate::ids::PowerId::Dexterity, potency);
                    self.player
                        .add_power(crate::ids::PowerId::LoseDexterity, potency);
                }
                PotionId::Steroid => {
                    // Flex Potion: Strength + LoseStrength at getPotency().
                    if self.combat.is_none() {
                        return;
                    }
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        10
                    } else {
                        5
                    };
                    self.player.add_power(crate::ids::PowerId::Strength, potency);
                    self.player
                        .add_power(crate::ids::PowerId::LoseStrength, potency);
                }
                PotionId::Regen => {
                    // AbstractPotion.getPotency doubles the base 5 with
                    // Sacred Bark. Regen heals at end of turn, then decrements.
                    if self.combat.is_some() {
                        let potency = if self.player.has_relic(RelicId::SacredBark) {
                            10
                        } else {
                            5
                        };
                        self.player.add_power(crate::ids::PowerId::Regen, potency);
                    }
                }
                PotionId::Swift => {
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        6
                    } else {
                        3
                    };
                    let statuses =
                        combat::draw_cards_rng(&mut self.player, potency, Some(&mut self.rng));
                    if let Some(combat) = self.combat.as_mut() {
                        combat::apply_fire_breathing(
                            &mut self.player,
                            &mut combat.monsters,
                            &mut self.rng,
                            statuses,
                        );
                    }
                }
                PotionId::Block => {
                    self.player.block += if self.player.has_relic(RelicId::SacredBark) {
                        24
                    } else {
                        12
                    };
                }
                PotionId::Ancient => {
                    // AncientPotion.use: ArtifactPower(getPotency()=1) in combat only.
                    if self.combat.is_some() {
                        self.player.add_power(crate::ids::PowerId::Artifact, 1);
                    }
                }
                PotionId::Fear => {
                    // FearPotion: VulnerablePower(target, potency,
                    // isSourceMonster=false). Sacred Bark doubles base 3.
                    if let (Some(combat), Some(t)) = (self.combat.as_mut(), target) {
                        if let Some(m) = combat.monsters.get_mut(t) {
                            let potency = if self.player.has_relic(RelicId::SacredBark) {
                                6
                            } else {
                                3
                            };
                            combat::apply_player_power_to_monster(
                                &self.player,
                                m,
                                &mut self.rng,
                                crate::ids::PowerId::Vulnerable,
                                potency,
                            );
                        }
                    }
                }
                PotionId::Weak => {
                    // WeakenPotion: WeakPower(target, 3, isSourceMonster=false).
                    if let (Some(combat), Some(t)) = (self.combat.as_mut(), target) {
                        if let Some(m) = combat.monsters.get_mut(t) {
                            combat::apply_player_power_to_monster(
                                &self.player,
                                m,
                                &mut self.rng,
                                crate::ids::PowerId::Weak,
                                3,
                            );
                        }
                    }
                }
                PotionId::Cultist => {
                    // CultistPotion: RitualPower(player, potency, playerControlled=true).
                    // AbstractPotion.getPotency doubles its base 1 with Sacred
                    // Bark. Player ritual ticks atEndOfTurn with no skipFirst.
                    if self.combat.is_some() {
                        let potency = if self.player.has_relic(RelicId::SacredBark) {
                            2
                        } else {
                            1
                        };
                        self.player.add_power(crate::ids::PowerId::Ritual, potency);
                        if let Some(p) = self
                            .player
                            .powers
                            .iter_mut()
                            .find(|p| p.id == crate::ids::PowerId::Ritual)
                        {
                            p.skip_first = false;
                        }
                    }
                }
                PotionId::Fire => {
                    if let (Some(combat), Some(t)) = (self.combat.as_mut(), target) {
                        let dead_before = combat.monsters.iter().filter(|m| m.dead).count();
                        if let Some(m) = combat.monsters.get_mut(t) {
                            // AbstractPotion.getPotency doubles FirePotion's
                            // base 20 while Sacred Bark is held.
                            let damage = if self.player.has_relic(RelicId::SacredBark) {
                                40
                            } else {
                                20
                            };
                            let block_before = m.block;
                            combat::deal_thorns(m, &mut self.rng, damage);
                            // AbstractCreature.decrementBlock calls brokeBlock
                            // for THORNS damage too. HandDrill.onBlockBroken
                            // queues Vulnerable behind FirePotion's DamageAction,
                            // before the next player command (seed 477).
                            if block_before > 0
                                && m.block == 0
                                && self.player.has_relic(RelicId::HandDrill)
                            {
                                combat::apply_player_power_to_monster(
                                    &self.player,
                                    m,
                                    &mut self.rng,
                                    crate::ids::PowerId::Vulnerable,
                                    2,
                                );
                            }
                        }
                        // FirePotion DamageAction can kill; GremlinHorn.onMonsterDeath
                        // addToBot Draw+Energy if combat is not over (seed 773).
                        combat::gremlin_horn_on_kills(
                            &mut self.player,
                            combat,
                            &mut self.rng,
                            dead_before,
                        );
                    }
                }
                PotionId::Explosive => {
                    // ExplosivePotion.use: DamageAllEnemiesAction using getPotency().
                    let damage = if self.player.has_relic(RelicId::SacredBark) {
                        20
                    } else {
                        10
                    };
                    if let Some(combat) = self.combat.as_mut() {
                        let dead_before = combat.monsters.iter().filter(|m| m.dead).count();
                        for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
                            combat::deal_thorns(m, &mut self.rng, damage);
                        }
                        combat::gremlin_horn_on_kills(
                            &mut self.player,
                            combat,
                            &mut self.rng,
                            dead_before,
                        );
                    }
                }
                PotionId::LiquidBronze => {
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        6
                    } else {
                        3
                    };
                    self.player.add_power(crate::ids::PowerId::Thorns, potency);
                }
                PotionId::Duplication => {
                    self.player.duplication += 1;
                }
                PotionId::Energy => {
                    // AbstractPotion.getPotency doubles Energy Potion's base 2.
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        4
                    } else {
                        2
                    };
                    self.player.energy += potency;
                }
                PotionId::Blood => {
                    let heal = (self.player.max_hp as f32 * 0.2).floor() as i32;
                    self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                    crate::combat::red_skull_on_hp_change(&mut self.player);
                }
                PotionId::FruitJuice => {
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        10
                    } else {
                        5
                    };
                    self.player.max_hp += potency;
                    self.player.hp += potency;
                }
                PotionId::EssenceOfSteel => {
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        8
                    } else {
                        4
                    };
                    self.player.add_power(crate::ids::PowerId::PlatedArmor, potency);
                }
                PotionId::BlessingOfTheForge => {
                    // BlessingOfTheForge.use -> ArmamentsAction(true): upgrade every
                    // upgradeable card in hand.
                    if self.combat.is_some() {
                        for c in self.player.hand.iter_mut() {
                            if c.can_upgrade() {
                                c.upgrade();
                            }
                        }
                    }
                }
                PotionId::GamblersBrew => {
                    // GamblingChipAction(player, true): discard any number, draw that many.
                    if !self.player.hand.is_empty() {
                        self.open_gambling_select();
                    }
                }
                PotionId::Focus => {
                    // FocusPotion.use: ApplyPower only if room.phase==COMBAT.
                    if self.combat.is_none() {
                        return;
                    }
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        4
                    } else {
                        2
                    };
                    self.player.add_power(crate::ids::PowerId::Focus, potency);
                }
                PotionId::PotionOfCapacity => {
                    // PotionOfCapacity.use -> IncreaseMaxOrbAction(potency).
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        4
                    } else {
                        2
                    };
                    combat::increase_max_orb_slots(&mut self.player, potency);
                }
                PotionId::EssenceOfDarkness => {
                    // EssenceOfDarknessAction: channel Dark once per orb slot
                    // (Java iterates player.orbs including EmptyOrbSlot).
                    if let Some(combat) = self.combat.as_mut() {
                        let n = self.player.max_orbs;
                        for _ in 0..n {
                            combat::channel_orb(
                                &mut self.player,
                                combat,
                                &mut self.rng,
                                crate::creature::OrbKind::Dark,
                            );
                        }
                    }
                }
                PotionId::DistilledChaos => {
                    // DistilledChaosPotion: getPotency() PlayTopCardActions.
                    // AbstractPotion doubles the base 3 with Sacred Bark.
                    let potency = if self.player.has_relic(RelicId::SacredBark) {
                        6
                    } else {
                        3
                    };
                    // Targets are
                    // rolled up front via cardRandomRng.getRandomMonster.
                    // The PlayTopCardActions drain before UseCardAction
                    // discards, so a mid-batch empty-deck shuffle must not
                    // include the in-flight cards (seed 38 Dualcast).
                    if let Some(combat) = self.combat.as_mut() {
                        let mut targets = Vec::new();
                        for _ in 0..potency {
                            targets.push(combat::random_alive_monster(
                                combat,
                                &mut self.rng.card_random,
                            ));
                        }
                        combat::play_top_cards(
                            &mut self.player,
                            combat,
                            &targets,
                            false,
                            &mut self.rng,
                            Some(&self.dungeon),
                        );
                    }
                }
                PotionId::SneckoOil => {
                    // SneckoOil: DrawCardAction(5) then RandomizeHandCostAction.
                    // cardRandomRng.random(3) for every card with cost >= 0.
                    if self.combat.is_some() {
                        let statuses =
                            combat::draw_cards_rng(&mut self.player, 5, Some(&mut self.rng));
                        if let Some(combat) = self.combat.as_mut() {
                            combat::apply_fire_breathing(
                                &mut self.player,
                                &mut combat.monsters,
                                &mut self.rng,
                                statuses,
                            );
                        }
                        for c in self.player.hand.iter_mut() {
                            if c.cost >= 0 {
                                let new_cost = self.rng.card_random.random_int(3) as i16;
                                if c.cost != new_cost {
                                    c.cost = new_cost;
                                    c.cost_for_turn = new_cost;
                                }
                            }
                        }
                    }
                }
                PotionId::LiquidMemories => {
                    self.begin_memories_select();
                }
                PotionId::Attack => {
                    self.begin_potion_discovery(Some(crate::ids::CardType::ATTACK), false);
                    self.player.potions[slot] = PotionInstance {
                        id: PotionId::Slot,
                        slot: slot as i32,
                    };
                    self.ornithopter_after_potion(true);
                    return;
                }
                PotionId::Skill => {
                    self.begin_potion_discovery(Some(crate::ids::CardType::SKILL), false);
                    self.player.potions[slot] = PotionInstance {
                        id: PotionId::Slot,
                        slot: slot as i32,
                    };
                    self.ornithopter_after_potion(true);
                    return;
                }
                PotionId::Power => {
                    self.begin_potion_discovery(Some(crate::ids::CardType::POWER), false);
                    self.player.potions[slot] = PotionInstance {
                        id: PotionId::Slot,
                        slot: slot as i32,
                    };
                    self.ornithopter_after_potion(true);
                    return;
                }
                PotionId::Colorless => {
                    self.begin_potion_discovery(None, true);
                    self.player.potions[slot] = PotionInstance {
                        id: PotionId::Slot,
                        slot: slot as i32,
                    };
                    self.ornithopter_after_potion(true);
                    return;
                }
                PotionId::EntropicBrew => {
                    // Combat: ObtainPotionAction(returnRandomPotion(true)).
                    // Out of combat: ObtainPotionEffect(returnRandomPotion()) —
                    // limited=false, so the first rarity-matching pick is kept.
                    // 861954 used the brew after Wheel of Change; limited=true
                    // burned extra potionRng and the next hallway dropped Regen.
                    let limited = self.combat.is_some();
                    self.player.potions[slot] = PotionInstance {
                        id: PotionId::Slot,
                        slot: slot as i32,
                    };
                    for _ in 0..self.player.potion_slots {
                        let p = crate::rewards::return_random_potion(
                            &mut self.rng,
                            self.character,
                            limited,
                        );
                        let _ = self.gain_potion(p);
                    }
                    self.on_use_potion_relics();
                    if let Some(combat) = &self.combat {
                        if combat.all_dead() {
                            self.finish_combat();
                        }
                    }
                    return;
                }
                _ => {}
            }
            self.on_use_potion_relics();
            if let Some(combat) = self.combat.as_mut() {
                // Fire/Explosive DamageAction can trip Mode Shift; GainBlock 20
                // is addToBottom and must resolve before the next command.
                combat::flush_guardian_defensive_block(combat);
            }
        }
        self.player.potions[slot] = PotionInstance {
            id: PotionId::Slot,
            slot: slot as i32,
        };
        let (all_dead, disc_to_hand, draw_to_hand, discovery, put_on_deck, exhaust_sel, skill_deck) =
            if let Some(c) = self.combat.as_ref() {
                (
                    c.all_dead(),
                    c.need_discard_to_hand,
                    c.need_draw_to_hand,
                    c.need_discovery,
                    c.need_put_on_deck,
                    c.need_exhaust_select,
                    c.need_skill_from_deck,
                )
            } else {
                (false, false, false, false, false, false, false)
            };
        if all_dead {
            self.finish_combat();
        } else if disc_to_hand {
            // Distilled Chaos autoplay Hologram: GRID after the 3 PlayTopCards
            // (seed 610 Cold Snap from discard).
            self.begin_discard_to_hand_select();
        } else if draw_to_hand {
            self.begin_draw_to_hand_select();
        } else if discovery {
            self.begin_discovery(None, false);
        } else if put_on_deck {
            self.begin_put_on_deck_select();
        } else if exhaust_sel {
            self.begin_exhaust_select();
        } else if skill_deck {
            self.begin_skill_from_deck_select();
        }
    }

    fn finish_combat(&mut self) {
        // Looter.die / Mugger.die call addStolenGoldToRewards; EscapeAction
        // only sets room.mugged and keeps the gold.
        let stolen: i32 = self
            .combat
            .as_ref()
            .map(|c| {
                c.monsters
                    .iter()
                    .filter(|m| !m.escaped)
                    .map(|m| m.stolen_gold)
                    .sum()
            })
            .unwrap_or(0);
        if self
            .combat
            .as_ref()
            .is_some_and(|combat| combat.slavers_collar_active)
        {
            self.player.energy_master -= 1;
        }
        after_combat_relics(&mut self.player);
        let colosseum_first_fight = self.current_room == RoomType::Event
            && self
                .event
                .as_ref()
                .is_some_and(|event| event.id == "Colosseum" && event.screen == 2);
        if colosseum_first_fight {
            // Colosseum sets rewardAllowed=false for the Slavers. reopen()
            // returns to POST_COMBAT instead of opening CombatReward. The
            // room still calls addPotionToRewards before checking that flag,
            // so preserve its roll (and blizzard update) then discard it.
            let _ = crate::rewards::roll_potion(
                &mut self.rng,
                &mut self.potion_blizzard,
                false,
                false,
                false,
                self.character,
                self.rewards.len(),
                self.player.has_relic(RelicId::White_Beast_Statue),
            );
            // Colosseum.reopen calls resetPlayer() and preBattlePrep() even
            // before the player chooses whether to take the second fight.
            self.player.block = 0;
            self.player.powers.clear();
            self.player.orbs.clear();
            self.player.max_orbs = self.player.master_max_orbs;
            self.player.draw = self.player.deck.clone();
            self.player.hand.clear();
            self.player.discard.clear();
            self.player.exhaust.clear();
            let shuffle_seed = self.rng.shuffle.random_long();
            shuffle_java(&mut self.player.draw, shuffle_seed);
            self.player.energy = self.player.energy_master;
            self.rewards.clear();
            self.combat = None;
            if let Some(event) = self.event.as_mut() {
                event.options = vec!["[Flee]".into(), "[Fight]".into()];
            }
            self.screen = Screen::Event;
            return;
        }
        let event_room = self.current_room == RoomType::Event;
        // MonsterGroup.haveMonstersEscaped: true only if every monster
        // escaped. Hallway gold and potion then skip (Looter/Mugger run).
        let all_escaped = self.combat.as_ref().is_some_and(|c| {
            !c.monsters.is_empty() && c.monsters.iter().all(|m| m.escaped)
        });
        // AbstractRoom.endBattle gold is `instanceof MonsterRoomBoss/Elite/MonsterRoom`.
        // EventRoom keeps pre-seeded rewards (Mushrooms gold+Odd Mushroom, MindBloom, …)
        // and does not roll hallway gold.
        if !event_room {
            self.rewards.clear();
            if stolen > 0 {
                self.add_stolen_gold_to_rewards(stolen);
            }
            let boss = self.current_room == RoomType::Boss;
            let elite = self.current_room == RoomType::Elite;
            if boss || elite || !all_escaped {
                let gold = crate::rewards::roll_monster_gold(&mut self.rng, boss, elite, self.ascension);
                self.add_gold_to_rewards(gold);
            }
            if elite {
                // MonsterRoomElite.returnRandomRelicTier: <50 common, >82 rare, else uncommon.
                let roll = self.rng.relic.random_range(0, 99);
                let tier = if roll < 50 {
                    RelicTier::COMMON
                } else if roll > 82 {
                    RelicTier::RARE
                } else {
                    RelicTier::UNCOMMON
                };
                if let Some(id) = self.take_relic(tier) {
                    self.add_relic_to_rewards(id);
                }
                if self.player.has_relic(RelicId::Black_Star) {
                    // Black Star: a second independent tier roll, excluding
                    // the three campfire relics from the consumed pool.
                    let roll = self.rng.relic.random_range(0, 99);
                    let tier = if roll < 50 {
                        RelicTier::COMMON
                    } else if roll > 82 {
                        RelicTier::RARE
                    } else {
                        RelicTier::UNCOMMON
                    };
                    if let Some(id) = self.take_noncamp_relic(tier) {
                        self.add_relic_to_rewards(id);
                    }
                }
                // MonsterRoomElite.addEmeraldKey: after relic(s), before potion/CARD.
                self.add_emerald_key_reward();
            }
        } else if stolen > 0 {
            self.add_stolen_gold_to_rewards(stolen);
        }
        let boss = self.current_room == RoomType::Boss;
        let elite = self.current_room == RoomType::Elite;
        // AbstractRoom.endBattle: skip addPotionToRewards only for
        // MonsterRoomBoss in TheBeyond / TheEnding (unless endless).
        // MonsterRoomBoss instanceof MonsterRoom, so Act 1/2 bosses still
        // use chance 40+blizzard. EventRoom uses the same 40+blizzard roll.
        let skip_combat_rewards = !event_room
            && boss
            && matches!(self.dungeon.act, crate::ids::Act::Beyond | crate::ids::Act::Ending);
        // Hallway + all escaped: addPotionToRewards still rolls with chance 0.
        let escaped_hallway = !event_room && !boss && !elite && all_escaped;
        if let Some(p) = crate::rewards::roll_potion(
            &mut self.rng,
            &mut self.potion_blizzard,
            elite,
            skip_combat_rewards,
            escaped_hallway,
            self.character,
            self.rewards.len(),
            self.player.has_relic(RelicId::White_Beast_Statue),
        ) {
            self.rewards.push(Reward::new(RewardKind::Potion(p)));
        }
        // AbstractRoom.update skips opening CombatRewardScreen entirely for
        // final bosses in TheBeyond/TheEnding. Since setupItemReward is what
        // creates the card reward, these fights must not roll reward cards.
        if skip_combat_rewards {
            self.card_reward.clear();
        } else {
            self.rewards.push(Reward {
                kind: RewardKind::Card,
                taken: false,
                relic_link: None,
                card_options: None,
            });
            self.generate_card_reward();
        }
        self.combat = None;
        self.screen = Screen::CombatReward;
    }

    /// AbstractRoom.addGoldToRewards: merge into an existing GOLD item.
    fn add_gold_to_rewards(&mut self, gold: i32) {
        if let Some(existing) = self.rewards.iter_mut().find_map(|r| match &mut r.kind {
            RewardKind::Gold(g) => Some(g),
            _ => None,
        }) {
            *existing += gold;
            return;
        }
        self.rewards.push(Reward {
            kind: RewardKind::Gold(gold),
            taken: false,
            relic_link: None,
            card_options: None,
        });
    }

    /// AbstractRoom.addStolenGoldToRewards: merge into an existing STOLEN_GOLD item.
    fn add_stolen_gold_to_rewards(&mut self, gold: i32) {
        if let Some(existing) = self.rewards.iter_mut().find_map(|r| match &mut r.kind {
            RewardKind::StolenGold(g) => Some(g),
            _ => None,
        }) {
            *existing += gold;
            return;
        }
        self.rewards.push(Reward {
            kind: RewardKind::StolenGold(gold),
            taken: false,
            relic_link: None,
            card_options: None,
        });
    }

    fn add_relic_to_rewards(&mut self, id: RelicId) {
        self.rewards.push(Reward::new(RewardKind::Relic(id)));
    }

    /// AbstractChest.open relic loop: Cursed Key then Matryoshka extra relic.
    fn on_chest_open(&mut self, boss_chest: bool) {
        let n = self.player.relics.len();
        for i in 0..n {
            match self.player.relics[i].id {
                RelicId::Cursed_Key if !boss_chest => {
                    // CursedKey.onChestOpen: ShowCardAndObtainEffect(returnRandomCurse()).
                    // ExactTextSim applies the obtain before the COMBAT_REWARD snapshot
                    // (seed 8 chest on floor 24 already has Clumsy in the master deck).
                    let id = self.return_random_curse();
                    self.obtain_master_deck_card(id);
                }
                RelicId::Matryoshka if !boss_chest => {
                    if self.player.relics[i].counter > 0 {
                        self.player.relics[i].counter -= 1;
                        if self.player.relics[i].counter == 0 {
                            self.player.relics[i].counter = -2;
                            self.player.relics[i].used_up = true;
                        }
                        let tier = if self.rng.relic.random_boolean_chance(0.75) {
                            RelicTier::COMMON
                        } else {
                            RelicTier::UNCOMMON
                        };
                        if let Some(id) = self.take_relic(tier) {
                            self.add_relic_to_rewards(id);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// NlothsMask.onChestOpenAfter: drop the first relic (and its sapphire link).
    fn on_chest_open_after(&mut self, boss_chest: bool) {
        if boss_chest {
            return;
        }
        let n = self.player.relics.len();
        for i in 0..n {
            if self.player.relics[i].id != RelicId::NlothsMask {
                continue;
            }
            if self.player.relics[i].counter > 0 {
                self.player.relics[i].counter -= 1;
                if self.player.relics[i].counter == 0 {
                    self.player.relics[i].counter = -2;
                    self.player.relics[i].used_up = true;
                }
                self.remove_one_relic_from_rewards();
            }
        }
    }

    fn remove_reward_at(&mut self, i: usize) {
        self.rewards.remove(i);
        for r in &mut self.rewards {
            match r.relic_link {
                Some(l) if l == i => r.relic_link = None,
                Some(l) if l > i => r.relic_link = Some(l - 1),
                _ => {}
            }
        }
    }

    fn remove_one_relic_from_rewards(&mut self) {
        let Some(i) = self
            .rewards
            .iter()
            .position(|r| matches!(r.kind, RewardKind::Relic(_)))
        else {
            return;
        };
        let remove_next = self.rewards[i].relic_link == Some(i + 1);
        self.remove_reward_at(i);
        if remove_next && i < self.rewards.len() {
            self.remove_reward_at(i);
        }
    }

    fn claim_reward_at(&mut self, real: usize) {
        let Some(kind) = self.rewards.get(real).filter(|r| !r.taken).map(|r| r.kind.clone()) else {
            return;
        };
        match kind {
            RewardKind::Gold(g) => {
                let gained = self.gold_gain(g);
                self.player.gold += gained;
            }
            RewardKind::StolenGold(g) => {
                if !self.player.has_relic(RelicId::Ectoplasm) {
                    self.player.gold += g;
                }
            }
            RewardKind::Potion(p) => {
                // RewardItem.claimReward returns true under Sozu so the reward
                // is consumed, but it does not call player.obtainPotion.
                if !self.player.has_relic(RelicId::Sozu) && !self.gain_potion(p) {
                    return;
                }
            }
            RewardKind::Relic(id) => self.gain_relic(id),
            RewardKind::EmeraldKey => self.has_emerald_key = true,
            RewardKind::SapphireKey => self.has_sapphire_key = true,
            RewardKind::Card => {
                self.open_reward_card_at(real);
                return;
            }
        }
        let link = self.rewards.get(real).and_then(|r| r.relic_link);
        if let Some(reward) = self.rewards.get_mut(real) {
            reward.taken = true;
        }
        // RewardItem.claimReward: taking a linked relic or sapphire key
        // sets the other isDone + ignoreReward (no obtain).
        if let Some(link) = link {
            if let Some(r) = self.rewards.get_mut(link) {
                r.taken = true;
            }
        }
    }

    fn random_potion(&mut self) -> Option<PotionId> {
        const COMMON: &[PotionId] = &[
            PotionId::Block,
            PotionId::Strength,
            PotionId::Dexterity,
            PotionId::Fire,
            PotionId::Explosive,
            PotionId::Weak,
            PotionId::Fear,
            PotionId::Attack,
            PotionId::Energy,
            PotionId::Swift,
            PotionId::Colorless,
            PotionId::BlessingOfTheForge,
        ];
        Some(COMMON[self.rng.potion.random_range(0, COMMON.len() as i32 - 1) as usize])
    }

    fn step_reward(&mut self, action: &Action) {
        match action {
            Action::Proceed => {
                self.flush_pending_cards();
                if self.current_room == RoomType::Boss {
                    self.reset_player_between_rooms();
                    self.dungeon.floor += 1;
                    self.rng.reset_floor_streams(self.seed, self.dungeon.floor);
                    if self.dungeon.act == crate::ids::Act::Beyond {
                        self.current_room = RoomType::Victory;
                        self.event = Some(EventState {
                            id: "SpireHeart".into(),
                            screen: 0,
                            options: vec!["[Continue]".into()],
                            data: Vec::new(),
                            library_cards: Vec::new(),
                            match_cards: Vec::new(),
                            match_chosen: None,
                            match_attempts: 0,
                        });
                        self.screen = Screen::Event;
                    } else {
                        self.screen = Screen::Treasure;
                        self.current_room = RoomType::BossTreasure;
                    }
                    // Both destination rooms are real room entries (seed 683
                    // TreasureRoomBoss MawBank +12; Beyond's VictoryRoom too).
                    self.maw_bank_on_enter_room();
                } else if self.current_room == RoomType::BossTreasure {
                    // TinyHouse.onEquip CombatRewardScreen overlay: Proceed
                    // continues the act transition that BossRelic Skip would.
                    self.begin_next_act();
                } else if self.event.as_ref().is_some_and(|e| {
                    (e.id == "SensoryStone" && e.screen == 2)
                        || (e.id == "Wheel of Change" && e.screen == 3)
                        || e.id == "Woman in Blue"
                        || e.id == "The Woman in Blue"
                }) {
                    self.screen = Screen::Event;
                } else {
                    self.open_map();
                }
            }
            Action::Choose { index, label, .. } => {
                if let Some(label) = label {
                    if label == "EMERALD_KEY" {
                        self.claim_emerald_key();
                        return;
                    }
                    if label == "RUBY_KEY" {
                        return;
                    }
                    if label == "STOLEN_GOLD" {
                        let gold = self.rewards.iter().find_map(|r| match r.kind {
                            RewardKind::StolenGold(g) if !r.taken => Some(g),
                            _ => None,
                        });
                        if let Some(g) = gold {
                            // RewardItem.STOLEN_GOLD calls player.gainGold;
                            // Ectoplasm consumes the reward but blocks the gain.
                            if !self.player.has_relic(RelicId::Ectoplasm) {
                                self.player.gold += g;
                            }
                            if let Some(r) = self.rewards.iter_mut().find(|r| matches!(r.kind, RewardKind::StolenGold(_)) && !r.taken) {
                                r.taken = true;
                            }
                        }
                        return;
                    }
                    if label == "GOLD" {
                        let gold = self.rewards.iter().find_map(|r| match r.kind {
                            RewardKind::Gold(g) if !r.taken => Some(g),
                            _ => None,
                        });
                        if let Some(g) = gold {
                            let gained = self.gold_gain(g);
                            self.player.gold += gained;
                            if let Some(r) = self.rewards.iter_mut().find(|r| matches!(r.kind, RewardKind::Gold(_)) && !r.taken) {
                                r.taken = true;
                            }
                        }
                        return;
                    }
                    if label == "POTION" {
                        let potion = self.rewards.iter().find_map(|r| match r.kind {
                            RewardKind::Potion(p) if !r.taken => Some(p),
                            _ => None,
                        });
                        if let Some(p) = potion {
                            if self.gain_potion(p) {
                                if let Some(r) = self.rewards.iter_mut().find(|r| matches!(r.kind, RewardKind::Potion(x) if x == p && !r.taken)) {
                                    r.taken = true;
                                }
                            }
                        }
                        return;
                    }
                    if label == "CARD" {
                        if let Some(real) = self
                            .rewards
                            .iter()
                            .position(|r| matches!(r.kind, RewardKind::Card) && !r.taken)
                        {
                            self.open_reward_card_at(real);
                        }
                        return;
                    }
                }
                let untaken: Vec<usize> = self
                    .rewards
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| !r.taken)
                    .map(|(i, _)| i)
                    .collect();
                if let Some(&real) = untaken.get(*index) {
                    self.claim_reward_at(real);
                }
            }
            _ => {}
        }
    }

    fn begin_discovery(&mut self, typ: Option<crate::ids::CardType>, colorless: bool) {
        self.card_reward = crate::rewards::discovery_cards(&self.dungeon, &mut self.rng, typ, colorless);
        self.discovery_combat = self.combat.is_some();
        self.discovery_typ = typ;
        self.discovery_colorless = colorless;
        self.discovery_copies = 1;
        self.screen = Screen::CardReward;
    }

    fn begin_potion_discovery(&mut self, typ: Option<crate::ids::CardType>, colorless: bool) {
        self.begin_discovery(typ, colorless);
        if self.player.has_relic(RelicId::SacredBark) {
            self.discovery_copies = 2;
        }
    }

    fn begin_toolbox_reward(&mut self) {
        self.card_reward = crate::rewards::discovery_cards(&self.dungeon, &mut self.rng, None, true);
        self.toolbox_reward = true;
        self.screen = Screen::CardReward;
    }

    fn generate_card_reward(&mut self) {
        let boss = self.current_room == RoomType::Boss;
        let elite = self.current_room == RoomType::Elite;
        let shop = self.current_room == RoomType::Shop;
        // TheCity: A12+ 0.125 else 0.25. TheBeyond/TheEnding: A12+ 0.25 else 0.5.
        let upgrade_chance = match self.dungeon.act {
            crate::ids::Act::Exordium => 0.0,
            crate::ids::Act::City => {
                if self.ascension >= 12 {
                    0.125
                } else {
                    0.25
                }
            }
            crate::ids::Act::Beyond | crate::ids::Act::Ending => {
                if self.ascension >= 12 {
                    0.25
                } else {
                    0.5
                }
            }
        };
        // AbstractDungeon.getRewardCards: relic.changeNumberOfCardsInReward.
        let mut n = 3i32;
        for r in &self.player.relics {
            n = match r.id {
                RelicId::Question_Card => n + 1,
                RelicId::Busted_Crown => n - 2,
                _ => n,
            };
        }
        self.card_reward = crate::rewards::reward_cards(
            &self.dungeon,
            &mut self.rng,
            &mut self.card_blizz,
            n.max(0) as usize,
            boss,
            elite,
            shop,
            upgrade_chance,
            &self.player,
        );
    }

    fn open_card_reward(&mut self) {
        if self.card_reward.is_empty() {
            self.generate_card_reward();
        }
        self.screen = Screen::CardReward;
    }

    fn open_reward_card_at(&mut self, real: usize) {
        if let Some(cards) = self
            .rewards
            .get(real)
            .and_then(|reward| reward.card_options.clone())
        {
            self.card_reward = cards;
        }
        self.active_card_reward = Some(real);
        self.open_card_reward();
    }

    /// CampfireSleepEffect: after Rest heal, Dream Catcher rolls `getRewardCards`
    /// and opens CardRewardScreen with a null RewardItem.
    fn open_dream_catcher_reward(&mut self) {
        if !self.player.has_relic(RelicId::Dream_Catcher) {
            return;
        }
        self.generate_card_reward();
        if !self.card_reward.is_empty() {
            self.screen = Screen::CardReward;
        }
    }

    fn pick_card(&mut self, rarity: CardRarity) -> Option<CardId> {
        let pool = match rarity {
            CardRarity::RARE => &self.dungeon.rare_cards,
            CardRarity::UNCOMMON => &self.dungeon.uncommon_cards,
            _ => &self.dungeon.common_cards,
        };
        if pool.is_empty() {
            return None;
        }
        Some(pool[self.rng.card.random_range(0, pool.len() as i32 - 1) as usize])
    }

    fn step_card_reward(&mut self, action: &Action) {
        match action {
            Action::Skip => {
                self.finish_card_reward();
            }
            Action::Choose { index, label, .. } => {
                let card = label
                    .as_ref()
                    .and_then(|name| {
                        let upgraded = name.ends_with('+');
                        let base = name.trim_end_matches('+');
                        self.card_reward
                            .iter()
                            .find(|c| c.sts_id() == base || c.sts_id().replace('_', " ") == base)
                            .cloned()
                            .or_else(|| crate::ids::CardId::from_sts_id(base).map(crate::card::Card::new))
                            .map(|mut c| {
                                if upgraded {
                                    c.upgrade();
                                }
                                c
                            })
                    })
                    .or_else(|| self.card_reward.get(*index).cloned());
                if let Some(mut card) = card {
                    if self.toolbox_reward {
                        if self.player.hand.len() < 10 {
                            self.player.hand.push(card);
                        } else {
                            self.player.discard.push(card);
                        }
                    } else if self.discovery_combat {
                        card.cost_for_turn = 0;
                        for _ in 0..self.discovery_copies.max(1) {
                            if self.player.hand.len() < 10 {
                                self.player.hand.push(card.clone());
                            } else {
                                self.player.discard.push(card.clone());
                            }
                        }
                        crate::rewards::burn_discovery_rng(
                            &self.dungeon,
                            &mut self.rng,
                            self.discovery_typ,
                            self.discovery_colorless,
                            15,
                        );
                    } else {
                        crate::rewards::preview_obtain(&self.player, &mut card);
                        self.pending_cards.push(card);
                    }
                }
                self.finish_card_reward();
            }
            _ => {}
        }
    }

    fn finish_card_reward(&mut self) {
        if self.toolbox_reward {
            self.toolbox_reward = false;
            self.card_reward.clear();
            self.screen = Screen::Combat;
            crate::combat::draw_opening_hand(&mut self.player, &mut self.rng);
            self.begin_gambling_chip();
            return;
        }
        if self.discovery_combat {
            self.discovery_combat = false;
            self.discovery_copies = 1;
            self.card_reward.clear();
            self.screen = Screen::Combat;
            self.apply_pending_ornithopter_heal();
            return;
        }
        if let Some(real) = self.active_card_reward.take() {
            if let Some(reward) = self.rewards.get_mut(real) {
                reward.taken = true;
            }
        }
        // FastCardObtainEffect lands before the next stable boundary.
        self.flush_pending_cards();
        self.card_reward.clear();
        if self.current_room == RoomType::Neow {
            self.present_neow_leave();
            return;
        }
        // CampfireSleepEffect opens CardRewardScreen with rItem=null; close
        // returns to RestRoom COMPLETE, not CombatReward.
        if self.current_room == RoomType::Rest {
            self.card_reward.clear();
            self.screen = Screen::Rest;
            return;
        }
        self.screen = Screen::CombatReward;
    }

    fn campfire_options(&self) -> Vec<&'static str> {
        let mut opts = Vec::new();
        if !self.player.has_relic(RelicId::Coffee_Dripper) {
            opts.push("Rest");
        }
        if self.player.deck.iter().any(|c| c.can_upgrade()) {
            opts.push("Smith");
        }
        if self.final_act_available && !self.has_ruby_key {
            opts.push("Recall");
        }
        opts
    }

    fn step_rest(&mut self, action: &Action) {
        match action {
            Action::Proceed | Action::Skip => {
                if self.rest_smithing && self.rest_smith_picked {
                    // GRID confirm after a smith pick; campfire stays open.
                    self.rest_smithing = false;
                    self.rest_smith_picked = false;
                    return;
                }
                self.rest_smithing = false;
                self.rest_smith_picked = false;
                self.open_map();
            }
            Action::Choose { index, label, .. } => {
                if matches!(label.as_deref(), Some("open") | Some("boss") | Some("map node")) {
                    return;
                }
                if self.rest_smithing {
                    if let Some(name) = label.as_deref() {
                        let base = name.trim_end_matches('+');
                        if let Some(card) = self.player.deck.iter_mut().find(|c| {
                            let id = c.sts_id();
                            c.can_upgrade()
                                && (id == base
                                    || id.replace('_', " ") == base
                                    || c.def().sts_id == base)
                        }) {
                            card.upgrade();
                        }
                    } else {
                        let upg: Vec<usize> = self
                            .player
                            .deck
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.can_upgrade())
                            .map(|(i, _)| i)
                            .collect();
                        if let Some(&i) = upg.get(*index) {
                            self.player.deck[i].upgrade();
                        }
                    }
                    self.rest_smith_picked = true;
                    return;
                }
                let kind = match label.as_deref() {
                    Some("Rest") => Some("Rest"),
                    Some(s) if s.contains("Sleep") => Some("Rest"),
                    Some("Smith") => Some("Smith"),
                    Some("Recall") => Some("Recall"),
                    _ => self.campfire_options().get(*index).copied(),
                };
                if self.rest_selected && kind != Some("Rest") {
                    return;
                }
                match kind {
                    Some("Rest") => {
                        let mut heal = (self.player.max_hp as f32 * 0.3).floor() as i32;
                        if self.player.has_relic(RelicId::Regal_Pillow) {
                            heal += 15;
                        }
                        if self.rest_selected {
                            // ChoiceDriver can queue a second CampfireSleepEffect.
                            // Heal lands after MAP is published, on the next enter_room.
                            self.pending_rest_heal += heal;
                        } else {
                            self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                            self.rest_selected = true;
                            self.open_dream_catcher_reward();
                        }
                    }
                    Some("Smith") => {
                        self.rest_selected = true;
                        self.rest_smithing = true;
                        self.rest_smith_picked = false;
                        self.rest_selected = true;
                    }
                    Some("Recall") => {
                        self.has_ruby_key = true;
                        self.rest_selected = true;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn open_shop(&mut self) {
        // MealTicket.justEnteredRoom: heal 15 when the room is a ShopRoom.
        if self.player.has_relic(RelicId::MealTicket) {
            self.heal_player(15);
        }
        let stock = crate::rewards::generate_shop(
            &mut self.dungeon,
            &mut self.rng,
            &self.player,
            self.card_blizz,
            self.ascension,
            self.character,
            self.current_room,
        );
        // ShopScreen.purgeCost is run-wide (resetPurgeCost on a new run).
        // Rebuilding the shop from stock was resetting it to 75, so a second
        // shop still treated purge as 75g and shifted choose indices
        // (144185: index 4 bought Darkness instead of Defragment).
        let purge_cost = if self.shop.purge_cost > 0 {
            self.shop.purge_cost
        } else {
            75
        };
        self.shop = ShopState {
            open: false,
            cards: stock.cards,
            relics: stock.relics,
            potions: stock.potions,
            purge_cost,
            purge_available: true,
        };
        self.screen = Screen::Shop;
    }

    fn apply_pending_shop_purge(&mut self) {
        let Some(i) = self.pending_shop_purge.take() else {
            return;
        };
        if self.player.gold < self.shop.purge_cost || !self.shop.purge_available {
            return;
        }
        self.spend_shop_gold(self.shop.purge_cost);
        self.shop.purge_available = false;
        self.shop.purge_cost += 25;
        if i < self.player.deck.len() {
            self.remove_master_deck_card(i);
        }
    }

    /// CardGroup.removeCard(card) on the master deck invokes the removed
    /// card's onRemoveFromMasterDeck hook before relic deck-change hooks.
    fn remove_master_deck_card(&mut self, index: usize) -> Option<Card> {
        if index >= self.player.deck.len() {
            return None;
        }
        let card = self.player.deck.remove(index);
        if card.id == CardId::Parasite {
            self.player.max_hp = (self.player.max_hp - 3).max(1);
            self.player.hp = self.player.hp.min(self.player.max_hp);
        }
        Some(card)
    }

    fn spend_shop_gold(&mut self, amount: i32) {
        self.player.gold -= amount;
        // ShopScreen: every relic.onSpendGold after loseGold.
        if let Some(r) = self.player.relics.iter_mut().find(|r| r.id == RelicId::MawBank) {
            if !r.used_up {
                r.used_up = true;
                r.counter = -2;
            }
        }
    }

    fn maw_bank_on_enter_room(&mut self) {
        // MawBank.onEnterRoom: +12 gold until the relic is used up by spending.
        let active = self
            .player
            .relics
            .iter()
            .any(|r| r.id == RelicId::MawBank && !r.used_up);
        if active && !self.player.has_relic(RelicId::Ectoplasm) {
            self.player.gold += 12;
        }
    }

    fn step_shop(&mut self, action: &Action) {
        if !self.shop.open {
            match action {
                Action::Proceed | Action::Skip => self.open_map(),
                Action::Choose { label, .. }
                    if label.as_deref() == Some("shop") || label.is_none() =>
                {
                    self.shop.open = true;
                }
                _ => {}
            }
            return;
        }
        match action {
            Action::Skip | Action::Proceed => {
                self.apply_pending_shop_purge();
                self.shop.open = false;
            }
            Action::Choose { index, label, .. } => {
                if !self.buy_shop_item(label.as_deref(), *index) {
                    self.shop.open = false;
                }
            }
            _ => {}
        }
    }

    fn buy_shop_item(&mut self, label: Option<&str>, index: usize) -> bool {
        if matches!(label, Some("purge")) {
            if self.player.gold >= self.shop.purge_cost && self.shop.purge_available {
                self.grid = Some(GridSelect {
                    kind: GridKind::Purge,
                    needed: 1,
                    confirm: false,
                    hovered: None,
                    picked: Vec::new(),
                    return_event: false,
                    return_shop: true,
                    return_screen: None,
                    immediate: false,
                });
                self.screen = Screen::Grid;
            }
            return true;
        }
        if let Some(name) = label {
            if let Some(i) = self
                .shop
                .cards
                .iter()
                .position(|o| !o.sold && shop_card_matches(&o.item, name))
            {
                let price = self.shop.cards[i].price;
                if self.player.gold >= price {
                    self.spend_shop_gold(price);
                    let card = self.shop.cards[i].item.clone();
                    self.shop.cards[i].sold = true;
                    self.player.deck.push(card);
                }
                return true;
            }
            if let Some(i) = self
                .shop
                .relics
                .iter()
                .position(|o| !o.sold && shop_relic_matches(o.item, name))
            {
                let price = self.shop.relics[i].price;
                if self.player.gold >= price {
                    self.spend_shop_gold(price);
                    let id = self.shop.relics[i].item;
                    self.shop.relics[i].sold = true;
                    self.gain_relic(id);
                    if id == RelicId::Membership_Card {
                        self.discount_current_shop(0.5, true);
                    }
                }
                return true;
            }
            if let Some(i) = self
                .shop
                .potions
                .iter()
                .position(|o| !o.sold && shop_potion_matches(o.item, name))
            {
                let price = self.shop.potions[i].price;
                if self.player.gold >= price && !self.player.has_relic(RelicId::Sozu) {
                    self.spend_shop_gold(price);
                    let id = self.shop.potions[i].item;
                    self.shop.potions[i].sold = true;
                    self.gain_potion(id);
                }
                return true;
            }
            return true;
        }
        let affordable = self.shop_affordable();
        if let Some(kind) = affordable.get(index).copied() {
            self.buy_shop_kind(kind);
        }
        true
    }

    fn shop_affordable(&self) -> Vec<ShopKind> {
        let mut out = Vec::new();
        if self.shop.purge_available && self.player.gold >= self.shop.purge_cost {
            out.push(ShopKind::Purge);
        }
        for (i, offer) in self.shop.cards.iter().enumerate() {
            if !offer.sold && self.player.gold >= offer.price {
                out.push(ShopKind::Card(i));
            }
        }
        for (i, offer) in self.shop.relics.iter().enumerate() {
            if !offer.sold && self.player.gold >= offer.price {
                out.push(ShopKind::Relic(i));
            }
        }
        for (i, offer) in self.shop.potions.iter().enumerate() {
            if !offer.sold && self.player.gold >= offer.price {
                out.push(ShopKind::Potion(i));
            }
        }
        out
    }

    fn buy_shop_kind(&mut self, kind: ShopKind) {
        match kind {
            ShopKind::Purge => {
                let _ = self.buy_shop_item(Some("purge"), 0);
            }
            ShopKind::Card(i) => {
                if let Some(offer) = self.shop.cards.get_mut(i) {
                    if !offer.sold && self.player.gold >= offer.price {
                        let price = offer.price;
                        self.player.deck.push(offer.item.clone());
                        offer.sold = true;
                        self.spend_shop_gold(price);
                    }
                }
            }
            ShopKind::Relic(i) => {
                if let Some(offer) = self.shop.relics.get_mut(i) {
                    if !offer.sold && self.player.gold >= offer.price {
                        let price = offer.price;
                        let id = offer.item;
                        offer.sold = true;
                        self.spend_shop_gold(price);
                        self.gain_relic(id);
                        if id == RelicId::Membership_Card {
                            self.discount_current_shop(0.5, true);
                        }
                    }
                }
            }
            ShopKind::Potion(i) => {
                if let Some(offer) = self.shop.potions.get_mut(i) {
                    if !offer.sold
                        && self.player.gold >= offer.price
                        && !self.player.has_relic(RelicId::Sozu)
                    {
                        let price = offer.price;
                        let id = offer.item;
                        offer.sold = true;
                        self.spend_shop_gold(price);
                        self.gain_potion(id);
                    }
                }
            }
        }
    }

    fn discount_current_shop(&mut self, multiplier: f32, affect_purge: bool) {
        for offer in &mut self.shop.cards {
            offer.price = crate::rewards::gdx_round(offer.price as f32 * multiplier);
        }
        for offer in &mut self.shop.relics {
            offer.price = crate::rewards::gdx_round(offer.price as f32 * multiplier);
        }
        for offer in &mut self.shop.potions {
            offer.price = crate::rewards::gdx_round(offer.price as f32 * multiplier);
        }
        if self.player.has_relic(RelicId::Smiling_Mask) {
            self.shop.purge_cost = 50;
        } else if affect_purge {
            self.shop.purge_cost =
                crate::rewards::gdx_round(self.shop.purge_cost as f32 * multiplier);
        }
    }

    fn generate_chest(&mut self) {
        // AbstractDungeon.getRandomChest() then AbstractChest.randomizeReward().
        let size = self.rng.treasure.random_range(0, 99);
        let (common, uncommon, gold_chance, gold_amt) = if size < 50 {
            (75, 25, 50, 25) // Small
        } else if size < 83 {
            (35, 50, 35, 50) // Medium
        } else {
            (0, 75, 50, 75) // Large
        };
        let roll = self.rng.treasure.random_range(0, 99);
        self.chest_gold = roll < gold_chance;
        self.chest_gold_amt = gold_amt;
        self.chest_tier = if roll < common {
            RelicTier::COMMON
        } else if roll < common + uncommon {
            RelicTier::UNCOMMON
        } else {
            RelicTier::RARE
        };
    }

    fn step_treasure(&mut self, action: &Action) {
        if let Action::Choose { label: Some(label), .. } = action {
            if label == "open" {
                // fall through
            }
        }
        if self.current_room == RoomType::BossTreasure {
            self.boss_relics.clear();
            for _ in 0..3 {
                if let Some(id) = self.take_relic(RelicTier::BOSS) {
                    self.boss_relics.push(id);
                }
            }
            self.screen = Screen::BossRelic;
        } else {
            self.rewards.clear();
            // AbstractChest.open(false): onChestOpen, gold, chest relic,
            // sapphire key linked to the last reward, then onChestOpenAfter.
            self.on_chest_open(false);
            if self.chest_gold {
                let lo = self.chest_gold_amt as f32 * 0.9;
                let hi = self.chest_gold_amt as f32 * 1.1;
                let gold = self.rng.treasure.random_float_range(lo, hi).round() as i32;
                self.add_gold_to_rewards(gold);
            }
            if let Some(id) = self.take_relic(self.chest_tier) {
                self.add_relic_to_rewards(id);
            }
            self.add_sapphire_key_reward();
            self.on_chest_open_after(false);
            self.screen = Screen::CombatReward;
        }
    }

    fn step_boss_relic(&mut self, action: &Action) {
        if let Action::Choose { index, label, .. } = action {
            let picked = label
                .as_ref()
                .and_then(|name| {
                    self.boss_relics
                        .iter()
                        .copied()
                        .find(|id| id.sts_id() == name.as_str())
                })
                .or_else(|| self.boss_relics.get(*index).copied());
            if let Some(id) = picked {
                self.gain_relic(id);
            }
            return;
        }
        if !matches!(action, Action::Proceed | Action::Skip) {
            return;
        }
        self.begin_next_act();
    }

    fn begin_next_act(&mut self) {
        // Act 2 transition
        if self.rng.card.counter > 0 && self.rng.card.counter < 250 {
            self.rng.card.set_counter(250);
        } else if self.rng.card.counter > 250 && self.rng.card.counter < 500 {
            self.rng.card.set_counter(500);
        } else if self.rng.card.counter > 500 && self.rng.card.counter < 750 {
            self.rng.card.set_counter(750);
        }
        // AbstractDungeon between acts: A5+ heal 75% of missing HP (MathUtils.round).
        if self.ascension >= 5 {
            let missing = (self.player.max_hp - self.player.hp).max(0);
            self.heal_player(crate::rewards::gdx_round(missing as f32 * 0.75));
        } else {
            self.heal_player(self.player.max_hp);
        }
        self.potion_blizzard = 0;
        self.event_elite_chance = 0.1;
        self.event_monster_chance = 0.1;
        self.event_shop_chance = 0.03;
        self.event_treasure_chance = 0.02;
        let next = match self.dungeon.act {
            crate::ids::Act::Exordium => crate::ids::Act::City,
            crate::ids::Act::City => crate::ids::Act::Beyond,
            crate::ids::Act::Beyond => crate::ids::Act::Ending,
            crate::ids::Act::Ending => crate::ids::Act::Ending,
        };
        self.dungeon.generate_act(
            next,
            self.seed,
            &mut self.rng,
            &self.unlocks,
            self.character,
            self.ascension,
            self.final_act_available && !self.has_emerald_key,
        );
        self.screen = Screen::Map;
        self.done = false;
    }

    fn start_combat_encounter(&mut self, encounter: EncounterId) {
        self.combat = Some(Combat::start(
            encounter,
            &mut self.player,
            &mut self.rng,
            self.dungeon.floor,
            self.seed,
            self.ascension,
        ));
        self.screen = Screen::Combat;
        if self.player.has_relic(RelicId::Toolbox) {
            self.begin_toolbox_reward();
        } else {
            self.begin_gambling_chip();
        }
    }

    fn start_combat_in_current_room(&mut self) {
        let encounter = if self.current_room == RoomType::Boss {
            EncounterId::from_sts_key(&self.dungeon.boss).unwrap_or(EncounterId::Hexaghost)
        } else if self.current_room == RoomType::Elite {
            self.dungeon.next_elite().unwrap_or(EncounterId::GremlinNob)
        } else {
            self.dungeon.next_monster().unwrap_or(EncounterId::Cultist)
        };
        self.combat = Some(Combat::start(
            encounter,
            &mut self.player,
            &mut self.rng,
            self.dungeon.floor,
            self.seed,
            self.ascension,
        ));
        self.screen = Screen::Combat;
        if self.current_room == RoomType::Elite {
            self.apply_emerald_elite_buff();
        }
        if self.player.has_relic(RelicId::Toolbox) {
            self.begin_toolbox_reward();
        } else {
            self.begin_gambling_chip();
        }
    }

    fn apply_emerald_elite_buff(&mut self) {
        // AbstractPlayer.applyStartOfCombat + MonsterRoomElite.applyEmeraldEliteBuff
        if !self.final_act_available {
            return;
        }
        let (x, y) = (self.current_x, self.current_y);
        if y < 0 || x < 0 || y as usize >= self.dungeon.map.height() {
            return;
        }
        if !self.dungeon.map.node(x, y).emerald_key {
            return;
        }
        let Some(combat) = self.combat.as_mut() else {
            return;
        };
        let act = self.dungeon.act as i32;
        match self.rng.map.random_range(0, 3) {
            0 => {
                for m in combat.monsters.iter_mut() {
                    m.add_power(crate::ids::PowerId::Strength, act + 1);
                }
            }
            1 => {
                // IncreaseMaxHpAction: MathUtils.round(maxHealth * 0.25F), then
                // increaseMaxHp which heals the bonus. floor() is 1 low on .5
                // (Lagavulin 114 → 142 vs Java 143; Sentry 42 → 52 vs 53).
                for m in combat.monsters.iter_mut() {
                    let bonus = crate::rewards::gdx_round(m.max_hp as f32 * 0.25);
                    m.max_hp += bonus;
                    m.hp += bonus;
                }
            }
            2 => {
                for m in combat.monsters.iter_mut() {
                    m.add_power(crate::ids::PowerId::Metallicize, act * 2 + 2);
                }
            }
            _ => {
                // MonsterRoomElite.applyEmeraldEliteBuff case 3:
                // RegenerateMonsterPower(1 + actNum * 2).
                for m in combat.monsters.iter_mut() {
                    m.add_power(crate::ids::PowerId::Regen, 1 + act * 2);
                }
            }
        }
        // Emerald elite actions resolve before relic atBattleStart hooks.
        // Preserved Insect therefore caps against the increased max HP when
        // the elite rolled IncreaseMaxHpAction (seed 79: 105 -> 78 HP).
        if self.player.has_relic(RelicId::PreservedInsect) {
            for m in combat.monsters.iter_mut() {
                let cap = (m.max_hp as f32 * 0.75) as i32;
                if m.hp > cap {
                    m.hp = cap;
                }
            }
        }
    }

    fn add_emerald_key_reward(&mut self) {
        // MonsterRoomElite.addEmeraldKey
        if !self.final_act_available || self.has_emerald_key || self.rewards.is_empty() {
            return;
        }
        let (x, y) = (self.current_x, self.current_y);
        if y < 0 || x < 0 || y as usize >= self.dungeon.map.height() {
            return;
        }
        if !self.dungeon.map.node(x, y).emerald_key {
            return;
        }
        self.rewards.push(Reward::new(RewardKind::EmeraldKey));
    }

    fn add_sapphire_key_reward(&mut self) {
        // AbstractChest.open: Settings.isFinalActAvailable && !hasSapphireKey.
        if !self.final_act_available || self.has_sapphire_key || self.rewards.is_empty() {
            return;
        }
        let last = self.rewards.len() - 1;
        self.rewards[last].relic_link = Some(last + 1);
        self.rewards.push(Reward {
            kind: RewardKind::SapphireKey,
            taken: false,
            relic_link: Some(last),
            card_options: None,
        });
    }

    fn claim_emerald_key(&mut self) {
        if let Some(r) = self
            .rewards
            .iter_mut()
            .find(|r| matches!(r.kind, RewardKind::EmeraldKey) && !r.taken)
        {
            r.taken = true;
            self.has_emerald_key = true;
        }
    }

    fn begin_memories_select(&mut self) {
        if self.player.discard.is_empty() {
            return;
        }
        let needed = if self.player.has_relic(RelicId::SacredBark) {
            2
        } else {
            1
        };
        if self.player.discard.len() <= needed {
            while !self.player.discard.is_empty() && self.player.hand.len() < 10 {
                let mut c = self.player.discard.remove(0);
                c.cost_for_turn = 0;
                self.player.hand.push(c);
            }
            return;
        }
        self.memories_select = true;
        self.grid = Some(GridSelect {
            kind: GridKind::DiscardToHand,
            needed,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: false,
            return_screen: None,
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    fn begin_gambling_chip(&mut self) {
        if !self.player.has_relic(RelicId::Gambling_Chip) || self.player.hand.is_empty() {
            return;
        }
        self.open_gambling_select();
    }

    fn open_gambling_select(&mut self) {
        self.gambling_select = true;
        self.exhaust_select = false;
        self.put_on_deck_select = false;
        self.hand_held.clear();
        self.pending_cards.clear();
        self.screen = Screen::HandSelect;
    }

    fn roll_event_room(&mut self, prev_room: RoomType) -> Option<RoomType> {
        // EventHelper.roll uses a reconstructed Random(seed, counter), then the
        // dungeon writes that instance back. Vanilla never fills the elite band
        // unless the Deadly Events modifier is on.
        let mut dup = StsRandom::from_seed_counter(self.seed, self.rng.event.counter);
        let roll = dup.random_float();
        self.rng.event = dup;
        let mut force_chest = false;
        if let Some(r) = self.player.relics.iter_mut().find(|r| r.id == RelicId::Tiny_Chest) {
            r.counter += 1;
            if r.counter == 4 {
                r.counter = 0;
                force_chest = true;
            }
        }
        let monster_size = (self.event_monster_chance * 100.0) as i32;
        // ShopRoom still current when Java rolls, so a shop does not convert into another shop.
        let shop_size = if prev_room == RoomType::Shop {
            0
        } else {
            (self.event_shop_chance * 100.0) as i32
        };
        let treasure_size = (self.event_treasure_chance * 100.0) as i32;
        let idx = (roll * 100.0) as i32;
        let mut fill = 0;
        let mut choice = if idx < fill + monster_size {
            Some(RoomType::Monster)
        } else {
            fill += monster_size;
            if idx < fill + shop_size {
                Some(RoomType::Shop)
            } else {
                fill += shop_size;
                if idx < fill + treasure_size {
                    Some(RoomType::Treasure)
                } else {
                    None
                }
            }
        };
        if force_chest {
            choice = Some(RoomType::Treasure);
        }
        self.after_event_roll(choice)
    }

    fn after_event_roll(&mut self, choice: Option<RoomType>) -> Option<RoomType> {
        if matches!(choice, Some(RoomType::Elite)) {
            self.event_elite_chance = 0.0;
        } else {
            self.event_elite_chance += 0.1;
        }
        // EventHelper.roll: Juzu converts MONSTER → EVENT after the chance
        // reset, so monsterChance still drops to 0.1.
        let mut choice = choice;
        if matches!(choice, Some(RoomType::Monster)) {
            if self.player.has_relic(RelicId::Juzu_Bracelet) {
                choice = None;
            }
            self.event_monster_chance = 0.1;
        } else {
            self.event_monster_chance += 0.1;
        }
        if matches!(choice, Some(RoomType::Shop)) {
            self.event_shop_chance = 0.03;
        } else {
            self.event_shop_chance += 0.03;
        }
        if matches!(choice, Some(RoomType::Treasure)) {
            self.event_treasure_chance = 0.02;
        } else {
            self.event_treasure_chance += 0.02;
        }
        choice
    }

    fn pick_event(&mut self, rng: &mut StsRandom) -> String {
        if rng.random_float_range(0.0, 1.0) < 0.25 {
            self.pick_shrine(rng)
        } else {
            self.pick_normal_event(rng)
        }
    }

    fn pick_shrine(&mut self, rng: &mut StsRandom) -> String {
        // AbstractDungeon.getShrine: shrineList plus specialOneTimeEventList
        // with per-event filters (act / gold / HP / relics / curses).
        let mut tmp = self.dungeon.shrine_list.clone();
        let dungeon_id = self.dungeon.id;
        let cursed = self.player.deck.iter().any(|c| {
            c.card_type() == crate::ids::CardType::CURSE
                && !matches!(
                    c.id,
                    CardId::Necronomicurse | CardId::CurseOfTheBell | CardId::AscendersBane
                )
        });
        for e in self.dungeon.special_one_time.iter() {
            let include = match e.as_str() {
                "Fountain of Cleansing" => cursed,
                "Designer" => {
                    matches!(dungeon_id, "TheCity" | "TheBeyond") && self.player.gold >= 75
                }
                "Duplicator" => matches!(dungeon_id, "TheCity" | "TheBeyond"),
                "FaceTrader" => matches!(dungeon_id, "TheCity" | "Exordium"),
                "Knowing Skull" => dungeon_id == "TheCity" && self.player.hp > 12,
                "N'loth" => dungeon_id == "TheCity" && self.player.relics.len() >= 2,
                "The Joust" => dungeon_id == "TheCity" && self.player.gold >= 50,
                "The Woman in Blue" => self.player.gold >= 50,
                "SecretPortal" => false,
                _ => true,
            };
            if include {
                tmp.push(e.clone());
            }
        }
        if tmp.is_empty() {
            return "Scrap Ooze".into();
        }
        let key = tmp[rng.random_int(tmp.len() as i32 - 1) as usize].clone();
        self.dungeon.shrine_list.retain(|s| s != &key);
        self.dungeon.special_one_time.retain(|s| s != &key);
        key
    }

    fn pick_normal_event(&mut self, rng: &mut StsRandom) -> String {
        let mut tmp = Vec::new();
        for e in self.dungeon.event_list.iter() {
            match e.as_str() {
                "Dead Adventurer" | "Mushrooms" => {
                    if self.dungeon.floor > 6 {
                        tmp.push(e.clone());
                    }
                }
                "The Cleric" => {
                    if self.player.gold >= 35 {
                        tmp.push(e.clone());
                    }
                }
                "The Moai Head" => {
                    let bloodied = (self.player.hp as f32) / (self.player.max_hp as f32) <= 0.5;
                    if self.player.has_relic(RelicId::Golden_Idol) || bloodied {
                        tmp.push(e.clone());
                    }
                }
                "Beggar" => {
                    if self.player.gold >= 75 {
                        tmp.push(e.clone());
                    }
                }
                "Colosseum" => {
                    if self.current_y > (self.dungeon.map.height() as i32) / 2 {
                        tmp.push(e.clone());
                    }
                }
                _ => tmp.push(e.clone()),
            }
        }
        if tmp.is_empty() {
            return self.pick_shrine(rng);
        }
        let key = tmp[rng.random_int(tmp.len() as i32 - 1) as usize].clone();
        self.dungeon.event_list.retain(|s| s != &key);
        key
    }

    fn start_event(&mut self) {
        // generateEvent uses a duplicate of eventRng and does not write it back.
        let mut local = StsRandom::from_seed_counter(self.seed, self.rng.event.counter);
        let id = self.pick_event(&mut local);
        let mut data = Vec::new();
        let options = match id.as_str() {
            "Scrap Ooze" => {
                // ScrapOoze: dmg=3, A15+ dmg=5; relicObtainChance=25.
                let dmg = if self.ascension >= 15 { 5 } else { 3 };
                data = vec![dmg, 25];
                vec![
                    format!("[Reach Inside] #rLose #r{dmg} #rHP. #g25%: #gFind #ga #gRelic."),
                    "[Leave]".into(),
                ]
            }
            "Woman in Blue" | "The Woman in Blue" => vec![
                "[Buy 1 Potion]".into(),
                "[Buy 2 Potions]".into(),
                "[Buy 3 Potions]".into(),
                "[Leave]".into(),
            ],
            "The Library" => vec!["[Read]".into(), "[Sleep]".into()],
            "Ghosts" => vec![
                "[Accept] #gReceive #g5 Apparition. #rLose #r40 #rMax #rHP.".into(),
                "[Refuse]".into(),
            ],
            "Falling" => vec!["[Continue]".into()],
            "SensoryStone" => vec!["[Interact]".into()],
            "Winding Halls" => {
                let (hp_pct, heal_pct) = if self.ascension >= 15 {
                    (0.18, 0.2)
                } else {
                    (0.125, 0.25)
                };
                let hp_loss = crate::rewards::gdx_round(self.player.max_hp as f32 * hp_pct);
                let heal = crate::rewards::gdx_round(self.player.max_hp as f32 * heal_pct);
                let max_hp_loss =
                    crate::rewards::gdx_round(self.player.max_hp as f32 * 0.05);
                data = vec![hp_loss, heal, max_hp_loss];
                vec!["[Continue]".into()]
            }
            "MindBloom" => vec!["[I am War]".into(), "[I am Awake]".into(), "[I am Rich]".into()],
            "World of Goop" => {
                let (lo, hi) = if self.ascension >= 15 { (35, 75) } else { (20, 50) };
                let mut loss = self.rng.misc.random_range(lo, hi);
                if loss > self.player.gold {
                    loss = self.player.gold;
                }
                data = vec![loss, 75, 11];
                vec![
                    format!("[Gather Gold] #gGain #g75 #gGold. #rTake #r11 #rDamage."),
                    format!("[Leave] #rLose #r{loss} #rGold."),
                ]
            }
            "Big Fish" => {
                let heal = self.player.max_hp / 3;
                data = vec![heal];
                vec![
                    format!("[Banana] #gHeal #g{heal} #gHP."),
                    "[Donut] #gMax #gHP #g+5.".into(),
                    "[Box] #gObtain #ga #gRelic. #rBecome #rCursed #r- #rRegret.".into(),
                ]
            }
            "The Cleric" => {
                let heal = (self.player.max_hp as f32 * 0.25) as i32;
                let purify = if self.ascension >= 15 { 75 } else { 50 };
                data = vec![heal, purify];
                let mut opts = Vec::new();
                if self.player.gold >= 35 {
                    opts.push(format!("[Heal] #y35 #yGold: #gHeal #g{heal} #gHP."));
                }
                if self.player.gold >= purify {
                    opts.push(format!("[Purify] #y{purify} #yGold: #gRemove #ga #gcard #gfrom #gyour #gdeck."));
                }
                opts.push("[Leave]".into());
                opts
            }
            "Living Wall" => {
                let mut opts = vec![
                    "[Forget] #gRemove #ga #gcard #gfrom #gyour #gdeck.".into(),
                    "[Change] #gTransform #ga #gcard #gin #gyour #gdeck.".into(),
                ];
                if self.player.deck.iter().any(|c| c.can_upgrade()) {
                    opts.push("[Grow] #gUpgrade #ga #gcard #gin #gyour #gdeck.".into());
                }
                opts
            }
            "Shining Light" => {
                let pct = if self.ascension >= 15 { 0.3 } else { 0.2 };
                let damage = (self.player.max_hp as f32 * pct + 0.5).floor() as i32;
                data = vec![damage];
                let mut opts = Vec::new();
                if self.player.deck.iter().any(|c| c.can_upgrade()) {
                    opts.push(format!(
                        "[Enter] #gUpgrade #g2 #grandom #gcards. #rLose #r{damage} #rHP."
                    ));
                }
                opts.push("[Leave]".into());
                opts
            }
            "WeMeetAgain" => {
                // Constructor order: getRandomPotion, getGoldAmount, getRandomNonBasicCard.
                let mut potion_slots: Vec<i32> = self
                    .player
                    .potions
                    .iter()
                    .filter(|p| p.id != PotionId::Slot)
                    .map(|p| p.slot)
                    .collect();
                let potion_slot = if potion_slots.is_empty() {
                    -1
                } else {
                    let seed = self.rng.misc.random_long();
                    shuffle_java(&mut potion_slots, seed);
                    potion_slots[0]
                };
                let gold_amt = if self.player.gold < 50 {
                    0
                } else if self.player.gold > 150 {
                    self.rng.misc.random_range(50, 150)
                } else {
                    self.rng.misc.random_range(50, self.player.gold)
                };
                let seed = self.rng.misc.random_long();
                let mut idxs: Vec<usize> = self
                    .player
                    .deck
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.rarity() != CardRarity::BASIC && c.card_type() != crate::ids::CardType::CURSE)
                    .map(|(i, _)| i)
                    .collect();
                shuffle_java(&mut idxs, seed);
                let card_idx = idxs.first().copied().map(|i| i as i32).unwrap_or(-1);
                data = vec![gold_amt, card_idx, potion_slot];
                let mut opts = Vec::new();
                // ChoiceDriver skips disabled buttons; potion is enabled when a real potion exists.
                if potion_slot >= 0 {
                    opts.push("[Give Potion] #rLose #ra #rPotion. #gObtain #ga #gRelic.".into());
                }
                if gold_amt != 0 {
                    opts.push(format!(
                        "[Give Gold] #rLose #r{gold_amt} #gGold. #gObtain #ga #gRelic."
                    ));
                }
                if card_idx >= 0 {
                    opts.push("[Give Card]".into());
                }
                opts.push("[Attack]".into());
                opts
            }
            "Golden Shrine" => {
                let pray = if self.ascension >= 15 { 50 } else { 100 };
                vec![
                    format!("[Pray] #gGain #g{pray} #gGold."),
                    "[Desecrate] #gGain #g275 #gGold. #rBecome #rCursed #r- #rRegret.".into(),
                    "[Leave]".into(),
                ]
            }
            "Golden Idol" => {
                vec![
                    "[Take] #gObtain #gGolden #gIdol.".into(),
                    "[Leave]".into(),
                ]
            }
            "Mushrooms" => {
                let heal = (self.player.max_hp as f32 * 0.25) as i32;
                data = vec![heal];
                vec![
                    "[Fight]".into(),
                    format!("[Eat] #gHeal #g{heal} #gHP. #rBecome #rCursed #r- #rParasite."),
                ]
            }
            "Masked Bandits" => vec![
                "[Pay] #rLose #rALL #rGold.".into(),
                "[Fight] #gObtain #gRed #gMask.".into(),
            ],
            "Addict" => {
                let can_pay = self.player.gold >= 85;
                data = vec![i32::from(can_pay)];
                let mut opts = Vec::new();
                if can_pay {
                    opts.push("[Offer Gold] #y85 #yGold: #gObtain #ga #gRelic.".into());
                }
                opts.push(
                    "[Rob] #gObtain #ga #gRelic. #rBecome #rCursed #r- #rShame.".into(),
                );
                opts.push("[Leave]".into());
                opts
            }
            "Nest" => {
                let gold = if self.ascension >= 15 { 50 } else { 99 };
                data = vec![gold, 6];
                vec!["[Continue]".into()]
            }
            "Knowing Skull" => {
                // KnowingSkull starts with an INTRO Continue. The four paid
                // choices are installed only after that button is pressed.
                data = vec![6, 6, 6, 6];
                vec!["[Continue]".into()]
            }
            "The Joust" => {
                // betFor and ownerWins are populated on the following screens.
                data = vec![0, 0];
                vec!["[Continue]".into()]
            }
            "Forgotten Altar" => {
                // ForgottenAltar ctor: hpLoss = MathUtils.round(max * 0.25),
                // A15+ 0.35. Golden Idol option is disabled without the relic;
                // ChoiceDriver skips disabled buttons so omit it.
                let pct = if self.ascension >= 15 { 0.35 } else { 0.25 };
                let hp_loss = crate::rewards::gdx_round(self.player.max_hp as f32 * pct);
                data = vec![hp_loss];
                let mut opts = Vec::new();
                if self.player.has_relic(RelicId::Golden_Idol) {
                    opts.push("[Offer] #gObtain #gBloody #gIdol.".into());
                }
                opts.push(format!(
                    "[Sacrifice] #gMax #gHP #g+5. #rLose #r{hp_loss} #rHP."
                ));
                opts.push("[Desecrate] #rBecome #rCursed #r- #rDecay.".into());
                opts
            }
            "Drug Dealer" => {
                // DrugDealer ctor: JAX, transform 2 if >=2 purgeable, MutagenicStrength.
                // Transform is disabled below that; ChoiceDriver skips it.
                let mut opts = vec!["[Ingest] #gObtain #gJ.A.X.".into()];
                let purgeable = self.player.deck.iter().filter(|c| purgeable_card(c)).count();
                if purgeable >= 2 {
                    opts.push("[Study] #gTransform #g2 #gcards.".into());
                }
                opts.push("[Inject] #gObtain #gMutagenic #gStrength.".into());
                opts
            }
            "Designer" => {
                // Designer ctor: two miscRng.randomBoolean(), then INTRO Continue.
                let upg_one = if self.rng.misc.random_boolean() { 1 } else { 0 };
                let rem_cards = if self.rng.misc.random_boolean() { 1 } else { 0 };
                let (adj, clean, full, hp) = if self.ascension >= 15 {
                    (50, 75, 110, 5)
                } else {
                    (40, 60, 90, 3)
                };
                data = vec![adj, clean, full, hp, upg_one, rem_cards, 0];
                vec!["[Continue]".into()]
            }
            "Lab" => vec!["[Search] #gFind #gsome #gPotions!".into()],
            "NoteForYourself" => {
                // NoteForYourself ctor: one INTRO option, then CHOOSE take/leave.
                vec!["[Continue]".into()]
            }
            "Back to Basics" => vec![
                "[Elegance] #gRemove #ga #gcard #gfrom #gyour #gdeck.".into(),
                "[Simplicity] #gUpgrade #gall #gStrikes #gand #gDefends.".into(),
            ],
            "Bonfire Elementals" => vec!["[Continue]".into()],
            "FaceTrader" => {
                // FaceTrader: A15+ gold=50 else 75; damage = maxHp/10 (min 1).
                let gold = if self.ascension >= 15 { 50 } else { 75 };
                let mut dmg = self.player.max_hp / 10;
                if dmg == 0 {
                    dmg = 1;
                }
                data = vec![dmg, gold];
                vec!["[Continue]".into()]
            }
            "Wheel of Change" => {
                // GremlinWheelGame: gold by act; A15+ hpLoss 0.15 else 0.1.
                data = vec![self.wheel_gold_amount(), 0, 0];
                vec!["[Play]".into()]
            }
            "Match and Keep!" => vec!["[Continue]".into()],
            "Colosseum" => vec!["[Continue]".into()],
            _ => vec!["[Continue]".into(), "[Leave]".into()],
        };
        if id == "Falling" {
            // Falling.setCards: attack, then skill, then power via miscRng.
            let attack = self.pick_deck_index_of_type(crate::ids::CardType::ATTACK);
            let skill = self.pick_deck_index_of_type(crate::ids::CardType::SKILL);
            let power = self.pick_deck_index_of_type(crate::ids::CardType::POWER);
            data = vec![
                skill.map(|i| i as i32).unwrap_or(-1),
                power.map(|i| i as i32).unwrap_or(-1),
                attack.map(|i| i as i32).unwrap_or(-1),
            ];
        }
        let (match_cards, match_attempts) = if id == "Match and Keep!" {
            (self.initialize_match_cards(), 5)
        } else {
            (Vec::new(), 0)
        };
        self.event = Some(EventState {
            id,
            screen: 0,
            options,
            data,
            library_cards: Vec::new(),
            match_cards,
            match_chosen: None,
            match_attempts,
        });
        self.screen = Screen::Event;
    }

    /// GremlinWheelGame.setGold: Exordium 100, City 200, Beyond 300.
    fn wheel_gold_amount(&self) -> i32 {
        match self.dungeon.act {
            crate::ids::Act::Exordium => 100,
            crate::ids::Act::City => 200,
            crate::ids::Act::Beyond | crate::ids::Act::Ending => 300,
        }
    }

    fn wheel_hp_loss_percent(&self) -> f32 {
        if self.ascension >= 15 {
            0.15
        } else {
            0.1
        }
    }

    fn wheel_prize_option(&self, result: i32) -> String {
        match result {
            0 => "[Prize!] YAY!!!!".into(),
            1 => "[Prize!] #gObtain #ga #gRelic.".into(),
            2 => "[Prize!] #gHeal #gto #gfull #ghealth.".into(),
            3 => "[Prize?] #rCurse #r- #rDecay.".into(),
            4 => "[Prize!] #gRemove #ga #gcard #gfrom #gyour #gdeck.".into(),
            _ => {
                let dmg = (self.player.max_hp as f32 * self.wheel_hp_loss_percent()) as i32;
                format!("[Prize?] #rLose #r{dmg} #rHP.")
            }
        }
    }

    /// GremlinWheelGame.preApplyResult: gold is granted when the COMPLETE
    /// dialog appears, before the prize button.
    fn wheel_pre_apply(&mut self, result: i32, gold: i32) {
        if result == 0 && !self.player.has_relic(RelicId::Ectoplasm) {
            self.player.gold += gold;
        }
    }

    /// GremlinWheelGame.applyResult. Relic uses noCardsInRewards (relic only).
    fn wheel_apply_result(&mut self, result: i32) {
        match result {
            0 => {}
            1 => {
                self.rewards.clear();
                self.card_reward.clear();
                if let Some(id) = self.next_screenless_relic() {
                    self.add_relic_to_rewards(id);
                }
                self.screen = Screen::CombatReward;
            }
            2 => self.heal_player(self.player.max_hp),
            3 => self.obtain_master_deck_card(CardId::Decay),
            4 => {
                if self.player.deck.iter().any(purgeable_card) {
                    self.open_grid(GridKind::Purge, 1, true);
                }
            }
            _ => {
                let dmg = (self.player.max_hp as f32 * self.wheel_hp_loss_percent()) as i32;
                let dmg = combat::on_lose_hp_last(&self.player, dmg);
                self.player.hp = (self.player.hp - dmg).max(0);
            }
        }
    }

    /// `GremlinMatchGame.initializeCards` then `Collections.shuffle(..., miscRng.randomLong())`.
    fn initialize_match_cards(&mut self) -> Vec<MatchCard> {
        let mut ids = Vec::new();
        ids.push(self.random_card(CardRarity::RARE, false));
        ids.push(self.random_card(CardRarity::UNCOMMON, false));
        ids.push(self.random_card(CardRarity::COMMON, false));
        if self.ascension >= 15 {
            ids.push(Some(self.return_random_curse()));
            ids.push(Some(self.return_random_curse()));
        } else {
            ids.push(Some(self.return_colorless_uncommon()));
            ids.push(Some(self.return_random_curse()));
        }
        ids.push(Some(self.start_card_for_event()));
        let ids: Vec<CardId> = ids.into_iter().flatten().collect();
        let mut cards: Vec<MatchCard> = ids
            .iter()
            .chain(ids.iter())
            .copied()
            .map(|id| MatchCard {
                id,
                flipped: false,
                revealed: false,
            })
            .collect();
        let seed = self.rng.misc.random_long();
        shuffle_java(&mut cards, seed);
        cards
    }

    /// `CardLibrary.getCurse`: `curses` HashMap iteration, skip specials, `cardRng`.
    fn return_random_curse(&mut self) -> CardId {
        const CURSES: &[CardId] = &[
            CardId::Regret,
            CardId::Injury,
            CardId::Shame,
            CardId::Parasite,
            CardId::Normality,
            CardId::Doubt,
            CardId::Writhe,
            CardId::Pain,
            CardId::Decay,
            CardId::Clumsy,
        ];
        let i = self.rng.card.random_range(0, CURSES.len() as i32 - 1) as usize;
        CURSES[i]
    }

    /// `AbstractDungeon.returnColorlessCard(UNCOMMON)`: shuffle `colorlessCardPool` in place.
    fn return_colorless_uncommon(&mut self) -> CardId {
        let seed = self.rng.shuffle.random_long();
        shuffle_java(
            std::sync::Arc::make_mut(&mut self.dungeon.colorless_cards).as_mut_slice(),
            seed,
        );
        self.dungeon
            .colorless_cards
            .iter()
            .copied()
            .find(|id| id.def().rarity == CardRarity::UNCOMMON)
            .unwrap_or(CardId::Swift_Strike)
    }

    fn start_card_for_event(&self) -> CardId {
        match self.character {
            Character::Ironclad => CardId::Bash,
            Character::Silent => CardId::Neutralize,
            Character::Defect => CardId::Zap,
            Character::Watcher => CardId::Eruption,
        }
    }

    fn step_gremlin_match(&mut self, index: usize) {
        let screen = self.event.as_ref().map(|e| e.screen).unwrap_or(0);
        match screen {
            0 => {
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Play]".into()];
                }
            }
            1 => {
                if let Some(event) = self.event.as_mut() {
                    for card in &mut event.match_cards {
                        card.flipped = true;
                    }
                    event.screen = 2;
                    event.options = match_play_options(&event.match_cards);
                }
            }
            2 => self.flip_match_card(index),
            _ => self.open_map(),
        }
    }

    fn flip_match_card(&mut self, index: usize) {
        let mut obtain = None;
        if let Some(event) = self.event.as_mut() {
            let flipped: Vec<usize> = event
                .match_cards
                .iter()
                .enumerate()
                .filter(|(_, c)| c.flipped)
                .map(|(i, _)| i)
                .collect();
            let Some(&pick) = flipped.get(index) else {
                return;
            };
            event.match_cards[pick].flipped = false;
            event.match_cards[pick].revealed = true;
            if let Some(chosen) = event.match_chosen.take() {
                let matched = event.match_cards[chosen].id == event.match_cards[pick].id;
                if matched {
                    obtain = Some(event.match_cards[chosen].id);
                    let (lo, hi) = if chosen < pick {
                        (chosen, pick)
                    } else {
                        (pick, chosen)
                    };
                    event.match_cards.remove(hi);
                    event.match_cards.remove(lo);
                } else {
                    event.match_cards[chosen].flipped = true;
                    event.match_cards[pick].flipped = true;
                }
                event.match_attempts -= 1;
                if event.match_attempts == 0 {
                    event.screen = 3;
                    event.options = vec!["[Leave]".into()];
                } else {
                    event.options = match_play_options(&event.match_cards);
                }
            } else {
                event.match_chosen = Some(pick);
                event.options = match_play_options(&event.match_cards);
            }
        }
        if let Some(id) = obtain {
            self.obtain_master_deck_card(id);
        }
    }

    /// `ShowCardAndObtainEffect` after the match waitTimer: `makeCopy`, then
    /// Omamori / `onObtainCard` / `souls.obtain` before the next snapshot.
    fn obtain_master_deck_card(&mut self, id: CardId) {
        let mut card = Card::new(id);
        if card.card_type() == crate::ids::CardType::CURSE {
            if let Some(oma) = self
                .player
                .relics
                .iter_mut()
                .find(|r| r.id == RelicId::Omamori)
            {
                if oma.counter != 0 {
                    oma.counter -= 1;
                    if oma.counter == 0 {
                        oma.used_up = true;
                    }
                    return;
                }
            }
        }
        crate::rewards::preview_obtain(&self.player, &mut card);
        if card.card_type() == crate::ids::CardType::CURSE
            && self.player.has_relic(RelicId::Darkstone_Periapt)
        {
            self.increase_max_hp(6);
        }
        if self.player.has_relic(RelicId::CeramicFish) && !self.player.has_relic(RelicId::Ectoplasm)
        {
            self.player.gold += 9;
        }
        self.player.deck.push(card);
    }

    /// FaceTrader.getRandomFace: shuffle missing face relics with `miscRng.randomLong()`.
    fn face_trader_random_face(&mut self) -> Option<RelicId> {
        let mut ids = Vec::new();
        for id in [
            RelicId::CultistMask,
            RelicId::FaceOfCleric,
            RelicId::GremlinMask,
            RelicId::NlothsMask,
            RelicId::SsserpentHead,
        ] {
            if !self.player.has_relic(id) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return RelicId::from_sts_id("Circlet");
        }
        let seed = self.rng.misc.random_long();
        shuffle_java(&mut ids, seed);
        ids.first().copied()
    }

    /// Designer MAIN buttons. Disabled options are omitted (ChoiceDriver skips them).
    fn designer_main_options(&self) -> Vec<String> {
        let adj = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(40);
        let clean = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(60);
        let full = self.event.as_ref().and_then(|e| e.data.get(2).copied()).unwrap_or(90);
        let hp = self.event.as_ref().and_then(|e| e.data.get(3).copied()).unwrap_or(3);
        let upg_one = self.event.as_ref().and_then(|e| e.data.get(4).copied()).unwrap_or(0) != 0;
        let rem_cards = self.event.as_ref().and_then(|e| e.data.get(5).copied()).unwrap_or(0) != 0;
        let upgradable = self.player.deck.iter().any(|c| c.can_upgrade());
        let unbottled = self
            .player
            .deck
            .iter()
            .filter(|c| purgeable_card(c) && !c.in_bottle)
            .count();
        let mut opts = Vec::new();
        if self.player.gold >= adj && upgradable {
            if upg_one {
                opts.push(format!("[Adjust] #y{adj} #yGold: #gUpgrade #g1 #gcard."));
            } else {
                opts.push(format!("[Adjust] #y{adj} #yGold: #gUpgrade #g2 #grandom #gcards."));
            }
        }
        if rem_cards {
            if self.player.gold >= clean && unbottled > 0 {
                opts.push(format!("[Clean Up] #y{clean} #yGold: #gRemove #g1 #gcard."));
            }
        } else if self.player.gold >= clean && unbottled >= 2 {
            opts.push(format!("[Clean Up] #y{clean} #yGold: #gTransform #g2 #gcards."));
        }
        if self.player.gold >= full && unbottled > 0 {
            opts.push(format!("[Full Service] #y{full} #yGold: #gRemove #g1, #gUpgrade #g1."));
        }
        opts.push(format!("[Punch] #rLose #r{hp} #rHP."));
        opts
    }

    fn pick_deck_index_of_type(&mut self, card_type: crate::ids::CardType) -> Option<usize> {
        let idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| c.card_type() == card_type)
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            None
        } else {
            let i = self.rng.misc.random_int(idxs.len() as i32 - 1) as usize;
            Some(idxs[i])
        }
    }

    fn step_event(&mut self, action: &Action) {
        let Action::Choose { index, label, .. } = action else {
            return;
        };
        let (id, screen, option_count) = match &self.event {
            Some(event) => (event.id.clone(), event.screen, event.options.len()),
            None => {
                self.open_map();
                return;
            }
        };
        if id == "SpireHeart" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Attack] #b???".into()];
                    }
                }
                1 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Continue]".into()];
                    }
                }
                2 => {
                    let go_to_ending = self.final_act_available
                        && self.has_ruby_key
                        && self.has_emerald_key
                        && self.has_sapphire_key;
                    if let Some(event) = self.event.as_mut() {
                        event.screen = if go_to_ending { 4 } else { 3 };
                        event.options = vec![if go_to_ending {
                            "[Approach Door]".into()
                        } else {
                            "[Continue]".into()
                        }];
                    }
                }
                3 => {
                    self.done = true;
                    self.screen = Screen::Terminal;
                }
                4 => {
                    if let Some(event) = self.event.as_mut() {
                        event.options.clear();
                    }
                    self.screen = Screen::DoorUnlock;
                }
                _ => {}
            }
            return;
        }
        if id == "Tomb of Lord Red Mask" && screen == 0 && *index == 1 {
            // TombRedMask INTRO's third raw button is Leave. ChoiceDriver
            // omits the disabled first button when the player has no mask, so
            // it arrives as normalized index 1 and opens the map immediately.
            self.open_map();
            return;
        }
        if id == "Colosseum" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Fight]".into()];
                    }
                }
                1 => {
                    // Colosseum FIGHT: rewardAllowed=false and the first
                    // encounter is exactly Blue + Red Slaver (seed 369).
                    self.rewards.clear();
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                    }
                    self.start_combat_encounter(EncounterId::ColosseumSlavers);
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Wheel of Change" {
            // GremlinWheelGame.buttonEffect: INTRO Play rolls miscRng.random(0,5)
            // then hides the dialog. ExactTextSim then publishes one event
            // boundary per discrete spin flag (buttonPressed, finishSpin,
            // doneSpinning, bounceIn=false) and COMPLETE. Timers are not
            // simulated; 5 chooses after Play land preApplyResult.
            match screen {
                0 => {
                    let result = self.rng.misc.random_range(0, 5);
                    if let Some(event) = self.event.as_mut() {
                        if event.data.len() < 3 {
                            event.data.resize(3, 0);
                        }
                        event.data[1] = result;
                        event.data[2] = 0;
                        event.screen = 1;
                        event.options = vec!["spin".into()];
                    }
                }
                1 => {
                    let (gold, result, step) = {
                        let e = self.event.as_ref();
                        (
                            e.and_then(|e| e.data.first().copied()).unwrap_or(100),
                            e.and_then(|e| e.data.get(1).copied()).unwrap_or(0),
                            e.and_then(|e| e.data.get(2).copied()).unwrap_or(0) + 1,
                        )
                    };
                    if let Some(event) = self.event.as_mut() {
                        if event.data.len() < 3 {
                            event.data.resize(3, 0);
                        }
                        event.data[2] = step;
                    }
                    if step >= 5 {
                        self.wheel_pre_apply(result, gold);
                        let prize = self.wheel_prize_option(result);
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 2;
                            event.options = vec![prize];
                        }
                    }
                }
                2 => {
                    let result = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(1).copied())
                        .unwrap_or(0);
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 3;
                        event.options = vec!["[Leave]".into()];
                    }
                    self.wheel_apply_result(result);
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Match and Keep!" {
            // GremlinMatchGame.buttonEffect: INTRO Continue → RULE Play → PLAY
            // flips. The private waitTimer blocks ExactTextSim after the
            // second flip of an attempt, so the next published boundary is
            // already resolved (match obtain or both cards face-down again).
            // Five attempts then CLEAN_UP/COMPLETE Leave.
            self.step_gremlin_match(*index);
            return;
        }
        if id == "NoteForYourself" {
            // INTRO Continue → CHOOSE take saved card or Leave → COMPLETE Leave/openMap.
            // Default obtain card is Iron Wave (playerPref NOTE_CARD missing in fixture).
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            "[Take] Iron Wave.".into(),
                            "[Leave]".into(),
                        ];
                    }
                }
                1 => {
                    if *index == 0 {
                        self.obtain_master_deck_card(CardId::Iron_Wave);
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "FaceTrader" {
            // FaceTrader.buttonEffect: INTRO Continue → MAIN Touch/Trade/Leave → RESULT Leave.
            match screen {
                0 => {
                    let dmg = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(1);
                    let gold = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(75);
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            format!("[Touch] #rLose #r{dmg} #rHP, #ggain #g{gold} #gGold."),
                            "[Trade] #g50%: #gGood #gFace. #r50%: #rBad #rFace.".into(),
                            "[Leave]".into(),
                        ];
                    }
                }
                1 => {
                    match *index {
                        0 => {
                            let dmg = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(1);
                            let gold = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(75);
                            let dmg = combat::on_lose_hp_last(&self.player, dmg);
                            self.player.hp = (self.player.hp - dmg).max(0);
                            if !self.player.has_relic(RelicId::Ectoplasm) {
                                self.player.gold += gold;
                            }
                        }
                        1 => {
                            if let Some(rid) = self.face_trader_random_face() {
                                self.gain_relic(rid);
                            }
                        }
                        _ => {}
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Masked Bandits" {
            match screen {
                0 if *index == 0 => {
                    self.player.gold = 0;
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Continue]".into()];
                    }
                }
                0 => {
                    // MaskedBandits.buttonEffect: event gold/relic are seeded before
                    // enterCombat. Starting first preserves that miscRng roll because
                    // Combat::start resets the already-entered event floor streams.
                    self.rewards.clear();
                    self.start_combat_encounter(EncounterId::MaskedBandits);
                    let gold = self.rng.misc.random_range(25, 35);
                    self.add_gold_to_rewards(gold);
                    if self.player.has_relic(RelicId::Red_Mask) {
                        if let Some(id) = RelicId::from_sts_id("Circlet") {
                            self.add_relic_to_rewards(id);
                        }
                    } else {
                        self.add_relic_to_rewards(RelicId::Red_Mask);
                    }
                }
                1 | 2 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen += 1;
                        event.options = vec!["[Continue]".into()];
                    }
                }
                3 => self.open_map(),
                _ => self.open_map(),
            }
            return;
        }
        if id == "Addict" {
            if screen == 0 {
                let can_pay = self
                    .event
                    .as_ref()
                    .and_then(|e| e.data.first().copied())
                    .unwrap_or(0)
                    != 0;
                let rob = if can_pay { 1 } else { 0 };
                let leave = if can_pay { 2 } else { 1 };
                if can_pay && *index == 0 {
                    if let Some(relic) = self.next_screenless_relic() {
                        self.player.gold -= 85;
                        self.gain_relic(relic);
                    }
                } else if *index == rob {
                    let relic = self.next_screenless_relic();
                    self.obtain_master_deck_card(CardId::Shame);
                    if let Some(relic) = relic {
                        self.gain_relic(relic);
                    }
                } else if *index == leave {
                    self.open_map();
                    return;
                }
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Leave]".into()];
                }
            } else {
                self.open_map();
            }
            return;
        }
        if id == "Nest" {
            match screen {
                0 => {
                    let gold = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.first().copied())
                        .unwrap_or(if self.ascension >= 15 { 50 } else { 99 });
                    let damage = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(1).copied())
                        .unwrap_or(6);
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            format!("[Smash and Grab] #gGain #g{gold} #gGold."),
                            format!(
                                "[Stay in Line] #rLose #r{damage} #rHP. #gObtain #gRitual #gDagger."
                            ),
                        ];
                    }
                }
                1 => {
                    if *index == 0 {
                        if !self.player.has_relic(RelicId::Ectoplasm) {
                            let gold = self
                                .event
                                .as_ref()
                                .and_then(|e| e.data.first().copied())
                                .unwrap_or(if self.ascension >= 15 { 50 } else { 99 });
                            self.player.gold += gold;
                        }
                    } else {
                        // Nest uses player.damage(DamageInfo(null, 6)): owner-null
                        // damage skips attacker hooks, then Tungsten Rod applies.
                        let damage = self
                            .event
                            .as_ref()
                            .and_then(|e| e.data.get(1).copied())
                            .unwrap_or(6);
                        let damage = combat::on_lose_hp_last(&self.player, damage);
                        self.player.hp = (self.player.hp - damage).max(0);
                        combat::red_skull_on_hp_change(&mut self.player);
                        self.obtain_master_deck_card(CardId::RitualDagger);
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Knowing Skull" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            "[Drink] #rLose #r6 #rHP. #gObtain #ga #gPotion.".into(),
                            "[Gold] #gGain #g90 #gGold. #rLose #r6 #rHP.".into(),
                            "[Knowledge] #rLose #r6 #rHP. #gObtain #ga #gColorless #gCard."
                                .into(),
                            "[Leave] #rLose #r6 #rHP.".into(),
                        ];
                    }
                }
                1 if *index == 3 => {
                    // KnowingSkull's leave branch uses owner-null HP_LOSS, so
                    // Tungsten Rod's onLoseHpLast can reduce it.
                    let damage = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(3).copied())
                        .unwrap_or(6);
                    let damage = combat::on_lose_hp_last(&self.player, damage);
                    self.player.hp = (self.player.hp - damage).max(0);
                    combat::red_skull_on_hp_change(&mut self.player);
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                1 => {
                    // Reward choices need their own walk witnesses: keep them
                    // staged in ASK instead of pretending the event completed.
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "The Joust" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            "[Murderer] #yBet #y50 #yGold - #g70%: #gwin #g100 #gGold."
                                .into(),
                            "[Owner] #yBet #y50 #yGold - #g30%: #gwin #g250 #gGold."
                                .into(),
                        ];
                    }
                }
                1 => {
                    self.player.gold = (self.player.gold - 50).max(0);
                    if let Some(event) = self.event.as_mut() {
                        event.data[0] = i32::from(*index == 1);
                        event.screen = 2;
                        event.options = vec!["[Watch]".into()];
                    }
                }
                2 => {
                    let owner_wins = self.rng.misc.random_boolean_chance(0.3);
                    if let Some(event) = self.event.as_mut() {
                        event.data[1] = i32::from(owner_wins);
                        event.screen = 3;
                        event.options = vec!["[Watch]".into()];
                    }
                }
                3 => {
                    let (bet_for, owner_wins) = self
                        .event
                        .as_ref()
                        .map(|e| (e.data[0] != 0, e.data[1] != 0))
                        .unwrap_or((false, false));
                    let payout = if bet_for == owner_wins {
                        if owner_wins { 250 } else { 100 }
                    } else {
                        0
                    };
                    if payout > 0 && !self.player.has_relic(RelicId::Ectoplasm) {
                        // AbstractPlayer.gainGold calls each relic's onGainGold.
                        self.player.gold += payout;
                        if self.player.has_relic(RelicId::Bloody_Idol) {
                            self.player.hp = (self.player.hp + 5).min(self.player.max_hp);
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 4;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Winding Halls" {
            match screen {
                0 => {
                    let hp_loss = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.first().copied())
                        .unwrap_or(0);
                    let heal = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(1).copied())
                        .unwrap_or(0);
                    let max_hp_loss = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(2).copied())
                        .unwrap_or(0);
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            format!(
                                "[Embrace Madness] #gReceive #g2 Madness. #rLose #r{hp_loss} #rHP."
                            ),
                            format!(
                                "[Focus] #rBecome #rCursed #r- #rWrithe. #gHeal #g{heal} #gHP."
                            ),
                            format!(
                                "[Retrace Your Steps] #rLose #r{max_hp_loss} #rMax #rHP."
                            ),
                        ];
                    }
                }
                1 => {
                    let data = self
                        .event
                        .as_ref()
                        .map(|e| e.data.clone())
                        .unwrap_or_default();
                    match *index {
                        0 => {
                            let damage = combat::on_lose_hp_last(
                                &self.player,
                                data.first().copied().unwrap_or(0),
                            );
                            self.player.hp = (self.player.hp - damage).max(0);
                            combat::red_skull_on_hp_change(&mut self.player);
                            self.obtain_master_deck_card(CardId::Madness);
                            self.obtain_master_deck_card(CardId::Madness);
                        }
                        1 => {
                            self.heal_player(data.get(1).copied().unwrap_or(0));
                            self.obtain_master_deck_card(CardId::Writhe);
                        }
                        _ => {
                            let loss = data.get(2).copied().unwrap_or(0);
                            self.player.max_hp = (self.player.max_hp - loss).max(1);
                            self.player.hp = self.player.hp.min(self.player.max_hp);
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if label.as_deref().is_some_and(|l| l.contains("[Leave]") || l == "Leave") {
            // Tomb INTRO Leave opens the map; Mausoleum/Scrap Ooze-style Leave goes to RESULT first.
            if id == "Tomb of Lord Red Mask" || screen > 0 || option_count == 1 {
                self.open_map();
                return;
            }
            if let Some(event) = self.event.as_mut() {
                event.screen = 1;
                event.options = vec!["[Leave]".into()];
            }
            return;
        }
        if id == "Scrap Ooze" {
            // ScrapOoze.buttonEffect: damage then miscRng.random(0,99) >= 99-chance.
            if screen > 0 {
                self.open_map();
                return;
            }
            if *index == 0 {
                let dmg = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(3);
                let chance = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(25);
                let dmg = combat::on_lose_hp_last(&self.player, dmg);
                self.player.hp = (self.player.hp - dmg).max(1);
                let roll = self.rng.misc.random_range(0, 99);
                if roll >= 99 - chance {
                    if let Some(rid) = self.next_screenless_relic() {
                        self.gain_relic(rid);
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                } else if let Some(event) = self.event.as_mut() {
                    let dmg = dmg + 1;
                    let chance = chance + 10;
                    event.data = vec![dmg, chance];
                    event.options = vec![
                        format!("[Deeper] #rLose #r{dmg} #rHP. #g{chance}%: #gFind #ga #gRelic."),
                        "[Leave]".into(),
                    ];
                }
            } else if let Some(event) = self.event.as_mut() {
                event.screen = 1;
                event.options = vec!["[Leave]".into()];
            }
            return;
        }
        if id == "Drug Dealer" {
            if screen == 0 {
                let chosen = self
                    .event
                    .as_ref()
                    .and_then(|e| e.options.get(*index))
                    .cloned()
                    .unwrap_or_default();
                if chosen.contains("Ingest") || chosen.contains("J.A.X") {
                    self.obtain_master_deck_card(CardId::J_A_X_);
                } else if chosen.contains("Study") || chosen.contains("Transform") {
                    if self.player.deck.iter().filter(|c| purgeable_card(c)).count() >= 2 {
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                        self.open_grid(GridKind::Transform, 2, true);
                        return;
                    }
                } else if chosen.contains("Inject") || chosen.contains("Mutagen") {
                    if self.player.has_relic(RelicId::MutagenicStrength) {
                        if let Some(id) = RelicId::from_sts_id("Circlet") {
                            self.gain_relic(id);
                        }
                    } else {
                        self.gain_relic(RelicId::MutagenicStrength);
                    }
                }
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Leave]".into()];
                }
            } else {
                self.open_map();
            }
            return;
        }
        if id == "Designer" {
            match screen {
                0 => {
                    let opts = self.designer_main_options();
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = opts;
                    }
                }
                1 => {
                    let chosen = self
                        .event
                        .as_ref()
                        .and_then(|e| e.options.get(*index))
                        .cloned()
                        .unwrap_or_default();
                    let adj = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(40);
                    let clean = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(60);
                    let full = self.event.as_ref().and_then(|e| e.data.get(2).copied()).unwrap_or(90);
                    let hp = self.event.as_ref().and_then(|e| e.data.get(3).copied()).unwrap_or(3);
                    let upg_one = self.event.as_ref().and_then(|e| e.data.get(4).copied()).unwrap_or(0) != 0;
                    let rem_cards = self.event.as_ref().and_then(|e| e.data.get(5).copied()).unwrap_or(0) != 0;
                    if chosen.contains("Adjust") {
                        self.player.gold = (self.player.gold - adj).max(0);
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 2;
                            event.options = vec!["[Leave]".into()];
                        }
                        if upg_one {
                            self.open_grid(GridKind::Upgrade, 1, true);
                            return;
                        }
                        let seed = self.rng.misc.random_long();
                        let mut idxs: Vec<usize> = self
                            .player
                            .deck
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.can_upgrade())
                            .map(|(i, _)| i)
                            .collect();
                        shuffle_java(&mut idxs, seed);
                        for &i in idxs.iter().take(2) {
                            if let Some(c) = self.player.deck.get_mut(i) {
                                c.upgrade();
                            }
                        }
                    } else if chosen.contains("Clean") {
                        self.player.gold = (self.player.gold - clean).max(0);
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 2;
                            event.options = vec!["[Leave]".into()];
                        }
                        if rem_cards {
                            self.open_grid(GridKind::Purge, 1, true);
                        } else {
                            self.open_grid(GridKind::Transform, 2, true);
                        }
                        return;
                    } else if chosen.contains("Full") {
                        self.player.gold = (self.player.gold - full).max(0);
                        if let Some(event) = self.event.as_mut() {
                            if event.data.len() < 7 {
                                event.data.resize(7, 0);
                            }
                            event.data[6] = 1;
                            event.screen = 2;
                            event.options = vec!["[Leave]".into()];
                        }
                        self.open_grid(GridKind::Purge, 1, true);
                        return;
                    } else if chosen.contains("Punch") {
                        // DamageInfo(null, hpLoss, HP_LOSS): TungstenRod applies.
                        let dmg = combat::on_lose_hp_last(&self.player, hp);
                        self.player.hp = (self.player.hp - dmg).max(0);
                        combat::red_skull_on_hp_change(&mut self.player);
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 2;
                            event.options = vec!["[Leave]".into()];
                        }
                    } else if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Forgotten Altar" {
            if screen == 0 {
                let has_idol = self.player.has_relic(RelicId::Golden_Idol);
                let sacrifice = if has_idol { 1 } else { 0 };
                let smash = if has_idol { 2 } else { 1 };
                if has_idol && *index == 0 {
                    // gainChalice: replace Golden Idol in-slot; Circlet if
                    // Bloody Idol is already owned. instantObtain callOnEquip=false.
                    if self.player.has_relic(RelicId::Bloody_Idol) {
                        if let Some(id) = RelicId::from_sts_id("Circlet") {
                            self.gain_relic(id);
                        }
                    } else if let Some(i) = self
                        .player
                        .relics
                        .iter()
                        .position(|r| r.id == RelicId::Golden_Idol)
                    {
                        self.player.relics[i] = RelicInstance {
                            id: RelicId::Bloody_Idol,
                            counter: -1,
                            used_up: false,
                        };
                    }
                } else if *index == sacrifice {
                    let hp_loss = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.first().copied())
                        .unwrap_or_else(|| {
                            crate::rewards::gdx_round(self.player.max_hp as f32 * 0.25)
                        });
                    self.increase_max_hp(5);
                    // DamageInfo(null, hpLoss): owner is null so Torii/powers
                    // skip; TungstenRod onLoseHpLast still applies.
                    let dmg = combat::on_lose_hp_last(&self.player, hp_loss);
                    self.player.hp = (self.player.hp - dmg).max(0);
                    combat::red_skull_on_hp_change(&mut self.player);
                } else if *index == smash {
                    self.obtain_master_deck_card(CardId::Decay);
                }
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Leave]".into()];
                }
            } else {
                self.open_map();
            }
            return;
        }
        if id == "Golden Shrine" {
            if screen == 0 {
                match *index {
                    0 => {
                        if !self.player.has_relic(RelicId::Ectoplasm) {
                            // GoldShrine: A15+ 50 else 100. Golden Idol is combat/chest only.
                            self.player.gold += if self.ascension >= 15 { 50 } else { 100 };
                        }
                    }
                    1 => {
                        if !self.player.has_relic(RelicId::Ectoplasm) {
                            self.player.gold += 275;
                        }
                        self.obtain_master_deck_card(CardId::Regret);
                    }
                    _ => {}
                }
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Leave]".into()];
                }
            } else {
                self.open_map();
            }
            return;
        }
        if id == "World of Goop" {
            if screen == 0 {
                match *index {
                    0 => {
                        let dmg = combat::on_lose_hp_last(&self.player, 11);
                        self.player.hp = (self.player.hp - dmg).max(1);
                        self.player.gold += 75;
                    }
                    _ => {
                        let loss = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(0);
                        self.player.gold = (self.player.gold - loss).max(0);
                    }
                }
                if let Some(event) = self.event.as_mut() {
                    event.screen = 1;
                    event.options = vec!["[Leave]".into()];
                }
            } else {
                self.open_map();
            }
            return;
        }
        if id == "Golden Idol" {
            match screen {
                0 => {
                    if *index == 0 {
                        self.gain_relic(RelicId::Golden_Idol);
                        let (dmg_pct, max_pct) = if self.ascension >= 15 {
                            (0.35, 0.1)
                        } else {
                            (0.25, 0.08)
                        };
                        let dmg = (self.player.max_hp as f32 * dmg_pct) as i32;
                        let mut max_loss = (self.player.max_hp as f32 * max_pct) as i32;
                        if max_loss < 1 {
                            max_loss = 1;
                        }
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.data = vec![dmg, max_loss];
                            event.options = vec![
                                "[Outrun] #rBecome #rCursed #r- #rInjury.".into(),
                                format!("[Smash] #rTake #r{dmg} #rDamage."),
                                format!("[Hide] #rLose #r{max_loss} #rMax #rHP."),
                            ];
                        }
                    } else if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                1 => {
                    let dmg = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(0);
                    let max_loss = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(1);
                    match *index {
                        0 => self.player.deck.push(Card::new(CardId::Injury)),
                        1 => {
                            // player.damage(DamageInfo) can kill (230296: 20-24 → 0).
                            let dmg = combat::on_lose_hp_last(&self.player, dmg);
                            self.player.hp = (self.player.hp - dmg).max(0);
                            if self.player.hp <= 0 {
                                self.screen = Screen::Terminal;
                                self.done = true;
                            }
                        }
                        _ => {
                            self.player.max_hp = (self.player.max_hp - max_loss).max(1);
                            if self.player.hp > self.player.max_hp {
                                self.player.hp = self.player.max_hp;
                            }
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Big Fish" {
            match screen {
                0 => {
                    match *index {
                        0 => {
                            let heal = self
                                .event
                                .as_ref()
                                .and_then(|e| e.data.first().copied())
                                .unwrap_or(self.player.max_hp / 3);
                            self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                        }
                        1 => {
                            self.player.max_hp += 5;
                            self.player.hp = (self.player.hp + 5).min(self.player.max_hp);
                        }
                        _ => {
                            self.player.deck.push(Card::new(CardId::Regret));
                            if let Some(id) = self.next_screenless_relic() {
                                self.gain_relic(id);
                            }
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "The Cleric" {
            match screen {
                0 => {
                    let chosen = self
                        .event
                        .as_ref()
                        .and_then(|e| e.options.get(*index))
                        .cloned()
                        .unwrap_or_default();
                    if chosen.contains("Heal") {
                        let heal = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(0);
                        self.player.gold -= 35;
                        self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                    } else if chosen.contains("Purify") {
                        let cost = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(50);
                        self.player.gold -= cost;
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                        self.open_grid(GridKind::Purge, 1, true);
                    } else if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Living Wall" {
            match screen {
                0 => {
                    let chosen = self
                        .event
                        .as_ref()
                        .and_then(|e| e.options.get(*index))
                        .cloned()
                        .unwrap_or_default();
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                    if chosen.contains("Forget") {
                        self.open_grid(GridKind::Purge, 1, true);
                    } else if chosen.contains("Change") {
                        self.open_grid(GridKind::Transform, 1, true);
                    } else if chosen.contains("Grow") {
                        self.open_grid(GridKind::Upgrade, 1, true);
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Back to Basics" {
            match screen {
                0 => {
                    if *index == 0 {
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                        if self.player.deck.iter().any(|c| purgeable_card(c) && !c.in_bottle) {
                            self.open_grid(GridKind::Purge, 1, true);
                            if let Some(g) = self.grid.as_mut() {
                                g.immediate = true;
                            }
                        }
                    } else {
                        for c in &mut self.player.deck {
                            if c.id.has_starter_strike_or_defend_tag() && c.can_upgrade() {
                                c.upgrade();
                            }
                        }
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Shining Light" {
            match screen {
                0 => {
                    let enter = self
                        .event
                        .as_ref()
                        .and_then(|e| e.options.get(*index))
                        .is_some_and(|s| s.contains("Enter") || s.contains("Upgrade"));
                    if enter {
                        let damage = self
                            .event
                            .as_ref()
                            .and_then(|e| e.data.first().copied())
                            .unwrap_or(0);
                        let damage = combat::on_lose_hp_last(&self.player, damage);
                        self.player.hp = (self.player.hp - damage).max(0);
                        let seed = self.rng.misc.random_long();
                        let mut idxs: Vec<usize> = self
                            .player
                            .deck
                            .iter()
                            .enumerate()
                            .filter(|(_, c)| c.can_upgrade())
                            .map(|(i, _)| i)
                            .collect();
                        shuffle_java(&mut idxs, seed);
                        for &i in idxs.iter().take(2) {
                            if let Some(c) = self.player.deck.get_mut(i) {
                                c.upgrade();
                            }
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "MindBloom" {
            let war = *index == 0
                || label.as_deref().is_some_and(|l| l.contains("War") || l.contains("Fight"));
            if war {
                let seed = self.rng.misc.random_long();
                let mut bosses = [
                    EncounterId::TheGuardian,
                    EncounterId::Hexaghost,
                    EncounterId::SlimeBoss,
                ];
                shuffle_java(&mut bosses, seed);
                // MindBloom.buttonEffect INTRO/0: rewards.clear, addGoldToRewards
                // (A13+ 25 else 50), addRelicToRewards(RARE), then enterCombatFromImage.
                self.rewards.clear();
                let gold = if self.ascension >= 13 { 25 } else { 50 };
                self.add_gold_to_rewards(gold);
                if let Some(id) = self.take_relic(RelicTier::RARE) {
                    self.add_relic_to_rewards(id);
                }
                self.start_combat_encounter(bosses[0]);
            }
            return;
        }
        if id == "SensoryStone" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            "[Recall] 1".into(),
                            "[Recall] 2".into(),
                            "[Recall] 3".into(),
                        ];
                    }
                }
                1 => {
                    // getRandomMemory: Collections.shuffle(new Random(miscRng.randomLong())).
                    let _ = self.rng.misc.random_long();
                    let n = (*index as i32 + 1).clamp(1, 3);
                    if *index == 1 {
                        let dmg = combat::on_lose_hp_last(&self.player, 5);
                        self.player.hp = (self.player.hp - dmg).max(0);
                    } else if *index == 2 {
                        let dmg = combat::on_lose_hp_last(&self.player, 10);
                        self.player.hp = (self.player.hp - dmg).max(0);
                    }
                    self.rewards.clear();
                    self.card_reward = crate::rewards::colorless_reward_cards(
                        &self.dungeon,
                        &mut self.rng,
                        &mut self.card_blizz,
                        3,
                        0.3,
                    );
                    self.rewards.push(Reward::new(RewardKind::Card));
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                    let _ = n;
                    self.screen = Screen::CombatReward;
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Falling" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec![
                            "[Land]".into(),
                            "[Channel]".into(),
                            "[Strike]".into(),
                        ];
                    }
                }
                1 => {
                    let idx = self
                        .event
                        .as_ref()
                        .and_then(|e| e.data.get(*index).copied())
                        .unwrap_or(-1);
                    if idx >= 0 {
                        let idx = idx as usize;
                        if idx < self.player.deck.len() {
                            self.remove_master_deck_card(idx);
                        }
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Mushrooms" {
            match screen {
                0 if *index == 0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 2;
                        event.options = vec!["[Fight]".into()];
                    }
                }
                0 => {
                    let heal = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(0);
                    self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                    self.player.deck.push(Card::new(CardId::Parasite));
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Leave]".into()];
                    }
                }
                2 => {
                    // Mushrooms.buttonEffect screen 2: addGoldToRewards(miscRng.random(20,30))
                    // then addRelicToRewards(OddMushroom) before enterCombat. EventRoom
                    // combat does not replace these with hallway gold.
                    let gold = self.rng.misc.random_range(20, 30);
                    self.rewards.clear();
                    self.add_gold_to_rewards(gold);
                    self.add_relic_to_rewards(RelicId::Odd_Mushroom);
                    self.start_combat_encounter(EncounterId::MushroomLair);
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Lab" {
            // Lab.buttonEffect INTRO: noCardsInRewards, 2 potions (3 below A15),
            // then CombatRewardScreen.open(). COMPLETE is the reward Proceed.
            match screen {
                0 => {
                    self.rewards.clear();
                    self.card_reward.clear();
                    let n = if self.ascension < 15 { 3 } else { 2 };
                    for _ in 0..n {
                        let p = crate::rewards::get_random_potion_for(&mut self.rng, self.character);
                        self.rewards.push(Reward::new(RewardKind::Potion(p)));
                    }
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                    }
                    self.screen = Screen::CombatReward;
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Woman in Blue" || id == "The Woman in Blue" {
            // WomanInBlue.buttonEffect INTRO: lose gold, add N potion RewardItems,
            // CombatRewardScreen.open(). RESULT Leave after Proceed.
            match screen {
                0 => {
                    if *index < 3 {
                        let cost = 20 + *index as i32 * 10;
                        if self.player.gold >= cost {
                            self.player.gold -= cost;
                        }
                        self.rewards.clear();
                        self.card_reward.clear();
                        for _ in 0..=*index {
                            let p = crate::rewards::get_random_potion_for(&mut self.rng, self.character);
                            self.rewards.push(Reward::new(RewardKind::Potion(p)));
                        }
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                        self.screen = Screen::CombatReward;
                    } else {
                        if self.ascension >= 15 {
                            let dmg = ((self.player.max_hp as f32 * 0.05).ceil() as i32).max(1);
                            let dmg = combat::on_lose_hp_last(&self.player, dmg);
                            self.player.hp = (self.player.hp - dmg).max(1);
                        }
                        if let Some(event) = self.event.as_mut() {
                            event.screen = 1;
                            event.options = vec!["[Leave]".into()];
                        }
                    }
                }
                _ => self.open_map(),
            }
            return;
        }
        if id == "Bonfire Elementals" {
            match screen {
                0 => {
                    if let Some(event) = self.event.as_mut() {
                        event.screen = 1;
                        event.options = vec!["[Offer] Receive a reward based on the offer.".into()];
                    }
                }
                1 => {
                    self.grid = Some(GridSelect {
                        kind: GridKind::Purge,
                        needed: 1,
                        confirm: false,
                        hovered: None,
                        picked: Vec::new(),
                        return_event: true,
                        return_shop: false,
                        return_screen: None,
                        immediate: false,
                    });
                    self.screen = Screen::Grid;
                }
                _ => self.open_map(),
            }
            return;
        }
        if screen > 0 || option_count == 1 {
            self.open_map();
            return;
        }
        match id.as_str() {
            "The Library" => {
                if *index == 1 || matches!(action, Action::Choose { label: Some(l), .. } if l.contains("Sleep"))
                {
                    let heal = (self.player.max_hp as f32 * 0.33 + 0.5).floor() as i32;
                    self.player.hp = (self.player.hp + heal).min(self.player.max_hp);
                } else {
                    self.open_library_grid();
                    return;
                }
            }
            "Ghosts" => {}
            "WeMeetAgain" => {
                let gold_amt = self.event.as_ref().and_then(|e| e.data.first().copied()).unwrap_or(0);
                let card_idx = self.event.as_ref().and_then(|e| e.data.get(1).copied()).unwrap_or(-1);
                let potion_slot = self.event.as_ref().and_then(|e| e.data.get(2).copied()).unwrap_or(-1);
                let chosen = self
                    .event
                    .as_ref()
                    .and_then(|e| e.options.get(*index))
                    .cloned()
                    .unwrap_or_default();
                if chosen.contains("Potion") {
                    if potion_slot >= 0 {
                        if let Some(p) = self.player.potions.iter_mut().find(|p| p.slot == potion_slot) {
                            p.id = PotionId::Slot;
                        }
                    }
                    if let Some(id) = self.next_screenless_relic() {
                        self.gain_relic(id);
                    }
                } else if chosen.contains("Gold") {
                    self.player.gold -= gold_amt;
                    if let Some(id) = self.next_screenless_relic() {
                        self.gain_relic(id);
                    }
                } else if chosen.contains("Card") {
                    if card_idx >= 0 {
                        let idx = card_idx as usize;
                        if idx < self.player.deck.len() {
                            self.remove_master_deck_card(idx);
                        }
                    }
                    if let Some(id) = self.next_screenless_relic() {
                        self.gain_relic(id);
                    }
                }
            }
            _ => {}
        }
        if let Some(event) = self.event.as_mut() {
            event.screen = 1;
            event.options = vec!["[Leave]".into()];
        }
    }

    fn begin_discard_to_hand_select(&mut self) {
        if self.player.discard.len() <= 1 {
            while !self.player.discard.is_empty() && self.player.hand.len() < 10 {
                let c = self.player.discard.remove(0);
                self.player.hand.push(c);
            }
            self.finish_discard_to_hand();
            return;
        }
        self.grid = Some(GridSelect {
            kind: GridKind::DiscardToHand,
            needed: 1,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: false,
            return_screen: None,
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    fn finish_discard_to_hand(&mut self) {
        if let Some(combat) = self.combat.as_mut() {
            crate::combat::flush_seek_reactions(&mut self.player, combat, &mut self.rng);
            crate::combat::flush_letter_opener(combat, &mut self.rng);
            if let Some(card) = combat.pending_exhaust.take() {
                if card.exhaust {
                    crate::combat::exhaust_card(&mut self.player, combat, card, &mut self.rng);
                } else if card.card_type() != crate::ids::CardType::POWER {
                    self.player.discard.push(card);
                }
            }
            crate::combat::flush_dark_embrace(&mut self.player, combat, &mut self.rng);
            combat.need_discard_to_hand = false;
            combat.need_draw_to_hand = false;
            combat.need_skill_from_deck = false;
            combat.skill_from_deck.clear();
            if combat.all_dead() {
                self.finish_combat();
                return;
            }
        }
        self.screen = Screen::Combat;
    }

    fn begin_skill_from_deck_select(&mut self) {
        let n = self
            .combat
            .as_ref()
            .map(|c| c.skill_from_deck.len())
            .unwrap_or(0);
        if n <= 1 {
            if n == 1 {
                let i = self.combat.as_ref().unwrap().skill_from_deck[0];
                combat::draw_pile_to_hand(&mut self.player, i);
            }
            self.finish_discard_to_hand();
            return;
        }
        self.grid = Some(GridSelect {
            kind: GridKind::SkillFromDeck,
            needed: 1,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: false,
            return_screen: None,
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    fn begin_draw_to_hand_select(&mut self) {
        let needed = self
            .combat
            .as_ref()
            .and_then(|c| c.pending_exhaust.as_ref())
            .map(|c| c.base_magic.max(1) as usize)
            .unwrap_or(1);
        if self.player.draw.len() <= needed {
            while !self.player.draw.is_empty() && self.player.hand.len() < 10 {
                let c = self.player.draw.pop().unwrap();
                self.player.hand.push(c);
            }
            self.finish_discard_to_hand();
            return;
        }
        self.grid = Some(GridSelect {
            kind: GridKind::DrawPileToHand,
            needed,
            confirm: false,
            hovered: None,
            picked: Vec::new(),
            return_event: false,
            return_shop: false,
            return_screen: None,
            immediate: false,
        });
        self.screen = Screen::Grid;
    }

    fn begin_put_on_deck_select(&mut self) {
        self.put_on_deck_select = true;
        self.exhaust_select = false;
        self.hand_held.clear();
        if self.player.hand.len() <= 1 {
            if let Some(c) = self.player.hand.pop() {
                self.put_card_from_forethought_or_top(c);
            }
            self.finish_put_on_deck();
        } else {
            self.screen = Screen::HandSelect;
        }
    }

    fn put_card_from_forethought_or_top(&mut self, mut card: crate::card::Card) {
        let fore = self
            .combat
            .as_ref()
            .is_some_and(|c| c.need_forethought);
        if fore {
            if card.cost > 0 {
                card.free_to_play_once = true;
            }
            self.player.draw.insert(0, card);
        } else {
            self.player.draw.push(card);
        }
    }

    fn finish_exhaust_draw(&mut self) {
        let n = self
            .combat
            .as_ref()
            .map(|c| c.draw_after_exhaust)
            .unwrap_or(0);
        if n <= 0 {
            return;
        }
        if let Some(combat) = self.combat.as_mut() {
            combat.draw_after_exhaust = 0;
            let drawn = crate::combat::draw_cards_rng(&mut self.player, n, Some(&mut self.rng));
            crate::combat::apply_fire_breathing(
                &self.player,
                &mut combat.monsters,
                &mut self.rng,
                drawn,
            );
        }
    }

    fn finish_put_on_deck(&mut self) {
        if let Some(combat) = self.combat.as_mut() {
            if let Some(card) = combat.pending_exhaust.take() {
                crate::combat::exhaust_card(&mut self.player, combat, card, &mut self.rng);
            }
            crate::combat::flush_dark_embrace(&mut self.player, combat, &mut self.rng);
            combat.need_put_on_deck = false;
            combat.need_forethought = false;
        }
        self.put_on_deck_select = false;
        self.screen = Screen::Combat;
    }

    fn begin_exhaust_select(&mut self) {
        self.exhaust_select = true;
        self.hand_held.clear();
        if self.player.hand.len() <= 1 {
            if let Some(c) = self.player.hand.pop() {
                if let Some(combat) = self.combat.as_mut() {
                    crate::combat::exhaust_card(&mut self.player, combat, c, &mut self.rng);
                } else {
                    self.player.exhaust.push(c);
                }
            }
            self.finish_exhaust_draw();
            if let Some(combat) = self.combat.as_mut() {
                crate::combat::flush_dark_embrace(&mut self.player, combat, &mut self.rng);
            }
            self.exhaust_select = false;
            self.screen = Screen::Combat;
        } else {
            self.screen = Screen::HandSelect;
        }
    }

    fn begin_armaments_select(&mut self) {
        self.exhaust_select = false;
        self.hand_held.clear();
        let upgradeable: Vec<usize> = self
            .player
            .hand
            .iter()
            .enumerate()
            .filter(|(_, c)| c.can_upgrade())
            .map(|(i, _)| i)
            .collect();
        if upgradeable.len() <= 1 {
            if let Some(&i) = upgradeable.first() {
                self.player.hand[i].upgrade();
            }
            self.screen = Screen::Combat;
        } else {
            let mut i = 0;
            while i < self.player.hand.len() {
                if !self.player.hand[i].can_upgrade() {
                    self.hand_held.push(self.player.hand.remove(i));
                } else {
                    i += 1;
                }
            }
            self.screen = Screen::HandSelect;
        }
    }

    fn step_hand_select(&mut self, action: &Action) {
        match action {
            Action::Choose { index, label, .. } => {
                let by_name = label.as_ref().and_then(|name| {
                    self.player
                        .hand
                        .iter()
                        .position(|c| c.sts_id() == name.as_str() || c.id.sts_id() == name.as_str())
                });
                let idx = by_name.unwrap_or(*index);
                if idx < self.player.hand.len() {
                    let mut card = self.player.hand.remove(idx);
                    if !self.exhaust_select && !self.put_on_deck_select && !self.gambling_select {
                        card.upgrade();
                    }
                    self.pending_cards.push(card);
                }
            }
            Action::Proceed => {
                if self.gambling_select {
                    let n = self.pending_cards.len() as i32;
                    self.player.discard.append(&mut self.pending_cards);
                    if let Some(combat) = self.combat.as_mut() {
                        let drawn = crate::combat::draw_cards_rng(&mut self.player, n, Some(&mut self.rng));
                        crate::combat::apply_fire_breathing(
                            &self.player,
                            &mut combat.monsters,
                            &mut self.rng,
                            drawn,
                        );
                    }
                    self.gambling_select = false;
                    self.screen = Screen::Combat;
                    self.apply_pending_ornithopter_heal();
                    return;
                }
                if self.put_on_deck_select {
                    let pending = std::mem::take(&mut self.pending_cards);
                    for card in pending {
                        self.put_card_from_forethought_or_top(card);
                    }
                    self.player.hand.append(&mut self.hand_held);
                    self.finish_put_on_deck();
                    return;
                }
                if self.exhaust_select {
                    let pending = std::mem::take(&mut self.pending_cards);
                    if let Some(combat) = self.combat.as_mut() {
                        for card in pending {
                            crate::combat::exhaust_card(&mut self.player, combat, card, &mut self.rng);
                        }
                    } else {
                        self.player.exhaust.extend(pending);
                    }
                    self.player.hand.append(&mut self.hand_held);
                    self.finish_exhaust_draw();
                    if let Some(combat) = self.combat.as_mut() {
                        crate::combat::flush_dark_embrace(&mut self.player, combat, &mut self.rng);
                    }
                    self.exhaust_select = false;
                } else {
                    self.player.hand.append(&mut self.pending_cards);
                    self.player.hand.append(&mut self.hand_held);
                }
                self.screen = Screen::Combat;
            }
            _ => {}
        }
    }

    fn gain_relic(&mut self, id: RelicId) {
        if id == RelicId::Busted_Crown
            || id == RelicId::Cursed_Key
            || id == RelicId::Coffee_Dripper
            || id == RelicId::Ectoplasm
            || id == RelicId::Fusion_Hammer
            || id == RelicId::Philosophers_Stone
            || id == RelicId::Velvet_Choker
        {
            self.player.energy_master += 1;
        }
        if id == RelicId::Old_Coin {
            // OldCoin.onEquip: player.gainGold(300). Ectoplasm skips gainGold.
            if !self.player.has_relic(RelicId::Ectoplasm) {
                self.player.gold += 300;
            }
        }
        if id == RelicId::Potion_Belt {
            // PotionBelt.onEquip: potionSlots += 2 and two empty PotionSlot entries.
            self.player.potion_slots += 2;
            let start = self.player.potions.len() as i32;
            self.player.potions.push(crate::creature::PotionInstance {
                id: crate::ids::PotionId::Slot,
                slot: start,
            });
            self.player.potions.push(crate::creature::PotionInstance {
                id: crate::ids::PotionId::Slot,
                slot: start + 1,
            });
        }
        // Fruit relics call increaseMaxHp(N, true): maxHealth += N, then heal(N).
        // Lee's Waffle also heals to full after the +7.
        match id {
            RelicId::Strawberry => self.increase_max_hp(7),
            RelicId::Pear => self.increase_max_hp(10),
            RelicId::Mango => self.increase_max_hp(14),
            RelicId::Lees_Waffle => {
                self.increase_max_hp(7);
                self.heal_player(self.player.max_hp);
            }
            _ => {}
        }
        let inst = RelicInstance {
            id,
            counter: match id {
                RelicId::Happy_Flower
                | RelicId::Pen_Nib
                | RelicId::InkBottle
                | RelicId::Nunchaku
                | RelicId::Incense_Burner
                | RelicId::Sundial
                | RelicId::Inserter => 0,
                RelicId::Matryoshka | RelicId::Omamori => 2,
                RelicId::NlothsMask => 1,
                _ => -1,
            },
            used_up: false,
        };
        // BossRelicSelectScreen: Black Blood / FrozenCore / Ring of the Serpent /
        // HolyWater instantObtain at slot 0, replacing the starter relic.
        if matches!(id, RelicId::FrozenCore | RelicId::Black_Blood) && !self.player.relics.is_empty() {
            self.player.relics[0] = inst;
        } else {
            self.player.relics.push(inst);
        }
        // Whetstone/War Paint onEquip shuffles with miscRng.randomLong() at
        // instantObtain, not at the next Map open (seed 1 Rip and Tear+).
        if id == RelicId::Whetstone {
            self.upgrade_random_cards(crate::ids::CardType::ATTACK, 2);
        } else if id == RelicId::War_Paint {
            self.upgrade_random_cards(crate::ids::CardType::SKILL, 2);
        }
        // Bottled*.onEquip: GRID of purgeable cards of that type. ChoiceDriver
        // closes immediately (not upgrade/transform/purge). The picked card
        // is flagged in_bottle and treated as innate at combat start.
        match id {
            RelicId::Bottled_Flame => self.open_bottle_grid(CardType::ATTACK),
            RelicId::Bottled_Lightning => self.open_bottle_grid(CardType::SKILL),
            RelicId::Bottled_Tornado => self.open_bottle_grid(CardType::POWER),
            RelicId::DollysMirror => self.open_dollys_mirror_grid(),
            RelicId::Empty_Cage => self.open_empty_cage_grid(),
            RelicId::Tiny_House => self.on_equip_tiny_house(),
            RelicId::Astrolabe => self.open_astrolabe_grid(),
            RelicId::Orrery => {
                // Four addCardToRewards calls followed by
                // CombatRewardScreen.open's automatic CARD reward. Each
                // RewardItem eagerly snapshots its own getRewardCards roll.
                self.rewards.clear();
                self.card_reward.clear();
                for _ in 0..5 {
                    self.generate_card_reward();
                    let mut reward = Reward::new(RewardKind::Card);
                    reward.card_options = Some(std::mem::take(&mut self.card_reward));
                    self.rewards.push(reward);
                }
                self.screen = Screen::CombatReward;
            }
            RelicId::Cauldron => {
                // Cauldron.onEquip adds five uniform character-pool potions,
                // opens CombatRewardScreen, then removes its automatic CARD.
                self.rewards.clear();
                for _ in 0..5 {
                    let potion = crate::rewards::get_random_potion_for(
                        &mut self.rng,
                        self.character,
                    );
                    self.rewards.push(Reward::new(RewardKind::Potion(potion)));
                }
                // CombatRewardScreen.open rolls its automatic CARD reward
                // before Cauldron removes that RewardItem. Preserve the card
                // RNG/blizzard side effects but do not expose the cards.
                self.generate_card_reward();
                self.card_reward.clear();
                self.screen = Screen::CombatReward;
            }
            _ => {}
        }
        if matches!(id, RelicId::Frozen_Egg_2 | RelicId::Molten_Egg_2 | RelicId::Toxic_Egg_2) {
            for card in &mut self.card_reward {
                crate::rewards::preview_obtain(&self.player, card);
            }
        }
    }

    /// `AbstractCreature.increaseMaxHp`: raise max HP, then `heal(amount)`.
    fn increase_max_hp(&mut self, amount: i32) {
        self.player.max_hp += amount;
        self.heal_player(amount);
    }

    /// `AbstractCreature.heal`: Mark of the Bloom zeros the heal.
    fn heal_player(&mut self, amount: i32) {
        let amount = if self.player.has_relic(RelicId::Mark_of_the_Bloom) {
            0
        } else {
            amount
        };
        self.player.hp = (self.player.hp + amount).min(self.player.max_hp);
    }

    /// AbstractRelic.onUsePotion after potion.use. ToyOrnithopter heals 5
    /// (HealAction in combat, player.heal out of combat — instant here).
    fn on_use_potion_relics(&mut self) {
        if !self.player.has_relic(RelicId::Toy_Ornithopter) {
            return;
        }
        // Combat HealAction sits behind potion.use() overlays (Discovery,
        // Gambler's Brew, Liquid Memories).
        if self.combat.is_some()
            && (matches!(self.screen, Screen::CardReward | Screen::HandSelect)
                || self.memories_select && self.screen == Screen::Grid)
        {
            self.pending_ornithopter_heal = true;
            return;
        }
        self.heal_player(5);
        combat::red_skull_on_hp_change(&mut self.player);
    }

    fn apply_pending_ornithopter_heal(&mut self) {
        if self.pending_ornithopter_heal {
            self.pending_ornithopter_heal = false;
            if self.player.has_relic(RelicId::Toy_Ornithopter) {
                self.heal_player(5);
                combat::red_skull_on_hp_change(&mut self.player);
            }
        }
    }

    /// DiscoveryAction is queued before HealAction, so combat Discovery
    /// snapshots must not include the +5 yet.
    fn ornithopter_after_potion(&mut self, discovery_overlay: bool) {
        if discovery_overlay
            && self.combat.is_some()
            && self.player.has_relic(RelicId::Toy_Ornithopter)
        {
            self.pending_ornithopter_heal = true;
        } else {
            self.on_use_potion_relics();
        }
    }

    fn gold_with_idol(&self, amount: i32) -> i32 {
        if self.player.has_relic(RelicId::Golden_Idol) {
            amount + ((amount as f32 * 0.25) + 0.5).floor() as i32
        } else {
            amount
        }
    }

    fn gold_gain(&self, amount: i32) -> i32 {
        // AbstractPlayer.gainGold consumes the reward but awards nothing while
        // Ectoplasm is owned.
        if self.player.has_relic(RelicId::Ectoplasm) {
            return 0;
        }
        // RewardItem.applyGoldBonus: TreasureRoom skips Golden Idol.
        if self.current_room == RoomType::Treasure {
            amount
        } else {
            self.gold_with_idol(amount)
        }
    }

    fn flush_pending_equip(&mut self) {
        let pending = std::mem::take(&mut self.pending_equip);
        for id in pending {
            match id {
                RelicId::Whetstone => self.upgrade_random_cards(crate::ids::CardType::ATTACK, 2),
                RelicId::War_Paint => self.upgrade_random_cards(crate::ids::CardType::SKILL, 2),
                _ => {}
            }
        }
    }

    fn upgrade_random_cards(&mut self, typ: crate::ids::CardType, n: usize) {
        let seed = self.rng.misc.random_long();
        let mut idxs: Vec<usize> = self
            .player
            .deck
            .iter()
            .enumerate()
            .filter(|(_, c)| c.card_type() == typ && c.can_upgrade())
            .map(|(i, _)| i)
            .collect();
        shuffle_java(&mut idxs, seed);
        for idx in idxs.into_iter().take(n) {
            if let Some(card) = self.player.deck.get_mut(idx) {
                card.upgrade();
            }
        }
    }

    fn take_relic(&mut self, tier: RelicTier) -> Option<RelicId> {
        let floor = self.dungeon.floor;
        let act = self.dungeon.act;
        let room = self.current_room;
        let player = &self.player;
        self.dungeon
            .next_relic(tier, &|id| crate::dungeon::relic_can_spawn(id, floor, act, room, player))
    }

    fn take_noncamp_relic(&mut self, tier: RelicTier) -> Option<RelicId> {
        loop {
            let id = self.take_relic(tier)?;
            if !matches!(id, RelicId::Peace_Pipe | RelicId::Shovel | RelicId::Girya) {
                return Some(id);
            }
        }
    }

    /// `AbstractDungeon.returnRandomScreenlessRelic(returnRandomRelicTier())`.
    fn next_screenless_relic(&mut self) -> Option<RelicId> {
        let roll = self.rng.relic.random_range(0, 99);
        let tier = if roll < 50 {
            RelicTier::COMMON
        } else if roll < 83 {
            RelicTier::UNCOMMON
        } else {
            RelicTier::RARE
        };
        loop {
            let id = self.take_relic(tier)?;
            if !matches!(
                id,
                RelicId::Bottled_Flame
                    | RelicId::Bottled_Lightning
                    | RelicId::Bottled_Tornado
                    | RelicId::Whetstone
            ) {
                return Some(id);
            }
        }
    }

    fn flush_pending_cards(&mut self) {
        self.player.deck.append(&mut self.pending_cards);
    }

    fn gain_potion(&mut self, id: PotionId) -> bool {
        // ObtainPotionAction / ObtainPotionEffect suppress every acquisition
        // while Sozu is held. Existing potions remain usable.
        if self.player.has_relic(RelicId::Sozu) {
            return false;
        }
        if let Some(slot) = self.player.potions.iter_mut().find(|p| p.id == PotionId::Slot) {
            slot.id = id;
            true
        } else {
            false
        }
    }

    pub fn grid_summary(&self) -> Option<String> {
        self.grid.as_ref().map(|g| {
            format!(
                "kind={:?} needed={} confirm={} hovered={:?} picked={:?} return_event={}",
                g.kind, g.needed, g.confirm, g.hovered, g.picked, g.return_event
            )
        })
    }

    pub fn replay(&mut self, actions: &[Action]) {
        for action in actions {
            self.step(action);
            if self.done {
                break;
            }
        }
    }
}

/// BetterDrawPileToHandAction GRID: sortAlphabetically then
/// sortByRarityPlusStatusCardType(false) (rarity descending, status last).
fn seek_draw_grid_indices(draw: &[Card]) -> Vec<usize> {
    fn java_rarity_ord(r: CardRarity) -> i32 {
        match r {
            CardRarity::BASIC => 0,
            CardRarity::SPECIAL => 1,
            CardRarity::COMMON => 2,
            CardRarity::UNCOMMON => 3,
            CardRarity::RARE => 4,
            CardRarity::CURSE => 5,
        }
    }
    fn grid_name(c: &Card) -> String {
        let mut name = match c.sts_id() {
            "Strike_B" | "Strike_R" | "Strike_G" | "Strike_P" => "Strike".into(),
            "Defend_B" | "Defend_R" | "Defend_G" | "Defend_P" => "Defend".into(),
            "AscendersBane" => "Ascender's Bane".into(),
            other => other.replace('_', " "),
        };
        // AbstractCard.upgradeName appends `+`, and CardNameComparator sorts
        // the displayed name. This orders Defragment before Defragment+ even
        // when the upgraded physical copy comes first in the shuffled pile.
        if c.upgraded {
            name.push('+');
        }
        name
    }
    let mut idxs: Vec<usize> = (0..draw.len()).collect();
    idxs.sort_by(|&a, &b| grid_name(&draw[a]).cmp(&grid_name(&draw[b])));
    idxs.sort_by(|&a, &b| java_rarity_ord(draw[b].rarity()).cmp(&java_rarity_ord(draw[a].rarity())));
    idxs.sort_by(|&a, &b| {
        let sa = draw[a].card_type() == CardType::STATUS;
        let sb = draw[b].card_type() == CardType::STATUS;
        sa.cmp(&sb)
    });
    idxs
}
