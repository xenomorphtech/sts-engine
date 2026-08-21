use crate::card::CardStats;
use crate::ids::{CardId, EncounterId, MonsterId};

pub fn card_stats(id: CardId, upgraded: bool) -> CardStats {
    match (id, upgraded) {
        (CardId::Strike_R, false) => CardStats::attack(1, 6),
        (CardId::Strike_R, true) => CardStats::attack(1, 9),
        (CardId::Defend_R, false) => CardStats::skill(1, 5, -1),
        (CardId::Defend_R, true) => CardStats::skill(1, 8, -1),
        (CardId::Bash, false) => CardStats {
            cost: 2,
            damage: 8,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Bash, true) => CardStats {
            cost: 2,
            damage: 10,
            block: -1,
            magic: 3,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Anger, false) => CardStats::attack(0, 6),
        (CardId::Anger, true) => CardStats::attack(0, 8),
        (CardId::Armaments, false) => CardStats::skill(1, 5, -1),
        (CardId::Armaments, true) => CardStats::skill(1, 5, -1),
        (CardId::Cleave, false) => CardStats::attack(1, 8),
        (CardId::Cleave, true) => CardStats::attack(1, 11),
        (CardId::Clothesline, false) => CardStats {
            cost: 2,
            damage: 12,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Clothesline, true) => CardStats {
            cost: 2,
            damage: 14,
            block: -1,
            magic: 3,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Flex, false) => CardStats::skill(0, -1, 2),
        (CardId::Flex, true) => CardStats::skill(0, -1, 4),
        (CardId::Havoc, false) => CardStats::skill(1, -1, -1),
        (CardId::Havoc, true) => CardStats::skill(0, -1, -1),
        (CardId::Headbutt, false) => CardStats::attack(1, 9),
        (CardId::Headbutt, true) => CardStats::attack(1, 12),
        (CardId::Heavy_Blade, false) => CardStats {
            cost: 2,
            damage: 14,
            block: -1,
            magic: 3,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Heavy_Blade, true) => CardStats {
            cost: 2,
            damage: 14,
            block: -1,
            magic: 5,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Iron_Wave, false) => CardStats {
            cost: 1,
            damage: 5,
            block: 5,
            magic: -1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Iron_Wave, true) => CardStats {
            cost: 1,
            damage: 7,
            block: 7,
            magic: -1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Perfected_Strike, false) => CardStats {
            cost: 2,
            damage: 6,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Perfected_Strike, true) => CardStats {
            cost: 2,
            damage: 6,
            block: -1,
            magic: 3,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Pommel_Strike, false) => CardStats {
            cost: 1,
            damage: 9,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Pommel_Strike, true) => CardStats {
            cost: 1,
            damage: 10,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Shrug_It_Off, false) => CardStats::skill(1, 8, 1),
        (CardId::Shrug_It_Off, true) => CardStats::skill(1, 11, 1),
        (CardId::Sword_Boomerang, false) => CardStats {
            cost: 1,
            damage: 3,
            block: -1,
            magic: 3,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Sword_Boomerang, true) => CardStats {
            cost: 1,
            damage: 3,
            block: -1,
            magic: 4,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Thunderclap, false) => CardStats {
            cost: 1,
            damage: 4,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Thunderclap, true) => CardStats {
            cost: 1,
            damage: 7,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::True_Grit, false) => CardStats::skill(1, 7, -1),
        (CardId::True_Grit, true) => CardStats::skill(1, 9, -1),
        (CardId::Twin_Strike, false) => CardStats {
            cost: 1,
            damage: 5,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Twin_Strike, true) => CardStats {
            cost: 1,
            damage: 7,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Warcry, false) => CardStats::skill(0, -1, 1),
        (CardId::Warcry, true) => CardStats::skill(0, -1, 2),
        (CardId::Wild_Strike, false) => CardStats::attack(1, 12),
        (CardId::Wild_Strike, true) => CardStats::attack(1, 17),
        (CardId::Battle_Trance, false) => CardStats::skill(0, -1, 3),
        (CardId::Battle_Trance, true) => CardStats::skill(0, -1, 4),
        (CardId::Bloodletting, false) => CardStats::skill(0, -1, 2),
        (CardId::Bloodletting, true) => CardStats::skill(0, -1, 3),
        (CardId::Burning_Pact, false) => CardStats::skill(1, -1, 2),
        (CardId::Burning_Pact, true) => CardStats::skill(1, -1, 3),
        (CardId::Strike_B, false) => CardStats::attack(1, 6),
        (CardId::Strike_B, true) => CardStats::attack(1, 9),
        (CardId::Defend_B, false) => CardStats::skill(1, 5, -1),
        (CardId::Defend_B, true) => CardStats::skill(1, 8, -1),
        (CardId::Zap, false) => CardStats::skill(1, -1, 1),
        (CardId::Zap, true) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Dualcast, false) => CardStats::skill(1, -1, -1),
        (CardId::Dualcast, true) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Ball_Lightning, false) => CardStats::attack(1, 7),
        (CardId::Ball_Lightning, true) => CardStats::attack(1, 10),
        (CardId::Cold_Snap, false) => CardStats::attack(1, 6),
        (CardId::Cold_Snap, true) => CardStats::attack(1, 9),
        (CardId::Blizzard, false) => CardStats {
            cost: 1, damage: 0, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Blizzard, true) => CardStats {
            cost: 1, damage: 0, block: -1, magic: 3, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Beam_Cell, false) => CardStats {
            cost: 0, damage: 3, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Beam_Cell, true) => CardStats {
            cost: 0, damage: 4, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Go_for_the_Eyes, false) => CardStats {
            cost: 0, damage: 3, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Go_for_the_Eyes, true) => CardStats {
            cost: 0, damage: 4, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Sweeping_Beam, false) => CardStats {
            cost: 1, damage: 6, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Sweeping_Beam, true) => CardStats {
            cost: 1, damage: 9, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Compile_Driver, false) => CardStats::attack(1, 7),
        (CardId::Compile_Driver, true) => CardStats::attack(1, 10),
        (CardId::Coolheaded, false) => CardStats::skill(1, -1, 1),
        (CardId::Coolheaded, true) => CardStats::skill(1, -1, 2),
        (CardId::Chill, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Chill, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: true,
        },
        (CardId::FTL, false) => CardStats {
            cost: 0, damage: 5, block: -1, magic: 3, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::FTL, true) => CardStats {
            cost: 0, damage: 6, block: -1, magic: 4, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Conserve_Battery, false) => CardStats::skill(1, 7, -1),
        (CardId::Conserve_Battery, true) => CardStats::skill(1, 10, -1),
        (CardId::Leap, false) => CardStats::skill(1, 9, -1),
        (CardId::Leap, true) => CardStats::skill(1, 12, -1),
        (CardId::Tempest, _) => CardStats {
            cost: -1,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Hologram, false) => CardStats {
            cost: 1, damage: -1, block: 3, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Hologram, true) => CardStats {
            cost: 1, damage: -1, block: 5, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Stack, false) => CardStats::skill(1, 0, -1),
        (CardId::Stack, true) => CardStats::skill(1, 3, -1),
        (CardId::Steam, false) => CardStats::skill(0, 6, -1),
        (CardId::Steam, true) => CardStats::skill(0, 8, -1),
        (CardId::Auto_Shields, false) => CardStats::skill(1, 11, -1),
        (CardId::Auto_Shields, true) => CardStats::skill(1, 15, -1),
        (CardId::BootSequence, u) => CardStats {
            cost: 0,
            damage: -1,
            block: if u { 13 } else { 10 },
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: true,
        },
        (CardId::Force_Field, false) => CardStats::skill(4, 12, -1),
        (CardId::Force_Field, true) => CardStats::skill(4, 16, -1),
        (CardId::Buffer, false) => CardStats::skill(2, -1, 1),
        (CardId::Buffer, true) => CardStats::skill(2, -1, 2),
        (CardId::Amplify, false) => CardStats::skill(1, -1, 1),
        (CardId::Amplify, true) => CardStats::skill(1, -1, 2),
        (CardId::Defragment, false) => CardStats::skill(1, -1, 1),
        (CardId::Defragment, true) => CardStats::skill(1, -1, 2),
        (CardId::Biased_Cognition, false) => CardStats::skill(1, -1, 4),
        (CardId::Biased_Cognition, true) => CardStats::skill(1, -1, 5),
        (CardId::Glacier, false) => CardStats {
            cost: 2, damage: -1, block: 7, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Glacier, true) => CardStats {
            cost: 2, damage: -1, block: 10, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Melter, false) => CardStats::attack(1, 10),
        (CardId::Melter, true) => CardStats::attack(1, 14),
        (CardId::Streamline, false) => CardStats {
            cost: 2, damage: 15, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Streamline, true) => CardStats {
            cost: 2, damage: 20, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Swift_Strike, false) => CardStats::attack(0, 7),
        (CardId::Swift_Strike, true) => CardStats::attack(0, 10),
        (CardId::Barrage, false) => CardStats::attack(1, 4),
        (CardId::Barrage, true) => CardStats::attack(1, 6),
        (CardId::Doom_and_Gloom, false) => CardStats {
            cost: 2,
            damage: 10,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Doom_and_Gloom, true) => CardStats {
            cost: 2,
            damage: 14,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Electrodynamics, false) => CardStats::skill(2, -1, 2),
        (CardId::Electrodynamics, true) => CardStats::skill(2, -1, 3),
        (CardId::Capacitor, false) => CardStats::skill(1, -1, 2),
        (CardId::Capacitor, true) => CardStats::skill(1, -1, 3),
        (CardId::Heatsinks, false) => CardStats::skill(1, -1, 1),
        (CardId::Heatsinks, true) => CardStats::skill(1, -1, 2),
        (CardId::Loop, false) => CardStats::skill(1, -1, 1),
        (CardId::Loop, true) => CardStats::skill(1, -1, 2),
        (CardId::Skim, false) => CardStats::skill(1, -1, 3),
        (CardId::Skim, true) => CardStats::skill(1, -1, 4),
        (CardId::Rip_and_Tear, false) => CardStats {
            cost: 1, damage: 7, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Rip_and_Tear, true) => CardStats {
            cost: 1, damage: 9, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Self_Repair, false) => CardStats::skill(1, -1, 7),
        (CardId::Self_Repair, true) => CardStats::skill(1, -1, 10),
        (CardId::Carnage, u) => CardStats {
            cost: 2,
            damage: if u { 28 } else { 20 },
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: true,
            innate: false,
        },
        (CardId::Combust, false) => CardStats::skill(1, -1, 5),
        (CardId::Combust, true) => CardStats::skill(1, -1, 7),
        (CardId::Dark_Embrace, false) => CardStats::skill(2, -1, -1),
        (CardId::Dark_Embrace, true) => CardStats::skill(1, -1, -1),
        (CardId::Disarm, u) => CardStats {
            cost: 1,
            damage: -1,
            block: -1,
            magic: if u { 3 } else { 2 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Dropkick, false) => CardStats::attack(1, 5),
        (CardId::Dropkick, true) => CardStats::attack(1, 8),
        (CardId::Dual_Wield, false) => CardStats::skill(1, -1, 1),
        (CardId::Dual_Wield, true) => CardStats::skill(1, -1, 2),
        (CardId::Entrench, false) => CardStats::skill(2, -1, -1),
        (CardId::Entrench, true) => CardStats::skill(1, -1, -1),
        (CardId::Evolve, false) => CardStats::skill(1, -1, 1),
        (CardId::Evolve, true) => CardStats::skill(1, -1, 2),
        (CardId::Feel_No_Pain, false) => CardStats::skill(1, -1, 3),
        (CardId::Feel_No_Pain, true) => CardStats::skill(1, -1, 4),
        (CardId::Fire_Breathing, false) => CardStats::skill(1, -1, 6),
        (CardId::Fire_Breathing, true) => CardStats::skill(1, -1, 10),
        (CardId::Flame_Barrier, false) => CardStats::skill(2, 12, 4),
        (CardId::Flame_Barrier, true) => CardStats::skill(2, 16, 6),
        (CardId::Ghostly_Armor, u) => CardStats {
            cost: 1,
            damage: -1,
            block: if u { 13 } else { 10 },
            magic: -1,
            exhaust: false,
            ethereal: true,
            innate: false,
        },
        (CardId::Hemokinesis, false) => CardStats {
            cost: 1,
            damage: 15,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Hemokinesis, true) => CardStats {
            cost: 1,
            damage: 20,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Inflame, false) => CardStats::skill(1, -1, 2),
        (CardId::Inflame, true) => CardStats::skill(1, -1, 3),
        (CardId::Intimidate, u) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: if u { 2 } else { 1 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Metallicize, false) => CardStats::skill(1, -1, 3),
        (CardId::Metallicize, true) => CardStats::skill(1, -1, 4),
        (CardId::Power_Through, false) => CardStats::skill(1, 15, 2),
        (CardId::Power_Through, true) => CardStats::skill(1, 20, 2),
        (CardId::Pummel, u) => CardStats {
            cost: 1,
            damage: 2,
            block: -1,
            magic: if u { 5 } else { 4 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Rage, false) => CardStats::skill(0, -1, 3),
        (CardId::Rage, true) => CardStats::skill(0, -1, 5),
        (CardId::Rampage, false) => CardStats {
            cost: 1,
            damage: 8,
            block: -1,
            magic: 5,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Rampage, true) => CardStats {
            cost: 1,
            damage: 8,
            block: -1,
            magic: 8,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Reckless_Charge, false) => CardStats::attack(0, 7),
        (CardId::Reckless_Charge, true) => CardStats::attack(0, 10),
        (CardId::Rupture, false) => CardStats::skill(1, -1, 1),
        (CardId::Rupture, true) => CardStats::skill(1, -1, 2),
        (CardId::Searing_Blow, u) => CardStats::attack(2, 12 + 4 * (u as i16)),
        (CardId::Second_Wind, false) => CardStats::skill(1, 5, -1),
        (CardId::Second_Wind, true) => CardStats::skill(1, 7, -1),
        (CardId::Seeing_Red, u) => CardStats {
            cost: if u { 0 } else { 1 },
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Sentinel, false) => CardStats::skill(1, 5, 2),
        (CardId::Sentinel, true) => CardStats::skill(1, 8, 3),
        (CardId::Sever_Soul, false) => CardStats::attack(2, 16),
        (CardId::Sever_Soul, true) => CardStats::attack(2, 20),
        (CardId::Shockwave, u) => CardStats {
            cost: 2,
            damage: -1,
            block: -1,
            magic: if u { 5 } else { 3 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Spot_Weakness, false) => CardStats::skill(1, -1, 3),
        (CardId::Spot_Weakness, true) => CardStats::skill(1, -1, 4),
        (CardId::Uppercut, false) => CardStats {
            cost: 2,
            damage: 13,
            block: -1,
            magic: 1,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Uppercut, true) => CardStats {
            cost: 2,
            damage: 13,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Whirlwind, false) => CardStats::attack(-1, 5),
        (CardId::Whirlwind, true) => CardStats::attack(-1, 8),
        (CardId::Barricade, false) => CardStats::skill(3, -1, -1),
        (CardId::Barricade, true) => CardStats::skill(2, -1, -1),
        (CardId::Berserk, false) => CardStats::skill(0, -1, 2),
        (CardId::Berserk, true) => CardStats::skill(0, -1, 1),
        (CardId::Bludgeon, false) => CardStats::attack(3, 32),
        (CardId::Bludgeon, true) => CardStats::attack(3, 42),
        (CardId::Brutality, u) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: false,
            innate: u,
        },
        (CardId::Corruption, false) => CardStats::skill(3, -1, -1),
        (CardId::Corruption, true) => CardStats::skill(2, -1, -1),
        (CardId::Demon_Form, false) => CardStats::skill(3, -1, 2),
        (CardId::Demon_Form, true) => CardStats::skill(3, -1, 3),
        (CardId::Double_Tap, false) => CardStats::skill(1, -1, 1),
        (CardId::Double_Tap, true) => CardStats::skill(1, -1, 2),
        (CardId::Exhume, u) => CardStats {
            cost: if u { 0 } else { 1 },
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Feed, u) => CardStats {
            cost: 1,
            damage: if u { 12 } else { 10 },
            block: -1,
            magic: if u { 4 } else { 3 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Fiend_Fire, u) => CardStats {
            cost: 2,
            damage: if u { 10 } else { 7 },
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Immolate, false) => CardStats::attack(2, 21),
        (CardId::Immolate, true) => CardStats::attack(2, 28),
        (CardId::Impervious, u) => CardStats {
            cost: 2,
            damage: -1,
            block: if u { 40 } else { 30 },
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Infernal_Blade, u) => CardStats {
            cost: if u { 0 } else { 1 },
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Juggernaut, false) => CardStats::skill(2, -1, 5),
        (CardId::Juggernaut, true) => CardStats::skill(2, -1, 7),
        (CardId::Limit_Break, u) => CardStats {
            cost: 1,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: !u,
            ethereal: false,
            innate: false,
        },
        (CardId::Offering, u) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: if u { 5 } else { 3 },
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Reaper, u) => CardStats {
            cost: 2,
            damage: if u { 5 } else { 4 },
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Slimed, _) => CardStats {
            cost: 1,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::AscendersBane, _) => CardStats {
            cost: -2,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: true,
            innate: false,
        },
        (CardId::Blind, false) | (CardId::Blind, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Trip, false) | (CardId::Trip, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Sadistic_Nature, u) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: if u { 7 } else { 5 },
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Dramatic_Entrance, false) => CardStats {
            cost: 0,
            damage: 8,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: true,
        },
        (CardId::Madness, false) => CardStats {
            cost: 1,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Madness, true) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Thinking_Ahead, false) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: 2,
            exhaust: true,
            ethereal: false,
            innate: false,
        },
        (CardId::Thinking_Ahead, true) => CardStats {
            cost: 0,
            damage: -1,
            block: -1,
            magic: 2,
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Dramatic_Entrance, true) => CardStats {
            cost: 0,
            damage: 12,
            block: -1,
            magic: -1,
            exhaust: true,
            ethereal: false,
            innate: true,
        },
        (CardId::Dazed, _) => CardStats {
            cost: -2,
            damage: -1,
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: true,
            innate: false,
        },
        (CardId::Burn, u) => CardStats {
            cost: -2,
            damage: -1,
            block: -1,
            magic: if u { 4 } else { 2 },
            exhaust: false,
            ethereal: false,
            innate: false,
        },
        (CardId::Consume, false) => CardStats::skill(2, -1, 2),
        (CardId::Consume, true) => CardStats::skill(2, -1, 3),
        (CardId::Darkness, false) => CardStats::skill(1, -1, 1),
        (CardId::Darkness, true) => CardStats::skill(1, -1, 1),
        (CardId::Rainbow, false) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Rainbow, true) => CardStats {
            // Java Rainbow.upgrade: exhaust = false, cost unchanged.
            cost: 2, damage: -1, block: -1, magic: 3, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Fission, false) | (CardId::Fission, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Multi_Cast, false) | (CardId::Multi_Cast, true) => CardStats {
            cost: -1, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Finesse, false) => CardStats::skill(0, 2, -1),
        (CardId::Finesse, true) => CardStats::skill(0, 4, -1),
        (CardId::Reboot, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 4, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Reboot, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 6, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Creative_AI, false) => CardStats {
            cost: 3, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Creative_AI, true) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Scrape, false) => CardStats {
            cost: 1, damage: 7, block: -1, magic: 4, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Scrape, true) => CardStats {
            cost: 1, damage: 10, block: -1, magic: 5, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Machine_Learning, false) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Machine_Learning, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::All_For_One, false) => CardStats::attack(2, 10),
        (CardId::All_For_One, true) => CardStats::attack(2, 14),
        (CardId::Fusion, false) => CardStats::skill(2, -1, 1),
        (CardId::Fusion, true) => CardStats::skill(1, -1, 1),
        (CardId::Seek, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Seek, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Impatience, false) => CardStats::skill(0, -1, 2),
        (CardId::Impatience, true) => CardStats::skill(0, -1, 3),
        (CardId::Flash_of_Steel, false) => CardStats::attack(0, 3),
        (CardId::Flash_of_Steel, true) => CardStats::attack(0, 6),
        (CardId::Panacea, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Panacea, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Genetic_Algorithm, false) => CardStats {
            cost: 1, damage: -1, block: 1, magic: 2, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Genetic_Algorithm, true) => CardStats {
            cost: 1, damage: -1, block: 1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Gash, false) => CardStats {
            cost: 0, damage: 3, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Gash, true) => CardStats {
            cost: 0, damage: 5, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Turbo, false) => CardStats::skill(0, -1, 2),
        (CardId::Turbo, true) => CardStats::skill(0, -1, 3),
        (CardId::Redo, false) => CardStats::skill(1, -1, -1),
        (CardId::Redo, true) => CardStats::skill(0, -1, -1),
        (CardId::Chaos, false) => CardStats::skill(1, -1, 1),
        (CardId::Chaos, true) => CardStats::skill(1, -1, 2),
        (CardId::White_Noise, false) => CardStats {
            cost: 1, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::White_Noise, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Lockon, false) => CardStats {
            cost: 1, damage: 8, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Lockon, true) => CardStats {
            cost: 1, damage: 11, block: -1, magic: 3, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Dramatic_Entrance, false) => CardStats {
            cost: 0, damage: 8, block: -1, magic: -1, exhaust: true, ethereal: false, innate: true,
        },
        (CardId::Dramatic_Entrance, true) => CardStats {
            cost: 0, damage: 12, block: -1, magic: -1, exhaust: true, ethereal: false, innate: true,
        },
        (CardId::Steam_Power, false) => CardStats::skill(0, -1, 2),
        (CardId::Steam_Power, true) => CardStats::skill(0, -1, 3),
        (CardId::Storm, false) => CardStats::skill(1, -1, 1),
        (CardId::Storm, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::Double_Energy, false) => CardStats {
            cost: 1, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Double_Energy, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Void, false) | (CardId::Void, true) => CardStats {
            cost: -2, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: true, innate: false,
        },
        (CardId::Good_Instincts, false) => CardStats::skill(0, 6, -1),
        (CardId::Good_Instincts, true) => CardStats::skill(0, 9, -1),
        (CardId::PanicButton, false) => CardStats {
            cost: 0, damage: -1, block: 30, magic: 2, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::PanicButton, true) => CardStats {
            cost: 0, damage: -1, block: 40, magic: 2, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Dark_Shackles, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 9, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Dark_Shackles, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 15, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Reprogram, false) => CardStats::skill(1, -1, 1),
        (CardId::Reprogram, true) => CardStats::skill(1, -1, 2),
        (CardId::Aggregate, false) => CardStats::skill(1, -1, 4),
        (CardId::Aggregate, true) => CardStats::skill(1, -1, 3),
        (CardId::Hello_World, false) => CardStats::skill(1, -1, 1),
        (CardId::Hello_World, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::Reinforced_Body, false) => CardStats {
            cost: -1, damage: -1, block: 7, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Reinforced_Body, true) => CardStats {
            cost: -1, damage: -1, block: 9, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Thunder_Strike, false) => CardStats::attack(3, 7),
        (CardId::Thunder_Strike, true) => CardStats::attack(3, 9),
        (CardId::Static_Discharge, false) => CardStats::skill(1, -1, 1),
        (CardId::Static_Discharge, true) => CardStats::skill(1, -1, 2),
        (CardId::Core_Surge, false) => CardStats {
            cost: 1, damage: 11, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Core_Surge, true) => CardStats {
            cost: 1, damage: 15, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Sunder, false) => CardStats::attack(3, 24),
        (CardId::Sunder, true) => CardStats::attack(3, 32),
        (CardId::HandOfGreed, false) => CardStats {
            cost: 2, damage: 20, block: -1, magic: 20, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::HandOfGreed, true) => CardStats {
            cost: 2, damage: 25, block: -1, magic: 25, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Apotheosis, false) => CardStats {
            cost: 2, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Apotheosis, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Writhe, _) => CardStats {
            cost: -2, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::Clumsy, _) => CardStats {
            cost: -2, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: true, innate: false,
        },
        (CardId::Echo_Form, false) => CardStats {
            cost: 3, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: true, innate: false,
        },
        (CardId::Echo_Form, true) => CardStats {
            cost: 3, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Ghostly, false) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: true, innate: false,
        },
        (CardId::Ghostly, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: 1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Violence, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Violence, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 4, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Deep_Breath, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Deep_Breath, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Discovery, false) => CardStats {
            cost: 1, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Discovery, true) => CardStats {
            cost: 1, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Forethought, false) | (CardId::Forethought, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::J_A_X_, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 2, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::J_A_X_, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 3, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Purity, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Purity, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 5, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Mind_Blast, false) => CardStats {
            cost: 2, damage: 0, block: -1, magic: -1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::Mind_Blast, true) => CardStats {
            cost: 1, damage: 0, block: -1, magic: -1, exhaust: false, ethereal: false, innate: true,
        },
        (CardId::Secret_Technique, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Secret_Technique, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Secret_Weapon, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Secret_Weapon, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: -1, exhaust: false, ethereal: false, innate: false,
        },
        (CardId::Chrysalis, false) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Chrysalis, true) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 5, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Metamorphosis, false) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Metamorphosis, true) => CardStats {
            cost: 2, damage: -1, block: -1, magic: 5, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Mayhem, false) => CardStats::skill(2, -1, 1),
        (CardId::Mayhem, true) => CardStats::skill(1, -1, 1),
        (CardId::Master_of_Strategy, false) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 3, exhaust: true, ethereal: false, innate: false,
        },
        (CardId::Master_of_Strategy, true) => CardStats {
            cost: 0, damage: -1, block: -1, magic: 4, exhaust: true, ethereal: false, innate: false,
        },
        _ => {
            let def = id.def();
            CardStats {
                cost: def.cost,
                damage: -1,
                block: -1,
                magic: -1,
                exhaust: false,
                ethereal: false,
                innate: false,
            }
        }
    }
}

