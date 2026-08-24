//! Orb-queue engineering: clever Defect lines the HTN turn planner cannot
//! find. Each test encodes the human-correct play and currently FAILS,
//! because the payoff crosses the one-turn search horizon or lives in a
//! blind spot of `orb_value` (target-blind, order-blind, evoke-timing-blind).
//!
//! These are capability targets, not regressions.

use sts_engine::action::Action;
use sts_engine::card::Card;
use sts_engine::combat::Combat;
use sts_engine::creature::{Orb, OrbKind};
use sts_engine::game::{Game, Screen};
use sts_engine::htn::HtnAgent;
use sts_engine::ids::{Act, CardId, Character, EncounterId, RoomType};
use sts_engine::Unlocks;

/// Run the agent's in-combat decisions for one player turn without stepping
/// EndTurn, so assertions can inspect what the planner chose to spend.
fn play_out_turn(game: &mut Game) -> Vec<Action> {
    let mut agent = HtnAgent::new();
    let mut played = Vec::new();
    for _ in 0..30 {
        if game.screen != Screen::Combat {
            break;
        }
        let action = agent.decide(game);
        if matches!(action, Action::EndTurn) {
            break;
        }
        game.step(&action);
        played.push(action);
    }
    println!("planner turn: {played:?}");
    println!(
        "orbs after turn: {:?}, monster hp: {:?}",
        game.player.orbs,
        game.combat
            .as_ref()
            .map(|combat| combat.monsters.iter().map(|m| m.hp).collect::<Vec<_>>())
    );
    played
}

fn quiet_intents(game: &mut Game) {
    for monster in &mut game.combat.as_mut().unwrap().monsters {
        monster.intent_damage = 0;
        monster.intent_hits = 0;
    }
}

fn dark_bank(game: &Game) -> i32 {
    game.player
        .orbs
        .iter()
        .filter(|orb| orb.kind == OrbKind::Dark)
        .map(|orb| orb.evoke)
        .max()
        .unwrap_or(0)
}

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

/// Turn 1 of a long boss fight, no incoming damage, a freshly channeled
/// Dark orb (6 stored) and a lone Dualcast. Popping the orb now deals 12
/// into a 200+ HP boss and destroys the bank; the human play is to end the
/// turn and let the orb ripen at +6/turn for a later, far larger evoke.
#[test]
#[ignore = "known HTN gap: orb-queue engineering (run with -- --ignored)"]
fn holds_a_young_dark_orb_instead_of_popping_it_into_a_boss() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(&mut game, EncounterId::Champ, Act::City, RoomType::Boss);
    quiet_intents(&mut game);
    game.player.energy = 1;
    game.player.hand = vec![Card::new(CardId::Dualcast)];
    game.player.orbs = vec![Orb {
        kind: OrbKind::Dark,
        evoke: 6,
    }];

    play_out_turn(&mut game);

    assert!(
        dark_bank(&game) >= 6,
        "planner popped the young Dark orb for 12 damage instead of ripening it"
    );
}

/// A ripened Dark bank (45 stored) with a 180 HP target and a 30 HP chaff
/// monster on the board. Dark evokes always hit the lowest-HP enemy, so a
/// Dualcast now dumps 90 stored damage into the 30 HP chaff. The human play
/// is to hold the evoke, clear the chaff with attacks over the next turns,
/// and only then release the bank into the real target.
#[test]
#[ignore = "known HTN gap: orb-queue engineering (run with -- --ignored)"]
fn does_not_dump_the_dark_bank_into_low_hp_chaff() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(
        &mut game,
        EncounterId::CultistAndChosen,
        Act::City,
        RoomType::Monster,
    );
    quiet_intents(&mut game);
    {
        let monsters = &mut game.combat.as_mut().unwrap().monsters;
        monsters[0].max_hp = 180;
        monsters[0].hp = 180;
        monsters[1].max_hp = 60;
        monsters[1].hp = 30;
    }
    game.player.energy = 3;
    game.player.hand = vec![Card::new(CardId::Dualcast)];
    game.player.orbs = vec![Orb {
        kind: OrbKind::Dark,
        evoke: 45,
    }];

    play_out_turn(&mut game);

    assert!(
        dark_bank(&game) >= 45,
        "planner dumped a 90-damage double evoke into a 30 HP chaff monster"
    );
}

