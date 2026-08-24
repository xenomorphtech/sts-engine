use crate::action::{Action, PotionOp};
use crate::card::Card;
use crate::creature::OrbKind;
use crate::game::{CampfireOption, EventOption, Game, GridKind, RewardKind, Screen, ShopChoice};
use crate::ids::{CardId, CardType, Character, EncounterId, EventId, PotionId, RelicId, RoomType};

use super::deckplan;
use super::params::params;

/// The current run objective terminates after The Beyond; `begin_next_act`
/// does not enter The Ending even with all three keys. Keep all key costs out
/// of policy decisions until that engine transition is enabled.
fn keys_advance_win_condition(_game: &Game) -> bool {
    false
}

#[derive(Clone, Copy, Debug, Default)]
struct DeckMetrics {
    size: i32,
    attacks: i32,
    strikes: i32,
    aoe: i32,
    block_cards: i32,
    scaling: i32,
    big_attacks: i32,
    channel: i32,
    frost_src: i32,
}

pub fn map_choice(game: &Game, actions: &[Action]) -> Action {
    let hp_frac = game.player.hp as f32 / game.player.max_hp.max(1) as f32;
    let act = game.dungeon.act as i32;
    let gold = game.player.gold;
    let metrics = deck_metrics(game);
    let strength = metrics.scaling * 2
        + game.player.deck.iter().filter(|c| c.upgraded).count() as i32
        + metrics.big_attacks;
    let need_emerald =
        keys_advance_win_condition(game) && game.final_act_available() && !game.has_emerald_key();

    let mut best = &actions[0];
    let mut best_v = f32::MIN;
    for a in actions {
        let v = action_value(game, a, hp_frac, act, gold, strength, need_emerald);
        if v > best_v {
            best_v = v;
            best = a;
        }
    }
    best.clone()
}

fn action_value(
    game: &Game,
    action: &Action,
    hp_frac: f32,
    act: i32,
    gold: i32,
    strength: i32,
    need_emerald: bool,
) -> f32 {
    let (x, y, room) = match action {
        Action::Choose {
            x: Some(x),
            y: Some(y),
            room: Some(r),
            ..
        } => (*x, *y, *r),
        _ => return 0.0,
    };
    node_value(
        game,
        x,
        y,
        room,
        hp_frac,
        act,
        gold,
        strength,
        need_emerald,
        0,
    )
}

fn node_value(
    game: &Game,
    x: i32,
    y: i32,
    room: RoomType,
    hp_frac: f32,
    act: i32,
    gold: i32,
    strength: i32,
    need_emerald: bool,
    elites_seen: usize,
) -> f32 {
    let own = room_value(
        game,
        x,
        y,
        room,
        hp_frac,
        act,
        gold,
        strength,
        need_emerald,
        queued_elite(game, elites_seen),
    );
    let projected_hp = projected_hp_after_room(game, room, hp_frac, act, y, strength);
    let survival = if projected_hp <= 0.0 {
        -2_000.0 + projected_hp * 500.0
    } else if projected_hp < 0.15 {
        -250.0 * (0.15 - projected_hp) / 0.15
    } else {
        0.0
    };
    if projected_hp <= 0.0 {
        return own + survival;
    }
    let child_elites_seen = elites_seen + usize::from(room == RoomType::Elite);
    let children = lookup_node(game, x, y)
        .map(|n| {
            n.edges
                .iter()
                .map(|e| {
                    let r = lookup_node(game, e.dst_x, e.dst_y)
                        .and_then(|d| d.room)
                        .unwrap_or(RoomType::Monster);
                    node_value(
                        game,
                        e.dst_x,
                        e.dst_y,
                        r,
                        projected_hp,
                        act,
                        gold,
                        strength,
                        need_emerald,
                        child_elites_seen,
                    )
                })
                .fold(None, |acc: Option<f32>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                })
        })
        .flatten();
    own + survival + children.unwrap_or(0.0)
}

fn queued_elite(game: &Game, elites_seen: usize) -> Option<EncounterId> {
    game.dungeon.elite_list.get(elites_seen).copied()
}

/// Carry an HP budget through map lookahead. The map is known, but exact
/// encounters and draws are not, so use conservative act/room priors adjusted
/// by the current deck. This makes a chain of future combats consume the same
/// resource that a future rest restores instead of evaluating every node at
/// today's HP.
fn projected_hp_after_room(
    game: &Game,
    room: RoomType,
    hp_frac: f32,
    act: i32,
    y: i32,
    strength: i32,
) -> f32 {
    match room {
        RoomType::Monster | RoomType::Elite | RoomType::Boss => {
            let metrics = deck_metrics(game);
            let base = match (act, room) {
                (1, RoomType::Monster) => 0.12,
                (1, RoomType::Elite) => 0.30,
                (1, RoomType::Boss) => 0.42,
                (2, RoomType::Monster) => 0.20,
                (2, RoomType::Elite) => 0.36,
                (2, RoomType::Boss) => 0.48,
                (3, RoomType::Monster) => 0.18,
                (3, RoomType::Elite) => 0.32,
                (3, RoomType::Boss) => 0.50,
                (_, RoomType::Monster) => 0.20,
                (_, RoomType::Elite) => 0.36,
                (_, RoomType::Boss) => 0.55,
                _ => 0.0,
            };
            let defense = (metrics.block_cards + metrics.frost_src / 2).clamp(0, 10) as f32;
            let offense = strength.clamp(0, 12) as f32;
            let mitigation = 0.006 * defense + 0.004 * offense;
            let loss = (base - mitigation).max(base * 0.58);
            hp_frac - loss
        }
        RoomType::Rest => {
            let p = params();
            let threshold = if act == 1 {
                p.rest_hp_act1
            } else {
                p.rest_hp_later
            };
            let threshold = if y >= 13 {
                threshold.max(p.rest_hp_preboss)
            } else {
                threshold
            };
            let can_rest = !game.player.has_relic(RelicId::Coffee_Dripper);
            if can_rest && hp_frac < threshold {
                (hp_frac + 0.30).min(1.0)
            } else {
                hp_frac
            }
        }
        RoomType::Shop if game.player.has_relic(RelicId::MealTicket) => {
            (hp_frac + 15.0 / game.player.max_hp.max(1) as f32).min(1.0)
        }
        _ => hp_frac,
    }
}

fn lookup_node(game: &Game, x: i32, y: i32) -> Option<&crate::map::MapNode> {
    game.dungeon
        .map
        .nodes
        .get(y as usize)
        .and_then(|row| row.iter().find(|n| n.x == x))
}

fn room_value(
    game: &Game,
    x: i32,
    y: i32,
    room: RoomType,
    hp_frac: f32,
    act: i32,
    gold: i32,
    strength: i32,
    need_emerald: bool,
    next_elite: Option<EncounterId>,
) -> f32 {
    match room {
        RoomType::Elite => {
            let p = params();
            let metrics = deck_metrics(game);
            let afford = hp_frac >= p.elite_afford_hp + game.ascension as f32 / 200.0
                && strength as f32
                    >= p.elite_strength_base
                        + p.elite_strength_slope * act as f32
                        + if game.ascension >= 15 { 2.0 } else { 0.0 };
            let matchup = elite_matchup(next_elite, metrics, strength);
            let mut v = if afford {
                p.elite_value + matchup as f32
            } else {
                p.elite_penalty + matchup.min(0) as f32
            };
            if hp_frac < p.elite_hp_floor {
                v = -400.0;
            }
            if need_emerald && lookup_node(game, x, y).is_some_and(|n| n.emerald_key) {
                v = if act == 1 {
                    if afford {
                        25.0
                    } else {
                        -150.0
                    }
                } else if hp_frac < 0.5 {
                    -120.0
                } else if afford || act >= 3 {
                    70.0
                } else {
                    -40.0
                };
            }
            v
        }
        RoomType::Rest => {
            let p = params();
            let mut v = if hp_frac < 0.65 {
                p.rest_low_value
            } else {
                p.rest_high_value
            };
            if y >= 14 {
                v = p.rest_preboss_value;
            }
            if keys_advance_win_condition(game)
                && game.final_act_available()
                && !game.has_ruby_key()
                && act >= 3
            {
                v += 25.0;
            }
            v
        }
        RoomType::Shop => {
            if gold >= 120 {
                (gold as f32 / params().shop_gold_div).min(60.0)
            } else if gold >= 75 {
                8.0
            } else {
                -5.0
            }
        }
        RoomType::Treasure => params().treasure_value,
        RoomType::Event => params().event_value,
        RoomType::Monster => {
            let shift = 0.05 * (act - 1) as f32;
            if hp_frac >= 0.6 + shift {
                params().monster_ok_value
            } else if hp_frac >= 0.4 + shift {
                params().monster_low_value
            } else if hp_frac >= 0.35 + shift {
                -45.0
            } else {
                -110.0
            }
        }
        RoomType::Boss => 5.0,
        _ => 0.0,
    }
}

