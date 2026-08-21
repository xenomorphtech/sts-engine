//! Act 4 (The Ending) room unit tests: rest, shop, Shield and Spear, Corrupt Heart.

use sts_engine::action::Action;
use sts_engine::combat::Combat;
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Act, Character, EncounterId, MonsterId, RoomType};
use sts_engine::rng::RngSet;
use sts_engine::Unlocks;

fn ending_game() -> Game {
    let mut game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    game.dungeon.generate_act(
        Act::Ending,
        2,
        &mut game.rng,
        &game.unlocks,
        Character::Ironclad,
        0,
        true,
    );
    game.screen = Screen::Map;
    game
}

#[test]
fn ending_map_is_rest_shop_elite_heart() {
    let game = ending_game();
    assert_eq!(game.dungeon.act, Act::Ending);
    assert_eq!(game.dungeon.boss, "The Heart");
    assert_eq!(game.dungeon.map.node(3, 0).room, Some(RoomType::Rest));
    assert_eq!(game.dungeon.map.node(3, 1).room, Some(RoomType::Shop));
    assert_eq!(game.dungeon.map.node(3, 2).room, Some(RoomType::Elite));
    assert_eq!(game.dungeon.map.node(3, 3).room, Some(RoomType::Boss));
    assert!(game.dungeon.map.node(3, 0).has_edges());
}

#[test]
fn ending_rest_heals_thirty_percent() {
    let mut game = ending_game();
    game.player.hp = 50;
    game.step(&Action::Choose {
        index: 0,
        label: Some("map node".into()),
        x: Some(3),
        y: Some(0),
        room: Some("RestRoom".into()),
    });
    assert_eq!(game.screen, Screen::Rest);
    game.step(&Action::Choose {
        index: 0,
        label: Some("Rest".into()),
        x: None,
        y: None,
        room: None,
    });
    assert_eq!(game.player.hp, 74); // 50 + floor(80 * 0.3)
    assert_eq!(game.screen, Screen::Rest);
    game.step(&Action::Proceed);
    assert_eq!(game.screen, Screen::Map);
}

#[test]
fn ending_shop_can_leave() {
    let mut game = ending_game();
    game.dungeon.first_room_chosen = true;
    game.current_x = 3;
    game.current_y = 0;
    game.step(&Action::Choose {
        index: 0,
        label: Some("map node".into()),
        x: Some(3),
        y: Some(1),
        room: Some("ShopRoom".into()),
    });
    assert_eq!(game.screen, Screen::Shop);
    assert_eq!(game.current_room, RoomType::Shop);
    game.step(&Action::Proceed);
    assert_eq!(game.screen, Screen::Map);
}

#[test]
fn ending_shop_exposes_purchases_to_htn() {
    let mut game = ending_game();
    game.player.gold = 1_000;
    game.dungeon.first_room_chosen = true;
    game.current_x = 3;
    game.current_y = 0;
    game.step(&Action::Choose {
        index: 0,
        label: Some("map node".into()),
        x: Some(3),
        y: Some(1),
        room: Some("ShopRoom".into()),
    });

    let mut agent = HtnAgent::new();
    let open = agent.decide(&game);
    assert!(matches!(
        open,
        Action::Choose {
            label: Some(ref label),
            ..
        } if label == "shop"
    ));
    game.step(&open);

    let legal = game.legal_actions();
    let purchases = legal
        .iter()
        .filter(|action| matches!(action, Action::Choose { .. }))
        .count();
    assert_eq!(purchases, 14, "purge plus every generated shop offer");
    assert!(legal.iter().any(|action| matches!(action, Action::Proceed)));

    let purchase = agent.decide(&game);
    assert!(matches!(
        purchase,
        Action::Choose {
            label: Some(ref label),
            ..
        } if label != "shop"
    ));
    let gold_before = game.player.gold;
    game.step(&purchase);
    assert!(game.screen == Screen::Grid || game.player.gold < gold_before);
}

#[test]
fn shield_and_spear_spawn() {
    let mut rng = RngSet::generate_seeds(2);
    let combat = Combat::start(EncounterId::ShieldAndSpear, &mut sts_engine::creature::Player::ironclad(), &mut rng, 52, 2, 0);
    assert_eq!(combat.monsters.len(), 2);
    assert_eq!(combat.monsters[0].id, MonsterId::SpireShield);
    assert_eq!(combat.monsters[1].id, MonsterId::SpireSpear);
    assert_eq!(combat.monsters[0].hp, 110);
    assert_eq!(combat.monsters[1].hp, 160);
}

#[test]
fn corrupt_heart_spawns_and_debuffs() {
    let mut rng = RngSet::generate_seeds(2);
    let mut player = sts_engine::creature::Player::ironclad();
    let mut combat = Combat::start(EncounterId::CorruptHeart, &mut player, &mut rng, 54, 2, 0);
    assert_eq!(combat.monsters[0].id, MonsterId::CorruptHeart);
    assert_eq!(combat.monsters[0].hp, 750);
    assert_eq!(combat.monsters[0].next_move, 3);
    sts_engine::combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert!(player.power_amount(sts_engine::ids::PowerId::Vulnerable) > 0);
    assert!(player.power_amount(sts_engine::ids::PowerId::Weak) > 0);
    assert!(player.power_amount(sts_engine::ids::PowerId::Frail) > 0);
}

#[test]
fn ending_elite_is_shield_and_spear() {
    let mut game = ending_game();
    game.dungeon.first_room_chosen = true;
    game.current_x = 3;
    game.current_y = 1;
    game.step(&Action::Choose {
        index: 0,
        label: Some("map node".into()),
        x: Some(3),
        y: Some(2),
        room: Some("MonsterRoomElite".into()),
    });
    assert_eq!(game.screen, Screen::Combat);
    let combat = game.combat.expect("elite combat");
    assert_eq!(combat.encounter, EncounterId::ShieldAndSpear);
}