pub fn encounter_monsters(id: EncounterId) -> Vec<MonsterId> {
    encounter_monsters_rng(id, None)
}

pub fn encounter_monsters_rng(id: EncounterId, mut rng: Option<&mut crate::rng::RngSet>) -> Vec<MonsterId> {
    match id {
        EncounterId::SmallSlimes => {
            if rng.as_mut().map(|r| r.misc.random_boolean()).unwrap_or(true) {
                vec![MonsterId::SpikeSlimeS, MonsterId::AcidSlimeM]
            } else {
                vec![MonsterId::AcidSlimeS, MonsterId::SpikeSlimeM]
            }
        }
        EncounterId::LargeSlime => {
            if rng.as_mut().map(|r| r.misc.random_boolean()).unwrap_or(true) {
                vec![MonsterId::AcidSlimeL]
            } else {
                vec![MonsterId::SpikeSlimeL]
            }
        }
        EncounterId::LotsOfSlimes => {
            let mut pool = vec![
                MonsterId::SpikeSlimeS,
                MonsterId::SpikeSlimeS,
                MonsterId::SpikeSlimeS,
                MonsterId::AcidSlimeS,
                MonsterId::AcidSlimeS,
            ];
            let mut out = Vec::with_capacity(5);
            if let Some(rng) = rng.as_mut() {
                while !pool.is_empty() {
                    let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
                    out.push(pool.remove(idx));
                }
            } else {
                out = pool;
            }
            out
        }
        EncounterId::TwoLouse | EncounterId::ThreeLouse => {
            let n = if id == EncounterId::ThreeLouse { 3 } else { 2 };
            let mut out = Vec::with_capacity(n);
            if let Some(rng) = rng.as_mut() {
                for _ in 0..n {
                    // MonsterHelper.getLouse: miscRng.randomBoolean() ? Normal : Defensive
                    out.push(if rng.misc.random_boolean() {
                        MonsterId::LouseNormal
                    } else {
                        MonsterId::LouseDefensive
                    });
                }
            } else {
                out.resize(n, MonsterId::LouseNormal);
            }
            out
        }
        EncounterId::ExordiumThugs => {
            // bottomHumanoid: weak wildlife then strong humanoid.
            // getLouse / getSlaver always roll even if not selected.
            if let Some(rng) = rng.as_mut() {
                let weak = bottom_weak_wildlife(rng);
                let strong = bottom_strong_humanoid(rng);
                vec![weak, strong]
            } else {
                vec![MonsterId::LouseNormal, MonsterId::Looter]
            }
        }
        EncounterId::ExordiumWildlife => {
            // bottomWildlife (2): strong wildlife then weak wildlife.
            if let Some(rng) = rng.as_mut() {
                let strong = bottom_strong_wildlife(rng);
                let weak = bottom_weak_wildlife(rng);
                vec![strong, weak]
            } else {
                vec![MonsterId::FungiBeast, MonsterId::JawWorm]
            }
        }
        EncounterId::GremlinGang => {
            // MonsterHelper.spawnGremlins: 8-card pool, draw 4 without replacement.
            let mut pool = vec![
                MonsterId::GremlinWarrior,
                MonsterId::GremlinWarrior,
                MonsterId::GremlinThief,
                MonsterId::GremlinThief,
                MonsterId::GremlinFat,
                MonsterId::GremlinFat,
                MonsterId::GremlinTsundere,
                MonsterId::GremlinWizard,
            ];
            let mut out = Vec::with_capacity(4);
            if let Some(rng) = rng.as_mut() {
                for _ in 0..4 {
                    let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
                    out.push(pool.remove(idx));
                }
            } else {
                out.extend(pool.into_iter().take(4));
            }
            out
        }
        EncounterId::GremlinLeader => {
            // MonsterHelper.spawnGremlin creates a fresh weighted pool for
            // each of the two starting minions, so duplicates are allowed.
            let pool = [
                MonsterId::GremlinWarrior,
                MonsterId::GremlinWarrior,
                MonsterId::GremlinThief,
                MonsterId::GremlinThief,
                MonsterId::GremlinFat,
                MonsterId::GremlinFat,
                MonsterId::GremlinTsundere,
                MonsterId::GremlinWizard,
            ];
            if let Some(rng) = rng.as_mut() {
                vec![
                    pool[rng.misc.random_int(7) as usize],
                    pool[rng.misc.random_int(7) as usize],
                    MonsterId::GremlinLeader,
                ]
            } else {
                vec![
                    MonsterId::GremlinWarrior,
                    MonsterId::GremlinWarrior,
                    MonsterId::GremlinLeader,
                ]
            }
        }
        EncounterId::ThreeShapes => spawn_shapes(rng, true),
        EncounterId::FourShapes => spawn_shapes(rng, false),
        other => encounter_monsters_fixed(other).to_vec(),
    }
}

