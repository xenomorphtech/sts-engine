use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use sts_engine::game::{Game, GridKind, Screen};
use sts_engine::ids::RoomType;
use sts_engine::Action;

const CHECKPOINT_VERSION: u32 = 2;
const FEATURE_SCHEMA: &str = "candidate-context-cross-v2";

#[derive(Debug, Deserialize)]
struct Checkpoint {
    version: u32,
    feature_schema: String,
    generation: usize,
    dimensions: usize,
    weights: Vec<f64>,
    #[serde(default)]
    best_generation: Option<usize>,
    #[serde(default)]
    best_weights: Vec<f64>,
    #[serde(default)]
    default_weight_source: Option<String>,
}

/// Immutable learned deck-building policy shared by all seed workers.
pub struct LearnedDeckPolicy {
    weights: Vec<f64>,
    mask: usize,
    generation: usize,
    weight_source: String,
}

impl LearnedDeckPolicy {
    pub fn load(path: &Path) -> Result<Self, String> {
        let input =
            BufReader::new(File::open(path).map_err(|error| {
                format!("open learned deck policy {}: {error}", path.display())
            })?);
        let checkpoint: Checkpoint = serde_json::from_reader(input)
            .map_err(|error| format!("parse learned deck policy {}: {error}", path.display()))?;
        Self::from_checkpoint(checkpoint)
            .map_err(|error| format!("invalid learned deck policy {}: {error}", path.display()))
    }

    fn from_checkpoint(checkpoint: Checkpoint) -> Result<Self, String> {
        if checkpoint.version != CHECKPOINT_VERSION {
            return Err(format!(
                "checkpoint version {} is unsupported (expected {CHECKPOINT_VERSION})",
                checkpoint.version
            ));
        }
        if checkpoint.feature_schema != FEATURE_SCHEMA {
            return Err(format!(
                "feature schema {:?} is unsupported (expected {FEATURE_SCHEMA:?})",
                checkpoint.feature_schema
            ));
        }
        if checkpoint.dimensions < 256 || !checkpoint.dimensions.is_power_of_two() {
            return Err(format!(
                "dimensions must be a power of two of at least 256, got {}",
                checkpoint.dimensions
            ));
        }

        // New checkpoints can explicitly select best_weights. Older checkpoints
        // intentionally default to their current weights, which is the policy
        // most recently trained and the source requested for normal runs.
        let weight_source = checkpoint
            .default_weight_source
            .unwrap_or_else(|| "weights".to_string());
        let (weights, generation) = match weight_source.as_str() {
            "weights" => (checkpoint.weights, checkpoint.generation),
            "best_weights" => {
                if checkpoint.best_weights.is_empty() {
                    return Err("default_weight_source is best_weights but none are stored".into());
                }
                (
                    checkpoint.best_weights,
                    checkpoint.best_generation.unwrap_or(checkpoint.generation),
                )
            }
            other => {
                return Err(format!(
                    "default_weight_source must be weights or best_weights, got {other:?}"
                ));
            }
        };
        if weights.len() != checkpoint.dimensions {
            return Err(format!(
                "selected {weight_source} length {} does not match dimensions {}",
                weights.len(),
                checkpoint.dimensions
            ));
        }
        if weights.iter().any(|weight| !weight.is_finite()) {
            return Err(format!(
                "selected {weight_source} contains a non-finite value"
            ));
        }

        Ok(Self {
            weights,
            mask: checkpoint.dimensions - 1,
            generation,
            weight_source,
        })
    }

    pub fn generation(&self) -> usize {
        self.generation
    }

    pub fn weight_source(&self) -> &str {
        &self.weight_source
    }

