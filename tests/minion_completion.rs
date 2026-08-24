use sts_engine::combat::{spawn_monster, Combat};
use sts_engine::game::Game;
use sts_engine::ids::{Character, EncounterId, MonsterId};
use sts_engine::Unlocks;

fn combat(encounter: EncounterId) -> (Game, Combat) {
    let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
    let combat = Combat::start(encounter, &mut game.player, &mut game.rng, 31, 2, 0);
    (game, combat)
}

fn kill(combat: &mut Combat, id: MonsterId) {
    let monster = combat.monsters.iter_mut().find(|m| m.id == id).unwrap();
    monster.hp = 0;
    monster.dead = true;
}

#[test]
fn collector_completes_with_living_torch_heads() {
    let (mut game, mut combat) = combat(EncounterId::Collector);
    combat
        .monsters
        .push(spawn_monster(MonsterId::TorchHead, &mut game.rng, 0));

    assert!(!combat.all_dead());
    kill(&mut combat, MonsterId::TheCollector);
    assert!(combat.all_dead());
}

#[test]
fn automaton_completes_with_living_bronze_orbs() {
    let (mut game, mut combat) = combat(EncounterId::Automaton);
    combat
        .monsters
        .push(spawn_monster(MonsterId::BronzeOrb, &mut game.rng, 0));

    assert!(!combat.all_dead());
    kill(&mut combat, MonsterId::BronzeAutomaton);
    assert!(combat.all_dead());
}

#[test]
fn gremlin_leader_completes_with_living_summons() {
    let (_, mut combat) = combat(EncounterId::GremlinLeader);

    assert!(!combat.all_dead());
    kill(&mut combat, MonsterId::GremlinLeader);
    assert!(combat.all_dead());
}

#[test]
fn ordinary_living_monsters_still_keep_combat_open() {
    let (_, mut combat) = combat(EncounterId::CenturionAndHealer);

    kill(&mut combat, MonsterId::Centurion);
    assert!(!combat.all_dead());
}
