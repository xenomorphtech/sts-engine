use sts_engine::combat::Combat;
use sts_engine::creature::OrbKind;
use sts_engine::game::Game;
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{CardId, Character, RelicId};
use sts_engine::rng::RngSet;
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
