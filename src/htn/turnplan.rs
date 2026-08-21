use crate::action::Action;
use crate::creature::{Monster, OrbKind, Player};
use crate::game::{Game, Screen};
use crate::ids::{Act, CardId, CardType, MonsterId, PotionId, PowerId, RelicId, RoomType};

const DMG_BASE: f32 = 6.0;
const DMG_PER_TURN: f32 = 4.5;
const DANGER_BASE: f32 = 20.0;
const DANGER_SCALE: f32 = 90.0;

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
        if non_progressing_status_play(game, &clone, first) {
            continue;
        }
        let strategic_value = rebound_play_value(game, first)
            + setup_play_value(game, first)
            + greedy_rest(&mut clone);
        let score = score_state(game, &clone) + strategic_value;
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

fn greedy_rest(game: &mut Game) -> f32 {
    let mut strategic_value = 0.0;
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
        let mut best: Option<Action> = None;
        let mut best_s = f32::MIN;
        for play in &plays {
            let mut c = game.clone();
            c.step(play);
            if non_progressing_status_play(game, &c, play) {
                continue;
            }
            let s = score_state(game, &c)
                + rebound_play_value(game, play)
                + setup_play_value(game, play);
            if s > best_s {
                best_s = s;
                best = Some(play.clone());
            }
        }
        let Some(best) = best else {
            if let Some(end) = legal
                .iter()
                .find(|action| matches!(action, Action::EndTurn))
            {
                game.step(end);
            }
            break;
        };
        strategic_value += rebound_play_value(game, &best) + setup_play_value(game, &best);
        game.step(&best);
    }
    strategic_value
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
    let danger = (DANGER_BASE + DANGER_SCALE * (1.0 - hp_frac).powi(2))
        * (1.0 + game.ascension as f32 / 50.0);
    match card.id {
        CardId::Self_Repair => card.base_magic.max(1) as f32 * danger * 1.25,
        CardId::Machine_Learning => {
            let turns_left = fight_length(fight_kind(game), game.dungeon.act);
            let damage_weight = DMG_BASE + DMG_PER_TURN * turns_left;
            card.base_magic.max(1) as f32 * turns_left * damage_weight * 2.2
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

    let turns_left = fight_length(fight_kind(after), after.dungeon.act);
    let damage_weight = DMG_BASE + DMG_PER_TURN * turns_left;
    let mut dealt = 0.0;
    let mut dead = 0;
    if let (Some(before_combat), Some(after_combat)) = (&before.combat, &after.combat) {
        for (index, monster) in before_combat.monsters.iter().enumerate() {
            if !monster.alive() {
                continue;
            }
            let hp_after = after_combat.monsters.get(index).map_or(0, |m| m.hp.max(0));
            dealt += (monster.hp - hp_after).max(0) as f32 * target_priority(monster.id);
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
    let mut danger = (DANGER_BASE + DANGER_SCALE * (1.0 - hp_frac).powi(2))
        * (1.0 + after.ascension as f32 / 50.0);
    let hp_left: i32 = living.iter().map(|m| m.hp.max(0)).sum();
    if dealt > 0.0 && hp_left as f32 / dealt <= 2.0 && projected_hp as f32 > unblocked {
        danger *= 0.55;
    }

    let mut value = dealt * damage_weight;
    value += dead as f32 * 900.0;
    value -= unblocked * danger;
    if !turn_advanced {
        value -= (after.player.block - incoming).max(0) as f32 * 0.8;
    }

    let strength = after.player.power_amount(PowerId::Strength)
        - before.player.power_amount(PowerId::Strength);
    let dexterity = after.player.power_amount(PowerId::Dexterity)
        - before.player.power_amount(PowerId::Dexterity);
    let focus =
        after.player.power_amount(PowerId::Focus) - before.player.power_amount(PowerId::Focus);
    value += strength as f32 * 4.0 * turns_left;
    value += dexterity as f32 * 3.0 * turns_left;
    value += focus as f32 * 4.0 * turns_left;
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
        value -= enemy_strength_gain as f32 * 20.0 * turns_left;
    }
    let focus_decay = after.player.power_amount(PowerId::Bias).max(0) as f32;
    value -= focus_decay * 4.0 * (turns_left * (turns_left + 1.0) / 2.0);
    value += orb_value(after, turns_left) - orb_value(before, turns_left);
    value += after.player.energy as f32 * 1.5;
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
    match (act, kind) {
        (Act::Exordium, FightKind::Normal) => 3.3,
        (Act::Exordium, FightKind::Elite) => 5.3,
        (Act::Exordium, FightKind::Boss) => 9.5,
        (Act::City, FightKind::Normal) => 5.0,
        (Act::City, FightKind::Elite) => 4.5,
        (Act::City, FightKind::Boss) => 8.0,
        (Act::Beyond, FightKind::Normal) => 5.5,
        (Act::Beyond, FightKind::Elite) => 6.0,
        (Act::Beyond, FightKind::Boss) => 10.0,
        (Act::Ending, FightKind::Normal) => 5.0,
        (Act::Ending, FightKind::Elite) => 6.0,
        (Act::Ending, FightKind::Boss) => 12.0,
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
    let focus = game.player.power_amount(PowerId::Focus);
    game.player
        .orbs
        .iter()
        .map(|orb| match orb.kind {
            OrbKind::Lightning => (3 + focus).max(0) as f32 * turns_left.min(4.0) * 0.8,
            OrbKind::Frost => (2 + focus).max(0) as f32 * turns_left.min(4.0),
            OrbKind::Dark => orb.evoke.max(6) as f32 * 0.45,
            OrbKind::Plasma => 12.0,
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
}
