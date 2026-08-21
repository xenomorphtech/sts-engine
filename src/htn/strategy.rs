use crate::action::Action;
use crate::card::Card;
use crate::game::{Game, GridKind, Screen};
use crate::ids::{CardId, CardType, Character, PotionId, RelicId, RoomType};

use super::deckplan;

const PICK_THRESHOLD: i32 = 85;

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
    let need_emerald = game.final_act_available() && !game.has_emerald_key();

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
        } => (
            *x,
            *y,
            RoomType::from_java_class(r).unwrap_or(RoomType::Monster),
        ),
        Action::Choose { label: Some(l), .. } if l == "boss" => {
            return 5.0;
        }
        _ => return 0.0,
    };
    node_value(game, x, y, room, hp_frac, act, gold, strength, need_emerald)
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
) -> f32 {
    let own = room_value(game, x, y, room, hp_frac, act, gold, strength, need_emerald);
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
                        hp_frac,
                        act,
                        gold,
                        strength,
                        need_emerald,
                    )
                })
                .fold(None, |acc: Option<f32>, v| {
                    Some(acc.map_or(v, |a| a.max(v)))
                })
        })
        .flatten();
    own + children.unwrap_or(0.0)
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
) -> f32 {
    match room {
        RoomType::Elite => {
            let metrics = deck_metrics(game);
            let afford = hp_frac >= 0.65 + game.ascension as f32 / 200.0
                && strength >= 3 + 2 * act + if game.ascension >= 15 { 2 } else { 0 };
            let matchup = elite_matchup(
                game.dungeon.elite_list.first().map(String::as_str),
                metrics,
                strength,
            );
            let mut v = if afford {
                40.0 + matchup as f32
            } else {
                -150.0 + matchup.min(0) as f32
            };
            if hp_frac < 0.4 {
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
            let mut v = if hp_frac < 0.65 { 35.0 } else { 18.0 };
            if y >= 14 {
                v = 50.0;
            }
            if game.final_act_available() && !game.has_ruby_key() && act >= 3 {
                v += 25.0;
            }
            v
        }
        RoomType::Shop => {
            if gold >= 120 {
                (gold as f32 / 5.0).min(60.0)
            } else if gold >= 75 {
                8.0
            } else {
                -5.0
            }
        }
        RoomType::Treasure => 25.0,
        RoomType::Event => 14.0,
        RoomType::Monster => {
            let shift = 0.05 * (act - 1) as f32;
            if hp_frac >= 0.6 + shift {
                15.0
            } else if hp_frac >= 0.4 + shift {
                -15.0
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

fn elite_matchup(next_elite: Option<&str>, metrics: DeckMetrics, strength: i32) -> i32 {
    match next_elite {
        Some("Gremlin Nob") => {
            if metrics.big_attacks >= 2 {
                15
            } else {
                -60
            }
        }
        Some("3 Sentries") => {
            if metrics.aoe >= 2 {
                20
            } else if metrics.aoe == 0 {
                -40
            } else {
                0
            }
        }
        Some("Lagavulin") => {
            if metrics.big_attacks >= 1 || strength >= 6 {
                15
            } else {
                -35
            }
        }
        Some("Book of Stabbing") => {
            if metrics.block_cards >= 6 {
                15
            } else {
                -35
            }
        }
        Some("Gremlin Leader") => {
            if metrics.aoe >= 2 {
                15
            } else {
                -30
            }
        }
        Some("Reptomancer") => {
            if metrics.aoe >= 1 {
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
    for a in legal {
        if let Action::Choose { label: Some(l), .. } = a {
            let lab = l.to_ascii_uppercase();
            if lab == "EMERALD_KEY" || lab == "GOLD" || lab == "STOLEN_GOLD" {
                return a.clone();
            }
            if lab == "SAPPHIRE_KEY"
                && game.final_act_available()
                && !game.has_sapphire_key()
                && game.dungeon.act as i32 >= 2
            {
                return a.clone();
            }
            if lab == "POTION" && empty_pots > 0 {
                return a.clone();
            }
            if lab == "RELIC" {
                return a.clone();
            }
            if lab == "CARD" {
                let best = game
                    .card_reward
                    .iter()
                    .map(|c| score_card(game, c))
                    .max()
                    .unwrap_or(0);
                if best >= PICK_THRESHOLD {
                    return a.clone();
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
        if s >= PICK_THRESHOLD {
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
        if let Action::Choose { index, label, .. } = a {
            let name = label
                .clone()
                .or_else(|| game.boss_relics.get(*index).map(|r| r.sts_id().to_string()))
                .unwrap_or_default();
            let r = boss_relic_rank(&name);
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
    let mut best: Option<(&Action, i32)> = None;
    for action in legal {
        let Action::Choose {
            label: Some(label), ..
        } = action
        else {
            continue;
        };
        let value = if label == "purge" {
            deckplan::shop_purge_value(game)
        } else if let Some(id) = CardId::from_sts_id(label) {
            score_card(game, &Card::new(id)) - 10
        } else if let Some(id) = RelicId::from_sts_id(label) {
            deckplan::shop_relic_value(game, id)
        } else if let Some(id) = PotionId::from_sts_id(label) {
            if empty_potions {
                shop_potion_value(id)
            } else {
                0
            }
        } else {
            0
        };
        if value > 45 && best.is_none_or(|(_, best_value)| value > best_value) {
            best = Some((action, value));
        }
    }
    best.map(|(action, _)| action.clone()).unwrap_or_else(|| {
        legal
            .iter()
            .find(|a| matches!(a, Action::Proceed | Action::Skip))
            .cloned()
            .unwrap_or_else(|| legal[0].clone())
    })
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

    if game.final_act_available() && !game.has_ruby_key() {
        let must_recall = act >= 3 && near_boss;
        let comfortable =
            (act >= 3 && hp_frac >= 0.6) || (act == 2 && hp_frac >= 0.9 && !near_boss);
        if must_recall || comfortable {
            if let Some(recall) = legal.iter().find(|a| {
                matches!(a, Action::Choose { label: Some(l), .. } if l.eq_ignore_ascii_case("recall"))
            }) {
                return recall.clone();
            }
        }
    }

    // Once Smith has opened its card list, upgrade the highest-value engine
    // piece instead of whichever deck card happens to be first.
    if !legal.iter().any(|a| {
        matches!(a, Action::Choose { label: Some(l), .. }
            if l.eq_ignore_ascii_case("rest") || l.eq_ignore_ascii_case("smith"))
    }) {
        let mut best: Option<(&Action, i32)> = None;
        for action in legal {
            let Action::Choose {
                label: Some(label), ..
            } = action
            else {
                continue;
            };
            if let Some(card) = game.player.deck.iter().find(|card| {
                card.can_upgrade()
                    && card
                        .sts_id()
                        .eq_ignore_ascii_case(label.trim_end_matches('+'))
            }) {
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

    let mut want = if hp_frac < (if act == 1 { 0.7 } else { 0.78 }) {
        "rest"
    } else {
        "smith"
    };
    if near_boss && hp_frac < 0.85 {
        want = "rest";
    }
    for a in legal {
        if let Action::Choose { label: Some(l), .. } = a {
            if l.eq_ignore_ascii_case(want) {
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

/// Rank Neow's blessings instead of defaulting to the first option: deck
/// thinning and the drawback-free third-category blessings dominate the
/// small category-0 rewards.
pub fn neow_choice(game: &Game, legal: &[Action]) -> Action {
    use crate::game::NeowKind;
    let rank = |kind: NeowKind| -> i32 {
        match kind {
            NeowKind::RemoveTwo => 100,
            NeowKind::TransformTwo => 85,
            NeowKind::RareRelic => 80,
            NeowKind::ThreeRareCards => 75,
            NeowKind::TwoFiftyGold => 70,
            NeowKind::RemoveCard => 55,
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
    };
    let mut best: Option<(&Action, i32)> = None;
    for action in legal {
        let Action::Choose { index, .. } = action else {
            continue;
        };
        let Some(option) = game.neow_options.get(*index) else {
            continue;
        };
        let value = rank(option.kind);
        if best.is_none_or(|(_, b)| value > b) {
            best = Some((action, value));
        }
    }
    best.map(|(action, _)| action.clone())
        .unwrap_or_else(|| event_choice(game, legal))
}

pub fn event_choice(game: &Game, legal: &[Action]) -> Action {
    let mut choices: Vec<(&Action, String)> = legal
        .iter()
        .filter_map(|action| match action {
            Action::Choose {
                label: Some(label), ..
            } => Some((action, strip_event_markup(label).to_ascii_lowercase())),
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
        .filter(|(_, label)| event_hp_loss(label, game.player.max_hp) < game.player.hp - 2)
        .cloned()
        .collect();
    if !survivable.is_empty() && survivable.len() < choices.len() {
        choices = survivable;
    }

    let event = game
        .event
        .as_ref()
        .map(|event| normalize_event_id(&event.id))
        .unwrap_or_default();
    let hp = game.player.hp;
    let max_hp = game.player.max_hp.max(1);
    let pick = |words: &[&str]| choice_containing(&choices, words);

    let selected = match event.as_str() {
        "forgottenaltar" => pick(&["offer"]).or_else(|| {
            if hp > 30.max(max_hp / 3) {
                pick(&["sacrifice"])
            } else {
                pick(&["desecrate", "curse", "leave"])
            }
        }),
        "windinghalls" => pick(&["retrace"]),
        _ => None,
    };
    if let Some(action) = selected {
        return action;
    }

    const SAFE: &[&str] = &[
        "leave", "continue", "proceed", "ignore", "depart", "refuse", "sleep", "escape", "talk",
    ];
    if let Some(action) = pick(SAFE) {
        return action;
    }
    choices[0].0.clone()
}

fn normalize_event_id(id: &str) -> String {
    id.chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn strip_event_markup(label: &str) -> String {
    let mut clean = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(ch) = chars.next() {
        if ch == '#' {
            let _ = chars.next();
        } else {
            clean.push(ch);
        }
    }
    clean
}

fn event_hp_loss(label: &str, max_hp: i32) -> i32 {
    let tokens: Vec<_> = label
        .split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '%')
        .filter(|token| !token.is_empty())
        .collect();
    for window in tokens.windows(3) {
        if window[0] == "lose" && window[2] == "hp" {
            if let Ok(amount) = window[1].parse::<i32>() {
                return amount;
            }
        }
        if window[0] == "take" && window[2] == "damage" {
            if let Ok(amount) = window[1].parse::<i32>() {
                return amount;
            }
        }
    }
    for window in tokens.windows(2) {
        if window[0] == "lose" {
            if let Some(percent) = window[1].strip_suffix('%') {
                if let Ok(percent) = percent.parse::<i32>() {
                    return max_hp * percent / 100;
                }
            }
        }
    }
    0
}

fn choice_containing(choices: &[(&Action, String)], words: &[&str]) -> Option<Action> {
    for word in words {
        if let Some((action, _)) = choices.iter().find(|(_, label)| label.contains(word)) {
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
            GridKind::Purge | GridKind::Transform => removal_score(card),
            GridKind::Upgrade => upgrade_score(card.id) - i32::from(card.upgraded) * 500,
            GridKind::Library | GridKind::Bottle(_) => score_card(game, card),
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
fn removal_score(card: &Card) -> i32 {
    match card.card_type() {
        CardType::CURSE => 900,
        CardType::STATUS => 800,
        _ => match card.id {
            CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P => 300,
            CardId::Defend_R | CardId::Defend_G | CardId::Defend_B | CardId::Defend_P => 160,
            id => 100 - defect_pick(id) - i32::from(card.upgraded) * 25,
        },
    }
}

/// Value of moving this card from a pile into the hand mid-combat
/// (Hologram, Seek, Secret Technique).
fn retrieve_score(game: &Game, card: &Card) -> i32 {
    let mut s = defect_pick(card.id) + i32::from(card.upgraded) * 25;
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
        size: game.player.deck.len() as i32,
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
        s += deckplan::card_adjustment(game, card);
    }
    if card.upgraded {
        s += 25;
    }

    let copies = game.player.deck.iter().filter(|c| c.id == card.id).count() as i32;
    let max_copies = max_copies(card.id);
    if copies >= max_copies {
        s -= 250;
    } else if copies == max_copies - 1 {
        s -= 40;
    }

    let metrics = deck_metrics(game);
    let act = game.dungeon.act as i32;
    if is_aoe(card.id) && metrics.aoe < 2 {
        s += 45;
    }
    if card.base_block > 0 && metrics.block_cards + metrics.frost_src / 2 < 5.max(metrics.size / 5)
    {
        s += 40;
    }
    if game.character == Character::Defect && is_channel(card.id) && metrics.channel < 5 {
        s += 35;
    }
    if is_scaling(card.id) && metrics.scaling < 2 + act {
        s += 40;
    }
    if game.character == Character::Defect && is_focus_source(card.id) {
        let focus_sources = game
            .player
            .deck
            .iter()
            .filter(|c| is_focus_source(c.id))
            .count();
        if focus_sources < 3 {
            s += 45;
        }
    }
    if act >= 2 && metrics.scaling < 2 && card.card_type() == CardType::ATTACK {
        if card.base_damage >= 14 {
            s += 45;
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
            s += 30;
        }
    }
    if act == 1 && card.card_type() == CardType::ATTACK {
        if metrics.attacks - metrics.strikes < 5 {
            s += 55;
        }
        if card.base_damage >= 10 {
            s += 25;
        }
    }
    if act == 1 && game.dungeon.floor >= 11 && card.base_block > 0 && metrics.block_cards < 7 {
        s += 35;
    }

    let target_size = match act {
        1 => 22,
        2 => 26,
        3 | 4 => 28,
        _ => 26,
    };
    if metrics.size >= target_size {
        s -= 160;
    } else if metrics.size >= target_size - 4 {
        s -= 60;
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
        CardId::Defragment
            | CardId::Biased_Cognition
            | CardId::Capacitor
            | CardId::Consume
            | CardId::Storm
            | CardId::Echo_Form
            | CardId::Creative_AI
            | CardId::Heatsinks
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

fn boss_relic_rank(name: &str) -> i32 {
    match name {
        "SlaversCollar" | "Slaver's Collar" => 95,
        "Velvet Choker" => 88,
        "Cursed Key" => 82,
        "Black Blood" => 80,
        "FrozenCore" | "Frozen Core" => 72,
        "Nuclear Battery" => 70,
        "Runic Pyramid" => 76,
        "Coffee Dripper" => 75,
        "Fusion Hammer" => 72,
        "Tiny House" => 60,
        "Busted Crown" => 20,
        "Snecko Eye" => 15,
        "Runic Dome" => 5,
        "Calling Bell" => 8,
        _ => 55,
    }
}

trait FromJavaClass {
    fn from_java_class(s: &str) -> Option<RoomType>;
}

impl FromJavaClass for RoomType {
    fn from_java_class(s: &str) -> Option<RoomType> {
        Some(if s.contains("MonsterRoomElite") {
            RoomType::Elite
        } else if s.contains("MonsterRoomBoss") {
            RoomType::Boss
        } else if s.contains("MonsterRoom") {
            RoomType::Monster
        } else if s.contains("RestRoom") {
            RoomType::Rest
        } else if s.contains("ShopRoom") {
            RoomType::Shop
        } else if s.contains("EventRoom") {
            RoomType::Event
        } else if s.contains("TreasureRoom") {
            RoomType::Treasure
        } else {
            return None;
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unlocks::Unlocks;

    #[test]
    fn elite_matchups_reflect_deck_shape() {
        let weak = DeckMetrics::default();
        assert_eq!(elite_matchup(Some("Gremlin Nob"), weak, 0), -60);
        assert_eq!(elite_matchup(Some("3 Sentries"), weak, 0), -40);

        let prepared = DeckMetrics {
            aoe: 2,
            big_attacks: 2,
            block_cards: 6,
            ..DeckMetrics::default()
        };
        assert_eq!(elite_matchup(Some("Gremlin Nob"), prepared, 6), 15);
        assert_eq!(elite_matchup(Some("3 Sentries"), prepared, 6), 20);
        assert_eq!(elite_matchup(Some("Book of Stabbing"), prepared, 6), 15);
    }

    #[test]
    fn defect_engine_and_claw_are_draftable() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        assert!(score_card(&game, &Card::new(CardId::Defragment)) > 300);
        game.player.deck.push(Card::new(CardId::All_For_One));
        game.player.deck.push(Card::new(CardId::Gash));
        game.player.deck.push(Card::new(CardId::Gash));
        assert!(score_card(&game, &Card::new(CardId::Gash)) >= PICK_THRESHOLD);
        assert!(score_card(&game, &Card::new(CardId::Rebound)) >= PICK_THRESHOLD);
    }

    #[test]
    fn event_hp_guard_distinguishes_hp_from_max_hp() {
        let sacrifice = strip_event_markup("[Sacrifice] #gMax #gHP #g+5. #rLose #r19 #rHP.")
            .to_ascii_lowercase();
        let retrace =
            strip_event_markup("[Retrace Your Steps] #rLose #r4 #rMax #rHP.").to_ascii_lowercase();
        assert_eq!(event_hp_loss(&sacrifice, 75), 19);
        assert_eq!(event_hp_loss(&retrace, 75), 0);
    }
}
