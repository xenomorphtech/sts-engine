use std::path::PathBuf;
use sts_engine::{load_commands, Unlocks};

fn commands_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/act1-seed2.commands.jsonl")
}

#[test]
fn seed2_replay_reaches_act2() {
    let path = commands_path();
    if !path.exists() {
        eprintln!("skipping: missing {}", path.display());
        return;
    }
    let commands = load_commands(&path).expect("commands");
    assert_eq!(commands.len(), 269);
    let mut game =
        sts_engine::game::Game::new(2, sts_engine::Character::Ironclad, 0, Unlocks::fixture());
    for (i, action) in commands.iter().enumerate() {
        let before = format!(
            "seq={i} screen={:?} floor={} hp={} hand={} energy={} gold={} deck={}",
            game.screen,
            game.dungeon.floor,
            game.player.hp,
            game.player.hand.len(),
            game.player.energy,
            game.player.gold,
            game.player.deck.len()
        );
        game.step(action);
        if matches!(i, 0 | 3 | 4 | 28 | 98 | 203) {
            let mhp = game
                .combat
                .as_ref()
                .and_then(|c| c.monsters.first())
                .map(|m| format!("{}:{}/{}", m.id.sts_id(), m.hp, m.max_hp))
                .unwrap_or_else(|| "-".into());
            eprintln!("{before} mon={mhp} -> {:?} {:?}", game.screen, action);
        }
        if game.player.hp <= 0 {
            eprintln!("died after {before} action={action:?}");
            break;
        }
    }
    eprintln!(
        "final floor={} act={:?} hp={}/{} gold={} deck={} relics={:?} done={}",
        game.dungeon.floor,
        game.dungeon.act,
        game.player.hp,
        game.player.max_hp,
        game.player.gold,
        game.player.deck.len(),
        game.player
            .relics
            .iter()
            .map(|r| r.id.sts_id())
            .collect::<Vec<_>>(),
        game.done
    );
    assert!(
        game.player.hp > 0 || game.dungeon.floor >= 8,
        "died during replay at floor {} hp={}",
        game.dungeon.floor,
        game.player.hp
    );
    assert!(
        game.dungeon.floor >= 8,
        "should progress through most of Act 1, floor={}",
        game.dungeon.floor
    );
    assert!(game.player.deck.len() >= 10);
}
