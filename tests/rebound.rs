use sts_engine::card::Card;
use sts_engine::combat::{draw_cards_rng, play_owned_card, Combat};
use sts_engine::creature::Player;
use sts_engine::game::{Game, Screen};
use sts_engine::ids::{CardId, Character, EncounterId, PowerId};
use sts_engine::rng::RngSet;
use sts_engine::{Action, Unlocks};

fn combat_fixture() -> (Player, Combat, RngSet) {
    let mut player = Player::defect();
    let mut rng = RngSet::generate_seeds(2);
    let combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 1, 2, 0);
    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    player.exhaust.clear();
    player.energy = 3;
    (player, combat, rng)
}

#[test]
fn rebound_places_the_next_non_power_card_on_top_of_draw() {
    let (mut player, mut combat, mut rng) = combat_fixture();

    assert!(!play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Rebound),
        Some(0),
        &mut rng,
        None,
    ));
    assert_eq!(player.power_amount(PowerId::Rebound), 1);
    assert_eq!(
        player.discard.last().map(|card| card.id),
        Some(CardId::Rebound)
    );

    let ball_lightning = Card::new(CardId::Ball_Lightning);
    assert!(!play_owned_card(
        &mut player,
        &mut combat,
        ball_lightning,
        Some(0),
        &mut rng,
        None,
    ));
    assert_eq!(player.power_amount(PowerId::Rebound), 0);
    assert_eq!(player.draw.last(), Some(&ball_lightning));
    assert!(!player
        .discard
        .iter()
        .any(|card| card.id == CardId::Ball_Lightning));

    assert_eq!(draw_cards_rng(&mut player, 1, Some(&mut rng)), 0);
    assert_eq!(
        player.hand.last().map(|card| card.id),
        Some(CardId::Ball_Lightning)
    );
}

#[test]
fn rebound_is_consumed_by_a_power_without_routing_it_to_draw() {
    let (mut player, mut combat, mut rng) = combat_fixture();
    player.add_power(PowerId::Rebound, 1);

    assert!(!play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Defragment),
        None,
        &mut rng,
        None,
    ));

    assert_eq!(player.power_amount(PowerId::Rebound), 0);
    assert!(player.draw.is_empty());
    assert!(player.discard.is_empty());
}

#[test]
fn exhaust_takes_precedence_over_rebound() {
    let (mut player, mut combat, mut rng) = combat_fixture();
    player.add_power(PowerId::Rebound, 1);
    let mut zap = Card::new(CardId::Zap);
    zap.exhaust = true;

    assert!(!play_owned_card(
        &mut player,
        &mut combat,
        zap,
        None,
        &mut rng,
        None,
    ));

    assert_eq!(player.power_amount(PowerId::Rebound), 0);
    assert!(player.draw.is_empty());
    assert_eq!(player.exhaust.last().map(|card| card.id), Some(CardId::Zap));
}

#[test]
fn rebound_destination_survives_a_card_selection_overlay() {
    let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::Cultist,
        &mut game.player,
        &mut game.rng,
        1,
        2,
        0,
    ));
    game.screen = Screen::Combat;
    let mut hologram = Card::new(CardId::Hologram);
    hologram.upgrade();
    *game.player.hand = vec![hologram];
    *game.player.draw = Vec::new();
    *game.player.discard = vec![Card::new(CardId::Strike_B), Card::new(CardId::Defend_B)];
    game.player.energy = 3;
    game.player.add_power(PowerId::Rebound, 1);

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::Grid);
    assert_eq!(game.player.power_amount(PowerId::Rebound), 0);

    let choice = game
        .legal_actions()
        .into_iter()
        .find(|action| matches!(action, Action::Choose { .. }))
        .expect("Hologram selection");
    game.step(&choice);

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.player.draw.last(), Some(&hologram));
    assert!(!game
        .player
        .discard
        .iter()
        .any(|card| card.id == CardId::Hologram));
}

#[test]
fn rebound_expires_at_end_of_turn() {
    let mut player = Player::defect();
    player.add_power(PowerId::Rebound, 1);
    sts_engine::creature::end_of_turn(&mut player.powers);
    assert_eq!(player.power_amount(PowerId::Rebound), 0);
}
