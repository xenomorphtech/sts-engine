use crate::action::Action;
use crate::game::Game;
use crate::ids::Character;
use crate::unlocks::Unlocks;
use flate2::read::GzDecoder;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

/// Stream a JSONL file. `*.gz` is decoded in memory (no temp file).
pub fn open_jsonl(path: impl AsRef<Path>) -> std::io::Result<Box<dyn BufRead>> {
    let path = path.as_ref();
    let file = File::open(path)?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if name.ends_with(".gz") {
        Ok(Box::new(BufReader::new(GzDecoder::new(file))))
    } else {
        Ok(Box::new(BufReader::new(file)))
    }
}

/// Prefer `path.gz` when the uncompressed file is missing.
pub fn resolve_jsonl(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    if path.exists() {
        return path.to_path_buf();
    }
    let gz = {
        let mut s = path.as_os_str().to_os_string();
        s.push(".gz");
        PathBuf::from(s)
    };
    if gz.exists() {
        gz
    } else {
        path.to_path_buf()
    }
}

pub fn load_commands(path: impl AsRef<Path>) -> std::io::Result<Vec<Action>> {
    let mut out = Vec::new();
    for line in open_jsonl(path)?.lines() {
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
