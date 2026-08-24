use std::path::PathBuf;
use sts_engine::game::Game;
use sts_engine::ids::Character;
use sts_engine::parity::{compare_generation, load_first_snapshot};
use sts_engine::Unlocks;

fn snapshot_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/act1-seed2-final-pruned.jsonl")
}

#[test]
fn seed2_dungeon_generation_matches_java() {
    let path = snapshot_path();
    if !path.exists() {
        eprintln!("skipping: missing {}", path.display());
        return;
    }
    let java = load_first_snapshot(&path).expect("snapshot");
    let game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    let report = compare_generation(&game, &java);
    if !report.ok() {
        for line in &report.mismatches {
            eprintln!("{line}");
        }
    }
    assert!(
        report.ok(),
        "{} generation mismatches vs Java seed 2",
        report.mismatches.len()
    );
}

#[test]
fn seed2_named_rng_initial_streams() {
    let game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    assert_eq!(game.rng.event.random.seed0, 4233148493373801447);
    assert_eq!(game.rng.event.counter, 0);
    assert_eq!(game.rng.relic.counter, 5);
    assert_eq!(game.rng.monster.counter, 37);
}

#[test]
fn seed2_floor1_starter_shuffle_matches_java() {
    use sts_engine::card::Card;
    use sts_engine::combat::{draw_cards_rng, Combat};
    use sts_engine::creature::Player;
    use sts_engine::ids::{CardId, EncounterId};
    use sts_engine::rng::RngSet;

    let mut rng = RngSet::generate_seeds(2);
    let mut player = Player::ironclad();
    assert_eq!(player.deck.len(), 10);
    let _ = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 1, 2, 0);
    let hand: Vec<_> = player.hand.iter().map(|c| c.sts_id()).collect();
    assert_eq!(
        hand,
        vec!["Defend_R", "Strike_R", "Strike_R", "Defend_R", "Defend_R"]
    );
    let draw: Vec<_> = player.draw.iter().map(|c| c.sts_id()).collect();
    assert_eq!(
        draw,
        vec!["Bash", "Defend_R", "Strike_R", "Strike_R", "Strike_R"]
    );
    let _ = (Card::new(CardId::Strike_R), draw_cards_rng);
}

#[test]
fn multiple_seeds_are_deterministic_and_distinct() {
    let a = Game::new(1, Character::Ironclad, 0, Unlocks::fixture());
    let a2 = Game::new(1, Character::Ironclad, 0, Unlocks::fixture());
    let b = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
    let c = Game::new(3, Character::Ironclad, 0, Unlocks::fixture());
    assert_eq!(a.dungeon.monster_list, a2.dungeon.monster_list);
    assert_eq!(a.dungeon.common_relics, a2.dungeon.common_relics);
    assert_eq!(a.rng.monster.snapshot(), a2.rng.monster.snapshot());
    assert_ne!(a.dungeon.monster_list, b.dungeon.monster_list);
    assert_ne!(b.dungeon.common_relics, c.dungeon.common_relics);
    assert_eq!(a.dungeon.boss, "Hexaghost");
    assert_eq!(b.dungeon.boss, "Hexaghost");
    assert_eq!(c.dungeon.boss, "Hexaghost");
}
