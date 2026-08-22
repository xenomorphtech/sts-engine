use crate::action::Action;
use crate::creature::{Monster, OrbKind, Player};
use crate::game::{Game, Screen};
use crate::ids::{Act, CardId, CardType, MonsterId, PotionId, PowerId, RelicId, RoomType};
use std::collections::{HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use super::params::params;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FightKind {
    Normal,
    Elite,
    Boss,
}

/// Pick the combat command: try each legal play as a first move, then greedy
/// the rest of the turn on a cloned `Game` (real engine rules, not a second sim).
pub fn plan_turn(game: &Game, legal: &[Action]) -> Action {
    if let Some(potion) = potion_policy(game, legal) {
        return potion;
    }
    let plays: Vec<&Action> = legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    if plays.is_empty() {
        return legal
            .iter()
            .find(|a| matches!(a, Action::EndTurn))
            .cloned()
            .unwrap_or_else(|| legal[0].clone());
    }
    if let Some(lethal) = exact_attack_lethal(game, legal) {
        return lethal;
    }
    let mut best_first: Option<&Action> = None;
    let mut best_score = f32::MIN;
    for first in &plays {
        let mut clone = game.clone();
        clone.step(first);
        resolve_grid_selects(&mut clone);
        if non_progressing_status_play(game, &clone, first) {
            continue;
        }
        let first_value = rebound_play_value(game, first) + setup_play_value(game, first);
        let (clone, rest_value) = searched_rest(game, clone);
        let score = score_state(game, &clone) + first_value + rest_value;
        if score > best_score {
            best_score = score;
            best_first = Some(*first);
        }
    }
    // Also consider ending the turn immediately (full block / empty energy).
    if let Some(end) = legal.iter().find(|a| matches!(a, Action::EndTurn)) {
        let mut clone = game.clone();
        clone.step(end);
        let score = score_state(game, &clone);
        if best_first.is_none() || score > best_score + 5.0 {
            return end.clone();
        }
    }
    best_first.cloned().unwrap_or_else(|| plays[0].clone())
}

const EXACT_LETHAL_NODE_BUDGET: usize = 20_000;

/// Find a proven same-turn kill before the bounded heuristic beam runs.
///
/// This deliberately searches only Attack plays. That keeps the branch factor
/// small enough for a hard per-decision budget, while covering the common case
/// where card order, Vulnerable, or target order separates lethal from a miss.
fn exact_attack_lethal(game: &Game, legal: &[Action]) -> Option<Action> {
    let turn = game.combat.as_ref()?.turn;
    let target_ehp = living_enemy_ehp(game);
    if target_ehp <= 0 || optimistic_attack_damage(game, legal) < target_ehp {
        return None;
    }

    let mut queue = VecDeque::new();
    let mut expanded = 0usize;
    for first in legal.iter().filter(|action| attack_play(game, action)) {
        if expanded >= EXACT_LETHAL_NODE_BUDGET {
            break;
        }
        expanded += 1;
        let mut after = game.clone();
        after.step(first);
        resolve_grid_selects(&mut after);
        if combat_won(&after) {
            return Some(first.clone());
        }
        if same_combat_turn(&after, turn) {
            queue.push_back((after, first.clone()));
        }
    }

    while let Some((state, first)) = queue.pop_front() {
        for action in state
            .legal_actions()
            .into_iter()
            .filter(|action| attack_play(&state, action))
        {
            if expanded >= EXACT_LETHAL_NODE_BUDGET {
                return None;
            }
            expanded += 1;
            let mut after = state.clone();
            after.step(&action);
            resolve_grid_selects(&mut after);
            if combat_won(&after) {
                return Some(first);
            }
            if same_combat_turn(&after, turn) {
                queue.push_back((after, first.clone()));
            }
        }
    }
    None
}

fn attack_play(game: &Game, action: &Action) -> bool {
    let Action::Play { hand_index, .. } = action else {
        return false;
    };
    game.player
        .hand
        .get(*hand_index)
        .is_some_and(|card| card.card_type() == CardType::ATTACK)
}

fn same_combat_turn(game: &Game, turn: i32) -> bool {
    game.screen == Screen::Combat
        && game.player.hp > 0
        && game
            .combat
            .as_ref()
            .is_some_and(|combat| combat.turn == turn && !combat.all_dead())
}

fn combat_won(game: &Game) -> bool {
    game.player.hp > 0
        && (game.combat.as_ref().is_some_and(|combat| combat.all_dead())
            || (game.combat.is_none() && game.screen == Screen::CombatReward))
}

fn living_enemy_ehp(game: &Game) -> i32 {
    game.combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| monster.alive())
                .map(|monster| monster.hp.saturating_add(monster.block))
                .sum()
        })
        .unwrap_or(0)
}

/// Cheap upper-biased gate for the exact search. Each distinct Attack is
/// simulated once per legal target, then its best immediate damage is doubled
/// to leave room for Vulnerable, strength scaling, and attack-draw chains.
fn optimistic_attack_damage(game: &Game, legal: &[Action]) -> i32 {
    let before = living_enemy_ehp(game);
    let mut best_by_hand = vec![0; game.player.hand.len()];
    for action in legal.iter().filter(|action| attack_play(game, action)) {
        let Action::Play { hand_index, .. } = action else {
            continue;
        };
        let mut after = game.clone();
        after.step(action);
        resolve_grid_selects(&mut after);
        let dealt = if combat_won(&after) {
            before
        } else {
            before.saturating_sub(living_enemy_ehp(&after))
        };
        best_by_hand[*hand_index] = best_by_hand[*hand_index].max(dealt);
    }
    best_by_hand.into_iter().sum::<i32>().saturating_mul(2)
}

#[derive(Clone)]
struct TurnSearchNode {
    game: Game,
    strategic_value: f32,
}

