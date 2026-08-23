use crate::generated::orders::{
    BLUE_RELIC_HASHMAP_ORDER, CARD_LIBRARY_HASHMAP_ORDER, RED_RELIC_HASHMAP_ORDER, SHARED_RELIC_HASHMAP_ORDER,
};
use crate::generated::relic_catalog::RELICS;
use crate::ids::{Act, CardId, CardRarity, CardType, Character, EncounterId, EventId, RelicId, RelicTier, RoomType};
use crate::java_util::shuffle_java;
use crate::map::{
    assign_row, distribute_rooms, generate_dungeon, generate_room_types, DungeonMap, MAP_DENSITY, MAP_HEIGHT,
    MAP_WIDTH,
};
use crate::rng::{RngSet, StsRandom};
use crate::unlocks::Unlocks;
use std::sync::Arc;

/// Cheaply cloned vector with ordinary mutable-`Vec` ergonomics. Mutations
/// detach only when a search branch still shares the previous value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct CowVec<T: Clone>(Arc<Vec<T>>);

impl<T: Clone> Default for CowVec<T> {
    fn default() -> Self {
        Self(Arc::new(Vec::new()))
    }
}

impl<T: Clone> std::ops::Deref for CowVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Clone> std::ops::DerefMut for CowVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Clone> AsRef<Vec<T>> for CowVec<T> {
    fn as_ref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T: Clone> From<Vec<T>> for CowVec<T> {
    fn from(value: Vec<T>) -> Self {
        Self(Arc::new(value))
    }
}

impl<T: Clone> FromIterator<T> for CowVec<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from(iter.into_iter().collect::<Vec<_>>())
    }
}

impl<T: Clone> IntoIterator for CowVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        Arc::try_unwrap(self.0)
            .unwrap_or_else(|shared| (*shared).clone())
            .into_iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a CowVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut CowVec<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

#[derive(Clone, Debug)]
pub struct Dungeon {
    pub act: Act,
    pub floor: i32,
    pub boss: EncounterId,
    pub boss_list: CowVec<EncounterId>,
    pub monster_list: CowVec<EncounterId>,
    pub elite_list: CowVec<EncounterId>,
    pub event_list: CowVec<EventId>,
    pub shrine_list: CowVec<EventId>,
    pub special_one_time: CowVec<EventId>,
    pub common_relics: Arc<Vec<RelicId>>,
    pub uncommon_relics: Arc<Vec<RelicId>>,
    pub rare_relics: Arc<Vec<RelicId>>,
    pub shop_relics: Arc<Vec<RelicId>>,
    pub boss_relics: Arc<Vec<RelicId>>,
    pub common_cards: Arc<Vec<CardId>>,
    pub uncommon_cards: Arc<Vec<CardId>>,
    pub rare_cards: Arc<Vec<CardId>>,
    pub colorless_cards: Arc<Vec<CardId>>,
    /// `srcColorlessCardPool`: addToBottom copy of colorlessCardPool. Discovery
    /// reads this; `returnColorlessCard` shuffles `colorless_cards` in place.
    pub src_colorless_cards: Arc<Vec<CardId>>,
    pub curse_cards: Arc<Vec<CardId>>,
    pub map: Arc<DungeonMap>,
    pub path_x: Vec<i32>,
    pub path_y: Vec<i32>,
    pub first_room_chosen: bool,
}

