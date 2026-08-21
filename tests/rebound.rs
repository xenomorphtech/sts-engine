use sts_engine::card::Card;
use sts_engine::combat::{play_owned_card, Combat};
use sts_engine::creature::Player;
use sts_engine::ids::{CardId, EncounterId, PowerId};
use sts_engine::rng::RngSet;

#[test]
fn rebound_places_the_next_non_power_card_on_top_of_draw() {
    let mut rng = RngSet::generate_seeds(2);
    let mut player = Player::defect();
    let mut combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 1, 2, 0);
    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    player.energy = 10;

    play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Rebound),
        Some(0),
        &mut rng,
        None,
    );
    assert_eq!(player.power_amount(PowerId::Rebound), 1);
    assert_eq!(player.discard.last().map(|c| c.id), Some(CardId::Rebound));

    play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Ball_Lightning),
        Some(0),
        &mut rng,
        None,
    );
    assert_eq!(player.power_amount(PowerId::Rebound), 0);
    assert_eq!(
        player.draw.last().map(|c| c.id),
        Some(CardId::Ball_Lightning)
    );
    assert!(!player
        .discard
        .iter()
        .any(|c| c.id == CardId::Ball_Lightning));
}

#[test]
fn rebound_expires_at_end_of_turn() {
    let mut player = Player::defect();
    player.add_power(PowerId::Rebound, 1);
    sts_engine::creature::end_of_turn(&mut player.powers);
    assert_eq!(player.power_amount(PowerId::Rebound), 0);
}