/// Fingerprint the mutable state relevant to the remainder of this turn.
///
/// Draw/discard/exhaust piles are intentionally excluded: order permutations
/// put the same cards in those piles in a different order, which is the width
/// waste this key is meant to collapse. The hand, RNG, relic counters, orbs,
/// powers, and full Combat state keep genuinely different tactical branches
/// distinct.
fn turn_state_hash(game: &Game) -> u64 {
    let mut hasher = DefaultHasher::new();
    std::mem::discriminant(&game.screen).hash(&mut hasher);
    game.rng.hash(&mut hasher);
    game.player.hp.hash(&mut hasher);
    game.player.max_hp.hash(&mut hasher);
    game.player.block.hash(&mut hasher);
    game.player.energy.hash(&mut hasher);
    game.player.energy_master.hash(&mut hasher);
    game.player.relics.hash(&mut hasher);
    game.player.powers.hash(&mut hasher);
    game.player.hand.hash(&mut hasher);
    game.player.duplication.hash(&mut hasher);
    game.player.pending_static.hash(&mut hasher);
    game.player.pending_evoke_lightning.hash(&mut hasher);
    game.player.pending_evoke_frost.hash(&mut hasher);
    game.player.pending_evoke_dark.hash(&mut hasher);
    game.player.orbs.hash(&mut hasher);
    game.player.max_orbs.hash(&mut hasher);
    game.combat.hash(&mut hasher);
    game.hand_select.hash(&mut hasher);
    game.pending_cards.hash(&mut hasher);
    game.potion_blizzard.hash(&mut hasher);
    game.card_blizz.hash(&mut hasher);
    hasher.finish()
}

