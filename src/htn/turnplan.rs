use crate::action::Action;
use crate::game::{Game, Screen};
use crate::ids::CardType;

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
    let mut best_first = plays[0];
    let mut best_score = f32::MIN;
    for first in &plays {
        let mut clone = game.clone();
        clone.step(first);
        greedy_rest(&mut clone);
        let score = score_state(game, &clone);
        if score > best_score {
            best_score = score;
            best_first = first;
        }
    }
    // Also consider ending the turn immediately (full block / empty energy).
    if let Some(end) = legal.iter().find(|a| matches!(a, Action::EndTurn)) {
        let mut clone = game.clone();
        clone.step(end);
        let score = score_state(game, &clone);
        if score > best_score + 5.0 {
            return end.clone();
        }
    }
    best_first.clone()
}

fn greedy_rest(game: &mut Game) {
    for _ in 0..8 {
        if game.screen != Screen::Combat || game.player.hp <= 0 {
            break;
        }
        if game.combat.as_ref().is_some_and(|c| c.all_dead()) {
            break;
        }
        let legal = game.legal_actions();
        let plays: Vec<Action> = legal
            .iter()
            .filter(|a| matches!(a, Action::Play { .. }))
            .cloned()
            .collect();
        if plays.is_empty() {
            if let Some(end) = legal.iter().find(|a| matches!(a, Action::EndTurn)) {
                game.step(end);
            }
            break;
        }
        let mut best = plays[0].clone();
        let mut best_s = f32::MIN;
        for play in &plays {
            let mut c = game.clone();
            c.step(play);
            let s = score_state(game, &c);
            if s > best_s {
                best_s = s;
                best = play.clone();
            }
        }
        game.step(&best);
    }
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
        return 4_000.0 + after.player.hp as f32;
    }
    let hp0: i32 = before
        .combat
        .as_ref()
        .map(|c| c.monsters.iter().filter(|m| m.alive()).map(|m| m.hp).sum())
        .unwrap_or(0);
    let hp1: i32 = living.iter().map(|m| m.hp).sum();
    let dealt = (hp0 - hp1).max(0) as f32;
    let incoming: i32 = living
        .iter()
        .map(|m| m.intent_damage.max(0) * m.intent_hits.max(1))
        .sum();
    let unblocked = (incoming - after.player.block).max(0) as f32;
    let mut value = dealt * 8.0;
    value -= unblocked * 12.0;
    value += after.player.hp as f32;
    value += after.player.orbs.len() as f32 * 6.0;
    value += after.player.energy as f32 * 1.5;
    if living.iter().any(|m| m.hp <= 0) {
        value += 900.0;
    }
    value
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
    if unblocked >= hp || hp <= max_hp / 8 {
        return legal.iter().find(|a| matches!(a, crate::action::Action::Potion { .. })).cloned();
    }
    let _ = CardType::ATTACK;
    None
}