fn elite_matchup(next_elite: Option<EncounterId>, metrics: DeckMetrics, strength: i32) -> i32 {
    match next_elite {
        Some(EncounterId::GremlinNob) => {
            if metrics.big_attacks >= 2 {
                15
            } else {
                -60
            }
        }
        Some(EncounterId::ThreeSentries) => {
            if metrics.aoe >= 2 {
                20
            } else if metrics.aoe == 0 {
                -40
            } else {
                0
            }
        }
        Some(EncounterId::Lagavulin) => {
            if metrics.big_attacks >= 1 || strength >= 6 {
                15
            } else {
                -35
            }
        }
        Some(EncounterId::BookOfStabbing) => {
            if metrics.block_cards + metrics.frost_src * 2 >= 6 {
                15
            } else {
                -35
            }
        }
        Some(EncounterId::GremlinLeader) => {
            if metrics.aoe >= 2 {
                15
            } else {
                -30
            }
        }
        Some(EncounterId::Reptomancer) => {
            if metrics.aoe >= 2 {
                10
            } else {
                -25
            }
        }
        Some(EncounterId::Slavers) => {
            if metrics.aoe >= 1 {
                10
            } else {
                -20
            }
        }
        Some(EncounterId::GiantHead) => {
            if metrics.scaling >= 2 || metrics.big_attacks >= 2 {
                10
            } else {
                -50
            }
        }
        Some(EncounterId::Nemesis) => {
            if metrics.big_attacks >= 2 {
                10
            } else {
                -25
            }
        }
        _ => 0,
    }
}

pub fn combat_reward(game: &Game, legal: &[Action]) -> Action {
    let empty_pots = game
        .player
        .potions
        .iter()
        .filter(|p| p.id == crate::ids::PotionId::Slot)
        .count();
    let untaken: Vec<_> = game.rewards.iter().filter(|reward| !reward.taken).collect();
    for a in legal {
        if let Action::Choose { index, .. } = a {
            let Some(reward) = untaken.get(*index) else {
                continue;
            };
            match reward.kind {
                RewardKind::EmeraldKey => {
                    if keys_advance_win_condition(game) && !game.has_emerald_key() {
                        return a.clone();
                    }
                }
                RewardKind::Gold(_) | RewardKind::StolenGold(_) | RewardKind::Relic(_) => {
                    return a.clone()
                }
                RewardKind::SapphireKey => {
                    if keys_advance_win_condition(game)
                        && game.final_act_available()
                        && !game.has_sapphire_key()
                        && game.dungeon.act as i32 >= 2
                    {
                        return a.clone();
                    }
                }
                RewardKind::Potion(id) => {
                    if empty_pots > 0 {
                        return a.clone();
                    }
                    if let Some(slot) = potion_swap_slot(game, id) {
                        if let Some(discard) = potion_discard_action(legal, slot) {
                            return discard;
                        }
                    }
                }
                RewardKind::Card => {
                    let best = game
                        .card_reward
                        .iter()
                        .map(|c| score_card(game, c))
                        .max()
                        .unwrap_or(0);
                    if best as f32 >= params().pick_threshold {
                        return a.clone();
                    }
                }
            }
        }
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Proceed))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
}

pub fn card_reward(game: &Game, legal: &[Action]) -> Action {
    let mut best: Option<(&Action, i32)> = None;
    for a in legal {
        if let Action::Choose { index, .. } = a {
            if let Some(card) = game.card_reward.get(*index) {
                let s = score_card(game, card);
                if best.map_or(true, |(_, b)| s > b) {
                    best = Some((a, s));
                }
            }
        }
    }
    if let Some((a, s)) = best {
        if s as f32 >= params().pick_threshold {
            return a.clone();
        }
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Skip))
        .cloned()
        .or_else(|| best.map(|(a, _)| a.clone()))
        .unwrap_or_else(|| legal[0].clone())
}

pub fn boss_relic(game: &Game, legal: &[Action]) -> Action {
    let mut best = None;
    let mut best_r = -1;
    for a in legal {
        if let Action::Choose { index, .. } = a {
            let Some(id) = game.boss_relics.get(*index).copied() else {
                continue;
            };
            let r = boss_relic_score(game, id);
            if r > best_r {
                best_r = r;
                best = Some(a);
            }
        }
    }
    if best_r < 20 {
        if let Some(p) = legal
            .iter()
            .find(|a| matches!(a, Action::Proceed | Action::Skip))
        {
            return p.clone();
        }
    }
    best.cloned().unwrap_or_else(|| legal[0].clone())
}

pub fn shop_choice(game: &Game, legal: &[Action]) -> Action {
    let empty_potions = game.player.potions.iter().any(|p| p.id == PotionId::Slot);
    let choices = game.shop_choices();
    let mut best: Option<(&Action, i32, Option<usize>)> = None;
    for action in legal {
        let Action::Choose { index, .. } = action else {
            continue;
        };
        let Some(choice) = choices.get(*index) else {
            continue;
        };
        let (value, swap_slot) = match choice {
            ShopChoice::Purge => (deckplan::shop_purge_value(game), None),
            ShopChoice::Card(card) => (score_card(game, card) - 10, None),
            ShopChoice::Relic(id) => (deckplan::shop_relic_value(game, *id), None),
            ShopChoice::Potion(id) if empty_potions => (shop_potion_value(*id), None),
            ShopChoice::Potion(id) => potion_swap_slot(game, *id)
                .map(|slot| (shop_potion_value(*id), Some(slot)))
                .unwrap_or((0, None)),
        };
        if value > 45 && best.is_none_or(|(_, best_value, _)| value > best_value) {
            best = Some((action, value, swap_slot));
        }
    }
    if let Some((action, _, swap_slot)) = best {
        if let Some(slot) = swap_slot {
            if let Some(discard) = potion_discard_action(legal, slot) {
                return discard;
            }
        } else {
            return action.clone();
        }
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Proceed | Action::Skip))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
}

fn potion_discard_action(legal: &[Action], slot: usize) -> Option<Action> {
    legal
        .iter()
        .find(|action| {
            matches!(
                action,
                Action::Potion {
                    action: PotionOp::Discard,
                    slot: action_slot,
                    ..
                } if *action_slot == slot
            )
        })
        .cloned()
}

fn potion_swap_slot(game: &Game, incoming: PotionId) -> Option<usize> {
    if game.player.potions.iter().any(|p| p.id == PotionId::Slot) {
        return None;
    }
    let (slot, held) = game
        .player
        .potions
        .iter()
        .enumerate()
        .filter(|(_, potion)| {
            !matches!(
                potion.id,
                PotionId::Slot | PotionId::Fairy | PotionId::EntropicBrew
            )
        })
        .min_by_key(|(_, potion)| shop_potion_value(potion.id))?;
    ((shop_potion_value(incoming) - shop_potion_value(held.id)) as f32
        > params().potion_swap_margin)
        .then_some(slot)
}

fn shop_potion_value(id: PotionId) -> i32 {
    match id {
        PotionId::Fairy => 400,
        PotionId::EntropicBrew => 300,
        PotionId::FruitJuice => 280,
        PotionId::Blood => 250,
        PotionId::HeartOfIron => 240,
        PotionId::Focus => 220,
        PotionId::EssenceOfSteel => 210,
        PotionId::Block => 200,
        PotionId::Strength => 195,
        PotionId::Fire => 190,
        PotionId::Explosive => 175,
        PotionId::Regen => 175,
        PotionId::Energy | PotionId::PotionOfCapacity => 160,
        PotionId::Dexterity => 155,
        PotionId::Weak => 145,
        PotionId::Attack | PotionId::Power => 140,
        PotionId::SmokeBomb | PotionId::SneckoOil | PotionId::Slot => 0,
        _ => 110,
    }
}