fn searched_rest(origin: &Game, start: Game) -> (Game, f32) {
    let width = params().search_width.round().max(1.0) as usize;
    let depth = params().search_depth.round().max(1.0) as usize;

    let mut frontier = vec![TurnSearchNode {
        game: start,
        strategic_value: 0.0,
    }];
    let mut finals = Vec::new();
    for _ in 0..depth {
        let mut next = Vec::new();
        let current = std::mem::take(&mut frontier);
        for node in current {
            if node.game.screen != Screen::Combat
                || node.game.player.hp <= 0
                || node
                    .game
                    .combat
                    .as_ref()
                    .is_some_and(|combat| combat.all_dead())
            {
                finals.push(node);
                continue;
            }
            let legal = node.game.legal_actions();
            if let Some(end) = legal
                .iter()
                .find(|action| matches!(action, Action::EndTurn))
            {
                let mut ended = node.clone();
                ended.game.step(end);
                finals.push(ended);
            }
            for play in legal
                .iter()
                .filter(|action| matches!(action, Action::Play { .. }))
            {
                let mut after = node.game.clone();
                after.step(play);
                resolve_grid_selects(&mut after);
                if non_progressing_status_play(&node.game, &after, play) {
                    continue;
                }
                next.push(TurnSearchNode {
                    game: after,
                    strategic_value: node.strategic_value
                        + rebound_play_value(&node.game, play)
                        + setup_play_value(&node.game, play),
                });
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_by(|a, b| {
            let a_score = score_state(origin, &a.game) + a.strategic_value;
            let b_score = score_state(origin, &b.game) + b.strategic_value;
            b_score.total_cmp(&a_score)
        });
        // Card-order permutations often reach the same state. Since the
        // frontier is already best-first, retain the highest-value route to
        // each state before spending the limited width on it.
        let mut seen = HashSet::with_capacity(next.len());
        next.retain(|node| seen.insert(turn_state_hash(&node.game)));
        next.truncate(width);
        frontier = next;
    }
    finals.extend(frontier);
    let best = finals
        .into_iter()
        .max_by(|a, b| {
            let a_score = score_state(origin, &a.game) + a.strategic_value;
            let b_score = score_state(origin, &b.game) + b.strategic_value;
            a_score.total_cmp(&b_score)
        })
        .expect("turn search always has an end or continuation");
    (best.game, best.strategic_value)
}

/// Step through in-combat grid selections (Hologram, Seek, Secret Technique)
/// with the same policy the agent uses, so the turn search values those plays
/// by their resolved outcome instead of treating the grid screen as terminal.
fn resolve_grid_selects(game: &mut Game) {
    for _ in 0..8 {
        if game.screen != Screen::Grid {
            return;
        }
        let legal = game.legal_actions();
        if legal.is_empty() {
            return;
        }
        let choice = crate::htn::strategy::grid_choice(game, &legal);
        game.step(&choice);
    }
}

fn non_progressing_status_play(before: &Game, after: &Game, action: &Action) -> bool {
    let Action::Play { hand_index, .. } = action else {
        return false;
    };
    let Some(card) = before.player.hand.get(*hand_index) else {
        return false;
    };
    if !matches!(card.card_type(), CardType::STATUS | CardType::CURSE) {
        return false;
    }
    let same_monsters = before
        .combat
        .as_ref()
        .zip(after.combat.as_ref())
        .is_some_and(|(old, new)| {
            old.monsters.len() == new.monsters.len()
                && old
                    .monsters
                    .iter()
                    .zip(&new.monsters)
                    .all(|(old, new)| old.hp == new.hp && old.block == new.block)
        });
    same_monsters
        && before.player.hp == after.player.hp
        && before.player.block == after.player.block
        && before.player.energy == after.player.energy
        && before.player.hand == after.player.hand
        && before.player.draw == after.player.draw
        && before.player.discard == after.player.discard
}

/// Rebound makes the chosen non-Power card the next draw. Score that future
/// draw while choosing the card immediately after Rebound; the cloned engine
/// handles the actual pile movement.
fn rebound_play_value(game: &Game, action: &Action) -> f32 {
    if game.player.power_amount(PowerId::Rebound) <= 0 {
        return 0.0;
    }
    let Action::Play { hand_index, .. } = action else {
        return 0.0;
    };
    let Some(card) = game.player.hand.get(*hand_index) else {
        return 0.0;
    };
    if card.card_type() == CardType::POWER || card.exhaust {
        return 0.0;
    }

    let tactical = match card.id {
        CardId::Glacier => 55.0,
        CardId::Hologram => 50.0,
        CardId::Coolheaded | CardId::Skim => 48.0,
        CardId::Doom_and_Gloom | CardId::Sunder => 46.0,
        CardId::Rebound | CardId::Streamline => 42.0,
        CardId::Ball_Lightning | CardId::Cold_Snap | CardId::Sweeping_Beam => 38.0,
        CardId::Go_for_the_Eyes | CardId::Beam_Cell => 32.0,
        CardId::Genetic_Algorithm => 30.0,
        _ => 0.0,
    };
    let repeatable_output =
        card.base_damage.max(0) as f32 * 2.5 + card.base_block.max(0) as f32 * 2.0;
    (tactical + repeatable_output).clamp(8.0, 100.0)
}

/// Value Self Repair's delayed heal while deciding whether to spend energy on
/// it. The cloned engine cannot observe that payoff until combat ends, so the
/// shallow turn search otherwise skips it for immediate chip damage.
fn setup_play_value(game: &Game, action: &Action) -> f32 {
    let Action::Play { hand_index, .. } = action else {
        return 0.0;
    };
    let Some(card) = game.player.hand.get(*hand_index) else {
        return 0.0;
    };
    let hp_frac = game.player.hp as f32 / game.player.max_hp.max(1) as f32;
    let danger = (params().danger_base + params().danger_scale * (1.0 - hp_frac).powi(2))
        * (1.0 + game.ascension as f32 / 50.0);
    match card.id {
        CardId::Biased_Cognition
            if game.combat.as_ref().is_some_and(|combat| {
                combat.monsters.iter().any(|monster| {
                    monster.id == MonsterId::Champ
                        && !monster.split_triggered
                        && monster.hp >= monster.max_hp / 2
                })
            }) =>
        {
            -2_000.0
        }
        CardId::Self_Repair => card.base_magic.max(1) as f32 * danger * 1.25,
        CardId::Machine_Learning => {
            let turns_left = fight_length(fight_kind(game), game.dungeon.act);
            let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
            card.base_magic.max(1) as f32 * turns_left * damage_weight * 2.2
        }
        CardId::Echo_Form => {
            let turns_left = fight_length(fight_kind(game), game.dungeon.act).max(1.0);
            let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
            card.base_magic.max(1) as f32 * turns_left * damage_weight * 12.0
        }
        _ => 0.0,
    }
}

/// Block available before monsters act if the player ends the turn now.
///
/// Mid-turn search used to see only block already on the player, which made
/// Frost and end-of-turn block effects look unsafe until after EndTurn had
/// actually been simulated. Keep this in the turn planner so pruning uses the
/// same ordering as combat::end_turn without mutating a cloned game.
fn end_of_turn_block(game: &Game) -> i32 {
    let player = &game.player;
    let mut block = player.block;

    // Orichalcum checks the pre-power block amount, then Metallicize and
    // Plated Armor resolve afterward.
    if block == 0 && player.has_relic(RelicId::Orichalcum) {
        block += 6;
    }
    block += player.power_amount(PowerId::Metallicize).max(0);
    block += player.power_amount(PowerId::PlatedArmor).max(0);

    let frost_passive = (2 + player.power_amount(PowerId::Focus)).max(0);
    let frozen_core_frost =
        player.has_relic(RelicId::FrozenCore) && (player.orbs.len() as i32) < player.max_orbs;
    let frost_orbs = player
        .orbs
        .iter()
        .filter(|orb| orb.kind == OrbKind::Frost)
        .count() as i32
        + i32::from(frozen_core_frost);
    block += frost_orbs * frost_passive;

    // Gold-Plated Cables triggers the front filled orb a second time. A Frost
    // supplied by Frozen Core is the front orb only when the row was empty.
    let front_is_frost = player
        .orbs
        .first()
        .is_some_and(|orb| orb.kind == OrbKind::Frost)
        || (player.orbs.is_empty() && frozen_core_frost);
    if front_is_frost && player.has_relic(RelicId::Cables) {
        block += frost_passive;
    }

    block
}

/// Maximum direct block the already-drawn hand can afford next turn. This is
/// a small 0/1 knapsack rather than a rollout: it prices the energy that must
/// be reserved for defense without speculating about future card effects.
fn cheap_hand_block(game: &Game) -> i32 {
    let energy = game.player.energy.max(0) as usize;
    let mut best = vec![0; energy + 1];

    for card in &game.player.hand {
        let block = crate::combat::derived_block(card, &game.player).max(0);
        if block == 0 {
            continue;
        }
        if card.cost_for_turn == -1 {
            // Reinforced Body and other X-cost block cards can consume any
            // remaining amount. Snapshot the old frontier so the same card
            // is not selected more than once.
            let old = best.clone();
            for spent_before in 0..=energy {
                for x in 0..=energy - spent_before {
                    best[spent_before + x] =
                        best[spent_before + x].max(old[spent_before] + block * x as i32);
                }
            }
            continue;
        }
        let cost = if card.free_to_play_once {
            0
        } else {
            card.cost_for_turn.max(0) as usize
        };
        if cost > energy {
            continue;
        }
        if cost == 0 {
            for value in &mut best {
                *value += block;
            }
        } else {
            for spent in (cost..=energy).rev() {
                best[spent] = best[spent].max(best[spent - cost] + block);
            }
        }
    }

    best.into_iter().max().unwrap_or(0)
}

fn score_state(before: &Game, after: &Game) -> f32 {
    if after.player.hp <= 0 {
        return -100_000.0;
    }
    let living: Vec<_> = after
        .combat
        .as_ref()
        .map(|c| c.monsters.iter().filter(|m| m.alive()).collect())
        .unwrap_or_default();
    if living.is_empty() {
        return 4_000.0 + after.player.hp as f32 + after.player.energy as f32 * 5.0;
    }

    let p = params();
    let turns_left = fight_length(fight_kind(after), after.dungeon.act);
    let damage_weight = p.dmg_base + p.dmg_per_turn * turns_left;
    let mut dealt = 0.0;
    let mut stripped_block = 0.0;
    let mut dead = 0;
    let mut laga_wake_penalty = 0.0;
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        for (index, monster) in before_combat.monsters.iter().enumerate() {
            if !monster.alive() {
                continue;
            }
            let hp_after = after_combat.monsters.get(index).map_or(0, |m| m.hp.max(0));
            let hp_damage = (monster.hp - hp_after).max(0);
            dealt += hp_damage as f32 * target_priority(monster.id);
            if monster.id == MonsterId::Lagavulin && monster.extra < 3 && hp_after > 0 {
                if let Some(monster_after) = after_combat.monsters.get(index) {
                    let woke_early = monster_after.extra >= 3 && hp_damage > 0;
                    let kill_is_close = hp_damage > 0
                        && hp_after as f32 / hp_damage as f32 <= p.laga_wake_kill_ratio;
                    if woke_early && !kill_is_close {
                        laga_wake_penalty += p.laga_wake_penalty;
                    }
                }
            }
            if monster.power_amount(PowerId::Barricade) > 0 && monster.block >= monster.hp.max(1) {
                if let Some(monster_after) = after_combat.monsters.get(index) {
                    stripped_block += (monster.block - monster_after.block).max(0) as f32
                        * target_priority(monster.id);
                }
            }
            if hp_after <= 0 {
                dead += 1;
            }
        }
    }
    let incoming: i32 = living
        .iter()
        .map(|monster| projected_incoming(&after.player, monster))
        .sum();

    let turn_advanced =
        before.combat.as_ref().map(|c| c.turn) != after.combat.as_ref().map(|c| c.turn);
    let scripted = if !turn_advanced && p.spike_danger > 0.0 {
        let horizon = p.spike_horizon.round().clamp(1.0, 3.0) as i32;
        living
            .iter()
            .map(|monster| scripted_incoming(&after.player, monster, horizon))
            .sum()
    } else {
        0
    };
    let effective_block = end_of_turn_block(after);
    let unblocked = if turn_advanced {
        (before.player.hp - after.player.hp).max(0) as f32
    } else {
        (incoming - effective_block).max(0) as f32
    };
    let projected_hp = if turn_advanced {
        after.player.hp
    } else {
        after.player.hp - unblocked as i32
    };
    if projected_hp <= 0 {
        return -100_000.0;
    }

    let hp_frac = projected_hp as f32 / after.player.max_hp.max(1) as f32;
    let mut danger = (p.danger_base + p.danger_scale * (1.0 - hp_frac).powi(2))
        * (1.0 + after.ascension as f32 / 50.0);
    let hp_left: i32 = living.iter().map(|m| m.hp.max(0)).sum();
    if dealt > 0.0 && hp_left as f32 / dealt <= 2.0 && projected_hp as f32 > unblocked {
        danger *= p.lethal_discount;
    }

    let mut value = dealt * damage_weight;
    value += stripped_block * damage_weight * p.strip_block_mult;
    value += dead as f32 * p.kill_bonus;
    value -= laga_wake_penalty;
    value -= unblocked * danger;
    if scripted > 0 {
        let bank = persistent_block_bank(after);
        value -= (scripted - bank).max(0) as f32 * p.spike_danger;
    }
    if !turn_advanced {
        // Do not tax setup cards merely because their immediate block exceeds
        // a quiet intent when a larger deterministic attack is imminent.
        if scripted <= incoming {
            value -= (effective_block - incoming).max(0) as f32 * p.overblock_penalty;
        }
    } else if p.next_exposure_weight > 0.0 || p.next_block_tax > 0.0 {
        // EndTurn has already rolled the real next intents and drawn the real
        // next hand. Penalize exposed damage as well as block that consumes
        // next turn's energy, while retaining a zero-cost default.
        let hand_block = cheap_hand_block(after);
        let wall = effective_block;
        let exposed = (incoming - wall - hand_block).max(0);
        let taxed_block = hand_block.min((incoming - wall).max(0));
        value -= exposed as f32 * p.next_exposure_weight * danger;
        value -= taxed_block as f32 * p.next_block_tax;
    }

    let strength = after.player.power_amount(PowerId::Strength)
        - before.player.power_amount(PowerId::Strength);
    let dexterity = after.player.power_amount(PowerId::Dexterity)
        - before.player.power_amount(PowerId::Dexterity);
    let focus =
        after.player.power_amount(PowerId::Focus) - before.player.power_amount(PowerId::Focus);
    value += strength as f32 * p.strength_weight * turns_left;
    value += dexterity as f32 * p.dexterity_weight * turns_left;
    value += focus as f32 * p.focus_weight * turns_left;
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        let enemy_strength_gain: i32 = after_combat
            .monsters
            .iter()
            .enumerate()
            .map(|(index, monster)| {
                let old = before_combat
                    .monsters
                    .get(index)
                    .map_or(0, |old| old.power_amount(PowerId::Strength));
                (monster.power_amount(PowerId::Strength) - old).max(0)
            })
            .sum();
        value -= enemy_strength_gain as f32 * p.enemy_strength_penalty * turns_left;
    }
    let status_gain = (status_card_count(&after.player) - status_card_count(&before.player)).max(0);
    value -= status_gain as f32 * p.status_gain_penalty;
    let focus_decay = after.player.power_amount(PowerId::Bias).max(0) as f32;
    value -= focus_decay * p.bias_decay_weight * (turns_left * (turns_left + 1.0) / 2.0);
    value += orb_value(after, turns_left) - orb_value(before, turns_left);
    value += after.player.energy as f32 * p.energy_value;
    value
}

