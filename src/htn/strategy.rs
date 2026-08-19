use crate::action::Action;
use crate::card::Card;
use crate::game::{Game, Screen};
use crate::ids::{CardId, RoomType};

const PICK_THRESHOLD: i32 = 80;

pub fn map_choice(game: &Game, actions: &[Action]) -> Action {
    let hp_frac = game.player.hp as f32 / game.player.max_hp.max(1) as f32;
    let act = game.dungeon.act as i32;
    let gold = game.player.gold;
    let strength = game.player.deck.iter().filter(|c| c.upgraded).count() as i32
        + scaling_score(game);
    let need_emerald = game.dungeon.act as i32 >= 2;

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
        Action::Choose { x: Some(x), y: Some(y), room: Some(r), .. } => {
            (*x, *y, RoomType::from_java_class(r).unwrap_or(RoomType::Monster))
        }
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
                    node_value(game, e.dst_x, e.dst_y, r, hp_frac, act, gold, strength, need_emerald)
                })
                .fold(None, |acc: Option<f32>, v| Some(acc.map_or(v, |a| a.max(v))))
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
            let afford = hp_frac >= 0.65 && strength >= 3 + 2 * act;
            let mut v = if afford { 40.0 } else { -150.0 };
            if hp_frac < 0.4 {
                v = -400.0;
            }
            if need_emerald && lookup_node(game, x, y).is_some_and(|n| n.emerald_key) {
                v = if act == 1 {
                    if afford { 25.0 } else { -150.0 }
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
        if let Some(p) = legal.iter().find(|a| matches!(a, Action::Proceed | Action::Skip)) {
            return p.clone();
        }
    }
    best.cloned().unwrap_or_else(|| legal[0].clone())
}

pub fn shop_choice(_game: &Game, legal: &[Action]) -> Action {
    legal
        .iter()
        .find(|a| matches!(a, Action::Proceed | Action::Skip))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
}

pub fn rest_choice(game: &Game, legal: &[Action]) -> Action {
    let hp_frac = game.player.hp as f32 / game.player.max_hp.max(1) as f32;
    let act = game.dungeon.act as i32;
    let near_boss = game.current_y >= 13;
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

pub fn event_choice(_game: &Game, legal: &[Action]) -> Action {
    let safe = ["leave", "continue", "proceed", "ignore", "depart", "refuse", "sleep", "escape", "talk"];
    for word in safe {
        for a in legal {
            if let Action::Choose { label: Some(l), .. } = a {
                if l.to_ascii_lowercase().contains(word) {
                    return a.clone();
                }
            }
        }
    }
    legal
        .iter()
        .find(|a| matches!(a, Action::Choose { .. }))
        .cloned()
        .unwrap_or_else(|| legal[0].clone())
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

fn scaling_score(game: &Game) -> i32 {
    game.player
        .deck
        .iter()
        .filter(|c| {
            matches!(
                c.id,
                CardId::Inflame
                    | CardId::Demon_Form
                    | CardId::Feel_No_Pain
                    | CardId::Dark_Embrace
                    | CardId::Metallicize
                    | CardId::Barricade
            )
        })
        .count() as i32
}

pub fn score_card(game: &Game, card: &Card) -> i32 {
    let mut s = card_pick(card.id);
    if game.character == crate::ids::Character::Defect {
        s = s.max(defect_pick(card.id));
    }
    if card.upgraded {
        s += 15;
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
        CardId::Zap => 40,
        CardId::Dualcast => 40,
        CardId::Strike_B => 20,
        CardId::Defend_B => 20,
        _ => card_pick(id),
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
