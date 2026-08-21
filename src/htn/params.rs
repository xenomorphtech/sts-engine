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
    pub upgraded_pick_bonus: f32,
    pub copies_full_penalty: f32,
    pub copies_near_penalty: f32,
    pub aoe_bonus: f32,
    pub block_bonus: f32,
    pub channel_bonus: f32,
    pub scaling_bonus: f32,
    pub focus_bonus: f32,
    pub act1_attack_bonus: f32,
    pub act1_big_damage_bonus: f32,
    pub act2_damage_bonus: f32,
    pub act2_finisher_bonus: f32,
    pub act1_late_block_bonus: f32,
    pub size_full_penalty: f32,
    pub size_near_penalty: f32,
    pub target_size_act1: f32,
    pub target_size_act2: f32,
    pub target_size_act3: f32,
    // Expected remaining fight turns by act and fight kind.
    pub fl_a1_normal: f32,
    pub fl_a1_elite: f32,
    pub fl_a1_boss: f32,
    pub fl_a2_normal: f32,
    pub fl_a2_elite: f32,
    pub fl_a2_boss: f32,
    pub fl_a3_normal: f32,
    pub fl_a3_elite: f32,
    pub fl_a3_boss: f32,
    pub fl_a4_normal: f32,
    pub fl_a4_elite: f32,
    pub fl_a4_boss: f32,
    /// Absolute per-card pick-score overrides keyed by sts id.
    pub pick: std::collections::HashMap<String, f32>,
    /// Absolute per-card upgrade-score overrides keyed by sts id.
    pub upgrade: std::collections::HashMap<String, f32>,
    /// Absolute boss-relic rank overrides keyed by sts id.
    pub boss_relic: std::collections::HashMap<String, f32>,
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
            upgraded_pick_bonus: 25.0,
            copies_full_penalty: 250.0,
            copies_near_penalty: 40.0,
            aoe_bonus: 45.0,
            block_bonus: 40.0,
            channel_bonus: 35.0,
            scaling_bonus: 40.0,
            focus_bonus: 45.0,
            act1_attack_bonus: 55.0,
            act1_big_damage_bonus: 25.0,
            act2_damage_bonus: 45.0,
            act2_finisher_bonus: 30.0,
            act1_late_block_bonus: 35.0,
            size_full_penalty: 160.0,
            size_near_penalty: 60.0,
            target_size_act1: 22.0,
            target_size_act2: 26.0,
            target_size_act3: 28.0,
            fl_a1_normal: 3.3,
            fl_a1_elite: 5.3,
            fl_a1_boss: 9.5,
            fl_a2_normal: 5.0,
            fl_a2_elite: 4.5,
            fl_a2_boss: 8.0,
            fl_a3_normal: 5.5,
            fl_a3_elite: 6.0,
            fl_a3_boss: 10.0,
            fl_a4_normal: 5.0,
            fl_a4_elite: 6.0,
            fl_a4_boss: 12.0,
            pick: std::collections::HashMap::new(),
            upgrade: std::collections::HashMap::new(),
            boss_relic: std::collections::HashMap::new(),
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