    pub fn start_run(&self) -> LearnedDeckRun<'_> {
        LearnedDeckRun {
            policy: self,
            tried: HashMap::new(),
        }
    }

    fn feature_index(&self, token: &str) -> usize {
        crc32fast::hash(token.as_bytes()) as usize & self.mask
    }

    fn score_token(&self, token: &str, value: f64) -> f64 {
        self.weights[self.feature_index(token)] * value
    }

    fn score(
        &self,
        game: &Game,
        context: DecisionContext,
        actions: &[Action],
        action: &Action,
    ) -> f64 {
        let offer_tokens = offer_tokens(action);
        let hp_ratio = f64::from(game.player.hp) / f64::from(game.player.max_hp.max(1));
        let scalar_context = [
            format!("phase={}", context.phase),
            format!("screen={:?}", game.screen),
            format!("act={}", game.dungeon.act as i32),
            format!("source={}", context.source.map_or("None", |source| source)),
            format!("gold_bucket={}", (game.player.gold / 50).min(10)),
            format!("hp_bucket={}", ((hp_ratio * 10.0) as i32).min(10)),
            format!("deck_bucket={}", (game.player.deck.len() / 5).min(10)),
            format!("offers={}", actions.len()),
            format!("shop_slots={}", context.shop_slots),
            format!(
                "opportunities_bucket={}",
                opportunities_remaining(game).saturating_div(5).min(15)
            ),
            format!("energy={}", game.player.energy_master),
        ];

        let mut deck = BTreeMap::<String, usize>::new();
        let mut upgraded = BTreeMap::<String, usize>::new();
        for card in &game.player.deck {
            let id = card.sts_id().to_string();
            *deck.entry(id.clone()).or_default() += 1;
            if card.upgraded {
                *upgraded.entry(id).or_default() += 1;
            }
        }
        let relics = game
            .player
            .relics
            .iter()
            .map(|relic| relic.id.sts_id().to_string())
            .collect::<BTreeSet<_>>();

        let mut score = 0.0;
        for offer_token in &offer_tokens {
            score += self.score_token(&format!("offer:{offer_token}"), 1.0);
            for scalar in &scalar_context {
                score += self.score_token(&format!("offer_context:{offer_token}|{scalar}"), 1.0);
            }
            for (card_id, count) in &deck {
                score += self.score_token(
                    &format!("offer_deck:{offer_token}|{card_id}"),
                    (*count as f64).sqrt(),
                );
                score += self.score_token(
                    &format!(
                        "offer_deck_count:{offer_token}|{card_id}|{}",
                        (*count).min(6)
                    ),
                    1.0,
                );
            }
            for (card_id, count) in &upgraded {
                score += self.score_token(
                    &format!("offer_upgraded:{offer_token}|{card_id}"),
                    (*count as f64).sqrt(),
                );
            }
            for relic_id in &relics {
                score += self.score_token(&format!("offer_relic:{offer_token}|{relic_id}"), 1.0);
            }

            score += self.score_token(
                &format!(
                    "offer_shape:{offer_token}|unique_cards={}",
                    deck.len().min(20)
                ),
                1.0,
            );
            score += self.score_token(
                &format!("offer_shape:{offer_token}|relics={}", relics.len().min(20)),
                1.0,
            );
            score += self.score_token(
                &format!(
                    "offer_shape:{offer_token}|upgraded={}",
                    upgraded.values().sum::<usize>().min(20)
                ),
                1.0,
            );
        }
        score
    }
}

/// Per-seed learned state. The tried set mirrors the training driver's cycle
/// guard so a repeated menu state cannot choose the same action forever.
pub struct LearnedDeckRun<'a> {
    policy: &'a LearnedDeckPolicy,
    tried: HashMap<String, HashSet<usize>>,
}

impl LearnedDeckRun<'_> {
    pub fn decide(&mut self, game: &Game) -> Option<Action> {
        let context = decision_context(game)?;
        let actions = game
            .legal_actions()
            .into_iter()
            .filter(|action| !matches!(action, Action::Potion { .. } | Action::Quit))
            .collect::<Vec<_>>();
        if actions.is_empty() || unopened_shop(game, &actions) {
            return None;
        }

        let fingerprint = decision_fingerprint(game, context, &actions);
        let tried = self.tried.entry(fingerprint).or_default();
        let mut best: Option<(usize, f64)> = None;
        for (index, action) in actions.iter().enumerate() {
            if tried.contains(&index) {
                continue;
            }
            let score = self.policy.score(game, context, &actions, action);
            if best.is_none_or(|(_, best_score)| score > best_score) {
                best = Some((index, score));
            }
        }
        let (index, _) = best?;
        tried.insert(index);
        Some(actions[index].clone())
    }
}

