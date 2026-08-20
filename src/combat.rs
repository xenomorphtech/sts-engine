use crate::card::Card;
use crate::content::encounter_monsters;
use crate::creature::{power_is_debuff, Intent, Monster, Orb, OrbKind, Player};
use crate::dungeon::Dungeon;
use crate::ids::{CardId, CardRarity, CardType, EncounterId, MonsterId, PotionId, PowerId, RelicId};
use crate::java_util::shuffle_java;
use crate::rng::RngSet;

#[derive(Clone, Debug)]
pub struct Combat {
    pub encounter: EncounterId,
    pub monsters: Vec<Monster>,
    pub turn: i32,
    pub cards_played_this_turn: i32,
    pub skills_this_turn: i32,
    pub attacks_this_turn: i32,
    pub need_exhaust_select: bool,
    pub need_put_on_deck: bool,
    /// BetterDiscardPileToHandAction GRID (Hologram).
    pub need_discard_to_hand: bool,
    /// BetterDrawPileToHandAction GRID (Seek).
    pub need_draw_to_hand: bool,
    /// DiscoveryAction combat card-reward overlay.
    pub need_discovery: bool,
    /// Forethought: HAND_SELECT then moveToBottomOfDeck + freeToPlayOnce.
    pub need_forethought: bool,
    /// SkillFromDeckToHandAction GRID (Secret Technique).
    pub need_skill_from_deck: bool,
    /// Draw-pile indices in addToRandomSpot order for that GRID.
    pub skill_from_deck: Vec<usize>,
    pub pending_exhaust: Option<Card>,
    pub draw_after_exhaust: i32,
    pub pending_dark_embrace: i32,
    /// InkBottle.onUseCard: addToBot(DrawCardAction) after the card's use() actions.
    pub pending_ink_bottle: i32,
    pub ascension: i32,
    /// GameActionManager.orbsChanneledThisCombat (Blizzard / Thunder Strike).
    pub orbs_channeled_this_combat: Vec<OrbKind>,
    /// AbstractCard.energyOnUse snapshotted at play. Duplication/Echo copies
    /// reuse this (CardQueueItem(tmp, m, card.energyOnUse)) even after the
    /// original spent the energy (seed 991 Tempest x2).
    pub energy_on_use: i32,
}

impl Combat {
    pub fn start(
        encounter: EncounterId,
        player: &mut Player,
        rng: &mut RngSet,
        floor: i32,
        seed: i64,
        ascension: i32,
    ) -> Self {
        rng.reset_floor_streams(seed, floor);
        let mut monsters = spawn_encounter(encounter, rng, ascension);
        apply_encounter_misc(encounter, rng);

        if player.has_relic(RelicId::PreservedInsect) && is_elite_encounter(encounter) {
            for m in &mut monsters {
                let cap = (m.max_hp as f32 * 0.75) as i32;
                if m.hp > cap {
                    m.hp = cap;
                }
            }
        }
        let ally_count = monsters.len() as i32;
        // Java: all constructors, then each usePreBattleAction, then getMove.
        for monster in monsters.iter_mut() {
            if encounter == EncounterId::JawWormHorde && monster.id == MonsterId::JawWorm {
                // JawWorm(hardMode): skip the opening Chomp and start bellowed.
                monster.first_move = false;
                monster.extra = 1;
            }
            apply_prebattle(monster, rng);
        }
        // BagOfMarbles.atBattleStart is after usePreBattleAction, so Sentry
        // Artifact (and similar) absorbs the Vulnerable.
        if player.has_relic(RelicId::Bag_of_Marbles) {
            for m in &mut monsters {
                if m.alive() {
                    m.add_power(PowerId::Vulnerable, 1);
                }
            }
        }
        for (i, monster) in monsters.iter_mut().enumerate() {
            monster.roll_move_group(rng, 0, ally_count, i as i32);
            // AbstractMonster.getMove/setMove fills EnemyMoveInfo only.
            // intent stays DEBUG and intentBaseDmg stays -1 until
            // BattleStartEffect.update → MonsterGroup.showIntent → createIntent
            // (duration < 3s after timer1). ExactTextSim can publish the first
            // combat_turn in that window, so ForTheEyesAction sees ibd < 0.
        }

        player.block = 0;
        player.powers.clear();
        player.pending_static = 0;
        player.pending_evoke_lightning.clear();
        player.pending_evoke_dark.clear();
        player.draw = player.deck.clone();
        player.hand.clear();
        player.discard.clear();
        player.exhaust.clear();
        let seed = rng.shuffle.random_long();
        shuffle_java(&mut player.draw, seed);
        // CardGroup.initializeDeck: innate (and bottled) cards sit on top.
        let mut rest = Vec::new();
        let mut on_top = Vec::new();
        for card in player.draw.drain(..) {
            if card.innate || card.in_bottle {
                on_top.push(card);
            } else {
                rest.push(card);
            }
        }
        player.draw = rest;
        player.draw.append(&mut on_top);
        player.energy = player.energy_master;
        if player.has_relic(RelicId::Lantern) {
            player.energy += 1;
        }
        // AncientTeaSet.atTurnStart: first turn after RestRoom.onPlayerEntry (counter == -2)
        // GainEnergyAction(2) only. No extra draw.
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Ancient_Tea_Set) {
            if r.counter == -2 {
                r.counter = -1;
                player.energy += 2;
            }
        }
        let _ = draw_cards_rng(player, 5, Some(rng));
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::HornCleat) {
            r.counter = 0;
        }
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::CaptainsWheel) {
            r.counter = 0;
        }
        if player.has_relic(RelicId::Anchor) {
            player.block += 10;
        }
        if player.has_relic(RelicId::Vajra) {
            player.add_power(PowerId::Strength, 1);
        }
        // OddlySmoothStone.atBattleStart: ApplyPowerAction Dexterity 1.
        if player.has_relic(RelicId::Oddly_Smooth_Stone) {
            player.add_power(PowerId::Dexterity, 1);
        }
        // ThreadAndNeedle.atBattleStart: ApplyPowerAction(player, PlatedArmorPower, 4).
        if player.has_relic(RelicId::Thread_and_Needle) {
            player.add_power(PowerId::PlatedArmor, 4);
        }
        // FossilizedHelix.atBattleStart: BufferPower 1.
        if player.has_relic(RelicId::FossilizedHelix) {
            player.add_power(PowerId::Buffer, 1);
        }
        // StoneCalendar.atBattleStart: counter = 0, then atTurnStart ++.
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::StoneCalendar) {
            r.counter = 0;
        }
        if player.has_relic(RelicId::Happy_Flower) {
            if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Happy_Flower) {
                r.counter += 1;
                if r.counter == 3 {
                    r.counter = 0;
                    player.energy += 1;
                }
            }
        }
        tick_turn_start_block_relics(player);
        if player.has_relic(RelicId::Bag_of_Preparation) {
            let _ = draw_cards_rng(player, 2, Some(rng));
        }
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Letter_Opener) {
            r.counter = 0;
        }
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Kunai) {
            r.counter = 0;
        }
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Shuriken) {
            r.counter = 0;
        }
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Ornamental_Fan) {
            r.counter = 0;
        }
        if player
            .relics
            .iter()
            .any(|r| r.id == RelicId::Pen_Nib && r.counter == 9)
        {
            player.add_power(PowerId::PenNib, 1);
        }
        // CentennialPuzzle.atPreBattle: usedThisCombat = false.
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Centennial_Puzzle) {
            r.used_up = false;
        }
        // BloodVial.atBattleStart: HealAction(player, player, 2). addToTop, so
        // Red Skull's addToBot bloodied check sees post-heal HP.
        if player.has_relic(RelicId::Blood_Vial) {
            player.hp = (player.hp + 2).min(player.max_hp);
        }
        // Pantograph.atBattleStart: HealAction(25) if any monster is BOSS.
        if player.has_relic(RelicId::Pantograph)
            && is_boss_encounter(encounter)
            && !player.has_relic(RelicId::Mark_of_the_Bloom)
        {
            player.hp = (player.hp + 25).min(player.max_hp);
        }
        red_skull_at_battle_start(player);
        // AbstractPlayer.preBattlePrep: maxOrbs=0, orbs.clear, then
        // increaseMaxOrbSlots(masterMaxOrbs, false). Capacitor / Consume must not
        // leak slots into the next fight.
        player.orbs.clear();
        player.max_orbs = player.master_max_orbs;
        let mut channeled = Vec::new();
        // Relic.atPreBattle in inventory order: CrackedCore Lightning,
        // SymbioticVirus Dark, NuclearBattery Plasma (seed 32 Spheric 18 vs 15).
        for r in &player.relics {
            if player.orbs.len() >= player.max_orbs as usize {
                break;
            }
            let kind = match r.id {
                RelicId::Cracked_Core => Some(OrbKind::Lightning),
                RelicId::Symbiotic_Virus => Some(OrbKind::Dark),
                RelicId::Nuclear_Battery => Some(OrbKind::Plasma),
                _ => None,
            };
            if let Some(kind) = kind {
                player.orbs.push(Orb { kind, evoke: 0 });
                channeled.push(kind);
            }
        }
        if player.has_relic(RelicId::DataDisk) {
            player.add_power(PowerId::Focus, 1);
        }
        // BronzeScales.atBattleStart: ApplyPowerAction Thorns 3 (addToTop).
        if player.has_relic(RelicId::Bronze_Scales) {
            player.add_power(PowerId::Thorns, 3);
        }
        // MercuryHourglass.atTurnStart: DamageAllEnemiesAction THORNS 3.
        let dead_before_hourglass = monsters.iter().filter(|m| m.dead).count();
        if player.has_relic(RelicId::Mercury_Hourglass) {
            for m in monsters.iter_mut().filter(|m| m.alive()) {
                deal_thorns(m, 3);
            }
        }

        let mut combat = Self {
            encounter,
            monsters,
            turn: 1,
            cards_played_this_turn: 0,
            skills_this_turn: 0,
            attacks_this_turn: 0,
            need_exhaust_select: false,
            need_put_on_deck: false,
            need_discard_to_hand: false,
            need_draw_to_hand: false,
            need_discovery: false,
            need_forethought: false,
            need_skill_from_deck: false,
            skill_from_deck: Vec::new(),
            pending_exhaust: None,
            draw_after_exhaust: 0,
            pending_dark_embrace: 0,
            pending_ink_bottle: 0,
            ascension,
            orbs_channeled_this_combat: channeled,
            energy_on_use: -1,
        };
        gremlin_horn_on_kills(player, &mut combat, rng, dead_before_hourglass);
        combat
    }

    pub fn living(&self) -> impl Iterator<Item = (usize, &Monster)> {
        self.monsters.iter().enumerate().filter(|(_, m)| m.alive())
    }

    pub fn all_dead(&self) -> bool {
        self.monsters.iter().all(|m| !m.alive())
    }

    /// MonsterGroup.showIntent → AbstractMonster.createIntent.
    pub fn publish_intents(&mut self) {
        for monster in &mut self.monsters {
            monster.create_intent();
        }
    }
}

fn is_elite_encounter(id: EncounterId) -> bool {
    matches!(
        id,
        EncounterId::GremlinNob
            | EncounterId::Lagavulin
            | EncounterId::ThreeSentries
            | EncounterId::BookOfStabbing
            | EncounterId::Slavers
            | EncounterId::GremlinLeader
            | EncounterId::GiantHead
    )
}

fn is_boss_encounter(id: EncounterId) -> bool {
    matches!(
        id,
        EncounterId::Hexaghost
            | EncounterId::TheGuardian
            | EncounterId::SlimeBoss
            | EncounterId::Automaton
            | EncounterId::AwakenedOne
            | EncounterId::Champ
            | EncounterId::Collector
            | EncounterId::ShieldAndSpear
            | EncounterId::CorruptHeart
    )
}

fn apply_prebattle(monster: &mut Monster, rng: &mut RngSet) {
    match monster.id {
        MonsterId::LouseNormal | MonsterId::LouseDefensive => {
            let curl = if monster.ascension >= 17 {
                rng.monster_hp.random_range(9, 12)
            } else if monster.ascension >= 7 {
                rng.monster_hp.random_range(4, 8)
            } else {
                rng.monster_hp.random_range(3, 7)
            };
            monster.add_power(PowerId::CurlUp, curl);
        }
        MonsterId::SphericGuardian => {
            monster.add_power(PowerId::Barricade, 1);
            monster.add_power(PowerId::Artifact, 3);
            monster.block += 40;
        }
        MonsterId::SnakePlant => {
            monster.add_power(PowerId::Malleable, 3);
        }
        MonsterId::Byrd => {
            let flight = if monster.ascension >= 17 { 4 } else { 3 };
            monster.add_power(PowerId::Flight, flight);
            monster.extra = 1;
        }
        MonsterId::ShelledParasite => {
            monster.add_power(PowerId::PlatedArmor, 14);
            monster.block += 14;
        }
        MonsterId::FungiBeast => {
            monster.add_power(PowerId::SporeCloud, 2);
        }
        MonsterId::Lagavulin => {
            monster.block += 8;
            monster.add_power(PowerId::Metallicize, 8);
        }
        MonsterId::Sentry => {
            monster.add_power(PowerId::Artifact, 1);
        }
        MonsterId::TheGuardian => {
            // TheGuardian.usePreBattleAction: ModeShiftPower(dmgThreshold).
            // A19: 40, A9: 35, else 30. extra stores the current threshold.
            let thresh = if monster.ascension >= 19 {
                40
            } else if monster.ascension >= 9 {
                35
            } else {
                30
            };
            monster.extra = thresh;
            monster.add_power(PowerId::ModeShift, thresh);
        }
        MonsterId::GremlinWarrior => {
            monster.add_power(PowerId::Angry, if monster.ascension >= 17 { 2 } else { 1 });
        }
        MonsterId::BronzeAutomaton => {
            monster.add_power(PowerId::Artifact, 3);
        }
        MonsterId::CorruptHeart => {
            monster.add_power(PowerId::Artifact, 2);
        }
        MonsterId::BookOfStabbing => {
            monster.add_power(PowerId::PainfulStabs, 1);
        }
        MonsterId::Spiker => {
            monster.add_power(PowerId::Thorns, 3);
        }
        MonsterId::Exploder => {
            monster.add_power(PowerId::Explosive, 3);
        }
        MonsterId::Transient => {
            monster.add_power(PowerId::Fading, 5);
        }
        MonsterId::GiantHead => {
            monster.add_power(PowerId::Slow, 1);
            if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::Slow) {
                p.amount = 0;
            }
        }
        MonsterId::JawWorm => {
            if monster.extra == 1 {
                // hardMode usePreBattleAction: bellowStr / bellowBlock.
                let (str_amt, block_amt) = if monster.ascension >= 17 {
                    (5, 9)
                } else if monster.ascension >= 2 {
                    (4, 6)
                } else {
                    (3, 6)
                };
                monster.add_power(PowerId::Strength, str_amt);
                monster.block += block_amt;
            }
        }
        MonsterId::AwakenedOne => {
            monster.add_power(PowerId::Regen, 10);
            monster.add_power(PowerId::Curiosity, 1);
            monster.add_power(PowerId::Unawakened, -1);
        }
        MonsterId::OrbWalker => {
            monster.add_power(
                PowerId::StrengthUp,
                if monster.ascension >= 17 { 5 } else { 3 },
            );
        }
        _ => {}
    }
}

fn apply_group_move(combat: &mut Combat, idx: usize, id: MonsterId, used_move: i32, rng: &mut RngSet) {
    match (id, used_move) {
        (MonsterId::Healer, 2) => {
            for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
                m.hp = (m.hp + 16).min(m.max_hp);
            }
        }
        (MonsterId::Healer, 3) => {
            for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
                m.add_power(PowerId::Strength, 2);
            }
        }
        (MonsterId::TheCollector, 3) => {
            let str = if combat.ascension >= 19 {
                5
            } else if combat.ascension >= 4 {
                4
            } else {
                3
            };
            for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
                m.add_power(PowerId::Strength, str);
            }
        }
        (MonsterId::Centurion, 2) => {
            let others: Vec<usize> = combat
                .monsters
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != idx && m.alive() && m.intent != Intent::Escape)
                .map(|(j, _)| j)
                .collect();
            let target = if others.is_empty() {
                idx
            } else {
                others[rng.ai.random_int(others.len() as i32 - 1) as usize]
            };
            combat.monsters[target].block += 15;
        }
        (MonsterId::BronzeOrb, 2) => {
            if let Some(auto) = combat
                .monsters
                .iter_mut()
                .find(|m| m.id == MonsterId::BronzeAutomaton && m.alive())
            {
                auto.block += 12;
            }
        }
        (MonsterId::GremlinTsundere, 1) => {
            // GainBlockRandomMonsterAction: aiRng among living non-self, non-ESCAPE.
            let block = if combat.ascension >= 17 {
                11
            } else if combat.ascension >= 7 {
                8
            } else {
                7
            };
            let others: Vec<usize> = combat
                .monsters
                .iter()
                .enumerate()
                .filter(|(j, m)| *j != idx && m.alive() && m.intent != Intent::Escape)
                .map(|(j, _)| j)
                .collect();
            let target = if others.is_empty() {
                idx
            } else {
                others[rng.ai.random_int(others.len() as i32 - 1) as usize]
            };
            combat.monsters[target].block += block;
            // takeTurn then SetMove: protect while another gremlin lives, else bash.
            let alive = combat.monsters.iter().filter(|m| m.alive()).count();
            if alive > 1 {
                combat.monsters[idx].set_move(1, Intent::Defend, 0, 1);
            } else {
                let dmg = if combat.ascension >= 2 { 8 } else { 6 };
                combat.monsters[idx].set_move(2, Intent::Attack, dmg, 1);
            }
        }
        _ => {}
    }
}

fn spawn_encounter(encounter: EncounterId, rng: &mut RngSet, ascension: i32) -> Vec<Monster> {
    match encounter {
        EncounterId::ExordiumThugs => vec![
            spawn_bottom_weak_wildlife(rng, ascension),
            spawn_bottom_strong_humanoid(rng, ascension),
        ],
        EncounterId::ExordiumWildlife => vec![
            spawn_bottom_strong_wildlife(rng, ascension),
            spawn_bottom_weak_wildlife(rng, ascension),
        ],
        other => crate::content::encounter_monsters_rng(other, Some(rng))
            .into_iter()
            .map(|id| spawn_monster(id, rng, ascension))
            .collect(),
    }
}

fn spawn_bottom_weak_wildlife(rng: &mut RngSet, ascension: i32) -> Monster {
    let louse = if rng.misc.random_boolean() {
        MonsterId::LouseNormal
    } else {
        MonsterId::LouseDefensive
    };
    let mut pool = vec![
        spawn_monster(louse, rng, ascension),
        spawn_monster(MonsterId::SpikeSlimeM, rng, ascension),
        spawn_monster(MonsterId::AcidSlimeM, rng, ascension),
    ];
    let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
    pool.remove(idx)
}

fn spawn_bottom_strong_humanoid(rng: &mut RngSet, ascension: i32) -> Monster {
    let cultist = spawn_monster(MonsterId::Cultist, rng, ascension);
    let slaver_id = if rng.misc.random_boolean() {
        MonsterId::SlaverRed
    } else {
        MonsterId::SlaverBlue
    };
    let slaver = spawn_monster(slaver_id, rng, ascension);
    let looter = spawn_monster(MonsterId::Looter, rng, ascension);
    let mut pool = vec![cultist, slaver, looter];
    let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
    pool.remove(idx)
}

fn spawn_bottom_strong_wildlife(rng: &mut RngSet, ascension: i32) -> Monster {
    let mut pool = vec![
        spawn_monster(MonsterId::FungiBeast, rng, ascension),
        spawn_monster(MonsterId::JawWorm, rng, ascension),
    ];
    let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
    pool.remove(idx)
}

fn apply_encounter_misc(_encounter: EncounterId, _rng: &mut RngSet) {
    // Louse / gremlin / slime composition rolls live in encounter_monsters_rng.
}

pub fn spawn_monster(id: MonsterId, rng: &mut RngSet, ascension: i32) -> Monster {
    let (hp_min, hp_max) = hp_range(id, ascension);
    if id == MonsterId::BronzeOrb {
        // Constructor burns monsterHpRng once, then setHp rolls the real range.
        let _ = rng.monster_hp.random_range(hp_min, hp_max);
    }
    if id == MonsterId::OrbWalker {
        // OrbWalker ctor: super(..., monsterHpRng.random(90, 96)) then setHp.
        let _ = rng.monster_hp.random_range(90, 96);
    }
    if id == MonsterId::TorchHead {
        let _ = rng.monster_hp.random_range(38, 40);
    }
    let hp = if id == MonsterId::Maw {
        // Maw ctor passes 300 into super and never calls setHp.
        300
    } else {
        rng.monster_hp.random_range(hp_min, hp_max)
    };
    Monster {
        id,
        hp,
        max_hp: hp,
        block: 0,
        powers: Vec::new(),
        intent: Intent::Debug,
        intent_damage: 0,
        intent_base_damage: -1,
        intent_hits: 1,
        next_move: -1,
        move_history: Vec::new(),
        dead: false,
        escaped: false,
        first_move: true,
        extra: if matches!(id, MonsterId::LouseNormal | MonsterId::LouseDefensive) {
            // Louse constructor: biteDamage = monsterHpRng.random(5, 7) at A0, (6, 8) at A2+.
            if ascension >= 2 {
                rng.monster_hp.random_range(6, 8)
            } else {
                rng.monster_hp.random_range(5, 7)
            }
        } else if id == MonsterId::BookOfStabbing {
            1
        } else if id == MonsterId::Darkling {
            rng.monster_hp.random_range(7, 11)
        } else if id == MonsterId::GiantHead {
            5
        } else if id == MonsterId::GremlinWizard {
            // GremlinWizard.currentCharge field initializer is 1, not 0.
            1
        } else if id == MonsterId::Maw {
            // Maw.turnCount field initializer is 1; getMove increments first.
            1
        } else {
            0
        },
        stolen_gold: 0,
        split_triggered: false,
        stasis_card: None,
        half_dead: false,
        ascension,
        pending_curl: 0,
        offset_x: 0,
        just_spawned: false,
    }
}

fn spawn_monster_at_hp(id: MonsterId, hp: i32, ascension: i32) -> Monster {
    Monster {
        id,
        hp,
        max_hp: hp,
        block: 0,
        powers: Vec::new(),
        intent: Intent::Debug,
        intent_damage: 0,
        intent_base_damage: -1,
        intent_hits: 1,
        next_move: -1,
        move_history: Vec::new(),
        dead: false,
        escaped: false,
        first_move: true,
        extra: 0,
        stolen_gold: 0,
        split_triggered: false,
        stasis_card: None,
        half_dead: false,
        // Split constructors pass currentHealth but keep AbstractDungeon.ascensionLevel.
        ascension,
        pending_curl: 0,
        offset_x: 0,
        just_spawned: false,
    }
}

