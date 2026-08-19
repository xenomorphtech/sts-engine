use crate::action::Action;
use crate::game::{Game, Screen};
use crate::ids::Character;
use crate::unlocks::Unlocks;

/// Gym-style wrapper for training loops. `step` takes an index into `legal_actions()`.
#[derive(Clone, Debug)]
pub struct TrainEnv {
    pub game: Game,
}

#[derive(Clone, Debug)]
pub struct StepInfo {
    pub reward: f32,
    pub done: bool,
    pub legal: Vec<Action>,
}

impl TrainEnv {
    pub fn new(seed: i64) -> Self {
        Self {
            game: Game::new(seed, Character::Ironclad, 0, Unlocks::fixture()),
        }
    }

    pub fn new_character(seed: i64, character: Character) -> Self {
        Self {
            game: Game::new(seed, character, 0, Unlocks::fixture()),
        }
    }

    pub fn reset(&mut self, seed: i64) -> Vec<Action> {
        self.game = Game::new(seed, Character::Ironclad, 0, Unlocks::fixture());
        self.game.legal_actions()
    }

    pub fn step(&mut self, action_index: usize) -> StepInfo {
        let legal = self.game.legal_actions();
        let hp_before = self.game.player.hp;
        if let Some(action) = legal.get(action_index) {
            self.game.step(action);
        }
        let done = self.game.done || self.game.player.hp <= 0 || self.game.screen == Screen::Terminal;
        let reward = if self.game.player.hp <= 0 {
            -1.0
        } else if self.game.dungeon.act as i32 >= 2 {
            1.0
        } else {
            (self.game.player.hp - hp_before) as f32 * 0.01
        };
        StepInfo {
            reward,
            done,
            legal: self.game.legal_actions(),
        }
    }

    pub fn compact_obs(&self) -> Vec<f32> {
        vec![
            self.game.player.hp as f32,
            self.game.player.max_hp as f32,
            self.game.player.gold as f32,
            self.game.player.energy as f32,
            self.game.dungeon.floor as f32,
            self.game.dungeon.act as i32 as f32,
            self.game.player.deck.len() as f32,
            self.game.player.hand.len() as f32,
        ]
    }
}
