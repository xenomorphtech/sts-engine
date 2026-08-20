use crate::card::Card;
use crate::creature::Player;
use crate::dungeon::Dungeon;
use crate::generated::relic_catalog::RELICS;
use crate::ids::{
    CardColor, CardId, CardRarity, CardType, Character, PotionId, PotionRarity, RelicId, RelicTier, RoomType,
};
use crate::rng::RngSet;

#[derive(Clone, Copy)]
struct PotionDef {
    id: &'static str,
    rarity: PotionRarity,
}

const POTION_POOL: &[PotionDef] = &[
    PotionDef { id: "BloodPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "ElixirPotion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "HeartOfIron", rarity: PotionRarity::RARE },
    PotionDef { id: "Block Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Dexterity Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Energy Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Explosive Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Fire Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Strength Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Swift Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Weak Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "FearPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "AttackPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SkillPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "PowerPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "ColorlessPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SteroidPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SpeedPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "BlessingOfTheForge", rarity: PotionRarity::COMMON },
    PotionDef { id: "Regen Potion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "Ancient Potion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "LiquidBronze", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "GamblersBrew", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "EssenceOfSteel", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "DuplicationPotion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "DistilledChaos", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "LiquidMemories", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "CultistPotion", rarity: PotionRarity::RARE },
    PotionDef { id: "Fruit Juice", rarity: PotionRarity::RARE },
    PotionDef { id: "SneckoOil", rarity: PotionRarity::RARE },
    PotionDef { id: "FairyPotion", rarity: PotionRarity::RARE },
    PotionDef { id: "SmokeBomb", rarity: PotionRarity::RARE },
    PotionDef { id: "EntropicBrew", rarity: PotionRarity::RARE },
];

pub fn roll_monster_gold(rng: &mut RngSet, boss: bool, elite: bool, ascension: i32) -> i32 {
    if boss {
        // AbstractRoom.endBattle MonsterRoomBoss: 100 + miscRng(-5, 5), then
        // A13+ MathUtils.round(tmp * 0.75F).
        let tmp = 100 + rng.misc.random_range(-5, 5);
        boss_gold_after_ascension(tmp, ascension)
    } else if elite {
        rng.treasure.random_range(25, 35)
    } else {
        rng.treasure.random_range(10, 20)
    }
}

fn boss_gold_after_ascension(tmp: i32, ascension: i32) -> i32 {
    if ascension >= 13 {
        gdx_round(tmp as f32 * 0.75)
    } else {
        tmp
    }
}

pub fn roll_potion(
    rng: &mut RngSet,
    blizzard: &mut i32,
    _elite: bool,
    skip: bool,
    character: Character,
    reward_count: usize,
    white_beast: bool,
) -> Option<PotionId> {
    // AbstractRoom.addPotionToRewards: MonsterRoomBoss instanceof MonsterRoom,
    // so Act 1/2 bosses use chance 40+blizzard. `skip` is Act 3/4 bosses,
    // where endBattle never calls addPotionToRewards (no potionRng).
    if skip {
        return None;
    }
    let mut chance = 40 + *blizzard;
    // WhiteBeastStatue: chance = 100 (still rolls potionRng; 0-99 never misses).
    if white_beast {
        chance = 100;
    }
    // AbstractRoom.addPotionToRewards: rewards.size() >= 4 forces miss.
    if reward_count >= 4 {
        chance = 0;
    }
    let roll = rng.potion.random_range(0, 99);
    if roll >= chance {
        *blizzard += 10;
        None
    } else {
        *blizzard -= 10;
        Some(return_random_potion(rng, character, false))
    }
}

pub fn random_shop_potion(rng: &mut RngSet) -> PotionId {
    return_random_potion(rng, Character::Ironclad, false)
}

const SHARED_POTIONS: &[PotionDef] = &[
    PotionDef { id: "Block Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Dexterity Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Energy Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Explosive Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Fire Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Strength Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Swift Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "Weak Potion", rarity: PotionRarity::COMMON },
    PotionDef { id: "FearPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "AttackPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SkillPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "PowerPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "ColorlessPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SteroidPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "SpeedPotion", rarity: PotionRarity::COMMON },
    PotionDef { id: "BlessingOfTheForge", rarity: PotionRarity::COMMON },
    PotionDef { id: "Regen Potion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "Ancient Potion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "LiquidBronze", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "GamblersBrew", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "EssenceOfSteel", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "DuplicationPotion", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "DistilledChaos", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "LiquidMemories", rarity: PotionRarity::UNCOMMON },
    PotionDef { id: "CultistPotion", rarity: PotionRarity::RARE },
    PotionDef { id: "Fruit Juice", rarity: PotionRarity::RARE },
    PotionDef { id: "SneckoOil", rarity: PotionRarity::RARE },
    PotionDef { id: "FairyPotion", rarity: PotionRarity::RARE },
    PotionDef { id: "SmokeBomb", rarity: PotionRarity::RARE },
    PotionDef { id: "EntropicBrew", rarity: PotionRarity::RARE },
];

fn character_potion_pool(character: Character) -> Vec<PotionDef> {
    let prefix: &[PotionDef] = match character {
        Character::Defect => &[
            PotionDef { id: "FocusPotion", rarity: PotionRarity::COMMON },
            PotionDef { id: "PotionOfCapacity", rarity: PotionRarity::UNCOMMON },
            PotionDef { id: "EssenceOfDarkness", rarity: PotionRarity::RARE },
        ],
        _ => &[
            PotionDef { id: "BloodPotion", rarity: PotionRarity::COMMON },
            PotionDef { id: "ElixirPotion", rarity: PotionRarity::UNCOMMON },
            PotionDef { id: "HeartOfIron", rarity: PotionRarity::RARE },
        ],
    };
    let mut out = prefix.to_vec();
    out.extend_from_slice(SHARED_POTIONS);
    out
}

/// PotionHelper.getRandomPotion(): uniform over the character pool via potionRng.
pub fn get_random_potion(rng: &mut RngSet) -> PotionId {
    get_random_potion_for(rng, Character::Ironclad)
}

pub fn get_random_potion_for(rng: &mut RngSet, character: Character) -> PotionId {
    let pool = character_potion_pool(character);
    let key = pool[rng.potion.random_int(pool.len() as i32 - 1) as usize].id;
    PotionId::from_sts_id(key).unwrap_or(PotionId::Block)
}

pub fn return_random_potion(rng: &mut RngSet, character: Character, limited: bool) -> PotionId {
    let roll = rng.potion.random_range(0, 99);
    let rarity = if roll < 65 {
        PotionRarity::COMMON
    } else if roll < 90 {
        PotionRarity::UNCOMMON
    } else {
        PotionRarity::RARE
    };
    let pool = character_potion_pool(character);
    let pick = |rng: &mut RngSet| {
        let def = &pool[rng.potion.random_int(pool.len() as i32 - 1) as usize];
        (PotionId::from_sts_id(def.id).unwrap_or(PotionId::Block), def.rarity, def.id)
    };
    let (mut id, mut got, key) = pick(rng);
    let mut spam = limited;
    if key != "Fruit Juice" && limited {
        // first pick is always discarded when limited; loop below refills.
    }
    while got != rarity || spam {
        spam = limited;
        let next = pick(rng);
        id = next.0;
        got = next.1;
        if next.2 != "Fruit Juice" {
            spam = false;
        }
    }
    id
}

pub fn reward_cards(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    blizz: &mut i32,
    n: usize,
    boss: bool,
    elite: bool,
    upgrade_chance: f32,
    player: &Player,
) -> Vec<Card> {
    let mut out = Vec::new();
    // MonsterRoom: rare 3 / uncommon 37. MonsterRoomElite: rare 10 / uncommon 40.
    let (rare_cut, uncommon_cut) = if elite { (10, 50) } else { (3, 40) };
    for _ in 0..n {
        let mut roll = rng.card.random_int(99);
        roll += *blizz;
        let rarity = if boss {
            *blizz = 5;
            CardRarity::RARE
        } else if roll < rare_cut {
            *blizz = 5;
            CardRarity::RARE
        } else if roll < uncommon_cut {
            CardRarity::UNCOMMON
        } else {
            *blizz -= 1;
            if *blizz < -40 {
                *blizz = -40;
            }
            CardRarity::COMMON
        };
        let pool = match rarity {
            CardRarity::RARE => &dungeon.rare_cards,
            CardRarity::UNCOMMON => &dungeon.uncommon_cards,
            _ => &dungeon.common_cards,
        };
        if pool.is_empty() {
            continue;
        }
        let mut chosen = pool[rng.card.random_int(pool.len() as i32 - 1) as usize];
        let mut guard = 0;
        while out.iter().any(|c: &Card| c.id == chosen) && guard < 20 {
            chosen = pool[rng.card.random_int(pool.len() as i32 - 1) as usize];
            guard += 1;
        }
        out.push(Card::new(chosen));
    }
    for card in &mut out {
        if card.rarity() != CardRarity::RARE && rng.card.random_boolean_chance(upgrade_chance) && card.can_upgrade() {
            card.upgrade();
        } else {
            preview_obtain(player, card);
        }
    }
    let _ = CardId::Anger;
    out
}

/// AbstractDungeon.getColorlessRewardCards: 3 colorless, rare via colorlessRareChance.
/// Neow colorless: `NeowReward.getColorlessRewardCards`. Rarity is uncommon
/// (or rare-only for the upgraded blessing). Picks use `cardRng` over the
/// colorless pool in library insertion order (no name sort).
pub fn neow_colorless_cards(dungeon: &Dungeon, rng: &mut RngSet, n: usize, rare_only: bool) -> Vec<Card> {
    let rarity = if rare_only {
        CardRarity::RARE
    } else {
        CardRarity::UNCOMMON
    };
    let mut out = Vec::new();
    for _ in 0..n {
        let mut pool: Vec<CardId> = dungeon
            .colorless_cards
            .iter()
            .copied()
            .filter(|id| id.def().rarity == rarity)
            .collect();
        pool.sort_by_key(|id| id.sts_id());
        if pool.is_empty() {
            continue;
        }
        let mut chosen = pool[rng.card.random_int(pool.len() as i32 - 1) as usize];
        let mut guard = 0;
        while out.iter().any(|c: &Card| c.id == chosen) && guard < 20 {
            chosen = pool[rng.card.random_int(pool.len() as i32 - 1) as usize];
            guard += 1;
        }
        out.push(Card::new(chosen));
    }
    out
}

pub fn colorless_reward_cards(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    blizz: &mut i32,
    n: usize,
    rare_chance: f32,
) -> Vec<Card> {
    let mut out = Vec::new();
    for _ in 0..n {
        let rarity = if rng.card.random_boolean_chance(rare_chance) {
            *blizz = 5;
            CardRarity::RARE
        } else {
            CardRarity::UNCOMMON
        };
        let Some(mut chosen) = get_colorless_from_pool(dungeon, rng, rarity) else {
            continue;
        };
        let mut guard = 0;
        while out.iter().any(|c: &Card| c.id == chosen) && guard < 20 {
            if let Some(next) = get_colorless_from_pool(dungeon, rng, rarity) {
                chosen = next;
            }
            guard += 1;
        }
        out.push(Card::new(chosen));
    }
    out
}

#[derive(Clone, Debug)]
pub struct ShopOffer<T> {
    pub item: T,
    pub price: i32,
    pub sold: bool,
}

#[derive(Clone, Debug)]
pub struct ShopStock {
    pub cards: Vec<ShopOffer<Card>>,
    pub relics: Vec<ShopOffer<RelicId>>,
    pub potions: Vec<ShopOffer<PotionId>>,
    pub purge_cost: i32,
}

/// libGDX `MathUtils.round` from desktop-1.0.jar: `(int)(value + 16384.5d) - 16384`.
/// ShopScreen.applyDiscount uses this (A16 1.1x, Courier 0.8, Membership 0.5).
pub(crate) fn gdx_round(value: f32) -> i32 {
    (value as f64 + 16384.5) as i32 - 16384
}

fn shop_roll_rarity(rng: &mut RngSet, card_blizz: i32) -> CardRarity {
    // ShopRoom: baseRareCardChance=9, baseUncommonCardChance=37, no blizzard mutation.
    let roll = rng.card.random_int(99) + card_blizz;
    if roll < 9 {
        CardRarity::RARE
    } else if roll < 46 {
        CardRarity::UNCOMMON
    } else {
        CardRarity::COMMON
    }
}

fn cards_of_type(pool: &[CardId], typ: CardType) -> Vec<CardId> {
    let mut tmp: Vec<CardId> = pool.iter().copied().filter(|id| id.def().card_type == typ).collect();
    tmp.sort_by_key(|id| id.sts_id());
    tmp
}

fn get_card_from_pool(dungeon: &Dungeon, rng: &mut RngSet, rarity: CardRarity, typ: CardType) -> Option<CardId> {
    let pool = match rarity {
        CardRarity::RARE => &dungeon.rare_cards,
        CardRarity::UNCOMMON => &dungeon.uncommon_cards,
        CardRarity::COMMON => &dungeon.common_cards,
        _ => return None,
    };
    let tmp = cards_of_type(pool, typ);
    if tmp.is_empty() {
        return match (rarity, typ) {
            (CardRarity::RARE, _) => get_card_from_pool(dungeon, rng, CardRarity::UNCOMMON, typ),
            (CardRarity::UNCOMMON, CardType::POWER) => get_card_from_pool(dungeon, rng, CardRarity::RARE, typ),
            (CardRarity::UNCOMMON, _) => get_card_from_pool(dungeon, rng, CardRarity::COMMON, typ),
            (CardRarity::COMMON, CardType::POWER) => get_card_from_pool(dungeon, rng, CardRarity::UNCOMMON, typ),
            _ => None,
        };
    }
    Some(tmp[rng.card.random_int(tmp.len() as i32 - 1) as usize])
}

fn get_colorless_from_pool(dungeon: &Dungeon, rng: &mut RngSet, rarity: CardRarity) -> Option<CardId> {
    let mut tmp: Vec<CardId> = dungeon
        .colorless_cards
        .iter()
        .copied()
        .filter(|id| id.def().rarity == rarity)
        .collect();
    if tmp.is_empty() {
        return None;
    }
    tmp.sort_by_key(|id| id.sts_id());
    Some(tmp[rng.card.random_int(tmp.len() as i32 - 1) as usize])
}

fn shop_colored_card(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    card_blizz: i32,
    typ: CardType,
    exclude: Option<CardId>,
) -> Card {
    loop {
        let rarity = shop_roll_rarity(rng, card_blizz);
        let Some(id) = get_card_from_pool(dungeon, rng, rarity, typ) else {
            continue;
        };
        if id.def().color == CardColor::COLORLESS || exclude == Some(id) {
            continue;
        }
        return Card::new(id);
    }
}

/// AbstractDungeon.returnTrulyRandomCardInCombat / DiscoveryAction choices.
pub fn discovery_cards(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    typ: Option<CardType>,
    colorless: bool,
) -> Vec<Card> {
    let mut out = Vec::new();
    let mut guard = 0;
    while out.len() < 3 && guard < 40 {
        guard += 1;
        let Some(id) = truly_random_combat_card(dungeon, rng, typ, colorless) else {
            break;
        };
        if out.iter().any(|c: &Card| c.id == id) {
            continue;
        }
        out.push(Card::new(id));
    }
    out
}

/// ExactTextSim keeps DiscoveryAction on the queue after the card is picked;
/// each leftover update rebuilds the 3-card list and burns cardRandomRng.
pub fn burn_discovery_rng(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    typ: Option<CardType>,
    colorless: bool,
    rounds: usize,
) {
    for _ in 0..rounds {
        let _ = discovery_cards(dungeon, rng, typ, colorless);
    }
}

pub(crate) fn random_power_in_combat(dungeon: &Dungeon, rng: &mut RngSet) -> Option<CardId> {
    truly_random_combat_card(dungeon, rng, Some(CardType::POWER), false)
}

fn truly_random_combat_card(
    dungeon: &Dungeon,
    rng: &mut RngSet,
    typ: Option<CardType>,
    colorless: bool,
) -> Option<CardId> {
    // CardGroup.addToTop appends; src pools copy via addToBottom, reversing
    // each rarity. returnTrulyRandomCardInCombat concatenates the src pools.
    let mut list: Vec<CardId> = if colorless {
        dungeon.src_colorless_cards.clone()
    } else {
        let mut commons = dungeon.common_cards.clone();
        let mut uncommons = dungeon.uncommon_cards.clone();
        let mut rares = dungeon.rare_cards.clone();
        commons.reverse();
        uncommons.reverse();
        rares.reverse();
        commons.extend(uncommons);
        commons.extend(rares);
        commons
    };
    if let Some(typ) = typ {
        list.retain(|id| id.def().card_type == typ);
    }
    // AbstractDungeon.returnTrulyRandomCardInCombat(type) skips HEALING.
    list.retain(|id| !id.has_healing_tag());
    if list.is_empty() {
        return None;
    }
    Some(list[rng.card_random.random_int(list.len() as i32 - 1) as usize])
}

pub fn preview_obtain(player: &Player, card: &mut Card) {
    let typ = card.card_type();
    if typ == CardType::ATTACK && player.has_relic(RelicId::Molten_Egg_2) {
        card.upgrade();
    } else if typ == CardType::SKILL && player.has_relic(RelicId::Toxic_Egg_2) {
        card.upgrade();
    } else if typ == CardType::POWER && player.has_relic(RelicId::Frozen_Egg_2) {
        card.upgrade();
    }
}

fn card_base_price(rarity: CardRarity) -> i32 {
    match rarity {
        CardRarity::COMMON => 50,
        CardRarity::UNCOMMON => 75,
        CardRarity::RARE => 150,
        _ => 9999,
    }
}

fn relic_base_price(id: RelicId) -> i32 {
    let tier = RELICS
        .iter()
        .find(|r| r.id == id)
        .map(|r| r.tier)
        .unwrap_or(RelicTier::COMMON);
    match tier {
        RelicTier::STARTER => 300,
        RelicTier::COMMON => 150,
        RelicTier::UNCOMMON => 250,
        RelicTier::RARE => 300,
        RelicTier::SHOP => 150,
        RelicTier::SPECIAL => 400,
        RelicTier::BOSS => 999,
    }
}

fn potion_base_price(id: PotionId) -> i32 {
    let rarity = match id {
        PotionId::Focus => PotionRarity::COMMON,
        PotionId::PotionOfCapacity => PotionRarity::UNCOMMON,
        PotionId::EssenceOfDarkness => PotionRarity::RARE,
        _ => POTION_POOL
            .iter()
            .find(|d| PotionId::from_sts_id(d.id) == Some(id))
            .map(|d| d.rarity)
            .unwrap_or(PotionRarity::COMMON),
    };
    match rarity {
        PotionRarity::COMMON => 50,
        PotionRarity::UNCOMMON => 75,
        PotionRarity::RARE | PotionRarity::PLACEHOLDER => 100,
    }
}

/// Merchant ctor + ShopScreen.init: 5 colored, 2 colorless, 3 relics, 3 potions.
pub fn generate_shop(
    dungeon: &mut Dungeon,
    rng: &mut RngSet,
    player: &Player,
    card_blizz: i32,
    ascension: i32,
    character: Character,
    room: RoomType,
) -> ShopStock {
    let attack1 = shop_colored_card(dungeon, rng, card_blizz, CardType::ATTACK, None);
    let attack2 = shop_colored_card(dungeon, rng, card_blizz, CardType::ATTACK, Some(attack1.id));
    let skill1 = shop_colored_card(dungeon, rng, card_blizz, CardType::SKILL, None);
    let skill2 = shop_colored_card(dungeon, rng, card_blizz, CardType::SKILL, Some(skill1.id));
    let power = shop_colored_card(dungeon, rng, card_blizz, CardType::POWER, None);
    let mut colored = vec![attack1, attack2, skill1, skill2, power];
    let mut colorless = Vec::new();
    if let Some(id) = get_colorless_from_pool(dungeon, rng, CardRarity::UNCOMMON) {
        colorless.push(Card::new(id));
    }
    if let Some(id) = get_colorless_from_pool(dungeon, rng, CardRarity::RARE) {
        colorless.push(Card::new(id));
    }
    for card in colored.iter_mut().chain(colorless.iter_mut()) {
        preview_obtain(player, card);
    }

    let mut cards = Vec::new();
    for card in &colored {
        // ShopScreen.initCards assigns `(int)tmpPrice` (truncate), not MathUtils.round.
        let price = (card_base_price(card.rarity()) as f32 * rng.merchant.random_float_range(0.9, 1.1)) as i32;
        cards.push(ShopOffer {
            item: card.clone(),
            price,
            sold: false,
        });
    }
    for card in &colorless {
        let price = (card_base_price(card.rarity()) as f32
            * rng.merchant.random_float_range(0.9, 1.1)
            * 1.2) as i32;
        cards.push(ShopOffer {
            item: card.clone(),
            price,
            sold: false,
        });
    }
    if !cards.is_empty() {
        let sale = rng.merchant.random_range(0, 4) as usize;
        if sale < colored.len() {
            cards[sale].price /= 2;
        }
    }

    let mut relics = Vec::new();
    let floor = dungeon.floor;
    let act = dungeon.act;
    for i in 0..3 {
        let tier = if i == 2 {
            RelicTier::SHOP
        } else {
            let roll = rng.merchant.random_int(99);
            if roll < 48 {
                RelicTier::COMMON
            } else if roll < 82 {
                RelicTier::UNCOMMON
            } else {
                RelicTier::RARE
            }
        };
        if let Some(id) = dungeon.next_relic_end(tier, &|id| {
            crate::dungeon::relic_can_spawn(id, floor, act, room, player)
        }) {
            let price = gdx_round(relic_base_price(id) as f32 * rng.merchant.random_float_range(0.95, 1.05));
            relics.push(ShopOffer {
                item: id,
                price,
                sold: false,
            });
        }
    }

    let mut potions = Vec::new();
    for _ in 0..3 {
        let id = return_random_potion(rng, character, false);
        let price = gdx_round(potion_base_price(id) as f32 * rng.merchant.random_float_range(0.95, 1.05));
        potions.push(ShopOffer {
            item: id,
            price,
            sold: false,
        });
    }

    // ShopScreen.init: A16 applyDiscount(1.1, false), then Courier 0.8 / Membership 0.5.
    if ascension >= 16 {
        apply_shop_discount(&mut cards, &mut relics, &mut potions, 1.1);
    }
    if player.has_relic(RelicId::The_Courier) {
        apply_shop_discount(&mut cards, &mut relics, &mut potions, 0.8);
    }
    if player.has_relic(RelicId::Membership_Card) {
        apply_shop_discount(&mut cards, &mut relics, &mut potions, 0.5);
    }

    ShopStock {
        cards,
        relics,
        potions,
        purge_cost: 75,
    }
}

fn apply_shop_discount(
    cards: &mut [ShopOffer<Card>],
    relics: &mut [ShopOffer<RelicId>],
    potions: &mut [ShopOffer<PotionId>],
    mult: f32,
) {
    for offer in cards.iter_mut() {
        offer.price = gdx_round(offer.price as f32 * mult);
    }
    for offer in relics.iter_mut() {
        offer.price = gdx_round(offer.price as f32 * mult);
    }
    for offer in potions.iter_mut() {
        offer.price = gdx_round(offer.price as f32 * mult);
    }
}

#[cfg(test)]
mod tests {
    use super::gdx_round;

    #[test]
    fn math_utils_round_half_away_matches_gdx() {
        // Sale Glacier: (int)tmpPrice/2 == 35, then applyDiscount(1.1F) → 39, not 38.
        assert_eq!(gdx_round(35.0 * 1.1), 39);
        assert_eq!(gdx_round(38.5), 39);
        assert_eq!(gdx_round(45.0 * 1.1), 50);
        assert_eq!(gdx_round(55.0 * 1.1), 61);
        assert_eq!(gdx_round(75.0 * 1.1), 83);
        // Emerald elite IncreaseMaxHpAction(0.25F).
        assert_eq!(gdx_round(114.0 * 0.25), 29);
        assert_eq!(gdx_round(42.0 * 0.25), 11);
        assert_eq!(gdx_round(45.0 * 0.25), 11);
    }

    #[test]
    fn a13_boss_gold_is_three_quarters_gdx_round() {
        // 606190 Hexaghost: misc rolled tmp=96, Java GOLD 72 not 96.
        assert_eq!(super::boss_gold_after_ascension(96, 20), 72);
        assert_eq!(super::boss_gold_after_ascension(100, 13), 75);
        assert_eq!(super::boss_gold_after_ascension(95, 20), 71);
        assert_eq!(super::boss_gold_after_ascension(100, 12), 100);
    }

    #[test]
    fn a5_act_transition_heals_three_quarters_of_missing() {
        // 606190 / 958546: 35/71 → +gdx_round(36*0.75)=27 → 62.
        assert_eq!(gdx_round((71 - 35) as f32 * 0.75), 27);
        assert_eq!(gdx_round(36.0 * 0.75), 27);
    }
}
