//! Tunable HTN policy parameters.
//!
//! The promoted A20 bake is the default at every ascension. Setting
//! `STS_HTN_PARAMS=/path/to/params.json` overrides any subset, which is the
//! interface the black-box optimizer uses.

use crate::ids::{CardId, RelicId};
use serde::de::Error as _;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
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
    pub next_exposure_weight: f32,
    pub next_block_tax: f32,
    pub next_hand_damage_mult: f32,
    pub energized_weight: f32,
    pub artifact_weight: f32,
    pub buffer_weight: f32,
    pub static_discharge_weight: f32,
    pub genetic_growth_weight: f32,
    pub spike_danger: f32,
    pub spike_horizon: f32,
    pub status_gain_penalty: f32,
    pub laga_wake_penalty: f32,
    pub laga_wake_kill_ratio: f32,
    // Orb heuristics.
    pub orb_horizon: f32,
    pub orb_lightning_mult: f32,
    pub orb_frost_mult: f32,
    pub orb_dark_stored: f32,
    pub orb_dark_growth: f32,
    /// Weight of the best targetable future Dark evoke in combat-score units.
    pub orb_dark_future_mult: f32,
    /// Reserve one eventual kill reward while low-HP chaff blocks a Dark bank.
    pub orb_dark_chaff_reserve: f32,
    /// Premium for channel capacity before the first Dark would be evicted.
    pub orb_dark_queue_flex: f32,
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
    pub hex_rest_effective_gain_min: f32,
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
    // Turn search shape.
    pub search_width: f32,
    pub search_depth: f32,
    // Potion policy thresholds.
    pub potion_desperate_hp_div: f32,
    pub potion_defense_hp_div: f32,
    pub potion_heal_hp_frac: f32,
    pub potion_block_min: f32,
    pub potion_block_hp_div: f32,
    pub potion_swap_margin: f32,
    pub potion_boss_dump_turn: f32,
    pub potion_boss_dump_hp: f32,
    pub entropic_min_empty: f32,
    /// Absolute per-card pick-score overrides keyed by sts id.
    #[serde(deserialize_with = "deserialize_card_scores")]
    pub pick: HashMap<CardId, f32>,
    /// Absolute per-card upgrade-score overrides keyed by sts id.
    #[serde(deserialize_with = "deserialize_card_scores")]
    pub upgrade: HashMap<CardId, f32>,
    /// Absolute boss-relic rank overrides keyed by sts id.
    #[serde(deserialize_with = "deserialize_relic_scores")]
    pub boss_relic: HashMap<RelicId, f32>,
}

fn deserialize_card_scores<'de, D>(deserializer: D) -> Result<HashMap<CardId, f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let input = HashMap::<String, f32>::deserialize(deserializer)?;
    input
        .into_iter()
        .map(|(name, score)| {
            CardId::from_sts_id(&name)
                .map(|id| (id, score))
                .ok_or_else(|| D::Error::custom(format!("unknown card id {name:?}")))
        })
        .collect()
}

fn deserialize_relic_scores<'de, D>(deserializer: D) -> Result<HashMap<RelicId, f32>, D::Error>
where
    D: Deserializer<'de>,
{
    let input = HashMap::<String, f32>::deserialize(deserializer)?;
    input
        .into_iter()
        .map(|(name, score)| {
            RelicId::from_sts_id(&name)
                .map(|id| (id, score))
                .ok_or_else(|| D::Error::custom(format!("unknown relic id {name:?}")))
        })
        .collect()
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
            next_exposure_weight: 0.0,
            next_block_tax: 0.0,
            next_hand_damage_mult: 0.35,
            energized_weight: 5.0,
            artifact_weight: 4.0,
            buffer_weight: 1.0,
            static_discharge_weight: 3.0,
            genetic_growth_weight: 12.0,
            spike_danger: 0.0,
            spike_horizon: 2.0,
            status_gain_penalty: 0.0,
            laga_wake_penalty: 0.0,
            laga_wake_kill_ratio: 3.0,
            orb_horizon: 4.7244,
            orb_lightning_mult: 0.9009,
            orb_frost_mult: 0.4423,
            orb_dark_stored: 0.5771,
            orb_dark_growth: 0.0938,
            orb_dark_future_mult: 1.0,
            orb_dark_chaff_reserve: 1.0,
            orb_dark_queue_flex: 0.2,
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
            hex_rest_effective_gain_min: 12.0,
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
            search_width: 8.0,
            search_depth: 6.0,
            potion_desperate_hp_div: 8.0,
            potion_defense_hp_div: 3.0,
            potion_heal_hp_frac: 0.5,
            potion_block_min: 12.0,
            potion_block_hp_div: 4.0,
            potion_swap_margin: 30.0,
            potion_boss_dump_turn: 3.0,
            potion_boss_dump_hp: 120.0,
            entropic_min_empty: 2.0,
            pick: HashMap::new(),
            upgrade: HashMap::new(),
            boss_relic: HashMap::new(),
        }
    }
}

