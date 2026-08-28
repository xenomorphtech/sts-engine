//! Native, in-process training for procedural Defect elite and boss combats.
//!
//! The engine, counterfactual forks, feature encoder, model, autograd, optimizer,
//! validation, and checkpoint writer all live in this process. HTN supplies the
//! cold-start continuation policy, so training does not depend on a Python or
//! model checkpoint teacher.

use crate::env::ActionParameters;
use crate::htn::HtnAgent;
use crate::{
    BatchForkRequest, BatchedTrainEnv, ProceduralCombatKind, ProceduralCombatSpec, RunMeasurements,
    RunOutcome, TrainEnv, TrainingObservation, TRAINING_FEATURE_BUCKETS,
};
use candle_core::{DType, Device, IndexOp, Tensor, D};
use candle_nn::{
    embedding, linear, linear_no_bias, loss, ops, AdamW, Embedding, Linear, Module, Optimizer,
    ParamsAdamW, VarBuilder, VarMap,
};
use rayon::prelude::*;
use serde::Serialize;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_STATE_FEATURES: usize = 256;
const MAX_ACTION_FEATURES: usize = 16;
const MAX_INVENTORY_IDENTITIES: usize = 192;
const MAX_CANDIDATE_IDENTITIES: usize = 16;
const MAX_HISTORY_STEPS: usize = 64;
const NUMERIC_MEASUREMENTS: usize = 45;
const ACTION_NUMERIC_MEASUREMENTS: usize = 31;
const CONTEXT_PARTS: usize = 9;

type TrainingResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrainingDevice {
    Auto,
    Cuda,
    Cpu,
}