fn hp_range(id: MonsterId, ascension: i32) -> (i32, i32) {
    let a7 = ascension >= 7;
    let a9 = ascension >= 9;
    match id {
        MonsterId::Cultist => {
            if a7 {
                (50, 56)
            } else {
                (48, 54)
            }
        }
        MonsterId::JawWorm => {
            if a7 {
                (42, 46)
            } else {
                (40, 44)
            }
        }
        MonsterId::AcidSlimeS => {
            if a7 {
                (9, 13)
            } else {
                (8, 12)
            }
        }
        MonsterId::AcidSlimeM => {
            if a7 {
                (29, 34)
            } else {
                (28, 32)
            }
        }
        MonsterId::AcidSlimeL => {
            if a7 {
                (68, 72)
            } else {
                (65, 69)
            }
        }
        MonsterId::SpikeSlimeS => {
            if a7 {
                (11, 15)
            } else {
                (10, 14)
            }
        }
        MonsterId::SpikeSlimeM => {
            if a7 {
                (29, 34)
            } else {
                (28, 32)
            }
        }
        MonsterId::SpikeSlimeL => {
            if a7 {
                (67, 73)
            } else {
                (64, 70)
            }
        }
        MonsterId::LouseNormal => {
            if a7 {
                (11, 16)
            } else {
                (10, 15)
            }
        }
        MonsterId::LouseDefensive => {
            if a7 {
                (12, 18)
            } else {
                (11, 17)
            }
        }
        MonsterId::FungiBeast => {
            if a7 {
                (24, 28)
            } else {
                (22, 28)
            }
        }
        MonsterId::SlaverBlue | MonsterId::SlaverRed => {
            if a7 {
                (48, 52)
            } else {
                (46, 50)
            }
        }
        MonsterId::Looter => {
            if a7 {
                (46, 50)
            } else {
                (44, 48)
            }
        }
        MonsterId::Mugger => {
            if a7 {
                (50, 54)
            } else {
                (48, 52)
            }
        }
        MonsterId::GremlinNob => {
            if ascension >= 8 {
                (85, 90)
            } else {
                (82, 86)
            }
        }
        MonsterId::Lagavulin => {
            if ascension >= 8 {
                (112, 115)
            } else {
                (109, 111)
            }
        }
        MonsterId::Sentry => {
            if ascension >= 8 {
                (39, 45)
            } else {
                (38, 42)
            }
        }
        MonsterId::GremlinFat => {
            if a7 {
                (14, 18)
            } else {
                (13, 17)
            }
        }
        MonsterId::GremlinTsundere => {
            if a7 {
                (13, 17)
            } else {
                (12, 15)
            }
        }
        MonsterId::GremlinThief => {
            if a7 {
                (11, 15)
            } else {
                (10, 14)
            }
        }
        MonsterId::GremlinWarrior => {
            if a7 {
                (21, 25)
            } else {
                (20, 24)
            }
        }
        MonsterId::GremlinWizard => {
            if a7 {
                (22, 26)
            } else {
                (21, 25)
            }
        }
        MonsterId::Hexaghost => {
            if a9 {
                (264, 264)
            } else {
                (250, 250)
            }
        }
        MonsterId::TheGuardian => {
            if a9 {
                (250, 250)
            } else {
                (240, 240)
            }
        }
        MonsterId::SlimeBoss => {
            if a9 {
                (150, 150)
            } else {
                (140, 140)
            }
        }
        MonsterId::SphericGuardian => (20, 20),
        MonsterId::Chosen => (95, 99),
        MonsterId::Centurion => (76, 80),
        MonsterId::Healer => (48, 56),
        MonsterId::SnakePlant => (75, 79),
        MonsterId::ShelledParasite => (68, 72),
        MonsterId::BronzeAutomaton => (300, 300),
        MonsterId::BronzeOrb => (52, 58),
        MonsterId::SpireShield => (110, 110),
        MonsterId::SpireSpear => (160, 160),
        MonsterId::CorruptHeart => (750, 750),
        MonsterId::BookOfStabbing => (160, 164),
        MonsterId::Spiker => (42, 56),
        MonsterId::Exploder => (30, 30),
        MonsterId::Repulsor => (29, 35),
        MonsterId::Darkling => (48, 56),
        MonsterId::Transient => (999, 999),
        MonsterId::GiantHead => (500, 500),
        MonsterId::AwakenedOne => (300, 300),
        MonsterId::Snecko => {
            if a7 {
                (120, 125)
            } else {
                (114, 120)
            }
        }
        MonsterId::Byrd => {
            if a7 {
                (26, 33)
            } else {
                (25, 31)
            }
        }
        MonsterId::Champ => {
            if a9 {
                (440, 440)
            } else {
                (420, 420)
            }
        }
        MonsterId::OrbWalker => {
            if a7 {
                (92, 102)
            } else {
                (90, 96)
            }
        }
        MonsterId::Maw => (300, 300),
        MonsterId::TheCollector => {
            if a9 {
                (300, 300)
            } else {
                (282, 282)
            }
        }
        MonsterId::TorchHead => {
            if a9 {
                (40, 45)
            } else {
                (38, 40)
            }
        }
        _ => (40, 44),
    }
}

impl Monster {
    pub fn roll_move(&mut self, rng: &mut RngSet) {
        self.roll_move_group(rng, 0, 1, 0);
    }

    pub fn roll_move_group(&mut self, rng: &mut RngSet, missing_hp: i32, allies: i32, index: i32) {
        let roll = rng.ai.random_int(99);
        self.get_move(roll, rng, missing_hp, allies, index);
    }

