//! Defect A0 GREEN registry: walk seeds listed in the JSONL file.
//!
//! The file `exact-text-sim/runtime/oracles/defect/a0/green_registry.jsonl`
//! is the source of truth. Do not keep a Rust array of seeds.

use sts_engine::green_registry::{GreenRegistry, GreenStatus};
use sts_engine::ids::Character;
use sts_engine::walk::{default_config, walk_oracle};
use sts_engine::Unlocks;
use std::path::PathBuf;

fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a0/green_registry.jsonl")
}

fn walk_a0(seed: &str) -> Result<sts_engine::walk::WalkOk, sts_engine::walk::WalkFail> {
    let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 0);
    walk_oracle(&cfg)
}

#[test]
fn registry_file_is_loadable() {
    let path = registry_path();
    if !path.exists() {
        eprintln!("skip: a0 green_registry.jsonl not written yet");
        return;
    }
    let reg = GreenRegistry::load(&path).expect("load a0 green_registry.jsonl");
    assert_eq!(reg.ascension, 0, "a0 registry meta.ascension");
}

#[test]
fn harvested_a0_oracles_walk_or_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a0");
    let mut seen = 0usize;
    let mut green = 0usize;
    for entry in std::fs::read_dir(&root).expect("a0 oracle dir") {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let seed = entry.file_name();
        let seed = seed.to_string_lossy();
        if seed.contains(' ') || !(entry.path().join("states.jsonl")).exists() {
            continue;
        }
        seen += 1;
        match walk_a0(&seed) {
            Ok(ok) => {
                assert!(ok.snaps > 0, "{seed} empty GREEN walk");
                green += 1;
            }
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => {
                eprintln!(
                    "{seed} RED last_ok {} seq {} {}",
                    fail.last_ok, fail.seq, fail.boundary
                );
            }
        }
    }
    assert!(seen > 0, "no harvested a0 oracles under {}", root.display());
    eprintln!("a0 harvested {green} GREEN / {seen} oracles");
}

#[test]
fn registry_greens_still_walk() {
    let path = registry_path();
    if !path.exists() {
        eprintln!("skip: a0 green_registry.jsonl not written yet");
        return;
    }
    let reg = GreenRegistry::load(&path).expect("load a0 registry");
    let seeds: Vec<String> = reg.green_seeds().into_iter().map(str::to_string).collect();
    for seed in &seeds {
        match walk_a0(seed) {
            Ok(ok) => {
                assert!(ok.snaps > 0, "{seed} empty walk");
            }
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => panic!("a0 registry seed {seed} is not GREEN:\n{fail}"),
        }
    }
}

