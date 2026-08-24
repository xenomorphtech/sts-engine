use crate::action::Action;
use crate::card::Card;
use crate::creature::{Monster, OrbKind, Player};
use crate::game::{CombatSearchCheckpoint, CombatSearchKey, CombatSearchState, Game, Screen};
use crate::ids::{Act, CardId, CardType, MonsterId, PotionId, PowerId, RelicId, RoomType};
use std::collections::{HashMap, VecDeque};
use std::ops::AddAssign;

use super::params::params;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FightKind {
    Normal,
    Elite,
    Boss,
}

/// Deterministic work performed while choosing combat actions.
///
/// These counters deliberately live on the caller's stack. They do not use
/// atomics or locks in the search hot path and can be aggregated after a seed
/// finishes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    pub plan_calls: u64,
    pub simulated_steps: u64,
    pub expanded_nodes: u64,
    pub score_evaluations: u64,
    pub lethal_expansions: u64,
    pub dedup_hits: u64,
}

impl AddAssign for SearchStats {
    fn add_assign(&mut self, rhs: Self) {
        self.plan_calls += rhs.plan_calls;
        self.simulated_steps += rhs.simulated_steps;
        self.expanded_nodes += rhs.expanded_nodes;
        self.score_evaluations += rhs.score_evaluations;
        self.lethal_expansions += rhs.lethal_expansions;
        self.dedup_hits += rhs.dedup_hits;
    }
}

/// Arena-backed exact-state set. The compact key only selects a collision
/// bucket; equality of the complete typed state decides deduplication.
struct ExactStateSet {
    states: Vec<CombatSearchState>,
    buckets: HashMap<CombatSearchKey, Vec<usize>>,
}

impl ExactStateSet {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            states: Vec::with_capacity(capacity),
            buckets: HashMap::with_capacity(capacity),
        }
    }

    fn insert(&mut self, state: CombatSearchState) -> Result<usize, usize> {
        let key = state.bucket_key();
        if let Some(indices) = self.buckets.get(&key) {
            if let Some(index) = indices
                .iter()
                .copied()
                .find(|index| self.states[*index].exact_eq(&state))
            {
                return Err(index);
            }
        }
        let index = self.states.len();
        self.states.push(state);
        self.buckets.entry(key).or_default().push(index);
        Ok(index)
    }

    fn get(&self, index: usize) -> &CombatSearchState {
        &self.states[index]
    }
}

fn simulated_step(game: &mut Game, action: &Action, stats: &mut SearchStats) {
    game.step(action);
    stats.simulated_steps += 1;
}

struct EvaluationContext<'a> {
    before: &'a Game,
    turns_left: f32,
    damage_weight: f32,
    before_orb_value: f32,
    before_genetic_training: Option<i32>,
}

impl<'a> EvaluationContext<'a> {
    fn new(before: &'a Game) -> Self {
        let turns_left = fight_length(fight_kind(before), before.dungeon.act);
        let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
        let before_orb_value = orb_value(before, turns_left, damage_weight);
        let training = genetic_training(&before.player);
        let before_genetic_training = (training > 0).then_some(training);
        Self {
            before,
            turns_left,
            damage_weight,
            before_orb_value,
            before_genetic_training,
        }
    }
}

fn evaluated_score(context: &EvaluationContext<'_>, after: &Game, stats: &mut SearchStats) -> f32 {
    stats.score_evaluations += 1;
    let tactical = score_state_with_context(context, after);
    let same_turn = context.before.combat.as_ref().map(|combat| combat.turn)
        == after.combat.as_ref().map(|combat| combat.turn);
    if after.player.hp <= 0 || (same_turn && projected_end_hp(after) <= 0) {
        tactical
    } else {
        tactical + setup_state_value(context, after)
    }
}

const DOOMED_SCORE: f32 = -1_000_000.0;
const DOOMED_SURVIVAL_STEP: f32 = 1_000.0;
const DOOMED_DAMAGE_TIEBREAK: f32 = 100.0;

/// Preserve useful ordering after every available line is lethal. A one-HP
/// improvement in the projected survival margin dominates all damage, while
/// damage still breaks ties between equally survivable losing lines.
fn doomed_score(context: &EvaluationContext<'_>, after: &Game, projected_hp: i32) -> f32 {
    let damage = context.before.combat.as_ref().map_or(0.0, |before_combat| {
        before_combat
            .monsters
            .iter()
            .enumerate()
            .filter(|(_, monster)| monster.alive())
            .map(|(index, monster)| {
                let hp_after = after
                    .combat
                    .as_ref()
                    .and_then(|combat| combat.monsters.get(index))
                    .map_or(0, |after_monster| after_monster.hp.max(0));
                (monster.hp.max(0) - hp_after).max(0) as f32
            })
            .sum::<f32>()
    });
    let damage_tiebreak = if damage > 0.0 {
        DOOMED_DAMAGE_TIEBREAK * damage / (damage + 1.0)
    } else {
        0.0
    };
    DOOMED_SCORE + projected_hp.min(0) as f32 * DOOMED_SURVIVAL_STEP + damage_tiebreak
}

fn projected_end_hp(game: &Game) -> i32 {
    let incoming = game.combat.as_ref().map_or(0, |combat| {
        combat
            .monsters
            .iter()
            .filter(|monster| monster.alive())
            .map(|monster| projected_incoming(&game.player, monster))
            .sum::<i32>()
    });
    game.player.hp - (incoming - end_of_turn_block(game)).max(0)
}

fn evaluated_after_end(
    context: &EvaluationContext<'_>,
    after: &Game,
    projected_hp: i32,
    stats: &mut SearchStats,
) -> f32 {
    if after.player.hp <= 0 {
        stats.score_evaluations += 1;
        doomed_score(context, after, projected_hp)
    } else if let Some(reset_score) = evaluated_time_warp_reset(context, after, stats) {
        reset_score
    } else {
        evaluated_score(context, after, stats)
    }
}

/// An EndTurn that preserves Time Eater's counter at eleven does not expose a
/// normal next hand: the first card played will grant Strength and immediately
/// run the enemy turn. Score that forced one-card turn exactly instead of
/// crediting the whole hand through the ordinary continuation heuristics.
fn evaluated_time_warp_reset(
    context: &EvaluationContext<'_>,
    after: &Game,
    stats: &mut SearchStats,
) -> Option<f32> {
    let time_eater = after.combat.as_ref()?.monsters.iter().find(|monster| {
        monster.id == MonsterId::TimeEater && monster.alive() && monster.extra == 11
    })?;
    debug_assert_eq!(time_eater.extra, 11);

    let mut best: Option<f32> = None;
    for action in after
        .legal_actions()
        .into_iter()
        .filter(|action| matches!(action, Action::Play { .. }))
    {
        let mut reset = after.clone();
        simulated_step(&mut reset, &action, stats);
        resolve_grid_selects(&mut reset, stats);
        let score = evaluated_score(context, &reset, stats);
        best = Some(best.map_or(score, |current| current.max(score)));
    }
    best
}

/// Pick the combat command with one shared search over the rest of the turn.
///
/// Newly reached exact states form the next frontier, while a search-wide fact
/// table prevents equivalent card orders from being expanded more than once.
/// Each state retains the first action that reached it so the winning branch
/// can be returned without keeping a separate beam per first play.
pub fn plan_turn(game: &Game, legal: &[Action]) -> Action {
    plan_turn_with_stats(game, legal).0
}

pub fn plan_turn_with_stats(game: &Game, legal: &[Action]) -> (Action, SearchStats) {
    let mut stats = SearchStats {
        plan_calls: 1,
        ..SearchStats::default()
    };
    if let Some(potion) = potion_policy(game, legal) {
        return (potion, stats);
    }
    let plays: Vec<&Action> = legal
        .iter()
        .filter(|a| matches!(a, Action::Play { .. }))
        .collect();
    if plays.is_empty() {
        let action = legal
            .iter()
            .find(|a| matches!(a, Action::EndTurn))
            .cloned()
            .unwrap_or_else(|| legal[0].clone());
        return (action, stats);
    }
    let checkpoint = game.combat_search_checkpoint();
    let mut scratch = game.clone();
    if let Some(lethal) = exact_attack_lethal(game, legal, &checkpoint, &mut scratch, &mut stats) {
        return (lethal, stats);
    }
    if let Some(lethal) =
        exact_mixed_turn_lethal(game, legal, &checkpoint, &mut scratch, &mut stats)
    {
        return (lethal, stats);
    }
    let action = searched_turn(game, legal, &checkpoint, &mut scratch, &mut stats)
        .unwrap_or_else(|| plays[0].clone());
    (action, stats)
}

const EXACT_LETHAL_NODE_BUDGET: usize = 20_000;
const EXACT_MIXED_LETHAL_NODE_BUDGET: usize = 2_000;
const EXACT_MIXED_LETHAL_EHP_CAP: i32 = 48;

/// Find a proven same-turn kill before the bounded heuristic beam runs.
///
/// This deliberately searches only Attack plays. That keeps the branch factor
/// small enough for a hard per-decision budget, while covering the common case
/// where card order, Vulnerable, or target order separates lethal from a miss.
fn exact_attack_lethal(
    game: &Game,
    legal: &[Action],
    checkpoint: &CombatSearchCheckpoint,
    scratch: &mut Game,
    stats: &mut SearchStats,
) -> Option<Action> {
    let root = checkpoint.root();
    let turn = game.combat.as_ref()?.turn;
    let target_ehp = living_enemy_ehp(game);
    if target_ehp <= 0
        || optimistic_attack_damage(game, legal, checkpoint, scratch, stats) < target_ehp
    {
        return None;
    }

    let mut queue = VecDeque::new();
    let mut seen = ExactStateSet::with_capacity(EXACT_LETHAL_NODE_BUDGET.min(1024));
    seen.insert(root.clone())
        .expect("an empty exact-state arena accepts its root");
    let mut expanded = 0usize;
    for first in legal.iter().filter(|action| attack_play(game, action)) {
        if expanded >= EXACT_LETHAL_NODE_BUDGET {
            break;
        }
        expanded += 1;
        stats.lethal_expansions += 1;
        scratch.restore_combat_search_state(checkpoint, root);
        simulated_step(scratch, first, stats);
        resolve_grid_selects(scratch, stats);
        if combat_won(scratch) {
            return Some(first.clone());
        }
        if same_combat_turn(scratch, turn) {
            match seen.insert(scratch.combat_search_state()) {
                Ok(index) => queue.push_back((index, first.clone())),
                Err(_) => stats.dedup_hits += 1,
            }
        }
    }

    while let Some((state_index, first)) = queue.pop_front() {
        scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
        let actions: Vec<_> = scratch
            .legal_actions()
            .into_iter()
            .filter(|action| attack_play(scratch, action))
            .collect();
        for action in actions {
            if expanded >= EXACT_LETHAL_NODE_BUDGET {
                return None;
            }
            expanded += 1;
            stats.lethal_expansions += 1;
            scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
            simulated_step(scratch, &action, stats);
            resolve_grid_selects(scratch, stats);
            if combat_won(scratch) {
                return Some(first);
            }
            if same_combat_turn(scratch, turn) {
                match seen.insert(scratch.combat_search_state()) {
                    Ok(index) => queue.push_back((index, first.clone())),
                    Err(_) => stats.dedup_hits += 1,
                }
            }
        }
    }
    None
}