    fn get_move(&mut self, num: i32, rng: &mut RngSet, missing_hp: i32, allies: i32, index: i32) {
        match self.id {
            MonsterId::Cultist => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(3, Intent::Buff, 0, 1);
                } else {
                    self.set_move(1, Intent::Attack, 6, 1);
                }
            }
            MonsterId::JawWorm => {
                // A0 chomp 11, A2/A17 chomp 12. Thrash is 7 at all ascensions.
                let chomp = if self.ascension >= 2 { 12 } else { 11 };
                if self.first_move {
                    self.first_move = false;
                    self.set_move(1, Intent::Attack, chomp, 1);
                } else if num < 25 {
                    if self.last_move(1) {
                        if rng.ai.random_boolean_chance(0.5625) {
                            self.set_move(2, Intent::DefendBuff, 0, 1);
                        } else {
                            self.set_move(3, Intent::AttackDefend, 7, 1);
                        }
                    } else {
                        self.set_move(1, Intent::Attack, chomp, 1);
                    }
                } else if num < 55 {
                    if self.last_two(3) {
                        if rng.ai.random_boolean_chance(0.357) {
                            self.set_move(1, Intent::Attack, chomp, 1);
                        } else {
                            self.set_move(2, Intent::DefendBuff, 0, 1);
                        }
                    } else {
                        self.set_move(3, Intent::AttackDefend, 7, 1);
                    }
                } else if self.last_move(2) {
                    if rng.ai.random_boolean_chance(0.416) {
                        self.set_move(1, Intent::Attack, chomp, 1);
                    } else {
                        self.set_move(3, Intent::AttackDefend, 7, 1);
                    }
                } else {
                    self.set_move(2, Intent::DefendBuff, 0, 1);
                }
            }
            MonsterId::SlaverBlue => {
                let stab = if self.ascension >= 2 { 13 } else { 12 };
                let rake = if self.ascension >= 2 { 8 } else { 7 };
                if num >= 40 && !self.last_two(1) {
                    self.set_move(1, Intent::Attack, stab, 1);
                } else if self.ascension >= 17 {
                    if !self.last_move(4) {
                        self.set_move(4, Intent::AttackDebuff, rake, 1);
                    } else {
                        self.set_move(1, Intent::Attack, stab, 1);
                    }
                } else if !self.last_two(4) {
                    self.set_move(4, Intent::AttackDebuff, rake, 1);
                } else {
                    self.set_move(1, Intent::Attack, stab, 1);
                }
            }
            MonsterId::SlaverRed => {
                let stab = if self.ascension >= 2 { 14 } else { 13 };
                let scrape = if self.ascension >= 2 { 9 } else { 8 };
                let used_entangle = self.extra != 0;
                if self.first_move {
                    self.first_move = false;
                    self.set_move(1, Intent::Attack, stab, 1);
                } else if num >= 75 && !used_entangle {
                    self.set_move(2, Intent::StrongDebuff, 0, 1);
                } else if num >= 55 && used_entangle && !self.last_two(1) {
                    self.set_move(1, Intent::Attack, stab, 1);
                } else if self.ascension >= 17 {
                    if !self.last_move(3) {
                        self.set_move(3, Intent::AttackDebuff, scrape, 1);
                    } else {
                        self.set_move(1, Intent::Attack, stab, 1);
                    }
                } else if !self.last_two(3) {
                    self.set_move(3, Intent::AttackDebuff, scrape, 1);
                } else {
                    self.set_move(1, Intent::Attack, stab, 1);
                }
            }
            MonsterId::Looter => {
                self.set_move(1, Intent::Attack, 10, 1);
            }
            MonsterId::Mugger => {
                let swipe = if self.ascension >= 2 { 11 } else { 10 };
                self.set_move(1, Intent::Attack, swipe, 1);
            }
            MonsterId::AcidSlimeS => {
                let dmg = if self.ascension >= 2 { 4 } else { 3 };
                if self.ascension >= 17 {
                    if self.last_two(1) {
                        self.set_move(1, Intent::Attack, dmg, 1);
                    } else {
                        self.set_move(2, Intent::Debuff, 0, 1);
                    }
                } else if rng.ai.random_boolean() {
                    self.set_move(1, Intent::Attack, dmg, 1);
                } else {
                    self.set_move(2, Intent::Debuff, 0, 1);
                }
            }
            MonsterId::AcidSlimeM => {
                acid_slime_m_move(self, num, rng, 7, 10);
            }
            MonsterId::AcidSlimeL => {
                if self.hp <= self.max_hp / 2 && !self.split_triggered {
                    self.set_move(3, Intent::Unknown, 0, 1);
                } else {
                    acid_slime_l_move(self, num, rng);
                }
            }
            MonsterId::SpikeSlimeL => {
                if self.hp <= self.max_hp / 2 && !self.split_triggered {
                    self.set_move(3, Intent::Unknown, 0, 1);
                } else {
                    // SpikeSlime_L.getMove: A17 uses lastMove(4) in the >=30 branch;
                    // Flame Tackle 18 at A2+ (Java A_2_TACKLE_DAMAGE).
                    let dmg = if self.ascension >= 2 { 18 } else { 16 };
                    if self.ascension >= 17 {
                        if num < 30 {
                            if self.last_two(1) {
                                self.set_move(4, Intent::Debuff, 0, 1);
                            } else {
                                self.set_move(1, Intent::AttackDebuff, dmg, 1);
                            }
                        } else if self.last_move(4) {
                            self.set_move(1, Intent::AttackDebuff, dmg, 1);
                        } else {
                            self.set_move(4, Intent::Debuff, 0, 1);
                        }
                    } else if num < 30 {
                        if self.last_two(1) {
                            self.set_move(4, Intent::Debuff, 0, 1);
                        } else {
                            self.set_move(1, Intent::AttackDebuff, dmg, 1);
                        }
                    } else if self.last_two(4) {
                        self.set_move(1, Intent::AttackDebuff, dmg, 1);
                    } else {
                        self.set_move(4, Intent::Debuff, 0, 1);
                    }
                }
            }
            MonsterId::SpikeSlimeS => self.set_move(1, Intent::Attack, if self.ascension >= 2 { 6 } else { 5 }, 1),
            MonsterId::SpikeSlimeM => {
                let dmg = if self.ascension >= 2 { 10 } else { 8 };
                if self.ascension >= 17 {
                    if num < 30 {
                        if self.last_two(1) {
                            self.set_move(4, Intent::Debuff, 0, 1);
                        } else {
                            self.set_move(1, Intent::AttackDebuff, dmg, 1);
                        }
                    } else if self.last_move(4) {
                        self.set_move(1, Intent::AttackDebuff, dmg, 1);
                    } else {
                        self.set_move(4, Intent::Debuff, 0, 1);
                    }
                } else if num < 30 {
                    if self.last_two(1) {
                        self.set_move(4, Intent::Debuff, 0, 1);
                    } else {
                        self.set_move(1, Intent::AttackDebuff, dmg, 1);
                    }
                } else if self.last_two(4) {
                    self.set_move(1, Intent::AttackDebuff, dmg, 1);
                } else {
                    self.set_move(4, Intent::Debuff, 0, 1);
                }
            }
            MonsterId::SphericGuardian => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(2, Intent::Defend, 0, 1);
                } else if self.extra == 0 {
                    self.extra = 1;
                    self.set_move(4, Intent::AttackDebuff, 10, 1);
                } else if self.last_move(1) {
                    self.set_move(3, Intent::AttackDefend, 10, 1);
                } else {
                    self.set_move(1, Intent::Attack, 10, 2);
                }
            }
            MonsterId::Chosen => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(5, Intent::Attack, 5, 2);
                } else if !self.split_triggered {
                    self.split_triggered = true;
                    self.set_move(4, Intent::StrongDebuff, 0, 1);
                } else if !self.last_move(3) && !self.last_move(2) {
                    if num < 50 {
                        self.set_move(3, Intent::AttackDebuff, 10, 1);
                    } else {
                        self.set_move(2, Intent::Debuff, 0, 1);
                    }
                } else if num < 40 {
                    self.set_move(1, Intent::Attack, 18, 1);
                } else {
                    self.set_move(5, Intent::Attack, 5, 2);
                }
            }
            MonsterId::SnakePlant => {
                if num < 65 {
                    if self.last_two(1) {
                        self.set_move(2, Intent::StrongDebuff, 0, 1);
                    } else {
                        self.set_move(1, Intent::Attack, 7, 3);
                    }
                } else if self.last_move(2) {
                    self.set_move(1, Intent::Attack, 7, 3);
                } else {
                    self.set_move(2, Intent::StrongDebuff, 0, 1);
                }
            }
            MonsterId::Centurion => {
                if num >= 65 && !self.last_two(2) && !self.last_two(3) {
                    if allies > 1 {
                        self.set_move(2, Intent::Defend, 0, 1);
                    } else {
                        self.set_move(3, Intent::Attack, 6, 3);
                    }
                } else if !self.last_two(1) {
                    self.set_move(1, Intent::Attack, 12, 1);
                } else if allies > 1 {
                    self.set_move(2, Intent::Defend, 0, 1);
                } else {
                    self.set_move(3, Intent::Attack, 6, 3);
                }
            }
            MonsterId::Healer => {
                if missing_hp > 15 && !self.last_two(2) {
                    self.set_move(2, Intent::Buff, 0, 1);
                } else if num >= 40 && !self.last_two(1) {
                    self.set_move(1, Intent::AttackDebuff, 8, 1);
                } else if !self.last_two(3) {
                    self.set_move(3, Intent::Buff, 0, 1);
                } else {
                    self.set_move(1, Intent::AttackDebuff, 8, 1);
                }
            }
            MonsterId::ShelledParasite => {
                // FELL=1 dmg 18+frail; DOUBLE_STRIKE=2 dmg 6x2; LIFE_SUCK=3 dmg 10 + vampire
                if self.first_move {
                    self.first_move = false;
                    if rng.ai.random_boolean() {
                        self.set_move(2, Intent::Attack, 6, 2);
                    } else {
                        self.set_move(3, Intent::AttackBuff, 10, 1);
                    }
                } else if num < 20 {
                    if !self.last_move(1) {
                        self.set_move(1, Intent::AttackDebuff, 18, 1);
                    } else {
                        let retry = rng.ai.random_range(20, 99);
                        self.get_move(retry, rng, missing_hp, allies, index);
                    }
                } else if num < 60 {
                    if !self.last_two(2) {
                        self.set_move(2, Intent::Attack, 6, 2);
                    } else {
                        self.set_move(3, Intent::AttackBuff, 10, 1);
                    }
                } else if !self.last_two(3) {
                    self.set_move(3, Intent::AttackBuff, 10, 1);
                } else {
                    self.set_move(2, Intent::Attack, 6, 2);
                }
            }
            MonsterId::FungiBeast => {
                if num < 60 {
                    if self.last_two(1) {
                        self.set_move(2, Intent::Buff, 0, 1);
                    } else {
                        self.set_move(1, Intent::Attack, 6, 1);
                    }
                } else if self.last_move(2) {
                    self.set_move(1, Intent::Attack, 6, 1);
                } else {
                    self.set_move(2, Intent::Buff, 0, 1);
                }
            }
            MonsterId::BronzeAutomaton => {
                // 1 flail 7x2, 2 hyper beam 45, 3 stun, 4 spawn orbs, 5 boost
                if self.first_move {
                    self.first_move = false;
                    self.set_move(4, Intent::Unknown, 0, 1);
                } else if self.extra == 4 {
                    self.set_move(2, Intent::Attack, 45, 1);
                    self.extra = 0;
                } else if self.last_move(2) {
                    self.set_move(3, Intent::Stun, 0, 1);
                } else {
                    if !self.last_move(3) && !self.last_move(5) && !self.last_move(4) {
                        self.set_move(5, Intent::DefendBuff, 0, 1);
                    } else {
                        self.set_move(1, Intent::Attack, 7, 2);
                    }
                    self.extra += 1;
                }
            }
            MonsterId::BronzeOrb => {
                if self.extra == 0 && num >= 25 {
                    self.set_move(3, Intent::StrongDebuff, 0, 1);
                    self.extra = 1;
                } else if num >= 70 && !self.last_two(2) {
                    self.set_move(2, Intent::Defend, 0, 1);
                } else if !self.last_two(1) {
                    self.set_move(1, Intent::Attack, 8, 1);
                } else {
                    self.set_move(2, Intent::Defend, 0, 1);
                }
            }
            MonsterId::SpireShield => {
                if num < 50 {
                    self.set_move(1, Intent::Attack, 12, 1);
                } else {
                    self.set_move(2, Intent::Defend, 0, 1);
                }
            }
            MonsterId::SpireSpear => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(1, Intent::Attack, 5, 2);
                } else if num < 50 {
                    self.set_move(2, Intent::Debuff, 0, 1);
                } else {
                    self.set_move(1, Intent::Attack, 5, 2);
                }
            }
            MonsterId::CorruptHeart => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(3, Intent::StrongDebuff, 0, 1);
                } else if self.extra % 3 == 0 {
                    self.set_move(1, Intent::Attack, 2, 12);
                } else if self.extra % 3 == 1 {
                    self.set_move(2, Intent::Attack, 40, 1);
                } else {
                    self.set_move(4, Intent::Buff, 0, 1);
                }
            }
            MonsterId::Hexaghost => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(5, Intent::Unknown, 0, 1); // Activate
                } else {
                    let tackle = if self.ascension >= 4 { 6 } else { 5 };
                    let inferno = if self.ascension >= 4 { 3 } else { 2 };
                    match self.extra {
                        0 => self.set_move(4, Intent::AttackDebuff, 6, 1), // Sear
                        1 => self.set_move(2, Intent::Attack, tackle, 2),
                        2 => self.set_move(4, Intent::AttackDebuff, 6, 1),
                        3 => self.set_move(3, Intent::DefendBuff, 0, 1), // Inflame
                        4 => self.set_move(2, Intent::Attack, tackle, 2),
                        5 => self.set_move(4, Intent::AttackDebuff, 6, 1),
                        _ => self.set_move(6, Intent::AttackDebuff, inferno, 6),
                    }
                }
            }
            MonsterId::BookOfStabbing => {
                if num < 15 {
                    if self.last_move(2) {
                        self.extra += 1;
                        self.set_move(1, Intent::Attack, 6, self.extra);
                    } else {
                        self.set_move(2, Intent::Attack, 21, 1);
                    }
                } else if self.last_two(1) {
                    self.set_move(2, Intent::Attack, 21, 1);
                } else {
                    self.extra += 1;
                    self.set_move(1, Intent::Attack, 6, self.extra);
                }
            }
            MonsterId::Spiker => {
                // extra = thornsCount (buff uses). After 6 buffs, only attack.
                if self.extra > 5 {
                    self.set_move(1, Intent::Attack, 7, 1);
                } else if num < 50 && !self.last_move(1) {
                    self.set_move(1, Intent::Attack, 7, 1);
                } else {
                    self.set_move(2, Intent::Buff, 0, 1);
                }
            }
            MonsterId::Exploder => {
                // extra = turnCount, incremented in take_turn.
                if self.extra < 2 {
                    self.set_move(1, Intent::Attack, 9, 1);
                } else {
                    self.set_move(2, Intent::Unknown, 0, 1);
                }
            }
            MonsterId::Repulsor => {
                if num < 20 && !self.last_move(2) {
                    self.set_move(2, Intent::Attack, 11, 1);
                } else {
                    self.set_move(1, Intent::Debuff, 0, 1);
                }
            }
            MonsterId::Transient => {
                self.set_move(1, Intent::Attack, 30 + self.extra * 10, 1);
            }
            MonsterId::SlimeBoss => {
                if self.first_move {
                    self.first_move = false;
                    self.set_move(4, Intent::StrongDebuff, 0, 1);
                }
            }
            MonsterId::GiantHead => {
                if self.extra <= 1 {
                    if self.extra > -6 {
                        self.extra -= 1;
                    }
                    self.set_move(2, Intent::Attack, 30 - self.extra * 5, 1);
                } else {
                    self.extra -= 1;
                    if num < 50 {
                        if !self.last_two(1) {
                            self.set_move(1, Intent::Debuff, 0, 1);
                        } else {
                            self.set_move(3, Intent::Attack, 13, 1);
                        }
                    } else if !self.last_two(3) {
                        self.set_move(3, Intent::Attack, 13, 1);
                    } else {
                        self.set_move(1, Intent::Debuff, 0, 1);
                    }
                }
            }
            MonsterId::AwakenedOne => {
                if self.extra == 0 {
                    if self.first_move {
                        self.first_move = false;
                        self.set_move(1, Intent::Attack, 20, 1);
                    } else if num < 25 {
                        if !self.last_move(2) {
                            self.set_move(2, Intent::Attack, 6, 4);
                        } else {
                            self.set_move(1, Intent::Attack, 20, 1);
                        }
                    } else if !self.last_two(1) {
                        self.set_move(1, Intent::Attack, 20, 1);
                    } else {
                        self.set_move(2, Intent::Attack, 6, 4);
                    }
                } else if self.first_move {
                    self.set_move(5, Intent::Attack, 40, 1);
                } else if num < 50 {
                    if !self.last_two(6) {
                        self.set_move(6, Intent::AttackDebuff, 18, 1);
                    } else {
                        self.set_move(8, Intent::Attack, 10, 3);
                    }
                } else if !self.last_two(8) {
                    self.set_move(8, Intent::Attack, 10, 3);
                } else {
                    self.set_move(6, Intent::AttackDebuff, 18, 1);
                }
            }
            MonsterId::GremlinFat => {
                let dmg = if self.ascension >= 2 { 5 } else { 4 };
                self.set_move(2, Intent::AttackDebuff, dmg, 1);
            }
            MonsterId::GremlinWarrior => {
                let dmg = if self.ascension >= 2 { 5 } else { 4 };
                self.set_move(1, Intent::Attack, dmg, 1);
            }
            MonsterId::GremlinThief => {
                let dmg = if self.ascension >= 2 { 10 } else { 9 };
                self.set_move(1, Intent::Attack, dmg, 1);
            }
            MonsterId::GremlinTsundere => {
                self.set_move(1, Intent::Defend, 0, 1);
            }
            MonsterId::GremlinWizard => {
                // getMove always CHARGE; attack is only set from takeTurn.
                self.set_move(2, Intent::Unknown, 0, 1);
            }
            MonsterId::Lagavulin => {
                if self.extra < 3 {
                    self.set_move(5, Intent::Sleep, 0, 1);
                } else {
                    let atk = if self.ascension >= 3 { 20 } else { 18 };
                    let debuff_turns = self.extra - 3;
                    if debuff_turns < 2 {
                        if self.last_two(3) {
                            self.set_move(1, Intent::StrongDebuff, 0, 1);
                        } else {
                            self.set_move(3, Intent::Attack, atk, 1);
                        }
                    } else {
                        self.set_move(1, Intent::StrongDebuff, 0, 1);
                    }
                }
            }
            MonsterId::GremlinNob => {
                let rush = if self.ascension >= 3 { 16 } else { 14 };
                let bash = if self.ascension >= 3 { 8 } else { 6 };
                if self.first_move {
                    self.first_move = false;
                    self.set_move(3, Intent::Buff, 0, 1);
                } else if self.ascension >= 18 {
                    if !self.last_move(2) {
                        self.set_move(2, Intent::AttackDebuff, bash, 1);
                    } else if self.last_two(1) {
                        self.set_move(2, Intent::AttackDebuff, bash, 1);
                    } else {
                        self.set_move(1, Intent::Attack, rush, 1);
                    }
                } else if num < 33 {
                    self.set_move(2, Intent::AttackDebuff, bash, 1);
                } else if self.last_two(1) {
                    self.set_move(2, Intent::AttackDebuff, bash, 1);
                } else {
                    self.set_move(1, Intent::Attack, rush, 1);
                }
            }
            MonsterId::LouseNormal | MonsterId::LouseDefensive => {
                let bite = self.extra.max(5);
                let grow = self.id != MonsterId::LouseDefensive;
                let set_debuff_or_grow = |m: &mut Monster| {
                    if grow {
                        m.set_move(4, Intent::Buff, 0, 1);
                    } else {
                        m.set_move(4, Intent::Debuff, 0, 1);
                    }
                };
                if self.ascension >= 17 {
                    if num < 25 {
                        if self.last_move(4) {
                            self.set_move(3, Intent::Attack, bite, 1);
                        } else {
                            set_debuff_or_grow(self);
                        }
                    } else if self.last_two(3) {
                        set_debuff_or_grow(self);
                    } else {
                        self.set_move(3, Intent::Attack, bite, 1);
                    }
                } else if num < 25 {
                    if self.last_two(4) {
                        self.set_move(3, Intent::Attack, bite, 1);
                    } else {
                        set_debuff_or_grow(self);
                    }
                } else if self.last_two(3) {
                    set_debuff_or_grow(self);
                } else {
                    self.set_move(3, Intent::Attack, bite, 1);
                }
            }
            MonsterId::Sentry => {
                // Sentry.getMove: first turn lastIndexOf % 2 == 0 -> BOLT else BEAM; then alternates.
                let beam = if self.ascension >= 3 { 10 } else { 9 };
                if self.first_move {
                    self.first_move = false;
                    if index % 2 == 0 {
                        self.set_move(3, Intent::Debuff, 0, 1);
                    } else {
                        self.set_move(4, Intent::Attack, beam, 1);
                    }
                } else if self.last_move(4) {
                    self.set_move(3, Intent::Debuff, 0, 1);
                } else {
                    self.set_move(4, Intent::Attack, beam, 1);
                }
            }
            MonsterId::Darkling => {
                if self.half_dead {
                    self.set_move(5, Intent::Buff, 0, 1);
                } else if self.first_move {
                    self.first_move = false;
                    if num < 50 {
                        self.set_move(2, Intent::Defend, 0, 1);
                    } else {
                        self.set_move(3, Intent::Attack, self.extra.max(7), 1);
                    }
                } else if num < 40 {
                    if !self.last_move(1) && index % 2 == 0 {
                        self.set_move(1, Intent::Attack, 8, 2);
                    } else {
                        let reroll = rng.ai.random_range(40, 99);
                        self.get_move(reroll, rng, missing_hp, allies, index);
                    }
                } else if num < 70 {
                    if !self.last_move(2) {
                        self.set_move(2, Intent::Defend, 0, 1);
                    } else {
                        self.set_move(3, Intent::Attack, self.extra.max(7), 1);
                    }
                } else if !self.last_two(3) {
                    self.set_move(3, Intent::Attack, self.extra.max(7), 1);
                } else {
                    let reroll = rng.ai.random_int(99);
                    self.get_move(reroll, rng, missing_hp, allies, index);
                }
            }
            MonsterId::TheGuardian => {
                // TheGuardian.getMove: isOpen -> CHARGE_UP else ROLL_ATTACK.
                // split_triggered is closeUpTriggered / !isOpen.
                if !self.split_triggered {
                    self.set_move(6, Intent::Defend, 0, 1);
                } else {
                    let roll = if self.ascension >= 4 { 10 } else { 9 };
                    self.set_move(3, Intent::Attack, roll, 1);
                }
            }
            MonsterId::Snecko => {
                let bite = if self.ascension >= 2 { 18 } else { 15 };
                let tail = if self.ascension >= 2 { 10 } else { 8 };
                if self.first_move {
                    self.first_move = false;
                    self.set_move(1, Intent::StrongDebuff, 0, 1);
                } else if num < 40 {
                    self.set_move(3, Intent::AttackDebuff, tail, 1);
                } else if self.last_two(2) {
                    self.set_move(3, Intent::AttackDebuff, tail, 1);
                } else {
                    self.set_move(2, Intent::Attack, bite, 1);
                }
            }
            MonsterId::Byrd => {
                let peck = 1;
                let peck_n = if self.ascension >= 2 { 6 } else { 5 };
                let swoop = if self.ascension >= 2 { 14 } else { 12 };
                if self.extra == 0 {
                    self.set_move(5, Intent::Attack, 3, 1);
                } else if self.first_move {
                    self.first_move = false;
                    if rng.ai.random_boolean_chance(0.375) {
                        self.set_move(6, Intent::Buff, 0, 1);
                    } else {
                        self.set_move(1, Intent::Attack, peck, peck_n);
                    }
                } else if num < 50 {
                    if self.last_two(1) {
                        if rng.ai.random_boolean_chance(0.4) {
                            self.set_move(3, Intent::Attack, swoop, 1);
                        } else {
                            self.set_move(6, Intent::Buff, 0, 1);
                        }
                    } else {
                        self.set_move(1, Intent::Attack, peck, peck_n);
                    }
                } else if num < 70 {
                    if self.last_move(3) {
                        if rng.ai.random_boolean_chance(0.375) {
                            self.set_move(6, Intent::Buff, 0, 1);
                        } else {
                            self.set_move(1, Intent::Attack, peck, peck_n);
                        }
                    } else {
                        self.set_move(3, Intent::Attack, swoop, 1);
                    }
                } else if self.last_move(6) {
                    if rng.ai.random_boolean_chance(0.2857) {
                        self.set_move(3, Intent::Attack, swoop, 1);
                    } else {
                        self.set_move(1, Intent::Attack, peck, peck_n);
                    }
                } else {
                    self.set_move(6, Intent::Buff, 0, 1);
                }
            }
            MonsterId::Champ => {
                // extra: numTurns in low 8 bits, forgeTimes in the rest.
                // split_triggered = thresholdReached (HP < max/2 anger).
                let mut num_turns = (self.extra & 0xFF) + 1;
                let mut forge_times = self.extra >> 8;
                let slash = if self.ascension >= 4 { 18 } else { 16 };
                let slap = if self.ascension >= 4 { 14 } else { 12 };
                let stance_roll = if self.ascension >= 19 { 30 } else { 15 };
                if self.hp < self.max_hp / 2 && !self.split_triggered {
                    self.split_triggered = true;
                    self.set_move(7, Intent::Buff, 0, 1);
                } else if !self.last_move(3)
                    && !self.last_move_before(3)
                    && self.split_triggered
                {
                    self.set_move(3, Intent::Attack, 10, 2);
                } else if num_turns == 4 && !self.split_triggered {
                    self.set_move(6, Intent::Debuff, 0, 1);
                    num_turns = 0;
                } else if !self.last_move(2) && forge_times < 2 && num <= stance_roll {
                    forge_times += 1;
                    self.set_move(2, Intent::DefendBuff, 0, 1);
                } else if !self.last_move(5) && !self.last_move(2) && num <= 30 {
                    self.set_move(5, Intent::Buff, 0, 1);
                } else if !self.last_move(4) && num <= 55 {
                    self.set_move(4, Intent::AttackDebuff, slap, 1);
                } else if !self.last_move(1) {
                    self.set_move(1, Intent::Attack, slash, 1);
                } else {
                    self.set_move(4, Intent::AttackDebuff, slap, 1);
                }
                self.extra = (forge_times << 8) | (num_turns & 0xFF);
            }
            MonsterId::OrbWalker => {
                let laser = if self.ascension >= 2 { 11 } else { 10 };
                let claw = if self.ascension >= 2 { 16 } else { 15 };
                if num < 40 {
                    if !self.last_two(2) {
                        self.set_move(2, Intent::Attack, claw, 1);
                    } else {
                        self.set_move(1, Intent::AttackDebuff, laser, 1);
                    }
                } else if !self.last_two(1) {
                    self.set_move(1, Intent::AttackDebuff, laser, 1);
                } else {
                    self.set_move(2, Intent::Attack, claw, 1);
                }
            }
            MonsterId::Maw => {
                // extra = turnCount (starts 1). split_triggered = roared.
                self.extra += 1;
                let slam = if self.ascension >= 2 { 30 } else { 25 };
                if !self.split_triggered {
                    self.set_move(2, Intent::StrongDebuff, 0, 1);
                } else if num < 50 && !self.last_move(5) {
                    let hits = (self.extra / 2).max(1);
                    self.set_move(5, Intent::Attack, 5, hits);
                } else if !self.last_move(3) && !self.last_move(5) {
                    self.set_move(3, Intent::Attack, slam, 1);
                } else {
                    self.set_move(4, Intent::Buff, 0, 1);
                }
            }
            MonsterId::TheCollector => {
                let fire = if self.ascension >= 4 { 21 } else { 18 };
                if self.first_move {
                    self.set_move(1, Intent::Unknown, 0, 1);
                } else if self.extra >= 3 && !self.split_triggered {
                    self.set_move(4, Intent::StrongDebuff, 0, 1);
                } else if num <= 70 && !self.last_two(2) {
                    self.set_move(2, Intent::Attack, fire, 1);
                } else if !self.last_move(3) {
                    self.set_move(3, Intent::DefendBuff, 0, 1);
                } else {
                    self.set_move(2, Intent::Attack, fire, 1);
                }
            }
            MonsterId::TorchHead => {
                self.set_move(1, Intent::Attack, 7, 1);
            }
            _ => self.set_move(1, Intent::Attack, 6, 1),
        }
    }

    fn set_move(&mut self, move_id: i32, intent: Intent, damage: i32, hits: i32) {
        self.next_move = move_id;
        self.intent = intent;
        self.intent_damage = damage;
        self.intent_hits = hits;
        self.move_history.push(move_id);
    }

    pub fn create_intent(&mut self) {
        // AbstractMonster.createIntent: copies move.baseDamage into intentBaseDmg.
        self.intent_base_damage = match self.intent {
            Intent::Attack | Intent::AttackBuff | Intent::AttackDebuff | Intent::AttackDefend => {
                self.intent_damage
            }
            _ => -1,
        };
    }

    fn last_move(&self, move_id: i32) -> bool {
        self.move_history.last() == Some(&move_id)
    }

    fn last_move_before(&self, move_id: i32) -> bool {
        self.move_history.len() >= 2 && self.move_history[self.move_history.len() - 2] == move_id
    }

    fn last_two(&self, move_id: i32) -> bool {
        self.move_history.len() >= 2
            && self.move_history[self.move_history.len() - 1] == move_id
            && self.move_history[self.move_history.len() - 2] == move_id
    }

    fn skip_roll_after_turn(&self) -> bool {
        matches!(
            self.id,
            MonsterId::AcidSlimeS
                | MonsterId::Looter
                | MonsterId::Mugger
                | MonsterId::Transient
                | MonsterId::SlimeBoss
                | MonsterId::GremlinWarrior
                | MonsterId::GremlinWizard
                | MonsterId::GremlinThief
                | MonsterId::GremlinTsundere
        )
            || (self.id == MonsterId::Hexaghost && self.next_move == 5)
            || (self.id == MonsterId::Byrd && self.next_move == 5)
            || (matches!(self.id, MonsterId::AcidSlimeL) && self.next_move == 3)
            // TheGuardian.takeTurn setMoves the next intent; no RollMoveAction.
            || self.id == MonsterId::TheGuardian
            // TorchHead.takeTurn SetMoveAction(TACKLE) instead of RollMoveAction.
            || self.id == MonsterId::TorchHead
    }

    pub fn take_turn(&mut self, player: &mut Player, rng: &mut RngSet, ascension: i32) -> Option<Vec<Monster>> {
        if !self.alive() {
            return None;
        }
        match (self.id, self.next_move) {
            (MonsterId::LouseNormal | MonsterId::LouseDefensive, 3) => {
                let _ = hit_player(player, self, rng, self.extra.max(5), 1);
            }
            (MonsterId::LouseNormal, 4) => {
                self.add_power(PowerId::Strength, if ascension >= 17 { 4 } else { 3 });
            }
            (MonsterId::LouseDefensive, 4) => {
                player.add_power_from_monster(PowerId::Weak, 2);
            }
            (MonsterId::Cultist, 3) => {
                // Constructor 3 / A2=4; takeTurn adds +1 at A17.
                let ritual = if ascension >= 17 {
                    5
                } else if ascension >= 2 {
                    4
                } else {
                    3
                };
                self.add_power(PowerId::Ritual, ritual);
            }
            (MonsterId::Cultist, 1) => {
                let _ = hit_player(player, self, rng, 6, 1);
            }
            (MonsterId::GremlinFat, 2) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 5 } else { 4 }, 1);
                player.add_power_from_monster(PowerId::Weak, 1);
                if ascension >= 17 {
                    player.add_power_from_monster(PowerId::Frail, 1);
                }
            }
            (MonsterId::GremlinWarrior, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 5 } else { 4 }, 1);
            }
            (MonsterId::GremlinThief, 1) => {
                let dmg = if ascension >= 2 { 10 } else { 9 };
                let _ = hit_player(player, self, rng, dmg, 1);
                self.set_move(1, Intent::Attack, dmg, 1);
            }
            (MonsterId::GremlinTsundere, 2) => {
                let dmg = if ascension >= 2 { 8 } else { 6 };
                let _ = hit_player(player, self, rng, dmg, 1);
                self.set_move(2, Intent::Attack, dmg, 1);
            }
            (MonsterId::GremlinWizard, 1) => {
                self.extra = 0;
                let atk = if ascension >= 2 { 30 } else { 25 };
                let _ = hit_player(player, self, rng, atk, 1);
                // A17+: stay on DOPE_MAGIC. Below A17, resume CHARGE.
                if ascension >= 17 {
                    self.set_move(1, Intent::Attack, atk, 1);
                } else {
                    self.set_move(2, Intent::Unknown, 0, 1);
                }
            }
            (MonsterId::GremlinWizard, 2) => {
                self.extra += 1;
                let atk = if ascension >= 2 { 30 } else { 25 };
                if self.extra == 3 {
                    self.set_move(1, Intent::Attack, atk, 1);
                } else {
                    self.set_move(2, Intent::Unknown, 0, 1);
                }
            }
            (MonsterId::Lagavulin, 4) => {}
            (MonsterId::Lagavulin, 5) => {
                self.extra += 1;
                if self.extra >= 3 {
                    self.powers.retain(|p| p.id != PowerId::Metallicize);
                }
            }
            (MonsterId::Lagavulin, 3) => {
                let _ = hit_player(player, self, rng, if ascension >= 3 { 20 } else { 18 }, 1);
                if self.extra >= 3 {
                    self.extra += 1;
                }
            }
            (MonsterId::Lagavulin, 1) => {
                let amt = if ascension >= 18 { -2 } else { -1 };
                player.add_power_from_monster(PowerId::Dexterity, amt);
                player.add_power_from_monster(PowerId::Strength, amt);
                if self.extra >= 3 {
                    self.extra = 3;
                }
            }
            (MonsterId::GremlinNob, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 3 { 16 } else { 14 }, 1);
            }
            (MonsterId::GremlinNob, 2) => {
                let _ = hit_player(player, self, rng, if ascension >= 3 { 8 } else { 6 }, 1);
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::GremlinNob, 3) => {
                self.add_power(PowerId::AngerNob, if ascension >= 18 { 3 } else { 2 });
            }
            (MonsterId::JawWorm, 1) => {
                // A0 chomp 11, A2/A17 chomp 12.
                let _ = hit_player(player, self, rng, if ascension >= 2 { 12 } else { 11 }, 1);
            }
            (MonsterId::JawWorm, 2) => {
                // A0 bellow 3 str / 6 block; A2 4/6; A17 5/9.
                let (str_amt, block_amt) = if ascension >= 17 {
                    (5, 9)
                } else if ascension >= 2 {
                    (4, 6)
                } else {
                    (3, 6)
                };
                self.block += block_amt;
                self.add_power(PowerId::Strength, str_amt);
            }
            (MonsterId::JawWorm, 3) => {
                hit_player(player, self, rng, 7, 1);
                self.block += 5;
            }
            (MonsterId::AwakenedOne, 1) => {
                let _ = hit_player(player, self, rng, 20, 1);
            }
            (MonsterId::AwakenedOne, 2) => {
                let _ = hit_player(player, self, rng, 6, 4);
            }
            (MonsterId::AwakenedOne, 3) => {
                self.half_dead = false;
                self.hp = self.max_hp;
                self.extra = 1;
                self.first_move = true;
                self.powers.retain(|p| {
                    !matches!(
                        p.id,
                        PowerId::Curiosity
                            | PowerId::Unawakened
                            | PowerId::Shackled
                            | PowerId::Vulnerable
                            | PowerId::Weak
                            | PowerId::Frail
                    )
                });
            }
            (MonsterId::AwakenedOne, 5) => {
                self.first_move = false;
                let _ = hit_player(player, self, rng, 40, 1);
            }
            (MonsterId::AwakenedOne, 6) => {
                let _ = hit_player(player, self, rng, 18, 1);
                add_to_random_spot(&mut player.draw, Card::new(CardId::Void), rng);
            }
            (MonsterId::AwakenedOne, 8) => {
                let _ = hit_player(player, self, rng, 10, 3);
            }
            (MonsterId::SlaverBlue, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 13 } else { 12 }, 1);
            }
            (MonsterId::SlaverBlue, 4) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 8 } else { 7 }, 1);
                player.add_power_from_monster(PowerId::Weak, if ascension >= 17 { 2 } else { 1 });
            }
            (MonsterId::SlaverRed, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 14 } else { 13 }, 1);
            }
            (MonsterId::SlaverRed, 2) => {
                player.add_power_from_monster(PowerId::Entangled, 1);
                self.extra = 1;
            }
            (MonsterId::SlaverRed, 3) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 9 } else { 8 }, 1);
                player.add_power_from_monster(PowerId::Vulnerable, if ascension >= 17 { 2 } else { 1 });
            }
            (MonsterId::Looter, 1) => {
                if self.extra == 0 {
                    let _ = rng.ai.random_boolean_chance(0.6);
                }
                let steal = if ascension >= 17 { 20 } else { 15 };
                looter_steal(self, player, steal);
                let _ = hit_player(player, self, rng, if ascension >= 2 { 11 } else { 10 }, 1);
                self.extra += 1;
                if self.extra == 2 {
                    // Looter.takeTurn: aiRng.randomBoolean(0.5F) is nextFloat()<0.5,
                    // not Random.nextBoolean().
                    if rng.ai.random_boolean_chance(0.5) {
                        self.set_move(2, Intent::Defend, 0, 1);
                    } else {
                        self.set_move(4, Intent::Attack, if ascension >= 2 { 14 } else { 12 }, 1);
                    }
                } else {
                    self.set_move(1, Intent::Attack, if ascension >= 2 { 11 } else { 10 }, 1);
                }
            }
            (MonsterId::Looter, 2) => {
                self.block += 6;
                self.set_move(3, Intent::Escape, 0, 1);
            }
            (MonsterId::Looter, 3) => {
                self.escaped = true;
                self.set_move(3, Intent::Escape, 0, 1);
            }
            (MonsterId::Looter, 4) => {
                let steal = if ascension >= 17 { 20 } else { 15 };
                looter_steal(self, player, steal);
                let _ = hit_player(player, self, rng, if ascension >= 2 { 14 } else { 12 }, 1);
                self.extra += 1;
                self.set_move(2, Intent::Defend, 0, 1);
            }
            (MonsterId::Mugger, 1) => {
                // Mugger.takeTurn MUG: talk rng only on the second swipe
                // (slashCount==1). playSfx uses aiRng.random(2).
                if self.extra == 1 {
                    let _ = rng.ai.random_boolean_chance(0.6);
                }
                let _ = rng.ai.random_int(2);
                let steal = if ascension >= 17 { 20 } else { 15 };
                looter_steal(self, player, steal);
                let _ = hit_player(player, self, rng, if ascension >= 2 { 11 } else { 10 }, 1);
                self.extra += 1;
                if self.extra == 2 {
                    if rng.ai.random_boolean_chance(0.5) {
                        self.set_move(2, Intent::Defend, 0, 1);
                    } else {
                        self.set_move(4, Intent::Attack, if ascension >= 2 { 18 } else { 16 }, 1);
                    }
                } else {
                    self.set_move(1, Intent::Attack, if ascension >= 2 { 11 } else { 10 }, 1);
                }
            }
            (MonsterId::Mugger, 2) => {
                self.block += if ascension >= 17 { 17 } else { 11 };
                self.set_move(3, Intent::Escape, 0, 1);
            }
            (MonsterId::Mugger, 3) => {
                self.escaped = true;
                self.set_move(3, Intent::Escape, 0, 1);
            }
            (MonsterId::Mugger, 4) => {
                let _ = rng.ai.random_int(2);
                let steal = if ascension >= 17 { 20 } else { 15 };
                looter_steal(self, player, steal);
                let _ = hit_player(player, self, rng, if ascension >= 2 { 18 } else { 16 }, 1);
                self.extra += 1;
                self.set_move(2, Intent::Defend, 0, 1);
            }
            (MonsterId::Snecko, 1) => {
                player.add_power_from_monster(PowerId::Confusion, 1);
            }
            (MonsterId::Snecko, 2) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 18 } else { 15 }, 1);
            }
            (MonsterId::Snecko, 3) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 10 } else { 8 }, 1);
                if ascension >= 17 {
                    player.add_power_from_monster(PowerId::Weak, 2);
                }
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::Byrd, 1) => {
                let n = if ascension >= 2 { 6 } else { 5 };
                let _ = hit_player(player, self, rng, 1, n);
            }
            (MonsterId::Byrd, 2) => {
                self.extra = 1;
                let flight = if ascension >= 17 { 4 } else { 3 };
                self.add_power(PowerId::Flight, flight);
            }
            (MonsterId::Byrd, 3) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 14 } else { 12 }, 1);
            }
            (MonsterId::Byrd, 4) => {}
            (MonsterId::Byrd, 5) => {
                let _ = hit_player(player, self, rng, 3, 1);
                self.set_move(2, Intent::Unknown, 0, 1);
            }
            (MonsterId::Byrd, 6) => {
                self.add_power(PowerId::Strength, 1);
            }
            (MonsterId::Champ, 1) => {
                let dmg = if ascension >= 4 { 18 } else { 16 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::Champ, 2) => {
                let block = if ascension >= 19 {
                    20
                } else if ascension >= 9 {
                    18
                } else {
                    15
                };
                let forge = if ascension >= 19 {
                    7
                } else if ascension >= 9 {
                    6
                } else {
                    5
                };
                self.block += block;
                self.add_power(PowerId::Metallicize, forge);
            }
            (MonsterId::Champ, 3) => {
                let _ = hit_player(player, self, rng, 10, 2);
            }
            (MonsterId::Champ, 4) => {
                let dmg = if ascension >= 4 { 14 } else { 12 };
                let _ = hit_player(player, self, rng, dmg, 1);
                player.add_power_from_monster(PowerId::Frail, 2);
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::Champ, 5) => {
                let str = if ascension >= 19 {
                    4
                } else if ascension >= 4 {
                    3
                } else {
                    2
                };
                self.add_power(PowerId::Strength, str);
            }
            (MonsterId::Champ, 6) => {
                player.add_power_from_monster(PowerId::Weak, 2);
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::Champ, 7) => {
                self.powers.retain(|p| !power_is_debuff(p.id, p.amount));
                let str = if ascension >= 19 {
                    4
                } else if ascension >= 4 {
                    3
                } else {
                    2
                };
                self.add_power(PowerId::Strength, str * 3);
            }
            (MonsterId::SpikeSlimeS, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 6 } else { 5 }, 1);
            }
            (MonsterId::AcidSlimeS, 1) => {
                let dmg = if ascension >= 2 { 4 } else { 3 };
                let _ = hit_player(player, self, rng, dmg, 1);
                self.set_move(2, Intent::Debuff, 0, 1);
            }
            (MonsterId::AcidSlimeS, 2) => {
                player.add_power_from_monster(PowerId::Weak, 1);
                self.set_move(1, Intent::Attack, if ascension >= 2 { 4 } else { 3 }, 1);
            }
            (MonsterId::SpikeSlimeM, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 10 } else { 8 }, 1);
                player.discard.push(Card::new(CardId::Slimed));
            }
            (MonsterId::SpikeSlimeM, 4) => player.add_power_from_monster(PowerId::Frail, 1),
            (MonsterId::AcidSlimeM, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 8 } else { 7 }, 1);
                player.discard.push(Card::new(CardId::Slimed));
            }
            (MonsterId::AcidSlimeM, 2) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 12 } else { 10 }, 1);
            }
            (MonsterId::AcidSlimeM, 4) => player.add_power_from_monster(PowerId::Weak, 1),
            (MonsterId::AcidSlimeL, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 12 } else { 11 }, 1);
                player.discard.push(Card::new(CardId::Slimed));
                player.discard.push(Card::new(CardId::Slimed));
            }
            (MonsterId::AcidSlimeL, 2) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 18 } else { 16 }, 1);
            }
            (MonsterId::AcidSlimeL, 4) => player.add_power_from_monster(PowerId::Weak, 2),
            (MonsterId::AcidSlimeL, 3) => {
                let hp = self.hp;
                self.hp = 0;
                self.dead = true;
                self.set_move(3, Intent::Unknown, 0, 1);
                return Some(split_into(MonsterId::AcidSlimeM, hp, rng, self.ascension, self.offset_x));
            }
            (MonsterId::SpikeSlimeL, 1) => {
                let _ = hit_player(player, self, rng, if ascension >= 2 { 18 } else { 16 }, 1);
                player.discard.push(Card::new(CardId::Slimed));
                player.discard.push(Card::new(CardId::Slimed));
            }
            (MonsterId::SpikeSlimeL, 4) => {
                // SpikeSlime_L FRAIL_LICK: 3 at A17, else 2.
                player.add_power_from_monster(PowerId::Frail, if ascension >= 17 { 3 } else { 2 })
            }
            (MonsterId::SpikeSlimeL, 3) => {
                let hp = self.hp;
                self.hp = 0;
                self.dead = true;
                self.set_move(3, Intent::Unknown, 0, 1);
                return Some(split_into(MonsterId::SpikeSlimeM, hp, rng, self.ascension, self.offset_x));
            }

            (MonsterId::SphericGuardian, 1) => {
                let _ = hit_player(player, self, rng, 10, 2);
            }
            (MonsterId::SphericGuardian, 2) => {
                self.block += 25;
            }
            (MonsterId::SphericGuardian, 3) => {
                self.block += 15;
                let _ = hit_player(player, self, rng, 10, 1);
            }
            (MonsterId::SphericGuardian, 4) => {
                let _ = hit_player(player, self, rng, 10, 1);
                player.add_power_from_monster(PowerId::Frail, 5);
            }
            (MonsterId::Chosen, 5) => {
                let _ = hit_player(player, self, rng, 5, 2);
            }
            (MonsterId::Chosen, 1) => {
                let _ = hit_player(player, self, rng, 18, 1);
            }
            (MonsterId::Chosen, 2) => {
                player.add_power_from_monster(PowerId::Weak, 3);
                self.add_power(PowerId::Strength, 3);
            }
            (MonsterId::Chosen, 3) => {
                let _ = hit_player(player, self, rng, 10, 1);
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::Chosen, 4) => {
                player.add_power(PowerId::Hex, 1);
            }
            (MonsterId::SnakePlant, 1) => {
                let _ = hit_player(player, self, rng, 7, 3);
            }
            (MonsterId::SnakePlant, 2) => {
                player.add_power_from_monster(PowerId::Frail, 2);
                player.add_power_from_monster(PowerId::Weak, 2);
            }
            (MonsterId::Centurion, 1) => {
                let _ = hit_player(player, self, rng, 12, 1);
            }
            (MonsterId::Centurion, 2) => {}
            (MonsterId::Centurion, 3) => {
                let _ = hit_player(player, self, rng, 6, 3);
            }
            (MonsterId::Healer, 1) => {
                let _ = hit_player(player, self, rng, 8, 1);
                player.add_power_from_monster(PowerId::Frail, 2);
            }
            (MonsterId::Healer, 2) => {}
            (MonsterId::Healer, 3) => {}
            (MonsterId::BookOfStabbing, 1) => {
                let hits = self.intent_hits.max(1);
                let _ = hit_player(player, self, rng, 6, hits);
            }
            (MonsterId::BookOfStabbing, 2) => {
                let _ = hit_player(player, self, rng, 21, 1);
            }
            (MonsterId::ShelledParasite, 1) => {
                let _ = hit_player(player, self, rng, 18, 1);
                player.add_power_from_monster(PowerId::Frail, 2);
            }
            (MonsterId::ShelledParasite, 2) => {
                let _ = hit_player(player, self, rng, 6, 2);
            }
            (MonsterId::ShelledParasite, 3) => {
                let dealt = hit_player(player, self, rng, 10, 1);
                if dealt > 0 {
                    self.hp = (self.hp + dealt).min(self.max_hp);
                }
            }
            (MonsterId::FungiBeast, 1) => {
                let _ = hit_player(player, self, rng, 6, 1);
            }
            (MonsterId::FungiBeast, 2) => {
                // A0 grow 3; A2 4; A17 5.
                let amt = if ascension >= 17 {
                    5
                } else if ascension >= 2 {
                    4
                } else {
                    3
                };
                self.add_power(PowerId::Strength, amt);
            }
            (MonsterId::BronzeAutomaton, 1) => {
                let _ = hit_player(player, self, rng, 7, 2);
            }
            (MonsterId::BronzeAutomaton, 2) => {
                let _ = hit_player(player, self, rng, 45, 1);
            }
            (MonsterId::BronzeAutomaton, 3) => {}
            (MonsterId::BronzeAutomaton, 4) => {
                let mut left = spawn_monster(MonsterId::BronzeOrb, rng, ascension);
                let mut right = spawn_monster(MonsterId::BronzeOrb, rng, ascension);
                left.roll_move(rng);
                right.roll_move(rng);
                return Some(vec![left, right]);
            }
            (MonsterId::BronzeAutomaton, 5) => {
                self.block += 9;
                self.add_power(PowerId::Strength, 3);
            }
            (MonsterId::BronzeOrb, 1) => {
                let _ = hit_player(player, self, rng, 8, 1);
            }
            (MonsterId::BronzeOrb, 2) => {}
            (MonsterId::BronzeOrb, 3) => {
                self.stasis_card = steal_stasis_card(player, rng);
            }
            (MonsterId::SpireShield, 1) => {
                let _ = hit_player(player, self, rng, 12, 1);
            }
            (MonsterId::SpireShield, 2) => {
                self.block += 30;
            }
            (MonsterId::SpireSpear, 1) => {
                let _ = hit_player(player, self, rng, 5, 2);
            }
            (MonsterId::SpireSpear, 2) => {
                player.add_power_from_monster(PowerId::Vulnerable, 2);
            }
            (MonsterId::Spiker, 1) => {
                let _ = hit_player(player, self, rng, 7, 1);
            }
            (MonsterId::Spiker, 2) => {
                self.extra += 1;
                self.add_power(PowerId::Thorns, 2);
            }
            (MonsterId::Exploder, 1) => {
                self.extra += 1;
                let _ = hit_player(player, self, rng, 9, 1);
            }
            (MonsterId::Exploder, 2) => {
                self.extra += 1;
            }
            (MonsterId::Repulsor, 1) => {
                for _ in 0..2 {
                    add_to_random_spot(&mut player.draw, Card::new(CardId::Dazed), rng);
                }
            }
            (MonsterId::Repulsor, 2) => {
                let _ = hit_player(player, self, rng, 11, 1);
            }
            (MonsterId::Sentry, 3) => {
                // BOLT: MakeTempCardInDiscardAction(Dazed, A18+ 3 else 2).
                let n = if ascension >= 18 { 3 } else { 2 };
                for _ in 0..n {
                    player.discard.push(Card::new(CardId::Dazed));
                }
            }
            (MonsterId::Sentry, 4) => {
                let dmg = if ascension >= 3 { 10 } else { 9 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::Darkling, 1) => {
                let _ = hit_player(player, self, rng, 8, 2);
            }
            (MonsterId::Darkling, 2) => {
                self.block += 12;
            }
            (MonsterId::Darkling, 3) => {
                let _ = hit_player(player, self, rng, self.extra.max(7), 1);
            }
            (MonsterId::Darkling, 4) => {}
            (MonsterId::Darkling, 5) => {
                self.hp = self.max_hp / 2;
                self.half_dead = false;
            }
            (MonsterId::OrbWalker, 1) => {
                let dmg = if ascension >= 2 { 11 } else { 10 };
                let _ = hit_player(player, self, rng, dmg, 1);
                // MakeTempCardInDiscardAndDeckAction(Burn): draw addToRandomSpot
                // then discard. cardRandomRng when draw is non-empty.
                let burn_draw = Card::new(CardId::Burn);
                if player.draw.is_empty() {
                    player.draw.push(burn_draw);
                } else {
                    let i = rng.card_random.random_int(player.draw.len() as i32 - 1) as usize;
                    player.draw.insert(i, burn_draw);
                }
                player.discard.push(Card::new(CardId::Burn));
            }
            (MonsterId::OrbWalker, 2) => {
                let dmg = if ascension >= 2 { 16 } else { 15 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::Maw, 2) => {
                let dur = if ascension >= 17 { 5 } else { 3 };
                player.add_power_from_monster(PowerId::Weak, dur);
                player.add_power_from_monster(PowerId::Frail, dur);
                self.split_triggered = true;
            }
            (MonsterId::Maw, 3) => {
                let dmg = if ascension >= 2 { 30 } else { 25 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::Maw, 4) => {
                let str = if ascension >= 17 { 5 } else { 3 };
                self.add_power(PowerId::Strength, str);
            }
            (MonsterId::Maw, 5) => {
                let hits = (self.extra / 2).max(1);
                let _ = hit_player(player, self, rng, 5, hits);
            }
            (MonsterId::TheCollector, 1) => {
                self.first_move = false;
                self.extra += 1;
                let mut left = spawn_monster(MonsterId::TorchHead, rng, ascension);
                left.set_move(1, Intent::Attack, 7, 1);
                left.offset_x = -185;
                left.just_spawned = true;
                let mut right = spawn_monster(MonsterId::TorchHead, rng, ascension);
                right.set_move(1, Intent::Attack, 7, 1);
                right.offset_x = -370;
                right.just_spawned = true;
                return Some(vec![left, right]);
            }
            (MonsterId::TheCollector, 2) => {
                let dmg = if ascension >= 4 { 21 } else { 18 };
                let _ = hit_player(player, self, rng, dmg, 1);
                self.extra += 1;
            }
            (MonsterId::TheCollector, 3) => {
                let block = if ascension >= 9 { 18 } else { 15 };
                self.block += if ascension >= 19 { block + 5 } else { block };
                self.extra += 1;
            }
            (MonsterId::TheCollector, 4) => {
                let n = if ascension >= 19 { 5 } else { 3 };
                player.add_power_from_monster(PowerId::Weak, n);
                player.add_power_from_monster(PowerId::Vulnerable, n);
                player.add_power_from_monster(PowerId::Frail, n);
                self.split_triggered = true;
                self.extra += 1;
            }
            (MonsterId::TorchHead, 1) => {
                let _ = hit_player(player, self, rng, 7, 1);
            }
            (MonsterId::Transient, 1) => {
                let _ = hit_player(player, self, rng, 30 + self.extra * 10, 1);
                self.extra += 1;
                self.set_move(1, Intent::Attack, 30 + self.extra * 10, 1);
            }
            (MonsterId::SlimeBoss, 1) => {
                // A4+: slam 38, else 35. Java queues DamageAction then
                // setMove(STICKY) before the hit resolves, so Bronze Scales
                // thorns that cross 50% HP overwrite STICKY with SPLIT.
                let slam = if self.ascension >= 4 { 38 } else { 35 };
                self.set_move(4, Intent::StrongDebuff, 0, 1);
                let _ = hit_player(player, self, rng, slam, 1);
            }
            (MonsterId::SlimeBoss, 2) => {
                let slam = if self.ascension >= 4 { 38 } else { 35 };
                self.set_move(1, Intent::Attack, slam, 1);
            }
            (MonsterId::SlimeBoss, 3) => {
                let hp = self.hp.max(1);
                self.hp = 0;
                self.dead = true;
                let mut spike = spawn_monster_at_hp(MonsterId::SpikeSlimeL, hp, self.ascension);
                let mut acid = spawn_monster_at_hp(MonsterId::AcidSlimeL, hp, self.ascension);
                // SlimeBoss split: SpikeSlime_L(-385, 20), AcidSlime_L(120, -8).
                spike.offset_x = -385;
                acid.offset_x = 120;
                spike.roll_move(rng);
                acid.roll_move(rng);
                return Some(vec![spike, acid]);
            }
            (MonsterId::SlimeBoss, 4) => {
                // A19+: MakeTempCardInDiscardAction(Slimed, 5) else 3.
                let n = if self.ascension >= 19 { 5 } else { 3 };
                for _ in 0..n {
                    player.discard.push(Card::new(CardId::Slimed));
                }
                self.set_move(2, Intent::Unknown, 0, 1);
            }
            (MonsterId::GiantHead, 1) => {
                player.add_power_from_monster(PowerId::Weak, 1);
            }
            (MonsterId::GiantHead, 2) => {
                let _ = hit_player(player, self, rng, 30 - self.extra * 5, 1);
            }
            (MonsterId::GiantHead, 3) => {
                let _ = hit_player(player, self, rng, 13, 1);
            }
            (MonsterId::CorruptHeart, 3) => {
                player.add_power_from_monster(PowerId::Vulnerable, 2);
                player.add_power_from_monster(PowerId::Weak, 2);
                player.add_power_from_monster(PowerId::Frail, 2);
                self.extra = 1;
            }
            (MonsterId::CorruptHeart, 1) => {
                let _ = hit_player(player, self, rng, 2, 12);
                self.extra += 1;
            }
            (MonsterId::CorruptHeart, 2) => {
                let _ = hit_player(player, self, rng, 40, 1);
                self.extra += 1;
            }
            (MonsterId::CorruptHeart, 4) => {
                self.add_power(PowerId::Strength, 2);
                self.extra += 1;
            }
            (MonsterId::Hexaghost, 5) => {
                let d = player.hp / 12 + 1;
                self.set_move(1, Intent::Attack, d, 6);
                self.extra = 6;
            }
            (MonsterId::Hexaghost, 1) => {
                let d = if self.intent_damage > 0 {
                    self.intent_damage
                } else {
                    player.hp / 12 + 1
                };
                let _ = hit_player(player, self, rng, d, 6);
                self.extra = 0;
            }
            (MonsterId::Hexaghost, 2) => {
                let dmg = if self.ascension >= 4 { 6 } else { 5 };
                let _ = hit_player(player, self, rng, dmg, 2);
                self.extra += 1;
            }
            (MonsterId::Hexaghost, 3) => {
                self.block += 12;
                let str = if self.ascension >= 19 { 3 } else { 2 };
                self.add_power(PowerId::Strength, str);
                self.extra += 1;
            }
            (MonsterId::Hexaghost, 4) => {
                let _ = hit_player(player, self, rng, 6, 1);
                let n = if self.ascension >= 19 { 2 } else { 1 };
                for _ in 0..n {
                    let mut burn = Card::new(CardId::Burn);
                    if self.split_triggered {
                        burn.upgrade();
                    }
                    player.discard.push(burn);
                }
                self.extra += 1;
            }
            (MonsterId::Hexaghost, 6) => {
                let dmg = if self.ascension >= 4 { 3 } else { 2 };
                let _ = hit_player(player, self, rng, dmg, 6);
                self.extra = 0;
                self.split_triggered = true;
                for pile in [&mut player.draw, &mut player.discard] {
                    for card in pile.iter_mut() {
                        if card.id == CardId::Burn {
                            card.upgrade();
                        }
                    }
                }
                for _ in 0..3 {
                    let mut burn = Card::new(CardId::Burn);
                    burn.upgrade();
                    player.discard.push(burn);
                }
            }
            (MonsterId::TheGuardian, 1) => {
                // CLOSE_UP: SharpHide A19 4 else 3, then ROLL_ATTACK.
                let hide = if ascension >= 19 { 4 } else { 3 };
                self.add_power(PowerId::SharpHide, hide);
                let roll = if self.ascension >= 4 { 10 } else { 9 };
                self.set_move(3, Intent::Attack, roll, 1);
            }
            (MonsterId::TheGuardian, 2) => {
                // useFierceBash: setMove(VENTSTEAM) is immediate after queueing
                // DamageAction, so player Thorns Mode Shift CLOSE_UP wins
                // (seed 275 Vent Steam Weak/Vuln vs Sharp Hide).
                self.set_move(7, Intent::StrongDebuff, 0, 1);
                let dmg = if self.ascension >= 4 { 36 } else { 32 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::TheGuardian, 3) => {
                self.set_move(4, Intent::AttackBuff, 8, 2);
                let dmg = if self.ascension >= 4 { 10 } else { 9 };
                let _ = hit_player(player, self, rng, dmg, 1);
            }
            (MonsterId::TheGuardian, 4) => {
                // Twin Slam takeTurn queues ChangeState("Offensive Mode") then
                // two DamageActions. ChangeState queues ApplyPower(ModeShift)
                // / Reset Threshold addToBottom, so the 8x2 (and player Thorns)
                // resolve while still defensive; Reset then wipes dmgTaken
                // (seed 32 Mode Shift 40 not 34, Sweeping Beam 6 vs leftover 20).
                let _ = hit_player(player, self, rng, 8, 2);
                self.split_triggered = false;
                self.add_power(PowerId::ModeShift, self.extra);
                self.block = 0;
                self.powers.retain(|p| p.id != PowerId::SharpHide);
                self.set_move(5, Intent::Attack, 5, 4);
            }
            (MonsterId::TheGuardian, 5) => {
                let _ = hit_player(player, self, rng, 5, 4);
                // takeTurn setMove(CHARGE_UP) runs before queued DamageActions.
                // Mode Shift ChangeState CLOSE_UP after those hits must win.
                if !self.split_triggered {
                    self.set_move(6, Intent::Defend, 0, 1);
                }
            }
            (MonsterId::TheGuardian, 6) => {
                self.block += 9;
                let bash = if self.ascension >= 4 { 36 } else { 32 };
                self.set_move(2, Intent::Attack, bash, 1);
            }
            (MonsterId::TheGuardian, 7) => {
                player.add_power_from_monster(PowerId::Weak, 2);
                player.add_power_from_monster(PowerId::Vulnerable, 2);
                self.set_move(5, Intent::Attack, 5, 4);
            }
            _ => {
                if self.intent_damage > 0 {
                    hit_player(player, self, rng, self.intent_damage, self.intent_hits.max(1));
                }
            }
        }
        None
    }
}

fn acid_slime_m_move(m: &mut Monster, num: i32, rng: &mut RngSet, wound: i32, tackle: i32) {
    if m.ascension >= 17 {
        if num < 40 {
            if m.last_two(1) {
                if rng.ai.random_boolean() {
                    m.set_move(2, Intent::Attack, tackle, 1);
                } else {
                    m.set_move(4, Intent::Debuff, 0, 1);
                }
            } else {
                m.set_move(1, Intent::AttackDebuff, wound, 1);
            }
        } else if num < 80 {
            if m.last_two(2) {
                if rng.ai.random_boolean_chance(0.5) {
                    m.set_move(1, Intent::AttackDebuff, wound, 1);
                } else {
                    m.set_move(4, Intent::Debuff, 0, 1);
                }
            } else {
                m.set_move(2, Intent::Attack, tackle, 1);
            }
        } else if m.last_move(4) {
            if rng.ai.random_boolean_chance(0.4) {
                m.set_move(1, Intent::AttackDebuff, wound, 1);
            } else {
                m.set_move(2, Intent::Attack, tackle, 1);
            }
        } else {
            m.set_move(4, Intent::Debuff, 0, 1);
        }
        return;
    }
    if num < 30 {
        if m.last_two(1) {
            if rng.ai.random_boolean() {
                m.set_move(2, Intent::Attack, tackle, 1);
            } else {
                m.set_move(4, Intent::Debuff, 0, 1);
            }
        } else {
            m.set_move(1, Intent::AttackDebuff, wound, 1);
        }
    } else if num < 70 {
        if m.last_move(2) {
            if rng.ai.random_boolean_chance(0.4) {
                m.set_move(1, Intent::AttackDebuff, wound, 1);
            } else {
                m.set_move(4, Intent::Debuff, 0, 1);
            }
        } else {
            m.set_move(2, Intent::Attack, tackle, 1);
        }
    } else if m.last_two(4) {
        if rng.ai.random_boolean_chance(0.4) {
            m.set_move(1, Intent::AttackDebuff, wound, 1);
        } else {
            m.set_move(2, Intent::Attack, tackle, 1);
        }
    } else {
        m.set_move(4, Intent::Debuff, 0, 1);
    }
}

fn acid_slime_l_move(m: &mut Monster, num: i32, rng: &mut RngSet) {
    // AcidSlime_L.getMove is not AcidSlime_M: A17 uses num<40 / <70 and
    // randomBoolean(0.6F) on the lastTwo branches (M uses <80 and 0.5).
    let wound = if m.ascension >= 2 { 12 } else { 11 };
    let tackle = if m.ascension >= 2 { 18 } else { 16 };
    if m.ascension >= 17 {
        if num < 40 {
            if m.last_two(1) {
                if rng.ai.random_boolean_chance(0.6) {
                    m.set_move(2, Intent::Attack, tackle, 1);
                } else {
                    m.set_move(4, Intent::Debuff, 0, 1);
                }
            } else {
                m.set_move(1, Intent::AttackDebuff, wound, 1);
            }
        } else if num < 70 {
            if m.last_two(2) {
                if rng.ai.random_boolean_chance(0.6) {
                    m.set_move(1, Intent::AttackDebuff, wound, 1);
                } else {
                    m.set_move(4, Intent::Debuff, 0, 1);
                }
            } else {
                m.set_move(2, Intent::Attack, tackle, 1);
            }
        } else if m.last_move(4) {
            if rng.ai.random_boolean_chance(0.4) {
                m.set_move(1, Intent::AttackDebuff, wound, 1);
            } else {
                m.set_move(2, Intent::Attack, tackle, 1);
            }
        } else {
            m.set_move(4, Intent::Debuff, 0, 1);
        }
        return;
    }
    acid_slime_m_move(m, num, rng, wound, tackle);
}

fn steal_stasis_card(player: &mut Player, rng: &mut RngSet) -> Option<Card> {
    let pile = if !player.draw.is_empty() {
        &mut player.draw
    } else {
        &mut player.discard
    };
    if pile.is_empty() {
        return None;
    }
    for rarity in [CardRarity::RARE, CardRarity::UNCOMMON, CardRarity::COMMON] {
        let mut idxs: Vec<usize> = pile
            .iter()
            .enumerate()
            .filter(|(_, c)| c.rarity() == rarity)
            .map(|(i, _)| i)
            .collect();
        if idxs.is_empty() {
            continue;
        }
        idxs.sort_by(|&a, &b| pile[a].sts_id().cmp(pile[b].sts_id()));
        let pick = rng.card_random.random_int(idxs.len() as i32 - 1) as usize;
        return Some(pile.remove(idxs[pick]));
    }
    let pick = rng.card_random.random_int(pile.len() as i32 - 1) as usize;
    Some(pile.remove(pick))
}

fn looter_steal(monster: &mut Monster, player: &mut Player, amt: i32) {
    let steal = amt.min(player.gold);
    player.gold -= steal;
    monster.stolen_gold += steal;
}

fn split_into(child: MonsterId, hp: i32, rng: &mut RngSet, ascension: i32, parent_x: i32) -> Vec<Monster> {
    let mut left = spawn_monster_at_hp(child, hp, ascension);
    let mut right = spawn_monster_at_hp(child, hp, ascension);
    left.offset_x = parent_x - 134;
    right.offset_x = parent_x + 134;
    left.roll_move(rng);
    right.roll_move(rng);
    vec![left, right]
}

/// SpawnMonsterAction smart insert: position = count of monsters with drawX < new.drawX.
fn smart_spawn_index(monsters: &[Monster], offset_x: i32) -> usize {
    monsters.iter().filter(|mo| offset_x > mo.offset_x).count()
}



/// TungstenRod.onLoseHpLast after decrementBlock / Buffer: incoming HP loss -1 if > 0.
pub fn on_lose_hp_last(player: &Player, damage: i32) -> i32 {
    if damage > 0 && player.has_relic(RelicId::TungstenRod) {
        damage - 1
    } else {
        damage
    }
}

/// Torii.onAttacked: Attack-type damage in (1, 5] becomes 1. Java runs this
/// after Buffer and before TungstenRod. HP_LOSS / THORNS never call this.
fn torii_on_attacked(player: &Player, damage: i32) -> i32 {
    if player.has_relic(RelicId::Torii) && damage > 1 && damage <= 5 {
        1
    } else {
        damage
    }
}

/// IntangiblePlayerPower.atDamageFinalReceive + AbstractPlayer.damage
/// hardcoded IntangiblePlayer check: damage > 1 becomes 1 for every
/// DamageInfo type (NORMAL, THORNS, HP_LOSS) before decrementBlock.
/// Sharp Hide / monster Thorns / Burn / Decay skip hit_player and must
/// still reduce (seed 604 Compile Driver: 3 Sharp Hide → 1, hp 43 not 41).
fn intangible_player(player: &Player, dmg: i32) -> i32 {
    if player.power_amount(PowerId::Intangible) > 0 && dmg > 1 {
        1
    } else {
        dmg
    }
}

/// BufferPower.onAttackedToChangeDamage after decrementBlock: consume 1 and return 0.
/// AbstractPlayer.damage: if currentHealth < 1, FairyPotion then Lizard Tail
/// (blocked by Mark of the Bloom). Returns true if death was prevented.
fn try_cheat_death(player: &mut Player) -> bool {
    if player.hp >= 1 {
        return false;
    }
    player.hp = 0;
    if player.has_relic(RelicId::Mark_of_the_Bloom) {
        return false;
    }
    if let Some(slot) = player.potions.iter().position(|p| p.id == PotionId::Fairy) {
        // FairyPotion.getPotency=30; healAmt = (int)(maxHealth * 0.30F)
        let heal = (player.max_hp * 30) / 100;
        player.hp = heal.max(1).min(player.max_hp);
        player.potions[slot].id = PotionId::Slot;
        red_skull_on_hp_change(player);
        return true;
    }
    if let Some(r) = player
        .relics
        .iter_mut()
        .find(|r| r.id == RelicId::Lizard_Tail && r.counter == -1)
    {
        r.counter = -2;
        r.used_up = true;
        let heal = player.max_hp / 2;
        player.hp = heal.max(1).min(player.max_hp);
        red_skull_on_hp_change(player);
        return true;
    }
    false
}

fn buffer_absorb(player: &mut Player, dmg: i32) -> i32 {
    if dmg <= 0 {
        return 0;
    }
    let Some(p) = player.powers.iter_mut().find(|p| p.id == PowerId::Buffer) else {
        return dmg;
    };
    if p.amount <= 0 {
        return dmg;
    }
    p.amount -= 1;
    if p.amount <= 0 {
        player.powers.retain(|x| x.id != PowerId::Buffer);
    }
    0
}

fn hit_player(player: &mut Player, monster: &mut Monster, rng: &mut RngSet, base: i32, hits: i32) -> i32 {
    let mut total = 0;
    for _ in 0..hits {
        // DamageInfo.applyPowers (monster → player): chain atDamageGive /
        // atDamageReceive as floats, then MathUtils.floor once.
        // Sequential floor(13*0.75)=9; floor(9*1.5)=13 misses 13*0.75*1.5=14.625→14
        // (seed 34 AcidSlime_M+SlaverRed, rust hp 39 vs Java 38).
        let mut dmg_f = (base + monster.power_amount(PowerId::Strength)) as f32;
        if monster.power_amount(PowerId::Weak) > 0 {
            dmg_f *= 0.75;
        }
        if player.power_amount(PowerId::Vulnerable) > 0 {
            // VulnerablePower.atDamageReceive: Odd Mushroom 1.25, else 1.5.
            dmg_f *= if player.has_relic(RelicId::Odd_Mushroom) {
                1.25
            } else {
                1.5
            };
        }
        let mut dmg = dmg_f.floor() as i32;
        if dmg < 0 {
            dmg = 0;
        }
        dmg = intangible_player(player, dmg);
        dmg = apply_block(&mut player.block, dmg);
        dmg = buffer_absorb(player, dmg);
        dmg = torii_on_attacked(player, dmg);
        // StaticDischargePower.onAttacked is after decrementBlock / Buffer
        // and before TungstenRod.onLoseHpLast. addToTop(ChannelAction) so
        // Frost evoke block lands before the next DamageAction (seed 389
        // Hexaghost Divider 56 HP vs 41 when channels waited until takeTurn
        // finished). damageAmount > 0 still channels even if Tungsten zeros HP.
        let static_n = if dmg > 0 {
            player.power_amount(PowerId::StaticDischarge)
        } else {
            0
        };
        dmg = on_lose_hp_last(player, dmg);
        if dmg > 0 {
            player.hp -= dmg;
            if let Some(p) = player.powers.iter_mut().find(|p| p.id == PowerId::PlatedArmor) {
                p.amount -= 1;
                if p.amount <= 0 {
                    player.powers.retain(|x| x.id != PowerId::PlatedArmor);
                }
            }
            if player.hp < 0 {
                player.hp = 0;
            }
            let _ = try_cheat_death(player);
            total += dmg;
            if monster.powers.iter().any(|p| p.id == PowerId::PainfulStabs) {
                player.discard.push(Card::new(CardId::Wound));
            }
        }
        for _ in 0..static_n {
            channel_static_lightning_mid_hit(player);
        }
        // ThornsPower.onAttacked addToTop DamageAction. If this hit is lethal,
        // ExactTextSim death snapshots before that thorns DamageAction (seed 1
        // AcidSlime_M 13 vs 10). Skip bounce when the player is already dead.
        let thorns = player.power_amount(PowerId::Thorns);
        if thorns > 0 && player.hp > 0 {
            deal_thorns(monster, thorns);
            let spores = monster.power_amount(PowerId::SporeCloud);
            if monster.dead && spores > 0 {
                player.add_power(PowerId::Vulnerable, spores);
                monster.powers.retain(|p| p.id != PowerId::SporeCloud);
            }
        }
    }
    if total > 0 {
        red_skull_on_hp_change(player);
        centennial_puzzle_was_hp_lost(player, rng);
    }
    total
}

fn red_skull_at_battle_start(player: &mut Player) {
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Red_Skull) {
        r.counter = 0;
        if player.hp <= player.max_hp / 2 {
            r.counter = 1;
            player.add_power(PowerId::Strength, 3);
        }
    }
}

