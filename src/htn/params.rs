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
            dmg_base: 6.0,
            dmg_per_turn: 4.5,
            danger_base: 20.0,
            danger_scale: 90.0,
            kill_bonus: 900.0,
            strip_block_mult: 0.55,
            lethal_discount: 0.55,
            overblock_penalty: 0.8,
            energy_value: 1.5,
            strength_weight: 4.0,
            dexterity_weight: 3.0,
            focus_weight: 4.0,
            enemy_strength_penalty: 20.0,
            bias_decay_weight: 4.0,
            orb_horizon: 4.0,
            orb_lightning_mult: 0.8,
            orb_frost_mult: 1.0,
            orb_dark_stored: 0.45,
            orb_dark_growth: 0.0,
            orb_plasma: 12.0,
            elite_afford_hp: 0.65,
            elite_strength_base: 3.0,
            elite_strength_slope: 2.0,
            elite_value: 40.0,
            elite_penalty: -150.0,
            elite_hp_floor: 0.4,
            rest_low_value: 35.0,
            rest_high_value: 18.0,
            rest_preboss_value: 50.0,
            shop_gold_div: 5.0,
            treasure_value: 25.0,
            event_value: 14.0,
            monster_ok_value: 15.0,
            monster_low_value: -15.0,
            rest_hp_act1: 0.7,
            rest_hp_later: 0.78,
            rest_hp_preboss: 0.85,
            pick_threshold: 85.0,
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