pub fn rest_choice(game: &Game, legal: &[Action]) -> Action {
    let hp_frac = game.player.hp as f32 / game.player.max_hp.max(1) as f32;
    let act = game.dungeon.act as i32;
    let near_boss = game.current_y >= 13;
    let options = game.campfire_options();

    if keys_advance_win_condition(game) && game.final_act_available() && !game.has_ruby_key() {
        let must_recall = act >= 3 && near_boss;
        let comfortable =
            (act >= 3 && hp_frac >= 0.6) || (act == 2 && hp_frac >= 0.9 && !near_boss);
        if must_recall || comfortable {
            if let Some(recall) = legal.iter().find(|action| {
                matches!(action, Action::Choose { index, .. }
                    if options.get(*index) == Some(&CampfireOption::Recall))
            }) {
                return recall.clone();
            }
        }
    }

    // Once Smith has opened its card list, upgrade the highest-value engine
    // piece instead of whichever deck card happens to be first.
    if game.rest_is_smithing() {
        let upgradeable: Vec<_> = game
            .player
            .deck
            .iter()
            .filter(|card| card.can_upgrade())
            .collect();
        let mut best: Option<(&Action, i32)> = None;
        for action in legal {
            let Action::Choose { index, .. } = action else {
                continue;
            };
            if let Some(card) = upgradeable.get(*index) {
                let score = upgrade_score(card.id);
                if best.is_none_or(|(_, best_score)| score > best_score) {
                    best = Some((action, score));
                }
            }
        }
        if let Some((action, _)) = best {
            return action.clone();
        }
    }

    let mut want = if hp_frac
        < (if act == 1 {
            params().rest_hp_act1
        } else {
            params().rest_hp_later
        }) {
        CampfireOption::Rest
    } else {
        CampfireOption::Smith
    };
    if near_boss && hp_frac < params().rest_hp_preboss {
        want = CampfireOption::Rest;
    }
    if want == CampfireOption::Rest
        && act == 1
        && near_boss
        && game.dungeon.boss == EncounterId::Hexaghost
        && hexaghost_smith_is_safer_value(game)
    {
        want = CampfireOption::Smith;
    }
    for a in legal {
        if let Action::Choose { index, .. } = a {
            if options.get(*index) == Some(&want) {
                return a.clone();
            }
        }
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Choose { .. }))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
}

fn hexaghost_smith_is_safer_value(game: &Game) -> bool {
    let hp = game.player.hp.max(0);
    let max_hp = game.player.max_hp.max(1);
    let rest_heal = max_hp * 3 / 10;
    let rested_hp = (hp + rest_heal).min(max_hp);
    let after_divider = |entry_hp: i32| entry_hp - 6 * (entry_hp / 12 + 1);
    let current_survival = after_divider(hp);
    let rested_survival = after_divider(rested_hp);
    current_survival as f32 >= max_hp as f32 * 0.25
        && ((rested_survival - current_survival) as f32) < params().hex_rest_effective_gain_min
}

/// Rank Neow's blessings instead of defaulting to the first option: deck
/// thinning and the drawback-free third-category blessings dominate the
/// small category-0 rewards.
pub fn neow_choice(game: &Game, legal: &[Action]) -> Action {
    let mut best: Option<(&Action, i32)> = None;
    for action in legal {
        let Action::Choose { index, .. } = action else {
            continue;
        };
        let Some(option) = game.neow_options.get(*index) else {
            continue;
        };
        let value = neow_reward_rank(game, option.kind);
        if best.is_none_or(|(_, b)| value > b) {
            best = Some((action, value));
        }
    }
    best.map(|(action, _)| action.clone())
        .unwrap_or_else(|| event_choice(game, legal))
}

fn neow_reward_rank(game: &Game, kind: crate::game::NeowKind) -> i32 {
    use crate::game::NeowKind;
    match kind {
        NeowKind::RemoveTwo if deckplan::strike_purge_ready(game) => 100,
        NeowKind::RemoveTwo => 25,
        NeowKind::TransformTwo => 85,
        NeowKind::RareRelic => 80,
        NeowKind::ThreeRareCards => 75,
        NeowKind::TwoFiftyGold => 70,
        NeowKind::RemoveCard if deckplan::strike_purge_ready(game) => 55,
        NeowKind::RemoveCard => 32,
        NeowKind::ThreeCards => 50,
        NeowKind::UpgradeCard => 48,
        NeowKind::RandomRareCard => 46,
        NeowKind::RandomCommonRelic => 45,
        NeowKind::TransformCard => 44,
        NeowKind::HundredGold => 42,
        NeowKind::TenHp => 40,
        NeowKind::RandomColorless2 => 38,
        NeowKind::ThreePotions => 35,
        NeowKind::RandomColorless => 30,
        NeowKind::ThreeEnemyKill => 25,
        NeowKind::TwentyHp => 60,
        NeowKind::BossRelic => 20,
    }
}

pub fn event_choice(game: &Game, legal: &[Action]) -> Action {
    let event_options = game
        .event
        .as_ref()
        .map(|event| event.options.as_slice())
        .unwrap_or(&[]);
    let mut choices: Vec<(&Action, EventOption)> = legal
        .iter()
        .filter_map(|action| match action {
            Action::Choose { index, .. } => event_options
                .get(*index)
                .copied()
                .map(|option| (action, option)),
            _ => None,
        })
        .collect();
    if choices.is_empty() {
        return legal[0].clone();
    }

    // Never accept an advertised HP cost that is lethal or leaves at most 2
    // HP while another option survives. This guard applies to every event.
    let survivable: Vec<_> = choices
        .iter()
        .filter(|(_, option)| event_hp_loss(game, *option) < game.player.hp - 2)
        .cloned()
        .collect();
    if !survivable.is_empty() && survivable.len() < choices.len() {
        choices = survivable;
    }

    let event_state = game.event.as_ref();
    let event = event_state.map(|event| event.id);
    let event_screen = event_state.map(|event| event.screen).unwrap_or(0);
    let hp = game.player.hp;
    let max_hp = game.player.max_hp.max(1);
    let hp_frac = hp as f32 / max_hp as f32;
    let gold = game.player.gold;
    let pick = |options: &[EventOption]| choice_matching(&choices, options);

    let selected = match event {
        Some(EventId::DrugDealer) => pick(&[EventOption::Study, EventOption::Inject]),
        Some(EventId::Falling) => {
            if event_screen == 1 {
                falling_event_choice(game, &choices)
            } else {
                pick(&[EventOption::Continue])
            }
        }
        Some(EventId::MatchAndKeep) => match_and_keep_choice(game, &choices),
        Some(EventId::KnowingSkull) => pick(&[EventOption::KnowingSkullLeave]),
        Some(EventId::ForgottenAltar) => pick(&[EventOption::Offer]).or_else(|| {
            if hp > 30.max(max_hp / 3) {
                pick(&[EventOption::Sacrifice])
            } else {
                pick(&[EventOption::Desecrate, EventOption::Leave])
            }
        }),
        Some(EventId::WindingHalls) => pick(&[EventOption::Retrace]),
        // Free shrine blessings: purge, upgrade, duplicate, transform.
        Some(
            EventId::Purifier
            | EventId::UpgradeShrine
            | EventId::Duplicator
            | EventId::Transmorgrifier
            | EventId::GoldenShrine,
        ) => pick(&[EventOption::Pray]),
        Some(EventId::AccursedBlacksmith) => pick(&[EventOption::Forge]),
        Some(EventId::ShiningLight) => {
            if hp_frac >= 0.7 {
                pick(&[EventOption::Enter])
            } else {
                None
            }
        }
        Some(EventId::Cleric) => {
            if gold >= 110 {
                pick(&[EventOption::Purify])
            } else if hp_frac < 0.55 && gold >= 40 {
                pick(&[EventOption::Heal])
            } else {
                None
            }
        }
        Some(EventId::Library) => {
            if hp_frac >= 0.65 {
                pick(&[EventOption::Read])
            } else {
                pick(&[EventOption::Sleep])
            }
        }
        Some(EventId::Addict) => {
            if gold >= 150 {
                pick(&[EventOption::OfferGold])
            } else {
                None
            }
        }
        Some(EventId::Beggar) => {
            if gold >= 160 {
                pick(&[EventOption::OfferGold])
            } else {
                None
            }
        }
        Some(EventId::MaskedBandits) => {
            let metrics = deck_metrics(game);
            let multi_target_ready = metrics.aoe > 0
                && (metrics.block_cards + metrics.frost_src >= 7 || metrics.scaling >= 3);
            let orb_wall_ready = metrics.scaling >= 4 && metrics.frost_src >= 2;
            let combat_ready = hp_frac >= 0.82 && (multi_target_ready || orb_wall_ready);
            if combat_ready {
                pick(&[EventOption::Fight])
            } else {
                pick(&[EventOption::Pay])
            }
        }
        _ => None,
    };
    if let Some(action) = selected {
        return action;
    }

    const SAFE: &[EventOption] = &[
        EventOption::Leave,
        EventOption::Continue,
        EventOption::Refuse,
        EventOption::Sleep,
        EventOption::Cowardice,
    ];
    if let Some(action) = pick(SAFE) {
        return action;
    }
    choices[0].0.clone()
}