/// CentennialPuzzle.wasHPLost: first unblocked HP loss each combat
/// `addToTop(new DrawCardAction(player, 3))`, so the draw resolves immediately
/// (during the enemy turn if the hit was a monster attack; those 3 cards then
/// sit under the next turn's 5-draw).
fn centennial_puzzle_was_hp_lost(player: &mut Player, rng: &mut RngSet) {
    let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Centennial_Puzzle) else {
        return;
    };
    if r.used_up {
        return;
    }
    r.used_up = true;
    // DrawCardAction is addToTop from wasHPLost; lethal damage
    // clearPostCombatActions drops DRAW (seed 452 death hand).
    if player.hp > 0 {
        let _ = draw_cards_rng(player, 3, Some(rng));
    }
}

pub fn red_skull_on_hp_change(player: &mut Player) {
    let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Red_Skull) else {
        return;
    };
    let bloodied = player.hp <= player.max_hp / 2;
    if bloodied && r.counter != 1 {
        r.counter = 1;
        player.add_power(PowerId::Strength, 3);
    } else if !bloodied && r.counter == 1 {
        r.counter = 0;
        player.add_power(PowerId::Strength, -3);
    }
}

pub fn apply_block(block: &mut i32, mut damage: i32) -> i32 {
    if *block > 0 {
        if damage >= *block {
            damage -= *block;
            *block = 0;
        } else {
            *block -= damage;
            damage = 0;
        }
    }
    damage
}

