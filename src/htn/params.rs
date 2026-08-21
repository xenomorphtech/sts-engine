//! Tunable HTN policy parameters.
//!
//! Every value defaults to the hand-tuned constant it replaced, so runs
//! without `STS_HTN_PARAMS` behave exactly like the fixed-constant build.
//! Setting `STS_HTN_PARAMS=/path/to/params.json` overrides any subset, which
//! is the interface the black-box optimizer uses.

use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct Params {
    // Combat state evaluation (turnplan).
    pub dmg_base: f32,
    pub dmg_per_turn: f32,
    pub danger_base: f32,
    pub danger_scale: f32,
    pub kill_bonus: f32,
    pub strip_block_mult: f32,
    pub lethal_discount: f32,
    pub overblock_penalty: f32,
    pub energy_value: f32,
    pub strength_weight: f32,
    pub dexterity_weight: f32,
    pub focus_weight: f32,
    pub enemy_strength_penalty: f32,
    pub bias_decay_weight: f32,
    // Orb heuristics.
    pub orb_horizon: f32,
    pub orb_lightning_mult: f32,
    pub orb_frost_mult: f32,
    pub orb_dark_stored: f32,
    pub orb_dark_growth: f32,
    pub orb_plasma: f32,
    // Map policy (strategy).
    pub elite_afford_hp: f32,
    pub elite_strength_base: f32,
    pub elite_strength_slope: f32,
    pub elite_value: f32,
    pub elite_penalty: f32,
    pub elite_hp_floor: f32,
    pub rest_low_value: f32,
    pub rest_high_value: f32,
    pub rest_preboss_value: f32,
    pub shop_gold_div: f32,
    pub treasure_value: f32,
    pub event_value: f32,
    pub monster_ok_value: f32,
    pub monster_low_value: f32,
    // Rest-site policy.
    pub rest_hp_act1: f32,
    pub rest_hp_later: f32,
    pub rest_hp_preboss: f32,
    // Drafting.
    pub pick_threshold: f32,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            dmg_base: 1.8947,
            dmg_per_turn: 0.6310,
            danger_base: 16.6813,
            danger_scale: 101.3391,
            kill_bonus: 680.6771,
            strip_block_mult: 0.9838,
            lethal_discount: 0.8422,
            overblock_penalty: 0.9977,
            energy_value: 0.5381,
            strength_weight: 4.2472,
            dexterity_weight: 5.6866,
            focus_weight: 4.1195,
            enemy_strength_penalty: 19.1909,
            bias_decay_weight: 4.2088,
            orb_horizon: 4.7244,
            orb_lightning_mult: 0.9009,
            orb_frost_mult: 0.4423,
            orb_dark_stored: 0.5771,
            orb_dark_growth: 0.0938,
            orb_plasma: 15.4869,
            elite_afford_hp: 0.7185,
            elite_strength_base: 4.0779,
            elite_strength_slope: 2.7412,
            elite_value: 34.2655,
            elite_penalty: -179.0511,
            elite_hp_floor: 0.4217,
            rest_low_value: 31.5554,
            rest_high_value: 27.8690,
            rest_preboss_value: 40.8818,
            shop_gold_div: 6.6098,
            treasure_value: 28.6692,
            event_value: 13.7921,
            monster_ok_value: 6.3865,
            monster_low_value: -24.5602,
            rest_hp_act1: 0.5618,
            rest_hp_later: 0.6959,
            rest_hp_preboss: 0.7681,
            pick_threshold: 65.0226,
        }
    }
}

static PARAMS: LazyLock<Params> = LazyLock::new(|| {
    let Some(path) = std::env::var_os("STS_HTN_PARAMS") else {
        return Params::default();
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("STS_HTN_PARAMS {}: {e}", path.to_string_lossy()));
    serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("STS_HTN_PARAMS {}: {e}", path.to_string_lossy()))
}

);

pub fn params() -> &'static Params {
    &PARAMS
}