/// Baked policy values, generated by the parameter optimizer. Parsed over
/// `Params::default()` via serde defaults, so absent fields keep code
/// defaults and the file is the single source of truth for tuned values.
#[cfg(test)]
const LEGACY_A0_JSON: &str = include_str!("../../tools/params_default.json");
const BAKED_A20_JSON: &str = include_str!("../../tools/params_a20.json");

fn selected_baked_json() -> (&'static str, &'static str) {
    (BAKED_A20_JSON, "tools/params_a20.json")
}

fn merge_json(base: &mut serde_json::Value, overrides: serde_json::Value) {
    match (base, overrides) {
        (serde_json::Value::Object(base), serde_json::Value::Object(overrides)) => {
            for (key, value) in overrides {
                match base.get_mut(&key) {
                    Some(base_value) => merge_json(base_value, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, value) => *base = value,
    }
}

static PARAMS: LazyLock<Params> = LazyLock::new(|| {
    let (baked_json, baked_name) = selected_baked_json();
    let Some(path) = std::env::var_os("STS_HTN_PARAMS") else {
        return serde_json::from_str(baked_json).unwrap_or_else(|e| panic!("{baked_name}: {e}"));
    };
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("STS_HTN_PARAMS {}: {e}", path.to_string_lossy()));
    let mut baked: serde_json::Value =
        serde_json::from_str(baked_json).unwrap_or_else(|e| panic!("{baked_name}: {e}"));
    let overrides = serde_json::from_str(&text)
        .unwrap_or_else(|e| panic!("STS_HTN_PARAMS {}: {e}", path.to_string_lossy()));
    merge_json(&mut baked, overrides);
    serde_json::from_value(baked)
        .unwrap_or_else(|e| panic!("STS_HTN_PARAMS {}: {e}", path.to_string_lossy()))
});

pub fn params() -> &'static Params {
    &PARAMS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_overrides_merge_nested_objects() {
        let mut base = serde_json::json!({
            "danger_base": 10.0,
            "pick": {"Glacier": 100.0, "Defragment": 90.0}
        });
        merge_json(
            &mut base,
            serde_json::json!({
                "danger_base": 12.0,
                "pick": {"Glacier": 120.0}
            }),
        );

        assert_eq!(base["danger_base"], 12.0);
        assert_eq!(base["pick"]["Glacier"], 120.0);
        assert_eq!(base["pick"]["Defragment"], 90.0);
    }

    #[test]
    fn legacy_a0_and_promoted_bakes_are_valid_and_distinct() {
        let a0: serde_json::Value = serde_json::from_str(LEGACY_A0_JSON).unwrap();
        let a20: serde_json::Value = serde_json::from_str(BAKED_A20_JSON).unwrap();
        assert_ne!(a0, a20);
    }

    #[test]
    fn promoted_a20_bake_is_the_default_at_every_ascension() {
        assert_eq!(
            selected_baked_json(),
            (BAKED_A20_JSON, "tools/params_a20.json")
        );
    }
}
