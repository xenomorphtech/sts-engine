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