fn projected_incoming(player: &Player, monster: &Monster) -> i32 {
    if monster.intent_damage <= 0 {
        return 0;
    }
    projected_attack(
        player,
        monster,
        monster.intent_damage,
        monster.intent_hits,
        0,
    )
}

fn status_card_count(player: &Player) -> i32 {
    [&player.hand, &player.draw, &player.discard, &player.exhaust]
        .into_iter()
        .flatten()
        .filter(|card| card.card_type() == CardType::STATUS)
        .count() as i32
}

fn projected_attack(
    player: &Player,
    monster: &Monster,
    base: i32,
    hits: i32,
    future_strength: i32,
) -> i32 {
    let mut damage = (base + monster.power_amount(PowerId::Strength) + future_strength) as f32;
    if monster.power_amount(PowerId::Weak) > 0 {
        damage *= 0.75;
    }
    if player.power_amount(PowerId::Vulnerable) > 0 {
        damage *= if player.has_relic(RelicId::Odd_Mushroom) {
            1.25
        } else {
            1.5
        };
    }
    let mut damage = damage.floor().max(0.0) as i32;
    if player.power_amount(PowerId::Intangible) > 0 && damage > 1 {
        damage = 1;
    }
    damage * hits.max(1)
}

/// Damage pressure from the next deterministic script beat, up to three
/// monster turns ahead. Random move rolls deliberately stay out of this table.
fn scripted_incoming(player: &Player, monster: &Monster, horizon: i32) -> i32 {
    let (turns, base, hits, future_strength) = match monster.id {
        MonsterId::Hexaghost if monster.next_move == 5 => {
            (1, player.hp / 12 + 1, 6, 0) // Activate -> Divider
        }
        MonsterId::Hexaghost if (4..=5).contains(&monster.extra) => (
            6 - monster.extra,
            if monster.ascension >= 4 { 3 } else { 2 },
            6,
            0,
        ),
        // The approach always contains one Boost before HYPER BEAM.
        MonsterId::BronzeAutomaton if (3..=4).contains(&monster.extra) => (
            5 - monster.extra,
            if monster.ascension >= 4 { 50 } else { 45 },
            1,
            4,
        ),
        MonsterId::GiantHead if monster.extra <= 2 => {
            let start = if monster.ascension >= 3 { 40 } else { 30 };
            if monster.extra >= 1 {
                (monster.extra, start, 1, 0)
            } else {
                (1, start - (monster.extra - 1).max(-6) * 5, 1, 0)
            }
        }
        MonsterId::AwakenedOne if monster.half_dead || monster.next_move == 3 => {
            (1, 40, 1, 0) // Rebirth -> Dark Echo
        }
        MonsterId::CorruptHeart => match monster.next_move {
            3 | 1 => (1, 40, 1, 0), // Debilitate/Blood Shots -> Echo
            2 => (2, 2, 12, 2),     // Echo -> Buff -> Blood Shots
            4 => (1, 2, 12, 2),     // Buff -> Blood Shots
            _ => return 0,
        },
        // Mega Debuff itself deals no damage, but its exact timing creates a
        // two-turn defensive deadline before the next move roll.
        MonsterId::TheCollector
            if monster.extra == 2 && !monster.split_triggered && horizon >= 2 =>
        {
            (2, if monster.ascension >= 4 { 21 } else { 18 }, 1, 0)
        }
        _ => return 0,
    };
    if turns > horizon {
        0
    } else {
        projected_attack(player, monster, base, hits, future_strength)
    }
}