impl Dungeon {
    pub fn generate_exordium(seed: i64, rng: &mut RngSet, unlocks: &Unlocks, character: Character, ascension: i32) -> Self {
        let mut dungeon = Self {
            act: Act::Exordium,
            floor: 0,
            boss: EncounterId::Hexaghost,
            boss_list: CowVec::default(),
            monster_list: CowVec::default(),
            elite_list: CowVec::default(),
            event_list: CowVec::default(),
            shrine_list: CowVec::default(),
            special_one_time: CowVec::default(),
            common_relics: Arc::new(Vec::new()),
            uncommon_relics: Arc::new(Vec::new()),
            rare_relics: Arc::new(Vec::new()),
            shop_relics: Arc::new(Vec::new()),
            boss_relics: Arc::new(Vec::new()),
            common_cards: Arc::new(Vec::new()),
            uncommon_cards: Arc::new(Vec::new()),
            rare_cards: Arc::new(Vec::new()),
            colorless_cards: Arc::new(Vec::new()),
            src_colorless_cards: Arc::new(Vec::new()),
            curse_cards: Arc::new(Vec::new()),
            map: Arc::new(DungeonMap { nodes: Vec::new() }),
            path_x: Vec::new(),
            path_y: Vec::new(),
            first_room_chosen: false,
        };
        dungeon.generate_monsters(&mut rng.monster);
        dungeon.initialize_boss(&mut rng.monster, unlocks);
        dungeon.initialize_events(ascension);
        dungeon.initialize_card_pools(character, unlocks);
        dungeon.initialize_relics(character, unlocks, &mut rng.relic, &[]);
        rng.map = StsRandom::from_seed(seed.wrapping_add(1));
        dungeon.generate_map(&mut rng.map, ascension, unlocks.final_act_available);
        dungeon
    }