fn bottom_weak_wildlife(rng: &mut crate::rng::RngSet) -> MonsterId {
    let louse = if rng.misc.random_boolean() {
        MonsterId::LouseNormal
    } else {
        MonsterId::LouseDefensive
    };
    let pool = [louse, MonsterId::SpikeSlimeM, MonsterId::AcidSlimeM];
    pool[rng.misc.random_int(pool.len() as i32 - 1) as usize]
}

fn bottom_strong_humanoid(rng: &mut crate::rng::RngSet) -> MonsterId {
    let slaver = if rng.misc.random_boolean() {
        MonsterId::SlaverRed
    } else {
        MonsterId::SlaverBlue
    };
    let pool = [MonsterId::Cultist, slaver, MonsterId::Looter];
    pool[rng.misc.random_int(pool.len() as i32 - 1) as usize]
}

fn bottom_strong_wildlife(rng: &mut crate::rng::RngSet) -> MonsterId {
    let pool = [MonsterId::FungiBeast, MonsterId::JawWorm];
    pool[rng.misc.random_int(pool.len() as i32 - 1) as usize]
}

/// MonsterHelper.spawnShapes: 2 of each shape, pick 3 (weak) or 4 via miscRng.
fn spawn_shapes(rng: Option<&mut crate::rng::RngSet>, weak: bool) -> Vec<MonsterId> {
    let mut pool = vec![
        MonsterId::Repulsor,
        MonsterId::Repulsor,
        MonsterId::Exploder,
        MonsterId::Exploder,
        MonsterId::Spiker,
        MonsterId::Spiker,
    ];
    let n = if weak { 3 } else { 4 };
    let mut out = Vec::with_capacity(n);
    if let Some(rng) = rng {
        for _ in 0..n {
            let idx = rng.misc.random_int(pool.len() as i32 - 1) as usize;
            out.push(pool.remove(idx));
        }
    } else {
        out.extend(pool.into_iter().take(n));
    }
    out
}

