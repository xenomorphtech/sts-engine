use crate::action::Action;
use crate::creature::{Monster, OrbKind, Player};
use crate::game::{Game, Screen};
use crate::ids::{Act, CardId, CardType, MonsterId, PotionId, PowerId, RelicId, RoomType};

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

#[derive(Clone)]
struct TurnSearchNode {
    game: Game,
    strategic_value: f32,
}

fn searched_rest(origin: &Game, start: Game) -> (Game, f32) {
    const WIDTH: usize = 8;
    const DEPTH: usize = 6;

    let mut frontier = vec![TurnSearchNode {
        game: start,
        strategic_value: 0.0,
    }];
    let mut finals = Vec::new();
    for _ in 0..DEPTH {
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
        next.truncate(WIDTH);
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
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        for (index, monster) in before_combat.monsters.iter().enumerate() {
            if !monster.alive() {
                continue;
            }
            let hp_after = after_combat.monsters.get(index).map_or(0, |m| m.hp.max(0));
            dealt += (monster.hp - hp_after).max(0) as f32 * target_priority(monster.id);
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
    let unblocked = if turn_advanced {
        (before.player.hp - after.player.hp).max(0) as f32
    } else {
        (incoming - after.player.block).max(0) as f32
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
    value -= unblocked * danger;
    if !turn_advanced {
        value -= (after.player.block - incoming).max(0) as f32 * p.overblock_penalty;
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
    let mut damage = (monster.intent_damage + monster.power_amount(PowerId::Strength)) as f32;
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
    damage * monster.intent_hits.max(1)
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
                    .is_some_and(|p| want.contains(&p.id))
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
        if empty >= 2 {
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

    if unblocked >= hp || hp <= max_hp / 8 {
        return find(DEFENSE)
            .or_else(|| find(HEAL))
            .or_else(|| find(OFFENSE));
    }
    if hp <= max_hp / 3 && unblocked > 0 {
        let total_hp: i32 = game
            .combat
            .as_ref()
            .map(|c| c.monsters.iter().filter(|m| m.alive()).map(|m| m.hp).sum())
            .unwrap_or(0);
        if total_hp <= 30 {
            if let Some(offense) = find(OFFENSE) {
                return Some(offense);
            }
        }
        if let Some(defense) = find(DEFENSE) {
            return Some(defense);
        }
    }
    if matches!(fight_kind(game), FightKind::Elite | FightKind::Boss) {
        if hp < max_hp / 2 {
            if let Some(heal) = find(HEAL) {
                return Some(heal);
            }
        }
        if unblocked >= 12.max(hp / 4) {
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
    use crate::creature::Player;
    use crate::ids::Character;
    use crate::rng::RngSet;
    use crate::Unlocks;

    #[test]
    fn measured_fight_lengths_are_act_specific() {
        assert_eq!(fight_length(FightKind::Normal, Act::Exordium), 3.3);
        assert_eq!(fight_length(FightKind::Normal, Act::City), 5.0);
        assert_eq!(fight_length(FightKind::Boss, Act::Ending), 12.0);
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