pub fn damage_monster(monster: &mut Monster, player: &mut Player, rng: &mut RngSet, base: i32, hits: i32) {
    // AbstractCreature.isDeadOrEscaped includes halfDead; DamageAllEnemies
    // skips those. Hitting a half-dead Darkling would setMove(COUNT) over
    // REINCARNATE (seed 8 Sweeping Beam, middle Darkling 0 vs 24 after EOT).
    if !monster.alive() || monster.half_dead {
        return;
    }
    // CurlUpPower: addToBot GainBlock after the unblocked hit, so multi-hit
    // cards (Barrage) land every hit before the block appears.
    let mut pending_curl = 0;
    for _ in 0..hits {
        let mut dmg_f = (base + player.power_amount(PowerId::Strength)) as f32;
        if player.power_amount(PowerId::PenNib) > 0 {
            dmg_f *= 2.0;
        }
        if player.power_amount(PowerId::Weak) > 0 {
            dmg_f *= 0.75;
        }
        if monster.power_amount(PowerId::Vulnerable) > 0 {
            // VulnerablePower.atDamageReceive: Paper Frog 1.75, else 1.5.
            dmg_f *= if player.has_relic(RelicId::Paper_Frog) {
                1.75
            } else {
                1.5
            };
        }
        let slow = monster.power_amount(PowerId::Slow);
        if slow > 0 {
            dmg_f *= 1.0 + slow as f32 * 0.1;
        }
        let mut dmg = dmg_f.floor() as i32;
        if dmg < 0 {
            dmg = 0;
        }
        if monster.power_amount(PowerId::Flight) > 0 {
            dmg = (dmg as f32 / 2.0) as i32;
        }
        // IntangiblePower.atDamageFinalReceive (Nemesis): damage > 1 becomes 1.
        if monster.power_amount(PowerId::Intangible) > 0 && dmg > 1 {
            dmg = 1;
        }
        dmg = apply_block(&mut monster.block, dmg);
        // Boot.onAttackToChangeDamage after decrementBlock: unblocked Attack 1–4 → 5.
        if dmg > 0 && dmg < 5 && player.has_relic(RelicId::Boot) {
            dmg = 5;
        }
        // ThornsPower.onAttacked: Attack-type hits bounce even if fully blocked or lethal.
        let thorns = monster.power_amount(PowerId::Thorns);
        if thorns > 0 {
            let bounced = intangible_player(player, thorns);
            let bounced = apply_block(&mut player.block, bounced);
            let bounced = buffer_absorb(player, bounced);
            let bounced = on_lose_hp_last(player, bounced);
            if bounced > 0 {
                player.hp -= bounced;
                if player.hp < 0 {
                    player.hp = 0;
                }
                let _ = try_cheat_death(player);
                red_skull_on_hp_change(player);
                centennial_puzzle_was_hp_lost(player, rng);
            }
        }
        if dmg > 0 && monster.id == MonsterId::Transient {
            monster.add_power(PowerId::Strength, -dmg);
            monster.add_power(PowerId::Shackled, dmg);
        }
        if dmg > 0 {
            let angry = monster.power_amount(PowerId::Angry);
            if angry > 0 {
                monster.add_power(PowerId::Strength, angry);
            }
            wake_asleep_lagavulin(monster);
            let lethal = dmg >= monster.hp;
            monster.hp -= dmg;
            if monster.hp <= 0 {
                monster.hp = 0;
                if monster.id == MonsterId::Darkling {
                    // Darkling.damage: only the first lethal hit while !halfDead
                    // sets COUNT. Later AOE must not overwrite REINCARNATE.
                    if !monster.half_dead {
                        monster.half_dead = true;
                        monster.dead = false;
                        monster.powers.clear();
                        monster.set_move(4, Intent::Unknown, 0, 1);
                    }
                } else {
                    monster.dead = true;
                    let spores = monster.power_amount(PowerId::SporeCloud);
                    if spores > 0 {
                        player.add_power(PowerId::Vulnerable, spores);
                        monster.powers.retain(|p| p.id != PowerId::SporeCloud);
                    }
                }
            }
            if !lethal {
                guardian_mode_shift_on_hp_loss(monster, dmg);
            }
            if dmg > 0 && !lethal {
                if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::Flight) {
                    p.amount -= 1;
                    if p.amount <= 0 {
                        monster.powers.retain(|x| x.id != PowerId::Flight);
                        monster.extra = 0;
                        monster.set_move(4, Intent::Stun, 0, 1);
                        monster.create_intent();
                    }
                }
            }
            if !lethal {
                // MalleablePower.onAttacked: monsters addToBot GainBlock, so
                // later DamageActions / orb evokes of this card land first
                // (Cold Snap then Dark evoke on Snake Plant).
                let malleable = monster.power_amount(PowerId::Malleable);
                if malleable > 0 {
                    pending_curl += malleable;
                    if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::Malleable) {
                        p.amount += 1;
                    }
                }
                let curl = monster.power_amount(PowerId::CurlUp);
                if curl > 0 {
                    pending_curl += curl;
                    monster.powers.retain(|p| p.id != PowerId::CurlUp);
                }
            }
            if dmg > 0 {
                if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::PlatedArmor) {
                    p.amount -= 1;
                    if p.amount <= 0 {
                        monster.powers.retain(|x| x.id != PowerId::PlatedArmor);
                    }
                }
            }
        }
        maybe_split(monster);
    }
    // Queue Curl Up block; flush_curl_up applies it after the whole card.
    monster.pending_curl += pending_curl;
}

fn flush_curl_up(combat: &mut Combat) {
    for monster in combat.monsters.iter_mut() {
        if monster.pending_curl > 0 {
            if monster.alive() {
                monster.block += monster.pending_curl;
            }
            monster.pending_curl = 0;
        }
    }
}

fn flush_spore_cloud(player: &mut Player, combat: &mut Combat) {
    for monster in combat.monsters.iter_mut().filter(|m| m.dead) {
        let spores = monster.power_amount(PowerId::SporeCloud);
        if spores > 0 {
            player.add_power(PowerId::Vulnerable, spores);
            monster.powers.retain(|p| p.id != PowerId::SporeCloud);
        }
    }
}

fn maybe_split(monster: &mut Monster) {
    if monster.dead {
        return;
    }
    // SlimeBoss.damage: hp <= max/2 && nextMove != 3. No splitTriggered —
    // slam's setMove(STICKY) runs before the hit resolves, so later thorns
    // or lightning can still overwrite STICKY with SPLIT (seed 906).
    if monster.id == MonsterId::SlimeBoss {
        if monster.hp <= monster.max_hp / 2 && monster.next_move != 3 {
            monster.set_move(3, Intent::Unknown, 0, 1);
        }
        return;
    }
    // AcidSlime_L / SpikeSlime_L.damage: also require !splitTriggered.
    if monster.split_triggered {
        return;
    }
    if !matches!(monster.id, MonsterId::AcidSlimeL | MonsterId::SpikeSlimeL) {
        return;
    }
    if monster.hp <= monster.max_hp / 2 && monster.next_move != 3 {
        monster.set_move(3, Intent::Unknown, 0, 1);
        monster.split_triggered = true;
    }
}

/// Lagavulin.damage: any HP loss while asleep (including THORNS / lightning) stuns.
/// changeState("OPEN") is ReducePowerAction(Metallicize, 8), not a full strip —
/// emerald Metallicize (act*2+2) can leave leftovers (654484: 12-8=4).
fn wake_asleep_lagavulin(monster: &mut Monster) {
    if monster.id == MonsterId::Lagavulin && monster.extra < 3 {
        monster.extra = 3;
        if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::Metallicize) {
            p.amount -= 8;
            if p.amount <= 0 {
                monster.powers.retain(|p| p.id != PowerId::Metallicize);
            }
        }
        monster.set_move(4, Intent::Stun, 0, 1);
    }
}

/// TheGuardian.damage: HP loss while isOpen counts toward Mode Shift (THORNS included).
fn guardian_mode_shift_on_hp_loss(monster: &mut Monster, lost: i32) {
    if monster.id != MonsterId::TheGuardian || lost <= 0 || monster.split_triggered || !monster.alive() {
        return;
    }
    let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::ModeShift) else {
        return;
    };
    p.amount -= lost;
    if p.amount <= 0 {
        monster.powers.retain(|p| p.id != PowerId::ModeShift);
        // ChangeStateAction("Defensive Mode") is addToBottom, so the +20
        // block lands after already-queued lightning DamageActions.
        monster.stolen_gold = 20;
        monster.extra += 10;
        monster.split_triggered = true;
        monster.set_move(1, Intent::Buff, 0, 1);
        monster.create_intent();
    }
}

pub(crate) fn flush_guardian_defensive_block(combat: &mut Combat) {
    for m in combat.monsters.iter_mut() {
        if m.id == MonsterId::TheGuardian && m.stolen_gold > 0 {
            m.block += m.stolen_gold;
            m.stolen_gold = 0;
        }
    }
}

pub fn deal_thorns(monster: &mut Monster, amount: i32) {
    if amount <= 0 || !monster.alive() || monster.half_dead {
        return;
    }
    let dmg = apply_block(&mut monster.block, amount);
    if dmg > 0 {
        if monster.id == MonsterId::Transient {
            monster.add_power(PowerId::Strength, -dmg);
            monster.add_power(PowerId::Shackled, dmg);
        }
        wake_asleep_lagavulin(monster);
        monster.hp -= dmg;
        if monster.hp <= 0 {
            monster.hp = 0;
            if monster.id == MonsterId::Darkling {
                monster.half_dead = true;
                monster.dead = false;
                monster.powers.clear();
                monster.set_move(4, Intent::Unknown, 0, 1);
            } else {
                monster.dead = true;
            }
        } else {
            guardian_mode_shift_on_hp_loss(monster, dmg);
        }
    }
    maybe_split(monster);
}

fn deal_card_damage(monster: &mut Monster, player: &mut Player, rng: &mut RngSet, card: &Card, hits: i32) {
    damage_monster(monster, player, rng, card.base_damage as i32, hits);
}

pub fn derived_damage(card: &Card, player: &Player) -> i32 {
    if card.base_damage < 0 {
        return card.base_damage as i32;
    }
    let mut dmg = card.base_damage as i32 + player.power_amount(PowerId::Strength);
    if player.power_amount(PowerId::Weak) > 0 {
        dmg = (dmg as f32 * 0.75).floor() as i32;
    }
    dmg
}

fn energy_on_use(player: &Player, combat: &Combat) -> i32 {
    if combat.energy_on_use >= 0 {
        combat.energy_on_use
    } else {
        player.energy
    }
}

pub fn derived_block(card: &Card, player: &Player) -> i32 {
    if card.base_block < 0 {
        return card.base_block as i32;
    }
    if player.power_amount(PowerId::NoBlock) > 0 {
        return 0;
    }
    let mut block = card.base_block as i32 + player.power_amount(PowerId::Dexterity);
    if player.power_amount(PowerId::Frail) > 0 {
        block = (block as f32 * 0.75).floor() as i32;
    }
    block
}

fn gain_player_block(player: &mut Player, amt: i32) {
    // AbstractCreature.addBlock does not consult NoBlockPower. That power's
    // modifyBlockLast is card applyPowers only (Panic Button: no block from cards).
    if amt <= 0 {
        return;
    }
    player.block += amt;
}

pub fn draw_cards(player: &mut Player, n: i32) {
    let _ = draw_cards_rng(player, n, None);
}

/// Returns how many Status/Curse cards were drawn (Fire Breathing).
pub fn draw_cards_rng(player: &mut Player, mut n: i32, mut rng: Option<&mut RngSet>) -> i32 {
    let mut statuses = 0;
    while n > 0 {
        if player.hand.len() >= 10 {
            break;
        }
        if player.draw.is_empty() {
            if player.discard.is_empty() {
                break;
            }
            player.draw.append(&mut player.discard);
            if let Some(rng) = rng.as_mut() {
                let seed = rng.shuffle.random_long();
                shuffle_java(&mut player.draw, seed);
            }
            continue;
        }
        if let Some(mut card) = player.draw.pop() {
            if matches!(card.card_type(), CardType::STATUS | CardType::CURSE) {
                statuses += 1;
            }
            // VoidCard.triggerWhenDrawn: LoseEnergyAction(1).
            if card.id == CardId::Void {
                player.energy = (player.energy - 1).max(0);
            }
            // ConfusionPower.onCardDraw: cardRandomRng.random(3) when cost >= 0.
            if player.power_amount(PowerId::Confusion) > 0 && card.cost >= 0 {
                if let Some(rng) = rng.as_mut() {
                    let new_cost = rng.card_random.random_int(3) as i16;
                    if card.cost != new_cost {
                        card.cost = new_cost;
                        card.cost_for_turn = new_cost;
                    }
                    card.free_to_play_once = false;
                }
            }
            player.hand.push(card);
        }
        n -= 1;
    }
    statuses
}

pub fn on_use_card(player: &mut Player, combat: &mut Combat, card: &Card, rng: &mut RngSet) {
    if player.has_relic(RelicId::Letter_Opener) && card.card_type() == CardType::SKILL {
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Letter_Opener) {
            r.counter += 1;
            if r.counter == 3 {
                r.counter = 0;
                for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                    deal_thorns(monster, 5);
                }
            }
        }
    }
    if card.card_type() == CardType::POWER {
        // BirdFacedUrn.onUseCard: HealAction(player, 2) addToTop for Power cards.
        if player.has_relic(RelicId::Bird_Faced_Urn) {
            player.hp = (player.hp + 2).min(player.max_hp);
            red_skull_on_hp_change(player);
        }
        // MummifiedHand.onUseCard: among hand cards with cost>0 and
        // costForTurn>0 and !freeToPlayOnce, cardRandomRng.random(0, n-1)
        // then setCostForTurn(0). Played card is already off the hand.
        if player.has_relic(RelicId::Mummified_Hand) {
            let idxs: Vec<usize> = player
                .hand
                .iter()
                .enumerate()
                .filter(|(_, c)| c.cost > 0 && c.cost_for_turn > 0 && !c.free_to_play_once)
                .map(|(i, _)| i)
                .collect();
            if !idxs.is_empty() {
                let pick = rng.card_random.random_range(0, idxs.len() as i32 - 1) as usize;
                player.hand[idxs[pick]].cost_for_turn = 0;
            }
        }
        // HeatsinkPower.onUseCard: addToTop(DrawCardAction) for Power cards.
        let n = player.power_amount(PowerId::Heatsink);
        if n > 0 {
            let drawn = draw_cards_rng(player, n, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, drawn);
        }
        // StormPower.onUseCard queues ChannelAction addToBot after card.use()
        // ApplyPower, so Focus from this Power applies before the Lightning
        // channel (and a full orb slot evoking Frost). Count is snapshotted
        // before apply so playing Storm itself does not channel.
        for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
            let curiosity = monster.power_amount(PowerId::Curiosity);
            if curiosity > 0 {
                monster.add_power(PowerId::Strength, curiosity);
            }
        }
    }
    // InkBottle.onUseCard: counter++ every play; at 10, addToBot(DrawCardAction(1)).
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::InkBottle) {
        if r.counter < 0 {
            r.counter = 0;
        }
        r.counter += 1;
        if r.counter == 10 {
            r.counter = 0;
            combat.pending_ink_bottle += 1;
        }
    }
}

fn add_to_random_spot(pile: &mut Vec<Card>, card: Card, rng: &mut RngSet) {
    if pile.is_empty() {
        pile.push(card);
    } else {
        let idx = rng.card_random.random_int(pile.len() as i32 - 1) as usize;
        pile.insert(idx, card);
    }
}

/// CardGroup.moveToExhaustPile: relics + powers onExhaust, then the card lands in the pile.
pub fn exhaust_card(player: &mut Player, combat: &mut Combat, card: Card, rng: &mut RngSet) {
    player.exhaust.push(card);
    let fnp = player.power_amount(PowerId::FeelNoPain);
    if fnp > 0 {
        player.block += fnp;
    }
    // DarkEmbracePower.onExhaust uses addToBot(DrawCardAction), so the draws
    // wait until the rest of the current card (and its discard) finish.
    if !combat.all_dead() {
        combat.pending_dark_embrace += player.power_amount(PowerId::DarkEmbrace);
    }
}

pub fn flush_dark_embrace(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    let n = combat.pending_dark_embrace;
    combat.pending_dark_embrace = 0;
    if n > 0 && !combat.all_dead() {
        let drawn = draw_cards_rng(player, n, Some(rng));
        apply_fire_breathing(player, &mut combat.monsters, drawn);
    }
}

/// InkBottle DrawCardAction is addToBot from UseCardAction's ctor, after use()
/// actions. DamageAction.clearPostCombatActions drops DRAW if the card kills.
fn flush_ink_bottle(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    let n = combat.pending_ink_bottle;
    combat.pending_ink_bottle = 0;
    if n > 0 && !combat.all_dead() {
        let drawn = draw_cards_rng(player, n, Some(rng));
        apply_fire_breathing(player, &mut combat.monsters, drawn);
    }
}

/// BetterDiscardPileToHandAction: move discard[index] to hand if there is room.
pub fn discard_pile_to_hand(player: &mut Player, index: usize) {
    if index >= player.discard.len() || player.hand.len() >= 10 {
        return;
    }
    let c = player.discard.remove(index);
    player.hand.push(c);
}

/// BetterDrawPileToHandAction: move draw[index] to hand if there is room.
pub fn draw_pile_to_hand(player: &mut Player, index: usize) {
    if index >= player.draw.len() || player.hand.len() >= 10 {
        return;
    }
    let c = player.draw.remove(index);
    player.hand.push(c);
}

pub fn apply_fire_breathing(player: &Player, monsters: &mut [Monster], statuses: i32) {
    let dmg = player.power_amount(PowerId::FireBreathing);
    if dmg <= 0 || statuses <= 0 {
        return;
    }
    for _ in 0..statuses {
        for monster in monsters.iter_mut().filter(|m| m.alive()) {
            deal_thorns(monster, dmg);
        }
    }
}

fn reduce_force_field_costs(player: &mut Player) {
    for c in player
        .hand
        .iter_mut()
        .chain(player.draw.iter_mut())
        .chain(player.discard.iter_mut())
    {
        if c.id == CardId::Force_Field {
            let diff = c.cost - c.cost_for_turn;
            c.cost = (c.cost - 1).max(0);
            c.cost_for_turn = (c.cost - diff).max(0);
        }
    }
}

pub fn play_card(
    player: &mut Player,
    combat: &mut Combat,
    hand_index: usize,
    target: Option<usize>,
    rng: &mut RngSet,
    dungeon: Option<&Dungeon>,
) -> bool {
    if hand_index >= player.hand.len() {
        return false;
    }
    let card = player.hand.remove(hand_index);
    play_owned_card(player, combat, card, target, rng, dungeon)
}

/// Java `AbstractCard.canUse` for STATUS/CURSE with cost -2 (Dazed, Wound, Burn).
/// PlayTopCard still queues them; GameActionManager then skips onUseCard/Hex/InkBottle
/// and UseCardAction discards (ethereal does not exhaust on this path).
fn status_or_curse_unplayable(card: &Card, player: &Player) -> bool {
    if card.card_type() == CardType::STATUS && card.cost_for_turn < -1 {
        !player.has_relic(RelicId::Medical_Kit)
    } else if card.card_type() == CardType::CURSE && card.cost_for_turn < -1 {
        !player.has_relic(RelicId::Blue_Candle)
    } else {
        false
    }
}