/// Find kills that need a Skill/Power setup or the exact end-of-turn orb
/// sequence. The wider action set is reserved for short kill windows so the
/// ordinary per-decision search budget remains stable.
fn exact_mixed_turn_lethal(
    game: &Game,
    legal: &[Action],
    checkpoint: &CombatSearchCheckpoint,
    scratch: &mut Game,
    stats: &mut SearchStats,
) -> Option<Action> {
    if living_enemy_ehp(game) > EXACT_MIXED_LETHAL_EHP_CAP {
        return None;
    }
    let root = checkpoint.root();
    let turn = game.combat.as_ref()?.turn;

    if let Some(end) = legal
        .iter()
        .find(|action| matches!(action, Action::EndTurn))
    {
        scratch.restore_combat_search_state(checkpoint, root);
        simulated_step(scratch, end, stats);
        stats.lethal_expansions += 1;
        if combat_won(scratch) {
            return Some(end.clone());
        }
    }

    let mut queue = VecDeque::new();
    let mut seen = ExactStateSet::with_capacity(EXACT_MIXED_LETHAL_NODE_BUDGET.min(1024));
    seen.insert(root.clone())
        .expect("an empty mixed-lethal arena accepts its root");
    let mut expanded = 0usize;
    for first in legal
        .iter()
        .filter(|action| matches!(action, Action::Play { .. }))
    {
        if expanded >= EXACT_MIXED_LETHAL_NODE_BUDGET {
            break;
        }
        expanded += 1;
        stats.lethal_expansions += 1;
        scratch.restore_combat_search_state(checkpoint, root);
        let status_baseline = StatusPlayBaseline::capture(scratch, first);
        simulated_step(scratch, first, stats);
        resolve_grid_selects(scratch, stats);
        if status_baseline.is_non_progressing(scratch) {
            continue;
        }
        if combat_won(scratch) {
            return Some(first.clone());
        }
        if same_combat_turn(scratch, turn) {
            match seen.insert(scratch.combat_search_state()) {
                Ok(index) => queue.push_back((index, first.clone())),
                Err(_) => stats.dedup_hits += 1,
            }
        }
    }

    while let Some((state_index, first)) = queue.pop_front() {
        scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
        let actions = scratch.legal_actions();
        if let Some(end) = actions
            .iter()
            .find(|action| matches!(action, Action::EndTurn))
        {
            scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
            simulated_step(scratch, end, stats);
            expanded += 1;
            stats.lethal_expansions += 1;
            if combat_won(scratch) {
                return Some(first);
            }
        }
        for action in actions
            .iter()
            .filter(|action| matches!(action, Action::Play { .. }))
        {
            if expanded >= EXACT_MIXED_LETHAL_NODE_BUDGET {
                return None;
            }
            expanded += 1;
            stats.lethal_expansions += 1;
            scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
            let status_baseline = StatusPlayBaseline::capture(scratch, action);
            simulated_step(scratch, action, stats);
            resolve_grid_selects(scratch, stats);
            if status_baseline.is_non_progressing(scratch) {
                continue;
            }
            if combat_won(scratch) {
                return Some(first);
            }
            if same_combat_turn(scratch, turn) {
                match seen.insert(scratch.combat_search_state()) {
                    Ok(index) => queue.push_back((index, first.clone())),
                    Err(_) => stats.dedup_hits += 1,
                }
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
fn optimistic_attack_damage(
    game: &Game,
    legal: &[Action],
    checkpoint: &CombatSearchCheckpoint,
    scratch: &mut Game,
    stats: &mut SearchStats,
) -> i32 {
    let root = checkpoint.root();
    let before = living_enemy_ehp(game);
    let mut best_by_hand = vec![0; game.player.hand.len()];
    for action in legal.iter().filter(|action| attack_play(game, action)) {
        let Action::Play { hand_index, .. } = action else {
            continue;
        };
        scratch.restore_combat_search_state(checkpoint, root);
        simulated_step(scratch, action, stats);
        resolve_grid_selects(scratch, stats);
        let dealt = if combat_won(scratch) {
            before
        } else {
            before.saturating_sub(living_enemy_ehp(scratch))
        };
        best_by_hand[*hand_index] = best_by_hand[*hand_index].max(dealt);
    }
    best_by_hand.into_iter().sum::<i32>().saturating_mul(2)
}

/// One semi-naive turn search. Reached states live in one global fact table;
/// the frontier contains only newly admitted states, and provenance records
/// which root action produced each fact.
fn searched_turn(
    origin: &Game,
    root_legal: &[Action],
    checkpoint: &CombatSearchCheckpoint,
    scratch: &mut Game,
    stats: &mut SearchStats,
) -> Option<Action> {
    let root = checkpoint.root();
    let evaluation = EvaluationContext::new(origin);
    let width = params().search_width.round().max(1.0) as usize;
    let depth = params().search_depth.round().max(1.0) as usize;
    let mut seen = ExactStateSet::with_capacity(width.saturating_mul(depth + 1).saturating_mul(12));
    let root_index = seen
        .insert(root.clone())
        .expect("an empty turn fact table accepts its root");
    debug_assert_eq!(root_index, 0);
    let mut first_actions: Vec<Option<Action>> = vec![None];
    let mut best_play: Option<(Action, f32)> = None;

    // Keep the historical small bias against ending a playable turn at the
    // root. EndTurn descendants compete normally through their provenance.
    let root_end = root_legal
        .iter()
        .find(|action| matches!(action, Action::EndTurn))
        .map(|end| {
            scratch.restore_combat_search_state(checkpoint, root);
            let projected_hp = projected_end_hp(scratch);
            simulated_step(scratch, end, stats);
            (
                end.clone(),
                evaluated_after_end(&evaluation, scratch, projected_hp, stats),
            )
        });

    let mut frontier = Vec::new();
    for first in root_legal
        .iter()
        .filter(|action| matches!(action, Action::Play { .. }))
    {
        scratch.restore_combat_search_state(checkpoint, root);
        let status_baseline = StatusPlayBaseline::capture(scratch, first);
        simulated_step(scratch, first, stats);
        resolve_grid_selects(scratch, stats);
        if status_baseline.is_non_progressing(scratch) {
            continue;
        }
        if scratch.screen != Screen::Combat
            || scratch.player.hp <= 0
            || scratch
                .combat
                .as_ref()
                .is_some_and(|combat| combat.all_dead())
        {
            keep_best(
                &mut best_play,
                first,
                evaluated_score(&evaluation, scratch, stats),
            );
            continue;
        }
        match seen.insert(scratch.combat_search_state()) {
            Ok(index) => {
                debug_assert_eq!(index, first_actions.len());
                first_actions.push(Some(first.clone()));
                let score = evaluated_score(&evaluation, scratch, stats);
                frontier.push((index, score));
            }
            Err(_) => stats.dedup_hits += 1,
        }
    }
    frontier.sort_by(|(_, a_score), (_, b_score)| b_score.total_cmp(a_score));
    frontier.truncate(width);

    for _ in 0..depth {
        let current = std::mem::take(&mut frontier);
        let mut next = Vec::new();
        for (state_index, _) in current {
            stats.expanded_nodes += 1;
            scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
            let first = first_actions[state_index]
                .as_ref()
                .expect("non-root facts have provenance")
                .clone();
            let legal = scratch.legal_actions();
            if let Some(end) = legal
                .iter()
                .find(|action| matches!(action, Action::EndTurn))
            {
                scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
                let projected_hp = projected_end_hp(scratch);
                simulated_step(scratch, end, stats);
                keep_best(
                    &mut best_play,
                    &first,
                    evaluated_after_end(&evaluation, scratch, projected_hp, stats),
                );
            }
            for play in legal
                .iter()
                .filter(|action| matches!(action, Action::Play { .. }))
            {
                scratch.restore_combat_search_state(checkpoint, seen.get(state_index));
                let status_baseline = StatusPlayBaseline::capture(scratch, play);
                simulated_step(scratch, play, stats);
                resolve_grid_selects(scratch, stats);
                if status_baseline.is_non_progressing(scratch) {
                    continue;
                }
                if scratch.screen != Screen::Combat
                    || scratch.player.hp <= 0
                    || scratch
                        .combat
                        .as_ref()
                        .is_some_and(|combat| combat.all_dead())
                {
                    keep_best(
                        &mut best_play,
                        &first,
                        evaluated_score(&evaluation, scratch, stats),
                    );
                    continue;
                }
                match seen.insert(scratch.combat_search_state()) {
                    Ok(index) => {
                        debug_assert_eq!(index, first_actions.len());
                        first_actions.push(Some(first.clone()));
                        let score = evaluated_score(&evaluation, scratch, stats);
                        next.push((index, score));
                    }
                    Err(_) => stats.dedup_hits += 1,
                }
            }
        }
        if next.is_empty() {
            break;
        }
        next.sort_by(|(_, a_score), (_, b_score)| b_score.total_cmp(a_score));
        next.truncate(width);
        frontier = next;
    }

    // The depth horizon is itself a valid continuation value.
    for (state_index, score) in frontier {
        let first = first_actions[state_index]
            .as_ref()
            .expect("frontier facts have provenance");
        keep_best(&mut best_play, first, score);
    }
    match (best_play, root_end) {
        (Some((play, play_score)), Some((end, end_score))) if end_score > play_score + 5.0 => {
            Some(end)
        }
        (Some((play, _)), _) => Some(play),
        (None, Some((end, _))) => Some(end),
        (None, None) => None,
    }
}

fn keep_best(best: &mut Option<(Action, f32)>, first: &Action, score: f32) {
    if best
        .as_ref()
        .is_none_or(|(_, best_score)| score > *best_score)
    {
        *best = Some((first.clone(), score));
    }
}

/// Step through in-combat grid selections (Hologram, Seek, Secret Technique)
/// with the same policy the agent uses, so the turn search values those plays
/// by their resolved outcome instead of treating the grid screen as terminal.
fn resolve_grid_selects(game: &mut Game, stats: &mut SearchStats) {
    for _ in 0..8 {
        if game.screen != Screen::Grid {
            return;
        }
        let legal = game.legal_actions();
        if legal.is_empty() {
            return;
        }
        let choice = crate::htn::strategy::grid_choice(game, &legal);
        simulated_step(game, &choice, stats);
    }
}

struct StatusPlayBaseline {
    is_status: bool,
    player_hp: i32,
    player_block: i32,
    player_energy: i32,
    hand: Vec<crate::card::Card>,
    draw: Vec<crate::card::Card>,
    discard: Vec<crate::card::Card>,
    monsters: Vec<(i32, i32)>,
}

impl StatusPlayBaseline {
    fn capture(game: &Game, action: &Action) -> Self {
        let is_status = match action {
            Action::Play { hand_index, .. } => {
                game.player.hand.get(*hand_index).is_some_and(|card| {
                    matches!(card.card_type(), CardType::STATUS | CardType::CURSE)
                })
            }
            _ => false,
        };
        let (hand, draw, discard, monsters) = if is_status {
            (
                game.player.hand.to_vec(),
                game.player.draw.to_vec(),
                game.player.discard.to_vec(),
                game.combat
                    .as_ref()
                    .map(|combat| {
                        combat
                            .monsters
                            .iter()
                            .map(|monster| (monster.hp, monster.block))
                            .collect()
                    })
                    .unwrap_or_default(),
            )
        } else {
            (Vec::new(), Vec::new(), Vec::new(), Vec::new())
        };
        Self {
            is_status,
            player_hp: game.player.hp,
            player_block: game.player.block,
            player_energy: game.player.energy,
            hand,
            draw,
            discard,
            monsters,
        }
    }

    fn is_non_progressing(&self, after: &Game) -> bool {
        if !self.is_status {
            return false;
        }
        let same_monsters = after.combat.as_ref().is_some_and(|combat| {
            combat.monsters.len() == self.monsters.len()
                && combat
                    .monsters
                    .iter()
                    .zip(&self.monsters)
                    .all(|(monster, &(hp, block))| monster.hp == hp && monster.block == block)
        });
        same_monsters
            && self.player_hp == after.player.hp
            && self.player_block == after.player.block
            && self.player_energy == after.player.energy
            && self.hand == *after.player.hand
            && self.draw == *after.player.draw
            && self.discard == *after.player.discard
    }
}

fn rebound_card_value(game: &Game, card: &Card) -> f32 {
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
        // Redo is Recursion's internal id. These cards are weak repeats in a
        // vacuum but become premium top-decks when a valuable front orb is
        // deliberately being carried into the next turn.
        CardId::Redo | CardId::Multi_Cast => 24.0 + front_orb_tool_value(game, card),
        _ => 0.0,
    };
    let repeatable_output =
        card.base_damage.max(0) as f32 * 2.5 + card.base_block.max(0) as f32 * 2.0;
    (tactical + repeatable_output).clamp(8.0, 100.0)
}

/// Rebound is represented by the real pile transformation. Value the card at
/// the resulting top of the draw pile, so equal search states always have the
/// same value and need no path-local action log.
fn rebound_state_value(before: &Game, after: &Game) -> f32 {
    if after.player.power_amount(PowerId::Rebound) > 0 {
        return 0.0;
    }
    let rebound_was_reachable = before.player.power_amount(PowerId::Rebound) > 0
        || before
            .player
            .hand
            .iter()
            .any(|card| card.id == CardId::Rebound);
    if !rebound_was_reachable {
        return 0.0;
    }
    let Some(card) = after.player.draw.last() else {
        return 0.0;
    };
    let old_copies = before.player.draw.iter().filter(|old| *old == card).count();
    let new_copies = after.player.draw.iter().filter(|new| *new == card).count();
    if new_copies <= old_copies {
        return 0.0;
    }
    rebound_card_value(after, card)
}

/// Value Self Repair's delayed heal while deciding whether to spend energy on
/// it. The search cannot observe that payoff until combat ends, so it otherwise
/// skips the setup card for immediate chip damage.
#[cfg(test)]
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

/// Setup value belongs to the reached state, not the route used to reach it.
fn setup_state_value(context: &EvaluationContext<'_>, after: &Game) -> f32 {
    let before = context.before;
    let turns_left = remaining_fight_length(after);
    let damage_weight = context.damage_weight;
    let p = params();
    let hp_frac = after.player.hp as f32 / after.player.max_hp.max(1) as f32;
    let danger = (p.danger_base + p.danger_scale * (1.0 - hp_frac).powi(2))
        * (1.0 + after.ascension as f32 / 50.0);
    let repair = (after.player.power_amount(PowerId::SelfRepair)
        - before.player.power_amount(PowerId::SelfRepair))
    .max(0) as f32;
    let draw = (after.player.power_amount(PowerId::DrawCard)
        - before.player.power_amount(PowerId::DrawCard))
    .max(0) as f32;
    let echo = (after.player.power_amount(PowerId::EchoForm)
        - before.player.power_amount(PowerId::EchoForm))
    .max(0) as f32;
    let energized = (after.player.power_amount(PowerId::Energized)
        - before.player.power_amount(PowerId::Energized))
    .max(0) as f32;
    let artifact = (after.player.power_amount(PowerId::Artifact)
        - before.player.power_amount(PowerId::Artifact))
    .max(0) as f32;
    let buffer = (after.player.power_amount(PowerId::Buffer)
        - before.player.power_amount(PowerId::Buffer))
    .max(0) as f32;
    let static_discharge = (after.player.power_amount(PowerId::StaticDischarge)
        - before.player.power_amount(PowerId::StaticDischarge))
    .max(0) as f32;
    let genetic_growth = context
        .before_genetic_training
        .map(|before| (genetic_training(&after.player) - before).max(0) as f32)
        .unwrap_or(0.0);
    let mut value = repair * danger * 1.25;
    value += draw * turns_left * damage_weight * 2.2;
    value += echo * turns_left.max(1.0) * damage_weight * 12.0;
    value += energized * damage_weight * p.energized_weight;
    value += artifact * turns_left * damage_weight * p.artifact_weight;
    value += buffer * danger * p.buffer_weight;
    value += static_discharge * turns_left * damage_weight * p.static_discharge_weight;
    value += genetic_growth * p.genetic_growth_weight;
    value += rebound_state_value(before, after);
    let bias_gain =
        after.player.power_amount(PowerId::Bias) - before.player.power_amount(PowerId::Bias);
    if bias_gain > 0
        && before.combat.as_ref().is_some_and(|combat| {
            combat.monsters.iter().any(|monster| {
                monster.id == MonsterId::Champ
                    && !monster.split_triggered
                    && monster.hp >= monster.max_hp / 2
            })
        })
    {
        value -= 2_000.0;
    }
    value
}

fn genetic_training(player: &Player) -> i32 {
    player
        .deck
        .iter()
        .filter(|card| card.id == CardId::Genetic_Algorithm)
        .map(|card| card.misc.max(1) as i32)
        .sum()
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

/// Best immediately playable attack output in the real next hand. This is a
/// compact knapsack continuation value, not another search horizon: EndTurn
/// has already produced the exact retained/drawn cards and exact next energy.
fn cheap_hand_damage(game: &Game) -> i32 {
    let energy = game.player.energy.max(0) as usize;
    let mut best = vec![0; energy + 1];

    for card in &game.player.hand {
        if card.card_type() != CardType::ATTACK {
            continue;
        }
        let damage = crate::combat::derived_damage(card, &game.player).max(0);
        if damage == 0 || card.cost_for_turn < -1 {
            continue;
        }
        if card.cost_for_turn == -1 {
            let old = best.clone();
            for spent_before in 0..=energy {
                for x in 0..=energy - spent_before {
                    best[spent_before + x] =
                        best[spent_before + x].max(old[spent_before] + damage * x as i32);
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
                *value += damage;
            }
        } else {
            for spent in (cost..=energy).rev() {
                best[spent] = best[spent].max(best[spent - cost] + damage);
            }
        }
    }

    best.into_iter().max().unwrap_or(0)
}

#[cfg(test)]
fn score_state(before: &Game, after: &Game) -> f32 {
    let context = EvaluationContext::new(before);
    score_state_with_context(&context, after)
}

fn score_state_with_context(context: &EvaluationContext<'_>, after: &Game) -> f32 {
    let before = context.before;
    let turn_advanced =
        before.combat.as_ref().map(|c| c.turn) != after.combat.as_ref().map(|c| c.turn);
    if after.player.hp <= 0 {
        return doomed_score(context, after, 0);
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
    let turns_left = context.turns_left;
    let damage_weight = context.damage_weight;
    let mut dealt = 0.0;
    let mut stripped_block = 0.0;
    let mut dead = 0;
    let mut phase_transitions = 0;
    let mut laga_wake_penalty = 0.0;
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        for (index, monster) in before_combat.monsters.iter().enumerate() {
            if !monster.alive() {
                continue;
            }
            let hp_after = after_combat.monsters.get(index).map_or(0, |m| m.hp.max(0));
            let hp_damage = (monster.hp - hp_after).max(0);
            let priority = encounter_target_priority(&before_combat.monsters, index);
            dealt += hp_damage as f32 * priority;
            if monster.id == MonsterId::Lagavulin && monster.extra < 3 && hp_after > 0 {
                if let Some(monster_after) = after_combat.monsters.get(index) {
                    let woke_early = monster_after.extra >= 3 && hp_damage > 0;
                    let kill_is_close = hp_damage > 0
                        && hp_after as f32 / hp_damage as f32 <= p.laga_wake_kill_ratio;
                    let passive_wake_is_inevitable =
                        lagavulin_passive_wake_is_inevitable(&before.player, monster);
                    if woke_early && turn_advanced && !kill_is_close && !passive_wake_is_inevitable
                    {
                        laga_wake_penalty += p.laga_wake_penalty;
                    }
                }
            }
            if monster.power_amount(PowerId::Barricade) > 0 && monster.block >= monster.hp.max(1) {
                if let Some(monster_after) = after_combat.monsters.get(index) {
                    stripped_block +=
                        (monster.block - monster_after.block).max(0) as f32 * priority;
                }
            }
            if hp_after <= 0 {
                if after_combat
                    .monsters
                    .get(index)
                    .is_some_and(|monster_after| monster_after.half_dead)
                {
                    phase_transitions += 1;
                } else {
                    dead += 1;
                }
            }
        }
    }
    let incoming: i32 = living
        .iter()
        .map(|monster| projected_incoming(&after.player, monster))
        .sum();

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
        return doomed_score(context, after, projected_hp);
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
    // Reaching a rebirth is still a full phase milestone. Its imminent phase
    // two attack is priced separately by encounter_deadline_pressure.
    value += phase_transitions as f32 * p.kill_bonus;
    value -= laga_wake_penalty;
    value -= unblocked * danger;
    if scripted > 0 {
        let bank = persistent_block_bank(after);
        value -= (scripted - bank).max(0) as f32 * p.spike_danger;
    }
    value -= encounter_deadline_pressure(after);
    value -= collector_debuff_pressure(after);
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
    if turn_advanced {
        value += cheap_hand_damage(after) as f32 * damage_weight * p.next_hand_damage_mult;
        value += next_hand_orb_value(after, damage_weight);
    }

    let persistent_horizon = remaining_fight_length(after);
    let strength = after.player.power_amount(PowerId::Strength)
        - before.player.power_amount(PowerId::Strength);
    let dexterity = after.player.power_amount(PowerId::Dexterity)
        - before.player.power_amount(PowerId::Dexterity);
    let focus =
        after.player.power_amount(PowerId::Focus) - before.player.power_amount(PowerId::Focus);
    value += strength as f32 * p.strength_weight * persistent_horizon;
    value += dexterity as f32 * p.dexterity_weight * persistent_horizon;
    value += focus as f32 * p.focus_weight * persistent_horizon;
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        let enemy_strength_cost: f32 = after_combat
            .monsters
            .iter()
            .enumerate()
            .map(|(index, monster)| {
                let Some(old) = before_combat.monsters.get(index) else {
                    return 0.0;
                };
                let gain = (monster.power_amount(PowerId::Strength)
                    - old.power_amount(PowerId::Strength))
                .max(0) as f32;
                // Curiosity Strength acquired in phase one survives the
                // rebirth. In phase two a Power creates no Strength delta, so
                // delaying the same card naturally avoids this lifetime tax.
                let exposure = persistent_horizon;
                gain * exposure
            })
            .sum();
        value -= enemy_strength_cost * p.enemy_strength_penalty;
    }
    let status_gain = (status_card_count(&after.player) - status_card_count(&before.player)).max(0);
    value -= status_gain as f32 * p.status_gain_penalty;
    let focus_decay = after.player.power_amount(PowerId::Bias).max(0) as f32;
    value -=
        focus_decay * p.bias_decay_weight * (persistent_horizon * (persistent_horizon + 1.0) / 2.0);
    value += orb_value(after, turns_left, damage_weight) - context.before_orb_value;
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
        // Crossing half HP schedules Anger and then Execute. Once Anger is
        // the current intent, Execute is the very next monster action.
        MonsterId::Champ if !monster.split_triggered && monster.hp < monster.max_hp / 2 => (
            2,
            10,
            2,
            if monster.ascension >= 19 {
                12
            } else if monster.ascension >= 4 {
                9
            } else {
                6
            },
        ),
        MonsterId::Champ if monster.split_triggered && monster.next_move == 7 => (
            1,
            10,
            2,
            if monster.ascension >= 19 {
                12
            } else if monster.ascension >= 4 {
                9
            } else {
                6
            },
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
        // Both shapes alternate deterministically. Donu's intervening buff
        // adds three Strength to the next two-hit attack from either shape.
        MonsterId::Donu if monster.next_move == 2 => {
            (1, if monster.ascension >= 4 { 12 } else { 10 }, 2, 3)
        }
        MonsterId::Donu if monster.next_move == 0 => {
            (2, if monster.ascension >= 4 { 12 } else { 10 }, 2, 3)
        }
        MonsterId::Deca if monster.next_move == 2 => {
            (1, if monster.ascension >= 4 { 12 } else { 10 }, 2, 0)
        }
        MonsterId::Deca if monster.next_move == 0 => {
            (2, if monster.ascension >= 4 { 12 } else { 10 }, 2, 3)
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

/// Cost of entering Collector's scripted Weak/Vulnerable/Frail window without
/// an answer. Each Artifact charge blocks one of the three applications;
/// Orange Pellets makes the position recoverable when all three card types
/// remain available.
fn collector_debuff_pressure(game: &Game) -> f32 {
    let Some(combat) = &game.combat else {
        return 0.0;
    };
    let Some(collector) = combat
        .monsters
        .iter()
        .find(|monster| monster.alive() && monster.id == MonsterId::TheCollector)
    else {
        return 0.0;
    };
    if collector.split_triggered {
        return 0.0;
    }
    let imminent = collector.next_move == 4 || collector.extra == 2;
    if !imminent {
        return 0.0;
    }
    let duration = if collector.ascension >= 19 { 5.0 } else { 3.0 };
    let uncovered = (3 - game.player.power_amount(PowerId::Artifact).clamp(0, 3)) as f32 / 3.0;
    let pellets_ready = game.player.has_relic(RelicId::OrangePellets)
        && [CardType::ATTACK, CardType::SKILL, CardType::POWER]
            .into_iter()
            .all(|kind| live_combat_cards(&game.player).any(|card| card.card_type() == kind));
    duration * uncovered * if pellets_ready { 4.0 } else { 10.0 }
}

/// Small reservation signal for deterministic boss beats that sit beyond the
/// exact next-hand simulation. It is intentionally boss-only and much smaller
/// than current-intent damage, so it breaks close choices without replacing
/// the exact search.
fn encounter_deadline_pressure(game: &Game) -> f32 {
    let Some(combat) = &game.combat else {
        return 0.0;
    };
    let scripted: i32 = combat
        .monsters
        .iter()
        .filter(|monster| {
            monster.alive()
                && matches!(
                    monster.id,
                    MonsterId::BronzeAutomaton
                        | MonsterId::Champ
                        | MonsterId::AwakenedOne
                        | MonsterId::Donu
                        | MonsterId::Deca
                )
        })
        .map(|monster| scripted_incoming(&game.player, monster, 2))
        .sum();
    (scripted - persistent_block_bank(game)).max(0) as f32 * 0.2
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

/// Measured fight lengths are full-fight priors. Convert them to a continuation
/// horizon by consuming one unit for every completed player turn.
fn remaining_fight_length(game: &Game) -> f32 {
    let kind = fight_kind(game);
    let minimum = match (game.dungeon.act, kind) {
        (Act::Exordium, FightKind::Normal) => 3.5,
        (Act::Exordium, FightKind::Elite) => 7.0,
        (Act::Exordium, FightKind::Boss) => 11.0,
        (Act::City, FightKind::Normal) => 6.0,
        (Act::City, FightKind::Elite) => 8.0,
        (Act::City, FightKind::Boss) => 12.0,
        (Act::Beyond, FightKind::Normal) => 7.0,
        (Act::Beyond, FightKind::Elite) => 8.0,
        (Act::Beyond, FightKind::Boss) => 14.0,
        (Act::Ending, FightKind::Normal) => 5.0,
        (Act::Ending, FightKind::Elite) => 7.0,
        (Act::Ending, FightKind::Boss) => 15.0,
    };
    // Empirical durations from losing runs are censored at death. In
    // particular, the A20 bake reports Act 2 elites as 2.7 turns even though
    // surviving Book/Leader/Slavers fights commonly last 5-9 turns. Do not let
    // that death bias erase the continuation value of powers after turn two.
    let baseline = fight_length(kind, game.dungeon.act).max(minimum);
    let elapsed = game
        .combat
        .as_ref()
        .map_or(0, |combat| combat.turn.saturating_sub(1)) as f32;
    (baseline - elapsed).max(1.0)
}

/// Once passive Lightning will pierce Lagavulin's remaining block at End Turn,
/// waking is no longer an option the player can preserve. Do not prune attacks
/// that complete that final setup turn.
fn lagavulin_passive_wake_is_inevitable(player: &Player, monster: &Monster) -> bool {
    if monster.id != MonsterId::Lagavulin || monster.extra >= 3 {
        return false;
    }
    let passive = (3 + player.power_amount(PowerId::Focus)).max(0);
    let lightning = player
        .orbs
        .iter()
        .filter(|orb| orb.kind == OrbKind::Lightning)
        .count() as i32;
    let cables = i32::from(
        player.has_relic(RelicId::Cables)
            && player
                .orbs
                .first()
                .is_some_and(|orb| orb.kind == OrbKind::Lightning),
    );
    passive.saturating_mul(lightning + cables) > monster.block.max(0)
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

fn stasis_card_priority(card: &Card) -> f32 {
    match card.id {
        CardId::Echo_Form
        | CardId::Buffer
        | CardId::Biased_Cognition
        | CardId::Glacier
        | CardId::Reinforced_Body
        | CardId::Core_Surge
        | CardId::Fission
        | CardId::Multi_Cast
        | CardId::Redo => 0.9,
        CardId::Defragment
        | CardId::Doom_and_Gloom
        | CardId::Darkness
        | CardId::Sunder
        | CardId::Genetic_Algorithm
        | CardId::Hologram => 0.6,
        _ if card.card_type() == CardType::POWER || card.cost >= 2 => 0.4,
        _ => 0.2,
    }
}

/// Target value depends on the encounter state, not just the monster type.
/// This keeps the static table useful for ordinary fights while representing
/// boss-specific deadlines and recoverable resources.
fn encounter_target_priority(monsters: &[Monster], index: usize) -> f32 {
    let Some(monster) = monsters.get(index) else {
        return 1.0;
    };
    let mut priority = target_priority(monster.id);
    if !matches!(
        monster.id,
        MonsterId::BronzeOrb
            | MonsterId::TorchHead
            | MonsterId::TheCollector
            | MonsterId::Donu
            | MonsterId::Deca
    ) {
        return priority;
    }
    match monster.id {
        MonsterId::BronzeOrb => {
            if let Some(card) = &monster.stasis_card {
                let hyper_beam_close = monsters.iter().any(|other| {
                    other.alive() && other.id == MonsterId::BronzeAutomaton && other.extra >= 3
                });
                let urgency = if hyper_beam_close { 1.35 } else { 1.0 };
                priority += stasis_card_priority(card) * urgency;
            }
        }
        MonsterId::TorchHead => {
            if monsters.iter().any(|other| {
                other.alive() && other.id == MonsterId::TheCollector && other.hp <= other.max_hp / 4
            }) {
                // Stop feeding the resummon loop when the boss itself is in
                // a short, reliable kill window.
                priority = 0.85;
            }
        }
        MonsterId::TheCollector if monster.hp <= monster.max_hp / 4 => {
            priority = 1.45;
        }
        MonsterId::Donu | MonsterId::Deca => {
            let donu = monsters
                .iter()
                .find(|other| other.alive() && other.id == MonsterId::Donu);
            let deca = monsters
                .iter()
                .find(|other| other.alive() && other.id == MonsterId::Deca);
            if let (Some(donu), Some(deca)) = (donu, deca) {
                let donu_ehp = donu.hp.max(0).saturating_add(donu.block.max(0)) as i64;
                let deca_ehp = deca.hp.max(0).saturating_add(deca.block.max(0)) as i64;
                let deca_is_immediate_kill = deca_ehp * 4 <= donu_ehp;
                priority = match monster.id {
                    MonsterId::Deca if deca_is_immediate_kill => 1.45,
                    MonsterId::Donu if deca_is_immediate_kill => 1.3,
                    MonsterId::Donu => 1.4,
                    MonsterId::Deca => 0.9,
                    _ => unreachable!(),
                };
            } else {
                priority = 1.0;
            }
        }
        _ => {}
    }
    priority
}

#[derive(Clone, Copy, Debug, Default)]
struct QueueForecast {
    channels: f32,
    storm_channels: f32,
}

#[derive(Clone, Copy, Debug, Default)]
struct QueueTiming {
    lifetime: f32,
    front_turns: f32,
}

#[derive(Clone, Debug)]
struct OrbTargetBudget {
    index: usize,
    hp: i32,
    remaining: f32,
    priority: f32,
    lock_on: bool,
}

fn live_combat_cards(player: &Player) -> impl Iterator<Item = &Card> {
    player
        .hand
        .iter()
        .chain(player.draw.iter())
        .chain(player.discard.iter())
}

fn card_channel_output(game: &Game, card: &Card) -> f32 {
    match card.id {
        CardId::Zap | CardId::Ball_Lightning | CardId::Cold_Snap | CardId::Doom_and_Gloom => {
            card.base_magic.max(1) as f32
        }
        CardId::Coolheaded | CardId::Darkness | CardId::Fusion => 1.0,
        CardId::Glacier | CardId::Electrodynamics => card.base_magic.max(2) as f32,
        CardId::Rainbow | CardId::Meteor_Strike => 3.0,
        CardId::Chaos => {
            if card.upgraded {
                2.0
            } else {
                1.0
            }
        }
        CardId::Chill => game.combat.as_ref().map_or(0.0, |combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| monster.alive())
                .count() as f32
        }),
        CardId::Tempest => {
            let chemical_x = if game.player.has_relic(RelicId::Chemical_X) {
                2
            } else {
                0
            };
            (game.player.energy.max(game.player.energy_master).max(0)
                + chemical_x
                + i32::from(card.upgraded)) as f32
        }
        _ => 0.0,
    }
}

fn queue_timing(
    queue_index: usize,
    free_slots: usize,
    horizon: f32,
    channel_rate: f32,
) -> QueueTiming {
    if horizon <= 0.0 {
        return QueueTiming::default();
    }
    if channel_rate <= 0.0 {
        return QueueTiming {
            lifetime: horizon,
            front_turns: if queue_index == 0 { horizon } else { 0.0 },
        };
    }

    let channels_until_evoke = free_slots.saturating_add(queue_index).saturating_add(1);
    let eviction_turn = channels_until_evoke as f32 / channel_rate;
    let front_start = if queue_index == 0 {
        0.0
    } else {
        free_slots.saturating_add(queue_index) as f32 / channel_rate
    };
    QueueTiming {
        lifetime: horizon.min(eviction_turn),
        front_turns: (horizon.min(eviction_turn) - front_start).max(0.0),
    }
}

fn orb_target_budgets(game: &Game) -> Vec<OrbTargetBudget> {
    game.combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .enumerate()
                .filter(|(_, monster)| monster.alive())
                .map(|(index, monster)| OrbTargetBudget {
                    index,
                    hp: monster.hp.max(0),
                    remaining: monster.hp.max(0).saturating_add(monster.block.max(0)) as f32,
                    priority: encounter_target_priority(&combat.monsters, index),
                    lock_on: monster.power_amount(PowerId::LockOn) > 0,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn adjusted_orb_damage(amount: f32, lock_on: bool) -> f32 {
    if lock_on {
        (amount * 1.5).floor()
    } else {
        amount
    }
}

/// Value several projected Lightning triggers against one shared EHP budget.
/// Fractional triggers are expected opportunities from the bounded queue
/// forecast. Electrodynamics spends damage on every target; ordinary
/// Lightning distributes the expectation across the targets still alive.
fn spend_lightning_budget(
    game: &Game,
    targets: &mut [OrbTargetBudget],
    amount: i32,
    triggers: f32,
    damage_weight: f32,
) -> f32 {
    if amount <= 0 || triggers <= 0.0 {
        return 0.0;
    }
    if targets.is_empty() {
        return amount as f32 * triggers * damage_weight;
    }

    let electro = game.player.power_amount(PowerId::Electro) > 0;
    let mut triggers_left = triggers;
    let mut value = 0.0;
    while triggers_left > 0.001 && targets.iter().any(|target| target.remaining > 0.0) {
        let share = triggers_left.min(1.0);
        let living = targets
            .iter()
            .filter(|target| target.remaining > 0.0)
            .count()
            .max(1) as f32;
        for target in targets.iter_mut().filter(|target| target.remaining > 0.0) {
            let probability = if electro { 1.0 } else { 1.0 / living };
            let damage = adjusted_orb_damage(amount as f32, target.lock_on) * share * probability;
            let useful = damage.min(target.remaining);
            target.remaining -= useful;
            value += useful * target.priority;
        }
        triggers_left -= share;
    }
    value * damage_weight
}

/// Expected channels before the continuation horizon closes. This is not a
/// second combat rollout: it uses the real pile composition to estimate how
/// often channel cards and Storm-compatible powers will be seen.
fn queue_forecast(game: &Game, horizon: f32) -> QueueForecast {
    let mut card_count = 0usize;
    let mut direct_channels = 0.0;
    let mut power_cards = 0usize;
    for card in live_combat_cards(&game.player) {
        card_count += 1;
        direct_channels += card_channel_output(game, card);
        power_cards += usize::from(card.card_type() == CardType::POWER);
    }
    if card_count == 0 || horizon <= 0.0 {
        return QueueForecast::default();
    }

    let draws_per_turn = (5 + game.player.power_amount(PowerId::DrawCard)).max(1) as f32;
    let cycles = (draws_per_turn * horizon / card_count as f32).clamp(0.25, 2.0);
    let power_opportunities = power_cards as f32 * cycles.min(1.0)
        + game.player.power_amount(PowerId::CreativeAI).max(0) as f32 * horizon;
    let storm_channels =
        game.player.power_amount(PowerId::Storm).max(0) as f32 * power_opportunities;
    QueueForecast {
        channels: direct_channels * cycles + storm_channels,
        storm_channels,
    }
}

fn orb_damage_on(monster: &Monster, amount: f32, priority: f32) -> f32 {
    let amount = if monster.power_amount(PowerId::LockOn) > 0 {
        (amount * 1.5).floor()
    } else {
        amount
    };
    let ehp = monster.hp.max(0).saturating_add(monster.block.max(0)) as f32;
    amount.min(ehp) * priority
}

/// Combat-score value of one Lightning trigger. Without Electrodynamics the
/// random target is an expectation; with it every living target contributes.
/// Lock-On is applied per target in the same order as the combat engine.
fn lightning_trigger_value(game: &Game, amount: i32, damage_weight: f32) -> f32 {
    let Some(combat) = &game.combat else {
        return amount.max(0) as f32 * damage_weight;
    };
    let living: Vec<_> = combat
        .monsters
        .iter()
        .enumerate()
        .filter(|(_, monster)| monster.alive())
        .collect();
    if living.is_empty() || amount <= 0 {
        return 0.0;
    }
    let total = living
        .iter()
        .map(|(index, monster)| {
            orb_damage_on(
                monster,
                amount as f32,
                encounter_target_priority(&combat.monsters, *index),
            )
        })
        .sum::<f32>();
    let expected = if game.player.power_amount(PowerId::Electro) > 0 {
        total
    } else {
        total / living.len() as f32
    };
    expected * damage_weight
}

fn release_probability(channel_pressure: f32, channels_until_evoke: usize) -> f32 {
    if channels_until_evoke == 0 {
        1.0
    } else {
        (channel_pressure / channels_until_evoke as f32).clamp(0.0, 1.0)
    }
}

fn forced_dark_value(game: &Game, amount: i32, damage_weight: f32) -> f32 {
    let forced = game.combat.as_ref().and_then(|combat| {
        combat
            .monsters
            .iter()
            .enumerate()
            .filter(|(_, monster)| monster.alive())
            .min_by_key(|(_, monster)| monster.hp)
            .map(|(index, monster)| (monster, encounter_target_priority(&combat.monsters, index)))
    });
    forced.map_or(
        amount.max(0) as f32 * damage_weight,
        |(monster, priority)| {
            orb_damage_on(monster, amount.max(0) as f32, priority) * damage_weight
        },
    )
}

fn orb_evoke_option_value(game: &Game, orb: crate::creature::Orb, damage_weight: f32) -> f32 {
    let focus = game.player.power_amount(PowerId::Focus);
    match orb.kind {
        OrbKind::Lightning => lightning_trigger_value(game, (8 + focus).max(0), damage_weight),
        OrbKind::Frost => (5 + focus).max(0) as f32 * params().orb_frost_mult,
        OrbKind::Dark => forced_dark_value(game, orb.evoke.max(6), damage_weight),
        // Two immediate energy is materially more accessible than Plasma's
        // passive-only baseline, but it still consumes the orb bank.
        OrbKind::Plasma => params().orb_plasma * 0.5,
    }
}

/// Target, release, and protection value for every Dark bank. Target EHP is
/// consumed greedily in queue order, preventing several banks from all
/// claiming the same ideal kill while other enemies remain unassigned.
fn dark_bank_values(
    game: &Game,
    horizon: f32,
    damage_weight: f32,
    channel_pressure: f32,
    front_extra_stacks: f32,
    targets: &mut [OrbTargetBudget],
) -> Vec<f32> {
    let p = params();
    let focus = game.player.power_amount(PowerId::Focus);
    let free_slots = (game.player.max_orbs.max(0) as usize).saturating_sub(game.player.orbs.len());
    let channel_rate = if horizon > 0.0 {
        channel_pressure / horizon
    } else {
        0.0
    };
    let mut reserved_chaff = Vec::new();
    let forced_target = targets
        .iter()
        .min_by_key(|target| target.hp)
        .map(|target| (target.index, target.hp));
    let mut values = vec![0.0; game.player.orbs.len()];

    for (queue_index, orb) in game.player.orbs.iter().enumerate() {
        if orb.kind != OrbKind::Dark {
            continue;
        }
        let channels_until_evoke = free_slots.saturating_add(queue_index).saturating_add(1);
        let timing = queue_timing(queue_index, free_slots, horizon, channel_rate);
        let stored = orb.evoke.max(6) as f32;
        let ordinary_growth = (6 + focus).max(0) as f32 * timing.lifetime;
        let repeated_front_growth =
            (6 + focus).max(0) as f32 * timing.front_turns * front_extra_stacks;
        let future = stored + ordinary_growth + repeated_front_growth;

        let mut best: Option<(usize, usize, f32)> = None;
        for (slot, target) in targets.iter().enumerate() {
            if target.remaining <= 0.0 {
                continue;
            }
            let adjusted = adjusted_orb_damage(future, target.lock_on);
            let useful = adjusted.min(target.remaining) * target.priority;
            if best.is_none_or(|(_, _, old)| useful > old) {
                best = Some((slot, target.index, useful));
            }
        }
        let future_value = if let Some((slot, _, useful)) = best {
            let adjusted = adjusted_orb_damage(future, targets[slot].lock_on);
            targets[slot].remaining = (targets[slot].remaining - adjusted).max(0.0);
            useful * damage_weight * p.orb_dark_future_mult
        } else if targets.is_empty() {
            future * damage_weight * p.orb_dark_future_mult
        } else {
            0.0
        };
        let assigned_target = best.map(|(_, index, _)| index);
        let chaff_reserve = if matches!((forced_target, assigned_target),
            (Some((forced, hp)), Some(best))
                if forced != best && hp < stored as i32 && !reserved_chaff.contains(&forced))
        {
            let forced = forced_target.unwrap().0;
            reserved_chaff.push(forced);
            p.kill_bonus * p.orb_dark_chaff_reserve
        } else {
            0.0
        };
        let safe_channels = free_slots.saturating_add(queue_index) as f32;
        let protected_fraction = safe_channels / (safe_channels + 1.0);
        let queue_protection = future_value * protected_fraction * p.orb_dark_queue_flex;
        let release = release_probability(channel_pressure, channels_until_evoke);
        let scheduled_release = future_value * release * 0.15;
        values[queue_index] = stored * p.orb_dark_stored
            + (ordinary_growth + repeated_front_growth) * p.orb_dark_growth
            + future_value
            + chaff_reserve
            + queue_protection
            + scheduled_release;
    }
    values
}

/// Continuation value of the exact ordered orb queue. Queue position affects
/// access to evocations, protection from future channels, and repeated front
/// triggers from Loop/Cables. Persistent Storm is joined with the power cards
/// that can still trigger it, while every Dark bank receives its own target.
fn orb_value(game: &Game, turns_left: f32, damage_weight: f32) -> f32 {
    if game.player.max_orbs <= 0 {
        return 0.0;
    }
    let p = params();
    let focus = game.player.power_amount(PowerId::Focus);
    // fight_length is a continuation horizon from the state being scored,
    // not an absolute turn number.
    let horizon = turns_left.max(0.0).min(p.orb_horizon);
    let free_slots = (game.player.max_orbs.max(0) as usize).saturating_sub(game.player.orbs.len());
    let forecast = queue_forecast(game, horizon);
    let channel_rate = if horizon > 0.0 {
        forecast.channels / horizon
    } else {
        0.0
    };
    let front_extra_stacks = game.player.power_amount(PowerId::Loop).max(0) as f32
        + f32::from(game.player.has_relic(RelicId::Cables));
    let mut dark_targets = orb_target_budgets(game);
    let dark_values = dark_bank_values(
        game,
        horizon,
        damage_weight,
        forecast.channels,
        front_extra_stacks,
        &mut dark_targets,
    );
    let mut lightning_targets = orb_target_budgets(game);
    let mut value = 0.0;

    for (queue_index, orb) in game.player.orbs.iter().enumerate() {
        let channels_until_evoke = free_slots.saturating_add(queue_index).saturating_add(1);
        let release = release_probability(forecast.channels, channels_until_evoke);
        let timing = queue_timing(queue_index, free_slots, horizon, channel_rate);
        let front_triggers = timing.front_turns * front_extra_stacks;
        value += match orb.kind {
            OrbKind::Lightning => {
                let passive = spend_lightning_budget(
                    game,
                    &mut lightning_targets,
                    (3 + focus).max(0),
                    timing.lifetime + front_triggers,
                    damage_weight,
                ) * p.orb_lightning_mult;
                let evoke = spend_lightning_budget(
                    game,
                    &mut lightning_targets,
                    (8 + focus).max(0),
                    release,
                    damage_weight,
                ) * 0.25;
                passive + evoke
            }
            OrbKind::Frost => {
                let passive = (2 + focus).max(0) as f32 * p.orb_frost_mult;
                passive * (timing.lifetime + front_triggers)
                    + orb_evoke_option_value(game, *orb, damage_weight) * release * 0.25
            }
            OrbKind::Dark => dark_values[queue_index],
            OrbKind::Plasma => {
                let position_access = 1.0 / channels_until_evoke as f32;
                let retained_fraction = if horizon > 0.0 {
                    timing.lifetime / horizon
                } else {
                    0.0
                };
                let repeated_front_energy = p.orb_plasma * 0.35 * front_triggers;
                p.orb_plasma * (0.45 + 0.35 * retained_fraction + 0.2 * position_access)
                    + repeated_front_energy
                    + p.orb_plasma * release * 0.35
            }
        };
    }

    // Empty capacity is an option only when the real piles contain channels
    // likely to use it. This offsets delayed evocations when preservation is
    // useful, while a smaller row retains the release-access terms above.
    let safe_channels = (free_slots as f32).min(forecast.channels);
    value += safe_channels * (p.orb_plasma * 0.18 + focus.max(0) as f32 * 0.5);

    // Storm has value in the power cards it can still observe, not merely in
    // the one channel generated by a same-turn power play.
    if forecast.storm_channels > 0.0 {
        let average_lifetime = (horizon * 0.5).max(1.0);
        value += forecast.storm_channels
            * lightning_trigger_value(game, (3 + focus).max(0), damage_weight)
            * p.orb_lightning_mult
            * average_lifetime;
    }
    value
}

fn card_is_affordable(card: &Card, energy: i32) -> bool {
    card.free_to_play_once || card.cost_for_turn < 0 || card.cost_for_turn as i32 <= energy
}

fn front_orb_tool_value(game: &Game, card: &Card) -> f32 {
    let Some(front) = game.player.orbs.first().copied() else {
        return 0.0;
    };
    let turns_left = fight_length(fight_kind(game), game.dungeon.act);
    let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
    let evoke = orb_evoke_option_value(game, front, damage_weight);
    match card.id {
        CardId::Redo => evoke * 0.6,
        CardId::Multi_Cast => {
            let mut triggers = game.player.energy.max(game.player.energy_master).max(0);
            if game.player.has_relic(RelicId::Chemical_X) {
                triggers += 2;
            }
            triggers += i32::from(card.upgraded);
            evoke * triggers.max(1) as f32 * 0.45
        }
        _ => 0.0,
    }
}

const NEXT_HAND_ROLLOUT_CARD_CAP: usize = 12;
const NEXT_HAND_ROLLOUT_STATE_CAP: usize = 4_096;
const NEXT_HAND_QUEUE_HORIZON: f32 = 2.0;
const ROLLOUT_EHP_SCALE: i32 = 4;

#[derive(Clone, Debug)]
struct RolloutTarget {
    priority: f32,
    lock_on: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct QueueRolloutState {
    remaining_cards: u16,
    energy: i16,
    max_orbs: i16,
    focus: i16,
    loop_stacks: i16,
    storm_stacks: i16,
    electro: bool,
    orbs: Vec<crate::creature::Orb>,
    target_hp: Vec<i32>,
    target_block: Vec<i32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct QueueRolloutResult {
    value: f32,
    max_energy: i32,
}

struct QueueRolloutContext<'a> {
    game: &'a Game,
    cards: Vec<Card>,
    targets: Vec<RolloutTarget>,
    damage_weight: f32,
    chemical_x: i32,
    cables: bool,
}

fn queue_rollout_card(card: &Card) -> bool {
    card.card_type() == CardType::ATTACK
        || matches!(
            card.id,
            CardId::Zap
                | CardId::Coolheaded
                | CardId::Darkness
                | CardId::Fusion
                | CardId::Glacier
                | CardId::Electrodynamics
                | CardId::Rainbow
                | CardId::Chaos
                | CardId::Chill
                | CardId::Tempest
                | CardId::Redo
                | CardId::Multi_Cast
                | CardId::Fission
                | CardId::Capacitor
                | CardId::Consume
                | CardId::Loop
                | CardId::Storm
        )
}

fn queue_rollout_cards(game: &Game) -> Vec<Card> {
    let mut cards: Vec<_> = game
        .player
        .hand
        .iter()
        .copied()
        .filter(queue_rollout_card)
        .collect();
    cards.sort_by_key(|card| {
        let queue_priority = i32::from(card.card_type() != CardType::ATTACK)
            + i32::from(matches!(
                card.id,
                CardId::Redo
                    | CardId::Multi_Cast
                    | CardId::Fission
                    | CardId::Capacitor
                    | CardId::Consume
            )) * 2;
        -queue_priority
    });
    cards.truncate(NEXT_HAND_ROLLOUT_CARD_CAP);
    cards
}

fn rollout_target_context(game: &Game) -> (Vec<RolloutTarget>, Vec<i32>, Vec<i32>) {
    let budgets = orb_target_budgets(game);
    let targets = budgets
        .iter()
        .map(|target| RolloutTarget {
            priority: target.priority,
            lock_on: target.lock_on,
        })
        .collect();
    let hp = budgets
        .iter()
        .map(|target| target.hp.saturating_mul(ROLLOUT_EHP_SCALE))
        .collect();
    let block = game
        .combat
        .as_ref()
        .map(|combat| {
            combat
                .monsters
                .iter()
                .filter(|monster| monster.alive())
                .map(|monster| monster.block.max(0).saturating_mul(ROLLOUT_EHP_SCALE))
                .collect()
        })
        .unwrap_or_default();
    (targets, hp, block)
}

fn spend_rollout_damage(state: &mut QueueRolloutState, index: usize, damage: i32) -> i32 {
    let blocked = damage.min(state.target_block[index]);
    state.target_block[index] -= blocked;
    let hp_damage = (damage - blocked).min(state.target_hp[index]);
    state.target_hp[index] -= hp_damage;
    blocked + hp_damage
}

fn rollout_lightning_value(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    amount: i32,
    triggers: f32,
) -> f32 {
    if amount <= 0 || triggers <= 0.0 {
        return 0.0;
    }
    if state.target_hp.is_empty() {
        return amount as f32 * triggers * context.damage_weight;
    }

    let mut value = 0.0;
    let mut triggers_left = triggers;
    while triggers_left > 0.001 && state.target_hp.iter().any(|hp| *hp > 0) {
        let trigger_share = triggers_left.min(1.0);
        let living = state.target_hp.iter().filter(|hp| **hp > 0).count().max(1) as f32;
        for index in 0..state.target_hp.len() {
            if state.target_hp[index] <= 0 {
                continue;
            }
            let probability = if state.electro { 1.0 } else { 1.0 / living };
            let damage = adjusted_orb_damage(amount as f32, context.targets[index].lock_on)
                * trigger_share
                * probability;
            let scaled = (damage * ROLLOUT_EHP_SCALE as f32).round().max(1.0) as i32;
            let useful = spend_rollout_damage(state, index, scaled);
            value += useful as f32 / ROLLOUT_EHP_SCALE as f32 * context.targets[index].priority;
        }
        triggers_left -= trigger_share;
    }
    value * context.damage_weight
}

fn rollout_dark_value(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    amount: i32,
) -> f32 {
    if state.target_hp.is_empty() {
        return amount.max(0) as f32 * context.damage_weight;
    }
    let Some(index) = state
        .target_hp
        .iter()
        .enumerate()
        .filter(|(_, hp)| **hp > 0)
        .min_by_key(|(_, hp)| **hp)
        .map(|(index, _)| index)
    else {
        return 0.0;
    };
    let damage = adjusted_orb_damage(amount.max(0) as f32, context.targets[index].lock_on);
    let scaled = (damage * ROLLOUT_EHP_SCALE as f32).round() as i32;
    let useful = spend_rollout_damage(state, index, scaled);
    useful as f32 / ROLLOUT_EHP_SCALE as f32
        * context.targets[index].priority
        * context.damage_weight
}

fn rollout_evoke(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    orb: crate::creature::Orb,
) -> f32 {
    match orb.kind {
        OrbKind::Lightning => {
            rollout_lightning_value(context, state, (8 + state.focus as i32).max(0), 1.0)
        }
        OrbKind::Frost => (5 + state.focus as i32).max(0) as f32 * params().orb_frost_mult,
        OrbKind::Dark => rollout_dark_value(context, state, orb.evoke.max(0)),
        OrbKind::Plasma => {
            state.energy = state.energy.saturating_add(2);
            params().orb_plasma * 0.5
        }
    }
}

fn rollout_channel(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    kind: OrbKind,
) -> f32 {
    if state.max_orbs <= 0 {
        return 0.0;
    }
    let mut value = 0.0;
    if state.orbs.len() >= state.max_orbs as usize {
        let front = state.orbs.remove(0);
        value += rollout_evoke(context, state, front);
    }
    if state.orbs.len() < state.max_orbs as usize {
        state.orbs.push(crate::creature::Orb {
            kind,
            evoke: if kind == OrbKind::Dark { 6 } else { 0 },
        });
    }
    value
}

fn rollout_queue_continuation(context: &QueueRolloutContext<'_>, state: &QueueRolloutState) -> f32 {
    let p = params();
    let mut projected = state.clone();
    let front_extra =
        (state.loop_stacks.max(0) as f32 + f32::from(context.cables)) * NEXT_HAND_QUEUE_HORIZON;
    let mut value = 0.0;
    for (index, orb) in state.orbs.iter().copied().enumerate() {
        let extra = if index == 0 { front_extra } else { 0.0 };
        match orb.kind {
            OrbKind::Lightning => {
                value += rollout_lightning_value(
                    context,
                    &mut projected,
                    (3 + state.focus as i32).max(0),
                    NEXT_HAND_QUEUE_HORIZON + extra,
                ) * p.orb_lightning_mult;
            }
            OrbKind::Frost => {
                value += (2 + state.focus as i32).max(0) as f32
                    * p.orb_frost_mult
                    * (NEXT_HAND_QUEUE_HORIZON + extra);
            }
            OrbKind::Dark => {
                let ordinary_growth =
                    (6 + state.focus as i32).max(0) as f32 * NEXT_HAND_QUEUE_HORIZON;
                let front_growth = (6 + state.focus as i32).max(0) as f32 * extra;
                let future = orb.evoke.max(6) as f32 + ordinary_growth + front_growth;
                // A retained bank only has a bounded chance to find a release
                // inside this two-turn continuation. An operator that evokes
                // it now receives the full useful-damage value instead.
                let future_value = rollout_dark_value(context, &mut projected, future as i32)
                    * p.orb_dark_future_mult
                    * 0.35;
                let protected = (state.max_orbs.max(0) as usize)
                    .saturating_sub(state.orbs.len())
                    .saturating_add(index) as f32;
                let protection = protected / (protected + 1.0);
                value += orb.evoke.max(6) as f32 * p.orb_dark_stored
                    + (ordinary_growth + front_growth) * p.orb_dark_growth
                    + future_value * (1.0 + protection * p.orb_dark_queue_flex);
            }
            OrbKind::Plasma => {
                value += p.orb_plasma * (0.8 + extra * 0.35);
            }
        }
    }
    value
}

fn rollout_channel_card(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    card: &Card,
    x_effect: i32,
) -> f32 {
    let mut value = 0.0;
    let mut channel = |kind, count: i32, state: &mut QueueRolloutState| {
        for _ in 0..count.max(0) {
            value += rollout_channel(context, state, kind);
        }
    };
    match card.id {
        CardId::Zap | CardId::Ball_Lightning => {
            channel(OrbKind::Lightning, card.base_magic.max(1) as i32, state)
        }
        CardId::Cold_Snap => channel(OrbKind::Frost, card.base_magic.max(1) as i32, state),
        CardId::Coolheaded => channel(OrbKind::Frost, 1, state),
        CardId::Darkness => {
            channel(OrbKind::Dark, 1, state);
            if card.upgraded {
                let gain = (6 + state.focus as i32).max(0);
                for orb in &mut state.orbs {
                    if orb.kind == OrbKind::Dark {
                        orb.evoke += gain;
                    }
                }
            }
        }
        CardId::Doom_and_Gloom => channel(OrbKind::Dark, card.base_magic.max(1) as i32, state),
        CardId::Fusion => channel(OrbKind::Plasma, 1, state),
        CardId::Glacier => channel(OrbKind::Frost, card.base_magic.max(2) as i32, state),
        CardId::Electrodynamics => {
            channel(OrbKind::Lightning, card.base_magic.max(2) as i32, state)
        }
        CardId::Rainbow => {
            channel(OrbKind::Lightning, 1, state);
            channel(OrbKind::Frost, 1, state);
            channel(OrbKind::Dark, 1, state);
        }
        CardId::Meteor_Strike => channel(OrbKind::Plasma, 3, state),
        CardId::Chaos => {
            channel(OrbKind::Lightning, 1, state);
            if card.upgraded {
                channel(OrbKind::Frost, 1, state);
            }
        }
        CardId::Chill => {
            let living = state.target_hp.iter().filter(|hp| **hp > 0).count() as i32;
            channel(
                OrbKind::Frost,
                living * card.base_magic.max(1) as i32,
                state,
            );
        }
        CardId::Tempest => channel(OrbKind::Lightning, x_effect, state),
        _ => {}
    }
    value
}

fn play_rollout_card(
    context: &QueueRolloutContext<'_>,
    state: &mut QueueRolloutState,
    card_index: usize,
) -> Option<f32> {
    let card = context.cards[card_index];
    let energy_before = state.energy.max(0) as i32;
    let x_card =
        matches!(card.id, CardId::Multi_Cast | CardId::Tempest) || card.cost_for_turn == -1;
    if !x_card && !card_is_affordable(&card, energy_before) {
        return None;
    }
    if card.cost_for_turn < -1 {
        return None;
    }

    state.remaining_cards &= !(1u16 << card_index);
    let mut x_effect = 0;
    if x_card {
        x_effect = energy_before;
        if matches!(card.id, CardId::Multi_Cast | CardId::Tempest) {
            x_effect += context.chemical_x + i32::from(card.upgraded);
        }
        if !card.free_to_play_once {
            state.energy = 0;
        }
    } else if !card.free_to_play_once {
        state.energy -= card.cost_for_turn.max(0);
    }

    let mut value = 0.0;
    if state.storm_stacks > 0 && card.card_type() == CardType::POWER {
        for _ in 0..state.storm_stacks {
            value += rollout_channel(context, state, OrbKind::Lightning);
        }
    }
    if card.id == CardId::Electrodynamics {
        state.electro = true;
    }

    if card.card_type() == CardType::ATTACK {
        let hits = if card.id == CardId::Barrage {
            state.orbs.len() as i32
        } else if card.cost_for_turn == -1 {
            x_effect
        } else {
            1
        };
        value += crate::combat::derived_damage(&card, &context.game.player).max(0) as f32
            * hits.max(0) as f32
            * context.damage_weight;
    }

    match card.id {
        CardId::Compile_Driver => {
            let mut kinds = Vec::new();
            for orb in &state.orbs {
                if !kinds.contains(&orb.kind) {
                    kinds.push(orb.kind);
                }
            }
            value += kinds.len() as f32 * context.damage_weight * 2.2;
        }
        CardId::Redo => {
            if !state.orbs.is_empty() {
                let front = state.orbs.remove(0);
                value += rollout_evoke(context, state, front);
                if state.orbs.len() < state.max_orbs.max(0) as usize {
                    state.orbs.push(front);
                }
            }
        }
        CardId::Multi_Cast => {
            if x_effect > 0 && !state.orbs.is_empty() {
                for trigger in 0..x_effect {
                    let front = state.orbs[0];
                    value += rollout_evoke(context, state, front);
                    if trigger + 1 == x_effect {
                        state.orbs.remove(0);
                    }
                }
            }
        }
        CardId::Fission => {
            let filled = state.orbs.len() as i32;
            if card.upgraded {
                while !state.orbs.is_empty() {
                    let front = state.orbs.remove(0);
                    value += rollout_evoke(context, state, front);
                }
            } else {
                state.orbs.clear();
            }
            state.energy = state.energy.saturating_add(filled as i16);
            value += filled as f32 * context.damage_weight * 2.0;
        }
        CardId::Capacitor => {
            state.max_orbs = (state.max_orbs + card.base_magic.max(2)).min(10);
        }
        CardId::Consume => {
            state.focus = state.focus.saturating_add(card.base_magic.max(2));
            state.max_orbs = (state.max_orbs - 1).max(0);
            state.orbs.truncate(state.max_orbs as usize);
        }
        CardId::Loop => {
            state.loop_stacks = state.loop_stacks.saturating_add(card.base_magic.max(1));
        }
        CardId::Storm => {
            state.storm_stacks = state.storm_stacks.saturating_add(card.base_magic.max(1));
        }
        _ => {
            value += rollout_channel_card(context, state, &card, x_effect);
        }
    }
    Some(value)
}

fn solve_queue_rollout(
    context: &QueueRolloutContext<'_>,
    state: QueueRolloutState,
    memo: &mut HashMap<QueueRolloutState, QueueRolloutResult>,
) -> QueueRolloutResult {
    if let Some(result) = memo.get(&state) {
        return *result;
    }
    let terminal = rollout_queue_continuation(context, &state);
    let mut result = QueueRolloutResult {
        value: terminal,
        max_energy: state.energy.max(0) as i32,
    };
    if memo.len() >= NEXT_HAND_ROLLOUT_STATE_CAP {
        return result;
    }

    let mut seen_cards = Vec::new();
    for card_index in 0..context.cards.len() {
        if state.remaining_cards & (1u16 << card_index) == 0 {
            continue;
        }
        let card = context.cards[card_index];
        if seen_cards.contains(&card) {
            continue;
        }
        seen_cards.push(card);
        let mut next = state.clone();
        let Some(immediate) = play_rollout_card(context, &mut next, card_index) else {
            continue;
        };
        let child = solve_queue_rollout(context, next, memo);
        result.max_energy = result.max_energy.max(child.max_energy);
        result.value = result.value.max(immediate + child.value);
    }
    memo.insert(state, result);
    result
}

fn next_hand_queue_rollout(game: &Game, damage_weight: f32) -> QueueRolloutResult {
    let cards = queue_rollout_cards(game);
    let (targets, target_hp, target_block) = rollout_target_context(game);
    let context = QueueRolloutContext {
        game,
        cards,
        targets,
        damage_weight,
        chemical_x: if game.player.has_relic(RelicId::Chemical_X) {
            2
        } else {
            0
        },
        cables: game.player.has_relic(RelicId::Cables),
    };
    let card_count = context.cards.len();
    let initial = QueueRolloutState {
        remaining_cards: if card_count == 0 {
            0
        } else {
            (1u16 << card_count) - 1
        },
        energy: game.player.energy.max(0).min(i16::MAX as i32) as i16,
        max_orbs: game.player.max_orbs.max(0).min(i16::MAX as i32) as i16,
        focus: game
            .player
            .power_amount(PowerId::Focus)
            .clamp(i16::MIN as i32, i16::MAX as i32) as i16,
        loop_stacks: game
            .player
            .power_amount(PowerId::Loop)
            .max(0)
            .min(i16::MAX as i32) as i16,
        storm_stacks: game
            .player
            .power_amount(PowerId::Storm)
            .max(0)
            .min(i16::MAX as i32) as i16,
        electro: game.player.power_amount(PowerId::Electro) > 0,
        orbs: game.player.orbs.to_vec(),
        target_hp,
        target_block,
    };
    let baseline_queue = rollout_queue_continuation(&context, &initial);
    let baseline_damage = cheap_hand_damage(game) as f32 * damage_weight;
    let mut memo = HashMap::new();
    let solved = solve_queue_rollout(&context, initial, &mut memo);
    QueueRolloutResult {
        value: (solved.value - baseline_queue - baseline_damage).max(0.0),
        max_energy: solved.max_energy.max(game.player.energy.max(0)),
    }
}

#[cfg(test)]
fn reachable_energy_with_orb_tools(game: &Game) -> i32 {
    if game.player.max_orbs <= 0 {
        return game.player.energy.max(0);
    }
    let turns_left = fight_length(fight_kind(game), game.dungeon.act);
    let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
    next_hand_queue_rollout(game, damage_weight).max_energy
}

/// Orb-specific output from a memoized, ordered search of the exact next hand.
/// Its terminal value is a two-turn queue continuation, so card costs,
/// Plasma releases, capacity changes, front rotation, and queue operators all
/// compete inside one line instead of receiving independent scalar bonuses.
fn next_hand_orb_value(game: &Game, damage_weight: f32) -> f32 {
    if game.player.max_orbs <= 0 {
        return 0.0;
    }
    next_hand_queue_rollout(game, damage_weight).value * params().next_hand_damage_mult
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
    fn continuation_horizon_shrinks_as_combat_turns_pass() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.current_room = RoomType::Boss;
        game.combat = Some(Combat::start(
            EncounterId::Hexaghost,
            &mut game.player,
            &mut game.rng,
            16,
            1,
            20,
        ));
        game.screen = Screen::Combat;
        assert_eq!(remaining_fight_length(&game), params().fl_a1_boss);

        game.combat.as_mut().unwrap().turn = 7;
        assert_eq!(remaining_fight_length(&game), params().fl_a1_boss - 6.0);
    }

    #[test]
    fn act_two_elite_horizon_is_not_censored_by_early_deaths() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.dungeon.act = Act::City;
        game.current_room = RoomType::Elite;
        game.combat = Some(Combat::start(
            EncounterId::BookOfStabbing,
            &mut game.player,
            &mut game.rng,
            23,
            2,
            20,
        ));
        game.screen = Screen::Combat;

        assert!(params().fl_a2_elite < 3.0);
        assert_eq!(remaining_fight_length(&game), 8.0);
        game.combat.as_mut().unwrap().turn = 5;
        assert_eq!(remaining_fight_length(&game), 4.0);
    }

    #[test]
    fn effective_end_of_turn_block_includes_frost_powers_and_relics() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.block = 0;
        game.player.add_power(PowerId::Focus, 2);
        game.player.add_power(PowerId::Metallicize, 3);
        game.player.add_power(PowerId::PlatedArmor, 4);
        *game.player.orbs = vec![Orb {
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
    fn exact_combat_state_tracks_piles_combat_and_rng() {
        let game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        let same = game.clone();
        assert!(game
            .combat_search_state()
            .exact_eq(&same.combat_search_state()));

        let mut different_energy = game.clone();
        different_energy.player.energy += 1;
        assert!(!game
            .combat_search_state()
            .exact_eq(&different_energy.combat_search_state()));

        let mut different_rng = game.clone();
        let _ = different_rng.rng.card.random_int(10);
        assert!(!game
            .combat_search_state()
            .exact_eq(&different_rng.combat_search_state()));

        let mut pile_order = game.clone();
        *pile_order.player.discard = vec![Card::new(CardId::Strike_B), Card::new(CardId::Defend_B)];
        let mut reversed = pile_order.clone();
        reversed.player.discard.reverse();
        assert!(!pile_order
            .combat_search_state()
            .exact_eq(&reversed.combat_search_state()));
    }

    #[test]
    fn combat_search_checkpoint_restores_after_a_play() {
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
        game.player.energy = 3;
        *game.player.hand = vec![Card::new(CardId::Strike_B)];

        let checkpoint = game.combat_search_checkpoint();
        let root = checkpoint.root();
        let root_legal = game.legal_actions();
        let mut scratch = game.clone();
        scratch.step(&Action::Play {
            hand_index: 0,
            target_index: Some(0),
        });
        assert!(!root.exact_eq(&scratch.combat_search_state()));

        scratch.restore_combat_search_state(&checkpoint, root);
        assert!(root.exact_eq(&scratch.combat_search_state()));
        assert_eq!(scratch.legal_actions(), root_legal);
    }

    #[test]
    fn combat_search_checkpoint_restores_after_a_winning_play() {
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
        game.player.energy = 3;
        *game.player.hand = vec![Card::new(CardId::Strike_B)];
        game.combat.as_mut().unwrap().monsters[0].hp = 1;

        let checkpoint = game.combat_search_checkpoint();
        let root = checkpoint.root();
        let root_legal = game.legal_actions();
        let mut scratch = game.clone();
        scratch.step(&Action::Play {
            hand_index: 0,
            target_index: Some(0),
        });
        assert_ne!(scratch.screen, Screen::Combat);

        scratch.restore_combat_search_state(&checkpoint, root);
        assert!(root.exact_eq(&scratch.combat_search_state()));
        assert_eq!(scratch.legal_actions(), root_legal);
        assert!(scratch.rewards.is_empty());
    }

    #[test]
    fn shared_search_deduplicates_equivalent_first_plays() {
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
        game.player.energy = 3;
        *game.player.hand = vec![Card::new(CardId::Defend_B), Card::new(CardId::Defend_B)];

        let legal = game.legal_actions();
        let (_, stats) = plan_turn_with_stats(&game, &legal);
        assert!(stats.dedup_hits > 0);
    }

    fn doomed_cultist_turn(monster_hp: i32) -> Game {
        use crate::combat::Combat;
        use crate::creature::Intent;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.current_room = RoomType::Monster;
        game.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut game.player,
            &mut game.rng,
            31,
            1,
            game.ascension,
        ));
        game.screen = Screen::Combat;
        game.player.hp = 12;
        game.player.max_hp = 71;
        game.player.energy = 3;
        game.player.block = 0;
        game.player.orbs.clear();
        *game.player.hand = vec![
            Card::new(CardId::Stack),
            Card::new(CardId::Zap),
            Card::new(CardId::Force_Field),
            Card::new(CardId::Defend_B),
            Card::new(CardId::Defend_B),
        ];
        game.player.draw.clear();
        game.player.discard.clear();
        game.player.exhaust.clear();

        let combat = game.combat.as_mut().unwrap();
        combat.turn = 7;
        let monster = &mut combat.monsters[0];
        monster.hp = monster_hp;
        monster.powers.clear();
        monster.add_power(PowerId::Strength, 25);
        monster.next_move = 1;
        monster.first_move = false;
        monster.intent = Intent::Attack;
        monster.intent_damage = 6;
        monster.intent_base_damage = 6;
        monster.intent_hits = 1;
        game
    }

    fn planned_card(game: &Game) -> (Action, CardId) {
        let action = plan_turn(game, &game.legal_actions());
        let Action::Play { hand_index, .. } = action else {
            panic!("expected a card play, got {action:?}");
        };
        (action, game.player.hand[hand_index].id)
    }

    fn orb_test_game() -> Game {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.current_room = RoomType::Monster;
        game.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut game.player,
            &mut game.rng,
            31,
            1,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.hand.clear();
        game.player.draw.clear();
        game.player.discard.clear();
        game.player.exhaust.clear();
        game.player.orbs.clear();
        game.player.max_orbs = 3;
        let monster = &mut game.combat.as_mut().unwrap().monsters[0];
        monster.hp = 300;
        monster.max_hp = 300;
        monster.block = 0;
        game
    }

    fn orb_test_value(game: &Game) -> f32 {
        let turns_left = 4.0;
        let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
        orb_value(game, turns_left, damage_weight)
    }

    fn next_orb_test_value(game: &Game) -> f32 {
        let turns_left = 4.0;
        let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;
        next_hand_orb_value(game, damage_weight)
    }

    #[test]
    fn doomed_turn_maximizes_survival_margin_before_damage() {
        let mut game = doomed_cultist_turn(9);
        for expected in [CardId::Defend_B, CardId::Defend_B, CardId::Stack] {
            let (action, card) = planned_card(&game);
            assert_eq!(card, expected);
            game.step(&action);
        }
        assert_eq!(game.player.block, 12);
    }

    #[test]
    fn doomed_turn_takes_end_of_turn_orb_lethal() {
        let mut game = doomed_cultist_turn(3);
        let (action, card) = planned_card(&game);
        assert_eq!(card, CardId::Zap);
        game.step(&action);
        game.step(&Action::EndTurn);
        assert!(game.player.hp > 0);
        assert_ne!(game.screen, Screen::Combat);
    }

    #[test]
    fn empty_orb_slot_values_zaps_next_turn_lightning_over_a_strike() {
        use crate::combat::Combat;
        use crate::creature::Intent;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.current_room = RoomType::Monster;
        game.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut game.player,
            &mut game.rng,
            31,
            1,
            game.ascension,
        ));
        game.screen = Screen::Combat;
        game.player.hp = 46;
        game.player.energy = 1;
        game.player.block = 5;
        game.player.orbs.clear();
        *game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Zap)];
        game.player.draw.clear();
        game.player.discard.clear();
        game.player.exhaust.clear();

        let combat = game.combat.as_mut().unwrap();
        combat.turn = 2;
        let monster = &mut combat.monsters[0];
        monster.hp = 39;
        monster.powers.clear();
        monster.next_move = 1;
        monster.first_move = false;
        monster.intent = Intent::Attack;
        monster.intent_damage = 6;
        monster.intent_base_damage = 6;
        monster.intent_hits = 1;

        let (_, card) = planned_card(&game);
        assert_eq!(card, CardId::Zap);
    }

    #[test]
    fn cheap_hand_block_respects_energy_and_x_costs() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.energy = 3;
        *game.player.hand = vec![
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
    fn automaton_orb_priority_tracks_the_stolen_card_and_hyper_beam_deadline() {
        let mut rng = RngSet::generate_seeds(2);
        let mut automaton = spawn_monster(MonsterId::BronzeAutomaton, &mut rng, 20);
        automaton.extra = 3;
        let orb = spawn_monster(MonsterId::BronzeOrb, &mut rng, 20);
        let ordinary = vec![automaton.clone(), orb.clone()];

        let mut critical = vec![automaton, orb];
        critical[1].stasis_card = Some(Card::new(CardId::Buffer));
        assert!(
            encounter_target_priority(&critical, 1) > encounter_target_priority(&ordinary, 1) + 1.0
        );
    }

    #[test]
    fn donu_and_deca_priority_switches_for_a_short_deca_kill() {
        let mut rng = RngSet::generate_seeds(2);
        let mut donu = spawn_monster(MonsterId::Donu, &mut rng, 20);
        donu.next_move = 2;
        let deca = spawn_monster(MonsterId::Deca, &mut rng, 20);
        let mut monsters = vec![donu, deca];

        assert!(encounter_target_priority(&monsters, 0) > encounter_target_priority(&monsters, 1));
        monsters[0].hp = 240;
        monsters[1].hp = 40;
        assert!(encounter_target_priority(&monsters, 1) > encounter_target_priority(&monsters, 0));
    }

    #[test]
    fn dark_queue_value_tracks_safe_channels_before_eviction() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut front = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        front.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut front.player,
            &mut front.rng,
            31,
            1,
            0,
        ));
        front.screen = Screen::Combat;
        front.player.max_orbs = 3;
        *front.player.orbs = vec![
            Orb {
                kind: OrbKind::Dark,
                evoke: 40,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
        ];
        let mut protected = front.clone();
        protected.player.orbs.swap(0, 1);
        let turns_left = fight_length(FightKind::Normal, front.dungeon.act);
        let damage_weight = params().dmg_base + params().dmg_per_turn * turns_left;

        assert!(
            orb_value(&protected, turns_left, damage_weight)
                > orb_value(&front, turns_left, damage_weight)
        );
    }

    #[test]
    fn loop_values_the_selected_front_orb_across_the_continuation_horizon() {
        let mut lightning_front = orb_test_game();
        *lightning_front.player.orbs = vec![
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
        ];
        let lightning_base = orb_test_value(&lightning_front);
        lightning_front.player.add_power(PowerId::Loop, 1);
        let lightning_loop_gain = orb_test_value(&lightning_front) - lightning_base;

        let mut frost_front = orb_test_game();
        *frost_front.player.orbs = vec![
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
        ];
        let frost_base = orb_test_value(&frost_front);
        frost_front.player.add_power(PowerId::Loop, 1);
        let frost_loop_gain = orb_test_value(&frost_front) - frost_base;

        assert!(lightning_loop_gain > frost_loop_gain);
        assert!(frost_loop_gain > 0.0);
    }

    #[test]
    fn queue_timing_stops_evicted_fronts_and_promotes_later_orbs() {
        let front = queue_timing(0, 0, 4.0, 2.0);
        let second = queue_timing(1, 0, 4.0, 2.0);
        let protected = queue_timing(0, 2, 4.0, 2.0);

        assert_eq!(front.lifetime, 0.5);
        assert_eq!(front.front_turns, 0.5);
        assert_eq!(second.lifetime, 1.0);
        assert_eq!(second.front_turns, 0.5);
        assert_eq!(protected.lifetime, 1.5);
        assert_eq!(protected.front_turns, 1.5);
    }

    #[test]
    fn later_dark_bank_receives_loop_growth_after_rotation() {
        let mut game = orb_test_game();
        game.player.max_orbs = 2;
        *game.player.orbs = vec![
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Dark,
                evoke: 30,
            },
        ];
        let mut ordinary_targets = orb_target_budgets(&game);
        let ordinary = dark_bank_values(&game, 4.0, 1.0, 2.0, 0.0, &mut ordinary_targets);
        let mut loop_targets = orb_target_budgets(&game);
        let looped = dark_bank_values(&game, 4.0, 1.0, 2.0, 1.0, &mut loop_targets);

        assert!(looped[1] > ordinary[1]);
    }

    #[test]
    fn plasma_value_tracks_release_distance_and_next_hand_energy_demand() {
        let mut front = orb_test_game();
        front.player.energy = 3;
        *front.player.orbs = vec![
            Orb {
                kind: OrbKind::Plasma,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
        ];
        let mut buried = front.clone();
        buried.player.orbs.rotate_left(1);
        assert!(orb_test_value(&front) > orb_test_value(&buried));

        *front.player.hand = vec![
            Card::new(CardId::Multi_Cast),
            Card::new(CardId::Meteor_Strike),
        ];
        *buried.player.hand = front.player.hand.to_vec();
        assert!(next_orb_test_value(&front) > next_orb_test_value(&buried));
        assert!(reachable_energy_with_orb_tools(&front) >= 5);
        assert!(reachable_energy_with_orb_tools(&buried) < 5);
    }

    #[test]
    fn capacity_prices_safe_channels_and_the_option_to_accelerate_release() {
        let mut full = orb_test_game();
        *full.player.orbs = vec![
            Orb {
                kind: OrbKind::Dark,
                evoke: 60,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
        ];
        full.player.draw.push(Card::new(CardId::Zap));
        let mut expanded = full.clone();
        expanded.player.max_orbs = 4;
        assert!(orb_test_value(&expanded) > orb_test_value(&full));

        let mut compact = orb_test_game();
        compact.player.max_orbs = 1;
        *compact.player.orbs = vec![Orb {
            kind: OrbKind::Plasma,
            evoke: 0,
        }];
        *compact.player.draw = (0..100)
            .map(|_| Card::new(CardId::Strike_B))
            .chain(std::iter::once(Card::new(CardId::Zap)))
            .collect();
        let mut roomy = compact.clone();
        roomy.player.max_orbs = 3;
        assert!(orb_test_value(&compact) > orb_test_value(&roomy));
    }

    #[test]
    fn multiple_dark_banks_claim_distinct_target_budgets() {
        let mut one_bank = orb_test_game();
        one_bank.combat.as_mut().unwrap().monsters[0].hp = 70;
        *one_bank.player.orbs = vec![Orb {
            kind: OrbKind::Dark,
            evoke: 60,
        }];
        let one_value = orb_test_value(&one_bank);

        let mut two_banks = one_bank.clone();
        two_banks.player.orbs.push(Orb {
            kind: OrbKind::Dark,
            evoke: 60,
        });
        let two_one_target = orb_test_value(&two_banks);
        assert!(two_one_target - one_value < one_value);

        let mut two_targets = two_banks.clone();
        let mut second = spawn_monster(MonsterId::Cultist, &mut two_targets.rng, 0);
        second.hp = 70;
        second.max_hp = 70;
        two_targets.combat.as_mut().unwrap().monsters.push(second);
        assert!(orb_test_value(&two_targets) > two_one_target);
    }

    #[test]
    fn electrodynamics_and_lock_on_join_retained_lightning_value() {
        let mut ordinary = orb_test_game();
        *ordinary.player.orbs = vec![Orb {
            kind: OrbKind::Lightning,
            evoke: 0,
        }];
        let mut second = spawn_monster(MonsterId::Cultist, &mut ordinary.rng, 0);
        second.hp = 300;
        second.max_hp = 300;
        ordinary.combat.as_mut().unwrap().monsters.push(second);
        let single_target = orb_test_value(&ordinary);

        let mut electro = ordinary.clone();
        electro.player.add_power(PowerId::Electro, 1);
        let all_enemy = orb_test_value(&electro);
        assert!(all_enemy > single_target * 1.5);

        electro.combat.as_mut().unwrap().monsters[0].add_power(PowerId::LockOn, 1);
        assert!(orb_test_value(&electro) > all_enemy);
    }

    #[test]
    fn projected_lightning_spends_one_shared_target_budget() {
        let mut game = orb_test_game();
        game.player.add_power(PowerId::Electro, 1);
        game.combat.as_mut().unwrap().monsters[0].hp = 5;
        let mut second = spawn_monster(MonsterId::Cultist, &mut game.rng, 0);
        second.hp = 5;
        second.max_hp = 5;
        game.combat.as_mut().unwrap().monsters.push(second);
        let mut targets = orb_target_budgets(&game);

        let first = spend_lightning_budget(&game, &mut targets, 8, 1.0, 1.0);
        let second = spend_lightning_budget(&game, &mut targets, 8, 1.0, 1.0);

        assert!(first > 0.0);
        assert_eq!(second, 0.0);
    }

    #[test]
    fn next_hand_value_joins_barrage_compile_fission_and_queue_tools() {
        let mut full = orb_test_game();
        full.player.energy = 3;
        *full.player.orbs = vec![
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Dark,
                evoke: 70,
            },
        ];
        *full.player.hand = vec![Card::new(CardId::Barrage)];
        let mut sparse = full.clone();
        sparse.player.orbs.truncate(1);
        assert!(next_orb_test_value(&full) > next_orb_test_value(&sparse));

        *full.player.hand = vec![Card::new(CardId::Compile_Driver)];
        let mut homogeneous = full.clone();
        *homogeneous.player.orbs = vec![
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Lightning,
                evoke: 0,
            },
        ];
        assert!(next_orb_test_value(&full) > next_orb_test_value(&homogeneous));

        let mut fission_plus = Card::new(CardId::Fission);
        fission_plus.upgrade();
        *full.player.hand = vec![fission_plus];
        let upgraded = next_orb_test_value(&full);
        *full.player.hand = vec![Card::new(CardId::Fission)];
        assert!(upgraded > next_orb_test_value(&full));

        *full.player.orbs = vec![Orb {
            kind: OrbKind::Dark,
            evoke: 90,
        }];
        *full.player.hand = vec![Card::new(CardId::Redo), Card::new(CardId::Multi_Cast)];
        let prepared_dark = next_orb_test_value(&full);
        full.player.orbs[0] = Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        };
        assert!(prepared_dark > next_orb_test_value(&full));
    }

    #[test]
    fn ordered_queue_rollout_respects_joint_channel_energy_costs() {
        let mut game = orb_test_game();
        game.player.energy = 1;
        game.player.max_orbs = 2;
        *game.player.orbs = vec![
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Plasma,
                evoke: 0,
            },
        ];
        *game.player.hand = vec![Card::new(CardId::Zap), Card::new(CardId::Zap)];

        // Both Zaps are individually affordable, but they cannot be played
        // together, so the buried Plasma is not reachable this turn.
        assert_eq!(reachable_energy_with_orb_tools(&game), 1);
    }

    #[test]
    fn ordered_queue_rollout_joins_channel_then_recursion() {
        let mut prepared = orb_test_game();
        prepared.player.energy = 2;
        prepared.player.max_orbs = 2;
        *prepared.player.orbs = vec![
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
            Orb {
                kind: OrbKind::Dark,
                evoke: 90,
            },
        ];
        *prepared.player.hand = vec![Card::new(CardId::Zap), Card::new(CardId::Redo)];

        let mut unprepared = prepared.clone();
        *unprepared.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Redo)];

        assert!(next_orb_test_value(&prepared) > next_orb_test_value(&unprepared));
    }

    #[test]
    fn ordered_capacity_change_protects_the_front_bank_before_channeling() {
        let mut game = orb_test_game();
        game.player.energy = 2;
        game.player.max_orbs = 2;
        *game.player.orbs = vec![
            Orb {
                kind: OrbKind::Dark,
                evoke: 90,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
        ];
        *game.player.hand = vec![Card::new(CardId::Capacitor), Card::new(CardId::Zap)];

        let cards = queue_rollout_cards(&game);
        let capacitor = cards
            .iter()
            .position(|card| card.id == CardId::Capacitor)
            .unwrap();
        let zap = cards
            .iter()
            .position(|card| card.id == CardId::Zap)
            .unwrap();
        let (targets, target_hp, target_block) = rollout_target_context(&game);
        let context = QueueRolloutContext {
            game: &game,
            cards,
            targets,
            damage_weight: 1.0,
            chemical_x: 0,
            cables: false,
        };
        let initial = QueueRolloutState {
            remaining_cards: (1u16 << context.cards.len()) - 1,
            energy: 2,
            max_orbs: 2,
            focus: 0,
            loop_stacks: 0,
            storm_stacks: 0,
            electro: false,
            orbs: game.player.orbs.to_vec(),
            target_hp,
            target_block,
        };

        let mut protected = initial.clone();
        play_rollout_card(&context, &mut protected, capacitor).unwrap();
        play_rollout_card(&context, &mut protected, zap).unwrap();
        assert_eq!(protected.orbs[0].kind, OrbKind::Dark);
        assert_eq!(protected.orbs.len(), 3);
        assert_eq!(protected.target_hp, initial.target_hp);

        let mut released = initial;
        play_rollout_card(&context, &mut released, zap).unwrap();
        assert_ne!(released.orbs[0].kind, OrbKind::Dark);
        assert!(released.target_hp[0] < protected.target_hp[0]);
    }

    #[test]
    fn consume_can_reduce_capacity_to_accelerate_a_planned_evoke() {
        let mut game = orb_test_game();
        game.player.energy = 3;
        game.player.max_orbs = 3;
        *game.player.orbs = vec![
            Orb {
                kind: OrbKind::Dark,
                evoke: 90,
            },
            Orb {
                kind: OrbKind::Frost,
                evoke: 0,
            },
        ];
        *game.player.hand = vec![Card::new(CardId::Consume), Card::new(CardId::Zap)];

        let cards = queue_rollout_cards(&game);
        let consume = cards
            .iter()
            .position(|card| card.id == CardId::Consume)
            .unwrap();
        let zap = cards
            .iter()
            .position(|card| card.id == CardId::Zap)
            .unwrap();
        let (targets, target_hp, target_block) = rollout_target_context(&game);
        let context = QueueRolloutContext {
            game: &game,
            cards,
            targets,
            damage_weight: 1.0,
            chemical_x: 0,
            cables: false,
        };
        let initial = QueueRolloutState {
            remaining_cards: (1u16 << context.cards.len()) - 1,
            energy: 3,
            max_orbs: 3,
            focus: 0,
            loop_stacks: 0,
            storm_stacks: 0,
            electro: false,
            orbs: game.player.orbs.to_vec(),
            target_hp,
            target_block,
        };

        let mut roomy = initial.clone();
        play_rollout_card(&context, &mut roomy, zap).unwrap();
        assert_eq!(roomy.target_hp, initial.target_hp);

        let mut compact = initial;
        play_rollout_card(&context, &mut compact, consume).unwrap();
        play_rollout_card(&context, &mut compact, zap).unwrap();
        assert_eq!(compact.max_orbs, 2);
        assert_eq!(compact.focus, 2);
        assert!(compact.target_hp[0] < roomy.target_hp[0]);
    }

    #[test]
    fn storm_values_future_power_triggers() {
        let mut dormant = orb_test_game();
        *dormant.player.hand = vec![Card::new(CardId::Defragment)];
        let without_storm = orb_test_value(&dormant);
        dormant.player.add_power(PowerId::Storm, 1);
        assert!(orb_test_value(&dormant) > without_storm);
    }

    #[test]
    fn queue_intelligence_is_neutral_without_orb_slots() {
        let game = Game::new(2, Character::Ironclad, 0, Unlocks::fixture());
        assert_eq!(game.player.max_orbs, 0);
        assert_eq!(orb_test_value(&game), 0.0);
        assert_eq!(next_orb_test_value(&game), 0.0);
    }

    #[test]
    fn rebound_promotes_recursion_and_multicast_for_a_prepared_front_orb() {
        let mut before = orb_test_game();
        *before.player.orbs = vec![Orb {
            kind: OrbKind::Dark,
            evoke: 80,
        }];
        *before.player.hand = vec![Card::new(CardId::Rebound)];
        let mut recursion = before.clone();
        recursion.player.draw.push(Card::new(CardId::Redo));
        let mut multicast = before.clone();
        multicast.player.draw.push(Card::new(CardId::Multi_Cast));
        let mut fallback = before.clone();
        fallback.player.draw.push(Card::new(CardId::Strike_B));

        assert!(rebound_state_value(&before, &recursion) > rebound_state_value(&before, &fallback));
        assert!(rebound_state_value(&before, &multicast) > rebound_state_value(&before, &fallback));
    }

    #[test]
    fn loop_and_cables_repeat_front_dark_growth_across_turns() {
        let mut base = orb_test_game();
        *base.player.orbs = vec![Orb {
            kind: OrbKind::Dark,
            evoke: 12,
        }];
        let ordinary = orb_test_value(&base);

        let mut looped = base.clone();
        looped.player.add_power(PowerId::Loop, 1);
        let loop_value = orb_test_value(&looped);
        assert!(loop_value > ordinary);

        let mut cabled = base.clone();
        cabled.player.relics.push(RelicInstance {
            id: RelicId::Cables,
            counter: -1,
            used_up: false,
        });
        let cable_value = orb_test_value(&cabled);
        assert!(cable_value > ordinary);

        cabled.player.add_power(PowerId::Loop, 1);
        assert!(orb_test_value(&cabled) > loop_value.max(cable_value));
    }

    #[test]
    fn rebound_target_score_prefers_a_high_value_repeat() {
        let mut before = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        *before.player.hand = vec![Card::new(CardId::Rebound), Card::new(CardId::Glacier)];
        let mut glacier_state = before.clone();
        glacier_state.player.draw.push(Card::new(CardId::Glacier));
        let mut strike_state = before.clone();
        strike_state.player.draw.push(Card::new(CardId::Strike_B));

        assert!(
            rebound_state_value(&before, &glacier_state)
                > rebound_state_value(&before, &strike_state)
        );
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
        *game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Self_Repair)];

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
        *game.player.hand = vec![Card::new(CardId::Strike_B), Card::new(CardId::Echo_Form)];

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
        *game.player.hand = vec![Card::new(CardId::Beam_Cell), Card::new(CardId::Strike_B)];
        let monster = &mut game.combat.as_mut().unwrap().monsters[0];
        monster.hp = 10;
        monster.block = 0;

        let legal = game.legal_actions();
        let mut stats = SearchStats::default();
        let checkpoint = game.combat_search_checkpoint();
        let mut scratch = game.clone();
        assert_eq!(
            exact_attack_lethal(&game, &legal, &checkpoint, &mut scratch, &mut stats),
            Some(Action::Play {
                hand_index: 0,
                target_index: Some(0),
            })
        );
        assert!(stats.lethal_expansions >= 2);
        assert!(stats.simulated_steps >= stats.lethal_expansions);

        let (planned, plan_stats) = plan_turn_with_stats(&game, &legal);
        assert_eq!(
            planned,
            Action::Play {
                hand_index: 0,
                target_index: Some(0),
            }
        );
        assert_eq!(plan_stats.plan_calls, 1);
        assert!(plan_stats.simulated_steps > 0);
        assert!(plan_stats.lethal_expansions > 0);
    }

    #[test]
    fn mixed_lethal_uses_focus_setup_before_end_turn_lightning() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.combat = Some(Combat::start(
            EncounterId::Cultist,
            &mut game.player,
            &mut game.rng,
            1,
            1,
            0,
        ));
        game.screen = Screen::Combat;
        game.player.energy = 1;
        *game.player.hand = vec![Card::new(CardId::Defragment)];
        *game.player.orbs = vec![Orb {
            kind: OrbKind::Lightning,
            evoke: 8,
        }];
        let monster = &mut game.combat.as_mut().unwrap().monsters[0];
        monster.hp = 4;
        monster.block = 0;

        let legal = game.legal_actions();
        let checkpoint = game.combat_search_checkpoint();
        let mut scratch = game.clone();
        let mut stats = SearchStats::default();
        assert_eq!(
            exact_mixed_turn_lethal(&game, &legal, &checkpoint, &mut scratch, &mut stats),
            Some(Action::Play {
                hand_index: 0,
                target_index: None,
            })
        );
        assert!(stats.lethal_expansions >= 2);
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
        let dump_turn = params().potion_boss_dump_turn.ceil() as i32;
        let combat = game.combat.as_mut().unwrap();
        combat.turn = (dump_turn - 1).max(2);
        combat.monsters[0].hp = params().potion_boss_dump_hp.floor() as i32;

        assert_eq!(potion_policy(&game, &game.legal_actions()), None);

        game.combat.as_mut().unwrap().turn = dump_turn;
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
    fn awakened_one_power_tax_ends_after_rebirth() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut phase_one = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        phase_one.combat = Some(Combat::start(
            EncounterId::AwakenedOne,
            &mut phase_one.player,
            &mut phase_one.rng,
            31,
            3,
            20,
        ));
        phase_one.screen = Screen::Combat;

        let mut powered_one = phase_one.clone();
        powered_one.player.add_power(PowerId::Focus, 1);
        let awakened = powered_one
            .combat
            .as_mut()
            .unwrap()
            .monsters
            .iter_mut()
            .find(|monster| monster.id == MonsterId::AwakenedOne)
            .unwrap();
        awakened.add_power(PowerId::Strength, awakened.power_amount(PowerId::Curiosity));
        let phase_one_gain =
            score_state(&phase_one, &powered_one) - score_state(&phase_one, &phase_one);

        let mut phase_two = phase_one.clone();
        let awakened = phase_two
            .combat
            .as_mut()
            .unwrap()
            .monsters
            .iter_mut()
            .find(|monster| monster.id == MonsterId::AwakenedOne)
            .unwrap();
        awakened.extra = 1;
        awakened.half_dead = false;
        let mut powered_two = phase_two.clone();
        powered_two.player.add_power(PowerId::Focus, 1);
        let phase_two_gain =
            score_state(&phase_two, &powered_two) - score_state(&phase_two, &phase_two);

        assert!(phase_two_gain > phase_one_gain + 100.0);
    }

    #[test]
    fn awakened_one_rebirth_keeps_phase_value_but_reserves_for_dark_echo() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut before = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        before.combat = Some(Combat::start(
            EncounterId::AwakenedOne,
            &mut before.player,
            &mut before.rng,
            31,
            3,
            20,
        ));
        before.screen = Screen::Combat;
        let mut rebirth = before.clone();
        let awakened = rebirth
            .combat
            .as_mut()
            .unwrap()
            .monsters
            .iter_mut()
            .find(|monster| monster.id == MonsterId::AwakenedOne)
            .unwrap();
        awakened.hp = 0;
        awakened.half_dead = true;
        awakened.dead = false;

        let mut truly_dead = rebirth.clone();
        let awakened = truly_dead
            .combat
            .as_mut()
            .unwrap()
            .monsters
            .iter_mut()
            .find(|monster| monster.id == MonsterId::AwakenedOne)
            .unwrap();
        awakened.half_dead = false;
        awakened.dead = true;

        let rebirth_score = score_state(&before, &rebirth);
        let dead_score = score_state(&before, &truly_dead);
        assert!(encounter_deadline_pressure(&rebirth) > 0.0);
        assert!(dead_score > rebirth_score);
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
        let mut partial_wake = asleep.clone();
        partial_wake.combat.as_mut().unwrap().monsters[0].extra = 3;
        assert_eq!(
            score_state(&before, &partial_wake),
            score_state(&before, &asleep)
        );

        let mut woken = partial_wake;
        woken.combat.as_mut().unwrap().turn += 1;
        assert!(score_state(&before, &woken) < score_state(&before, &asleep));

        let mut near_lethal = woken.clone();
        near_lethal.combat.as_mut().unwrap().monsters[0].hp = 20;
        assert!(score_state(&before, &near_lethal) > score_state(&before, &woken));

        let before_monster = &mut before.combat.as_mut().unwrap().monsters[0];
        before_monster.block = 0;
        assert!(lagavulin_passive_wake_is_inevitable(
            &before.player,
            before_monster
        ));
        let mut forced_wake = before.clone();
        let forced_monster = &mut forced_wake.combat.as_mut().unwrap().monsters[0];
        forced_monster.hp -= 10;
        forced_monster.extra = 3;
        forced_wake.combat.as_mut().unwrap().turn += 1;
        assert!(score_state(&before, &forced_wake) > score_state(&before, &before));
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

        let mut champ = spawn_monster(MonsterId::Champ, &mut rng, 20);
        champ.hp = champ.max_hp / 2 - 1;
        assert_eq!(scripted_incoming(&player, &champ, 1), 0);
        assert_eq!(scripted_incoming(&player, &champ, 2), 44);
        champ.split_triggered = true;
        champ.next_move = 7;
        assert_eq!(scripted_incoming(&player, &champ, 1), 44);

        let mut donu = spawn_monster(MonsterId::Donu, &mut rng, 20);
        donu.next_move = 2;
        assert_eq!(scripted_incoming(&player, &donu, 1), 30);
        let mut deca = spawn_monster(MonsterId::Deca, &mut rng, 20);
        deca.next_move = 2;
        assert_eq!(scripted_incoming(&player, &deca, 1), 24);
    }

    #[test]
    fn collector_mega_debuff_pressure_is_answered_by_artifact() {
        use crate::combat::Combat;
        use crate::ids::EncounterId;

        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.combat = Some(Combat::start(
            EncounterId::Collector,
            &mut game.player,
            &mut game.rng,
            31,
            2,
            20,
        ));
        game.screen = Screen::Combat;
        let collector = game
            .combat
            .as_mut()
            .unwrap()
            .monsters
            .iter_mut()
            .find(|monster| monster.id == MonsterId::TheCollector)
            .unwrap();
        collector.first_move = false;
        collector.extra = 2;
        assert!(collector_debuff_pressure(&game) >= 50.0);

        let unprotected = collector_debuff_pressure(&game);
        game.player.add_power(PowerId::Artifact, 1);
        assert!(collector_debuff_pressure(&game) < unprotected);
        assert!(collector_debuff_pressure(&game) > 0.0);
        game.player.add_power(PowerId::Artifact, 2);
        assert_eq!(collector_debuff_pressure(&game), 0.0);
    }

    #[test]
    fn persistent_block_bank_excludes_ordinary_block() {
        let mut game = Game::new(2, Character::Defect, 20, Unlocks::fixture());
        game.player.block = 20;
        *game.player.orbs = vec![Orb {
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
        *game.player.hand = vec![Card::new(CardId::Biased_Cognition)];
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