    pub fn generate_act(
        &mut self,
        act: Act,
        seed: i64,
        rng: &mut RngSet,
        unlocks: &Unlocks,
        character: Character,
        ascension: i32,
        place_emerald: bool,
    ) {
        self.act = act;
        self.path_x.clear();
        self.path_y.clear();
        self.first_room_chosen = false;
        self.monster_list.clear();
        self.elite_list.clear();
        self.boss_list.clear();
        self.event_list.clear();
        self.shrine_list.clear();
        match act {
            Act::City => {
                generate_weighted(
                    &mut rng.monster,
                    &mut self.monster_list,
                    &[
                        (EncounterId::SphericGuardian, 2.0),
                        (EncounterId::Chosen, 2.0),
                        (EncounterId::ShellParasite, 2.0),
                        (EncounterId::ThreeByrds, 2.0),
                        (EncounterId::TwoThieves, 2.0),
                    ],
                    2,
                    false,
                );
                let strong = [
                    (EncounterId::ChosenAndByrds, 2.0),
                    (EncounterId::SentryAndSphere, 2.0),
                    (EncounterId::SnakePlant, 6.0),
                    (EncounterId::Snecko, 4.0),
                    (EncounterId::CenturionAndHealer, 6.0),
                    (EncounterId::CultistAndChosen, 3.0),
                    (EncounterId::ThreeCultists, 3.0),
                    (EncounterId::ShelledParasiteAndFungi, 3.0),
                ];
                let exclusions = first_strong_exclusions(self.monster_list.last().copied());
                let weights = normalize(&strong);
                loop {
                    let picked = roll(&weights, rng.monster.random_float());
                    if !exclusions.iter().any(|e| *e == picked) {
                        self.monster_list.push(picked);
                        break;
                    }
                }
                generate_weighted(&mut rng.monster, &mut self.monster_list, &strong, 12, false);
                generate_weighted(
                    &mut rng.monster,
                    &mut self.elite_list,
                    &[
                        (EncounterId::GremlinLeader, 1.0),
                        (EncounterId::Slavers, 1.0),
                        (EncounterId::BookOfStabbing, 1.0),
                    ],
                    10,
                    true,
                );
                if !unlocks.boss_seen(EncounterId::Champ) {
                    self.boss_list.push(EncounterId::Champ);
                } else if !unlocks.boss_seen(EncounterId::Automaton) {
                    self.boss_list.push(EncounterId::Automaton);
                } else if !unlocks.boss_seen(EncounterId::Collector) {
                    self.boss_list.push(EncounterId::Collector);
                } else {
                    self.boss_list.extend([
                        EncounterId::Automaton,
                        EncounterId::Collector,
                        EncounterId::Champ,
                    ]);
                    shuffle_java(&mut self.boss_list, rng.monster.random_long());
                }
                self.event_list.extend(
                    [
                        EventId::Addict,
                        EventId::BackToBasics,
                        EventId::Beggar,
                        EventId::Colosseum,
                        EventId::CursedTome,
                        EventId::DrugDealer,
                        EventId::ForgottenAltar,
                        EventId::Ghosts,
                        EventId::MaskedBandits,
                        EventId::Nest,
                        EventId::Library,
                        EventId::Mausoleum,
                        EventId::Vampires,
                    ]
                );
                self.initialize_shrines();
                rng.map = StsRandom::from_seed(seed.wrapping_add(2 * 100));
            }
            Act::Beyond => {
                generate_weighted(
                    &mut rng.monster,
                    &mut self.monster_list,
                    &[
                        (EncounterId::ThreeDarklings, 2.0),
                        (EncounterId::OrbWalker, 2.0),
                        (EncounterId::ThreeShapes, 2.0),
                    ],
                    2,
                    false,
                );
                let strong = [
                    (EncounterId::SpireGrowth, 1.0),
                    (EncounterId::Transient, 1.0),
                    (EncounterId::FourShapes, 1.0),
                    (EncounterId::Maw, 1.0),
                    (EncounterId::SphereAndTwoShapes, 1.0),
                    (EncounterId::JawWormHorde, 1.0),
                    (EncounterId::ThreeDarklings, 1.0),
                    (EncounterId::WrithingMass, 1.0),
                ];
                let exclusions = first_strong_exclusions(self.monster_list.last().copied());
                let weights = normalize(&strong);
                loop {
                    let picked = roll(&weights, rng.monster.random_float());
                    if !exclusions.iter().any(|e| *e == picked) {
                        self.monster_list.push(picked);
                        break;
                    }
                }
                generate_weighted(&mut rng.monster, &mut self.monster_list, &strong, 12, false);
                generate_weighted(
                    &mut rng.monster,
                    &mut self.elite_list,
                    &[
                        (EncounterId::GiantHead, 2.0),
                        (EncounterId::Nemesis, 2.0),
                        (EncounterId::Reptomancer, 2.0),
                    ],
                    10,
                    true,
                );
                if !unlocks.boss_seen(EncounterId::AwakenedOne) {
                    self.boss_list.push(EncounterId::AwakenedOne);
                } else if !unlocks.boss_seen(EncounterId::DonuAndDeca) {
                    self.boss_list.push(EncounterId::DonuAndDeca);
                } else if !unlocks.boss_seen(EncounterId::TimeEater) {
                    self.boss_list.push(EncounterId::TimeEater);
                } else {
                    self.boss_list.extend([
                        EncounterId::AwakenedOne,
                        EncounterId::TimeEater,
                        EncounterId::DonuAndDeca,
                    ]);
                    shuffle_java(&mut self.boss_list, rng.monster.random_long());
                }
                self.event_list.extend(
                    [
                        EventId::Falling,
                        EventId::MindBloom,
                        EventId::MoaiHead,
                        EventId::MysteriousSphere,
                        EventId::SensoryStone,
                        EventId::TombOfLordRedMask,
                        EventId::WindingHalls,
                    ]
                );
                self.initialize_shrines();
                rng.map = StsRandom::from_seed(seed.wrapping_add(3 * 200));
            }
            Act::Ending => {
                self.boss_list.push(EncounterId::CorruptHeart);
                self.elite_list.push(EncounterId::ShieldAndSpear);
                // TheEnding still runs the AbstractDungeon constructor, which
                // clears and rebuilds all five card pools before its special map.
                self.initialize_card_pools(character, unlocks);
                rng.map = StsRandom::from_seed(seed.wrapping_add(4 * 300));
                self.boss = EncounterId::CorruptHeart;
                self.map = Arc::new(crate::map::generate_ending_map());
                let _ = rng.misc.random_int(1);
                return;
            }
            Act::Exordium => {}
        }
        if self.boss_list.len() == 1 {
            let duplicate = self.boss_list[0].clone();
            self.boss_list.push(duplicate);
        }
        if let Some(boss) = self.boss_list.first() {
            self.boss = boss.clone();
        }
        self.initialize_card_pools(character, unlocks);
        // Relic pools persist across acts; only Exordium shuffles them.
        // AbstractDungeon.setEmeraldElite: only if isFinalActAvailable && !hasEmeraldKey.
        self.generate_map(&mut rng.map, ascension, place_emerald);
        let _ = rng.misc.random_int(1);
    }