/// Orb slots are full with the Dark bank in front. Playing Zap overflows the
/// queue and force-evokes the front orb: 40 stored damage lands on a 5 HP
/// chaff monster. The lightning orb is not worth evicting the bank; the
/// human play is to end the turn with Zap unplayed.
#[test]
#[ignore = "known HTN gap: orb-queue engineering (run with -- --ignored)"]
fn does_not_overflow_evict_the_dark_bank_with_zap() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(
        &mut game,
        EncounterId::CultistAndChosen,
        Act::City,
        RoomType::Monster,
    );
    quiet_intents(&mut game);
    {
        let monsters = &mut game.combat.as_mut().unwrap().monsters;
        monsters[0].max_hp = 200;
        monsters[0].hp = 200;
        monsters[1].max_hp = 60;
        monsters[1].hp = 5;
    }
    game.player.energy = 1;
    game.player.hand = vec![Card::new(CardId::Zap)];
    game.player.max_orbs = 3;
    game.player.orbs = vec![
        Orb {
            kind: OrbKind::Dark,
            evoke: 40,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
    ];

    play_out_turn(&mut game);

    assert!(
        dark_bank(&game) >= 40,
        "planner channeled Zap into full slots, wasting the 40-damage bank on 5 HP chaff"
    );
}

/// One empty orb slot, Dark bank in front, quiet enemy turn. Glacier's
/// second Frost overflows the queue and force-evokes the 30-damage bank
/// into a 6 HP chaff monster while the block is worthless. The human play
/// is to skip Glacier this turn and keep the queue intact.
#[test]
#[ignore = "known HTN gap: orb-queue engineering (run with -- --ignored)"]
fn declines_glacier_when_its_second_frost_would_evict_the_dark_bank() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(
        &mut game,
        EncounterId::CultistAndChosen,
        Act::City,
        RoomType::Monster,
    );
    quiet_intents(&mut game);
    {
        let monsters = &mut game.combat.as_mut().unwrap().monsters;
        monsters[0].max_hp = 200;
        monsters[0].hp = 200;
        monsters[1].max_hp = 60;
        monsters[1].hp = 6;
    }
    game.player.energy = 2;
    game.player.hand = vec![Card::new(CardId::Glacier)];
    game.player.max_orbs = 3;
    game.player.orbs = vec![
        Orb {
            kind: OrbKind::Dark,
            evoke: 30,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
    ];

    play_out_turn(&mut game);

    assert!(
        dark_bank(&game) >= 30,
        "planner played Glacier for dead block and overflowed the 30-damage bank into 6 HP chaff"
    );
}

/// Steering across turns: the 60 HP target should eat the 50-damage bank,
/// but the 30 HP tag-along is currently the lowest-HP enemy and would soak
/// it. The chaff cannot be killed this turn (Strike deals 6), so the human
/// line is to Strike the chaff, END TURN with Zap unplayed, finish the
/// chaff next turn, and only then trigger the overflow evoke into the real
/// target. The planner should spend the Strike but must not spend Zap.
#[test]
#[ignore = "known HTN gap: orb-queue engineering (run with -- --ignored)"]
fn steers_the_evoke_by_shaping_hp_before_releasing_the_bank() {
    let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
    start_combat(
        &mut game,
        EncounterId::CultistAndChosen,
        Act::City,
        RoomType::Monster,
    );
    quiet_intents(&mut game);
    {
        let monsters = &mut game.combat.as_mut().unwrap().monsters;
        monsters[0].max_hp = 60;
        monsters[0].hp = 60;
        monsters[1].max_hp = 60;
        monsters[1].hp = 30;
    }
    game.player.energy = 3;
    game.player.hand = vec![Card::new(CardId::Zap), Card::new(CardId::Strike_B)];
    game.player.max_orbs = 3;
    game.player.orbs = vec![
        Orb {
            kind: OrbKind::Dark,
            evoke: 50,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
    ];

    play_out_turn(&mut game);

    assert!(
        dark_bank(&game) >= 50,
        "planner released the 50-damage bank into the 30 HP soak instead of shaping HP first"
    );
}