impl TrainingDevice {
    pub fn from_cli(value: &str) -> Option<Self> {
        match value {
            "auto" => Some(Self::Auto),
            "cuda" => Some(Self::Cuda),
            "cpu" => Some(Self::Cpu),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct NativeTrainingConfig {
    pub seconds: f64,
    pub ascension: i32,
    pub max_combat_actions: usize,
    pub batch_scenarios: usize,
    pub root_actions: usize,
    pub burn_in_actions: usize,
    pub final_validation_scenarios: usize,
    pub hidden_size: usize,
    pub learning_rate: f64,
    pub weight_decay: f64,
    pub score_temperature: f64,
    pub value_loss_weight: f64,
    pub seed: u64,
    pub seed_source: u64,
    pub validation_seed_source: u64,
    pub device: TrainingDevice,
    pub output: PathBuf,
}

impl Default for NativeTrainingConfig {
    fn default() -> Self {
        Self {
            seconds: 600.0,
            ascension: 20,
            max_combat_actions: 500,
            batch_scenarios: 96,
            root_actions: 4,
            burn_in_actions: 16,
            final_validation_scenarios: 120,
            hidden_size: 96,
            learning_rate: 3e-4,
            weight_decay: 0.02,
            score_temperature: 0.15,
            value_loss_weight: 1.0,
            seed: 20_260_828,
            seed_source: 20_263_001,
            validation_seed_source: 20_264_001,
            device: TrainingDevice::Auto,
            output: PathBuf::from("artifacts/selfplay/defect-a20-procedural-combat-v2.safetensors"),
        }
    }
}

impl NativeTrainingConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !self.seconds.is_finite() || self.seconds <= 0.0 {
            return Err("training seconds must be positive".to_string());
        }
        if !(0..=20).contains(&self.ascension) {
            return Err("ascension must be between 0 and 20".to_string());
        }
        if self.max_combat_actions == 0
            || self.batch_scenarios == 0
            || self.root_actions < 2
            || self.root_actions > 32
            || self.hidden_size == 0
            || self.final_validation_scenarios == 0
        {
            return Err(
                "training sizes must be positive and root-actions must be 2..=32".to_string(),
            );
        }
        if self.learning_rate <= 0.0
            || self.weight_decay < 0.0
            || self.score_temperature <= 0.0
            || self.value_loss_weight <= 0.0
        {
            return Err("training rates must be positive (weight decay may be zero)".to_string());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct MenuSample {
    observation: TrainingObservation,
    measurements: RunMeasurements,
    history: Vec<u16>,
    action_scores: Vec<(usize, f32)>,
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct CollectionStats {
    pub scenarios: usize,
    pub menus: usize,
    pub candidates: usize,
    pub branches: usize,
    pub branch_steps: usize,
}

impl std::ops::AddAssign for CollectionStats {
    fn add_assign(&mut self, rhs: Self) {
        self.scenarios += rhs.scenarios;
        self.menus += rhs.menus;
        self.candidates += rhs.candidates;
        self.branches += rhs.branches;
        self.branch_steps += rhs.branch_steps;
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct ModelMetrics {
    pub loss: f32,
    pub top_accuracy: f32,
    pub mean_regret: f32,
    pub margin_mae: f32,
}

#[derive(Debug, Serialize)]
pub struct NativeTrainingSummary {
    pub format: &'static str,
    pub checkpoint: String,
    pub device: String,
    pub continuation_policy: &'static str,
    pub config: NativeCheckpointConfig,
    pub elapsed_seconds: f64,
    pub collection_seconds: f64,
    pub optimization_seconds: f64,
    pub checkpoint_seconds: f64,
    pub batches: usize,
    pub parameters: usize,
    pub totals: CollectionStats,
    pub scenarios_per_second: f64,
    pub branch_steps_per_second: f64,
    pub candidate_rows_per_second: f64,
    pub final_unseen_random: ModelMetrics,
    pub final_unseen_trained: ModelMetrics,
}

#[derive(Debug, Serialize)]
pub struct NativeCheckpointConfig {
    pub schema_version: u32,
    pub feature_buckets: u64,
    pub hidden_size: usize,
    pub numeric_measurements: usize,
    pub action_numeric_measurements: usize,
    pub ascension: i32,
    pub batch_scenarios: usize,
    pub root_actions: usize,
    pub burn_in_actions: usize,
    pub learning_rate: f64,
    pub weight_decay: f64,
    pub score_temperature: f64,
    pub value_loss_weight: f64,
    pub seed: u64,
    pub seed_source: u64,
    pub validation_seed_source: u64,
}

#[derive(Clone, Copy)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
        value ^ (value >> 31)
    }

    fn range(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next() % upper as u64) as usize
        }
    }

    fn shuffle<T>(&mut self, values: &mut [T]) {
        for index in (1..values.len()).rev() {
            values.swap(index, self.range(index + 1));
        }
    }
}

fn scenario_specs(rng: &mut SplitMix64, count: usize, offset: usize) -> Vec<ProceduralCombatSpec> {
    (0..count)
        .map(|index| {
            let cell = offset + index;
            ProceduralCombatSpec {
                seed: rng.next() as i64,
                act: Some((cell % 3 + 1) as i32),
                kind: if (cell / 3) % 2 == 0 {
                    ProceduralCombatKind::Elite
                } else {
                    ProceduralCombatKind::Boss
                },
            }
        })
        .collect()
}

fn action_index(agent: &mut HtnAgent, env: &TrainEnv) -> Option<usize> {
    if env.outcome().done() {
        return None;
    }
    let selected = agent.decide(&env.game);
    env.game
        .legal_actions()
        .iter()
        .position(|action| action == &selected)
}

fn decision_signature(observation: &TrainingObservation, action_index: usize) -> u16 {
    let mut value = 0xcbf29ce484222325u64;
    let action = observation
        .actions
        .iter()
        .find(|action| action.index == action_index)
        .expect("selected action must be in the visible legal menu");
    for feature in observation
        .state_features
        .iter()
        .take(4)
        .chain(action.features.iter())
    {
        value ^= u64::from(*feature);
        value = value.wrapping_mul(0x100000001b3);
    }
    (value % TRAINING_FEATURE_BUCKETS + 1) as u16
}

fn normalized_terminal_margin(root: &RunMeasurements, terminal_score: i32) -> f32 {
    if terminal_score >= 0 {
        (terminal_score as f32 / root.max_hp.max(1) as f32).min(1.0)
    } else {
        (terminal_score as f32 / root.enemy_max_hp.max(1) as f32).max(-1.0)
    }
}

fn collect_counterfactual_menus(
    batch: &mut BatchedTrainEnv,
    specs: Vec<ProceduralCombatSpec>,
    rng: &mut SplitMix64,
    root_actions: usize,
    burn_in_actions: usize,
) -> Result<(Vec<MenuSample>, CollectionStats), String> {
    let scenario_count = specs.len();
    let mut rows = batch.reset_combat(specs)?;
    let mut agents = vec![HtnAgent::new(); rows.len()];
    let mut histories = vec![Vec::new(); rows.len()];
    let mut burn_in = (0..rows.len())
        .map(|_| rng.range(burn_in_actions + 1))
        .collect::<Vec<_>>();

    while burn_in.iter().any(|remaining| *remaining > 0) {
        let actions = agents
            .par_iter_mut()
            .zip(batch.environments().par_iter())
            .zip(rows.par_iter())
            .zip(burn_in.par_iter())
            .map(|(((agent, env), row), remaining)| {
                (*remaining > 0 && row.outcome == RunOutcome::Running)
                    .then(|| action_index(agent, env))
                    .flatten()
            })
            .collect::<Vec<_>>();
        if !actions.iter().any(Option::is_some) {
            break;
        }
        for (index, action) in actions.iter().enumerate() {
            if let Some(action) = action {
                if let Some(observation) = rows[index].observation.as_ref() {
                    histories[index].push(decision_signature(observation, *action));
                }
                burn_in[index] = burn_in[index].saturating_sub(1);
            } else {
                burn_in[index] = 0;
            }
        }
        rows = batch.step(actions)?;
    }

    let teacher_actions = agents
        .par_iter()
        .zip(batch.environments().par_iter())
        .map(|(agent, env)| {
            let mut probe = agent.clone();
            action_index(&mut probe, env)
        })
        .collect::<Vec<_>>();

    let mut roots = Vec::new();
    let mut requests = Vec::new();
    let mut branch_agents = Vec::new();
    for (environment, row) in rows.iter().enumerate() {
        let Some(observation) = row.observation.as_ref() else {
            continue;
        };
        if row.outcome != RunOutcome::Running || observation.actions.len() < 2 {
            continue;
        }
        let mut selected = Vec::new();
        if let Some(action) = teacher_actions[environment] {
            selected.push(action);
        }
        let mut remaining = observation
            .actions
            .iter()
            .map(|action| action.index)
            .filter(|action| !selected.contains(action))
            .collect::<Vec<_>>();
        rng.shuffle(&mut remaining);
        selected.extend(remaining);
        selected.truncate(root_actions.min(observation.actions.len()));
        for action in selected {
            roots.push((environment, action));
            requests.push(BatchForkRequest {
                environment,
                action,
            });
            branch_agents.push(agents[environment].clone());
        }
    }
    if requests.is_empty() {
        return Ok((
            Vec::new(),
            CollectionStats {
                scenarios: scenario_count,
                ..CollectionStats::default()
            },
        ));
    }

    let mut branch_rows = batch.fork_compact(requests)?;
    let mut terminal_scores = branch_rows
        .iter()
        .map(|row| row.terminal_score)
        .collect::<Vec<_>>();
    let mut branch_steps = branch_rows.len();

    while branch_rows
        .iter()
        .any(|row| row.outcome == RunOutcome::Running)
    {
        let actions = branch_agents
            .par_iter_mut()
            .zip(batch.branch_environments().par_iter())
            .zip(branch_rows.par_iter())
            .map(|((agent, env), row)| {
                (row.outcome == RunOutcome::Running)
                    .then(|| action_index(agent, env))
                    .flatten()
            })
            .collect::<Vec<_>>();
        if !actions.iter().any(Option::is_some) {
            break;
        }
        branch_steps += actions.iter().filter(|action| action.is_some()).count();
        branch_rows = batch.branch_step_compact(actions)?;
        for (index, row) in branch_rows.iter().enumerate() {
            if terminal_scores[index].is_none() {
                terminal_scores[index] = row.terminal_score;
            }
        }
    }

    let mut grouped = vec![Vec::new(); rows.len()];
    for (branch, (environment, action)) in roots.iter().copied().enumerate() {
        if let Some(score) = terminal_scores[branch] {
            grouped[environment].push((
                action,
                normalized_terminal_margin(&rows[environment].measurements, score),
            ));
        }
    }
    let samples = grouped
        .into_iter()
        .enumerate()
        .filter_map(|(environment, action_scores)| {
            if action_scores.len() < 2 {
                return None;
            }
            Some(MenuSample {
                observation: rows[environment].observation.clone()?,
                measurements: rows[environment].measurements.clone(),
                history: histories[environment].clone(),
                action_scores,
            })
        })
        .collect::<Vec<_>>();
    let candidates = samples
        .iter()
        .map(|sample| sample.action_scores.len())
        .sum();
    let menus = samples.len();
    Ok((
        samples,
        CollectionStats {
            scenarios: scenario_count,
            menus,
            candidates,
            branches: roots.len(),
            branch_steps,
        },
    ))
}

fn symlog_scaled(value: f32, scale: f32) -> f32 {
    if scale == 1.0 {
        value
    } else {
        value.signum() * value.abs().ln_1p() / scale.ln_1p()
    }
}

fn measurement_vector(m: &RunMeasurements) -> [f32; NUMERIC_MEASUREMENTS] {
    let values = [
        m.act as f32,
        m.floor as f32,
        m.hp as f32,
        m.max_hp as f32,
        m.block as f32,
        m.gold as f32,
        m.energy as f32,
        m.energy_master as f32,
        m.deck_size as f32,
        m.upgraded_cards as f32,
        m.distinct_cards as f32,
        m.deck_base_damage as f32,
        m.deck_base_block as f32,
        m.deck_base_magic as f32,
        m.relics as f32,
        m.potions as f32,
        m.player_power_amount as f32,
        m.hand_size as f32,
        m.draw_size as f32,
        m.discard_size as f32,
        m.exhaust_size as f32,
        m.playable_cards as f32,
        m.zero_cost_cards as f32,
        m.orb_slots as f32,
        m.filled_orbs as f32,
        m.dark_evoke as f32,
        m.combat_turn as f32,
        m.cards_played_this_turn as f32,
        m.living_enemies as f32,
        m.enemy_hp as f32,
        m.enemy_max_hp as f32,
        m.enemy_block as f32,
        m.enemy_power_amount as f32,
        m.incoming_attack as f32,
        m.legal_actions as f32,
        m.deck_total_cost as f32,
        m.deck_attack_cards as f32,
        m.deck_skill_cards as f32,
        m.deck_power_cards as f32,
        m.deck_exhaust_cards as f32,
        m.deck_orb_cards as f32,
        m.deck_card_access as f32,
        m.deck_energy_cards as f32,
        m.deck_focus_cards as f32,
        m.ascension as f32,
    ];
    let scales = [
        3.0, 52.0, 100.0, 100.0, 100.0, 500.0, 10.0, 10.0, 50.0, 50.0, 50.0, 500.0, 500.0, 100.0,
        30.0, 5.0, 100.0, 10.0, 50.0, 50.0, 50.0, 10.0, 10.0, 10.0, 10.0, 500.0, 50.0, 20.0, 10.0,
        500.0, 500.0, 500.0, 500.0, 200.0, 100.0, 100.0, 50.0, 50.0, 20.0, 20.0, 30.0, 20.0, 20.0,
        10.0, 20.0,
    ];
    std::array::from_fn(|index| symlog_scaled(values[index], scales[index]))
}

fn action_parameter_vector(
    p: &ActionParameters,
    m: &RunMeasurements,
) -> [f32; ACTION_NUMERIC_MEASUREMENTS] {
    let known = f32::from(p.known);
    let raw = [
        known,
        p.hp_delta as f32,
        p.max_hp_delta as f32,
        p.enemy_hp_delta as f32,
        p.block_delta as f32,
        p.enemy_block_delta as f32,
        p.energy_delta as f32,
        p.gold_delta as f32,
        p.hand_delta as f32,
        p.draw_delta as f32,
        p.discard_delta as f32,
        p.exhaust_delta as f32,
        p.deck_size_delta as f32,
        p.upgraded_cards_delta as f32,
        p.relic_delta as f32,
        p.potion_delta as f32,
        p.orb_slots_delta as f32,
        p.filled_orbs_delta as f32,
        p.orb_evoke_delta as f32,
        p.incoming_attack_delta as f32,
        p.living_enemies_delta as f32,
        p.turn_delta as f32,
        p.cards_played_delta as f32,
        p.player_power_delta as f32,
        p.enemy_power_delta as f32,
    ];
    let raw_scales = [
        1.0, 100.0, 100.0, 500.0, 100.0, 500.0, 10.0, 500.0, 10.0, 10.0, 10.0, 10.0, 10.0, 10.0,
        5.0, 5.0, 10.0, 10.0, 500.0, 500.0, 5.0, 10.0, 20.0, 100.0, 200.0,
    ];
    let mut output = [0.0; ACTION_NUMERIC_MEASUREMENTS];
    if p.known {
        for index in 0..raw.len() {
            output[index] = symlog_scaled(raw[index], raw_scales[index]);
        }
        output[25] = (p.hp_delta as f32 / m.hp.max(1) as f32).clamp(-2.0, 2.0);
        output[26] = (p.max_hp_delta as f32 / m.max_hp.max(1) as f32).clamp(-2.0, 2.0);
        output[27] = (p.enemy_hp_delta as f32 / m.enemy_hp.max(1) as f32).clamp(-2.0, 2.0);
        output[28] = (p.gold_delta as f32 / m.gold.max(1) as f32).clamp(-2.0, 2.0);
        output[29] = f32::from(p.hp_delta < 0 && m.hp + p.hp_delta <= 0);
        output[30] = f32::from(p.enemy_hp_delta < 0 && m.enemy_hp + p.enemy_hp_delta <= 0);
    }
    output
}

fn pad_ids(source: &[u16], width: usize, ids: &mut Vec<u32>, weights: &mut Vec<f32>) {
    let visible = source.len().min(width);
    let scale = 1.0 / visible.max(1) as f32;
    for index in 0..width {
        if index < visible {
            ids.push(u32::from(source[index]));
            weights.push(scale);
        } else {
            ids.push(0);
            weights.push(0.0);
        }
    }
}

struct EncodedBatch {
    state_ids: Tensor,
    state_weights: Tensor,
    action_ids: Tensor,
    action_weights: Tensor,
    inventory_ids: Tensor,
    inventory_weights: Tensor,
    candidate_ids: Tensor,
    candidate_weights: Tensor,
    history_ids: Tensor,
    history_weights: Tensor,
    numeric: Tensor,
    action_numeric: Tensor,
    scores: Tensor,
    score_values: Vec<f32>,
    menu_ranges: Vec<(usize, usize)>,
}

impl EncodedBatch {
    fn new(samples: &[MenuSample], device: &Device) -> TrainingResult<Self> {
        let rows = samples
            .iter()
            .map(|sample| sample.action_scores.len())
            .sum::<usize>();
        let mut state_ids = Vec::with_capacity(rows * MAX_STATE_FEATURES);
        let mut state_weights = Vec::with_capacity(rows * MAX_STATE_FEATURES);
        let mut action_ids = Vec::with_capacity(rows * MAX_ACTION_FEATURES);
        let mut action_weights = Vec::with_capacity(rows * MAX_ACTION_FEATURES);
        let mut inventory_ids = Vec::with_capacity(rows * MAX_INVENTORY_IDENTITIES);
        let mut inventory_weights = Vec::with_capacity(rows * MAX_INVENTORY_IDENTITIES);
        let mut candidate_ids = Vec::with_capacity(rows * MAX_CANDIDATE_IDENTITIES);
        let mut candidate_weights = Vec::with_capacity(rows * MAX_CANDIDATE_IDENTITIES);
        let mut history_ids = Vec::with_capacity(rows * MAX_HISTORY_STEPS);
        let mut history_weights = Vec::with_capacity(rows * MAX_HISTORY_STEPS);
        let mut numeric = Vec::with_capacity(rows * NUMERIC_MEASUREMENTS);
        let mut action_numeric = Vec::with_capacity(rows * ACTION_NUMERIC_MEASUREMENTS);
        let mut scores = Vec::with_capacity(rows);
        let mut menu_ranges = Vec::with_capacity(samples.len());

        for sample in samples {
            let start = scores.len();
            let measurements = measurement_vector(&sample.measurements);
            for (action_index, score) in &sample.action_scores {
                let action = sample
                    .observation
                    .actions
                    .iter()
                    .find(|action| action.index == *action_index)
                    .ok_or_else(|| format!("action {action_index} vanished from encoded menu"))?;
                pad_ids(
                    &sample.observation.state_features,
                    MAX_STATE_FEATURES,
                    &mut state_ids,
                    &mut state_weights,
                );
                pad_ids(
                    &action.features,
                    MAX_ACTION_FEATURES,
                    &mut action_ids,
                    &mut action_weights,
                );
                pad_ids(
                    &sample.observation.inventory_identities,
                    MAX_INVENTORY_IDENTITIES,
                    &mut inventory_ids,
                    &mut inventory_weights,
                );
                pad_ids(
                    &action.candidate_identities,
                    MAX_CANDIDATE_IDENTITIES,
                    &mut candidate_ids,
                    &mut candidate_weights,
                );
                pad_ids(
                    &sample.history,
                    MAX_HISTORY_STEPS,
                    &mut history_ids,
                    &mut history_weights,
                );
                numeric.extend_from_slice(&measurements);
                action_numeric.extend_from_slice(&action_parameter_vector(
                    &action.parameters,
                    &sample.measurements,
                ));
                scores.push(*score);
            }
            menu_ranges.push((start, scores.len()));
        }

        let ids = |values, width| Tensor::from_vec(values, (rows, width), device);
        let floats = |values, width| Tensor::from_vec(values, (rows, width), device);
        Ok(Self {
            state_ids: ids(state_ids, MAX_STATE_FEATURES)?,
            state_weights: floats(state_weights, MAX_STATE_FEATURES)?,
            action_ids: ids(action_ids, MAX_ACTION_FEATURES)?,
            action_weights: floats(action_weights, MAX_ACTION_FEATURES)?,
            inventory_ids: ids(inventory_ids, MAX_INVENTORY_IDENTITIES)?,
            inventory_weights: floats(inventory_weights, MAX_INVENTORY_IDENTITIES)?,
            candidate_ids: ids(candidate_ids, MAX_CANDIDATE_IDENTITIES)?,
            candidate_weights: floats(candidate_weights, MAX_CANDIDATE_IDENTITIES)?,
            history_ids: ids(history_ids, MAX_HISTORY_STEPS)?,
            history_weights: floats(history_weights, MAX_HISTORY_STEPS)?,
            numeric: floats(numeric, NUMERIC_MEASUREMENTS)?,
            action_numeric: floats(action_numeric, ACTION_NUMERIC_MEASUREMENTS)?,
            scores: Tensor::from_vec(scores.clone(), rows, device)?,
            score_values: scores,
            menu_ranges,
        })
    }
}

struct NumericProjection {
    first: Linear,
    second: Linear,
}

impl NumericProjection {
    fn new(input: usize, hidden: usize, vb: VarBuilder<'_>) -> candle_core::Result<Self> {
        Ok(Self {
            first: linear(input, hidden, vb.pp("first"))?,
            second: linear_no_bias(hidden, hidden, vb.pp("second"))?,
        })
    }

    fn forward(&self, input: &Tensor) -> candle_core::Result<Tensor> {
        self.second
            .forward(&ops::silu(&self.first.forward(&rms_normalize(input)?)?)?)
    }
}

fn rms_normalize(input: &Tensor) -> candle_core::Result<Tensor> {
    let width = input.dim(D::Minus1)? as f64;
    let scale = ((input.sqr()?.sum_keepdim(D::Minus1)? / width)? + 1e-5)?.sqrt()?;
    input.broadcast_div(&scale)
}

struct NativeCombatModel {
    embedding: Embedding,
    state_projection: Linear,
    action_projection: Linear,
    inventory_projection: Linear,
    candidate_projection: Linear,
    history_projection: Linear,
    numeric_projection: NumericProjection,
    action_numeric_projection: NumericProjection,
    hidden: Linear,
    output: Linear,
}

impl NativeCombatModel {
    fn new(hidden: usize, vb: VarBuilder<'_>) -> candle_core::Result<Self> {
        let context = hidden * CONTEXT_PARTS;
        Ok(Self {
            embedding: embedding(
                TRAINING_FEATURE_BUCKETS as usize + 1,
                hidden,
                vb.pp("embedding"),
            )?,
            state_projection: linear_no_bias(hidden, hidden, vb.pp("state_projection"))?,
            action_projection: linear_no_bias(hidden, hidden, vb.pp("action_projection"))?,
            inventory_projection: linear_no_bias(hidden, hidden, vb.pp("inventory_projection"))?,
            candidate_projection: linear_no_bias(hidden, hidden, vb.pp("candidate_projection"))?,
            history_projection: linear_no_bias(hidden, hidden, vb.pp("history_projection"))?,
            numeric_projection: NumericProjection::new(
                NUMERIC_MEASUREMENTS,
                hidden,
                vb.pp("numeric_projection"),
            )?,
            action_numeric_projection: NumericProjection::new(
                ACTION_NUMERIC_MEASUREMENTS,
                hidden,
                vb.pp("action_numeric_projection"),
            )?,
            hidden: linear(context, hidden * 3, vb.pp("hidden"))?,
            output: linear(hidden * 3, 2, vb.pp("output"))?,
        })
    }

    fn pool(&self, ids: &Tensor, weights: &Tensor) -> candle_core::Result<Tensor> {
        self.embedding
            .forward(ids)?
            .broadcast_mul(&weights.unsqueeze(2)?)?
            .sum(1)
    }

    fn forward(&self, batch: &EncodedBatch) -> candle_core::Result<Tensor> {
        let state = self
            .state_projection
            .forward(&self.pool(&batch.state_ids, &batch.state_weights)?)?;
        let action = self
            .action_projection
            .forward(&self.pool(&batch.action_ids, &batch.action_weights)?)?;
        let inventory = self
            .inventory_projection
            .forward(&self.pool(&batch.inventory_ids, &batch.inventory_weights)?)?;
        let candidate = self
            .candidate_projection
            .forward(&self.pool(&batch.candidate_ids, &batch.candidate_weights)?)?;
        let history = self
            .history_projection
            .forward(&self.pool(&batch.history_ids, &batch.history_weights)?)?;
        let state_action = (&state * &action)?;
        let inventory_candidate = (&inventory * &candidate)?;
        let numeric = self.numeric_projection.forward(&batch.numeric)?;
        let action_numeric = self
            .action_numeric_projection
            .forward(&batch.action_numeric)?;
        let context = Tensor::cat(
            &[
                &state,
                &action,
                &state_action,
                &history,
                &inventory,
                &candidate,
                &inventory_candidate,
                &numeric,
                &action_numeric,
            ],
            1,
        )?;
        self.output.forward(&ops::silu(
            &self.hidden.forward(&rms_normalize(&context)?)?,
        )?)
    }
}

fn model_loss(
    prediction: &Tensor,
    batch: &EncodedBatch,
    score_temperature: f64,
    value_loss_weight: f64,
) -> candle_core::Result<Tensor> {
    let logits = prediction.i((.., 0))?;
    let mut policy_losses = Vec::with_capacity(batch.menu_ranges.len());
    for (start, end) in &batch.menu_ranges {
        let count = end - start;
        let menu_logits = logits.narrow(0, *start, count)?;
        let targets = batch.scores.narrow(0, *start, count)?;
        let target_distribution = ops::softmax(&(targets / score_temperature)?, 0)?;
        let log_probabilities = ops::log_softmax(&menu_logits, 0)?;
        policy_losses.push(
            (&target_distribution * &log_probabilities)?
                .sum_all()?
                .neg()?,
        );
    }
    let policy_loss = Tensor::stack(&policy_losses, 0)?.mean_all()?;
    let margin_loss = loss::huber(&prediction.i((.., 1))?, &batch.scores, 1.0)?;
    policy_loss + margin_loss * value_loss_weight
}

fn model_metrics(
    prediction: &Tensor,
    batch: &EncodedBatch,
    loss: f32,
) -> candle_core::Result<ModelMetrics> {
    let values = prediction.to_device(&Device::Cpu)?.to_vec2::<f32>()?;
    let mut correct = 0usize;
    let mut regret = 0.0f32;
    for (start, end) in &batch.menu_ranges {
        let chosen = (*start..*end)
            .max_by(|left, right| values[*left][0].total_cmp(&values[*right][0]))
            .expect("menus are nonempty");
        let best = batch.score_values[*start..*end]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        let selected = batch.score_values[chosen];
        correct += usize::from(selected >= best - 1e-7);
        regret += best - selected;
    }
    let menus = batch.menu_ranges.len().max(1) as f32;
    let margin_mae = values
        .iter()
        .zip(&batch.score_values)
        .map(|(prediction, target)| (prediction[1] - target).abs())
        .sum::<f32>()
        / batch.score_values.len().max(1) as f32;
    Ok(ModelMetrics {
        loss,
        top_accuracy: correct as f32 / menus,
        mean_regret: regret / menus,
        margin_mae,
    })
}

fn parameter_count(varmap: &VarMap) -> usize {
    varmap
        .all_vars()
        .iter()
        .map(|variable| variable.elem_count())
        .sum()
}

fn select_device(requested: TrainingDevice) -> TrainingResult<(Device, String)> {
    match requested {
        TrainingDevice::Cpu => Ok((Device::Cpu, "cpu".to_string())),
        TrainingDevice::Cuda => {
            #[cfg(feature = "native-training-cuda")]
            {
                Ok((Device::new_cuda(0)?, "cuda".to_string()))
            }
            #[cfg(not(feature = "native-training-cuda"))]
            {
                Err("CUDA requested, but the binary lacks --features native-training-cuda".into())
            }
        }
        TrainingDevice::Auto => {
            #[cfg(feature = "native-training-cuda")]
            {
                if let Ok(device) = Device::new_cuda(0) {
                    return Ok((device, "cuda".to_string()));
                }
            }
            Ok((Device::Cpu, "cpu".to_string()))
        }
    }
}

fn save_checkpoint(varmap: &VarMap, output: &Path) -> TrainingResult<()> {
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    let pending = output.with_extension(format!("safetensors.pending-{}", std::process::id()));
    varmap.save(&pending)?;
    fs::rename(pending, output)?;
    Ok(())
}

fn duration_seconds(duration: Duration) -> f64 {
    duration.as_secs_f64()
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn train_native(config: &NativeTrainingConfig) -> TrainingResult<NativeTrainingSummary> {
    config.validate()?;
    let (device, device_name) = select_device(config.device)?;
    if device_name == "cuda" {
        device.set_seed(config.seed)?;
    }
    let varmap = VarMap::new();
    let model = NativeCombatModel::new(
        config.hidden_size,
        VarBuilder::from_varmap(&varmap, DType::F32, &device),
    )?;
    let parameters = parameter_count(&varmap);
    let initial_path = config
        .output
        .with_extension(format!("initial-{}.safetensors", std::process::id()));
    if let Some(parent) = initial_path.parent() {
        fs::create_dir_all(parent)?;
    }
    varmap.save(&initial_path)?;
    let initial_checkpoint = RemoveOnDrop(initial_path);
    let mut optimizer = AdamW::new(
        varmap.all_vars(),
        ParamsAdamW {
            lr: config.learning_rate,
            weight_decay: config.weight_decay,
            ..ParamsAdamW::default()
        },
    )?;
    let mut engine = BatchedTrainEnv::new(config.ascension, config.max_combat_actions);
    let mut seed_rng = SplitMix64::new(config.seed_source);
    let mut selection_rng = SplitMix64::new(config.seed ^ 0x5ce0_a710);
    let started = Instant::now();
    let budget = Duration::from_secs_f64(config.seconds);
    let mut collection_time = Duration::ZERO;
    let mut optimization_time = Duration::ZERO;
    let mut totals = CollectionStats::default();
    let mut batches = 0usize;

    while started.elapsed() < budget {
        let collection_started = Instant::now();
        let specs = scenario_specs(&mut seed_rng, config.batch_scenarios, totals.scenarios);
        let (samples, stats) = collect_counterfactual_menus(
            &mut engine,
            specs,
            &mut selection_rng,
            config.root_actions,
            config.burn_in_actions,
        )?;
        collection_time += collection_started.elapsed();
        totals += stats;
        if samples.is_empty() {
            continue;
        }

        let optimization_started = Instant::now();
        let encoded = EncodedBatch::new(&samples, &device)?;
        let prediction = model.forward(&encoded)?;
        let loss = model_loss(
            &prediction,
            &encoded,
            config.score_temperature,
            config.value_loss_weight,
        )?;
        let loss_value = loss.to_device(&Device::Cpu)?.to_vec0::<f32>()?;
        optimizer.backward_step(&loss)?;
        device.synchronize()?;
        optimization_time += optimization_started.elapsed();
        batches += 1;
        let metrics = model_metrics(&prediction, &encoded, loss_value)?;
        println!(
            "batch={batches} scenarios={} menus={} branches={} steps={} loss={:.4} top={:.3} regret={:.4} margin_mae={:.4}",
            totals.scenarios,
            totals.menus,
            totals.branches,
            totals.branch_steps,
            metrics.loss,
            metrics.top_accuracy,
            metrics.mean_regret,
            metrics.margin_mae,
        );
    }

    let mut validation_rng = SplitMix64::new(config.validation_seed_source);
    let validation_specs = scenario_specs(
        &mut validation_rng,
        config.final_validation_scenarios,
        totals.scenarios,
    );
    let (validation_samples, _) = collect_counterfactual_menus(
        &mut engine,
        validation_specs,
        &mut selection_rng,
        config.root_actions,
        config.burn_in_actions,
    )?;
    if validation_samples.is_empty() {
        return Err("final validation produced no usable legal-action menus".into());
    }
    let validation = EncodedBatch::new(&validation_samples, &device)?;
    let trained_prediction = model.forward(&validation)?;
    let trained_loss = model_loss(
        &trained_prediction,
        &validation,
        config.score_temperature,
        config.value_loss_weight,
    )?
    .to_device(&Device::Cpu)?
    .to_vec0::<f32>()?;
    let trained_metrics = model_metrics(&trained_prediction, &validation, trained_loss)?;

    let mut initial_varmap = VarMap::new();
    let initial_model = NativeCombatModel::new(
        config.hidden_size,
        VarBuilder::from_varmap(&initial_varmap, DType::F32, &device),
    )?;
    initial_varmap.load(&initial_checkpoint.0)?;
    let initial_prediction = initial_model.forward(&validation)?;
    let initial_loss = model_loss(
        &initial_prediction,
        &validation,
        config.score_temperature,
        config.value_loss_weight,
    )?
    .to_device(&Device::Cpu)?
    .to_vec0::<f32>()?;
    let initial_metrics = model_metrics(&initial_prediction, &validation, initial_loss)?;

    let checkpoint_started = Instant::now();
    save_checkpoint(&varmap, &config.output)?;
    let checkpoint_time = checkpoint_started.elapsed();
    let elapsed = started.elapsed();
    let active_seconds = duration_seconds(collection_time + optimization_time).max(f64::EPSILON);
    let summary = NativeTrainingSummary {
        format: "sts-native-procedural-combat-v2",
        checkpoint: config.output.display().to_string(),
        device: device_name,
        continuation_policy: "rust_htn",
        config: NativeCheckpointConfig {
            schema_version: 1,
            feature_buckets: TRAINING_FEATURE_BUCKETS,
            hidden_size: config.hidden_size,
            numeric_measurements: NUMERIC_MEASUREMENTS,
            action_numeric_measurements: ACTION_NUMERIC_MEASUREMENTS,
            ascension: config.ascension,
            batch_scenarios: config.batch_scenarios,
            root_actions: config.root_actions,
            burn_in_actions: config.burn_in_actions,
            learning_rate: config.learning_rate,
            weight_decay: config.weight_decay,
            score_temperature: config.score_temperature,
            value_loss_weight: config.value_loss_weight,
            seed: config.seed,
            seed_source: config.seed_source,
            validation_seed_source: config.validation_seed_source,
        },
        elapsed_seconds: duration_seconds(elapsed),
        collection_seconds: duration_seconds(collection_time),
        optimization_seconds: duration_seconds(optimization_time),
        checkpoint_seconds: duration_seconds(checkpoint_time),
        batches,
        parameters,
        totals,
        scenarios_per_second: totals.scenarios as f64 / active_seconds,
        branch_steps_per_second: totals.branch_steps as f64 / active_seconds,
        candidate_rows_per_second: totals.candidates as f64 / active_seconds,
        final_unseen_random: initial_metrics,
        final_unseen_trained: trained_metrics,
    };
    let metadata = config.output.with_extension("metrics.json");
    fs::write(&metadata, serde_json::to_vec_pretty(&summary)?)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_vector_widths_are_stable() {
        let measurements = RunMeasurements::default();
        assert_eq!(measurement_vector(&measurements).len(), 45);
        assert_eq!(
            action_parameter_vector(&ActionParameters::default(), &measurements).len(),
            31
        );
    }

    #[test]
    fn native_model_backpropagates_and_round_trips() -> TrainingResult<()> {
        let device = Device::Cpu;
        let varmap = VarMap::new();
        let model =
            NativeCombatModel::new(8, VarBuilder::from_varmap(&varmap, DType::F32, &device))?;
        let observation = TrainingObservation {
            state_features: vec![1, 2],
            inventory_identities: vec![3, 4],
            actions: vec![
                crate::TrainingAction {
                    index: 0,
                    action: crate::Action::EndTurn,
                    features: vec![5],
                    candidate_identities: vec![3],
                    parameters: ActionParameters::default(),
                },
                crate::TrainingAction {
                    index: 1,
                    action: crate::Action::Quit,
                    features: vec![6],
                    candidate_identities: vec![4],
                    parameters: ActionParameters::default(),
                },
            ],
        };
        let samples = vec![MenuSample {
            observation,
            measurements: RunMeasurements::default(),
            history: vec![7],
            action_scores: vec![(0, 0.5), (1, -0.5)],
        }];
        let encoded = EncodedBatch::new(&samples, &device)?;
        let before = model.forward(&encoded)?;
        let loss = model_loss(&before, &encoded, 0.15, 1.0)?;
        let mut optimizer = AdamW::new_lr(varmap.all_vars(), 1e-3)?;
        optimizer.backward_step(&loss)?;
        let after = model.forward(&encoded)?;
        assert_ne!(before.to_vec2::<f32>()?, after.to_vec2::<f32>()?);

        let path = std::env::temp_dir().join(format!(
            "sts-native-training-test-{}.safetensors",
            std::process::id()
        ));
        varmap.save(&path)?;
        let mut loaded_map = VarMap::new();
        let loaded =
            NativeCombatModel::new(8, VarBuilder::from_varmap(&loaded_map, DType::F32, &device))?;
        loaded_map.load(&path)?;
        fs::remove_file(path)?;
        assert_eq!(
            after.to_vec2::<f32>()?,
            loaded.forward(&encoded)?.to_vec2::<f32>()?
        );
        Ok(())
    }
}