    fn generate_monsters(&mut self, monster: &mut StsRandom) {
        generate_weighted(
            monster,
            &mut self.monster_list,
            &[
                (EncounterId::Cultist, 2.0),
                (EncounterId::JawWorm, 2.0),
                (EncounterId::TwoLouse, 2.0),
                (EncounterId::SmallSlimes, 2.0),
            ],
            3,
            false,
        );
        let exclusions = first_strong_exclusions(self.monster_list.last().copied());
        let strong = [
            (EncounterId::BlueSlaver, 2.0),
            (EncounterId::GremlinGang, 1.0),
            (EncounterId::Looter, 2.0),
            (EncounterId::LargeSlime, 2.0),
            (EncounterId::LotsOfSlimes, 1.0),
            (EncounterId::ExordiumThugs, 1.5),
            (EncounterId::ExordiumWildlife, 1.5),
            (EncounterId::RedSlaver, 1.0),
            (EncounterId::ThreeLouse, 2.0),
            (EncounterId::TwoFungiBeasts, 2.0),
        ];
        let weights = normalize(&strong);
        loop {
            let picked = roll(&weights, monster.random_float());
            if !exclusions.iter().any(|e| *e == picked) {
                self.monster_list.push(picked);
                break;
            }
        }
        generate_weighted(monster, &mut self.monster_list, &strong, 12, false);
        generate_weighted(
            monster,
            &mut self.elite_list,
            &[
                (EncounterId::GremlinNob, 1.0),
                (EncounterId::Lagavulin, 1.0),
                (EncounterId::ThreeSentries, 1.0),
            ],
            10,
            true,
        );
    }

    fn initialize_boss(&mut self, monster: &mut StsRandom, unlocks: &Unlocks) {
        if !unlocks.boss_seen(EncounterId::TheGuardian) {
            self.boss_list.push(EncounterId::TheGuardian);
        } else if !unlocks.boss_seen(EncounterId::Hexaghost) {
            self.boss_list.push(EncounterId::Hexaghost);
        } else if !unlocks.boss_seen(EncounterId::SlimeBoss) {
            self.boss_list.push(EncounterId::SlimeBoss);
        } else {
            self.boss_list.extend([
                EncounterId::TheGuardian,
                EncounterId::Hexaghost,
                EncounterId::SlimeBoss,
            ]);
            shuffle_java(&mut self.boss_list, monster.random_long());
        }
        if self.boss_list.len() == 1 {
            let duplicate = self.boss_list[0].clone();
            self.boss_list.push(duplicate);
        } else if self.boss_list.is_empty() {
            self.boss_list.extend([
                EncounterId::TheGuardian,
                EncounterId::Hexaghost,
                EncounterId::SlimeBoss,
            ]);
            shuffle_java(&mut self.boss_list, monster.random_long());
        }
        self.boss = self.boss_list[0].clone();
    }

    fn initialize_events(&mut self, ascension: i32) {
        self.event_list.extend(
            [
                EventId::BigFish,
                EventId::Cleric,
                EventId::DeadAdventurer,
                EventId::GoldenIdol,
                EventId::GoldenWing,
                EventId::WorldOfGoop,
                EventId::LiarsGame,
                EventId::LivingWall,
                EventId::Mushrooms,
                EventId::ScrapOoze,
                EventId::ShiningLight,
            ]
        );
        self.special_one_time.extend(
            [
                EventId::AccursedBlacksmith,
                EventId::BonfireElementals,
                EventId::Designer,
                EventId::Duplicator,
                EventId::FaceTrader,
                EventId::FountainOfCleansing,
                EventId::KnowingSkull,
                EventId::Lab,
                EventId::Nloth,
                EventId::NoteForYourself,
                EventId::SecretPortal,
                EventId::Joust,
                EventId::WeMeetAgain,
                EventId::WomanInBlue,
            ]
        );
        // NoteForYourself.isNoteForYourselfAvailable is false at A15+.
        // Removing it here preserves Java's subsequent special-event order.
        if ascension >= 15 {
            self.special_one_time.retain(|event| *event != EventId::NoteForYourself);
        }
        self.initialize_shrines();
    }