fn encounter_monsters_fixed(id: EncounterId) -> &'static [MonsterId] {
    match id {
        EncounterId::Cultist => &[MonsterId::Cultist],
        EncounterId::JawWorm => &[MonsterId::JawWorm],
        EncounterId::TwoLouse => &[MonsterId::LouseNormal, MonsterId::LouseNormal],
        EncounterId::SmallSlimes => &[MonsterId::AcidSlimeM, MonsterId::SpikeSlimeS],
        EncounterId::BlueSlaver => &[MonsterId::SlaverBlue],
        EncounterId::Looter => &[MonsterId::Looter],
        EncounterId::LargeSlime => &[MonsterId::AcidSlimeL],
        EncounterId::LotsOfSlimes => &[
            MonsterId::SpikeSlimeS,
            MonsterId::SpikeSlimeS,
            MonsterId::SpikeSlimeS,
            MonsterId::AcidSlimeS,
            MonsterId::AcidSlimeS,
        ],
        EncounterId::RedSlaver => &[MonsterId::SlaverRed],
        EncounterId::ThreeLouse => &[MonsterId::LouseNormal, MonsterId::LouseDefensive, MonsterId::LouseNormal],
        EncounterId::TwoFungiBeasts => &[MonsterId::FungiBeast, MonsterId::FungiBeast],
        EncounterId::MushroomLair => &[MonsterId::FungiBeast, MonsterId::FungiBeast, MonsterId::FungiBeast],
        EncounterId::GremlinNob => &[MonsterId::GremlinNob],
        EncounterId::Lagavulin => &[MonsterId::Lagavulin],
        EncounterId::ThreeSentries => &[MonsterId::Sentry, MonsterId::Sentry, MonsterId::Sentry],
        EncounterId::Hexaghost => &[MonsterId::Hexaghost],
        EncounterId::TheGuardian => &[MonsterId::TheGuardian],
        EncounterId::SlimeBoss => &[MonsterId::SlimeBoss],
        EncounterId::GremlinGang => &[
            MonsterId::GremlinFat,
            MonsterId::GremlinTsundere,
            MonsterId::GremlinThief,
            MonsterId::GremlinWarrior,
        ],
        EncounterId::ExordiumThugs => &[MonsterId::Looter, MonsterId::SlaverBlue],
        EncounterId::ExordiumWildlife => &[MonsterId::FungiBeast, MonsterId::JawWorm],
        EncounterId::ShieldAndSpear => &[MonsterId::SpireShield, MonsterId::SpireSpear],
        EncounterId::CorruptHeart => &[MonsterId::CorruptHeart],
        EncounterId::SphericGuardian => &[MonsterId::SphericGuardian],
        EncounterId::Chosen => &[MonsterId::Chosen],
        EncounterId::CenturionAndHealer => &[MonsterId::Centurion, MonsterId::Healer],
        EncounterId::SnakePlant => &[MonsterId::SnakePlant],
        EncounterId::ShelledParasiteAndFungi => &[MonsterId::ShelledParasite, MonsterId::FungiBeast],
        EncounterId::Automaton => &[MonsterId::BronzeAutomaton],
        EncounterId::BookOfStabbing => &[MonsterId::BookOfStabbing],
        EncounterId::Slavers => &[
            MonsterId::SlaverBlue,
            MonsterId::Taskmaster,
            MonsterId::SlaverRed,
        ],
        EncounterId::GremlinLeader => &[
            MonsterId::GremlinWarrior,
            MonsterId::GremlinWarrior,
            MonsterId::GremlinLeader,
        ],
        EncounterId::MaskedBandits => &[
            MonsterId::BanditChild,
            MonsterId::BanditLeader,
            MonsterId::BanditBear,
        ],
        EncounterId::ThreeDarklings => &[MonsterId::Darkling, MonsterId::Darkling, MonsterId::Darkling],
        EncounterId::Transient => &[MonsterId::Transient],
        EncounterId::GiantHead => &[MonsterId::GiantHead],
        EncounterId::JawWormHorde => &[MonsterId::JawWorm, MonsterId::JawWorm, MonsterId::JawWorm],
        EncounterId::AwakenedOne => &[MonsterId::Cultist, MonsterId::Cultist, MonsterId::AwakenedOne],
        EncounterId::ThreeShapes => &[MonsterId::Spiker, MonsterId::Exploder, MonsterId::Exploder],
        EncounterId::FourShapes => &[
            MonsterId::Spiker,
            MonsterId::Exploder,
            MonsterId::Exploder,
            MonsterId::Repulsor,
        ],
        EncounterId::TwoThieves => &[MonsterId::Looter, MonsterId::Mugger],
        EncounterId::ShellParasite => &[MonsterId::ShelledParasite],
        EncounterId::SentryAndSphere => &[MonsterId::Sentry, MonsterId::SphericGuardian],
        EncounterId::CultistAndChosen => &[MonsterId::Cultist, MonsterId::Chosen],
        EncounterId::ThreeCultists => &[MonsterId::Cultist, MonsterId::Cultist, MonsterId::Cultist],
        EncounterId::Snecko => &[MonsterId::Snecko],
        EncounterId::ThreeByrds => &[MonsterId::Byrd, MonsterId::Byrd, MonsterId::Byrd],
        EncounterId::ChosenAndByrds => &[MonsterId::Byrd, MonsterId::Chosen],
        EncounterId::Champ => &[MonsterId::Champ],
        EncounterId::OrbWalker => &[MonsterId::OrbWalker],
        EncounterId::Maw => &[MonsterId::Maw],
        EncounterId::Collector => &[MonsterId::TheCollector],
    }
}