fn action_at_choice_index(choices: &[(&Action, EventOption)], wanted: usize) -> Option<Action> {
    choices.iter().find_map(|(action, _)| match action {
        Action::Choose { index, .. } if *index == wanted => Some((*action).clone()),
        _ => None,
    })
}

fn falling_event_choice(game: &Game, choices: &[(&Action, EventOption)]) -> Option<Action> {
    let event = game.event.as_ref()?;
    event
        .data
        .iter()
        .enumerate()
        .filter_map(|(choice_index, deck_index)| {
            let deck_index = usize::try_from(*deck_index).ok()?;
            let card = game.player.deck.get(deck_index)?;
            Some((choice_index, removal_score(game, card, false)))
        })
        .max_by_key(|(_, score)| *score)
        .and_then(|(choice_index, _)| action_at_choice_index(choices, choice_index))
}

fn match_and_keep_choice(game: &Game, choices: &[(&Action, EventOption)]) -> Option<Action> {
    let (chosen, visible) = game.match_game_choices()?;
    match_and_keep_choice_from_state(game, choices, chosen, &visible)
}

fn match_and_keep_choice_from_state(
    game: &Game,
    choices: &[(&Action, EventOption)],
    chosen: Option<CardId>,
    visible: &[Option<CardId>],
) -> Option<Action> {
    let is_curse = |id: CardId| Card::new(id).card_type() == CardType::CURSE;

    if let Some(chosen) = chosen {
        if !is_curse(chosen) {
            if let Some(index) = visible.iter().position(|id| *id == Some(chosen)) {
                return action_at_choice_index(choices, index);
            }
        }
        if let Some(index) = visible.iter().position(Option::is_none) {
            return action_at_choice_index(choices, index);
        }
        let index = visible
            .iter()
            .position(|id| id.is_some_and(|id| id != chosen))?;
        return action_at_choice_index(choices, index);
    }

    let mut best_pair: Option<(usize, i32)> = None;
    for (index, id) in visible.iter().enumerate() {
        let Some(id) = *id else {
            continue;
        };
        if is_curse(id)
            || visible
                .iter()
                .skip(index + 1)
                .all(|other| *other != Some(id))
        {
            continue;
        }
        let value = score_card(game, &Card::new(id));
        if best_pair.is_none_or(|(_, best)| value > best) {
            best_pair = Some((index, value));
        }
    }
    if let Some((index, _)) = best_pair {
        return action_at_choice_index(choices, index);
    }
    visible
        .iter()
        .position(Option::is_none)
        .and_then(|index| action_at_choice_index(choices, index))
        .or_else(|| {
            visible
                .iter()
                .enumerate()
                .filter_map(|(index, id)| id.filter(|id| !is_curse(*id)).map(|id| (index, id)))
                .max_by_key(|(_, id)| score_card(game, &Card::new(*id)))
                .and_then(|(index, _)| action_at_choice_index(choices, index))
        })
}

fn event_hp_loss(game: &Game, option: EventOption) -> i32 {
    let event = game.event.as_ref();
    let data = |index| {
        event
            .and_then(|event| event.data.get(index))
            .copied()
            .unwrap_or(0)
    };
    match (event.map(|event| event.id), option) {
        (Some(EventId::ScrapOoze), EventOption::ReachInside | EventOption::Deeper) => data(0),
        (Some(EventId::ShiningLight), EventOption::Enter) => data(0),
        (Some(EventId::WindingHalls), EventOption::EmbraceMadness) => data(0),
        (Some(EventId::GoldenIdol), EventOption::Smash) => data(0),
        (Some(EventId::FaceTrader), EventOption::Touch) => data(0),
        (Some(EventId::Nest), EventOption::StayInLine) => data(1),
        (Some(EventId::KnowingSkull), EventOption::KnowingSkullPotion) => data(0),
        (Some(EventId::KnowingSkull), EventOption::KnowingSkullGold) => data(1),
        (Some(EventId::KnowingSkull), EventOption::KnowingSkullCard) => data(2),
        (Some(EventId::KnowingSkull), EventOption::KnowingSkullLeave) => data(3),
        (Some(EventId::ForgottenAltar), EventOption::Sacrifice) => data(0),
        (Some(EventId::WorldOfGoop), EventOption::GatherGold) => data(2),
        (Some(EventId::GoldenWing), EventOption::Pray) => data(0),
        (Some(EventId::Designer), EventOption::Punch) => data(3),
        (Some(EventId::SensoryStone), EventOption::Recall(2)) => 5,
        (Some(EventId::SensoryStone), EventOption::Recall(3)) => 10,
        _ => 0,
    }
}

fn choice_matching(choices: &[(&Action, EventOption)], wanted: &[EventOption]) -> Option<Action> {
    for option in wanted {
        if let Some((action, _)) = choices.iter().find(|(_, candidate)| candidate == option) {
            return Some((*action).clone());
        }
    }
    None
}

/// Kind-aware grid selection: purge/transform the worst card, upgrade the
/// best engine piece, retrieve the most useful combat card, and obtain the
/// highest-scored library card instead of whichever index comes first.
pub fn grid_choice(game: &Game, legal: &[Action]) -> Action {
    let fallback = || {
        legal
            .iter()
            .find(|a| matches!(a, Action::Proceed))
            .cloned()
            .or_else(|| {
                legal
                    .iter()
                    .find(|a| matches!(a, Action::Choose { .. }))
                    .cloned()
            })
            .unwrap_or_else(|| legal[0].clone())
    };
    let Some((kind, cards)) = game.grid_view() else {
        return fallback();
    };
    if cards.is_empty() {
        // Confirm stage: Proceed applies the hovered card this policy chose.
        return fallback();
    }
    let mut best: Option<(usize, i32)> = None;
    for (index, card) in &cards {
        let value = match kind {
            GridKind::Purge => removal_score(game, card, false),
            GridKind::Transform => removal_score(game, card, true),
            GridKind::Upgrade => upgrade_score(card.id) - i32::from(card.upgraded) * 500,
            GridKind::Library | GridKind::Bottle(_) | GridKind::Copy => score_card(game, card),
            GridKind::DiscardToHand | GridKind::DrawPileToHand | GridKind::SkillFromDeck => {
                retrieve_score(game, card)
            }
        };
        if best.is_none_or(|(_, b)| value > b) {
            best = Some((*index, value));
        }
    }
    let Some((want, _)) = best else {
        return fallback();
    };
    legal
        .iter()
        .find(|a| matches!(a, Action::Choose { index, .. } if *index == want))
        .cloned()
        .unwrap_or_else(fallback)
}

/// How much the deck improves when this card leaves it.
fn removal_score(game: &Game, card: &Card, transforming: bool) -> i32 {
    match card.card_type() {
        CardType::CURSE => 900,
        CardType::STATUS => 800,
        _ => match card.id {
            CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P => {
                if transforming || deckplan::strike_purge_ready(game) {
                    300
                } else {
                    120
                }
            }
            CardId::Defend_R | CardId::Defend_G | CardId::Defend_B | CardId::Defend_P => 160,
            id => 100 - defect_pick(id) - i32::from(card.upgraded) * 25,
        },
    }
}

/// Value of moving this card from a pile into the hand mid-combat
/// (Hologram, Seek, Secret Technique).
fn retrieve_score(game: &Game, card: &Card) -> i32 {
    let mut s = defect_pick(card.id)
        + i32::from(card.upgraded) * 25
        + queue_tool_retrieve_bonus(game, card);
    let incoming: i32 = game
        .combat
        .as_ref()
        .map(|c| {
            c.monsters
                .iter()
                .filter(|m| m.alive())
                .map(|m| m.intent_damage.max(0) * m.intent_hits.max(1))
                .sum()
        })
        .unwrap_or(0);
    let unblocked = (incoming - game.player.block).max(0);
    if unblocked > 0 {
        s += i32::from(card.base_block.max(0)) * 6;
    }
    if matches!(card.card_type(), CardType::STATUS | CardType::CURSE) {
        s -= 400;
    }
    // A card that is still playable with the energy left this turn is worth
    // more than one that will sit in hand.
    if card.cost >= 0 && i32::from(card.cost) <= game.player.energy {
        s += 20;
    }
    s
}