/// Block that can still exist on the scripted attack turn. Ordinary Defend
/// block is intentionally excluded; Frost, powers, and retention persist.
fn persistent_block_bank(game: &Game) -> i32 {
    let player = &game.player;
    let mut block = if player.power_amount(PowerId::Barricade) > 0 {
        player.block
    } else if player.has_relic(RelicId::Calipers) {
        (player.block - 15).max(0)
    } else {
        0
    };
    if block == 0 && player.has_relic(RelicId::Orichalcum) {
        block += 6;
    }
    block += player.power_amount(PowerId::Metallicize).max(0);
    block += player.power_amount(PowerId::PlatedArmor).max(0);

    let frost_passive = (2 + player.power_amount(PowerId::Focus)).max(0);
    let frozen_core_frost =
        player.has_relic(RelicId::FrozenCore) && (player.orbs.len() as i32) < player.max_orbs;
    let frost_orbs = player
        .orbs
        .iter()
        .filter(|orb| orb.kind == OrbKind::Frost)
        .count() as i32
        + i32::from(frozen_core_frost);
    block += frost_orbs * frost_passive;
    let front_is_frost = player
        .orbs
        .first()
        .is_some_and(|orb| orb.kind == OrbKind::Frost)
        || (player.orbs.is_empty() && frozen_core_frost);
    if front_is_frost && player.has_relic(RelicId::Cables) {
        block += frost_passive;
    }
    block
}

fn fight_kind(game: &Game) -> FightKind {
    if game.current_room == RoomType::Boss {
        return FightKind::Boss;
    }
    if game.current_room == RoomType::Elite {
        return FightKind::Elite;
    }
    if game.combat.as_ref().is_some_and(|combat| {
        combat.monsters.iter().any(|m| {
            matches!(
                m.id,
                MonsterId::CorruptHeart | MonsterId::SpireShield | MonsterId::SpireSpear
            )
        })
    }) {
        return FightKind::Boss;
    }
    FightKind::Normal
}

fn fight_length(kind: FightKind, act: Act) -> f32 {
    let p = params();
    match (act, kind) {
        (Act::Exordium, FightKind::Normal) => p.fl_a1_normal,
        (Act::Exordium, FightKind::Elite) => p.fl_a1_elite,
        (Act::Exordium, FightKind::Boss) => p.fl_a1_boss,
        (Act::City, FightKind::Normal) => p.fl_a2_normal,
        (Act::City, FightKind::Elite) => p.fl_a2_elite,
        (Act::City, FightKind::Boss) => p.fl_a2_boss,
        (Act::Beyond, FightKind::Normal) => p.fl_a3_normal,
        (Act::Beyond, FightKind::Elite) => p.fl_a3_elite,
        (Act::Beyond, FightKind::Boss) => p.fl_a3_boss,
        (Act::Ending, FightKind::Normal) => p.fl_a4_normal,
        (Act::Ending, FightKind::Elite) => p.fl_a4_elite,
        (Act::Ending, FightKind::Boss) => p.fl_a4_boss,
    }
}

