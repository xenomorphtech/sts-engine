//! Native combat-HRM inference and the stable state tokenizer shared with the trainer.

use crate::ids::EncounterId;
use crate::Action;
use ort::ep::{self, ExecutionProvider};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const RUNTIME_FORMAT: &str = "sts-combat-hrm-onnx";
const DEFAULT_ORT_LIBRARY: &str = "/usr/lib/libonnxruntime.so";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Act3Boss {
    AwakenedOne,
    DonuAndDeca,
    TimeEater,
}

impl Act3Boss {
    pub fn from_encounter(encounter: EncounterId) -> Option<Self> {
        match encounter {
            EncounterId::AwakenedOne => Some(Self::AwakenedOne),
            EncounterId::DonuAndDeca => Some(Self::DonuAndDeca),
            EncounterId::TimeEater => Some(Self::TimeEater),
            _ => None,
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AwakenedOne => "AwakenedOne",
            Self::DonuAndDeca => "DonuAndDeca",
            Self::TimeEater => "TimeEater",
        }
    }
}

impl std::fmt::Display for Act3Boss {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HrmDevice {
    Auto,
    Cuda,
    Cpu,
}

impl HrmDevice {
    pub fn from_cli(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "cuda" => Some(Self::Cuda),
            "cpu" => Some(Self::Cpu),
            _ => None,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ModelDefaults {
    max_tokens: usize,
    batch_size: usize,
}

#[derive(Debug, Deserialize)]
struct RuntimeMetadata {
    schema_version: u32,
    format: String,
    onnx_sha256: String,
    vocabulary: Vec<String>,
    action_list: Vec<String>,
    model_defaults: ModelDefaults,
    split_map: HashMap<String, String>,
}

pub struct HrmInferenceInput<'a> {
    pub boss: Act3Boss,
    pub state: &'a Value,
    pub legal_actions: &'a [Action],
}

pub struct HrmInferenceChoice {
    pub action: Action,
    pub fallback: bool,
}

pub struct HrmPolicy {
    session: Session,
    token_to_id: HashMap<String, i64>,
    action_to_id: HashMap<String, usize>,
    actions: Vec<Action>,
    max_tokens: usize,
    batch_size: usize,
    split_map: HashMap<usize, String>,
    device: &'static str,
    encoding_time: Duration,
    inference_time: Duration,
}

impl HrmPolicy {
    pub fn load(
        metadata_path: &Path,
        onnx_path: &Path,
        requested_device: HrmDevice,
    ) -> Result<Self, String> {
        let metadata_bytes = std::fs::read(metadata_path)
            .map_err(|error| format!("read {}: {error}", metadata_path.display()))?;
        let metadata: RuntimeMetadata = serde_json::from_slice(&metadata_bytes)
            .map_err(|error| format!("parse {}: {error}", metadata_path.display()))?;
        if metadata.schema_version != 1 || metadata.format != RUNTIME_FORMAT {
            return Err(format!(
                "{} is not a supported combat HRM runtime manifest",
                metadata_path.display()
            ));
        }
        if metadata.model_defaults.max_tokens == 0
            || metadata.model_defaults.batch_size == 0
            || metadata.vocabulary.len() < 3
            || metadata.action_list.is_empty()
        {
            return Err(format!(
                "{} contains invalid model dimensions",
                metadata_path.display()
            ));
        }
        let actual_onnx_hash = sha256_file(onnx_path)?;
        if actual_onnx_hash != metadata.onnx_sha256 {
            return Err(format!(
                "ONNX hash differs from {}: expected {}, got {}",
                metadata_path.display(),
                metadata.onnx_sha256,
                actual_onnx_hash
            ));
        }

        let ort_library = std::env::var_os("ORT_DYLIB_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_ORT_LIBRARY));
        let initialized = ort::init_from(&ort_library)
            .map_err(|error| format!("load {}: {error}", ort_library.display()))?
            .commit();
        if !initialized {
            return Err("ONNX Runtime was initialized earlier with different settings".to_string());
        }

        let cuda = ep::CUDA::default().with_device_id(0);
        let cuda_available = cuda
            .is_available()
            .map_err(|error| format!("query CUDA execution provider: {error}"))?;
        let use_cuda = match requested_device {
            HrmDevice::Cpu => false,
            HrmDevice::Cuda => {
                if !cuda_available {
                    return Err(
                        "CUDA was requested but ONNX Runtime has no CUDA provider".to_string()
                    );
                }
                true
            }
            HrmDevice::Auto => cuda_available,
        };
        let mut builder = Session::builder()
            .map_err(|error| format!("create ONNX session builder: {error}"))?
            .with_optimization_level(GraphOptimizationLevel::All)
            .map_err(|error| format!("configure ONNX graph optimization: {error}"))?
            .with_memory_pattern(false)
            .map_err(|error| format!("configure dynamic-batch memory handling: {error}"))?;
        if use_cuda {
            builder = builder
                .with_execution_providers([ep::CUDA::default()
                    .with_device_id(0)
                    .with_arena_extend_strategy(ep::ArenaExtendStrategy::SameAsRequested)
                    .build()
                    .error_on_failure()])
                .map_err(|error| format!("enable CUDA inference: {error}"))?;
        } else {
            builder = builder
                .with_intra_threads(
                    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get),
                )
                .map_err(|error| format!("configure CPU inference threads: {error}"))?;
        }
        let session = builder
            .commit_from_file(onnx_path)
            .map_err(|error| format!("load {}: {error}", onnx_path.display()))?;

        let token_to_id = metadata
            .vocabulary
            .into_iter()
            .enumerate()
            .map(|(index, token)| (token, index as i64))
            .collect();
        let mut actions = Vec::with_capacity(metadata.action_list.len());
        let mut action_to_id = HashMap::with_capacity(metadata.action_list.len());
        for (index, encoded) in metadata.action_list.into_iter().enumerate() {
            let action: Action = serde_json::from_str(&encoded)
                .map_err(|error| format!("invalid runtime action {encoded:?}: {error}"))?;
            action_to_id.insert(encoded, index);
            actions.push(action);
        }
        let split_map = metadata
            .split_map
            .into_iter()
            .map(|(index, split)| {
                index
                    .parse::<usize>()
                    .map(|index| (index, split))
                    .map_err(|error| format!("invalid split-map index {index:?}: {error}"))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        // Two training batches amortize launches without exhausting an 8 GiB
        // card on the model's quadratic 384-token attention workspace.
        let batch_size = metadata.model_defaults.batch_size.saturating_mul(2).max(1);
        Ok(Self {
            session,
            token_to_id,
            action_to_id,
            actions,
            max_tokens: metadata.model_defaults.max_tokens,
            batch_size,
            split_map,
            device: if use_cuda { "cuda" } else { "cpu" },
            encoding_time: Duration::ZERO,
            inference_time: Duration::ZERO,
        })
    }

    pub fn device(&self) -> &'static str {
        self.device
    }

    pub fn split(&self, puzzle_index: usize) -> Option<&str> {
        self.split_map.get(&puzzle_index).map(String::as_str)
    }

    pub fn encoding_time(&self) -> Duration {
        self.encoding_time
    }

    pub fn inference_time(&self) -> Duration {
        self.inference_time
    }

    pub fn choose(
        &mut self,
        inputs: &[HrmInferenceInput<'_>],
    ) -> Result<Vec<HrmInferenceChoice>, String> {
        let mut choices = Vec::with_capacity(inputs.len());
        for chunk in inputs.chunks(self.batch_size) {
            let encoding_started = Instant::now();
            let mut input_ids = vec![0_i64; chunk.len() * self.max_tokens];
            let encoded_rows = chunk
                .par_iter()
                .map(|input| {
                    let tokens = state_tokens(input.boss, input.state, input.legal_actions);
                    let token_ids = tokens
                        .into_iter()
                        .take(self.max_tokens)
                        .map(|token| self.token_to_id.get(&token).copied().unwrap_or(1))
                        .collect::<Vec<_>>();
                    let legal_ids = input
                        .legal_actions
                        .iter()
                        .filter_map(|action| {
                            self.action_to_id
                                .get(&canonical_action_key(action))
                                .copied()
                        })
                        .collect::<Vec<_>>();
                    (token_ids, legal_ids)
                })
                .collect::<Vec<_>>();
            let mut legal_ids = Vec::with_capacity(chunk.len());
            for (row, (token_ids, row_legal_ids)) in encoded_rows.into_iter().enumerate() {
                let offset = row * self.max_tokens;
                input_ids[offset..offset + token_ids.len()].copy_from_slice(&token_ids);
                legal_ids.push(row_legal_ids);
            }
            self.encoding_time += encoding_started.elapsed();

            let inference_started = Instant::now();
            let tensor = Tensor::<i64>::from_array(([chunk.len(), self.max_tokens], input_ids))
                .map_err(|error| format!("build HRM input tensor: {error}"))?;
            let outputs = self
                .session
                .run(ort::inputs!["input_ids" => tensor])
                .map_err(|error| format!("run HRM ONNX inference: {error}"))?;
            let (shape, logits) = outputs["action_logits"]
                .try_extract_tensor::<f32>()
                .map_err(|error| format!("read HRM action logits: {error}"))?;
            if shape.as_ref() != [chunk.len() as i64, self.actions.len() as i64] {
                return Err(format!(
                    "unexpected HRM output shape {:?}; expected [{}, {}]",
                    shape,
                    chunk.len(),
                    self.actions.len()
                ));
            }
            self.inference_time += inference_started.elapsed();
            for (row, input) in chunk.iter().enumerate() {
                if legal_ids[row].is_empty() {
                    let action = input
                        .legal_actions
                        .first()
                        .ok_or_else(|| "HRM decision has no legal action".to_string())?
                        .clone();
                    choices.push(HrmInferenceChoice {
                        action,
                        fallback: true,
                    });
                    continue;
                }
                let row_logits = &logits[row * self.actions.len()..(row + 1) * self.actions.len()];
                let best_id = legal_ids[row]
                    .iter()
                    .copied()
                    .max_by(|left, right| row_logits[*left].total_cmp(&row_logits[*right]))
                    .expect("known legal actions is non-empty");
                choices.push(HrmInferenceChoice {
                    action: self.actions[best_id].clone(),
                    fallback: false,
                });
            }
        }
        Ok(choices)
    }
}

fn sha256_file(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut source =
        std::fs::File::open(path).map_err(|error| format!("open {}: {error}", path.display()))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn canonical_action_key(action: &Action) -> String {
    let value = serde_json::to_value(action).expect("Action serialization cannot fail");
    canonical_json(&value)
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Array(values) => format!(
            "[{}]",
            values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Value::Object(values) => {
            let mut fields = values.iter().collect::<Vec<_>>();
            fields.sort_by_key(|(key, _)| *key);
            format!(
                "{{{}}}",
                fields
                    .into_iter()
                    .map(|(key, value)| format!(
                        "{}:{}",
                        serde_json::to_string(key).expect("JSON key serialization cannot fail"),
                        canonical_json(value)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        value => serde_json::to_string(value).expect("JSON scalar serialization cannot fail"),
    }
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(Value::Number(value)) => value.as_f64().is_some_and(|value| value != 0.0),
        Some(Value::String(value)) => !value.is_empty(),
        Some(Value::Array(value)) => !value.is_empty(),
        Some(Value::Object(value)) => !value.is_empty(),
    }
}

fn python_string(value: Option<&Value>, missing: &str) -> String {
    match value {
        None => missing.to_string(),
        Some(Value::Null) => "None".to_string(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
    }
}

fn bucket_signed(value: i64) -> String {
    if (-32..=128).contains(&value) {
        return value.to_string();
    }
    let magnitude = value.unsigned_abs();
    if magnitude <= 1024 {
        return format!("q8:{}", ((value as f64 / 8.0).round_ties_even() as i64) * 8);
    }
    let sign = if value < 0 { 'n' } else { 'p' };
    format!("{sign}log2:{}", 63 - magnitude.leading_zeros())
}

fn bucket_unsigned(value: u64) -> String {
    if value <= 128 {
        return value.to_string();
    }
    if value <= 1024 {
        return format!("q8:{}", ((value as f64 / 8.0).round_ties_even() as u64) * 8);
    }
    format!("plog2:{}", 63 - value.leading_zeros())
}

fn bucket_number(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "none".to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(value)) => {
            if let Some(value) = value.as_i64() {
                bucket_signed(value)
            } else if let Some(value) = value.as_u64() {
                bucket_unsigned(value)
            } else if let Some(value) = value.as_f64() {
                let percent = (value * 100.0).round_ties_even() as i64;
                format!("pct:{}", percent.clamp(-1000, 1000))
            } else {
                value.to_string()
            }
        }
        Some(Value::String(value)) => value.clone(),
        Some(value) => value.to_string(),
    }
}

fn scalar_token(prefix: &str, value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => format!("{prefix}={value}"),
        _ => format!("{prefix}={}", bucket_number(value)),
    }
}

fn scalar_zero(prefix: &str, object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .map(|value| scalar_token(prefix, Some(value)))
        .unwrap_or_else(|| format!("{prefix}=0"))
}

fn card_token(zone: &str, card: &Map<String, Value>) -> String {
    let mut flags = String::new();
    for key in [
        "free_to_play_once",
        "exhaust",
        "ethereal",
        "retain",
        "innate",
        "in_bottle",
    ] {
        if is_truthy(card.get(key)) {
            flags.push(key.as_bytes()[0] as char);
        }
    }
    if flags.is_empty() {
        flags.push('-');
    }
    format!(
        "CARD:{zone}:{}:u{}:tu{}:c{}:ct{}:m{}:f{flags}",
        python_string(card.get("id"), "?"),
        usize::from(is_truthy(card.get("upgraded"))),
        bucket_number(card.get("times_upgraded").or_else(|| None)),
        bucket_number(card.get("cost").or_else(|| None)),
        bucket_number(card.get("cost_for_turn").or_else(|| None)),
        bucket_number(card.get("misc").or_else(|| None)),
    )
}

fn card_token_with_defaults(zone: &str, card: &Map<String, Value>) -> String {
    let mut normalized = card.clone();
    for key in ["times_upgraded", "misc"] {
        normalized.entry(key.to_string()).or_insert(Value::from(0));
    }
    for key in ["cost", "cost_for_turn"] {
        normalized.entry(key.to_string()).or_insert(Value::from(-1));
    }
    card_token(zone, &normalized)
}

fn add_power_tokens(tokens: &mut Vec<String>, owner: &str, powers: Option<&Value>) {
    let Some(powers) = powers.and_then(Value::as_array) else {
        return;
    };
    for power in powers {
        let Some(power) = power.as_object() else {
            continue;
        };
        let id = python_string(power.get("id"), "?");
        tokens.push(format!("POWER:{owner}:{id}"));
        for key in ["amount", "misc"] {
            tokens.push(scalar_zero(
                &format!("POWER:{owner}:{id}:{key}"),
                power,
                key,
            ));
        }
        if is_truthy(power.get("just_applied")) {
            tokens.push(format!("POWER:{owner}:{id}:just"));
        }
        if is_truthy(power.get("skip_first")) {
            tokens.push(format!("POWER:{owner}:{id}:skip"));
        }
    }
}

fn signed_state_bits(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Number(value)) => value
            .as_i64()
            .map(|value| value as u64)
            .or_else(|| value.as_u64())
            .unwrap_or(0),
        Some(Value::String(value)) => value.parse::<i64>().map_or(0, |value| value as u64),
        _ => 0,
    }
}

fn add_rng_tokens(tokens: &mut Vec<String>, rng: Option<&Value>) {
    let Some(rng) = rng.and_then(Value::as_object) else {
        return;
    };
    for stream in ["ai", "card", "card_random", "misc", "monster", "shuffle"] {
        let Some(state) = rng.get(stream).and_then(Value::as_object) else {
            continue;
        };
        tokens.push(scalar_zero(
            &format!("RNG:{stream}:counter"),
            state,
            "counter",
        ));
        for field in ["state0", "state1"] {
            let raw = signed_state_bits(state.get(field));
            for byte_index in 0..8 {
                let byte = (raw >> (byte_index * 8)) & 0xff;
                tokens.push(format!(
                    "RNG:{stream}:{field}:b{byte_index}:hi={}",
                    byte >> 4
                ));
            }
        }
    }
}

fn add_cards(tokens: &mut Vec<String>, zone: &str, cards: Option<&Value>) {
    let Some(cards) = cards.and_then(Value::as_array) else {
        return;
    };
    tokens.extend(
        cards
            .iter()
            .filter_map(Value::as_object)
            .map(|card| card_token_with_defaults(zone, card)),
    );
}

fn state_tokens(boss: Act3Boss, state: &Value, legal_actions: &[Action]) -> Vec<String> {
    let empty = Map::new();
    let state = state.as_object().unwrap_or(&empty);
    let game = state
        .get("game")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let player = state
        .get("player")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let combat = state
        .get("combat")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let mut tokens = vec!["[CLS]".to_string(), format!("BOSS={boss}")];

    for key in [
        "screen",
        "current_room",
        "ascension",
        "character",
        "card_blizz",
        "potion_blizzard",
    ] {
        if let Some(value) = game.get(key) {
            tokens.push(scalar_token(&format!("GAME:{key}"), Some(value)));
        }
    }
    if let Some(keys) = game.get("keys").and_then(Value::as_object) {
        let mut keys = keys.iter().collect::<Vec<_>>();
        keys.sort_by_key(|(key, _)| *key);
        for (key, value) in keys {
            tokens.push(scalar_token(&format!("KEY:{key}"), Some(value)));
        }
    }
    add_cards(&mut tokens, "pending", game.get("pending_cards"));
    if let Some(grid) = game.get("grid").filter(|value| !value.is_null()) {
        if let Some(grid) = grid.as_object() {
            tokens.push(scalar_token("GRID:kind", grid.get("kind")));
            for key in ["confirm", "can_cancel"] {
                if let Some(value) = grid.get(key) {
                    tokens.push(scalar_token(&format!("GRID:{key}"), Some(value)));
                }
            }
        } else {
            tokens.push(scalar_token("GRID", Some(grid)));
        }
    }
    if is_truthy(game.get("hand_select")) {
        let length = game
            .get("hand_select")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        tokens.push(format!("HAND_SELECT={length}"));
    }

    for key in [
        "hp",
        "max_hp",
        "block",
        "energy",
        "energy_master",
        "gold",
        "max_orbs",
        "master_max_orbs",
        "potion_slots",
        "pending_evoke_dark",
        "pending_evoke_frost",
        "pending_evoke_lightning",
        "pending_static",
    ] {
        if let Some(value) = player.get(key) {
            tokens.push(scalar_token(&format!("PLAYER:{key}"), Some(value)));
        }
    }

    for key in [
        "encounter",
        "turn",
        "cards_played_this_turn",
        "echo_cards_duplicated_this_turn",
        "skills_this_turn",
        "attacks_this_turn",
        "orange_pellets_mask",
        "draw_after_exhaust",
        "pending_dark_embrace",
        "pending_ink_bottle",
        "pending_letter_opener",
        "pending_hex_after_seek",
        "energy_on_use",
        "force_end_turn",
        "need_exhaust_select",
        "need_put_on_deck",
        "need_discard_to_hand",
        "need_draw_to_hand",
        "need_discovery",
        "need_forethought",
        "need_skill_from_deck",
        "pending_rebound",
    ] {
        if let Some(value) = combat.get(key) {
            tokens.push(scalar_token(&format!("COMBAT:{key}"), Some(value)));
        }
    }

    if let Some(monsters) = combat.get("monsters").and_then(Value::as_array) {
        for (monster_index, monster) in monsters.iter().enumerate() {
            let Some(monster) = monster.as_object() else {
                continue;
            };
            let id = python_string(monster.get("id"), "?");
            let owner = format!("M{monster_index}:{id}");
            tokens.push(format!(
                "MONSTER:{owner}:intent={}:dead={}:half={}",
                python_string(monster.get("intent"), "?"),
                usize::from(is_truthy(monster.get("dead"))),
                usize::from(is_truthy(monster.get("half_dead"))),
            ));
            for key in [
                "hp",
                "max_hp",
                "block",
                "intent_damage",
                "intent_base_damage",
                "intent_hits",
                "next_move",
                "extra",
                "pending_reactive",
                "pending_curl",
                "pending_hand_drill",
            ] {
                tokens.push(scalar_zero(&format!("MONSTER:{owner}:{key}"), monster, key));
            }
            if let Some(history) = monster.get("move_history").and_then(Value::as_array) {
                for (offset, move_id) in history
                    .iter()
                    .skip(history.len().saturating_sub(4))
                    .enumerate()
                {
                    tokens.push(scalar_token(
                        &format!("MONSTER:{owner}:history:{offset}"),
                        Some(move_id),
                    ));
                }
            }
            add_power_tokens(&mut tokens, &owner, monster.get("powers"));
        }
    }

    tokens.push("ZONE:hand".to_string());
    add_cards(&mut tokens, "hand", player.get("hand"));

    if let Some(orbs) = player.get("orbs").and_then(Value::as_array) {
        for (orb_index, orb) in orbs.iter().enumerate() {
            let Some(orb) = orb.as_object() else {
                continue;
            };
            let kind = python_string(orb.get("kind"), "?");
            tokens.push(format!("ORB:{orb_index}:{kind}"));
            let mut fields = orb.iter().collect::<Vec<_>>();
            fields.sort_by_key(|(key, _)| *key);
            for (key, value) in fields {
                if key != "kind" {
                    tokens.push(scalar_token(
                        &format!("ORB:{orb_index}:{kind}:{key}"),
                        Some(value),
                    ));
                }
            }
        }
    }

    add_power_tokens(&mut tokens, "PLAYER", player.get("powers"));
    if let Some(relics) = player.get("relics").and_then(Value::as_array) {
        for relic in relics.iter().filter_map(Value::as_object) {
            let id = python_string(relic.get("id"), "?");
            tokens.push(format!("RELIC:{id}"));
            tokens.push(
                relic
                    .get("counter")
                    .map(|value| scalar_token(&format!("RELIC:{id}:counter"), Some(value)))
                    .unwrap_or_else(|| format!("RELIC:{id}:counter=-1")),
            );
            if is_truthy(relic.get("used_up")) {
                tokens.push(format!("RELIC:{id}:used"));
            }
        }
    }
    if let Some(potions) = player.get("potions").and_then(Value::as_array) {
        for potion in potions.iter().filter_map(Value::as_object) {
            tokens.push(format!(
                "POTION:{}:{}",
                python_string(potion.get("slot"), "?"),
                python_string(potion.get("id"), "?"),
            ));
        }
    }

    tokens.extend(
        legal_actions
            .iter()
            .map(|action| format!("LEGAL:{}", canonical_action_key(action))),
    );

    for zone in ["draw", "discard", "exhaust"] {
        tokens.push(format!("ZONE:{zone}"));
        add_cards(&mut tokens, zone, player.get(zone));
    }

    let mut deck_counts = BTreeMap::<(String, bool), usize>::new();
    if let Some(deck) = player.get("deck").and_then(Value::as_array) {
        for card in deck.iter().filter_map(Value::as_object) {
            *deck_counts
                .entry((
                    python_string(card.get("id"), "?"),
                    is_truthy(card.get("upgraded")),
                ))
                .or_default() += 1;
        }
    }
    for ((id, upgraded), count) in deck_counts {
        tokens.push(format!(
            "DECK:{id}:u{}:n{}",
            usize::from(upgraded),
            bucket_unsigned(count as u64)
        ));
    }

    add_rng_tokens(&mut tokens, state.get("rng"));
    tokens
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn act_three_boss_is_closed_and_json_compatible() {
        for (boss, wire) in [
            (Act3Boss::AwakenedOne, r#""AwakenedOne""#),
            (Act3Boss::DonuAndDeca, r#""DonuAndDeca""#),
            (Act3Boss::TimeEater, r#""TimeEater""#),
        ] {
            assert_eq!(serde_json::to_string(&boss).unwrap(), wire);
            assert_eq!(serde_json::from_str::<Act3Boss>(wire).unwrap(), boss);
        }
        assert!(serde_json::from_str::<Act3Boss>(r#""GiantHead""#).is_err());
        assert_eq!(
            Act3Boss::from_encounter(EncounterId::TimeEater),
            Some(Act3Boss::TimeEater)
        );
        assert_eq!(Act3Boss::from_encounter(EncounterId::GiantHead), None);
    }

    #[test]
    fn python_number_buckets_match_boundaries_and_ties() {
        assert_eq!(bucket_number(Some(&json!(128))), "128");
        assert_eq!(bucket_number(Some(&json!(132))), "q8:128");
        assert_eq!(bucket_number(Some(&json!(140))), "q8:144");
        assert_eq!(bucket_number(Some(&json!(-36))), "q8:-32");
        assert_eq!(bucket_number(Some(&json!(1025))), "plog2:10");
        assert_eq!(bucket_number(Some(&json!(-1025))), "nlog2:10");
        assert_eq!(bucket_number(Some(&json!(1.234))), "pct:123");
    }

    #[test]
    fn action_key_is_canonical() {
        assert_eq!(
            canonical_action_key(&Action::Play {
                hand_index: 3,
                target_index: Some(1),
            }),
            r#"{"hand_index":3,"op":"play","target_index":1}"#
        );
    }

    #[test]
    fn card_flags_and_defaults_match_training_tokenizer() {
        let card = json!({
            "id": "Zap",
            "upgraded": true,
            "exhaust": true,
            "ethereal": true
        });
        assert_eq!(
            card_token_with_defaults("hand", card.as_object().unwrap()),
            "CARD:hand:Zap:u1:tu0:c-1:ct-1:m0:fee"
        );
    }

    #[test]
    fn rng_tokens_use_coarse_position_aware_bytes() {
        let rng = json!({
            "ai": {
                "counter": 3,
                "state0": 0x7f10,
                "state1": -1
            }
        });
        let mut tokens = Vec::new();
        add_rng_tokens(&mut tokens, Some(&rng));
        assert_eq!(tokens[0], "RNG:ai:counter=3");
        assert_eq!(tokens[1], "RNG:ai:state0:b0:hi=1");
        assert_eq!(tokens[2], "RNG:ai:state0:b1:hi=7");
        assert_eq!(tokens[9], "RNG:ai:state1:b0:hi=15");
        assert_eq!(tokens[16], "RNG:ai:state1:b7:hi=15");
    }
}
