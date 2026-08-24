//! Durable Defect A20 GREEN-seed registry (JSONL).
//!
//! One object per line. `t=seed` is the latest status of a seed; `t=regression`
//! is an append-only history entry. Status is never silently deleted: a seed
//! that later mismatches becomes `regression` and a history line is appended.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
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
#[serde(tag = "t")]
enum Line {
    #[serde(rename = "meta")]
    Meta {
        character: String,
        ascension: i32,
        unlocks: String,
    },
    #[serde(rename = "seed")]
    Seed {
        seed: String,
        sts_seed: i64,
        last_ok: usize,
        snaps: usize,
        first_green_at: String,
        last_verified_at: String,
        status: GreenStatus,
    },
    #[serde(rename = "regression")]
    Regression {
        seed: String,
        at: String,
        last_ok: usize,
        detail: String,
    },
}

/// Snapshot JSON used by the previous registry format.
#[derive(Deserialize)]
struct JsonSnapshot {
    character: String,
    ascension: i32,
    unlocks: String,
    #[serde(default)]
    seeds: BTreeMap<String, GreenSeed>,
    #[serde(default)]
    regressions: Vec<RegressionEvent>,
}

#[derive(Clone, Debug)]
pub struct GreenRegistry {
    pub character: String,
    pub ascension: i32,
    pub unlocks: String,
    pub seeds: BTreeMap<String, GreenSeed>,
    pub regressions: Vec<RegressionEvent>,
    path: PathBuf,
}

impl GreenRegistry {
    pub fn new() -> Self {
        Self {
            character: "DEFECT".into(),
            ascension: 20,
            unlocks: "fixture".into(),
            seeds: BTreeMap::new(),
            regressions: Vec::new(),
            path: PathBuf::new(),
        }
    }

