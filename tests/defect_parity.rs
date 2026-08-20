use sts_engine::combat::Combat;
use sts_engine::creature::OrbKind;
use sts_engine::game::Game;
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{CardId, Character, RelicId};
use sts_engine::rng::RngSet;
use sts_engine::walk::{default_config, walk_oracle};
use sts_engine::Unlocks;

#[test]
fn defect_starter_loadout() {
    let p = sts_engine::creature::Player::defect();
    assert_eq!(p.hp, 75);
    assert_eq!(p.max_hp, 75);
    assert_eq!(p.max_orbs, 3);
    assert_eq!(p.relics[0].id, RelicId::Cracked_Core);
    let ids: Vec<_> = p.deck.iter().map(|c| c.id).collect();
    assert_eq!(ids.iter().filter(|id| **id == CardId::Strike_B).count(), 4);
    assert_eq!(ids.iter().filter(|id| **id == CardId::Defend_B).count(), 4);
    assert!(ids.contains(&CardId::Zap));
    assert!(ids.contains(&CardId::Dualcast));
}

#[test]
fn cracked_core_channels_lightning_at_battle_start() {
    let mut player = sts_engine::creature::Player::defect();
    let mut rng = RngSet::generate_seeds(2);
    let combat = Combat::start(sts_engine::ids::EncounterId::Cultist, &mut player, &mut rng, 1, 2, 0);
    assert_eq!(player.orbs.len(), 1);
    assert_eq!(player.orbs[0].kind, OrbKind::Lightning);
    assert!(!combat.monsters.is_empty());
}

#[test]
fn zap_channels_a_lightning_orb() {
    use sts_engine::action::Action;
    let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
    // Talk + first neow option, then pick first map node and play until Zap is in hand.
    for _ in 0..8 {
        if game.combat.is_some() {
            break;
        }
        let legal = game.legal_actions();
        if let Some(a) = legal.into_iter().next() {
            game.step(&a);
        }
    }
    if game.combat.is_none() {
        return;
    }
    let before = game.player.orbs.len();
    let zap = game.player.hand.iter().position(|c| c.id == CardId::Zap);
    if let Some(i) = zap {
        game.step(&Action::Play {
            hand_index: i,
            target_index: None,
        });
        assert!(game.player.orbs.len() > before || before >= game.player.max_orbs as usize);
    }
}

#[test]
fn htn_emits_legal_actions_for_defect_and_ironclad() {
    for character in [Character::Ironclad, Character::Defect] {
        let mut game = Game::new(2, character, 0, Unlocks::fixture());
        let mut agent = HtnAgent::new();
        for _ in 0..40 {
            if game.done || game.player.hp <= 0 {
                break;
            }
            let legal = game.legal_actions();
            if legal.is_empty() {
                break;
            }
            let action = agent.decide(&game);
            if matches!(action, sts_engine::Action::Quit) {
                break;
            }
            assert!(
                legal.iter().any(|a| std::mem::discriminant(a) == std::mem::discriminant(&action)
                    || *a == action),
                "{character:?} HTN chose {action:?} not in {legal:?}"
            );
            game.step(&action);
        }
        assert!(game.dungeon.floor >= 1 || game.screen != sts_engine::game::Screen::Neow);
    }
}