/// UseCardAction for a card already taken off hand or draw (PlayTopCardAction).
pub fn play_owned_card(
    player: &mut Player,
    combat: &mut Combat,
    mut card: Card,
    target: Option<usize>,
    rng: &mut RngSet,
    dungeon: Option<&Dungeon>,
) -> bool {
    if status_or_curse_unplayable(&card, player) {
        // UseCardAction.update clears freeToPlayOnce before discard. All For
        // One's AllCostToHandAction pulls cost==0 OR freeToPlayOnce (seed 213).
        card.free_to_play_once = false;
        if card.exhaust {
            exhaust_card(player, combat, card, rng);
        } else if card.card_type() != CardType::POWER {
            player.discard.push(card);
        }
        flush_dark_embrace(player, combat, rng);
        return false;
    }
    let cost = if card.free_to_play_once || card.cost_for_turn < 0 {
        0
    } else {
        card.cost_for_turn
    };
    player.energy -= cost as i32;
    card.free_to_play_once = false;
    // ChangeStateAction("Defensive Mode") is addToBottom of the previous
    // command (Fire/Explosive potion DamageAction). Flush before this card
    // deals so Sweeping Beam hits the 20 block (seed 149 Guardian 200 vs 194).
    flush_guardian_defensive_block(combat);

    let plays = if player.duplication > 0 {
        player.duplication -= 1;
        2
    } else {
        1
    };
    // AbstractCard.energyOnUse = EnergyPanel.totalCount at play. The
    // Duplication copy is CardQueueItem(tmp, m, card.energyOnUse, true, true)
    // so X-cost uses the original amount and freeToPlayOnce (seed 991).
    combat.energy_on_use = player.energy;
    combat.need_exhaust_select = false;
    combat.need_put_on_deck = false;
    combat.need_discard_to_hand = false;
    combat.need_draw_to_hand = false;
    combat.need_discovery = false;
    combat.need_forethought = false;
    combat.need_skill_from_deck = false;
    combat.skill_from_deck.clear();
    combat.draw_after_exhaust = 0;
    let needs_select = (card.id == CardId::Armaments && !card.upgraded && !player.hand.is_empty())
        || (card.id == CardId::True_Grit && card.upgraded && !player.hand.is_empty())
        || card.id == CardId::Thinking_Ahead
        || (card.id == CardId::Burning_Pact && player.hand.len() > 1)
        || (card.id == CardId::Hologram && player.discard.len() > 1)
        || (card.id == CardId::Seek && player.draw.len() > card.base_magic.max(1) as usize)
        || card.id == CardId::Discovery
        || (card.id == CardId::Purity && !player.hand.is_empty())
        || (card.id == CardId::Forethought && !player.hand.is_empty())
        || (card.id == CardId::Secret_Technique
            && player.draw.iter().filter(|c| c.card_type() == CardType::SKILL).count() > 1)
        || (card.id == CardId::Secret_Weapon
            && player.draw.iter().filter(|c| c.card_type() == CardType::ATTACK).count() > 1);
    for play_i in 0..plays {
        if play_i > 0 {
            card.free_to_play_once = true;
        }
        // UseCardAction.onUseCard fires when the card is played, before GRID.
        // Storm channels are addToBot after this card's ApplyPower, so snapshot
        // the stack now and channel after apply_card_effect (seed 169 Focus).
        let storm = if card.card_type() == CardType::POWER {
            player.power_amount(PowerId::Storm)
        } else {
            0
        };
        on_use_card(player, combat, &card, rng);
        let dead_before = combat.monsters.iter().filter(|m| m.dead).count();
        // SharpHidePower.onUseCard queues THORNS (DAMAGE, kept after combat)
        // before Damage/Channel resolve. Snapshot while the owner is alive
        // (seed 723 Ball Lightning evoke killed Guardian, rust skipped hide).
        let sharp_hide: i32 = if card.card_type() == CardType::ATTACK {
            combat
                .monsters
                .iter()
                .filter(|m| m.alive())
                .map(|m| m.power_amount(PowerId::SharpHide))
                .sum()
        } else {
            0
        };
        apply_card_effect(player, combat, &mut card, target, rng, dungeon);
        for _ in 0..storm {
            channel_orb(player, combat, rng, OrbKind::Lightning);
        }
        flush_curl_up(combat);
        flush_guardian_defensive_block(combat);
        flush_ink_bottle(player, combat, rng);
        // HexPower.onUseCard: Java checks card.type != ATTACK.
        if card.card_type() != CardType::ATTACK && player.power_amount(PowerId::Hex) > 0 {
            let n = player.power_amount(PowerId::Hex);
            for _ in 0..n {
                add_to_random_spot(&mut player.draw, Card::new(CardId::Dazed), rng);
            }
        }
        gremlin_horn_on_kills(player, combat, rng, dead_before);
        for monster in combat.monsters.iter_mut().filter(|m| m.dead) {
            let spores = monster.power_amount(PowerId::SporeCloud);
            if spores > 0 {
                player.add_power(PowerId::Vulnerable, spores);
                monster.powers.retain(|p| p.id != PowerId::SporeCloud);
            }
            if let Some(card) = monster.stasis_card.take() {
                if player.hand.len() < 10 {
                    player.hand.push(card);
                } else {
                    player.discard.push(card);
                }
            }
        }
        if card.card_type() == CardType::ATTACK {
            let rage = player.power_amount(PowerId::Rage);
            if rage > 0 {
                player.block += rage;
            }
            combat.attacks_this_turn += 1;
            // UseCardAction ctor: player powers, then relics, then monster
            // powers — all addToBot after card.use(). Ornamental Fan GainBlock
            // therefore lands before Sharp Hide THORNS (seed 872 Cold Snap:
            // 4 block then hide 3 → hp 58 block 1, not hp 55 block 4).
            player.powers.retain(|p| p.id != PowerId::PenNib);
            let apply_pen_nib = if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Pen_Nib)
            {
                r.counter += 1;
                if r.counter == 10 {
                    r.counter = 0;
                    false
                } else {
                    r.counter == 9
                }
            } else {
                false
            };
            if apply_pen_nib {
                player.add_power(PowerId::PenNib, 1);
            }
            if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Kunai) {
                if r.counter < 0 {
                    r.counter = 0;
                }
                r.counter += 1;
                if r.counter >= 3 {
                    r.counter = 0;
                    player.add_power(PowerId::Dexterity, 1);
                }
            }
            if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Shuriken) {
                if r.counter < 0 {
                    r.counter = 0;
                }
                r.counter += 1;
                if r.counter % 3 == 0 {
                    r.counter = 0;
                    player.add_power(PowerId::Strength, 1);
                }
            }
            if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Nunchaku) {
                if r.counter < 0 {
                    r.counter = 0;
                }
                r.counter += 1;
                if r.counter % 10 == 0 {
                    r.counter = 0;
                    player.energy += 1;
                }
            }
            if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Ornamental_Fan) {
                if r.counter < 0 {
                    r.counter = 0;
                }
                r.counter += 1;
                if r.counter % 3 == 0 {
                    r.counter = 0;
                    player.block += 4;
                }
            }
            if sharp_hide > 0 {
                let dmg = intangible_player(player, sharp_hide);
                let dmg = apply_block(&mut player.block, dmg);
                let dmg = buffer_absorb(player, dmg);
                let dmg = on_lose_hp_last(player, dmg);
                if dmg > 0 {
                    player.hp -= dmg;
                    if player.hp < 0 {
                        player.hp = 0;
                    }
                    let _ = try_cheat_death(player);
                    red_skull_on_hp_change(player);
                    centennial_puzzle_was_hp_lost(player, rng);
                }
            }
        }
        combat.cards_played_this_turn += 1;
        if card.card_type() == CardType::POWER {
            // ForceField.triggerOnCardPlayed: UseCardAction walks hand,
            // discard, and draw. updateCost(-1) per Power played.
            reduce_force_field_costs(player);
        }
        if card.card_type() == CardType::SKILL {
            combat.skills_this_turn += 1;
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                let anger = monster.power_amount(PowerId::AngerNob);
                if anger > 0 {
                    monster.add_power(PowerId::Strength, anger);
                }
            }
        }
    }

    // BattleStartEffect.showIntent often lands during the first card's
    // queued actions (DamageAction duration). UseCardAction itself does not
    // call createIntent; this is the tickless stand-in so a later GftE in
    // the same opening turn sees getIntentBaseDmg() >= 0.
    combat.publish_intents();
    if (card.id == CardId::Thinking_Ahead && card.exhaust)
        || (card.id == CardId::Hologram && combat.need_discard_to_hand)
        || (card.id == CardId::Seek && combat.need_draw_to_hand)
        || ((card.id == CardId::Secret_Technique || card.id == CardId::Secret_Weapon)
            && combat.need_skill_from_deck)
    {
        // UseCardAction runs after BetterDiscardPileToHandAction, so the played
        // card is still in limbo while GRID is open.
        combat.pending_exhaust = Some(card);
    } else if card.exhaust {
        exhaust_card(player, combat, card, rng);
    } else if card.card_type() != CardType::POWER {
        player.discard.push(card);
    }
    flush_dark_embrace(player, combat, rng);
    for monster in combat.monsters.iter_mut() {
        if monster.alive() && monster.powers.iter().any(|p| p.id == PowerId::Slow) {
            monster.add_power(PowerId::Slow, 1);
        }
    }
    resolve_darklings(combat);
    if !needs_select {
        unceasing_top_on_refresh_hand(player, combat, rng);
    }
    needs_select
}

/// UnceasingTop.onRefreshHand: if the hand is empty during the player turn
/// and a pile remains, DrawCardAction(1) (seed 114 Guardian).
fn unceasing_top_on_refresh_hand(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    if !player.has_relic(RelicId::Unceasing_Top) {
        return;
    }
    if !player.hand.is_empty() || combat.all_dead() {
        return;
    }
    if player.power_amount(PowerId::NoDraw) > 0 {
        return;
    }
    if player.draw.is_empty() && player.discard.is_empty() {
        return;
    }
    let n = draw_cards_rng(player, 1, Some(rng));
    apply_fire_breathing(player, &mut combat.monsters, n);
}

/// PlayTopCardAction: autoplay the top of the draw pile (shuffle discard first
/// if the draw pile is empty). Distilled Chaos exhausts=false; Havoc true.
pub fn play_top_card(
    player: &mut Player,
    combat: &mut Combat,
    target: Option<usize>,
    exhausts: bool,
    rng: &mut RngSet,
    dungeon: Option<&Dungeon>,
) {
    play_top_cards(player, combat, &[target], exhausts, rng, dungeon);
}

/// DistilledChaosPotion queues 3 PlayTopCardActions (ActionType.WAIT).
/// NewQueueCardAction is addToTop, so each card plays before the next pop
/// (Coolheaded draws can become the next top). Remaining WAIT is dropped
/// once the room is dead.
pub fn play_top_cards(
    player: &mut Player,
    combat: &mut Combat,
    targets: &[Option<usize>],
    exhausts: bool,
    rng: &mut RngSet,
    dungeon: Option<&Dungeon>,
) {
    // Pop every PlayTopCard up front so an empty-deck shuffle during the
    // batch does not see in-flight cards in discard (seed 38 Dualcast).
    let mut pending: Vec<(Card, Option<usize>)> = Vec::new();
    for &target in targets {
        if player.draw.is_empty() && player.discard.is_empty() {
            break;
        }
        if player.draw.is_empty() {
            reshuffle_if_needed(player, rng);
        }
        let Some(mut card) = player.draw.pop() else {
            break;
        };
        card.free_to_play_once = true;
        if exhausts {
            card.exhaust = true;
        }
        pending.push((card, target));
    }
    let mut rest = pending.into_iter();
    while let Some((card, target)) = rest.next() {
        // PlayTopCardAction is WAIT; clearPostCombatActions drops leftover
        // WAIT after a lethal autoplay (seed 211 block 7 vs 0).
        if combat.all_dead() {
            let mut back = vec![card];
            back.extend(rest.map(|(c, _)| c));
            for c in back.into_iter().rev() {
                player.draw.push(c);
            }
            break;
        }
        let _ = play_owned_card(player, combat, card, target, rng, dungeon);
    }
}

/// MonsterGroup.getRandomMonster(null, true, rng): alive, not half-dead.
pub fn random_alive_monster(combat: &Combat, rng: &mut crate::rng::StsRandom) -> Option<usize> {
    let alive: Vec<usize> = combat
        .monsters
        .iter()
        .enumerate()
        .filter(|(_, m)| m.hp > 0 && !m.dead && !m.escaped && !m.half_dead)
        .map(|(i, _)| i)
        .collect();
    if alive.is_empty() {
        return None;
    }
    Some(alive[rng.random_range(0, alive.len() as i32 - 1) as usize])
}

fn resolve_darklings(combat: &mut Combat) {
    let any_dark = combat.monsters.iter().any(|m| m.id == MonsterId::Darkling);
    if !any_dark {
        return;
    }
    let any_up = combat
        .monsters
        .iter()
        .any(|m| m.id == MonsterId::Darkling && !m.half_dead && !m.dead);
    if !any_up {
        for monster in &mut combat.monsters {
            if monster.id == MonsterId::Darkling {
                monster.half_dead = false;
                monster.dead = true;
                monster.hp = 0;
            }
        }
    }
}

