use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Character, RoomType};
use sts_engine::{Action, Unlocks};

const SEED: i64 = 979_071_298_687_117_498;

#[test]
fn act_one_row_thirteen_leads_to_the_mandatory_row_fourteen_rest() {
    let mut game = Game::new(SEED, Character::Defect, 20, Unlocks::fixture());
    game.dungeon.first_room_chosen = true;
    game.current_x = 5;
    game.current_y = 13;
    game.current_room = RoomType::Event;
    game.screen = Screen::Map;

    let choices: Vec<_> = game
        .legal_actions()
        .into_iter()
        .filter_map(|action| match action {
            Action::Choose { x, y, room, .. } => Some((x, y, room)),
            _ => None,
        })
        .collect();

    assert!(!choices.is_empty());
    assert!(
        choices.iter().all(|(_, y, room)| {
            *y == Some(14) && room.as_deref() == Some("com.megacrit.cardcrawl.rooms.RestRoom")
        }),
        "row 13 choices were {choices:?}"
    );
}

#[test]
fn rust_htn_reaches_we_meet_again_on_act_one_floor_fourteen() {
    let mut game = Game::new(SEED, Character::Defect, 20, Unlocks::fixture());
    let mut agent = HtnAgent::new();

    for _ in 0..500 {
        if game.dungeon.floor == 14 && game.screen == Screen::Event {
            break;
        }
        let action = agent.decide(&game);
        assert_ne!(
            action,
            Action::Quit,
            "HTN stopped at floor {} on {:?}",
            game.dungeon.floor,
            game.screen
        );
        game.step(&action);
    }

    assert_eq!(game.dungeon.floor, 14);
    assert_eq!(game.screen, Screen::Event);
    assert_eq!(
        game.event.as_ref().map(|event| event.id.as_str()),
        Some("WeMeetAgain")
    );
}