#[test]
fn fossilized_helix_buffers_first_hit() {
    // 961743 Helix Buffer 1 absorbs Fat's 5; without it rust HP 43 vs Java 48.
    for (seed, min_ok) in [("961743", 55)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Fossilized Helix last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn decay_eot_and_weak_potion_lockstep() {
    // 453310 Decay THORNS 2 after Frost so JawWorm 7 vs 6 block chips 1 HP.
    // 684676 Weak Potion 3 so Lagavulin 20 → 15.
    for (seed, min_ok) in [("453310", 55), ("684676", 56)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Decay/Weak potion last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn fear_and_cultist_potion_lockstep() {
    // 544590 / 381412 Fear Potion Vulnerable 3 then Strike (9 vs 6 / 1 through
    // Lagavulin block). 957958 Cultist Potion Ritual → EOT Strength 1, Strike 7.
    for (seed, min_ok) in [("957958", 51), ("544590", 53), ("381412", 54)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Fear/Cultist potion last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn entropic_brew_out_of_combat_is_not_limited() {
    // 861954: Entropic Brew after Wheel of Change. Java returnRandomPotion()
    // (limited=false); rust always passed true and the next hallway dropped
    // Regen, shifting the Melter pick.
    for (seed, min_ok) in [("861954", 46)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Entropic Brew last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn snecko_oil_draws_five_and_randomizes_costs() {
    // 203190 Snecko Oil: draw 5 then cardRandomRng.random(3) per cost>=0 card.
    // Was a no-op (hand 5 vs Java 10).
    for (seed, min_ok) in [("203190", 55)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Snecko Oil last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn shop_purge_cost_persists_across_shops() {
    // 144185 purged at floor 2 (75→100). Floor 5 shop must keep 100 so
    // choose index 4 is Defragment, not Darkness (purge-shifted).
    for (seed, min_ok) in [("144185", 50)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at persisted shop purge last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn distilled_chaos_plays_top_of_draw() {
    // 972720 Distilled Chaos autoplays 3 draw-pile cards (two Defends + Strike
    // → player block 10, Lagavulin block 8→2). Was a no-op.
    for (seed, min_ok) in [("972720", 47)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Distilled Chaos last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn emerald_elite_max_hp_uses_gdx_round() {
    // MonsterRoomElite.applyEmeraldEliteBuff case 1: IncreaseMaxHpAction
    // MathUtils.round(maxHealth * 0.25F). floor() left Lagavulin 114 at 142
    // (Java 143) and Sentry 42 at 52 (Java 53).
    for (seed, min_ok) in [("972720", 46), ("959835", 50), ("836077", 54)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at emerald elite HP last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn acid_slime_l_a17_move_lockstep() {
    // AcidSlime_L A17 getMove uses num<70 and randomBoolean(0.6), not M's
    // num<80 / 0.5. 420468/982689 were slam vs Java Weak lick.
    for (seed, min_ok) in [("420468", 45), ("982689", 48), ("220939", 52), ("439068", 52)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at AcidSlime_L A17 last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn essence_of_darkness_lockstep() {
    // 430181 Essence of Darkness channels Dark per orb slot; a full bar
    // evokes the Cracked Core Lightning (8) before the third Dark lands.
    for (seed, min_ok) in [("430181", 48)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Essence of Darkness last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn feather_watch_apotheosis_lockstep() {
    // 583493 Eternal Feather rest heal (deck/5)*3. 215484 Pocketwatch
    // atTurnStartPostDraw +3 if previous turn played <=3. 195259 Apotheosis.
    for (seed, min_ok) in [("583493", 43), ("215484", 50), ("195259", 44)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Feather/Watch/Apotheosis last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn writhe_innate_flex_swift_shrine_gold_lockstep() {
    // 932701 Writhe is innate (on top after shuffle). 268103 Golden Shrine
    // Pray +50 is gainGold, not RewardItem idol bonus. 620958 Flex Potion
    // (Steroid) Str+5/LoseStr+5. 455171 Swift Potion draws 3.
    for (seed, min_ok) in [
        ("932701", 36),
        ("268103", 38),
        ("620958", 35),
        ("455171", 44),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Writhe/potion/shrine last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn event_room_from_shop_is_not_shop_lockstep() {
    // EventHelper.roll: if getCurrRoom() is still ShopRoom, shopSize=0 so the
    // ? node after a shop cannot convert to Shop (idx 10-11 is Treasure).
    for (seed, min_ok) in [
        ("439817", 28),
        ("297788", 29),
        ("362538", 29),
        ("682932", 29),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at event-from-shop last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn meal_ticket_hourglass_lockstep() {
    // 924030/873442 MealTicket.justEnteredRoom heals 15 in ShopRoom.
    // 97200/885469 MercuryHourglass.atTurnStart deals 3 THORNS to all enemies.
    for (seed, min_ok) in [
        ("924030", 31),
        ("873442", 33),
        ("97200", 29),
        ("885469", 34),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at MealTicket/Hourglass last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn frost_orb_ignores_noblock_lockstep() {
    // 627737 Panic Button NoBlock + Frost passive at EOT: Java addBlock still
    // grants 2, so JawWorm Thrash 7 vs 2 block (hp 52) not vs 0 (hp 50).
    // 457969 Dark Shackles Str-9 bite must clamp to 0, not add to player block.
    for (seed, min_ok) in [("627737", 30), ("457969", 26)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Frost/NoBlock last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn hand_of_greed_lockstep() {
    // 259462 Hand of Greed 20 damage (GreedAction) on Cultist.
    for (seed, min_ok) in [("259462", 29)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Hand of Greed last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn curl_up_boot_after_block_lockstep() {
    // 234953 Rip and Tear 7×2 before Curl Up addToBot block; 154632/909358
    // Boot onAttackToChangeDamage after decrementBlock (unblocked 1–4 → 5).
    for (seed, min_ok) in [("234953", 9), ("154632", 12), ("909358", 12)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Curl Up/Boot last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn woman_in_blue_reward_screen_lockstep() {
    // 809652 / 706888 / 874370: Woman in Blue opens CombatReward with
    // PotionHelper potions, then Proceed returns to the event Leave.
    for (seed, min_ok) in [("809652", 17), ("706888", 19), ("874370", 19), ("217337", 20)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at Woman in Blue last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn reinforced_thunder_static_shrine_lockstep() {
    // 672603 Reinforced Body X block, 44477 Thunder Strike random hits,
    // 29041 Static Discharge channel on hit, 544172 Golden Shrine +50 gold.
    for (seed, min_ok) in [
        ("672603", 11),
        ("44477", 12),
        ("29041", 17),
        ("544172", 14),
        ("804998", 17),
        ("888563", 16),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn reboot_order_boot_shackles_lockstep() {
    // 883356/642248/249652 first-combat Reboot hand order; 608360 Boot
    // Weak Strike 4→5; 475701 Dark Shackles Str-9.
    for (seed, min_ok) in [
        ("883356", 5),
        ("642248", 6),
        ("249652", 6),
        ("608360", 9),
        ("475701", 9),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn claw_recursion_white_noise_lockon_lockstep() {
    // 497600 Claw (Gash), 446709 White Noise, 214833 Lock-On,
    // 638400 Chaos/Overclock, 324780 Dramatic Entrance, 905059 Good
    // Instincts, 948645 Panic Button.
    for (seed, min_ok) in [
        ("497600", 7),
        ("446709", 6),
        ("214833", 8),
        ("638400", 6),
        ("324780", 6),
        ("905059", 6),
        ("948645", 6),
    ] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn flash_panacea_genetic_rip_lockstep() {
    // 989496 Flash of Steel draw, 328727 Panacea Artifact, 160663 Genetic
    // Algorithm 1 block, 31732 Rip and Tear random hits without a target.
    for (seed, min_ok) in [("989496", 7), ("328727", 13), ("160663", 22), ("31732", 22)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn marbles_seek_impatience_lockstep() {
    // 478329 Bag of Marbles Vulnerable, 808348 Seek draw-pile GRID,
    // 971636 Impatience conditional draw.
    for (seed, min_ok) in [("478329", 4), ("808348", 7), ("971636", 11), ("377225", 59)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented effect last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn machine_learning_all_for_one_fusion_lockstep() {
    // 395084 Machine Learning extra draw, 631058 All For One 10 damage,
    // 321898 Fusion channels Plasma.
    for (seed, min_ok) in [("395084", 7), ("631058", 5), ("321898", 34)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn reboot_creative_ai_scrape_lockstep() {
    // 875398 Reboot reshuffle+draw, 31813 Creative AI start-of-turn power card,
    // 369514 Scrape 7 damage.
    for (seed, min_ok) in [("875398", 4), ("31813", 14), ("369514", 27)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at unimplemented card last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn fruit_juice_does_not_defer_first_map_node() {
    // 191892: Neow three potions, Fruit Juice in belt. Map choose must enter
    // floor 1, not park pending_room.
    let cfg = default_config(Character::Defect, "191892", Unlocks::fixture(), 20);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 5,
                "Fruit Juice still deferred first hallway: {fail}"
            );
        }
    }
}

#[test]
fn unimplemented_defect_cards_and_bronze_scales_lockstep() {
    // First-combat reds: Fission draw (107249), Multi-Cast evokes (112185),
    // Finesse block+draw (779907), Bronze Scales thorns (511896).
    for (seed, min_ok) in [("107249", 8), ("112185", 4), ("779907", 8), ("511896", 7)] {
        let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
        match walk_oracle(&cfg) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {}
            Err(fail) => {
                assert!(
                    fail.last_ok > min_ok,
                    "{seed} still fails at the unimplemented effect last_ok={} want > {min_ok}: {fail}",
                    fail.last_ok
                );
            }
        }
    }
}

#[test]
fn maw_bank_grants_gold_on_first_hallway() {
    // 116441: Neow MawBank, Java gold=111 on floor 1. Missing onEnterRoom left rust at 99.
    let cfg = default_config(Character::Defect, "116441", Unlocks::fixture(), 20);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 3,
                "MawBank gold still desyncs at first hallway: {fail}"
            );
        }
    }
}

#[test]
fn neow_transform_uses_running_commons_and_src_uncommon_rare() {
    // 463905: Java transformed Strike_B → Capacitor. Concatenating all three
    // running pools picked Steam Power instead (src uncommon/rare are reversed).
    let cfg = default_config(Character::Defect, "463905", Unlocks::fixture(), 20);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 3,
                "neow transform still desyncs at Leave: {fail}"
            );
        }
    }
}