    /// Load JSONL, falling back to the old JSON snapshot if the JSONL is absent.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = jsonl_path(path.as_ref());
        if path.exists() {
            return Self::load_jsonl(&path);
        }
        let json_path = path.with_extension("json");
        if json_path.exists() {
            let mut reg = Self::load_json(&json_path)?;
            reg.path = path;
            return Ok(reg);
        }
        Ok(Self {
            path,
            ..Self::new()
        })
    }

    fn load_jsonl(path: &Path) -> Result<Self, String> {
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        let mut reg = Self {
            path: path.to_path_buf(),
            ..Self::new()
        };
        for (i, line) in BufReader::new(file).lines().enumerate() {
            let line = line.map_err(|e| e.to_string())?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Line>(&line) {
                Ok(Line::Meta {
                    character,
                    ascension,
                    unlocks,
                }) => {
                    reg.character = character;
                    reg.ascension = ascension;
                    reg.unlocks = unlocks;
                }
                Ok(Line::Seed {
                    seed,
                    sts_seed,
                    last_ok,
                    snaps,
                    first_green_at,
                    last_verified_at,
                    status,
                }) => {
                    reg.seeds.insert(
                        seed,
                        GreenSeed {
                            sts_seed,
                            last_ok,
                            snaps,
                            first_green_at,
                            last_verified_at,
                            status,
                        },
                    );
                }
                Ok(Line::Regression {
                    seed,
                    at,
                    last_ok,
                    detail,
                }) => {
                    reg.regressions.push(RegressionEvent {
                        seed,
                        at,
                        last_ok,
                        detail,
                    });
                }
                Err(e) => return Err(format!("{}:{}: {e}", path.display(), i + 1)),
            }
        }
        Ok(reg)
    }

    fn load_json(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let snap: JsonSnapshot = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        Ok(Self {
            character: snap.character,
            ascension: snap.ascension,
            unlocks: snap.unlocks,
            seeds: snap.seeds,
            regressions: snap.regressions,
            path: jsonl_path(path),
        })
    }

    /// Compact rewrite of the JSONL (one meta, one line per seed, then history).
    pub fn save(&self, path: impl AsRef<Path>) -> Result<(), String> {
        let path = jsonl_path(path.as_ref());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = String::new();
        out.push_str(
            &serde_json::to_string(&Line::Meta {
                character: self.character.clone(),
                ascension: self.ascension,
                unlocks: self.unlocks.clone(),
            })
            .map_err(|e| e.to_string())?,
        );
        out.push('\n');
        for (seed, rec) in &self.seeds {
            out.push_str(
                &serde_json::to_string(&Line::Seed {
                    seed: seed.clone(),
                    sts_seed: rec.sts_seed,
                    last_ok: rec.last_ok,
                    snaps: rec.snaps,
                    first_green_at: rec.first_green_at.clone(),
                    last_verified_at: rec.last_verified_at.clone(),
                    status: rec.status.clone(),
                })
                .map_err(|e| e.to_string())?,
            );
            out.push('\n');
        }
        for ev in &self.regressions {
            out.push_str(
                &serde_json::to_string(&Line::Regression {
                    seed: ev.seed.clone(),
                    at: ev.at.clone(),
                    last_ok: ev.last_ok,
                    detail: ev.detail.clone(),
                })
                .map_err(|e| e.to_string())?,
            );
            out.push('\n');
        }
        fs::write(&path, out).map_err(|e| e.to_string())
    }

    fn append_line(&self, path: impl AsRef<Path>, line: &Line) -> Result<(), String> {
        let path = jsonl_path(path.as_ref());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| e.to_string())?;
        writeln!(
            f,
            "{}",
            serde_json::to_string(line).map_err(|e| e.to_string())?
        )
        .map_err(|e| e.to_string())
    }

    pub fn record_green(&mut self, seed: &str, last_ok: usize, snaps: usize, sts_seed: i64) {
        let now = now_rfc3339();
        let first = self
            .seeds
            .get(seed)
            .map(|s| s.first_green_at.clone())
            .unwrap_or_else(|| now.clone());
        self.seeds.insert(
            seed.to_string(),
            GreenSeed {
                sts_seed,
                last_ok,
                snaps,
                first_green_at: first.clone(),
                last_verified_at: now.clone(),
                status: GreenStatus::Green,
            },
        );
        if !self.path.as_os_str().is_empty() {
            let _ = self.append_line(
                &self.path,
                &Line::Seed {
                    seed: seed.to_string(),
                    sts_seed,
                    last_ok,
                    snaps,
                    first_green_at: first,
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
            at: now.clone(),
            last_ok,
            detail: detail.to_string(),
        });
        if !self.path.as_os_str().is_empty() {
            if let Some(rec) = self.seeds.get(seed) {
                let _ = self.append_line(
                    &self.path,
                    &Line::Seed {
                        seed: seed.to_string(),
                        sts_seed: rec.sts_seed,
                        last_ok: rec.last_ok,
                        snaps: rec.snaps,
                        first_green_at: rec.first_green_at.clone(),
                        last_verified_at: rec.last_verified_at.clone(),
                        status: GreenStatus::Regression,
                    },
                );
            }
            let _ = self.append_line(
                &self.path,
                &Line::Regression {
                    seed: seed.to_string(),
                    at: now,
                    last_ok,
                    detail: detail.to_string(),
                },
            );
        }
    }

    pub fn green_count(&self) -> usize {
        self.seeds
            .values()
            .filter(|s| s.status == GreenStatus::Green)
            .count()
    }

    pub fn green_seeds(&self) -> Vec<&str> {
        self.seeds
            .iter()
            .filter(|(_, s)| s.status == GreenStatus::Green)
            .map(|(k, _)| k.as_str())
            .collect()
    }
}

impl Default for GreenRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn jsonl_path(path: &Path) -> PathBuf {
    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
        path.to_path_buf()
    } else if path.extension().and_then(|e| e.to_str()) == Some("json") {
        path.with_extension("jsonl")
    } else {
        path.to_path_buf()
    }
}

fn now_rfc3339() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jsonl_roundtrip_keeps_regression_history() {
        let dir = std::env::temp_dir().join(format!("sts-green-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("green_registry.jsonl");
        let _ = fs::remove_file(&path);

        let mut reg = GreenRegistry::load(&path).expect("load empty");
        reg.record_green("617755", 203, 198, 316940755);
        assert_eq!(reg.green_count(), 1);
        reg.record_regression("617755", 12, "first mismatch at seq 13 hp");
        assert_eq!(reg.green_count(), 0);
        assert_eq!(reg.seeds["617755"].status, GreenStatus::Regression);
        assert_eq!(reg.regressions.len(), 1);
        reg.save(&path).expect("compact");

        let loaded = GreenRegistry::load(&path).expect("reload");
        assert_eq!(loaded.green_count(), 0);
        assert_eq!(loaded.seeds["617755"].status, GreenStatus::Regression);
        assert_eq!(loaded.regressions.len(), 1);
        assert_eq!(loaded.regressions[0].detail, "first mismatch at seq 13 hp");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }
}