    fn initialize_shrines(&mut self) {
        // Exordium puts Wheel of Change last. TheCity and TheBeyond insert
        // it second (after Match and Keep!). Same index into tmp then picks
        // a different shrine (seed 8 Act 3: Golden Shrine vs Wheel of Change).
        let shrines: &[EventId] = match self.act {
            Act::Exordium => &[
                EventId::MatchAndKeep,
                EventId::GoldenShrine,
                EventId::Transmorgrifier,
                EventId::Purifier,
                EventId::UpgradeShrine,
                EventId::WheelOfChange,
            ],
            _ => &[
                EventId::MatchAndKeep,
                EventId::WheelOfChange,
                EventId::GoldenShrine,
                EventId::Transmorgrifier,
                EventId::Purifier,
                EventId::UpgradeShrine,
            ],
        };
        self.shrine_list.extend_from_slice(shrines);
    }

    fn initialize_card_pools(&mut self, character: Character, unlocks: &Unlocks) {
        Arc::make_mut(&mut self.common_cards).clear();
        Arc::make_mut(&mut self.uncommon_cards).clear();
        Arc::make_mut(&mut self.rare_cards).clear();
        Arc::make_mut(&mut self.colorless_cards).clear();
        Arc::make_mut(&mut self.curse_cards).clear();
        let wanted_color = match character {
            Character::Ironclad => crate::ids::CardColor::RED,
            Character::Silent => crate::ids::CardColor::GREEN,
            Character::Defect => crate::ids::CardColor::BLUE,
            Character::Watcher => crate::ids::CardColor::PURPLE,
        };
        for &id in CARD_LIBRARY_HASHMAP_ORDER {
            let def = id.def();
            if unlocks.card_locked(id) {
                continue;
            }
            if def.color == wanted_color && def.rarity != CardRarity::BASIC {
                match def.rarity {
                    CardRarity::COMMON => Arc::make_mut(&mut self.common_cards).push(id),
                    CardRarity::UNCOMMON => Arc::make_mut(&mut self.uncommon_cards).push(id),
                    CardRarity::RARE => Arc::make_mut(&mut self.rare_cards).push(id),
                    _ => {}
                }
            }
        }
        for &id in CARD_LIBRARY_HASHMAP_ORDER {
            let def = id.def();
            if def.color == crate::ids::CardColor::COLORLESS
                && def.rarity != CardRarity::BASIC
                && def.rarity != CardRarity::SPECIAL
                && def.card_type != crate::ids::CardType::STATUS
            {
                Arc::make_mut(&mut self.colorless_cards).push(id);
            }
        }
        // srcColorlessCardPool.addToBottom each colorlessCardPool card.
        self.src_colorless_cards = Arc::new(self.colorless_cards.as_ref().clone());
        Arc::make_mut(&mut self.src_colorless_cards).reverse();
        for &id in CARD_LIBRARY_HASHMAP_ORDER {
            if id.def().card_type == crate::ids::CardType::CURSE
                && !matches!(
                    id,
                    CardId::Necronomicurse
                        | CardId::AscendersBane
                        | CardId::CurseOfTheBell
                        | CardId::Pride
                )
            {
                Arc::make_mut(&mut self.curse_cards).push(id);
            }
        }
    }

    fn initialize_relics(
        &mut self,
        character: Character,
        unlocks: &Unlocks,
        relic_rng: &mut StsRandom,
        owned_relics: &[RelicId],
    ) {
        let mut common = Vec::new();
        let mut uncommon = Vec::new();
        let mut rare = Vec::new();
        let mut shop = Vec::new();
        let mut boss = Vec::new();
        populate_relic_pool(&mut common, RelicTier::COMMON, character, unlocks);
        populate_relic_pool(&mut uncommon, RelicTier::UNCOMMON, character, unlocks);
        populate_relic_pool(&mut rare, RelicTier::RARE, character, unlocks);
        populate_relic_pool(&mut shop, RelicTier::SHOP, character, unlocks);
        populate_relic_pool(&mut boss, RelicTier::BOSS, character, unlocks);
        shuffle_java(&mut common, relic_rng.random_long());
        shuffle_java(&mut uncommon, relic_rng.random_long());
        shuffle_java(&mut rare, relic_rng.random_long());
        shuffle_java(&mut shop, relic_rng.random_long());
        shuffle_java(&mut boss, relic_rng.random_long());
        for owned in owned_relics {
            for pool in [&mut common, &mut uncommon, &mut rare, &mut shop, &mut boss] {
                if let Some(i) = pool.iter().position(|r| r == owned) {
                    pool.remove(i);
                    break;
                }
            }
        }
        self.common_relics = Arc::new(common);
        self.uncommon_relics = Arc::new(uncommon);
        self.rare_relics = Arc::new(rare);
        self.shop_relics = Arc::new(shop);
        self.boss_relics = Arc::new(boss);
    }