/// Redo/Multi-Cast are Recursion's and Multi-Cast's internal queue tools.
/// Their ordinary draft score understates a combat retrieval when the front
/// orb has a prepared evoke or releases energy for the rest of the hand.
fn queue_tool_retrieve_bonus(game: &Game, card: &Card) -> i32 {
    let front = game.player.orbs.first();
    match (card.id, front.map(|orb| orb.kind)) {
        (CardId::Redo, Some(OrbKind::Dark)) => front.unwrap().evoke.max(6).min(240) * 2,
        (CardId::Multi_Cast, Some(OrbKind::Dark)) => {
            front.unwrap().evoke.max(6).min(160) * game.player.energy.max(1)
        }
        (CardId::Redo, Some(OrbKind::Plasma)) => 140,
        (CardId::Multi_Cast, Some(OrbKind::Plasma)) => 180,
        (CardId::Redo | CardId::Multi_Cast, Some(OrbKind::Lightning | OrbKind::Frost)) => 80,
        (CardId::Undo, _)
            if game
                .player
                .hand
                .iter()
                .any(|held| matches!(held.id, CardId::Redo | CardId::Multi_Cast)) =>
        {
            90
        }
        _ => 0,
    }
}

pub fn hand_select(game: &Game, legal: &[Action]) -> Action {
    let mut best: Option<(&Action, i32)> = None;
    for a in legal {
        if let Action::Choose { index, .. } = a {
            let card = if game.screen == Screen::HandSelect {
                game.player.hand.get(*index)
            } else {
                None
            };
            let score = card.map(junk_score).unwrap_or(0);
            if best.map_or(true, |(_, b)| score > b) {
                best = Some((a, score));
            }
        }
    }
    if let Some((a, _)) = best {
        return a.clone();
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Proceed))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
}

fn junk_score(card: &Card) -> i32 {
    match card.id {
        CardId::Strike_R | CardId::Strike_B | CardId::Defend_R | CardId::Defend_B => 200,
        CardId::Burn | CardId::Dazed | CardId::Wound | CardId::Slimed => 400,
        _ => 80 - card_pick(card.id),
    }
}

fn deck_metrics(game: &Game) -> DeckMetrics {
    let mut metrics = DeckMetrics {
        // Deck-shape thresholds describe actionable cards. Ascender's Bane
        // and other cost -2 curse/status cards should not make the deck look
        // one pick larger before they can ever be played.
        size: game
            .player
            .deck
            .iter()
            .filter(|card| !crate::combat::status_or_curse_unplayable(card, &game.player))
            .count() as i32,
        ..DeckMetrics::default()
    };
    for card in &game.player.deck {
        if card.card_type() == CardType::ATTACK {
            metrics.attacks += 1;
            if card.base_damage >= 18 {
                metrics.big_attacks += 1;
            }
        }
        if matches!(
            card.id,
            CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P
        ) {
            metrics.strikes += 1;
        }
        if card.base_block > 0 {
            metrics.block_cards += 1;
        }
        if is_scaling(card.id) {
            metrics.scaling += 1;
        }
        if is_aoe(card.id) {
            metrics.aoe += 1;
        }
        if is_channel(card.id) {
            metrics.channel += 1;
        }
        if is_frost_source(card.id) {
            metrics.frost_src += 1;
        }
    }
    metrics
}

pub fn score_card(game: &Game, card: &Card) -> i32 {
    let mut s = card_pick(card.id);
    if game.character == Character::Defect {
        s = s.max(defect_pick(card.id));
    }
    let p = params();
    if card.upgraded {
        s += p.upgraded_pick_bonus as i32;
    }

    let metrics = deck_metrics(game);
    let act = game.dungeon.act as i32;
    if is_aoe(card.id) && metrics.aoe < 2 {
        s += p.aoe_bonus as i32;
    }
    if card.base_block > 0 && metrics.block_cards + metrics.frost_src / 2 < 5.max(metrics.size / 5)
    {
        s += p.block_bonus as i32;
    }
    if game.character == Character::Defect && is_channel(card.id) && metrics.channel < 5 {
        s += p.channel_bonus as i32;
    }
    if is_scaling(card.id) && metrics.scaling < 2 + act {
        s += p.scaling_bonus as i32;
    }
    if game.character == Character::Defect && is_focus_source(card.id) {
        let focus_sources = game
            .player
            .deck
            .iter()
            .filter(|c| is_focus_source(c.id))
            .count();
        if focus_sources < 3 {
            s += p.focus_bonus as i32;
        }
    }
    if act >= 2 && metrics.scaling < 2 && card.card_type() == CardType::ATTACK {
        if card.base_damage >= 14 {
            s += p.act2_damage_bonus as i32;
        }
        if matches!(
            card.id,
            CardId::Doom_and_Gloom
                | CardId::Sunder
                | CardId::Melter
                | CardId::Blizzard
                | CardId::Hyperbeam
                | CardId::Rip_and_Tear
        ) {
            s += p.act2_finisher_bonus as i32;
        }
    }
    if act == 1 && card.card_type() == CardType::ATTACK {
        if metrics.attacks - metrics.strikes < 5 {
            s += p.act1_attack_bonus as i32;
        }
        if card.base_damage >= 10 {
            s += p.act1_big_damage_bonus as i32;
        }
    }
    if act == 1 && game.dungeon.floor >= 11 && card.base_block > 0 && metrics.block_cards < 7 {
        s += p.act1_late_block_bonus as i32;
    }

    if game.character == Character::Defect {
        let stable_prior =
            defect_pick_base(card.id) + i32::from(card.upgraded) * p.upgraded_pick_bonus as i32;
        s = deckplan::planned_card_score(game, card, s, stable_prior);
    }

    // Copy and deck-size constraints stay outside the package override.  A
    // coherent plan still should not acquire its fourth payoff or ignore the
    // opportunity cost of another late-game draw.
    let copies = game.player.deck.iter().filter(|c| c.id == card.id).count() as i32;
    let max_copies = max_copies(card.id);
    if copies >= max_copies {
        s -= p.copies_full_penalty as i32;
    } else if copies == max_copies - 1 {
        s -= p.copies_near_penalty as i32;
    }

    let target_size = match act {
        1 => p.target_size_act1 as i32,
        2 => p.target_size_act2 as i32,
        3 | 4 => p.target_size_act3 as i32,
        _ => 26,
    };
    if metrics.size >= target_size {
        s -= p.size_full_penalty as i32;
    } else if metrics.size >= target_size - 4 {
        s -= p.size_near_penalty as i32;
    }
    s
}

fn card_pick(id: CardId) -> i32 {
    match id {
        CardId::Demon_Form => 300,
        CardId::Inflame => 230,
        CardId::Reaper => 280,
        CardId::Impervious => 270,
        CardId::Shrug_It_Off => 235,
        CardId::Feel_No_Pain => 190,
        CardId::Offering => 250,
        CardId::Bludgeon => 220,
        CardId::Pommel_Strike => 180,
        CardId::Armaments => 165,
        CardId::Barricade => 170,
        CardId::The_Bomb => 340,
        CardId::Thinking_Ahead => 200,
        CardId::Dramatic_Entrance => 220,
        CardId::Flame_Barrier => 200,
        CardId::Second_Wind => 120,
        CardId::Clothesline => 130,
        CardId::Iron_Wave => 140,
        CardId::True_Grit => 110,
        CardId::Burning_Pact => 110,
        CardId::Metallicize => 150,
        CardId::Rage => 60,
        CardId::Dark_Embrace => 120,
        CardId::Bash => 80,
        CardId::Perfected_Strike => 60,
        CardId::Sword_Boomerang => 130,
        CardId::Zap => 40,
        CardId::Dualcast => 40,
        _ => 40,
    }
}

fn defect_pick(id: CardId) -> i32 {
    if let Some(v) = params().pick.get(&id) {
        return *v as i32;
    }
    defect_pick_base(id)
}