fn apply_card_effect(
    player: &mut Player,
    combat: &mut Combat,
    card: &mut Card,
    target: Option<usize>,
    rng: &mut RngSet,
    dungeon: Option<&Dungeon>,
) {
    let dmg = card.base_damage as i32;
    let block = derived_block(card, player);
    match card.id {
        CardId::Perfected_Strike => {
            // Java PerfectedStrike: baseDamage + magic * countCards() over hand+draw+discard.
            // play_card already removed this card from hand and has not discarded it yet,
            // so count the in-play card separately (it has the STRIKE tag).
            let strike_count = player
                .hand
                .iter()
                .chain(player.draw.iter())
                .chain(player.discard.iter())
                .filter(|c| c.id.has_strike_tag())
                .count() as i32
                + i32::from(card.id.has_strike_tag());
            let perfected = card.base_damage as i32 + card.base_magic as i32 * strike_count;
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, perfected, 1);
                }
            }
        }
        CardId::Strike_R | CardId::Strike_B | CardId::Bash | CardId::Bludgeon | CardId::Hemokinesis | CardId::Anger => {
            if card.id == CardId::Hemokinesis {
                let dmg = on_lose_hp_last(player, intangible_player(player, card.base_magic as i32));
                if dmg > 0 {
                    player.hp -= dmg;
                    red_skull_on_hp_change(player);
                    centennial_puzzle_was_hp_lost(player, rng);
                }
            }
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                    if card.id == CardId::Bash {
                        m.add_power(PowerId::Vulnerable, card.base_magic as i32);
                    }
                    if card.id == CardId::Anger {
                        player.discard.push(Card::new(CardId::Anger));
                    }
                }
            }
        }
        CardId::Sword_Boomerang => {
            let hits = card.base_magic.max(1) as i32;
            for _ in 0..hits {
                let alive: Vec<usize> = combat
                    .monsters
                    .iter()
                    .enumerate()
                    .filter(|(_, m)| m.alive())
                    .map(|(i, _)| i)
                    .collect();
                if alive.is_empty() {
                    break;
                }
                let pick = rng.card_random.random_range(0, alive.len() as i32 - 1) as usize;
                let idx = alive[pick];
                if let Some(m) = combat.monsters.get_mut(idx) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
        }
        CardId::Cleave | CardId::Immolate | CardId::Reaper | CardId::Thunderclap | CardId::Whirlwind => {
            let hits = if card.id == CardId::Whirlwind {
                energy_on_use(player, combat)
            } else {
                1
            };
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                damage_monster(monster, player, rng, dmg, hits);
                if card.id == CardId::Thunderclap {
                    monster.add_power(PowerId::Vulnerable, 1);
                }
            }
            if card.id == CardId::Whirlwind {
                player.energy = 0;
            }
        }
        CardId::Burning_Pact => {
            let n = card.base_magic as i32;
            if player.hand.is_empty() {
                let drawn = draw_cards_rng(player, n, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, drawn);
            } else if player.hand.len() == 1 {
                let c = player.hand.remove(0);
                exhaust_card(player, combat, c, rng);
                let drawn = draw_cards_rng(player, n, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, drawn);
            } else {
                combat.need_exhaust_select = true;
                combat.draw_after_exhaust = n;
            }
        }
        CardId::Zap => {
            let n = card.base_magic.max(1) as i32;
            for _ in 0..n {
                channel_orb(player, combat, rng, OrbKind::Lightning);
            }
        }
        CardId::Dualcast => {
            evoke_front(player, combat, rng, false);
            evoke_front(player, combat, rng, true);
        }
        CardId::Fission => {
            // FissionAction: capture filledOrbCount, then addToTop Draw, GainEnergy,
            // then EvokeAll (upgraded) or RemoveAllOrbs (unupgraded).
            let n = player.orbs.len() as i32;
            if card.upgraded {
                while !player.orbs.is_empty() {
                    evoke_front(player, combat, rng, true);
                }
            } else {
                player.orbs.clear();
            }
            player.energy += n;
            let drawn = draw_cards_rng(player, n, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, drawn);
        }
        CardId::Multi_Cast => {
            // MulticastAction: if hasOrb, effect = energyOnUse (+2 Chemical X, +1 upgraded).
            // EvokeWithoutRemoving (effect-1) times, then EvokeOrbAction (removes).
            if !player.orbs.is_empty() {
                let mut effect = energy_on_use(player, combat);
                if player.has_relic(RelicId::Chemical_X) {
                    effect += 2;
                }
                if card.upgraded {
                    effect += 1;
                }
                if effect > 0 {
                    for _ in 0..(effect - 1) {
                        evoke_front(player, combat, rng, false);
                    }
                    evoke_front(player, combat, rng, true);
                    if !card.free_to_play_once {
                        player.energy = 0;
                    }
                }
            }
        }
        CardId::Ball_Lightning => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            // DamageAction -> GameActionManager.clearPostCombatActions drops ChannelAction.
            if !combat.all_dead() {
                for _ in 0..card.base_magic.max(1) {
                    channel_orb(player, combat, rng, OrbKind::Lightning);
                }
            }
        }
        CardId::Cold_Snap => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            // DamageAction -> GameActionManager.clearPostCombatActions drops ChannelAction.
            if !combat.all_dead() {
                for _ in 0..card.base_magic.max(1) {
                    channel_orb(player, combat, rng, OrbKind::Frost);
                }
            }
        }
        CardId::Beam_Cell => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                    m.add_power(PowerId::Vulnerable, card.base_magic.max(1) as i32);
                }
            }
        }
        CardId::Go_for_the_Eyes => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    // ForTheEyesAction is queued after DamageAction, before
                    // Mode Shift's addToBottom ChangeStateAction. Snapshot
                    // getIntentBaseDmg() before damage so a trip to CLOSE_UP
                    // (BUFF, ibd -1) does not skip Weak (seed 54 ROLL_ATTACK 9).
                    let apply_weak = m.intent_base_damage >= 0;
                    damage_monster(m, player, rng, dmg, 1);
                    if apply_weak {
                        if let Some(m) = combat.monsters.get_mut(i) {
                            m.add_power(PowerId::Weak, card.base_magic.max(1) as i32);
                        }
                    }
                }
            }
        }
        CardId::FTL => {
            if combat.cards_played_this_turn < card.base_magic.max(3) as i32 {
                let n = draw_cards_rng(player, 1, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, n);
            }
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
        }
        CardId::Chill => {
            let living = combat
                .monsters
                .iter()
                .filter(|m| m.alive() && !m.half_dead && !m.escaped)
                .count() as i32;
            let n = living * card.base_magic.max(1) as i32;
            for _ in 0..n {
                channel_orb(player, combat, rng, OrbKind::Frost);
            }
        }
        CardId::Blizzard => {
            // Blizzard.use: baseDamage = frostCount * magic, then calculateCardDamage
            // (Strength applies even when frostCount is 0 — seed 840 SlaverRed 20 vs 18).
            let frost = combat
                .orbs_channeled_this_combat
                .iter()
                .filter(|k| **k == OrbKind::Frost)
                .count() as i32;
            let per = frost * card.base_magic.max(2) as i32;
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                damage_monster(monster, player, rng, per, 1);
            }
        }
        CardId::Sweeping_Beam => {
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                damage_monster(monster, player, rng, dmg, 1);
            }
            let n = draw_cards_rng(player, card.base_magic.max(1) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Compile_Driver => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            let mut kinds = Vec::new();
            for orb in &player.orbs {
                if !kinds.contains(&orb.kind) {
                    kinds.push(orb.kind);
                }
            }
            let n = draw_cards_rng(player, kinds.len() as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Coolheaded => {
            channel_orb(player, combat, rng, OrbKind::Frost);
            let n = draw_cards_rng(player, card.base_magic.max(1) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Tempest => {
            // TempestAction: effect = energyOnUse, +2 Chemical X, +1 if upgraded.
            // If effect > 0: ChannelAction(Lightning) * effect, then energy.use(total)
            // unless freeToPlayOnce. X=0 unupgraded is a no-op besides exhaust.
            let mut effect = energy_on_use(player, combat);
            if player.has_relic(RelicId::Chemical_X) {
                effect += 2;
            }
            if card.upgraded {
                effect += 1;
            }
            if effect > 0 {
                for _ in 0..effect {
                    channel_orb(player, combat, rng, OrbKind::Lightning);
                }
                if !card.free_to_play_once {
                    player.energy = 0;
                }
            }
        }
        CardId::Conserve_Battery => {
            if block > 0 {
                player.block += block;
            }
            player.add_power(PowerId::Energized, 1);
        }
        CardId::Leap | CardId::BootSequence => {
            if block > 0 {
                player.block += block;
            }
        }
        CardId::Deep_Breath => {
            // DeepBreath.use: if discard nonempty, EmptyDeckShuffleAction
            // (onShuffle relics, shuffle discard, souls addToTop draw) then
            // ShuffleAction(draw, false) — a second shuffleRng.randomLong,
            // no relic trigger. Then DrawCardAction(magic 1/2).
            if !player.discard.is_empty() {
                on_shuffle_relics(player);
                let seed = rng.shuffle.random_long();
                shuffle_java(&mut player.discard, seed);
                player.draw.append(&mut player.discard);
                let seed = rng.shuffle.random_long();
                shuffle_java(&mut player.draw, seed);
            }
            let n = draw_cards_rng(player, card.base_magic.max(1) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Finesse => {
            if block > 0 {
                player.block += block;
            }
            let n = draw_cards_rng(player, 1, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Reboot => {
            // ShuffleAllAction finishes (discard.shuffle + souls addToTop to
            // draw) *before* PutOnDeckAction. ShuffleAction then shuffles the
            // combined draw. Input order before the final shuffle matters.
            // CardGroup.shuffle(rng) always burns randomLong(), even on empty.
            let seed = rng.shuffle.random_long();
            shuffle_java(&mut player.discard, seed);
            player.draw.append(&mut player.discard);
            while !player.hand.is_empty() {
                let i = rng.card_random.random_int(player.hand.len() as i32 - 1) as usize;
                let c = player.hand.remove(i);
                player.draw.push(c);
            }
            let seed = rng.shuffle.random_long();
            shuffle_java(&mut player.draw, seed);
            let n = draw_cards_rng(player, card.base_magic.max(4) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Creative_AI => {
            player.add_power(PowerId::CreativeAI, card.base_magic.max(1) as i32);
        }
        CardId::Mayhem => {
            player.add_power(PowerId::Mayhem, card.base_magic.max(1) as i32);
        }
        CardId::Scrape => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            let n = card.base_magic.max(4) as i32;
            let before = player.hand.len();
            let statuses = draw_cards_rng(player, n, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, statuses);
            let drawn = player.hand.len().saturating_sub(before);
            let start = player.hand.len().saturating_sub(drawn);
            let mut i = start;
            while i < player.hand.len() {
                if player.hand[i].cost_for_turn != 0 && !player.hand[i].free_to_play_once {
                    let c = player.hand.remove(i);
                    player.discard.push(c);
                } else {
                    i += 1;
                }
            }
        }
        CardId::Hologram => {
            if block > 0 {
                player.block += block;
            }
            // BetterDiscardPileToHandAction(1): auto-move if discard.size <= 1.
            if player.discard.len() <= 1 {
                while !player.discard.is_empty() && player.hand.len() < 10 {
                    let c = player.discard.remove(0);
                    player.hand.push(c);
                }
            } else {
                combat.need_discard_to_hand = true;
            }
        }
        CardId::Seek => {
            let n = card.base_magic.max(1) as usize;
            if player.draw.len() <= n {
                while !player.draw.is_empty() && player.hand.len() < 10 {
                    let c = player.draw.pop().unwrap();
                    player.hand.push(c);
                }
            } else {
                combat.need_draw_to_hand = true;
            }
        }
        CardId::Impatience => {
            // ConditionalDrawAction: draw magic if no ATTACK remains in hand.
            if !player.hand.iter().any(|c| c.card_type() == CardType::ATTACK) {
                let n = draw_cards_rng(player, card.base_magic.max(2) as i32, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, n);
            }
        }
        CardId::Stack => {
            let mut amt = player.discard.len() as i32;
            if card.upgraded {
                amt += 3;
            }
            amt += player.power_amount(PowerId::Dexterity);
            if player.power_amount(PowerId::Frail) > 0 {
                amt = (amt as f32 * 0.75).floor() as i32;
            }
            if amt > 0 {
                player.block += amt;
            }
        }
        CardId::Steam => {
            if block > 0 {
                player.block += block;
            }
            card.base_block = (card.base_block - 1).max(0);
        }
        CardId::Auto_Shields => {
            if player.block == 0 && block > 0 {
                player.block += block;
            }
        }
        CardId::Buffer => {
            player.add_power(PowerId::Buffer, card.base_magic.max(1) as i32);
        }
        CardId::Defragment => {
            player.add_power(PowerId::Focus, card.base_magic.max(1) as i32);
        }
        CardId::Biased_Cognition => {
            player.add_power(PowerId::Focus, card.base_magic.max(4) as i32);
            player.add_power(PowerId::Bias, 1);
        }
        CardId::Glacier => {
            if block > 0 {
                player.block += block;
            }
            for _ in 0..card.base_magic.max(2) {
                channel_orb(player, combat, rng, OrbKind::Frost);
            }
        }
        CardId::Discovery => {
            combat.need_discovery = true;
        }
        CardId::Rainbow => {
            channel_orb(player, combat, rng, OrbKind::Lightning);
            channel_orb(player, combat, rng, OrbKind::Frost);
            channel_orb(player, combat, rng, OrbKind::Dark);
        }
        CardId::Fusion => {
            for _ in 0..card.base_magic.max(1) {
                channel_orb(player, combat, rng, OrbKind::Plasma);
            }
        }
        CardId::Machine_Learning => {
            player.add_power(PowerId::DrawCard, card.base_magic.max(1) as i32);
        }
        CardId::All_For_One => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            // AllCostToHandAction(0): each discard card with cost==0 (or freeToPlayOnce)
            // queues DiscardToHandAction.
            let mut i = 0;
            while i < player.discard.len() {
                let c = &player.discard[i];
                if c.cost == 0 || c.free_to_play_once {
                    if player.hand.len() < 10 {
                        let c = player.discard.remove(i);
                        player.hand.push(c);
                        continue;
                    }
                }
                i += 1;
            }
        }
        CardId::Darkness => {
            channel_orb(player, combat, rng, OrbKind::Dark);
            if card.upgraded {
                // DarkImpulseAction: each Dark onEndOfTurn (evoke += passive = 6+Focus).
                impulse_dark_orbs(player);
            }
        }
        CardId::Melter => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    m.block = 0;
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
        }
        CardId::Streamline => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            let reduce = card.base_magic.max(1);
            card.cost = (card.cost - reduce).max(0);
            card.cost_for_turn = (card.cost_for_turn - reduce).max(0);
        }
        CardId::Swift_Strike | CardId::Rip_and_Tear => {
            let hits = if card.id == CardId::Rip_and_Tear { 2 } else { 1 };
            if card.id == CardId::Rip_and_Tear && target.is_none() {
                // NewRipAndTearAction -> AttackDamageRandomEnemyAction: cardRandomRng
                // getRandomMonster(null, true) per hit.
                for _ in 0..hits {
                    damage_random_alive(player, combat, rng, dmg);
                }
            } else if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, hits);
                }
            }
        }
        CardId::Flash_of_Steel => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            let n = draw_cards_rng(player, 1, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Panacea => {
            player.add_power(PowerId::Artifact, card.base_magic.max(1) as i32);
        }
        CardId::Violence => {
            // DrawPileToHandAction(magic, ATTACK): attacks into tmp via
            // addToRandomSpot (cardRandomRng), then amount times shuffle tmp
            // (shuffleRng.randomLong) and move the bottom card to hand.
            let n = card.base_magic.max(3) as usize;
            let mut tmp: Vec<usize> = Vec::new();
            for (i, c) in player.draw.iter().enumerate() {
                if c.card_type() == CardType::ATTACK {
                    if tmp.is_empty() {
                        tmp.push(i);
                    } else {
                        let at = rng.card_random.random_int(tmp.len() as i32 - 1) as usize;
                        tmp.insert(at, i);
                    }
                }
            }
            let mut picked: Vec<usize> = Vec::new();
            for _ in 0..n {
                if tmp.is_empty() {
                    break;
                }
                let seed = rng.shuffle.random_long();
                shuffle_java(&mut tmp, seed);
                picked.push(tmp.remove(0));
            }
            for (k, &idx) in picked.iter().enumerate() {
                let mut adj = idx;
                for &prev in picked.iter().take(k) {
                    if prev < idx {
                        adj -= 1;
                    }
                }
                if adj >= player.draw.len() {
                    continue;
                }
                let c = player.draw.remove(adj);
                if player.hand.len() >= 10 {
                    player.discard.push(c);
                } else {
                    player.hand.push(c);
                }
            }
        }
        CardId::Genetic_Algorithm => {
            // Java misc starts at 1; GainBlock uses applyPowers() amount, then
            // IncreaseMiscAction adds magic to masterDeck + in-battle copies.
            let cur = if card.misc <= 0 { 1 } else { card.misc };
            card.base_block = cur;
            let blk = derived_block(card, player);
            if blk > 0 {
                player.block += blk;
            }
            let inc = card.base_magic.max(2) as i16;
            card.misc = cur + inc;
            card.base_block = card.misc;
            if let Some(d) = player
                .deck
                .iter_mut()
                .find(|c| c.id == CardId::Genetic_Algorithm && (c.misc == 0 || c.misc == cur))
            {
                d.misc = card.misc;
                d.base_block = card.base_block;
            }
        }
        CardId::Gash => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            // GashAction: this card, then discard/draw/hand Claws.
            let inc = card.base_magic.max(2);
            card.base_damage += inc;
            for c in player
                .hand
                .iter_mut()
                .chain(player.draw.iter_mut())
                .chain(player.discard.iter_mut())
            {
                if c.id == CardId::Gash {
                    c.base_damage += inc;
                }
            }
        }
        CardId::Turbo => {
            player.energy += card.base_magic.max(2) as i32;
            player.discard.push(Card::new(CardId::Void));
        }
        CardId::Redo => {
            // RedoAction: EvokeOrbAction then ChannelAction(same orb, autoEvoke=false).
            if let Some(orb) = player.orbs.first().copied() {
                evoke_front(player, combat, rng, true);
                if player.orbs.len() < player.max_orbs as usize {
                    player.orbs.push(orb);
                    combat.orbs_channeled_this_combat.push(orb.kind);
                }
            }
        }
        CardId::Chaos => {
            // AbstractOrb.getRandomOrb(true): Dark, Frost, Lightning, Plasma via cardRandomRng.
            let n = if card.upgraded { 2 } else { 1 };
            const KINDS: [OrbKind; 4] = [
                OrbKind::Dark,
                OrbKind::Frost,
                OrbKind::Lightning,
                OrbKind::Plasma,
            ];
            for _ in 0..n {
                let i = rng.card_random.random_int(KINDS.len() as i32 - 1) as usize;
                channel_orb(player, combat, rng, KINDS[i]);
            }
        }
        CardId::White_Noise => {
            if let Some(dungeon) = dungeon {
                if let Some(id) = crate::rewards::random_power_in_combat(dungeon, rng) {
                    let mut c = Card::new(id);
                    c.cost_for_turn = 0;
                    if player.hand.len() < 10 {
                        player.hand.push(c);
                    } else {
                        player.discard.push(c);
                    }
                }
            }
        }
        CardId::Lockon => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                    m.add_power(PowerId::LockOn, card.base_magic.max(2) as i32);
                }
            }
        }
        CardId::Blind => {
            // Blind.use: unupgraded Weak on the target; upgraded Weak on all.
            let n = card.base_magic.max(2) as i32;
            if card.upgraded {
                for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                    monster.add_power(PowerId::Weak, n);
                }
            } else if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    m.add_power(PowerId::Weak, n);
                }
            }
        }
        CardId::Dramatic_Entrance => {
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                damage_monster(monster, player, rng, dmg, 1);
            }
        }
        CardId::Steam_Power => {
            let n = draw_cards_rng(player, card.base_magic.max(2) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
            player.discard.push(Card::new(CardId::Burn));
        }
        CardId::Storm => {
            player.add_power(PowerId::Storm, card.base_magic.max(1) as i32);
        }
        CardId::Double_Energy => {
            player.energy *= 2;
        }
        CardId::PanicButton => {
            // GainBlock then NoBlockPower (modifyBlockLast = 0 for later gains).
            if block > 0 {
                player.block += block;
            }
            player.add_power(PowerId::NoBlock, card.base_magic.max(2) as i32);
        }
        CardId::Dark_Shackles => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    let amt = card.base_magic.max(9) as i32;
                    let had_art = m.power_amount(PowerId::Artifact) > 0;
                    m.add_power(PowerId::Strength, -amt);
                    if !had_art {
                        m.add_power(PowerId::Shackled, amt);
                    }
                }
            }
        }
        CardId::Reprogram => {
            // Focus -magic, Strength +magic, Dexterity +magic.
            let n = card.base_magic.max(1) as i32;
            player.add_power(PowerId::Focus, -n);
            player.add_power(PowerId::Strength, n);
            player.add_power(PowerId::Dexterity, n);
        }
        CardId::Aggregate => {
            let div = card.base_magic.max(1) as i32;
            player.energy += player.draw.len() as i32 / div;
        }
        CardId::Hello_World => {
            player.add_power(PowerId::HelloWorld, 1);
        }
        CardId::Reinforced_Body => {
            let mut effect = energy_on_use(player, combat);
            if player.has_relic(RelicId::Chemical_X) {
                effect += 2;
            }
            if effect > 0 {
                // `block` is derived_block (0 under NoBlock). Do not fall back to 7.
                for _ in 0..effect {
                    if block > 0 {
                        player.block += block;
                    }
                }
                if !card.free_to_play_once {
                    player.energy = 0;
                }
            }
        }
        CardId::Thunder_Strike => {
            let n = combat
                .orbs_channeled_this_combat
                .iter()
                .filter(|k| **k == OrbKind::Lightning)
                .count();
            for _ in 0..n {
                damage_random_alive(player, combat, rng, dmg);
            }
        }
        CardId::Static_Discharge => {
            player.add_power(PowerId::StaticDischarge, card.base_magic.max(1) as i32);
        }
        CardId::Core_Surge => {
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                }
            }
            player.add_power(PowerId::Artifact, card.base_magic.max(1) as i32);
        }
        CardId::Sunder => {
            let mut killed = false;
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                    killed = m.hp <= 0 || m.dead;
                }
            }
            if killed {
                player.energy += 3;
            }
        }
        CardId::Apotheosis => {
            // ApotheosisAction: upgrade every canUpgrade card in hand/draw/discard/exhaust.
            for c in player.hand.iter_mut() {
                if c.can_upgrade() {
                    c.upgrade();
                }
            }
            for c in player.draw.iter_mut() {
                if c.can_upgrade() {
                    c.upgrade();
                }
            }
            for c in player.discard.iter_mut() {
                if c.can_upgrade() {
                    c.upgrade();
                }
            }
            for c in player.exhaust.iter_mut() {
                if c.can_upgrade() {
                    c.upgrade();
                }
            }
        }
        CardId::HandOfGreed => {
            // GreedAction: damage, then gainGold(magic) if dying and not Minion.
            let mut killed = false;
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, 1);
                    killed = (m.hp <= 0 || m.dead) && !m.half_dead;
                }
            }
            if killed {
                player.gold += card.base_magic.max(20) as i32;
            }
        }
        CardId::Barrage => {
            let hits = player.orbs.len() as i32;
            if hits > 0 {
                if let Some(i) = target {
                    if let Some(m) = combat.monsters.get_mut(i) {
                        damage_monster(m, player, rng, dmg, hits);
                    }
                }
            }
        }
        CardId::Doom_and_Gloom => {
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                damage_monster(monster, player, rng, dmg, 1);
            }
            // DamageAction -> GameActionManager.clearPostCombatActions drops ChannelAction.
            if !combat.all_dead() {
                for _ in 0..card.base_magic.max(1) {
                    channel_orb(player, combat, rng, OrbKind::Dark);
                }
            }
        }
        CardId::Electrodynamics => {
            if player.power_amount(PowerId::Electro) == 0 {
                player.add_power(PowerId::Electro, 1);
            }
            let n = card.base_magic.max(2) as i32;
            for _ in 0..n {
                channel_orb(player, combat, rng, OrbKind::Lightning);
            }
        }
        CardId::Capacitor => {
            // IncreaseMaxOrbAction -> AbstractPlayer.increaseMaxOrbSlots (combat maxOrbs only).
            player.max_orbs += card.base_magic.max(2) as i32;
        }
        CardId::Consume => {
            // Consume.use: ApplyPower Focus, then DecreaseMaxOrbAction(1).
            player.add_power(PowerId::Focus, card.base_magic.max(2) as i32);
            decrease_max_orb_slots(player, 1);
        }
        CardId::Heatsinks => {
            player.add_power(PowerId::Heatsink, card.base_magic.max(1) as i32);
        }
        CardId::Loop => {
            player.add_power(PowerId::Loop, card.base_magic.max(1) as i32);
        }
        CardId::Skim => {
            let n = draw_cards_rng(player, card.base_magic.max(3) as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        CardId::Self_Repair => {
            player.add_power(PowerId::SelfRepair, card.base_magic.max(7) as i32);
        }
        CardId::The_Bomb => {
            player.add_power(PowerId::TheBomb, 3);
            if let Some(p) = player.powers.iter_mut().rev().find(|p| p.id == PowerId::TheBomb) {
                p.misc = if card.upgraded { 50 } else { 40 };
            }
        }
        CardId::Madness => {
            // MadnessAction: prefer a card with costForTurn > 0, else cost > 0.
            // getRandomCard(cardRandomRng) then reject until it qualifies.
            let better = player.hand.iter().any(|c| c.cost_for_turn > 0);
            let possible = player.hand.iter().any(|c| c.cost > 0);
            if better || possible {
                loop {
                    if player.hand.is_empty() {
                        break;
                    }
                    let i = rng.card_random.random_int(player.hand.len() as i32 - 1) as usize;
                    let c = &mut player.hand[i];
                    let ok = if better {
                        c.cost_for_turn > 0
                    } else {
                        c.cost > 0
                    };
                    if ok {
                        c.cost = 0;
                        c.cost_for_turn = 0;
                        break;
                    }
                }
            }
        }
        CardId::Thinking_Ahead => {
            let n = draw_cards_rng(player, 2, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
            if !player.hand.is_empty() {
                combat.need_put_on_deck = true;
            }
        }
        CardId::Forethought => {
            if !player.hand.is_empty() {
                combat.need_forethought = true;
                combat.need_put_on_deck = true;
            }
        }
        CardId::Mind_Blast => {
            // applyPowers: baseDamage = drawPile.size(), then DamageAction.
            let base = player.draw.len() as i32;
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, base, 1);
                }
            }
        }
        CardId::Chrysalis | CardId::Metamorphosis => {
            // use() rolls returnTrulyRandomCardInCombat N times, then queues
            // MakeTempCardInDrawPileAction(randomSpot). Inserts must not interleave
            // with the picks (seed 63: pick/insert/pick drew Steam Power).
            let n = card.base_magic.max(3) as i32;
            let typ = if card.id == CardId::Chrysalis {
                CardType::SKILL
            } else {
                CardType::ATTACK
            };
            if let Some(dungeon) = dungeon {
                let mut made = Vec::new();
                for _ in 0..n {
                    let Some(id) = crate::rewards::random_combat_card_of_type(dungeon, rng, typ)
                    else {
                        break;
                    };
                    let mut c = Card::new(id);
                    if c.cost > 0 {
                        c.cost = 0;
                        c.cost_for_turn = 0;
                    }
                    made.push(c);
                }
                for c in made {
                    add_to_random_spot(&mut player.draw, c, rng);
                }
            }
        }
        CardId::Secret_Technique | CardId::Secret_Weapon => {
            // SkillFromDeckToHandAction / AttackFromDeckToHandAction: matching
            // cards into tmp via addToRandomSpot (seed 509 Secret Weapon GRID).
            let want = if card.id == CardId::Secret_Weapon {
                CardType::ATTACK
            } else {
                CardType::SKILL
            };
            combat.skill_from_deck.clear();
            for (i, c) in player.draw.iter().enumerate() {
                if c.card_type() != want {
                    continue;
                }
                if combat.skill_from_deck.is_empty() {
                    combat.skill_from_deck.push(i);
                } else {
                    let idx = rng.card_random.random_int(combat.skill_from_deck.len() as i32 - 1)
                        as usize;
                    combat.skill_from_deck.insert(idx, i);
                }
            }
            match combat.skill_from_deck.len() {
                0 => {}
                1 => {
                    let i = combat.skill_from_deck[0];
                    combat.skill_from_deck.clear();
                    draw_pile_to_hand(player, i);
                }
                _ => combat.need_skill_from_deck = true,
            }
        }
        CardId::Purity => {
            if !player.hand.is_empty() {
                combat.need_exhaust_select = true;
            }
        }
        CardId::True_Grit => {
            if block > 0 {
                player.block += block;
            }
            if card.upgraded {
                if !player.hand.is_empty() {
                    combat.need_exhaust_select = true;
                }
            } else if !player.hand.is_empty() {
                let idx = rng.card_random.random_int(player.hand.len() as i32 - 1) as usize;
                let c = player.hand.remove(idx);
                exhaust_card(player, combat, c, rng);
            }
        }
        CardId::Defend_R | CardId::Defend_B | CardId::Shrug_It_Off | CardId::Armaments | CardId::Flame_Barrier => {
            if block > 0 {
                player.block += block;
            }
            if card.id == CardId::Shrug_It_Off {
                reshuffle_if_needed(player, rng);
                let n = crate::combat::draw_cards_rng(player, 1, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, n);
            }
            if card.id == CardId::Armaments && card.upgraded {
                for c in &mut player.hand {
                    c.upgrade();
                }
            }
        }
        CardId::Rage => player.add_power(PowerId::Rage, card.base_magic as i32),
        CardId::Fire_Breathing => player.add_power(PowerId::FireBreathing, card.base_magic as i32),
        CardId::Brutality => player.add_power(PowerId::Brutality, 1),
        CardId::Inflame => player.add_power(PowerId::Strength, card.base_magic as i32),
        CardId::Metallicize => player.add_power(PowerId::Metallicize, card.base_magic as i32),
        CardId::Barricade => player.add_power(PowerId::Barricade, 1),
        CardId::Feel_No_Pain => player.add_power(PowerId::FeelNoPain, card.base_magic.max(3) as i32),
        CardId::Dark_Embrace => player.add_power(PowerId::DarkEmbrace, 1),
        CardId::Flex => {
            player.add_power(PowerId::Strength, card.base_magic as i32);
            player.add_power(PowerId::LoseStrength, card.base_magic as i32);
        }
        CardId::Second_Wind => {
            let mut gained = 0;
            let mut i = 0;
            while i < player.hand.len() {
                if player.hand[i].card_type() != CardType::ATTACK {
                    let c = player.hand.remove(i);
                    exhaust_card(player, combat, c, rng);
                    gained += derived_block(card, player);
                } else {
                    i += 1;
                }
            }
            player.block += gained;
        }
        CardId::Pommel_Strike | CardId::Twin_Strike | CardId::Clothesline | CardId::Iron_Wave | CardId::Headbutt => {
            if card.id == CardId::Iron_Wave && block > 0 {
                player.block += block;
            }
            let hits = if card.id == CardId::Twin_Strike { 2 } else { 1 };
            if let Some(i) = target {
                if let Some(m) = combat.monsters.get_mut(i) {
                    damage_monster(m, player, rng, dmg, hits);
                    if card.id == CardId::Clothesline {
                        m.add_power(PowerId::Weak, card.base_magic as i32);
                    }
                }
            }
            if card.id == CardId::Pommel_Strike {
                reshuffle_if_needed(player, rng);
                let n = draw_cards_rng(player, card.base_magic as i32, Some(rng));
                apply_fire_breathing(player, &mut combat.monsters, n);
            }
        }
        CardId::J_A_X_ => {
            // JAX.use: LoseHPAction(3) then Strength magic (2, +1 upgraded).
            let dmg = on_lose_hp_last(player, intangible_player(player, 3));
            if dmg > 0 {
                player.hp -= dmg;
                red_skull_on_hp_change(player);
                centennial_puzzle_was_hp_lost(player, rng);
                let _ = try_cheat_death(player);
            }
            player.add_power(PowerId::Strength, card.base_magic.max(2) as i32);
        }
        CardId::Bloodletting => player.energy += card.base_magic as i32,
        CardId::Offering => {
            let dmg = on_lose_hp_last(player, intangible_player(player, 6));
            if dmg > 0 {
                player.hp -= dmg;
                red_skull_on_hp_change(player);
                centennial_puzzle_was_hp_lost(player, rng);
            }
            player.energy += 2;
            reshuffle_if_needed(player, rng);
            let n = draw_cards_rng(player, card.base_magic as i32, Some(rng));
            apply_fire_breathing(player, &mut combat.monsters, n);
        }
        _ => {
            if dmg > 0 {
                if let Some(i) = target {
                    if let Some(m) = combat.monsters.get_mut(i) {
                        damage_monster(m, player, rng, dmg, 1);
                    }
                } else {
                    for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                        damage_monster(monster, player, rng, dmg, 1);
                    }
                }
            }
            if block > 0 {
                player.block += block;
            }
        }
    }
}

/// GremlinHorn.onMonsterDeath: if this kill did not end combat, +1 energy and draw 1.
/// Sequential deaths in one card: the last remaining enemy dying does not trigger.
pub(crate) fn gremlin_horn_on_kills(
    player: &mut Player,
    combat: &mut Combat,
    rng: &mut RngSet,
    dead_before: usize,
) {
    if !player.has_relic(RelicId::Gremlin_Horn) {
        return;
    }
    let dead_after = combat.monsters.iter().filter(|m| m.dead).count();
    let newly = dead_after.saturating_sub(dead_before);
    if newly == 0 {
        return;
    }
    let remaining = combat.monsters.iter().filter(|m| m.alive()).count();
    let triggers = if remaining > 0 {
        newly
    } else {
        newly.saturating_sub(1)
    };
    for _ in 0..triggers {
        player.energy += 1;
        let n = draw_cards_rng(player, 1, Some(rng));
        apply_fire_breathing(player, &mut combat.monsters, n);
    }
}

fn is_end_turn_autoplay(id: CardId) -> bool {
    matches!(
        id,
        CardId::Burn | CardId::Decay | CardId::Doubt | CardId::Shame | CardId::Regret
    )
}

pub fn reshuffle_if_needed(player: &mut Player, rng: &mut RngSet) {
    if player.draw.is_empty() && !player.discard.is_empty() {
        // EmptyDeckShuffleAction ctor: relic.onShuffle, then shuffle discard
        // and souls.addToTop into draw. Draw was empty so append+shuffle of
        // the same cards with the same seed matches.
        on_shuffle_relics(player);
        let seed = rng.shuffle.random_long();
        shuffle_java(&mut player.discard, seed);
        player.draw.append(&mut player.discard);
    }
}

/// EmptyDeckShuffleAction constructor: Sundial every 3rd shuffle +2 energy,
/// Abacus GainBlock 6.
fn on_shuffle_relics(player: &mut Player) {
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Sundial) {
        if r.counter < 0 {
            r.counter = 0;
        }
        r.counter += 1;
        if r.counter == 3 {
            r.counter = 0;
            player.energy += 2;
        }
    }
    if player.has_relic(RelicId::TheAbacus) {
        gain_player_block(player, 6);
    }
}