#[derive(Clone, Copy)]
struct DecisionContext {
    phase: &'static str,
    source: Option<&'static str>,
    shop_slots: usize,
}

fn decision_context(game: &Game) -> Option<DecisionContext> {
    match game.screen {
        Screen::Neow => Some(DecisionContext {
            phase: "neow",
            source: None,
            shop_slots: 0,
        }),
        Screen::CardReward if game.combat.is_none() => {
            if game.dungeon.floor == 0 {
                Some(DecisionContext {
                    phase: "neow",
                    source: None,
                    shop_slots: 0,
                })
            } else if matches!(
                game.current_room,
                RoomType::Shop | RoomType::Treasure | RoomType::BossTreasure
            ) {
                Some(DecisionContext {
                    phase: "relic_resolution",
                    source: room_source(game.current_room),
                    shop_slots: usize::from(game.current_room == RoomType::Shop),
                })
            } else {
                let (phase, source) = match game.current_room {
                    RoomType::Elite => ("elite_card_reward", Some("elite_bundle")),
                    RoomType::Boss => ("boss_card_reward", Some("boss_reward")),
                    RoomType::Monster => ("card_reward", Some("normal_card_reward")),
                    _ => return None,
                };
                Some(DecisionContext {
                    phase,
                    source,
                    shop_slots: 0,
                })
            }
        }
        Screen::Shop => Some(DecisionContext {
            phase: "shop",
            source: Some("shop"),
            // Full runs do not expose the synthetic schedule's counter. One is
            // its most common decision-time value and matches the average shop.
            shop_slots: 1,
        }),
        Screen::BossRelic => Some(DecisionContext {
            phase: "boss_relic",
            source: Some("boss_reward"),
            shop_slots: 0,
        }),
        Screen::Grid if game.combat.is_none() => {
            let (kind, _) = game.grid_view()?;
            match kind {
                GridKind::Upgrade
                | GridKind::DiscardToHand
                | GridKind::DrawPileToHand
                | GridKind::SkillFromDeck
                | GridKind::Library => None,
                GridKind::Purge | GridKind::Transform if game.dungeon.floor == 0 => {
                    Some(DecisionContext {
                        phase: "neow",
                        source: None,
                        shop_slots: 0,
                    })
                }
                GridKind::Purge if game.current_room == RoomType::Shop => Some(DecisionContext {
                    phase: "relic_resolution",
                    source: Some("shop"),
                    shop_slots: 1,
                }),
                GridKind::Copy | GridKind::Bottle(_) => Some(DecisionContext {
                    phase: "relic_resolution",
                    source: room_source(game.current_room),
                    shop_slots: usize::from(game.current_room == RoomType::Shop),
                }),
                GridKind::Purge | GridKind::Transform => None,
            }
        }
        _ => None,
    }
}

fn room_source(room: RoomType) -> Option<&'static str> {
    match room {
        RoomType::Monster => Some("normal_card_reward"),
        RoomType::Elite => Some("elite_bundle"),
        RoomType::Shop => Some("shop"),
        RoomType::Treasure | RoomType::BossTreasure => Some("treasure_relic"),
        RoomType::Boss => Some("boss_reward"),
        _ => None,
    }
}

fn unopened_shop(game: &Game, actions: &[Action]) -> bool {
    game.screen == Screen::Shop
        && actions.iter().any(|action| {
            matches!(
                action,
                Action::Choose {
                    label: Some(label),
                    ..
                } if label == "shop"
            )
        })
}

fn opportunities_remaining(game: &Game) -> usize {
    // The compressed curriculum averages about 40 opportunities over the
    // three normal acts. Project full-run floor progress onto that range.
    let floor = game.dungeon.floor.clamp(0, 56) as usize;
    (56usize.saturating_sub(floor) * 40).div_ceil(56)
}

fn offer_tokens(action: &Action) -> Vec<String> {
    let action = json!({"kind": "game", "action": action});
    let mut tokens = Vec::new();
    flatten_offer(&action, "", &mut tokens);
    tokens
}