fn defect_pick_base(id: CardId) -> i32 {
    match id {
        CardId::Defragment => 290,
        CardId::Echo_Form => 280,
        CardId::Electrodynamics => 240,
        CardId::Glacier => 240,
        CardId::Coolheaded => 190,
        CardId::Cold_Snap => 160,
        CardId::Ball_Lightning => 180,
        CardId::Loop => 170,
        CardId::Conserve_Battery => 160,
        CardId::Skim => 200,
        CardId::BootSequence => 150,
        CardId::Biased_Cognition => 150,
        CardId::Capacitor => 140,
        CardId::Buffer => 160,
        CardId::Self_Repair => 260,
        CardId::Machine_Learning => 170,
        CardId::Compile_Driver => 140,
        CardId::Sweeping_Beam => 155,
        CardId::Doom_and_Gloom => 170,
        CardId::Blizzard => 170,
        CardId::Melter => 120,
        CardId::Rip_and_Tear => 110,
        CardId::Sunder => 140,
        CardId::Hyperbeam => 130,
        CardId::Core_Surge => 140,
        CardId::FTL => 130,
        CardId::Streamline => 100,
        CardId::Barrage => 60,
        CardId::Thunder_Strike => 90,
        CardId::All_For_One => 100,
        CardId::Stack => 80,
        CardId::Go_for_the_Eyes => 115,
        CardId::Auto_Shields => 120,
        CardId::Reinforced_Body => 110,
        CardId::Force_Field => 70,
        CardId::Chill => 90,
        CardId::Chaos => 70,
        CardId::Turbo => 90,
        CardId::Fusion => 70,
        CardId::Double_Energy => 90,
        CardId::Consume => 90,
        CardId::Heatsinks => 90,
        CardId::Static_Discharge => 60,
        CardId::Storm => 110,
        CardId::Creative_AI => 90,
        CardId::Seek => 150,
        CardId::Reprogram => 30,
        CardId::White_Noise => 60,
        CardId::Rainbow => 60,
        CardId::Tempest => 90,
        CardId::Meteor_Strike => 60,
        CardId::Zap => 40,
        CardId::Dualcast => 40,
        CardId::Leap => 90,
        // With exact Rebound mechanics and sequencing, this is a useful Act 1
        // damage pick instead of an always-skipped generic attack.
        CardId::Rebound => 70,
        CardId::Scrape => 60,
        CardId::Beam_Cell => 70,
        CardId::Genetic_Algorithm => 40,
        CardId::Hologram => 60,
        CardId::Recycle => 40,
        CardId::Darkness => 50,
        // Slay the Spire's internal id for Claw is "Gash".
        CardId::Gash => 40,
        CardId::Strike_B => 20,
        CardId::Defend_B => 20,
        _ => card_pick(id),
    }
}

fn is_scaling(id: CardId) -> bool {
    matches!(
        id,
        CardId::Demon_Form
            | CardId::Inflame
            | CardId::Limit_Break
            | CardId::Spot_Weakness
            | CardId::Noxious_Fumes
            | CardId::Catalyst
            | CardId::Footwork
            | CardId::Defragment
            | CardId::Echo_Form
            | CardId::Biased_Cognition
            | CardId::Machine_Learning
            | CardId::Loop
            | CardId::MentalFortress
            | CardId::A_Thousand_Cuts
            | CardId::Venomology
            | CardId::DevaForm
            | CardId::Apotheosis
    )
}

fn is_aoe(id: CardId) -> bool {
    matches!(
        id,
        CardId::Whirlwind
            | CardId::Cleave
            | CardId::Immolate
            | CardId::Thunderclap
            | CardId::Dagger_Spray
            | CardId::Die_Die_Die
            | CardId::All_Out_Attack
            | CardId::Bouncing_Flask
            | CardId::Electrodynamics
            | CardId::Blizzard
            | CardId::Doom_and_Gloom
            | CardId::Sweeping_Beam
            | CardId::Hyperbeam
            | CardId::Consecrate
            | CardId::Conclude
            | CardId::Tantrum
            | CardId::Ragnarok
            | CardId::Shockwave
            | CardId::Dramatic_Entrance
            | CardId::Corpse_Explosion
    )
}

fn is_channel(id: CardId) -> bool {
    matches!(
        id,
        CardId::Zap
            | CardId::Ball_Lightning
            | CardId::Cold_Snap
            | CardId::Coolheaded
            | CardId::Glacier
            | CardId::Doom_and_Gloom
            | CardId::Rainbow
            | CardId::Darkness
            | CardId::Fusion
            | CardId::Tempest
            | CardId::Meteor_Strike
            | CardId::Chill
            | CardId::Electrodynamics
    )
}

fn is_frost_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Cold_Snap | CardId::Coolheaded | CardId::Glacier | CardId::Rainbow | CardId::Chill
    )
}

fn is_focus_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Defragment | CardId::Biased_Cognition | CardId::Consume
    )
}

fn max_copies(id: CardId) -> i32 {
    match id {
        CardId::Echo_Form
        | CardId::Electrodynamics
        | CardId::Self_Repair
        | CardId::Machine_Learning
        | CardId::Blizzard
        | CardId::Seek => 1,
        CardId::Glacier
        | CardId::Coolheaded
        | CardId::Loop
        | CardId::Skim
        | CardId::Buffer
        | CardId::Ball_Lightning => 2,
        CardId::Defragment => 3,
        _ => 3,
    }
}

fn upgrade_score(id: CardId) -> i32 {
    if let Some(v) = params().upgrade.get(&id) {
        return *v as i32;
    }
    match id {
        CardId::Defragment => 270,
        CardId::Echo_Form => 260,
        CardId::Glacier => 240,
        CardId::Electrodynamics => 230,
        CardId::Skim => 210,
        CardId::Coolheaded | CardId::Loop => 200,
        CardId::Self_Repair => 190,
        CardId::Ball_Lightning | CardId::Blizzard => 180,
        CardId::Buffer | CardId::Doom_and_Gloom => 170,
        CardId::Eruption => 300,
        CardId::Bash => 235,
        CardId::Defend_R | CardId::Defend_G | CardId::Defend_B | CardId::Defend_P => 40,
        CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P => 30,
        _ => 60,
    }
}

fn boss_relic_score(game: &Game, id: RelicId) -> i32 {
    let mut score = boss_relic_rank(id);
    if game.player.energy_master <= 3 {
        // Reliable every-turn energy deserves the full premium. Collar only
        // helps in elites/bosses, while Choker's sixth-card ceiling makes its
        // fourth energy conditional for the low-cost Defect decks that most
        // need it.
        score += match id {
            RelicId::Coffee_Dripper | RelicId::Fusion_Hammer | RelicId::Cursed_Key => 40,
            RelicId::Nuclear_Battery => 38,
            RelicId::Ectoplasm | RelicId::Philosophers_Stone | RelicId::Sozu => 30,
            RelicId::Velvet_Choker => 20,
            RelicId::SlaversCollar => 15,
            RelicId::Busted_Crown | RelicId::Runic_Dome => 15,
            _ => 0,
        };
    }

    let metrics = deck_metrics(game);
    match id {
        RelicId::Velvet_Choker => {
            let zero_cost = game
                .player
                .deck
                .iter()
                .filter(|card| card.card_type() != CardType::CURSE && card.cost == 0)
                .count() as i32;
            let cheap_cards = game
                .player
                .deck
                .iter()
                .filter(|card| card.card_type() != CardType::CURSE && (0..=1).contains(&card.cost))
                .count() as i32;
            let draw_cards = game
                .player
                .deck
                .iter()
                .filter(|card| {
                    matches!(
                        card.id,
                        CardId::Compile_Driver
                            | CardId::Coolheaded
                            | CardId::Skim
                            | CardId::Scrape
                            | CardId::FTL
                            | CardId::Reboot
                            | CardId::Seek
                            | CardId::Sweeping_Beam
                            | CardId::Machine_Learning
                            | CardId::Heatsinks
                    )
                })
                .count() as i32;
            score -= zero_cost * 9 + draw_cards * 7 + (cheap_cards - 8).max(0) * 2;
        }
        RelicId::Coffee_Dripper => {
            let sustain = game
                .player
                .deck
                .iter()
                .any(|card| card.id == CardId::Self_Repair)
                || game.player.relics.iter().any(|relic| {
                    matches!(
                        relic.id,
                        RelicId::Blood_Vial
                            | RelicId::Eternal_Feather
                            | RelicId::MealTicket
                            | RelicId::Meat_on_the_Bone
                            | RelicId::Toy_Ornithopter
                    )
                });
            if !sustain {
                score -= 28;
            }
        }
        RelicId::Fusion_Hammer => {
            let premium_upgrades = game
                .player
                .deck
                .iter()
                .filter(|card| {
                    !card.upgraded
                        && matches!(
                            card.id,
                            CardId::Defragment
                                | CardId::Electrodynamics
                                | CardId::Glacier
                                | CardId::Buffer
                                | CardId::Self_Repair
                                | CardId::Loop
                                | CardId::Coolheaded
                        )
                })
                .count() as i32;
            score -= (premium_upgrades * 6).min(24);
        }
        RelicId::FrozenCore if metrics.block_cards + metrics.frost_src / 2 < 5 => {
            score += 18;
        }
        _ => {}
    }
    score
}