    fn generate_map(&mut self, map_rng: &mut StsRandom, ascension: i32, place_emerald: bool) {
        self.map = Arc::new(generate_dungeon(MAP_HEIGHT, MAP_WIDTH, MAP_DENSITY, map_rng));
        let mut count = 0;
        for (y, row) in self.map.nodes.iter().enumerate() {
            for node in row {
                if node.has_edges() && y != self.map.nodes.len() - 2 {
                    count += 1;
                }
            }
        }
        let rooms = generate_room_types(count, 0.05, 0.12, 0.08, 0.22, ascension);
        let last = self.map.nodes.len() - 1;
        let map = Arc::make_mut(&mut self.map);
        assign_row(map, last, RoomType::Rest);
        assign_row(map, 0, RoomType::Monster);
        assign_row(map, 8, RoomType::Treasure);
        distribute_rooms(map, map_rng, rooms);
        if place_emerald {
            self.place_emerald_elite(map_rng);
        }
    }

    fn place_emerald_elite(&mut self, map_rng: &mut StsRandom) {
        let mut elites = Vec::new();
        for row in &self.map.nodes {
            for node in row {
                if node.room == Some(RoomType::Elite) {
                    elites.push((node.x, node.y));
                }
            }
        }
        if elites.is_empty() {
            return;
        }
        let pick = map_rng.random_range(0, elites.len() as i32 - 1) as usize;
        let (x, y) = elites[pick];
        if let Some(node) = Arc::make_mut(&mut self.map)
            .nodes
            .get_mut(y as usize)
            .and_then(|r| r.iter_mut().find(|n| n.x == x))
        {
            node.emerald_key = true;
        }
    }

    pub fn next_monster(&mut self) -> Option<EncounterId> {
        if self.monster_list.is_empty() {
            None
        } else {
            Some(self.monster_list.remove(0))
        }
    }

    pub fn next_elite(&mut self) -> Option<EncounterId> {
        if self.elite_list.is_empty() {
            None
        } else {
            Some(self.elite_list.remove(0))
        }
    }

    pub fn next_relic(&mut self, tier: RelicTier, can_spawn: &dyn Fn(RelicId) -> bool) -> Option<RelicId> {
        self.return_random_relic_key(tier, can_spawn)
    }

    pub fn next_relic_end(&mut self, tier: RelicTier, can_spawn: &dyn Fn(RelicId) -> bool) -> Option<RelicId> {
        self.return_end_random_relic_key(tier, can_spawn)
    }

