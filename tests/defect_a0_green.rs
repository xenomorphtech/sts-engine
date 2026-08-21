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
        if seed.contains(' ')
            || !(entry.path().join("states.jsonl").exists()
                || entry.path().join("states.jsonl.gz").exists())
        {
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
fn masked_bandits_enters_the_three_bandit_combat() {
    // Seed 340 chooses Fight at Masked Bandits. The event must seed gold +
    // Red Mask rewards and enter the Pointy/Leader/Bear combat immediately.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 179,
                "340 still fails before Masked Bandits combat last_ok={} want > 179: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sadistic_nature_damages_after_player_debuffs() {
    // Seed 279 plays Sadistic Nature, then Go for the Eyes against Guardian.
    // Weak must apply first and queue Sadistic's 5 THORNS damage (43 -> 38).
    let cfg = default_config(Character::Defect, "279", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 185,
                "279 still fails at Sadistic Nature last_ok={} want > 185: {fail}",
                fail.last_ok
            );
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
fn gfte_weak_snapshots_intent_before_mode_shift() {
    // 54: Blind.use applies Weak 2 (Java WeakPower). Without it Guardian
    // ROLL_ATTACK 9 vs 8 frost is 1 HP; Weak 9*0.75=6 is fully blocked.
    // ForTheEyesAction snapshots getIntentBaseDmg before Mode Shift.
    let cfg = default_config(Character::Defect, "54", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 152,
                "54 still fails at GftE Weak last_ok={} want > 152: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn speed_potion_is_combat_only() {
    // 357: SpeedPotion is COMBAT-only (canUse). LoseDexterityPower.atEndOfTurn
    // is ApplyPowerAction(Dexterity, -amount), so Ancient Potion Artifact 1
    // eats the -5 and Glacier+ stays 15 (10 + Dex 5).
    let cfg = default_config(Character::Defect, "357", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "357 still fails at Speed Potion last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn eot_orbs_killing_skip_burn_autoplay() {
    // 968: EOT lightning kills Hexaghost; cleanCardQueue drops queued Burn
    // autoplays. Playing them anyway Fairy-revives (22 vs 8).
    let cfg = default_config(Character::Defect, "968", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 164,
                "968 still fails at EOT Burn after Hexaghost death last_ok={} want > 164: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn jax_loses_hp_and_gains_strength() {
    // 32: JAX.use LoseHP 3 then Strength 2 (seq 207 65 vs 62, no Strength).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 207,
                "32 still fails at JAX last_ok={} want > 207: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn nuclear_battery_channels_plasma_at_prebattle() {
    // 32: NuclearBattery.atPreBattle channels Plasma after Cracked Core.
    // Without it turn-1 Cold Snap does not evoke Lightning (Spheric 15 vs 18).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 187,
                "32 still fails at Nuclear Battery Plasma last_ok={} want > 187: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_twin_slam_then_mode_shift() {
    // 32: Twin Slam queues ChangeState Offensive then 8x2. ApplyPower(Mode
    // Shift) / Reset Threshold are addToBottom of ChangeState, so player
    // Thorns 3x2 do not stick on the new Mode Shift. Rust applied Mode Shift
    // first (24 leftover, Sweeping Beam hit 20 block: 97 vs 91).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 161,
                "32 still fails at Guardian Twin Slam/Mode Shift last_ok={} want > 161: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn tiny_house_opens_combat_reward() {
    // 906: TinyHouse.onEquip increaseMaxHp(5) and CombatRewardScreen.open
    // (gold 50, miscRng potion, CARD). Rust stayed on BossRelic at 36 HP.
    let cfg = default_config(Character::Defect, "906", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 160,
                "906 still fails at Tiny House last_ok={} want > 160: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn explosive_potion_triggers_gremlin_horn() {
    // 773: ExplosivePotion DamageAllEnemiesAction kills AcidSlime_M.
    // GremlinHorn.onMonsterDeath addToBot Draw+Energy while the other M lives
    // (hand 6 with Strike_B, energy 4). Rust dealt the 10 and skipped Horn.
    let cfg = default_config(Character::Defect, "773", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 160,
                "773 still fails at Explosive/Gremlin Horn last_ok={} want > 160: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn duplication_tempest_keeps_energy_on_use() {
    // 991: DuplicationPower queues CardQueueItem(tmp, m, card.energyOnUse).
    // The copy channels X=3 Lightning after the original spent the energy.
    let cfg = default_config(Character::Defect, "991", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "991 still fails at Duplication Tempest last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_mode_shift_block_applies_before_next_card() {
    // 149: Fire/Explosive trip Mode Shift; GainBlock 20 is addToBottom of that
    // potion DamageAction. Sweeping Beam must hit the 20 block (200 vs 194).
    let cfg = default_config(Character::Defect, "149", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "149 still fails at Guardian Mode Shift block last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn static_discharge_channels_before_next_divider_hit() {
    // 389: StaticDischargePower.onAttacked addToTop(ChannelAction). Frost
    // evoke block lands before remaining Hexaghost Divider hits (56 vs 41).
    let cfg = default_config(Character::Defect, "389", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "389 still fails at Static Discharge/Divider last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn empty_cage_opens_purge_grid_once() {
    // 723: EmptyCage.onEquip opens a 2-card purge GRID. Staying on BossRelic
    // treated the next Choose as another relic pick (two Empty Cages).
    let cfg = default_config(Character::Defect, "723", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 148,
                "723 still fails at Empty Cage last_ok={} want > 148: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn regen_skips_when_eot_orbs_end_combat() {
    // 714: RegenPower.atEndOfTurn is AbstractRoom.endTurn after orbs.
    // Lethal EOT lightning skips RegenAction (Java hp 14 Regen 2, rust ticked).
    let cfg = default_config(Character::Defect, "714", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 142,
                "714 still fails at EOT Regen last_ok={} want > 142: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sharp_hide_hits_after_lethal_attack() {
    // 723: SharpHide onUseCard queues THORNS before Channel/evoke resolve.
    // Ball Lightning+ evoke killed Guardian; rust skipped hide (block 6 vs 3).
    let cfg = default_config(Character::Defect, "723", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 141,
                "723 still fails at Sharp Hide last_ok={} want > 141: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_drops_wait_after_lethal() {
    // 211: DistilledChaosPotion queues PlayTopCardAction (WAIT). After a
    // lethal top card, clearPostCombatActions drops the rest so Glacier
    // never grants 7 block.
    let cfg = default_config(Character::Defect, "211", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 141,
                "211 still fails at Distilled Chaos last_ok={} want > 141: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn rainbow_upgrade_stops_exhaust() {
    // 935: Java Rainbow.upgrade sets exhaust=false (cost stays 2). Rust kept
    // exhaust so EndTurn shuffled Rainbow into draw vs exhaust pile.
    let cfg = default_config(Character::Defect, "935", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 143,
                "935 still fails at Rainbow+ exhaust last_ok={} want > 143: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn blizzard_applies_strength_with_zero_frost() {
    // 840: Blizzard.use calculateCardDamage after baseDamage = frostCount*2;
    // frostCount 0 still gets Strength 2 (Java SlaverRed 18, rust skipped the hit).
    let cfg = default_config(Character::Defect, "840", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 140,
                "840 still fails at Blizzard last_ok={} want > 140: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn chrysalis_shuffles_zero_cost_skills_into_draw() {
    // 63: Chrysalis use() rolls 3 SKILLs then queues random-spot inserts
    // (interleaving pick/insert drew Steam Power). Calipers loseBlock(15).
    let cfg = default_config(Character::Defect, "63", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 176,
                "63 still fails at Chrysalis/Calipers last_ok={} want > 176: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mind_blast_damage_is_draw_pile_size() {
    // 543: Mind Blast applyPowers baseDamage = drawPile.size(); rust dealt 0
    // so Looter lived (18) while Java ended combat.
    let cfg = default_config(Character::Defect, "543", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 137,
                "543 still fails at Mind Blast last_ok={} want > 137: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn apotheosis_keeps_gash_combat_bonus() {
    // 270: GashAction +2 this combat, then Apotheosis upgradeDamage(2)
    // on the mutated base. Catalog overwrite reset Gash+ to 5 so
    // Hexaghost took 5 (hp 158) instead of 7 (hp 156).
    let cfg = default_config(Character::Defect, "270", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 177,
                "270 still fails at Gash+/Apotheosis last_ok={} want > 177: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn master_of_strategy_draws_and_exhausts() {
    // 193: Master of Strategy DrawCardAction(3) + exhaust. Rust discarded
    // it and skipped the draws (hand 4 vs 7).
    let cfg = default_config(Character::Defect, "193", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 175,
                "193 still fails at Master of Strategy last_ok={} want > 175: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn abacus_block_on_empty_deck_shuffle() {
    // 443: EmptyDeckShuffleAction during turn-start draw fires Abacus
    // GainBlock 6 after loseBlock (block 6 vs 0). draw_cards_rng shuffled
    // without onShuffle.
    let cfg = default_config(Character::Defect, "443", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "443 still fails at Abacus shuffle block last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn astrolabe_transforms_three_cards() {
    // 133: Astrolabe.onEquip GRID 3, transformCard(c, true, miscRng).
    // Choose 13/11/7 = Ball Lightning, Melter, Compile Driver → Gash+/White
    // Noise+/Steam+. Missing the GRID left the old deck.
    let cfg = default_config(Character::Defect, "133", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "133 still fails at Astrolabe last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn maw_bank_gold_on_boss_treasure_enter() {
    // 683: MawBank.onEnterRoom TreasureRoomBoss +12 after boss rewards Proceed.
    let cfg = default_config(Character::Defect, "683", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "683 still fails at MawBank boss chest last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn duplication_does_not_leave_original_free() {
    // 423: DuplicationPotion copy is a separate CardQueueItem with
    // freeToPlayOnce; the original Zap is discarded without it. Scrape then
    // discards the redrawn Zap (cost 1). Rust kept Zap in hand.
    let cfg = default_config(Character::Defect, "423", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 172,
                "423 still fails at Duplication/Scrape Zap last_ok={} want > 172: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn seek_upgraded_grid_picks_two() {
    // 96: Apotheosis upgrades Seek to magic 2. BetterDrawPileToHandAction
    // GRID stays open after the first Choose (Sweeping Beam); second is
    // Melter. Rust closed after one pick (hand 3 vs 2).
    let cfg = default_config(Character::Defect, "96", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "96 still fails at Seek+ GRID last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn trip_applies_vulnerable() {
    // 496: Trip.use Vulnerable 2 on Guardian. Ball Lightning then hits 7*1.5
    // plus Lightning evoke 8 (214 not 217).
    let cfg = default_config(Character::Defect, "496", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 171,
                "496 still fails at Trip Vulnerable last_ok={} want > 171: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn philosophers_stone_gives_enemies_strength() {
    // 645: PhilosopherStone.atBattleStart Strength 1 on Looter+Mugger
    // (intent 11 not 10). Missing it, EndTurn is 60 HP vs Java 58.
    let cfg = default_config(Character::Defect, "645", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "645 still fails at Philosopher's Stone last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn frozen_core_channels_frost_on_empty_eot() {
    // 870: FrozenCore.onPlayerEndTurn channels Frost if hasEmptyOrb, before
    // TriggerEndOfTurnOrbsAction. Missing it, Parasite 6x2 is 12 not 10.
    let cfg = default_config(Character::Defect, "870", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "870 still fails at Frozen Core EOT Frost last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn secret_weapon_opens_attack_from_deck_grid() {
    // 509: Secret Weapon AttackFromDeckToHandAction GRID; Choose 1 is Cold Snap,
    // exhaust. Rust discarded Secret Weapon and never pulled the attack.
    let cfg = default_config(Character::Defect, "509", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "509 still fails at Secret Weapon last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn secret_technique_opens_skill_from_deck_grid() {
    // 806: Secret Technique GRID of skills; Choose 3 is Glacier, exhaust.
    let cfg = default_config(Character::Defect, "806", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 146,
                "806 still fails at Secret Technique last_ok={} want > 146: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn forethought_puts_card_on_bottom_of_draw() {
    // 277: Colorless Forethought HAND_SELECT; Choose 6 then Proceed.
    let cfg = default_config(Character::Defect, "277", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 135,
                "277 still fails at Forethought last_ok={} want > 135: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn unceasing_top_draws_on_empty_hand() {
    // 114: last card of the turn empties the hand; Top draws Strike_B.
    let cfg = default_config(Character::Defect, "114", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 147,
                "114 still fails at Unceasing Top last_ok={} want > 147: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn stone_calendar_hits_all_on_turn_seven() {
    // 342: Stone Calendar counter 7 deals 52 THORNS (Guardian 97 vs 45).
    let cfg = default_config(Character::Defect, "342", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 147,
                "342 still fails at Stone Calendar last_ok={} want > 147: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ornithopter_defers_heal_on_gambling_overlay() {
    // 287: Gambler's Brew HAND_SELECT snapshot is still pre-Ornithopter heal.
    let cfg = default_config(Character::Defect, "287", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 148,
                "287 still fails at Ornithopter gambling heal last_ok={} want > 148: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_hologram_opens_discard_grid() {
    // 610: Distilled Chaos autoplays Hologram; GRID must open so Choose 1
    // returns Cold Snap and exhausts Hologram.
    let cfg = default_config(Character::Defect, "610", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 111,
                "610 still fails at Distilled Chaos Hologram last_ok={} want > 111: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn creative_ai_rolls_before_loop_lightning() {
    // 937: CreativeAI atStartOfTurn RNG is immediate; Loop only queues
    // LightningOrbPassiveAction. Rust Loop roll stole the POWER pick.
    let cfg = default_config(Character::Defect, "937", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 121,
                "937 still fails at CreativeAI/Loop RNG last_ok={} want > 121: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn purity_opens_exhaust_hand_select() {
    // 241: Colorless Potion Purity must HAND_SELECT (Choose 2 is Strike).
    let cfg = default_config(Character::Defect, "241", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 122,
                "241 still fails at Purity HAND_SELECT last_ok={} want > 122: {fail}",
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
fn guardian_thorns_mode_shift_beats_vent_steam() {
    // 275: Fierce Bash setMove(Vent Steam) before DamageAction resolves.
    // Player Thorns 3 crosses Mode Shift; CLOSE_UP must stick (Sharp Hide,
    // no Weak/Vuln). Rust overwrote CLOSE_UP (hp 45 vs 42, Guardian 178 vs 173).
    let cfg = default_config(Character::Defect, "275", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 168,
                "275 still fails at Fierce Bash Mode Shift last_ok={} want > 168: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ornamental_fan_block_before_sharp_hide() {
    // 872: UseCardAction ctor relics-then-monster-powers after card.use.
    // Fan GainBlock 4 then Sharp Hide 3 (hp 58 block 1, not 55/4).
    let cfg = default_config(Character::Defect, "872", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 166,
                "872 still fails at Fan vs Sharp Hide last_ok={} want > 166: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mayhem_plays_top_card_at_turn_start() {
    // 533: MayhemPower.atStartOfTurn queues PlayTopCard after DrawCardAction.
    // Missing the power left Genetic Algorithm in the deck (hand/block diverge).
    let cfg = default_config(Character::Defect, "533", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 168,
                "533 still fails at Mayhem last_ok={} want > 168: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn incense_burner_intangible_cuts_sharp_hide() {
    // 604: Incense Burner Intangible 1; Guardian Sharp Hide 3. Compile Driver
    // THORNS is 1 not 3 (Java hp 43 vs rust 41 at seq 166).
    let cfg = default_config(Character::Defect, "604", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 165,
                "604 still fails at Intangible Sharp Hide last_ok={} want > 165: {fail}",
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
