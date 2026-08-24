//! Cross-turn Defect planning regressions for effects whose value is not
//! immediate damage or block.

use sts_engine::action::Action;
use sts_engine::card::Card;
use sts_engine::combat::Combat;
use sts_engine::creature::{Orb, OrbKind};
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Act, CardId, Character, EncounterId, PowerId, RoomType};
use sts_engine::Unlocks;

fn start_combat(game: &mut Game, encounter: EncounterId, act: Act, room: RoomType) {
    game.dungeon.act = act;
    game.current_room = room;
    game.combat = Some(Combat::start(
        encounter,
        &mut game.player,
        &mut game.rng,
        31,
        2,
        game.ascension,
    ));
    game.screen = Screen::Combat;
}

fn quiet_intents(game: &mut Game) {
    for monster in &mut game.combat.as_mut().unwrap().monsters {
        monster.intent_damage = 0;
        monster.intent_hits = 0;
    }
}

fn play_out_turn(game: &mut Game) -> Vec<Action> {
    let mut agent = HtnAgent::new();
    let mut played = Vec::new();
    for _ in 0..30 {
        let action = agent.decide(game);
        if matches!(action, Action::EndTurn) {
            break;
        }
        game.step(&action);
        played.push(action);
        if game.screen != Screen::Combat {
            break;
        }
    }
    played
}

fn single_card_turn(card: Card, energy: i32) -> (Game, Vec<Action>) {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(&mut game, EncounterId::Cultist, Act::Exordium, RoomType::Monster);
    quiet_intents(&mut game);
    game.player.energy = energy;
    game.player.deck = vec![card.clone()].into();
    game.player.hand = vec![card].into();
    game.player.draw.clear();
    game.player.discard.clear();
    let played = play_out_turn(&mut game);
    (game, played)
}

#[test]
fn equilibrium_blocks_and_retains_the_rest_of_the_hand_for_one_turn() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(&mut game, EncounterId::Cultist, Act::Exordium, RoomType::Monster);
    game.player.energy = 2;
    game.player.hand = vec![Card::new(CardId::Undo), Card::new(CardId::Sunder)].into();
    game.player.draw.clear();
    game.player.discard.clear();

    game.step(&Action::Play { hand_index: 0, target_index: None });
    assert_eq!(game.player.block, 13);
    assert_eq!(game.player.power_amount(PowerId::Equilibrium), 1);
    game.step(&Action::EndTurn);

    assert!(game.player.hand.iter().any(|card| card.id == CardId::Sunder));
    assert_eq!(game.player.power_amount(PowerId::Equilibrium), 0);
}

#[test]
fn equilibrium_is_used_to_carry_an_unplayable_attack_into_next_turn() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(&mut game, EncounterId::Cultist, Act::Exordium, RoomType::Monster);
    quiet_intents(&mut game);
    game.player.energy = 2;
    game.player.hand = vec![Card::new(CardId::Undo), Card::new(CardId::Sunder)].into();
    game.player.draw.clear();
    game.player.discard.clear();

    let played = play_out_turn(&mut game);

    assert!(matches!(played.first(), Some(Action::Play { hand_index: 0, .. })));
}

#[test]
fn genetic_algorithm_is_trained_on_a_quiet_turn() {
    let (game, played) = single_card_turn(Card::new(CardId::Genetic_Algorithm), 1);

    assert!(!played.is_empty());
    let trained = game
        .player
        .deck
        .iter()
        .find(|card| card.id == CardId::Genetic_Algorithm)
        .map(|card| card.misc)
        .unwrap_or(0);
    assert!(trained >= 3, "Genetic Algorithm did not gain permanent block: {trained}");
}

#[test]
fn charge_battery_is_used_to_bank_next_turn_energy() {
    let (game, played) = single_card_turn(Card::new(CardId::Conserve_Battery), 1);

    assert!(!played.is_empty());
    assert_eq!(game.player.power_amount(PowerId::Energized), 1);
}

#[test]
fn buffer_is_installed_before_a_future_attack() {
    let (game, played) = single_card_turn(Card::new(CardId::Buffer), 2);

    assert!(!played.is_empty());
    assert_eq!(game.player.power_amount(PowerId::Buffer), 1);
}

#[test]
fn static_discharge_is_installed_before_a_future_attack() {
    let (game, played) = single_card_turn(Card::new(CardId::Static_Discharge), 1);

    assert!(!played.is_empty());
    assert_eq!(game.player.power_amount(PowerId::StaticDischarge), 1);
}

#[test]
fn core_surge_is_played_before_biased_cognition() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::all());
    start_combat(&mut game, EncounterId::Cultist, Act::Exordium, RoomType::Monster);
    quiet_intents(&mut game);
    game.player.energy = 2;
    game.player.hand = vec![
        Card::new(CardId::Biased_Cognition),
        Card::new(CardId::Core_Surge),
    ]
    .into();

    let played = play_out_turn(&mut game);

    assert!(matches!(played.first(), Some(Action::Play { hand_index: 1, .. })));
    assert_eq!(game.player.power_amount(PowerId::Focus), 4);
    assert_eq!(game.player.power_amount(PowerId::Bias), 0);
}

#[test]
fn fission_does_not_erase_a_dark_bank_for_empty_draw_and_energy() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(&mut game, EncounterId::Cultist, Act::Exordium, RoomType::Monster);
    quiet_intents(&mut game);
    game.player.energy = 0;
    game.player.hand = vec![Card::new(CardId::Fission)].into();
    game.player.draw.clear();
    game.player.discard.clear();
    game.player.orbs = vec![
        Orb { kind: OrbKind::Dark, evoke: 40 },
        Orb { kind: OrbKind::Frost, evoke: 0 },
    ]
    .into();

    let played = play_out_turn(&mut game);

    assert!(played.is_empty());
    assert!(game.player.orbs.iter().any(|orb| orb.kind == OrbKind::Dark && orb.evoke >= 40));
}

#[test]
fn multi_cast_does_not_dump_a_dark_bank_into_low_hp_chaff() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(
        &mut game,
        EncounterId::CultistAndChosen,
        Act::City,
        RoomType::Monster,
    );
    quiet_intents(&mut game);
    let monsters = &mut game.combat.as_mut().unwrap().monsters;
    monsters[0].hp = 180;
    monsters[0].max_hp = 180;
    monsters[1].hp = 30;
    monsters[1].max_hp = 60;
    game.player.energy = 1;
    game.player.hand = vec![Card::new(CardId::Multi_Cast)].into();
    game.player.orbs = vec![Orb { kind: OrbKind::Dark, evoke: 45 }].into();

    let played = play_out_turn(&mut game);

    assert!(played.is_empty());
    assert!(game.player.orbs.iter().any(|orb| orb.kind == OrbKind::Dark && orb.evoke >= 45));
}
