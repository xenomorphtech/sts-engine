use crate::action::{Action, PotionOp};
use crate::game::{Game, Screen};
use crate::ids::PotionId;
use std::collections::VecDeque;

use super::{strategy, turnplan};

/// Reactive HTN: re-decompose WinRun → CompleteAct at every decision.
#[derive(Clone, Debug, Default)]
pub struct HtnAgent {
    visited_shop_floors: Vec<i32>,
    recent: VecDeque<(Screen, i32, Action)>,
}

impl HtnAgent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decide(&mut self, game: &Game) -> Action {
        let legal = game.legal_actions();
        if legal.is_empty() {
            return Action::Quit;
        }
        if game.done || game.screen == Screen::Terminal || game.player.hp <= 0 {
            return Action::Quit;
        }
        let cmd = self.method(game, &legal).unwrap_or_else(|| {
            legal
                .iter()
                .find(|a| !matches!(a, Action::Potion { .. }))
                .cloned()
                .unwrap_or_else(|| legal[0].clone())
        });
        self.anti_stall(game, cmd, &legal)
    }

    fn method(&mut self, game: &Game, legal: &[Action]) -> Option<Action> {
        if game.player.hp < (game.player.max_hp as f32 * 0.45) as i32 {
            if let Some(heal) = find_potion(legal, &[PotionId::Blood, PotionId::FruitJuice]) {
                return Some(heal);
            }
        }
        match game.screen {
            Screen::Combat => Some(turnplan::plan_turn(game, legal)),
            Screen::Map => {
                let nodes: Vec<Action> = legal
                    .iter()
                    .filter(|a| matches!(a, Action::Choose { label: Some(l), .. } if l == "map node" || l == "boss"))
                    .cloned()
                    .collect();
                if nodes.is_empty() {
                    return legal
                        .iter()
                        .find(|a| !matches!(a, Action::Potion { .. }))
                        .cloned();
                }
                Some(strategy::map_choice(game, &nodes))
            }
            Screen::CardReward => Some(strategy::card_reward(game, legal)),
            Screen::CombatReward => Some(strategy::combat_reward(game, legal)),
            Screen::BossRelic => Some(strategy::boss_relic(game, legal)),
            Screen::Shop => Some(self.enter_shop(game, legal)),
            Screen::Rest => Some(strategy::rest_choice(game, legal)),
            Screen::Treasure => legal
                .iter()
                .find(|a| matches!(a, Action::Choose { .. }))
                .cloned(),
            Screen::Event | Screen::Neow => Some(strategy::event_choice(game, legal)),
            Screen::HandSelect => Some(strategy::hand_select(game, legal)),
            Screen::Grid => legal
                .iter()
                .find(|a| matches!(a, Action::Proceed))
                .cloned()
                .or_else(|| {
                    legal
                        .iter()
                        .find(|a| matches!(a, Action::Choose { .. }))
                        .cloned()
                }),
            Screen::ActTransition => Some(Action::Proceed),
            Screen::Terminal => Some(Action::Quit),
        }
    }

    fn enter_shop(&mut self, game: &Game, legal: &[Action]) -> Action {
        let floor = game.dungeon.floor;
        if let Some(shop) = legal.iter().find(|a| {
            matches!(a, Action::Choose { label: Some(l), .. } if l.eq_ignore_ascii_case("shop"))
        }) {
            if !self.visited_shop_floors.contains(&floor) {
                self.visited_shop_floors.push(floor);
                return shop.clone();
            }
        }
        strategy::shop_choice(game, legal)
    }

    fn anti_stall(&mut self, game: &Game, cmd: Action, legal: &[Action]) -> Action {
        let key = (game.screen, game.dungeon.floor, cmd.clone());
        self.recent.push_back(key.clone());
        if self.recent.len() > 12 {
            self.recent.pop_front();
        }
        let repeats = self.recent.iter().filter(|k| *k == &key).count();
        if repeats >= 3 && matches!(cmd, Action::Choose { .. } | Action::Skip | Action::Proceed) {
            if let Some(p) = legal.iter().find(|a| matches!(a, Action::Proceed)) {
                if !matches!(cmd, Action::Proceed) {
                    return p.clone();
                }
            }
            if let Some(s) = legal.iter().find(|a| matches!(a, Action::Skip)) {
                if !matches!(cmd, Action::Skip) {
                    return s.clone();
                }
            }
        }
        cmd
    }
}

fn find_potion(legal: &[Action], want: &[PotionId]) -> Option<Action> {
    legal
        .iter()
        .find(|a| match a {
            Action::Potion {
                action: PotionOp::Use,
                ..
            } => true,
            _ => false,
        })
        .filter(|_| !want.is_empty())
        .cloned()
}