#[test]
fn artifact_absorbs_lagavulin_siphon_dexterity() {
    // 213: Ancient Potion Artifact 1 eats Dexterity -1; Strength -1 still lands.
    // Defend then stays 5 block (rust had Dexterity -1 → 4, 9 vs Java 10).
    let cfg = default_config(Character::Defect, "213", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 67,
                "213 still fails at Artifact/siphon last_ok={} want > 67: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn whetstone_upgrades_attacks_at_obtain() {
    // Seed 1: Whetstone onEquip must use miscRng at instantObtain so Rip and
    // Tear is + (9x2) vs Slime Boss, not unupgraded 7x2 (mons 123 vs 119).
    let cfg = default_config(Character::Defect, "1", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 205,
                "1 still fails at lethal thorns last_ok={} want > 205: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn slime_boss_split_inserts_by_draw_x() {
    // Seed 8: slime split, Cursed Key, Champ, then Shame EOT Frail on Defend.
    let cfg = default_config(Character::Defect, "8", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 689,
                "8 still fails at Act 3 event last_ok={} want > 689: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn acid_slime_l_split_survives_roll_move() {
    // Seed 776: Bronze Scales thorns during AcidSlime_L tackle crosses 50% HP.
    // Java queues SetMoveAction(SPLIT) after RollMoveAction so getMove cannot
    // overwrite the split (player 29 vs 13, two AcidSlime_M kids).
    let cfg = default_config(Character::Defect, "776", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 73,
                "776 still fails at AcidSlime_L split last_ok={} want > 73: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn clumsy_is_ethereal_at_end_of_turn() {
    // Seed 944: Clumsy in hand at EOT must ExhaustSpecificCardAction, not
    // discard-and-reshuffle (hand Compile Driver vs Strike_B at seq 56).
    let cfg = default_config(Character::Defect, "944", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 54,
                "944 still fails at Clumsy ethereal last_ok={} want > 54: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn pantograph_heals_at_boss() {
    // Seed 5: Pantograph boss heal, then Byrd FlightPower.atStartOfTurn
    // restores storedAmount so Sweeping Beam still halves (10→7 not 10→4).
    let cfg = default_config(Character::Defect, "5", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 257,
                "5 still fails at Byrd Flight restore last_ok={} want > 257: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn runic_pyramid_keeps_hand_at_end_of_turn() {
    // 213: Runic Pyramid skips DiscardAtEndOfTurnAction. Rust discarded
    // Hologram/Consume/Defend_B; Java kept them (hand 8 vs 5 after the draw).
    let cfg = default_config(Character::Defect, "213", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 429,
                "213 still fails after Distilled Chaos Dazed autoplay last_ok={} want > 429: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn hex_does_not_insert_dazed_on_cold_snap() {
    // 213 seq 429: Cold Snap is an Attack; HexPower.onUseCard skips ATTACK.
    // Extra Dazed in hand meant rust treated the play as a non-attack.
    use sts_engine::card::Card;
    use sts_engine::combat::{play_card, Combat};
    use sts_engine::creature::{Orb, OrbKind, Player, RelicInstance};
    use sts_engine::ids::{CardId, EncounterId, PowerId, RelicId};
    use sts_engine::rng::RngSet;

    let mut rng = RngSet::generate_seeds(213);
    let mut player = Player::defect();
    player.energy = 3;
    player.max_orbs = 2;
    player.orbs = vec![
        Orb { kind: OrbKind::Frost, evoke: 9 },
        Orb { kind: OrbKind::Lightning, evoke: 12 },
    ];
    player.add_power(PowerId::Hex, 1);
    player.add_power(PowerId::Focus, 4);
    player.relics.push(RelicInstance {
        id: RelicId::InkBottle,
        counter: 8,
        used_up: false,
    });
    let mut combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 29, 213, 0);
    player.orbs = vec![
        Orb { kind: OrbKind::Frost, evoke: 9 },
        Orb { kind: OrbKind::Lightning, evoke: 12 },
    ];
    player.max_orbs = 2;
    player.hand = vec![
        Card::new(CardId::Skim),
        Card::new(CardId::Capacitor),
        Card::new(CardId::Buffer),
        Card::new(CardId::Capacitor),
        Card::new(CardId::Dazed),
        Card::new(CardId::Dazed),
        Card::new(CardId::Dazed),
        Card::new(CardId::Cold_Snap),
    ];
    player.draw.clear();
    player.discard.clear();
    play_card(&mut player, &mut combat, 7, Some(0), &mut rng, None);
    let dazed_hand = player.hand.iter().filter(|c| c.id == CardId::Dazed).count();
    let dazed_draw = player.draw.iter().filter(|c| c.id == CardId::Dazed).count();
    assert_eq!(
        dazed_hand, 3,
        "Cold Snap+Hex must not add Dazed; hand_dazed={dazed_hand} draw_dazed={dazed_draw} hand={:?}",
        player.hand.iter().map(|c| c.sts_id()).collect::<Vec<_>>()
    );
    assert_eq!(dazed_draw, 0, "Hex must not insert Dazed on an Attack");
}

#[test]
fn autoplay_dazed_skips_hex_and_ink_bottle() {
    // PlayTopCard of Dazed: canUse is false, UseCardAction.dontTriggerOnUseCard.
    // Rust used to Hex-insert and tick InkBottle, leaving an extra Dazed for
    // seed 213 Cold Snap to draw.
    use sts_engine::card::Card;
    use sts_engine::combat::{play_owned_card, Combat};
    use sts_engine::creature::{Player, RelicInstance};
    use sts_engine::ids::{CardId, EncounterId, PowerId, RelicId};
    use sts_engine::rng::RngSet;

    let mut rng = RngSet::generate_seeds(213);
    let mut player = Player::defect();
    player.add_power(PowerId::Hex, 1);
    player.relics.push(RelicInstance {
        id: RelicId::InkBottle,
        counter: 9,
        used_up: false,
    });
    let mut combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 1, 213, 0);
    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Dazed),
        None,
        &mut rng,
        None,
    );
    let ink = player
        .relics
        .iter()
        .find(|r| r.id == RelicId::InkBottle)
        .map(|r| r.counter)
        .unwrap_or(-1);
    assert_eq!(ink, 9, "unplayable Dazed must not tick InkBottle");
    assert_eq!(
        player.draw.iter().filter(|c| c.id == CardId::Dazed).count(),
        0,
        "Hex must not fire on unplayable Dazed autoplay"
    );
    assert_eq!(
        player.discard.iter().filter(|c| c.id == CardId::Dazed).count(),
        1,
        "Dazed autoplay still discards via UseCardAction"
    );
    assert!(
        !player.discard.iter().any(|c| c.id == CardId::Dazed && c.free_to_play_once),
        "UseCardAction clears freeToPlayOnce so All For One does not retrieve Dazed"
    );
}

#[test]
fn storm_channels_after_the_power_applies() {
    // 169: Defragment +1 Focus then Storm channels Lightning, evoking Frost at 6 not 5.
    let cfg = default_config(Character::Defect, "169", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 111,
                "169 still fails at Storm+Focus last_ok={} want > 111: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn act1_boss_still_rolls_potion_chance() {
    // 169: Hexaghost addPotionToRewards uses 40+blizzard (MonsterRoomBoss
    // instanceof MonsterRoom). Rust forced chance 0, skipped SteroidPotion,
    // then Choose 0 claimed CARD instead of the potion.
    let cfg = default_config(Character::Defect, "169", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 335,
                "169 still fails at Hex Dazed timing last_ok={} want > 335: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn colorless_potion_discovery_matches_src_pool() {
    // 28: ColorlessPotion DiscoveryAction reads srcColorlessCardPool. Rust shuffled
    // colorlessCardPool in place for returnColorlessCard, then reversed that.
    let cfg = default_config(Character::Defect, "28", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 101,
                "28 still fails at Colorless discovery last_ok={} want > 101: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_shuffles_without_in_flight_cards() {
    // 38: Dualcast is still in limbo when the next PlayTopCard shuffles.
    let cfg = default_config(Character::Defect, "38", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 92,
                "38 still fails at Distilled Chaos last_ok={} want > 92: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn note_for_yourself_leave_stays_in_event() {
    // 49: NoteForYourself Leave must not consume the COMPLETE click as a map node.
    let cfg = default_config(Character::Defect, "49", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 75,
                "49 still fails at NoteForYourself last_ok={} want > 75: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn monster_weak_vulnerable_floors_once() {
    // 34: SlaverRed Weak * player Vulnerable must chain then floor (14, not 13).
    let cfg = default_config(Character::Defect, "34", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 66,
                "34 still fails at Weak+Vulnerable last_ok={} want > 66: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn golden_shrine_pray_is_100_gold_on_a0() {
    // 18: GoldShrine pray is 100 below A15 (rust had hardcoded 50).
    let cfg = default_config(Character::Defect, "18", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 21,
                "18 still fails at Golden Shrine last_ok={} want > 21: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn bird_faced_urn_heals_on_power() {
    // 12: Dualcast/Defragment/Electrodynamics with Bird Faced Urn; rust skipped the +2 heal.
    let cfg = default_config(Character::Defect, "12", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 109,
                "12 still fails at Bird Faced Urn last_ok={} want > 109: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn shuriken_gives_strength_every_third_attack() {
    // 20: Shuriken leftover Strength 1, Melter hits SlaverBlue 11 not 12.
    let cfg = default_config(Character::Defect, "20", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 107,
                "20 still fails at Shuriken last_ok={} want > 107: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn potion_belt_adds_two_slots() {
    // 41: Potion Belt onEquip +2 slots; Fruit Juice in slot 3 is +5 HP at the shop.
    let cfg = default_config(Character::Defect, "41", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 138,
                "41 still fails at Potion Belt last_ok={} want > 138: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn lagavulin_siphon_keeps_negative_strength() {
    // 713578: Lagavulin move 1 applies Strength -1; rust used to drop amount<=0 at EOT.
    let cfg = default_config(Character::Defect, "713578", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 117,
                "713578 still fails at Lagavulin siphon last_ok={} want > 117: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn toy_ornithopter_heals_after_discovery_overlay() {
    // 45: Colorless/typed potion DiscoveryAction is on the queue before
    // ToyOrnithopter HealAction, so the CARD_REWARD snapshot is still 56 HP.
    let cfg = default_config(Character::Defect, "45", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 127,
                "45 still fails at Ornithopter discovery heal last_ok={} want > 127: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn gremlin_horn_draws_when_eot_orbs_kill() {
    // 906: Electro lightning kills AcidSlime_M at EOT; Gremlin Horn
    // DrawCardAction resolves before DiscardAtEndOfTurn. SlimeBoss slam
    // must not overwrite a thorns-triggered SPLIT (last_ok 97 → split).
    let cfg = default_config(Character::Defect, "906", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 139,
                "906 still fails at Horn/SlimeBoss split last_ok={} want > 139: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn odd_mushroom_cuts_vulnerable_to_25_percent() {
    // 89/958: FungiBeast + Odd Mushroom. Vulnerable is 1.25× not 1.5×
    // (hp 40 vs rust 38 / 56 vs 54 after EndTurn).
    let cfg = default_config(Character::Defect, "89", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 128,
                "89 still fails at Odd Mushroom last_ok={} want > 128: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn incense_burner_applies_intangible_every_six_turns() {
    // 493: Incense Burner counter ticks atTurnStart; at 6 apply Intangible
    // so FungiBeast hits for 1 not 9 (hp 24 vs rust 16).
    let cfg = default_config(Character::Defect, "493", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 112,
                "493 still fails at Incense Burner last_ok={} want > 112: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn regression_is_recorded_not_deleted() {
    let mut reg = GreenRegistry::new();
    reg.record_green("407258", 250, 242, 210390258);
    assert_eq!(reg.green_count(), 1);
    assert!(reg.seeds.contains_key("407258"));

    reg.record_regression("407258", 12, "first mismatch at seq 13 hp");
    assert!(
        reg.seeds.contains_key("407258"),
        "regression must not drop the seed"
    );
    assert_eq!(reg.seeds["407258"].status, GreenStatus::Regression);
    assert_eq!(reg.green_count(), 0);
    assert_eq!(reg.regressions.len(), 1);
    assert_eq!(reg.regressions[0].seed, "407258");
}
