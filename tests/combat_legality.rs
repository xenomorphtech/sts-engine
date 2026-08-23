use sts_engine::action::Action;
use sts_engine::card::Card;
use sts_engine::combat::Combat;
use sts_engine::creature::RelicInstance;
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{CardId, Character, EncounterId, RelicId};
use sts_engine::Unlocks;

#[test]
fn unplayable_status_is_not_a_legal_play_without_medical_kit() {
    let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
    let combat = Combat::start(
        EncounterId::Cultist,
        &mut game.player,
        &mut game.rng,
        1,
        2,
        0,
    );
    game.combat = Some(combat);
    game.screen = Screen::Combat;
    *game.player.hand = vec![Card::new(CardId::Dazed)];

    assert!(!game
        .legal_actions()
        .iter()
        .any(|action| matches!(action, Action::Play { .. })));

    game.player.relics.push(RelicInstance {
        id: RelicId::Medical_Kit,
        counter: -1,
        used_up: false,
    });
    assert!(game
        .legal_actions()
        .iter()
        .any(|action| matches!(action, Action::Play { .. })));
    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });
    assert!(game.player.hand.is_empty());
    assert!(game
        .player
        .discard
        .iter()
        .all(|card| card.id != CardId::Dazed));
    assert!(game
        .player
        .exhaust
        .iter()
        .any(|card| card.id == CardId::Dazed));
}

#[test]
fn htn_ends_turn_instead_of_replaying_a_zero_progress_dazed_loop() {
    let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
    let combat = Combat::start(
        EncounterId::Chosen,
        &mut game.player,
        &mut game.rng,
        30,
        2,
        0,
    );
    game.combat = Some(combat);
    game.screen = Screen::Combat;
    *game.player.hand = vec![Card::new(CardId::Dazed)];
    *game.player.draw = vec![Card::new(CardId::Dazed)];
    game.player.discard.clear();
    game.player.relics.extend([
        RelicInstance {
            id: RelicId::Medical_Kit,
            counter: -1,
            used_up: false,
        },
        RelicInstance {
            id: RelicId::Unceasing_Top,
            counter: -1,
            used_up: false,
        },
    ]);
    game.player.add_power(sts_engine::ids::PowerId::Hex, 1);

    let mut agent = HtnAgent::new();
    assert_eq!(agent.decide(&game), Action::EndTurn);
}