pub fn end_turn(player: &mut Player, combat: &mut Combat, rng: &mut RngSet, dungeon: Option<&Dungeon>) {
    // GameActionManager.callEndOfTurnActions: applyEndOfTurnRelics then
    // applyEndOfTurnPreCardPowers. Orichalcum.onPlayerEndTurn addToTop GainBlock 6
    // if currentBlock==0, so it resolves before PlatedArmor/Metallicize addToBot.
    if player.has_relic(RelicId::Orichalcum) && player.block == 0 {
        player.block += 6;
    }
    // StoneCalendar.onPlayerEndTurn: 52 THORNS to all enemies when counter==7.
    if player
        .relics
        .iter()
        .any(|r| r.id == RelicId::StoneCalendar && r.counter == 7)
    {
        let dead_before = combat.monsters.iter().filter(|m| m.dead).count();
        for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
            deal_thorns(m, 52);
        }
        gremlin_horn_on_kills(player, combat, rng, dead_before);
        flush_spore_cloud(player, combat);
        if combat.all_dead() {
            return;
        }
    }
    let metal = player.power_amount(PowerId::Metallicize);
    if metal > 0 {
        player.block += metal;
    }
    let plated = player.power_amount(PowerId::PlatedArmor);
    if plated > 0 {
        player.block += plated;
    }
    crate::creature::end_of_turn(&mut player.powers);
    // GameActionManager.callEndOfTurnActions: addToBottom(TriggerEndOfTurnOrbsAction)
    // *then* hand.triggerOnEndOfTurnForPlayingCard. Burn.use queues DamageAction
    // addToBot, so Frost block resolves before each Burn hit. Apply orbs first,
    // then each Burn L-to-R so Fairy/Lizard Tail can revive mid-sequence.
    // LightningOrb with Electro can kill mid-EOT. GremlinHorn.onMonsterDeath
    // addToBot DrawCardAction, which resolves before DiscardAtEndOfTurn
    // (room.endTurn waits until the action queue is empty). The extra card
    // is then discarded with the rest of the hand (seed 906 AcidSlime_M).
    let dead_before_orbs = combat.monsters.iter().filter(|m| m.dead).count();
    apply_orb_passives(player, combat, rng);
    gremlin_horn_on_kills(player, combat, rng, dead_before_orbs);
    flush_spore_cloud(player, combat);
    if player.hp <= 0 {
        return;
    }
    // AbstractMonster.die: if areMonstersBasicallyDead, cleanCardQueue
    // drops CardQueueItems still in hand (Burn autoplays). Lightning EOT
    // killing Hexaghost therefore skips Burns (seed 968 Fairy 22 vs 8).
    if combat.all_dead() {
        return;
    }
    // triggerOnEndOfTurnForPlayingCard L-to-R after orbs. Burn 2/4, Decay
    // THORNS 2 (hits block). 453310 Decay after Frost left 1 HP vs JawWorm.
    // UseCardAction discards each autoplayed card as it resolves. A lethal
    // Burn (seed 255 Hexaghost, 2 HP + Burn in hand) cancels the rest of
    // EOT: remaining cards stay in hand, DiscardAtEndOfTurn is skipped.
    // Regret.use LoseHP(hand.size()) is captured at trigger, before discard.
    let regret_n = player.hand.len() as i32;
    let mut i = 0;
    while i < player.hand.len() {
        let id = player.hand[i].id;
        if !is_end_turn_autoplay(id) {
            i += 1;
            continue;
        }
        let upgraded = player.hand[i].upgraded;
        match id {
            CardId::Shame => {
                // Shame.use: FrailPower(player, 1, true) — justApplied.
                player.add_power_from_monster(PowerId::Frail, 1);
            }
            CardId::Doubt => {
                player.add_power_from_monster(PowerId::Weak, 1);
            }
            CardId::Regret => {
                let dmg = on_lose_hp_last(player, intangible_player(player, regret_n));
                if dmg > 0 {
                    player.hp -= dmg;
                    if player.hp < 0 {
                        player.hp = 0;
                    }
                    let _ = try_cheat_death(player);
                    red_skull_on_hp_change(player);
                    centennial_puzzle_was_hp_lost(player, rng);
                }
            }
            CardId::Burn | CardId::Decay => {
                let raw = if id == CardId::Burn && upgraded { 4 } else { 2 };
                let dmg = intangible_player(player, raw);
                let dmg = apply_block(&mut player.block, dmg);
                let dmg = buffer_absorb(player, dmg);
                let dmg = on_lose_hp_last(player, dmg);
                if dmg > 0 {
                    player.hp -= dmg;
                    if player.hp < 0 {
                        player.hp = 0;
                    }
                    let _ = try_cheat_death(player);
                    red_skull_on_hp_change(player);
                    centennial_puzzle_was_hp_lost(player, rng);
                }
            }
            _ => {}
        }
        let card = player.hand.remove(i);
        player.discard.push(card);
        if player.hp <= 0 {
            return;
        }
    }
    // RegenPower.atEndOfTurn is AbstractRoom.endTurn, after
    // callEndOfTurnActions (orbs + Burn). RegenAction is addToTop only while
    // phase==COMBAT, so a lethal lightning skips it (seed 714 hp 16 vs 14).
    if !combat.all_dead() {
        let regen = player.power_amount(PowerId::Regen);
        if regen > 0 {
            player.hp = (player.hp + regen).min(player.max_hp);
            red_skull_on_hp_change(player);
            if let Some(p) = player.powers.iter_mut().find(|p| p.id == PowerId::Regen) {
                p.amount -= 1;
            }
            player.powers.retain(|p| p.id != PowerId::Regen || p.amount > 0);
        }
    }
    // DiscardAtEndOfTurnAction: retain/selfRetain cards are pulled to limbo
    // first (not yet modeled). Runic Pyramid and Equilibrium skip the
    // DiscardAction loop; ethereal still exhausts via triggerOnEndOfPlayerTurn.
    let keep_hand = player.has_relic(RelicId::Runic_Pyramid);
    let mut rest = Vec::new();
    for card in player.hand.drain(..) {
        if is_end_turn_autoplay(card.id) && !keep_hand {
            player.discard.push(card);
        } else {
            rest.push(card);
        }
    }
    let mut kept = Vec::new();
    for card in rest.into_iter().rev() {
        if card.ethereal {
            player.exhaust.push(card);
        } else if keep_hand {
            kept.push(card);
        } else {
            player.discard.push(card);
        }
    }
    if keep_hand {
        kept.reverse();
        player.hand = kept;
    }

    if combat.all_dead() {
        return;
    }

    // TheBombPower.atEndOfTurn: each unique fuse ticks independently. When
    // amount==1 it queues explode; if that kills the room, later fuses and
    // the rest of EOT are cancelled (clearPostCombatActions).
    let mut bomb_i = 0;
    while bomb_i < player.powers.len() {
        if player.powers[bomb_i].id != PowerId::TheBomb {
            bomb_i += 1;
            continue;
        }
        let explode = player.powers[bomb_i].amount == 1;
        let bomb_dmg = player.powers[bomb_i].misc;
        player.powers[bomb_i].amount -= 1;
        if player.powers[bomb_i].amount <= 0 {
            player.powers.remove(bomb_i);
        } else {
            bomb_i += 1;
        }
        if explode {
            for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
                let dealt = apply_block(&mut monster.block, bomb_dmg);
                if dealt > 0 {
                    monster.hp -= dealt;
                    if monster.hp <= 0 {
                        monster.hp = 0;
                        if monster.id == MonsterId::Darkling {
                            monster.half_dead = true;
                            monster.dead = false;
                            monster.powers.clear();
                            monster.set_move(4, Intent::Unknown, 0, 1);
                        } else {
                            monster.dead = true;
                        }
                    }
                }
            }
            if combat.all_dead() {
                return;
            }
        }
    }

    for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
        if monster.power_amount(PowerId::Barricade) == 0 {
            monster.block = 0;
        }
    }

    for m in &mut combat.monsters {
        m.just_spawned = false;
    }
    let mut i = 0;
    while i < combat.monsters.len() {
        if !combat.monsters[i].alive() || combat.monsters[i].just_spawned {
            i += 1;
            continue;
        }
        let skip_roll = combat.monsters[i].skip_roll_after_turn();
        let id = combat.monsters[i].id;
        let used_move = combat.monsters[i].next_move;
        let spawned = combat.monsters[i].take_turn(player, rng, combat.ascension);
        let static_n = player.pending_static;
        player.pending_static = 0;
        for _ in 0..static_n {
            channel_orb(player, combat, rng, OrbKind::Lightning);
        }
        flush_mid_hit_evokes(player, combat, rng);
        apply_group_move(combat, i, id, used_move, rng);
        if player.hp <= 0 && !try_cheat_death(player) {
            player.hp = 0;
            return;
        }
        flush_spore_cloud(player, combat);
        // ExplosivePower.duringTurn after takeTurn (GameActionManager).
        let fading = combat.monsters[i].power_amount(PowerId::Fading);
        if fading > 0 && combat.monsters[i].alive() {
            if fading == 1 {
                combat.monsters[i].hp = 0;
                combat.monsters[i].dead = true;
            } else if let Some(p) = combat.monsters[i].powers.iter_mut().find(|p| p.id == PowerId::Fading) {
                p.amount -= 1;
            }
        }
        let explosive = combat.monsters[i].power_amount(PowerId::Explosive);
        if explosive > 0 && combat.monsters[i].alive() {
            if explosive == 1 {
                let dealt = intangible_player(player, 30);
                let dealt = apply_block(&mut player.block, dealt);
                let dealt = buffer_absorb(player, dealt);
                let dealt = on_lose_hp_last(player, dealt);
                if dealt > 0 {
                    player.hp -= dealt;
                    if player.hp < 0 {
                        player.hp = 0;
                    }
                    let _ = try_cheat_death(player);
                    red_skull_on_hp_change(player);
                    centennial_puzzle_was_hp_lost(player, rng);
                }
                combat.monsters[i].hp = 0;
                combat.monsters[i].dead = true;
            } else if let Some(p) = combat.monsters[i].powers.iter_mut().find(|p| p.id == PowerId::Explosive) {
                p.amount -= 1;
            }
        }
        if let Some(p) = combat.monsters[i].powers.iter_mut().find(|p| p.id == PowerId::Malleable) {
            p.amount = 3;
        }
        let plated = combat.monsters[i].power_amount(PowerId::PlatedArmor);
        if plated > 0 && combat.monsters[i].alive() {
            combat.monsters[i].block += plated;
        }
        let metal = combat.monsters[i].power_amount(PowerId::Metallicize);
        if metal > 0 && combat.monsters[i].alive() {
            combat.monsters[i].block += metal;
        }
        let regen = combat.monsters[i].power_amount(PowerId::Regen);
        if regen > 0 && combat.monsters[i].alive() && !combat.monsters[i].half_dead {
            combat.monsters[i].hp = (combat.monsters[i].hp + regen).min(combat.monsters[i].max_hp);
        }
        if let Some(kids) = spawned {
            let mut parent_idx = i;
            for mut kid in kids {
                kid.just_spawned = true;
                let pos = smart_spawn_index(&combat.monsters, kid.offset_x);
                combat.monsters.insert(pos, kid);
                if pos <= parent_idx {
                    parent_idx += 1;
                }
            }
            if !skip_roll {
                combat.monsters[parent_idx].roll_move(rng);
            }
            i = parent_idx + 1;
            continue;
        }
        if combat.monsters[i].alive() && !skip_roll {
            let missing: i32 = combat
                .monsters
                .iter()
                .filter(|m| m.alive())
                .map(|m| (m.max_hp - m.hp).max(0))
                .sum();
            let allies = combat.monsters.iter().filter(|m| m.alive()).count() as i32;
            combat.monsters[i].roll_move_group(rng, missing, allies, i as i32);
        }
        // AcidSlime_L.damage: setMove(SPLIT) plus addToBottom SetMoveAction
        // after RollMoveAction, so a thorns hit that crosses 50% HP during
        // takeTurn still wins over getMove (seed 776 Bronze Scales).
        if combat.monsters[i].alive()
            && matches!(
                combat.monsters[i].id,
                MonsterId::AcidSlimeL | MonsterId::SpikeSlimeL
            )
            && combat.monsters[i].split_triggered
            && combat.monsters[i].hp <= combat.monsters[i].max_hp / 2
        {
            combat.monsters[i].set_move(3, Intent::Unknown, 0, 1);
        }
        i += 1;
    }
    resolve_darklings(combat);

    if player.hp <= 0 && !try_cheat_death(player) {
        player.hp = 0;
        return;
    }
    if combat.all_dead() {
        return;
    }

    for monster in &mut combat.monsters {
        let ritual = crate::creature::end_of_round(&mut monster.powers);
        if ritual != 0 {
            monster.add_power(PowerId::Strength, ritual);
        }
    }
    let _ = crate::creature::end_of_round(&mut player.powers);
    combat.turn += 1;
    for monster in combat.monsters.iter_mut().filter(|m| m.alive()) {
        // FlightPower.atStartOfTurn: amount = storedAmount (Byrd 3, A17 4).
        // Hits only ReducePower the current amount; the power stays until
        // stacks hit 0 and onRemove grounds the Byrd (seed 5 Sweeping Beam).
        if let Some(p) = monster.powers.iter_mut().find(|p| p.id == PowerId::Flight) {
            if p.misc > 0 {
                p.amount = p.misc;
            }
        }
        monster.create_intent();
    }
    // Pocketwatch.atTurnStartPostDraw: if previous turn played <=3 cards and
    // this is not the first turn, DrawCardAction(3) after the turn's draw.
    let pocketwatch = player.has_relic(RelicId::Pocketwatch)
        && combat.turn > 1
        && combat.cards_played_this_turn <= 3;
    combat.cards_played_this_turn = 0;
    combat.skills_this_turn = 0;
    combat.attacks_this_turn = 0;
    // GameActionManager.getNextAction: Barricade/Blur keep block; Calipers
    // loseBlock(15); otherwise loseBlock() all.
    if player.power_amount(PowerId::Barricade) == 0 {
        if player.has_relic(RelicId::Calipers) {
            player.block = (player.block - 15).max(0);
        } else {
            player.block = 0;
        }
    }
    // CreativeAI / Hello World roll cardRandomRng immediately in
    // atStartOfTurn. Loop only queues LightningOrbPassiveAction, so the
    // POWER pick must happen before Loop's lightning getRandomMonster
    // (seed 937 Electrodynamics vs Machine Learning).
    let ai = player.power_amount(PowerId::CreativeAI);
    if ai > 0 {
        if let Some(dungeon) = dungeon {
            for _ in 0..ai {
                if let Some(id) = crate::rewards::random_power_in_combat(dungeon, rng) {
                    if player.hand.len() < 10 {
                        player.hand.push(Card::new(id));
                    }
                }
            }
        }
    }
    let hello = player.power_amount(PowerId::HelloWorld);
    if hello > 0 {
        if let Some(dungeon) = dungeon {
            for _ in 0..hello {
                if dungeon.common_cards.is_empty() {
                    break;
                }
                let i = rng.card_random.random_int(dungeon.common_cards.len() as i32 - 1) as usize;
                let id = dungeon.common_cards[i];
                if player.hand.len() < 10 {
                    player.hand.push(Card::new(id));
                } else {
                    player.discard.push(Card::new(id));
                }
            }
        }
    }
    // LoopPower.atStartOfTurn calls orb onEndOfTurn while BiasPower still
    // only has an addToBot Focus-1 queued, so lightning/frost/dark snapshot
    // the pre-Bias amount. Apply Loop before Bias, after CreativeAI RNG.
    let loops = player.power_amount(PowerId::Loop);
    for _ in 0..loops {
        apply_front_orb_passive(player, combat, rng);
    }
    flush_guardian_defensive_block(combat);
    let bias = player.power_amount(PowerId::Bias);
    if bias > 0 {
        player.add_power(PowerId::Focus, -bias);
    }
    player.energy = player.energy_master;
    let energized = player.power_amount(PowerId::Energized);
    if energized > 0 {
        player.energy += energized;
        player.powers.retain(|p| p.id != PowerId::Energized);
    }
    if player.has_relic(RelicId::Happy_Flower) {
        if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Happy_Flower) {
            r.counter += 1;
            if r.counter == 3 {
                r.counter = 0;
                player.energy += 1;
            }
        }
    }
    tick_turn_start_block_relics(player);
    if player.has_relic(RelicId::Mercury_Hourglass) {
        let dead_before = combat.monsters.iter().filter(|m| m.dead).count();
        for m in combat.monsters.iter_mut().filter(|m| m.alive()) {
            deal_thorns(m, 3);
        }
        gremlin_horn_on_kills(player, combat, rng, dead_before);
        flush_spore_cloud(player, combat);
        if combat.all_dead() {
            return;
        }
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Letter_Opener) {
        r.counter = 0;
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Kunai) {
        r.counter = 0;
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Shuriken) {
        r.counter = 0;
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Ornamental_Fan) {
        r.counter = 0;
    }
    if player.power_amount(PowerId::Brutality) > 0 {
        let n = player.power_amount(PowerId::Brutality);
        let dmg = on_lose_hp_last(player, intangible_player(player, n));
        if dmg > 0 {
            player.hp -= dmg;
            red_skull_on_hp_change(player);
            centennial_puzzle_was_hp_lost(player, rng);
        }
        reshuffle_if_needed(player, rng);
        let statuses = draw_cards_rng(player, n, Some(rng));
        apply_fire_breathing(player, &mut combat.monsters, statuses);
    }
    // DrawPower (Machine Learning) bumps gameHandSize; DrawCardAction uses that.
    let draw_n = 5 + player.power_amount(PowerId::DrawCard);
    let statuses = draw_cards_rng(player, draw_n, Some(rng));
    apply_fire_breathing(player, &mut combat.monsters, statuses);
    if pocketwatch {
        let n = draw_cards_rng(player, 3, Some(rng));
        apply_fire_breathing(player, &mut combat.monsters, n);
    }
    // MayhemPower.atStartOfTurn queues a wrapper addToBot whose update
    // addToBot PlayTopCardAction. DrawCardAction is queued after the wrapper,
    // so the autoplay is after the turn's draw (seed 533 Genetic Algorithm).
    let mayhem = player.power_amount(PowerId::Mayhem);
    if mayhem > 0 {
        let mut targets = Vec::with_capacity(mayhem as usize);
        for _ in 0..mayhem {
            targets.push(random_alive_monster(combat, &mut rng.card_random));
        }
        play_top_cards(player, combat, &targets, false, rng, dungeon);
    }
}



fn tick_turn_start_block_relics(player: &mut Player) {
    // IncenseBurner.atTurnStart: counter 0 onEquip, +1 each turn, Intangible
    // at 6 then reset. Default -1 (never equipped) steps to 1 like Java.
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::StoneCalendar) {
        r.counter += 1;
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::Incense_Burner) {
        if r.counter == -1 {
            r.counter += 2;
        } else {
            r.counter += 1;
        }
        if r.counter == 6 {
            r.counter = 0;
            player.add_power(PowerId::Intangible, 1);
        }
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::HornCleat) {
        if r.counter >= 0 {
            r.counter += 1;
        }
        if r.counter == 2 {
            player.block += 14;
            r.counter = -1;
        }
    }
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::CaptainsWheel) {
        if r.counter >= 0 {
            r.counter += 1;
        }
        if r.counter == 3 {
            player.block += 18;
            r.counter = -1;
        }
    }
}

pub fn after_combat_relics(player: &mut Player) {
    // AbstractRoom.endBattle: Meat on the Bone.onTrigger before onVictory.
    if player.has_relic(RelicId::Meat_on_the_Bone)
        && player.hp > 0
        && (player.hp as f32) <= (player.max_hp as f32) / 2.0
    {
        player.hp = (player.hp + 12).min(player.max_hp);
        red_skull_on_hp_change(player);
    }
    if player.has_relic(RelicId::Burning_Blood) && player.hp > 0 {
        player.hp = (player.hp + 6).min(player.max_hp);
    }
    let repair = player.power_amount(PowerId::SelfRepair);
    if repair > 0 && player.hp > 0 {
        player.hp = (player.hp + repair).min(player.max_hp);
    }
    // Piles clear; block/powers stay until nextRoomTransition.resetPlayer.
    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    player.exhaust.clear();
    player.duplication = 0;
    player.orbs.clear();
    if let Some(r) = player.relics.iter_mut().find(|r| r.id == RelicId::StoneCalendar) {
        r.counter = -1;
    }
}

fn focus_of(player: &Player) -> i32 {
    player.power_amount(PowerId::Focus)
}

fn orb_passive_amount(kind: OrbKind, focus: i32) -> i32 {
    let base = match kind {
        OrbKind::Lightning => 3,
        OrbKind::Frost => 2,
        OrbKind::Dark => 0,
        OrbKind::Plasma => 1,
    };
    (base + focus).max(0)
}

fn orb_evoke_amount(orb: Orb, focus: i32) -> i32 {
    match orb.kind {
        OrbKind::Lightning => (8 + focus).max(0),
        OrbKind::Frost => (5 + focus).max(0),
        // Dark.applyFocus only updates passiveAmount. onEvoke uses evokeAmount as stored.
        OrbKind::Dark => orb.evoke.max(0),
        OrbKind::Plasma => 2,
    }
}

fn dark_passive_gain(focus: i32) -> i32 {
    (6 + focus).max(0)
}

/// DarkImpulseAction: every Dark orb onEndOfTurn, then Cables extra on the front Dark.
fn impulse_dark_orbs(player: &mut Player) {
    let gain = dark_passive_gain(focus_of(player));
    for orb in player.orbs.iter_mut() {
        if orb.kind == OrbKind::Dark {
            orb.evoke += gain;
        }
    }
    if player.has_relic(RelicId::Cables) {
        if let Some(front) = player.orbs.first_mut() {
            if front.kind == OrbKind::Dark {
                front.evoke += gain;
            }
        }
    }
}

/// AbstractPlayer.increaseMaxOrbSlots: combat `maxOrbs` only, no-op at 10.
/// IncreaseMaxOrbAction calls this with 1 per potency point.
pub fn increase_max_orb_slots(player: &mut Player, amount: i32) {
    for _ in 0..amount {
        if player.max_orbs == 10 {
            break;
        }
        player.max_orbs += 1;
    }
}

/// AbstractPlayer.decreaseMaxOrbSlots: drop the last slot (empty or filled) without evoking.
fn decrease_max_orb_slots(player: &mut Player, amount: i32) {
    for _ in 0..amount {
        if player.max_orbs <= 0 {
            break;
        }
        player.max_orbs -= 1;
        if player.orbs.len() > player.max_orbs as usize {
            player.orbs.pop();
        }
    }
}

/// StaticDischargePower.onAttacked addToTop(ChannelAction(Lightning)).
/// Frost evoke is GainBlockAction addToTop, so block must land before the
/// next hit. Lightning/Dark evokes need Combat; stash amounts until
/// `flush_mid_hit_evokes` after take_turn.
fn channel_static_lightning_mid_hit(player: &mut Player) {
    if player.max_orbs <= 0 {
        return;
    }
    if player.orbs.len() >= player.max_orbs as usize {
        let Some(&orb) = player.orbs.first() else {
            return;
        };
        let amt = orb_evoke_amount(orb, focus_of(player));
        match orb.kind {
            OrbKind::Frost => gain_player_block(player, amt),
            OrbKind::Lightning => player.pending_evoke_lightning.push(amt),
            OrbKind::Dark => player.pending_evoke_dark.push(amt),
            OrbKind::Plasma => player.energy += amt,
        }
        player.orbs.remove(0);
    }
    if player.orbs.len() < player.max_orbs as usize {
        player.orbs.push(Orb {
            kind: OrbKind::Lightning,
            evoke: 0,
        });
    }
}

fn flush_mid_hit_evokes(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    let lightning = std::mem::take(&mut player.pending_evoke_lightning);
    for amt in lightning {
        lightning_hit_player(Some(player), combat, rng, amt);
    }
    let dark = std::mem::take(&mut player.pending_evoke_dark);
    for amt in dark {
        dark_evoke_hit(combat, amt);
    }
}

pub fn channel_orb(player: &mut Player, combat: &mut Combat, rng: &mut RngSet, kind: OrbKind) {
    // ChannelAction is dropped by GameActionManager.clearPostCombatActions
    // (658249: duplicated Glacier's last Frost channel after Lightning evoke
    // killed Jaw Worm — Java block 19, rust kept channeling to 24).
    if combat.all_dead() {
        return;
    }
    if player.max_orbs <= 0 {
        return;
    }
    if player.orbs.len() >= player.max_orbs as usize {
        evoke_front(player, combat, rng, true);
    }
    if player.orbs.len() < player.max_orbs as usize {
        player.orbs.push(Orb {
            kind,
            evoke: if kind == OrbKind::Dark { 6 } else { 0 },
        });
        combat.orbs_channeled_this_combat.push(kind);
    }
}

fn evoke_front(player: &mut Player, combat: &mut Combat, rng: &mut RngSet, remove: bool) {
    let Some(&orb) = player.orbs.first() else {
        return;
    };
    apply_evoke(player, combat, rng, orb);
    if remove {
        player.orbs.remove(0);
    }
}

fn apply_evoke(player: &mut Player, combat: &mut Combat, rng: &mut RngSet, orb: Orb) {
    let amt = orb_evoke_amount(orb, focus_of(player));
    match orb.kind {
        OrbKind::Lightning => lightning_hit_player(Some(player), combat, rng, amt),
        OrbKind::Frost => gain_player_block(player, amt),
        OrbKind::Dark => dark_evoke_hit(combat, amt),
        OrbKind::Plasma => player.energy += amt,
    }
}

fn apply_front_orb_passive(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    let Some(&orb) = player.orbs.first() else {
        return;
    };
    let amt = orb_passive_amount(orb.kind, focus_of(player));
    match orb.kind {
        OrbKind::Lightning => lightning_hit_player(Some(player), combat, rng, amt),
        OrbKind::Frost => gain_player_block(player, amt),
        OrbKind::Dark => {
            let gain = dark_passive_gain(focus_of(player));
            if let Some(front) = player.orbs.first_mut() {
                front.evoke += gain;
            }
        }
        OrbKind::Plasma => {}
    }
}

fn apply_orb_passives(player: &mut Player, combat: &mut Combat, rng: &mut RngSet) {
    let focus = focus_of(player);
    let electro = player.power_amount(PowerId::Electro) > 0;
    let n = player.orbs.len();
    for i in 0..n {
        let kind = player.orbs[i].kind;
        let amt = orb_passive_amount(kind, focus);
        match kind {
            OrbKind::Lightning => {
                if electro {
                    lightning_hit_player(Some(player), combat, rng, amt);
                } else {
                    lightning_hit(combat, rng, amt);
                }
            }
            OrbKind::Frost => gain_player_block(player, amt),
            // Dark.onEndOfTurn: this.evokeAmount += this.passiveAmount (6+Focus).
            OrbKind::Dark => player.orbs[i].evoke += dark_passive_gain(focus),
            OrbKind::Plasma => {}
        }
    }
    // TriggerEndOfTurnOrbsAction: Cables extra onEndOfTurn on the front filled orb.
    if player.has_relic(RelicId::Cables) {
        apply_front_orb_passive(player, combat, rng);
    }
    flush_guardian_defensive_block(combat);
}

fn damage_random_alive(player: &mut Player, combat: &mut Combat, rng: &mut RngSet, dmg: i32) {
    let alive: Vec<usize> = combat
        .monsters
        .iter()
        .enumerate()
        .filter(|(_, m)| m.alive() && !m.half_dead && !m.escaped)
        .map(|(i, _)| i)
        .collect();
    if alive.is_empty() {
        return;
    }
    let pick = rng.card_random.random_int(alive.len() as i32 - 1) as usize;
    if let Some(m) = combat.monsters.get_mut(alive[pick]) {
        damage_monster(m, player, rng, dmg, 1);
    }
}

fn lightning_hit(combat: &mut Combat, rng: &mut RngSet, amount: i32) {
    lightning_hit_player(None, combat, rng, amount);
}

/// DarkOrbEvokeAction ctor: first `!isDeadOrEscaped` monster with lowest `currentHealth`.
/// Ties keep the earlier monster. No cardRandomRng.
fn dark_evoke_hit(combat: &mut Combat, amount: i32) {
    if amount <= 0 {
        return;
    }
    let mut best: Option<usize> = None;
    for (i, m) in combat.monsters.iter().enumerate() {
        // AbstractCreature.isDeadOrEscaped: isDying || halfDead || isEscaping
        if m.dead || m.escaped || m.half_dead || m.hp <= 0 {
            continue;
        }
        match best {
            None => best = Some(i),
            Some(b) if m.hp < combat.monsters[b].hp => best = Some(i),
            _ => {}
        }
    }
    if let Some(i) = best {
        if let Some(m) = combat.monsters.get_mut(i) {
            deal_thorns(m, apply_lock_on(m, amount));
        }
    }
}

fn lightning_hit_player(player: Option<&Player>, combat: &mut Combat, rng: &mut RngSet, amount: i32) {
    if amount <= 0 {
        return;
    }
    let electro = player.is_some_and(|p| p.power_amount(PowerId::Electro) > 0);
    let alive: Vec<usize> = combat
        .monsters
        .iter()
        .enumerate()
        .filter(|(_, m)| m.alive() && !m.half_dead)
        .map(|(i, _)| i)
        .collect();
    if alive.is_empty() {
        return;
    }
    if electro {
        for i in alive {
            if let Some(m) = combat.monsters.get_mut(i) {
                let amt = apply_lock_on(m, amount);
                deal_thorns(m, amt);
            }
        }
        return;
    }
    let pick = rng.card_random.random_int(alive.len() as i32 - 1) as usize;
    if let Some(m) = combat.monsters.get_mut(alive[pick]) {
        let amt = apply_lock_on(m, amount);
        deal_thorns(m, amt);
    }
}

/// AbstractOrb.applyLockOn: `(int)(dmg * 1.5F)` when the target has Lockon.
fn apply_lock_on(monster: &Monster, dmg: i32) -> i32 {
    if monster.power_amount(PowerId::LockOn) > 0 {
        (dmg as f32 * 1.5) as i32
    } else {
        dmg
    }
}

fn monster_is_attacking(m: &Monster) -> bool {
    matches!(
        m.intent,
        Intent::Attack | Intent::AttackBuff | Intent::AttackDebuff | Intent::AttackDefend
    )
}
