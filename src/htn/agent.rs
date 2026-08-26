use crate::action::{Action, PotionOp};
use crate::game::{CampfireOption, Game, GridKind, RewardKind, Screen, ShopChoice};
use crate::ids::{CardId, EventId, PotionId, RelicId};
use std::collections::VecDeque;

use super::{strategy, turnplan};
use turnplan::SearchStats;

/// Reactive HTN: re-decompose WinRun → CompleteAct at every decision.
#[derive(Clone, Debug, Default)]
pub struct HtnAgent {
    visited_shop_floors: Vec<i32>,
    recent: VecDeque<DecisionKey>,
    turn_plan: turnplan::TurnPlanMemory,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DecisionKey {
    screen: Screen,
    floor: i32,
    command: CommandIdentity,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CommandIdentity {
    Action(Action),
    Choice(ChoiceIdentity),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RewardIdentity {
    Gold(i32),
    StolenGold(i32),
    Potion(PotionId),
    Relic(RelicId),
    Card,
    EmeraldKey,
    SapphireKey,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ShopIdentity {
    Purge,
    Card(CardId),
    Relic(RelicId),
    Potion(PotionId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ChoiceIdentity {
    CombatReward {
        reward_index: usize,
        reward: RewardIdentity,
    },
    CardReward {
        choice_index: usize,
        card: CardId,
    },
    BossRelic {
        choice_index: usize,
        relic: RelicId,
    },
    Shop {
        choice_index: usize,
        item: ShopIdentity,
    },
    Campfire(CampfireOption),
    Smith {
        choice_index: usize,
        card: CardId,
    },
    Event {
        event: EventId,
        screen: i32,
        choice_index: usize,
        option: crate::game::EventOption,
    },
    Neow {
        screen: i32,
        index: usize,
    },
    Hand {
        choice_index: usize,
        card: CardId,
    },
    Grid {
        kind: GridKind,
        choice_index: usize,
        card: Option<CardId>,
    },
}

impl DecisionKey {
    fn new(game: &Game, action: &Action) -> Self {
        let command = choice_identity(game, action)
            .map(CommandIdentity::Choice)
            .unwrap_or_else(|| CommandIdentity::Action(action.clone()));
        Self {
            screen: game.screen,
            floor: game.dungeon.floor,
            command,
        }
    }
}

fn reward_identity(kind: &RewardKind) -> RewardIdentity {
    match kind {
        RewardKind::Gold(amount) => RewardIdentity::Gold(*amount),
        RewardKind::StolenGold(amount) => RewardIdentity::StolenGold(*amount),
        RewardKind::Potion(id) => RewardIdentity::Potion(*id),
        RewardKind::Relic(id) => RewardIdentity::Relic(*id),
        RewardKind::Card => RewardIdentity::Card,
        RewardKind::EmeraldKey => RewardIdentity::EmeraldKey,
        RewardKind::SapphireKey => RewardIdentity::SapphireKey,
    }
}

fn choice_identity(game: &Game, action: &Action) -> Option<ChoiceIdentity> {
    let Action::Choose { index, .. } = action else {
        return None;
    };
    match game.screen {
        Screen::CombatReward => game
            .rewards
            .iter()
            .enumerate()
            .filter(|(_, reward)| !reward.taken)
            .nth(*index)
            .map(|(reward_index, reward)| ChoiceIdentity::CombatReward {
                reward_index,
                reward: reward_identity(&reward.kind),
            }),
        Screen::CardReward => game
            .card_reward
            .get(*index)
            .map(|card| ChoiceIdentity::CardReward {
                choice_index: *index,
                card: card.id,
            }),
        Screen::BossRelic => {
            game.boss_relics
                .get(*index)
                .copied()
                .map(|relic| ChoiceIdentity::BossRelic {
                    choice_index: *index,
                    relic,
                })
        }
        Screen::Shop => game
            .shop_choices()
            .get(*index)
            .map(|choice| ChoiceIdentity::Shop {
                choice_index: *index,
                item: match choice {
                    ShopChoice::Purge => ShopIdentity::Purge,
                    ShopChoice::Card(card) => ShopIdentity::Card(card.id),
                    ShopChoice::Relic(id) => ShopIdentity::Relic(*id),
                    ShopChoice::Potion(id) => ShopIdentity::Potion(*id),
                },
            }),
        Screen::Rest if game.rest_is_smithing() => game
            .player
            .deck
            .iter()
            .filter(|card| card.can_upgrade())
            .nth(*index)
            .map(|card| ChoiceIdentity::Smith {
                choice_index: *index,
                card: card.id,
            }),
        Screen::Rest => game
            .campfire_options()
            .get(*index)
            .copied()
            .map(ChoiceIdentity::Campfire),
        Screen::Event => game.event.as_ref().and_then(|event| {
            event
                .options
                .get(*index)
                .copied()
                .map(|option| ChoiceIdentity::Event {
                    event: event.id,
                    screen: event.screen,
                    choice_index: *index,
                    option,
                })
        }),
        Screen::Neow => Some(ChoiceIdentity::Neow {
            screen: game.neow_screen,
            index: *index,
        }),
        Screen::HandSelect => game
            .player
            .hand
            .get(*index)
            .map(|card| ChoiceIdentity::Hand {
                choice_index: *index,
                card: card.id,
            }),
        Screen::Grid => game.grid_view().map(|(kind, cards)| ChoiceIdentity::Grid {
            kind,
            choice_index: *index,
            card: cards
                .into_iter()
                .find_map(|(choice_index, card)| (choice_index == *index).then_some(card.id)),
        }),
        _ => None,
    }
}

impl HtnAgent {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn decide(&mut self, game: &Game) -> Action {
        self.decide_with_stats(game).0
    }

    pub fn decide_with_stats(&mut self, game: &Game) -> (Action, SearchStats) {
        let mut stats = SearchStats::default();
        let legal = game.legal_actions();
        if legal.is_empty() {
            return (Action::Quit, stats);
        }
        if game.done || game.screen == Screen::Terminal || game.player.hp <= 0 {
            return (Action::Quit, stats);
        }
        let cmd = self.method(game, &legal, &mut stats).unwrap_or_else(|| {
            legal
                .iter()
                .find(|a| !matches!(a, Action::Potion { .. }))
                .cloned()
                .unwrap_or_else(|| legal[0].clone())
        });
        (self.anti_stall(game, cmd, &legal), stats)
    }

    fn method(
        &mut self,
        game: &Game,
        legal: &[Action],
        stats: &mut SearchStats,
    ) -> Option<Action> {
        if game.player.hp < (game.player.max_hp as f32 * 0.45) as i32 {
            if let Some(heal) = find_potion(game, legal, &[PotionId::Blood, PotionId::FruitJuice]) {
                return Some(heal);
            }
        }
        match game.screen {
            Screen::Combat => {
                let (action, turn_stats) =
                    turnplan::plan_turn_with_memory(game, legal, &mut self.turn_plan);
                *stats += turn_stats;
                Some(action)
            }
            Screen::Map => {
                let nodes: Vec<Action> = legal
                    .iter()
                    .filter(|a| {
                        matches!(a, Action::Choose { x: Some(_), y: Some(_), .. })
                    })
                    .cloned()
                    .collect();
                if nodes.is_empty() {
                    return legal.iter().find(|a| !matches!(a, Action::Potion { .. })).cloned();
                }
                Some(strategy::map_choice(game, &nodes))
            }
            Screen::CardReward => Some(strategy::card_reward(game, legal)),
            Screen::CombatReward => Some(strategy::combat_reward(game, legal)),
            Screen::BossRelic => Some(strategy::boss_relic(game, legal)),
            Screen::Shop => Some(self.enter_shop(game, legal)),
            Screen::Rest => Some(strategy::rest_choice(game, legal)),
            Screen::Treasure => legal.iter().find(|a| matches!(a, Action::Choose { .. })).cloned(),
            Screen::Event => Some(strategy::event_choice(game, legal)),
            Screen::Neow => Some(strategy::neow_choice(game, legal)),
            Screen::HandSelect => Some(strategy::hand_select(game, legal)),
            Screen::Grid => Some(strategy::grid_choice(game, legal)),
            Screen::DoorUnlock | Screen::ActTransition => Some(Action::Proceed),
            Screen::Terminal => Some(Action::Quit),
        }
    }

    fn enter_shop(&mut self, game: &Game, legal: &[Action]) -> Action {
        let floor = game.dungeon.floor;
        if !game.shop_is_open() {
            let shop = legal.iter().find(|a| matches!(a, Action::Choose { .. }));
            if let Some(shop) = shop {
                if !self.visited_shop_floors.contains(&floor) {
                    self.visited_shop_floors.push(floor);
                    return shop.clone();
                }
            }
        }
        strategy::shop_choice(game, legal)
    }

    fn grid_choice(&self, game: &Game, legal: &[Action]) -> Option<Action> {
        legal
            .iter()
            .find(|action| matches!(action, Action::Proceed))
            .cloned()
            .or_else(|| {
                legal
                    .iter()
                    .filter(|action| matches!(action, Action::Choose { .. }))
                    .min_by_key(|action| {
                        let key = DecisionKey::new(game, action);
                        self.recent.iter().filter(|prior| **prior == key).count()
                    })
                    .cloned()
            })
            .or_else(|| legal.first().cloned())
    }

    fn anti_stall(&mut self, game: &Game, cmd: Action, legal: &[Action]) -> Action {
        let key = DecisionKey::new(game, &cmd);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::Character;
    use crate::Unlocks;

    fn choose(index: usize) -> Action {
        Action::Choose {
            index,
            x: None,
            y: None,
            room: None,
        }
    }

    #[test]
    fn multi_pick_grid_rotates_away_from_prior_choice() {
        let mut game = Game::new(642, Character::Defect, 0, Unlocks::fixture());
        game.screen = Screen::Grid;
        game.dungeon.floor = 12;
        let legal = vec![choose(0), choose(1), choose(2)];
        let mut agent = HtnAgent::new();

        let first = agent.grid_choice(&game, &legal).unwrap();
        assert_eq!(first, legal[0]);
        agent.recent.push_back(DecisionKey::new(&game, &first));

        assert_eq!(agent.grid_choice(&game, &legal), Some(legal[1].clone()));
    }

    #[test]
    fn duplicate_cards_on_a_multi_pick_grid_have_distinct_identities() {
        let first = ChoiceIdentity::Grid {
            kind: GridKind::Transform,
            choice_index: 0,
            card: Some(CardId::Strike_B),
        };
        let second = ChoiceIdentity::Grid {
            kind: GridKind::Transform,
            choice_index: 1,
            card: Some(CardId::Strike_B),
        };

        assert_ne!(first, second);
    }

    #[test]
    fn compact_reward_indices_do_not_look_like_a_stall() {
        use crate::ids::RoomType;

        let mut game = Game::new(12, Character::Defect, 0, Unlocks::fixture());
        let prefix = [
            choose(0),
            choose(0),
            choose(1),
            choose(0),
            Action::Choose {
                index: 1,
                x: Some(2),
                y: Some(0),
                room: Some(RoomType::Monster),
            },
            Action::Play {
                hand_index: 2,
                target_index: None,
            },
            Action::Play {
                hand_index: 2,
                target_index: Some(0),
            },
            Action::Play {
                hand_index: 2,
                target_index: Some(1),
            },
        ];
        for action in prefix {
            assert!(
                game.legal_actions().contains(&action),
                "illegal replay action {action:?}"
            );
            game.step(&action);
        }

        let mut agent = HtnAgent::new();
        for expected_reward in [
            RewardIdentity::Gold(11),
            RewardIdentity::Potion(PotionId::Colorless),
        ] {
            let action = agent.decide(&game);
            let key = DecisionKey::new(&game, &action);
            assert!(matches!(
                key.command,
                CommandIdentity::Choice(ChoiceIdentity::CombatReward { reward, .. })
                    if reward == expected_reward
            ));
            game.step(&action);
        }

        let card = agent.decide(&game);
        assert!(matches!(
            DecisionKey::new(&game, &card).command,
            CommandIdentity::Choice(ChoiceIdentity::CombatReward {
                reward: RewardIdentity::Card,
                ..
            })
        ));
    }

    #[test]
    fn emergency_heal_selects_the_requested_potion_slot() {
        use crate::creature::PotionInstance;

        let mut game = Game::new(12, Character::Defect, 0, Unlocks::fixture());
        game.player.potions = vec![
            PotionInstance { id: PotionId::Duplication, slot: 0 },
            PotionInstance { id: PotionId::Blood, slot: 1 },
        ].into();
        let legal = vec![
            Action::Potion { action: PotionOp::Use, slot: 0, target_index: None },
            Action::Potion { action: PotionOp::Use, slot: 1, target_index: None },
        ];

        assert_eq!(
            find_potion(&game, &legal, &[PotionId::Blood, PotionId::FruitJuice]),
            Some(legal[1].clone())
        );
        assert_eq!(find_potion(&game, &legal, &[PotionId::FruitJuice]), None);
    }
}

fn find_potion(game: &Game, legal: &[Action], want: &[PotionId]) -> Option<Action> {
    legal.iter().find(|action| {
        let Action::Potion { action: PotionOp::Use, slot, .. } = action else {
            return false;
        };
        game.player.potions.get(*slot).is_some_and(|potion| want.contains(&potion.id))
    }).cloned()
}
