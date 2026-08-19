//! Durable Defect A20 GREEN-seed registry.
//!
//! Status is never silently deleted: a seed that later mismatches becomes
//! `regression` and is appended to `regressions`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GreenStatus {
    Green,
    Regression,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GreenSeed {
    pub sts_seed: i64,
    pub last_ok: usize,
    pub snaps: usize,
    pub first_green_at: String,
    pub last_verified_at: String,
    pub status: GreenStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegressionEvent {
    pub seed: String,
    pub at: String,
    pub last_ok: usize,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GreenRegistry {
    pub character: String,
    pub ascension: i32,
    pub unlocks: String,
    pub seeds: BTreeMap<String, GreenSeed>,
    #[serde(default)]
    pub regressions: Vec<RegressionEvent>,
}

impl GreenRegistry {
    pub fn new() -> Self {
        Self {
            character: "DEFECT".into(),
            ascension: 20,
            unlocks: "fixture".into(),
            seeds: BTreeMap::new(),
            regressions: Vec::new(),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        if !path.exists() {
            return Ok(Self::new());
        }
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, text + "\n").map_err(|e| e.to_string())
    }

    pub fn record_green(&mut self, seed: &str, last_ok: usize, snaps: usize, sts_seed: i64) {
        let now = now_rfc3339();
        if let Some(existing) = self.seeds.get_mut(seed) {
            existing.last_ok = last_ok;
            existing.snaps = snaps;
            existing.sts_seed = sts_seed;
            existing.last_verified_at = now;
            existing.status = GreenStatus::Green;
        } else {
            self.seeds.insert(
                seed.to_string(),
                GreenSeed {
                    sts_seed,
                    last_ok,
                    snaps,
                    first_green_at: now.clone(),
                    last_verified_at: now,
                    status: GreenStatus::Green,
                },
            );
        }
    }

    /// Mark a previously recorded seed as a regression. The seed stays in `seeds`.
    pub fn record_regression(&mut self, seed: &str, last_ok: usize, detail: &str) {
        let now = now_rfc3339();
        if let Some(existing) = self.seeds.get_mut(seed) {
            existing.status = GreenStatus::Regression;
            existing.last_verified_at = now.clone();
            existing.last_ok = last_ok;
        }
        self.regressions.push(RegressionEvent {
            seed: seed.to_string(),
            at: now,
            last_ok,
            detail: detail.to_string(),
        });
    }

    pub fn green_count(&self) -> usize {
        self.seeds
            .values()
            .filter(|s| s.status == GreenStatus::Green)
            .count()
    }
}

impl Default for GreenRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}
