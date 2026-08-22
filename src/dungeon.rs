use crate::generated::orders::{
    BLUE_RELIC_HASHMAP_ORDER, CARD_LIBRARY_HASHMAP_ORDER, RED_RELIC_HASHMAP_ORDER, SHARED_RELIC_HASHMAP_ORDER,
};
use crate::generated::relic_catalog::RELICS;
use crate::ids::{Act, CardId, CardRarity, CardType, Character, EncounterId, RelicId, RelicTier, RoomType};
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
#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug)]
pub struct Dungeon {
    pub act: Act,
    pub id: &'static str,
    pub name: &'static str,
    pub floor: i32,
    pub boss: String,
    pub boss_list: CowVec<String>,
    pub monster_list: CowVec<String>,
    pub elite_list: CowVec<String>,
    pub event_list: CowVec<String>,
    pub shrine_list: CowVec<String>,
    pub special_one_time: CowVec<String>,
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
            id: "Exordium",
            name: "Exordium",
            floor: 0,
            boss: String::new(),
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
                self.id = "TheCity";
                self.name = "The City";
                generate_weighted(
                    &mut rng.monster,
                    &mut self.monster_list,
                    &[
                        ("Spheric Guardian", 2.0),
                        ("Chosen", 2.0),
                        ("Shell Parasite", 2.0),
                        ("3 Byrds", 2.0),
                        ("2 Thieves", 2.0),
                    ],
                    2,
                    false,
                );
                let strong = [
                    ("Chosen and Byrds", 2.0),
                    ("Sentry and Sphere", 2.0),
                    ("Snake Plant", 6.0),
                    ("Snecko", 4.0),
                    ("Centurion and Healer", 6.0),
                    ("Cultist and Chosen", 3.0),
                    ("3 Cultists", 3.0),
                    ("Shelled Parasite and Fungi", 3.0),
                ];
                let exclusions = first_strong_exclusions(
                    self.monster_list.last().map(String::as_str).unwrap_or(""),
                );
                let weights = normalize(&strong);
                loop {
                    let picked = roll(&weights, rng.monster.random_float());
                    if !exclusions.iter().any(|e| *e == picked) {
                        self.monster_list.push(picked.to_string());
                        break;
                    }
                }
                generate_weighted(&mut rng.monster, &mut self.monster_list, &strong, 12, false);
                generate_weighted(
                    &mut rng.monster,
                    &mut self.elite_list,
                    &[
                        ("Gremlin Leader", 1.0),
                        ("Slavers", 1.0),
                        ("Book of Stabbing", 1.0),
                    ],
                    10,
                    true,
                );
                if !unlocks.boss_seen("CHAMP") {
                    self.boss_list.push("Champ".into());
                } else if !unlocks.boss_seen("AUTOMATON") {
                    self.boss_list.push("Automaton".into());
                } else if !unlocks.boss_seen("COLLECTOR") {
                    self.boss_list.push("Collector".into());
                } else {
                    self.boss_list
                        .extend(["Automaton", "Collector", "Champ"].map(str::to_string));
                    shuffle_java(&mut self.boss_list, rng.monster.random_long());
                }
                self.event_list.extend(
                    [
                        "Addict",
                        "Back to Basics",
                        "Beggar",
                        "Colosseum",
                        "Cursed Tome",
                        "Drug Dealer",
                        "Forgotten Altar",
                        "Ghosts",
                        "Masked Bandits",
                        "Nest",
                        "The Library",
                        "The Mausoleum",
                        "Vampires",
                    ]
                    .map(str::to_string),
                );
                self.initialize_shrines();
                rng.map = StsRandom::from_seed(seed.wrapping_add(2 * 100));
            }
            Act::Beyond => {
                self.id = "TheBeyond";
                self.name = "The Beyond";
                generate_weighted(
                    &mut rng.monster,
                    &mut self.monster_list,
                    &[("3 Darklings", 2.0), ("Orb Walker", 2.0), ("3 Shapes", 2.0)],
                    2,
                    false,
                );
                let strong = [
                    ("Spire Growth", 1.0),
                    ("Transient", 1.0),
                    ("4 Shapes", 1.0),
                    ("Maw", 1.0),
                    ("Sphere and 2 Shapes", 1.0),
                    ("Jaw Worm Horde", 1.0),
                    ("3 Darklings", 1.0),
                    ("Writhing Mass", 1.0),
                ];
                let exclusions = first_strong_exclusions(
                    self.monster_list.last().map(String::as_str).unwrap_or(""),
                );
                let weights = normalize(&strong);
                loop {
                    let picked = roll(&weights, rng.monster.random_float());
                    if !exclusions.iter().any(|e| *e == picked) {
                        self.monster_list.push(picked.to_string());
                        break;
                    }
                }
                generate_weighted(&mut rng.monster, &mut self.monster_list, &strong, 12, false);
                generate_weighted(
                    &mut rng.monster,
                    &mut self.elite_list,
                    &[("Giant Head", 2.0), ("Nemesis", 2.0), ("Reptomancer", 2.0)],
                    10,
                    true,
                );
                if !unlocks.boss_seen("CROW") {
                    self.boss_list.push("Awakened One".into());
                } else if !unlocks.boss_seen("DONUT") {
                    self.boss_list.push("Donu and Deca".into());
                } else if !unlocks.boss_seen("WIZARD") {
                    self.boss_list.push("Time Eater".into());
                } else {
                    self.boss_list
                        .extend(["Awakened One", "Time Eater", "Donu and Deca"].map(str::to_string));
                    shuffle_java(&mut self.boss_list, rng.monster.random_long());
                }
                self.event_list.extend(
                    [
                        "Falling",
                        "MindBloom",
                        "The Moai Head",
                        "Mysterious Sphere",
                        "SensoryStone",
                        "Tomb of Lord Red Mask",
                        "Winding Halls",
                    ]
                    .map(str::to_string),
                );
                self.initialize_shrines();
                rng.map = StsRandom::from_seed(seed.wrapping_add(3 * 200));
            }
            Act::Ending => {
                self.id = "TheEnding";
                self.name = "The Ending";
                self.boss_list.push("The Heart".into());
                self.elite_list.push("Shield and Spear".into());
                // TheEnding still runs the AbstractDungeon constructor, which
                // clears and rebuilds all five card pools before its special map.
                self.initialize_card_pools(character, unlocks);
                rng.map = StsRandom::from_seed(seed.wrapping_add(4 * 300));
                self.boss = "The Heart".into();
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
                ("Cultist", 2.0),
                ("Jaw Worm", 2.0),
                ("2 Louse", 2.0),
                ("Small Slimes", 2.0),
            ],
            3,
            false,
        );
        let exclusions = first_strong_exclusions(self.monster_list.last().map(String::as_str).unwrap_or(""));
        let strong = [
            ("Blue Slaver", 2.0),
            ("Gremlin Gang", 1.0),
            ("Looter", 2.0),
            ("Large Slime", 2.0),
            ("Lots of Slimes", 1.0),
            ("Exordium Thugs", 1.5),
            ("Exordium Wildlife", 1.5),
            ("Red Slaver", 1.0),
            ("3 Louse", 2.0),
            ("2 Fungi Beasts", 2.0),
        ];
        let weights = normalize(&strong);
        loop {
            let picked = roll(&weights, monster.random_float());
            if !exclusions.iter().any(|e| *e == picked) {
                self.monster_list.push(picked.to_string());
                break;
            }
        }
        generate_weighted(monster, &mut self.monster_list, &strong, 12, false);
        generate_weighted(
            monster,
            &mut self.elite_list,
            &[("Gremlin Nob", 1.0), ("Lagavulin", 1.0), ("3 Sentries", 1.0)],
            10,
            true,
        );
    }

    fn initialize_boss(&mut self, monster: &mut StsRandom, unlocks: &Unlocks) {
        if !unlocks.boss_seen("GUARDIAN") {
            self.boss_list.push("The Guardian".into());
        } else if !unlocks.boss_seen("GHOST") {
            self.boss_list.push("Hexaghost".into());
        } else if !unlocks.boss_seen("SLIME") {
            self.boss_list.push("Slime Boss".into());
        } else {
            self.boss_list
                .extend(["The Guardian", "Hexaghost", "Slime Boss"].map(str::to_string));
            shuffle_java(&mut self.boss_list, monster.random_long());
        }
        if self.boss_list.len() == 1 {
            let duplicate = self.boss_list[0].clone();
            self.boss_list.push(duplicate);
        } else if self.boss_list.is_empty() {
            self.boss_list
                .extend(["The Guardian", "Hexaghost", "Slime Boss"].map(str::to_string));
            shuffle_java(&mut self.boss_list, monster.random_long());
        }
        self.boss = self.boss_list[0].clone();
    }

    fn initialize_events(&mut self, ascension: i32) {
        self.event_list.extend(
            [
                "Big Fish",
                "The Cleric",
                "Dead Adventurer",
                "Golden Idol",
                "Golden Wing",
                "World of Goop",
                "Liars Game",
                "Living Wall",
                "Mushrooms",
                "Scrap Ooze",
                "Shining Light",
            ]
            .map(str::to_string),
        );
        self.special_one_time.extend(
            [
                "Accursed Blacksmith",
                "Bonfire Elementals",
                "Designer",
                "Duplicator",
                "FaceTrader",
                "Fountain of Cleansing",
                "Knowing Skull",
                "Lab",
                "N'loth",
                "NoteForYourself",
                "SecretPortal",
                "The Joust",
                "WeMeetAgain",
                "The Woman in Blue",
            ]
            .map(str::to_string),
        );
        // NoteForYourself.isNoteForYourselfAvailable is false at A15+.
        // Removing it here preserves Java's subsequent special-event order.
        if ascension >= 15 {
            self.special_one_time.retain(|event| event != "NoteForYourself");
        }
        self.initialize_shrines();
    }

    fn initialize_shrines(&mut self) {
        // Exordium puts Wheel of Change last. TheCity and TheBeyond insert
        // it second (after Match and Keep!). Same index into tmp then picks
        // a different shrine (seed 8 Act 3: Golden Shrine vs Wheel of Change).
        let shrines: &[&str] = match self.act {
            Act::Exordium => &[
                "Match and Keep!",
                "Golden Shrine",
                "Transmorgrifier",
                "Purifier",
                "Upgrade Shrine",
                "Wheel of Change",
            ],
            _ => &[
                "Match and Keep!",
                "Wheel of Change",
                "Golden Shrine",
                "Transmorgrifier",
                "Purifier",
                "Upgrade Shrine",
            ],
        };
        self.shrine_list.extend(shrines.iter().map(|s| (*s).to_string()));
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
        for sts_id in CARD_LIBRARY_HASHMAP_ORDER {
            let Some(id) = CardId::from_sts_id(sts_id) else {
                continue;
            };
            let def = id.def();
            if unlocks.card_locked(sts_id) {
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
        for sts_id in CARD_LIBRARY_HASHMAP_ORDER {
            let Some(id) = CardId::from_sts_id(sts_id) else {
                continue;
            };
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
        for sts_id in CARD_LIBRARY_HASHMAP_ORDER {
            let Some(id) = CardId::from_sts_id(sts_id) else {
                continue;
            };
            if id.def().card_type == crate::ids::CardType::CURSE
                && !matches!(
                    *sts_id,
                    "Necronomicurse" | "AscendersBane" | "CurseOfTheBell" | "Pride"
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
            EncounterId::from_sts_key(&self.monster_list.remove(0))
        }
    }

    pub fn next_elite(&mut self) -> Option<EncounterId> {
        if self.elite_list.is_empty() {
            None
        } else {
            EncounterId::from_sts_key(&self.elite_list.remove(0))
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
                    return RelicId::from_sts_id("Circlet");
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
                    return RelicId::from_sts_id("Red Circlet");
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
                    return RelicId::from_sts_id("Circlet");
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
                    return RelicId::from_sts_id("Red Circlet");
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
    let id_to_tier: std::collections::HashMap<&str, RelicTier> =
        RELICS.iter().map(|r| (r.sts_id, r.tier)).collect();
    for id in SHARED_RELIC_HASHMAP_ORDER {
        if id_to_tier.get(id) == Some(&tier) && !unlocks.relic_locked(id) {
            pool.push(RelicId::from_sts_id(id).expect("generated shared relic id"));
        }
    }
    if character == Character::Ironclad {
        for id in RED_RELIC_HASHMAP_ORDER {
            if id_to_tier.get(id) == Some(&tier) && !unlocks.relic_locked(id) {
                pool.push(RelicId::from_sts_id(id).expect("generated red relic id"));
            }
        }
    } else if character == Character::Defect {
        for id in BLUE_RELIC_HASHMAP_ORDER {
            if id_to_tier.get(id) == Some(&tier) && !unlocks.relic_locked(id) {
                pool.push(RelicId::from_sts_id(id).expect("generated blue relic id"));
            }
        }
    }
}

fn first_strong_exclusions(last_weak: &str) -> Vec<&'static str> {
    match last_weak {
        "Looter" => vec!["Exordium Thugs"],
        "Blue Slaver" => vec!["Red Slaver", "Exordium Thugs"],
        "2 Louse" => vec!["3 Louse"],
        "Small Slimes" => vec!["Large Slime", "Lots of Slimes"],
        "Spheric Guardian" => vec!["Sentry and Sphere"],
        "3 Byrds" => vec!["Chosen and Byrds"],
        "Chosen" => vec!["Chosen and Byrds", "Cultist and Chosen"],
        "3 Darklings" => vec!["3 Darklings"],
        "Orb Walker" => vec!["Orb Walker"],
        "3 Shapes" => vec!["4 Shapes"],
        _ => Vec::new(),
    }
}

fn normalize(items: &[(&str, f32)]) -> Vec<(String, f32)> {
    let mut sorted: Vec<(&str, f32)> = items.to_vec();
    sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    let total: f32 = sorted.iter().map(|i| i.1).sum();
    sorted
        .into_iter()
        .map(|(n, w)| (n.to_string(), w / total))
        .collect()
}

fn roll(items: &[(String, f32)], roll: f32) -> &str {
    let mut current = 0.0f32;
    for (name, weight) in items {
        current += *weight;
        if roll < current {
            return name;
        }
    }
    items.last().map(|(n, _)| n.as_str()).unwrap_or("ERROR")
}

fn generate_weighted(
    rng: &mut StsRandom,
    dest: &mut Vec<String>,
    raw: &[(&str, f32)],
    count: i32,
    elites: bool,
) {
    let weights = normalize(raw);
    let mut i = 0;
    while i < count {
        let picked = roll(&weights, rng.random_float()).to_string();
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