    /// `AbstractDungeon.returnRandomRelicKey`: pop front, `!canSpawn` retries from the end.
    pub fn return_random_relic_key(
        &mut self,
        tier: RelicTier,
        can_spawn: &dyn Fn(RelicId) -> bool,
    ) -> Option<RelicId> {
        let id = match tier {
            RelicTier::COMMON => {
                if self.common_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::UNCOMMON, can_spawn);
                }
                self.pop_relic(RelicTier::COMMON, false)
            }
            RelicTier::UNCOMMON => {
                if self.uncommon_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::RARE, can_spawn);
                }
                self.pop_relic(RelicTier::UNCOMMON, false)
            }
            RelicTier::RARE => {
                if self.rare_relics.is_empty() {
                    return Some(RelicId::Circlet);
                }
                self.pop_relic(RelicTier::RARE, false)
            }
            RelicTier::SHOP => {
                if self.shop_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::UNCOMMON, can_spawn);
                }
                self.pop_relic(RelicTier::SHOP, false)
            }
            RelicTier::BOSS => {
                if self.boss_relics.is_empty() {
                    return Some(RelicId::Red_Circlet);
                }
                self.pop_relic(RelicTier::BOSS, false)
            }
            _ => return None,
        };
        match id {
            Some(id) if can_spawn(id) => Some(id),
            _ => self.return_end_random_relic_key(tier, can_spawn),
        }
    }

    /// `AbstractDungeon.returnEndRandomRelicKey`: pop end (boss still front).
    pub fn return_end_random_relic_key(
        &mut self,
        tier: RelicTier,
        can_spawn: &dyn Fn(RelicId) -> bool,
    ) -> Option<RelicId> {
        let id = match tier {
            RelicTier::COMMON => {
                if self.common_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::UNCOMMON, can_spawn);
                }
                self.pop_relic(RelicTier::COMMON, true)
            }
            RelicTier::UNCOMMON => {
                if self.uncommon_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::RARE, can_spawn);
                }
                self.pop_relic(RelicTier::UNCOMMON, true)
            }
            RelicTier::RARE => {
                if self.rare_relics.is_empty() {
                    return Some(RelicId::Circlet);
                }
                self.pop_relic(RelicTier::RARE, true)
            }
            RelicTier::SHOP => {
                if self.shop_relics.is_empty() {
                    return self.return_random_relic_key(RelicTier::UNCOMMON, can_spawn);
                }
                self.pop_relic(RelicTier::SHOP, true)
            }
            RelicTier::BOSS => {
                if self.boss_relics.is_empty() {
                    return Some(RelicId::Red_Circlet);
                }
                self.pop_relic(RelicTier::BOSS, false)
            }
            _ => return None,
        };
        match id {
            Some(id) if can_spawn(id) => Some(id),
            _ => self.return_end_random_relic_key(tier, can_spawn),
        }
    }

    fn pop_relic(&mut self, tier: RelicTier, from_end: bool) -> Option<RelicId> {
        let pool = match tier {
            RelicTier::COMMON => &mut self.common_relics,
            RelicTier::UNCOMMON => &mut self.uncommon_relics,
            RelicTier::RARE => &mut self.rare_relics,
            RelicTier::SHOP => &mut self.shop_relics,
            RelicTier::BOSS => &mut self.boss_relics,
            _ => return None,
        };
        let pool = Arc::make_mut(pool);
        if pool.is_empty() {
            return None;
        }
        let idx = if from_end { pool.len() - 1 } else { 0 };
        Some(pool.remove(idx))
    }
}

/// `AbstractRelic.canSpawn` overrides. Endless is not modeled (always false).
pub fn relic_can_spawn(id: RelicId, floor: i32, act: Act, room: RoomType, player: &crate::creature::Player) -> bool {
    let before_act4 = floor <= 48;
    let not_in_shop = room != RoomType::Shop;
    match id {
        RelicId::Bottled_Flame => player
            .deck
            .iter()
            .any(|c| c.card_type() == CardType::ATTACK && c.rarity() != CardRarity::BASIC),
        RelicId::Bottled_Lightning => player
            .deck
            .iter()
            .any(|c| c.card_type() == CardType::SKILL && c.rarity() != CardRarity::BASIC),
        RelicId::Bottled_Tornado => player.deck.iter().any(|c| c.card_type() == CardType::POWER),
        RelicId::Ectoplasm => act as i32 <= 1,
        RelicId::Black_Blood => player.has_relic(RelicId::Burning_Blood),
        RelicId::FrozenCore => player.has_relic(RelicId::Cracked_Core),
        RelicId::Tiny_Chest => floor <= 35,
        RelicId::Matryoshka | RelicId::WingedGreaves => floor <= 40,
        RelicId::PreservedInsect => floor <= 52,
        RelicId::The_Courier | RelicId::MawBank | RelicId::Old_Coin | RelicId::Smiling_Mask => {
            before_act4 && not_in_shop
        }
        RelicId::Girya | RelicId::Peace_Pipe | RelicId::Shovel => {
            if floor >= 48 {
                return false;
            }
            let campfire = player
                .relics
                .iter()
                .filter(|r| matches!(r.id, RelicId::Peace_Pipe | RelicId::Shovel | RelicId::Girya))
                .count();
            campfire < 2
        }
        RelicId::Ancient_Tea_Set
        | RelicId::CeramicFish
        | RelicId::Darkstone_Periapt
        | RelicId::Dream_Catcher
        | RelicId::Frozen_Egg_2
        | RelicId::Juzu_Bracelet
        | RelicId::MealTicket
        | RelicId::Meat_on_the_Bone
        | RelicId::Molten_Egg_2
        | RelicId::Omamori
        | RelicId::Potion_Belt
        | RelicId::Prayer_Wheel
        | RelicId::Question_Card
        | RelicId::Regal_Pillow
        | RelicId::Singing_Bowl
        | RelicId::Toxic_Egg_2 => before_act4,
        _ => true,
    }
}