fn target_priority(id: MonsterId) -> f32 {
    match id {
        MonsterId::Healer => 1.6,
        MonsterId::GremlinWizard => 1.5,
        MonsterId::Cultist => 1.5,
        MonsterId::BronzeOrb => 1.6,
        MonsterId::TorchHead => 1.5,
        MonsterId::Donu => 1.4,
        MonsterId::Deca => 0.9,
        MonsterId::GremlinTsundere => 0.8,
        MonsterId::Mugger | MonsterId::Looter => 1.15,
        MonsterId::Centurion => 0.9,
        MonsterId::Sentry => 1.25,
        MonsterId::SpireSpear => 1.45,
        MonsterId::SpireShield => 0.85,
        _ => 1.0,
    }
}

fn orb_value(game: &Game, turns_left: f32) -> f32 {
    let p = params();
    let focus = game.player.power_amount(PowerId::Focus);
    let horizon = turns_left.min(p.orb_horizon);
    game.player
        .orbs
        .iter()
        .map(|orb| match orb.kind {
            OrbKind::Lightning => (3 + focus).max(0) as f32 * horizon * p.orb_lightning_mult,
            OrbKind::Frost => (2 + focus).max(0) as f32 * horizon * p.orb_frost_mult,
            OrbKind::Dark => {
                orb.evoke.max(6) as f32 * p.orb_dark_stored
                    + (6 + focus).max(0) as f32 * horizon * p.orb_dark_growth
            }
            OrbKind::Plasma => p.orb_plasma,
        })
        .sum()
}

