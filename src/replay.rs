use crate::action::Action;
use crate::game::Game;
use crate::ids::Character;
use crate::unlocks::Unlocks;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

pub fn load_commands(path: impl AsRef<Path>) -> std::io::Result<Vec<Action>> {
    let file = File::open(path)?;
    let mut out = Vec::new();
    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let action: Action = serde_json::from_str(&line)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        out.push(action);
    }
    Ok(out)
}

pub fn replay_seed(seed: i64, commands: &[Action], unlocks: Unlocks) -> Game {
    let mut game = Game::new(seed, Character::Ironclad, 0, unlocks);
    for action in commands {
        if matches!(action, crate::action::Action::Quit) {
            break;
        }
        game.step(action);
        if game.done {
            break;
        }
    }
    game
}