fn populate_relic_pool(pool: &mut Vec<RelicId>, tier: RelicTier, character: Character, unlocks: &Unlocks) {
    for &id in SHARED_RELIC_HASHMAP_ORDER {
        if RELICS[id as usize].tier == tier && !unlocks.relic_locked(id) {
            pool.push(id);
        }
    }
    if character == Character::Ironclad {
        for &id in RED_RELIC_HASHMAP_ORDER {
            if RELICS[id as usize].tier == tier && !unlocks.relic_locked(id) {
                pool.push(id);
            }
        }
    } else if character == Character::Defect {
        for &id in BLUE_RELIC_HASHMAP_ORDER {
            if RELICS[id as usize].tier == tier && !unlocks.relic_locked(id) {
                pool.push(id);
            }
        }
    }
}

fn first_strong_exclusions(last_weak: Option<EncounterId>) -> Vec<EncounterId> {
    match last_weak {
        Some(EncounterId::Looter) => vec![EncounterId::ExordiumThugs],
        Some(EncounterId::BlueSlaver) => {
            vec![EncounterId::RedSlaver, EncounterId::ExordiumThugs]
        }
        Some(EncounterId::TwoLouse) => vec![EncounterId::ThreeLouse],
        Some(EncounterId::SmallSlimes) => {
            vec![EncounterId::LargeSlime, EncounterId::LotsOfSlimes]
        }
        Some(EncounterId::SphericGuardian) => vec![EncounterId::SentryAndSphere],
        Some(EncounterId::ThreeByrds) => vec![EncounterId::ChosenAndByrds],
        Some(EncounterId::Chosen) => {
            vec![EncounterId::ChosenAndByrds, EncounterId::CultistAndChosen]
        }
        Some(EncounterId::ThreeDarklings) => vec![EncounterId::ThreeDarklings],
        Some(EncounterId::OrbWalker) => vec![EncounterId::OrbWalker],
        Some(EncounterId::ThreeShapes) => vec![EncounterId::FourShapes],
        _ => Vec::new(),
    }
}

fn normalize<T: Copy>(items: &[(T, f32)]) -> Vec<(T, f32)> {
    let mut sorted = items.to_vec();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f32 = sorted.iter().map(|i| i.1).sum();
    sorted
        .into_iter()
        .map(|(item, weight)| (item, weight / total))
        .collect()
}

fn roll<T: Copy>(items: &[(T, f32)], roll: f32) -> T {
    let mut current = 0.0f32;
    for (item, weight) in items {
        current += *weight;
        if roll < current {
            return *item;
        }
    }
    items.last().expect("weighted choice cannot be empty").0
}

fn generate_weighted<T: Copy + PartialEq>(
    rng: &mut StsRandom,
    dest: &mut Vec<T>,
    raw: &[(T, f32)],
    count: i32,
    elites: bool,
) {
    let weights = normalize(raw);
    let mut i = 0;
    while i < count {
        let picked = roll(&weights, rng.random_float());
        if dest.is_empty() {
            dest.push(picked);
            i += 1;
        } else if dest[dest.len() - 1] == picked {
            continue;
        } else if !elites && dest.len() > 1 && dest[dest.len() - 2] == picked {
            continue;
        } else {
            dest.push(picked);
            i += 1;
        }
    }
}