fn flatten_offer(value: &Value, prefix: &str, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            let mut keys = object.keys().collect::<Vec<_>>();
            keys.sort_unstable();
            for key in keys {
                if key == "action_index" || key == "index" {
                    continue;
                }
                let child = if prefix.is_empty() {
                    key.to_string()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_offer(&object[key], &child, output);
            }
        }
        Value::Array(values) => {
            for value in values {
                flatten_offer(value, prefix, output);
            }
        }
        Value::Null => {}
        Value::Bool(value) => output.push(format!(
            "{prefix}={}",
            if *value { "True" } else { "False" }
        )),
        Value::String(value) => output.push(format!("{prefix}={value}")),
        Value::Number(value) => output.push(format!("{prefix}={value}")),
    }
}

fn decision_fingerprint(game: &Game, context: DecisionContext, actions: &[Action]) -> String {
    let deck = game
        .player
        .deck
        .iter()
        .map(|card| {
            json!({
                "id": card.sts_id(),
                "upgraded": card.upgraded,
                "times_upgraded": card.times_upgraded,
            })
        })
        .collect::<Vec<_>>();
    let relics = game
        .player
        .relics
        .iter()
        .map(|relic| {
            json!({
                "id": relic.id.sts_id(),
                "counter": relic.counter,
                "used_up": relic.used_up,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "phase": context.phase,
        "engine_screen": format!("{:?}", game.screen),
        "source": context.source,
        "hp": game.player.hp,
        "max_hp": game.player.max_hp,
        "gold": game.player.gold,
        "deck": deck,
        "relics": relics,
        "offers": actions,
        "shop_purchase_slots_remaining": context.shop_slots,
    }))
    .expect("learned deck fingerprint contains only serializable engine values")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkpoint(source: Option<&str>) -> Checkpoint {
        Checkpoint {
            version: CHECKPOINT_VERSION,
            feature_schema: FEATURE_SCHEMA.to_string(),
            generation: 90,
            dimensions: 256,
            weights: vec![1.0; 256],
            best_generation: Some(55),
            best_weights: vec![2.0; 256],
            default_weight_source: source.map(str::to_string),
        }
    }

    #[test]
    fn crc32_indices_match_python_zlib_features() {
        let policy = LearnedDeckPolicy::from_checkpoint(Checkpoint {
            dimensions: 32768,
            weights: vec![0.0; 32768],
            best_weights: Vec::new(),
            best_generation: None,
            default_weight_source: None,
            ..checkpoint(None)
        })
        .unwrap();

        assert_eq!(policy.feature_index("offer:action.label=Claw"), 18_746);
        assert_eq!(
            policy.feature_index("offer_deck:action.label=Claw|Claw"),
            16_000
        );
        assert_eq!(
            policy.feature_index("offer_context:action.label=Claw|phase=card_reward"),
            16_314
        );
    }

    #[test]
    fn old_checkpoints_default_to_current_weights() {
        let policy = LearnedDeckPolicy::from_checkpoint(checkpoint(None)).unwrap();
        assert_eq!(policy.generation(), 90);
        assert_eq!(policy.weight_source(), "weights");
        assert_eq!(policy.weights[0], 1.0);
    }

    #[test]
    fn checkpoint_can_explicitly_default_to_best_weights() {
        let policy = LearnedDeckPolicy::from_checkpoint(checkpoint(Some("best_weights"))).unwrap();
        assert_eq!(policy.generation(), 55);
        assert_eq!(policy.weight_source(), "best_weights");
        assert_eq!(policy.weights[0], 2.0);
    }

    #[test]
    fn choose_offer_encoding_matches_training_json_shape() {
        let tokens = offer_tokens(&Action::Choose {
            index: 7,
            label: Some("Claw".into()),
            x: None,
            y: None,
            room: None,
        });
        assert_eq!(
            tokens,
            vec![
                "action.label=Claw".to_string(),
                "action.op=choose".to_string(),
                "kind=game".to_string(),
            ]
        );
    }
}