fn potion_policy(game: &Game, legal: &[Action]) -> Option<Action> {
    let hp = game.player.hp;
    let max_hp = game.player.max_hp;
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
    let find = |want: &[PotionId]| {
        legal
            .iter()
            .find(|action| {
                let Action::Potion {
                    action: crate::action::PotionOp::Use,
                    slot,
                    ..
                } = action
                else {
                    return false;
                };
                game.player
                    .potions
                    .get(*slot)
                    .is_some_and(|potion| want.contains(&potion.id))
            })
            .cloned()
    };

    if let Some(brew) = find(&[PotionId::EntropicBrew]) {
        let empty = game
            .player
            .potions
            .iter()
            .filter(|p| p.id == PotionId::Slot)
            .count();
        let min_empty = params().entropic_min_empty.max(1.0).round() as usize;
        if empty >= min_empty {
            return Some(brew);
        }
    }

    let opening_boss = matches!(fight_kind(game), FightKind::Boss)
        && game.combat.as_ref().is_some_and(|combat| combat.turn <= 1);
    if opening_boss {
        if hp < max_hp {
            if let Some(regen) = find(&[PotionId::Regen]) {
                return Some(regen);
            }
        }
        if let Some(setup) = find(&[
            PotionId::Focus,
            PotionId::Cultist,
            PotionId::Strength,
            PotionId::Dexterity,
            PotionId::EssenceOfSteel,
            PotionId::LiquidBronze,
            PotionId::PotionOfCapacity,
            PotionId::EssenceOfDarkness,
            PotionId::Power,
            PotionId::Attack,
            PotionId::Skill,
            PotionId::Colorless,
            PotionId::Energy,
            PotionId::Swift,
            PotionId::BlessingOfTheForge,
            PotionId::DistilledChaos,
        ]) {
            return Some(setup);
        }
        if let Some(boss_debuff) = find(&[PotionId::Fear, PotionId::Weak]) {
            return Some(boss_debuff);
        }
    }

    const DEFENSE: &[PotionId] = &[
        PotionId::Block,
        PotionId::EssenceOfSteel,
        PotionId::HeartOfIron,
        PotionId::Weak,
        PotionId::Dexterity,
    ];
    const HEAL: &[PotionId] = &[PotionId::Blood, PotionId::FruitJuice, PotionId::Regen];
    const OFFENSE: &[PotionId] = &[
        PotionId::Fire,
        PotionId::Explosive,
        PotionId::Strength,
        PotionId::Steroid,
        PotionId::Cultist,
        PotionId::Attack,
        PotionId::Power,
        PotionId::Fear,
        PotionId::Focus,
        PotionId::PotionOfCapacity,
        PotionId::Energy,
    ];

    let total_enemy_hp: i32 = game
        .combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| monster.alive())
                .map(|monster| monster.hp)
                .sum()
        })
        .unwrap_or(0);

    if unblocked >= hp || hp <= (max_hp as f32 / params().potion_desperate_hp_div) as i32 {
        return find(DEFENSE)
            .or_else(|| find(HEAL))
            .or_else(|| find(OFFENSE));
    }
    if hp <= (max_hp as f32 / params().potion_defense_hp_div) as i32 && unblocked > 0 {
        if total_enemy_hp <= 30 {
            if let Some(offense) = find(OFFENSE) {
                return Some(offense);
            }
        }
        if let Some(defense) = find(DEFENSE) {
            return Some(defense);
        }
    }
    let turn = game.combat.as_ref().map(|combat| combat.turn).unwrap_or(0);
    let heart = game.combat.as_ref().is_some_and(|combat| {
        combat
            .monsters
            .iter()
            .any(|monster| monster.alive() && monster.id == MonsterId::CorruptHeart)
    });
    let boss_dump = matches!(fight_kind(game), FightKind::Boss)
        && ((heart && turn >= 2)
            || (turn as f32 >= params().potion_boss_dump_turn
                && total_enemy_hp as f32 <= params().potion_boss_dump_hp));
    if boss_dump {
        if let Some(offense) = find(OFFENSE) {
            return Some(offense);
        }
    }
    if matches!(fight_kind(game), FightKind::Elite | FightKind::Boss) {
        if hp < (max_hp as f32 * params().potion_heal_hp_frac) as i32 {
            if let Some(heal) = find(HEAL) {
                return Some(heal);
            }
        }
        if unblocked
            >= (params().potion_block_min as i32)
                .max((hp as f32 / params().potion_block_hp_div) as i32)
        {
            if let Some(defense) = find(DEFENSE) {
                return Some(defense);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::card::Card;
    use crate::combat::spawn_monster;
    use crate::creature::{Orb, Player, RelicInstance};
    use crate::ids::Character;
    use crate::rng::RngSet;
    use crate::Unlocks;

    #[test]
    fn measured_fight_lengths_are_act_specific() {
        assert_eq!(
            fight_length(FightKind::Normal, Act::Exordium),
            params().fl_a1_normal
        );
        assert_eq!(
            fight_length(FightKind::Normal, Act::City),
            params().fl_a2_normal
        );
        assert_eq!(
            fight_length(FightKind::Boss, Act::Ending),
            params().fl_a4_boss
        );
        assert_ne!(
            fight_length(FightKind::Normal, Act::Exordium),
            fight_length(FightKind::Normal, Act::City)
        );
    }

    #[test]
    fn effective_end_of_turn_block_includes_frost_powers_and_relics() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.block = 0;
        game.player.add_power(PowerId::Focus, 2);
        game.player.add_power(PowerId::Metallicize, 3);
        game.player.add_power(PowerId::PlatedArmor, 4);
        game.player.orbs = vec![Orb {
            kind: OrbKind::Frost,
            evoke: 5,
        }];
        game.player.relics.extend([
            RelicInstance {
                id: RelicId::Orichalcum,
                counter: -1,
                used_up: false,
            },
            RelicInstance {
                id: RelicId::Cables,
                counter: -1,
                used_up: false,
            },
            RelicInstance {
                id: RelicId::FrozenCore,
                counter: -1,
                used_up: false,
            },
        ]);

        // Orichalcum 6 + Metallicize 3 + Plated Armor 4 + two Frost passives
        // (existing + Frozen Core) at 4 each + Cables repeating the front 4.
        assert_eq!(end_of_turn_block(&game), 25);
    }

    #[test]
    fn orichalcum_does_not_trigger_when_block_is_already_present() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.block = 1;
        game.player.relics.push(RelicInstance {
            id: RelicId::Orichalcum,
            counter: -1,
            used_up: false,
        });

        assert_eq!(end_of_turn_block(&game), 1);
    }

    #[test]
    fn turn_state_hash_tracks_combat_and_rng_state() {
        let game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        let same = game.clone();
        assert_eq!(turn_state_hash(&game), turn_state_hash(&same));

        let mut different_energy = game.clone();
        different_energy.player.energy += 1;
        assert_ne!(turn_state_hash(&game), turn_state_hash(&different_energy));

        let mut different_rng = game.clone();
        let _ = different_rng.rng.card.random_int(10);
        assert_ne!(turn_state_hash(&game), turn_state_hash(&different_rng));
    }

    #[test]
    fn cheap_hand_block_respects_energy_and_x_costs() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.energy = 3;
        game.player.hand = vec![
            Card::new(CardId::Defend_B),
            Card::new(CardId::Reinforced_Body),
        ];

        assert_eq!(cheap_hand_block(&game), 21);

        game.player.energy = 1;
        assert_eq!(cheap_hand_block(&game), 7);
    }

    #[test]
    fn sentries_and_spear_are_priority_targets() {
        assert_eq!(target_priority(MonsterId::Sentry), 1.25);
        assert!(target_priority(MonsterId::SpireSpear) > target_priority(MonsterId::SpireShield));
        assert!(
            target_priority(MonsterId::BronzeOrb) > target_priority(MonsterId::BronzeAutomaton)
        );
        assert!(target_priority(MonsterId::Cultist) > target_priority(MonsterId::AwakenedOne));
        assert!(target_priority(MonsterId::Donu) > target_priority(MonsterId::Deca));
    }

    #[test]
    fn rebound_target_score_prefers_a_high_value_repeat() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Glacier)];
        game.player.add_power(PowerId::Rebound, 1);
        let strike = Action::Play {
            hand_index: 0,
            target_index: Some(0),
        };
        let glacier = Action::Play {
            hand_index: 1,
            target_index: None,
        };
        assert!(rebound_play_value(&game, &glacier) > rebound_play_value(&game, &strike));
    }

    #[test]
    fn wounded_boss_fight_values_self_repair_before_chip_damage() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        game.current_room = RoomType::Boss;
        game.combat = Some(Combat::start(
            EncounterId::Champ,
            &mut game.player,
            &mut game.rng,
            31,
            2,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.hp = 20;
        game.player.energy = 1;
        game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Self_Repair)];

        let legal = game.legal_actions();
        assert_eq!(
            plan_turn(&game, &legal),
            Action::Play {
                hand_index: 1,
                target_index: None,
            }
        );
    }

    #[test]
    fn safe_boss_fight_values_echo_form_before_chip_damage() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        game.current_room = RoomType::Boss;
        game.combat = Some(Combat::start(
            EncounterId::Champ,
            &mut game.player,
            &mut game.rng,
            31,
            2,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.energy = 3;
        game.combat.as_mut().unwrap().monsters[0].intent_damage = 0;
        game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Echo_Form)];

        let legal = game.legal_actions();
        assert_eq!(
            plan_turn(&game, &legal),
            Action::Play {
                hand_index: 1,
                target_index: None,
            }
        );
    }

    #[test]
    fn exact_lethal_orders_vulnerable_before_damage() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut game.player,
            &mut game.rng,
            31,
            1,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.energy = 2;
        game.player.hand = vec![Card::new(CardId::Beam_Cell), Card::new(CardId::Strike_B)];
        let monster = &mut game.combat.as_mut().unwrap().monsters[0];
        monster.hp = 10;
        monster.block = 0;

        let legal = game.legal_actions();
        assert_eq!(
            exact_attack_lethal(&game, &legal),
            Some(Action::Play {
                hand_index: 0,
                target_index: Some(0),
            })
        );
        assert_eq!(
            plan_turn(&game, &legal),
            exact_attack_lethal(&game, &legal).unwrap()
        );
    }

    #[test]
    fn boss_opening_spends_long_duration_potions() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.current_room = RoomType::Boss;
        game.combat = Some(Combat::start(
            EncounterId::Champ,
            &mut game.player,
            &mut game.rng,
            31,
            2,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.potions[0].id = PotionId::Focus;
        let legal = game.legal_actions();

        assert_eq!(
            potion_policy(&game, &legal),
            Some(Action::Potion {
                action: crate::action::PotionOp::Use,
                slot: 0,
                target_index: None,
            })
        );
    }

    #[test]
    fn late_boss_dump_spends_offense_below_the_hp_threshold() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.current_room = RoomType::Boss;
        game.combat = Some(Combat::start(
            EncounterId::Hexaghost,
            &mut game.player,
            &mut game.rng,
            31,
            1,
            20,
        ));
        game.screen = Screen::Combat;
        game.player.potions[0].id = PotionId::Fire;
        let combat = game.combat.as_mut().unwrap();
        combat.turn = 2;
        combat.monsters[0].hp = 100;

        assert_eq!(potion_policy(&game, &game.legal_actions()), None);

        game.combat.as_mut().unwrap().turn = 3;
        assert!(matches!(
            potion_policy(&game, &game.legal_actions()),
            Some(Action::Potion {
                action: crate::action::PotionOp::Use,
                slot: 0,
                ..
            })
        ));
    }

    #[test]
    fn projected_incoming_accounts_for_strength_weak_and_vulnerable() {
        let mut rng = RngSet::generate_seeds(2);
        let mut monster = spawn_monster(MonsterId::Cultist, &mut rng, 0);
        monster.intent_damage = 20;
        monster.intent_hits = 2;
        monster.add_power(PowerId::Strength, 3);
        monster.add_power(PowerId::Weak, 1);
        let mut player = Player::defect();
        player.add_power(PowerId::Vulnerable, 1);
        assert_eq!(projected_incoming(&player, &monster), 50);
    }

    #[test]
    fn status_gain_penalty_prices_combat_pollution() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut before = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        before.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut before.player,
            &mut before.rng,
            1,
            2,
            20,
        ));
        before.screen = Screen::Combat;
        let unchanged = before.clone();
        let mut polluted = before.clone();
        polluted.player.discard.push(Card::new(CardId::Dazed));

        assert_eq!(status_card_count(&before.player), 0);
        assert_eq!(status_card_count(&polluted.player), 1);
        assert!(score_state(&before, &polluted) < score_state(&before, &unchanged));
    }

    #[test]
    fn lagavulin_wake_guard_allows_only_kill_proximity() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut before = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        before.combat = Some(Combat::start(
            EncounterId::Lagavulin,
            &mut before.player,
            &mut before.rng,
            7,
            2,
            20,
        ));
        before.screen = Screen::Combat;
        let mut asleep = before.clone();
        let asleep_monster = &mut asleep.combat.as_mut().unwrap().monsters[0];
        asleep_monster.hp -= 10;
        let mut woken = asleep.clone();
        woken.combat.as_mut().unwrap().monsters[0].extra = 3;
        assert!(score_state(&before, &woken) < score_state(&before, &asleep));

        let mut near_lethal = woken.clone();
        near_lethal.combat.as_mut().unwrap().monsters[0].hp = 20;
        assert!(score_state(&before, &near_lethal) > score_state(&before, &woken));
    }

    #[test]
    fn scripted_incoming_tracks_boss_deadlines() {
        let mut rng = RngSet::generate_seeds(2);
        let mut player = Player::defect();
        player.hp = 60;

        let mut hexaghost = spawn_monster(MonsterId::Hexaghost, &mut rng, 20);
        hexaghost.next_move = 5;
        assert_eq!(scripted_incoming(&player, &hexaghost, 1), 36);

        let mut automaton = spawn_monster(MonsterId::BronzeAutomaton, &mut rng, 20);
        automaton.extra = 3;
        assert_eq!(scripted_incoming(&player, &automaton, 1), 0);
        assert_eq!(scripted_incoming(&player, &automaton, 2), 54);

        let mut head = spawn_monster(MonsterId::GiantHead, &mut rng, 20);
        head.extra = 2;
        assert_eq!(scripted_incoming(&player, &head, 2), 40);

        let mut awakened = spawn_monster(MonsterId::AwakenedOne, &mut rng, 20);
        awakened.half_dead = true;
        awakened.next_move = 3;
        assert_eq!(scripted_incoming(&player, &awakened, 1), 40);
    }

    #[test]
    fn persistent_block_bank_excludes_ordinary_block() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.block = 20;
        game.player.orbs = vec![Orb {
            kind: OrbKind::Frost,
            evoke: 5,
        }];
        assert_eq!(persistent_block_bank(&game), 2);

        game.player.relics.push(RelicInstance {
            id: RelicId::Calipers,
            counter: -1,
            used_up: false,
        });
        assert_eq!(persistent_block_bank(&game), 7);
    }

    #[test]
    fn stripping_enemy_block_has_tactical_value() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut before = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        before.combat = Some(Combat::start(
            EncounterId::SphericGuardian,
            &mut before.player,
            &mut before.rng,
            36,
            3,
            0,
        ));
        before.screen = Screen::Combat;
        before.combat.as_mut().unwrap().monsters[0].block = 20;
        let unchanged = before.clone();
        let mut stripped = before.clone();
        stripped.combat.as_mut().unwrap().monsters[0].block = 10;

        assert!(score_state(&before, &stripped) > score_state(&before, &unchanged));
    }

    #[test]
    fn biased_cognition_waits_for_champs_second_phase() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.combat = Some(Combat::start(
            EncounterId::Champ,
            &mut game.player,
            &mut game.rng,
            31,
            2,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.hand = vec![Card::new(CardId::Biased_Cognition)];
        let play = Action::Play {
            hand_index: 0,
            target_index: None,
        };
        assert!(setup_play_value(&game, &play) < -1_000.0);

        let champ = &mut game.combat.as_mut().unwrap().monsters[0];
        champ.hp = champ.max_hp / 2 - 1;
        assert_eq!(setup_play_value(&game, &play), 0.0);
    }
}