fn boss_relic_rank(id: RelicId) -> i32 {
    if let Some(v) = params().boss_relic.get(&id) {
        return *v as i32;
    }
    match id {
        RelicId::SlaversCollar => 95,
        RelicId::Velvet_Choker => 88,
        RelicId::Cursed_Key => 82,
        RelicId::Black_Blood => 80,
        RelicId::FrozenCore => 72,
        RelicId::Nuclear_Battery => 70,
        RelicId::Runic_Pyramid => 76,
        RelicId::Coffee_Dripper => 75,
        RelicId::Fusion_Hammer => 72,
        RelicId::Tiny_House => 60,
        RelicId::Busted_Crown => 20,
        RelicId::Snecko_Eye => 15,
        RelicId::Runic_Dome => 5,
        RelicId::Calling_Bell => 8,
        _ => 55,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::{Orb, OrbKind};
    use crate::map::{DungeonMap, MapEdge, MapNode};
    use crate::unlocks::Unlocks;
    use std::sync::Arc;

    fn route_node(x: i32, y: i32, room: RoomType, edge: Option<(i32, i32)>) -> MapNode {
        MapNode {
            x,
            y,
            room: Some(room),
            taken: false,
            emerald_key: false,
            edges: edge
                .into_iter()
                .map(|(dst_x, dst_y)| MapEdge {
                    src_x: x,
                    src_y: y,
                    dst_x,
                    dst_y,
                    taken: false,
                })
                .collect(),
            parents: Vec::new(),
        }
    }

    #[test]
    fn route_lookahead_spends_hp_across_future_combats() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.dungeon.act = crate::ids::Act::City;
        game.player.hp = game.player.max_hp;
        game.dungeon.map = Arc::new(DungeonMap {
            nodes: vec![
                vec![
                    route_node(0, 0, RoomType::Monster, Some((0, 1))),
                    route_node(1, 0, RoomType::Event, None),
                ],
                vec![route_node(0, 1, RoomType::Monster, Some((0, 2)))],
                vec![route_node(0, 2, RoomType::Monster, Some((0, 3)))],
                vec![route_node(0, 3, RoomType::Monster, None)],
            ],
        });
        let combat_chain = Action::Choose {
            index: 0,
            x: Some(0),
            y: Some(0),
            room: Some(RoomType::Monster),
        };
        let event = Action::Choose {
            index: 1,
            x: Some(1),
            y: Some(0),
            room: Some(RoomType::Event),
        };

        // Four combats have slightly more static room value than one event,
        // but are not a survivable route for the starter deck once their HP
        // costs are propagated.
        assert_eq!(map_choice(&game, &[combat_chain, event.clone()]), event);
    }

    #[test]
    fn fast_defect_deck_prefers_nuclear_battery_to_velvet_choker() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        for id in [
            CardId::Go_for_the_Eyes,
            CardId::Beam_Cell,
            CardId::FTL,
            CardId::Compile_Driver,
            CardId::Coolheaded,
            CardId::Skim,
        ] {
            game.player.deck.push(Card::new(id));
        }
        game.boss_relics = vec![RelicId::Velvet_Choker, RelicId::Nuclear_Battery];
        let legal = [
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 1,
                x: None,
                y: None,
                room: None,
            },
        ];

        assert_eq!(boss_relic(&game, &legal), legal[1]);
    }

    #[test]
    fn elite_matchups_reflect_deck_shape() {
        let weak = DeckMetrics::default();
        assert_eq!(elite_matchup(Some(EncounterId::GremlinNob), weak, 0), -60);
        assert_eq!(
            elite_matchup(Some(EncounterId::ThreeSentries), weak, 0),
            -40
        );

        let prepared = DeckMetrics {
            aoe: 2,
            big_attacks: 2,
            block_cards: 6,
            ..DeckMetrics::default()
        };
        assert_eq!(
            elite_matchup(Some(EncounterId::GremlinNob), prepared, 6),
            15
        );
        assert_eq!(
            elite_matchup(Some(EncounterId::ThreeSentries), prepared, 6),
            20
        );
        assert_eq!(
            elite_matchup(Some(EncounterId::BookOfStabbing), prepared, 6),
            15
        );
        assert_eq!(elite_matchup(Some(EncounterId::Slavers), prepared, 6), 10);
        assert_eq!(elite_matchup(Some(EncounterId::GiantHead), weak, 0), -50);
        assert_eq!(elite_matchup(Some(EncounterId::Nemesis), prepared, 6), 10);
    }

    #[test]
    fn route_lookahead_advances_the_deterministic_elite_queue() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.dungeon.elite_list =
            vec![EncounterId::BookOfStabbing, EncounterId::GremlinLeader].into();

        assert_eq!(queued_elite(&game, 0), Some(EncounterId::BookOfStabbing));
        assert_eq!(queued_elite(&game, 1), Some(EncounterId::GremlinLeader));
        assert_eq!(queued_elite(&game, 2), None);
    }

    #[test]
    fn defect_engine_and_claw_are_draftable() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        assert!(score_card(&game, &Card::new(CardId::Defragment)) > 300);
        let claw_without_payoff = score_card(&game, &Card::new(CardId::Gash));
        game.player.deck.push(Card::new(CardId::All_For_One));
        assert!(score_card(&game, &Card::new(CardId::Gash)) > claw_without_payoff);
    }

    #[test]
    fn act_one_purge_stops_at_two_strikes_until_damage_is_replaced() {
        use crate::game::NeowKind;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let strike = Card::new(CardId::Strike_B);
        let defend = Card::new(CardId::Defend_B);

        assert!(removal_score(&game, &strike, false) > removal_score(&game, &defend, false));
        assert!(removal_score(&game, &strike, true) > removal_score(&game, &defend, true));
        assert!(
            neow_reward_rank(&game, NeowKind::RemoveTwo)
                > neow_reward_rank(&game, NeowKind::ThreeCards)
        );

        let mut removed = 0;
        game.player.deck.retain(|card| {
            if card.id == CardId::Strike_B && removed < 2 {
                removed += 1;
                false
            } else {
                true
            }
        });
        assert!(removal_score(&game, &strike, false) < removal_score(&game, &defend, false));
        assert!(
            neow_reward_rank(&game, NeowKind::RemoveTwo)
                < neow_reward_rank(&game, NeowKind::ThreeCards)
        );

        for id in [
            CardId::Cold_Snap,
            CardId::Ball_Lightning,
            CardId::Sweeping_Beam,
            CardId::Doom_and_Gloom,
        ] {
            game.player.deck.push(Card::new(id));
        }
        assert!(removal_score(&game, &strike, false) > removal_score(&game, &defend, false));
        assert!(
            neow_reward_rank(&game, NeowKind::RemoveTwo)
                > neow_reward_rank(&game, NeowKind::ThreeCards)
        );
    }

    #[test]
    fn retrieval_promotes_queue_tools_for_a_prepared_front_orb() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.player.energy = 3;
        *game.player.orbs = vec![Orb {
            kind: OrbKind::Dark,
            evoke: 80,
        }];

        assert!(
            retrieve_score(&game, &Card::new(CardId::Redo))
                > retrieve_score(&game, &Card::new(CardId::Strike_B))
        );
        assert!(
            retrieve_score(&game, &Card::new(CardId::Multi_Cast))
                > retrieve_score(&game, &Card::new(CardId::Strike_B))
        );

        game.player.hand.push(Card::new(CardId::Multi_Cast));
        assert!(queue_tool_retrieve_bonus(&game, &Card::new(CardId::Undo)) > 0);
    }

    #[test]
    fn combat_reward_uses_the_compact_index_after_prior_rewards_are_taken() {
        let mut game = Game::new(12, Character::Defect, 0, Unlocks::fixture());
        let prefix = [
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 1,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
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
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
        ];
        for action in prefix {
            assert!(
                game.legal_actions().contains(&action),
                "illegal replay action {action:?}"
            );
            game.step(&action);
        }

        let legal = game.legal_actions();
        let card = Action::Choose {
            index: 0,
            x: None,
            y: None,
            room: None,
        };
        let score = score_card(&game, &game.card_reward[0]);
        assert!(score as f32 >= params().pick_threshold, "score={score}");
        assert_eq!(combat_reward(&game, &legal), card);
    }

    #[test]
    fn event_hp_guard_distinguishes_hp_from_max_hp() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.event = Some(crate::game::EventState::policy_fixture(
            EventId::ForgottenAltar,
            0,
            vec![EventOption::Sacrifice],
            vec![19],
        ));
        assert_eq!(event_hp_loss(&game, EventOption::Sacrifice), 19);

        game.event = Some(crate::game::EventState::policy_fixture(
            EventId::WindingHalls,
            0,
            vec![EventOption::Retrace],
            vec![4],
        ));
        assert_eq!(event_hp_loss(&game, EventOption::Retrace), 0);
    }

    #[test]
    fn unreachable_act_four_keys_are_not_taken() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.dungeon.act = crate::ids::Act::Beyond;
        game.current_y = 14;

        let emerald = Action::Choose {
            index: 0,
            x: None,
            y: None,
            room: None,
        };
        let gold = Action::Choose {
            index: 1,
            x: None,
            y: None,
            room: None,
        };
        game.rewards = vec![
            crate::game::Reward::new(RewardKind::EmeraldKey),
            crate::game::Reward::new(RewardKind::Gold(10)),
        ];
        assert_eq!(combat_reward(&game, &[emerald, gold.clone()]), gold);

        let recall = Action::Choose {
            index: 0,
            x: None,
            y: None,
            room: None,
        };
        let smith = Action::Choose {
            index: 1,
            x: None,
            y: None,
            room: None,
        };
        game.player.hp = game.player.max_hp;
        assert_eq!(rest_choice(&game, &[recall, smith.clone()]), smith);
    }

    #[test]
    fn hexaghost_rest_choice_accounts_for_divider_breakpoints() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.current_y = 14;
        game.dungeon.boss = EncounterId::Hexaghost;
        let rest = Action::Choose {
            index: 0,
            x: None,
            y: None,
            room: None,
        };
        let smith = Action::Choose {
            index: 1,
            x: None,
            y: None,
            room: None,
        };

        game.player.hp = 55;
        assert_eq!(rest_choice(&game, &[rest.clone(), smith.clone()]), smith);
        game.player.hp = 35;
        assert_eq!(rest_choice(&game, &[rest.clone(), smith]), rest);
    }

    #[test]
    fn deck_size_excludes_ascenders_bane() {
        let a0 = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let a20 = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        assert_eq!(a0.player.deck.len(), 10);
        assert_eq!(a20.player.deck.len(), 11);
        assert_eq!(deck_metrics(&a0).size, 10);
        assert_eq!(deck_metrics(&a20).size, 10);
    }

    #[test]
    fn focus_sources_are_cards_that_actually_produce_focus() {
        assert!(is_focus_source(CardId::Defragment));
        assert!(is_focus_source(CardId::Biased_Cognition));
        assert!(is_focus_source(CardId::Consume));
        assert!(!is_focus_source(CardId::Capacitor));
        assert!(!is_focus_source(CardId::Storm));
        assert!(!is_focus_source(CardId::Echo_Form));
    }

    #[test]
    fn potion_reward_replaces_the_weakest_unprotected_slot() {
        use crate::game::Reward;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.screen = Screen::CombatReward;
        game.player.potions[0].id = PotionId::Weak;
        game.player.potions[1].id = PotionId::Fairy;
        game.rewards = vec![Reward::new(RewardKind::Potion(PotionId::Focus))];

        let legal = game.legal_actions();
        let discard = Action::Potion {
            action: PotionOp::Discard,
            slot: 0,
            target_index: None,
        };
        assert!(legal.contains(&discard));
        assert_eq!(combat_reward(&game, &legal), discard.clone());

        game.step(&discard);
        let legal = game.legal_actions();
        let claim = combat_reward(&game, &legal);
        let potion_index = game
            .rewards
            .iter()
            .filter(|reward| !reward.taken)
            .position(|reward| matches!(reward.kind, RewardKind::Potion(_)))
            .expect("potion reward");
        assert!(matches!(claim, Action::Choose { index, .. } if index == potion_index));
        game.step(&claim);
        assert_eq!(game.player.potions[0].id, PotionId::Focus);
        assert_eq!(game.player.potions[1].id, PotionId::Fairy);
    }

    #[test]
    fn shop_swap_discards_before_buying_and_protects_premium_potions() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.potions[0].id = PotionId::Weak;
        game.player.potions[1].id = PotionId::Fairy;
        assert_eq!(potion_swap_slot(&game, PotionId::Focus), Some(0));

        game.player.potions[0].id = PotionId::Slot;
        assert_eq!(potion_swap_slot(&game, PotionId::Focus), None);

        game.player.potions[0].id = PotionId::EntropicBrew;
        assert_eq!(potion_swap_slot(&game, PotionId::Focus), None);
    }

    #[test]
    fn event_policy_handles_safe_value_and_memory_choices() {
        use crate::game::EventState;

        let set_event =
            |game: &mut Game, id: EventId, screen: i32, options: &[EventOption], data: Vec<i32>| {
                game.screen = Screen::Event;
                game.event = Some(EventState::policy_fixture(
                    id,
                    screen,
                    options.to_vec(),
                    data,
                ));
            };
        let chosen_option = |game: &Game| match event_choice(game, &game.legal_actions()) {
            Action::Choose { index, .. } => game
                .event
                .as_ref()
                .and_then(|event| event.options.get(index))
                .copied()
                .expect("event option"),
            action => panic!("expected event choice, got {action:?}"),
        };

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        set_event(
            &mut game,
            EventId::DrugDealer,
            0,
            &[EventOption::Ingest, EventOption::Study, EventOption::Inject],
            vec![],
        );
        assert_eq!(chosen_option(&game), EventOption::Study);

        game.player.hp = 30;
        game.player.gold = 108;
        set_event(
            &mut game,
            EventId::MaskedBandits,
            0,
            &[EventOption::Pay, EventOption::Fight],
            vec![],
        );
        assert_eq!(chosen_option(&game), EventOption::Pay);
        game.player.hp = game.player.max_hp;
        assert_eq!(chosen_option(&game), EventOption::Pay);
        game.player.deck.extend([
            Card::new(CardId::Electrodynamics),
            Card::new(CardId::Glacier),
            Card::new(CardId::Glacier),
        ]);
        assert_eq!(chosen_option(&game), EventOption::Fight);

        let strike = game
            .player
            .deck
            .iter()
            .position(|card| card.id == CardId::Strike_B)
            .unwrap() as i32;
        let defend = game
            .player
            .deck
            .iter()
            .position(|card| card.id == CardId::Defend_B)
            .unwrap() as i32;
        set_event(
            &mut game,
            EventId::Falling,
            1,
            &[EventOption::Land, EventOption::Channel, EventOption::Strike],
            vec![defend, -1, strike],
        );
        // Falling is an Act 3 event, where a basic Strike is the preferred loss.
        assert_eq!(chosen_option(&game), EventOption::Strike);

        let actions = [
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            },
            Action::Choose {
                index: 1,
                x: None,
                y: None,
                room: None,
            },
        ];
        let choices: Vec<_> = actions
            .iter()
            .enumerate()
            .map(|(index, action)| (action, EventOption::MatchCard(index)))
            .collect();
        assert_eq!(
            match_and_keep_choice_from_state(
                &game,
                &choices,
                Some(CardId::AscendersBane),
                &[Some(CardId::AscendersBane), None],
            ),
            Some(actions[1].clone())
        );
    }

    #[test]
    fn knowing_skull_policy_leaves_the_staged_reward_screen() {
        use crate::game::EventState;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.screen = Screen::Event;
        game.event = Some(EventState::policy_fixture(
            EventId::KnowingSkull,
            1,
            vec![
                EventOption::KnowingSkullPotion,
                EventOption::KnowingSkullGold,
                EventOption::KnowingSkullCard,
                EventOption::KnowingSkullLeave,
            ],
            vec![6, 6, 6, 6],
        ));
        let initial_hp = game.player.hp;

        let paid_leave = event_choice(&game, &game.legal_actions());
        assert_eq!(
            paid_leave,
            Action::Choose {
                index: 3,
                x: None,
                y: None,
                room: None,
            }
        );
        game.step(&paid_leave);

        let event = game.event.as_ref().expect("Knowing Skull result screen");
        assert_eq!(game.player.hp, initial_hp - 6);
        assert_eq!(event.screen, 2);
        assert_eq!(event.options, [EventOption::Leave]);

        let final_leave = event_choice(&game, &game.legal_actions());
        assert_eq!(
            final_leave,
            Action::Choose {
                index: 0,
                x: None,
                y: None,
                room: None,
            }
        );
        game.step(&final_leave);

        assert_eq!(game.screen, Screen::Map);
        assert!(game.event.is_none());
    }
}
