use crate::generated::orders::{DEFAULT_LOCKED_CARDS, DEFAULT_LOCKED_RELICS};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct Unlocks {
    pub locked_relics: HashSet<String>,
    pub locked_cards: HashSet<String>,
    pub seen_bosses: HashSet<String>,
    pub everything_unlocked: bool,
    /// Java `Settings.isFinalActAvailable`: Ironclad+Silent+Defect WIN, not daily/trial.
    pub final_act_available: bool,
}

impl Unlocks {
    /// Java ExactTextSim instance profile (`runtime/profile-fixture` by default).
    ///
    /// Same prefs UnlockTracker / Settings read: `STSSeenBosses`, `STSUnlocks`,
    /// `STSPlayer` `*_WIN`. Override with `STS_PROFILE_FIXTURE`.
    pub fn fixture() -> Self {
        profile_cache().clone()
    }

    /// Guardian+Champ seen, default card/relic locks. For transcripts captured
    /// before hunts used `runtime/profile-fixture` (all bosses seen).
    pub fn guardian_champ() -> Self {
        legacy_hardcoded()
    }

    pub fn all() -> Self {
        Self {
            locked_relics: HashSet::new(),
            locked_cards: HashSet::new(),
            seen_bosses: [
                "GUARDIAN",
                "GHOST",
                "SLIME",
                "CHAMP",
                "AUTOMATON",
                "COLLECTOR",
                "CROW",
                "DONUT",
                "WIZARD",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
            everything_unlocked: true,
            final_act_available: true,
        }
    }

    pub fn from_profile_dir(dir: impl AsRef<Path>) -> Option<Self> {
        load_profile_dir(dir.as_ref())
    }

    pub fn relic_locked(&self, id: &str) -> bool {
        !self.everything_unlocked && self.locked_relics.contains(id)
    }

    pub fn card_locked(&self, id: &str) -> bool {
        !self.everything_unlocked && self.locked_cards.contains(id)
    }

    pub fn boss_seen(&self, key: &str) -> bool {
        self.seen_bosses.contains(key)
    }
}

fn profile_cache() -> &'static Unlocks {
    static CACHED: OnceLock<Unlocks> = OnceLock::new();
    CACHED.get_or_init(|| {
        if let Some(dir) = discover_profile_dir() {
            if let Some(loaded) = load_profile_dir(&dir) {
                return loaded;
            }
        }
        legacy_hardcoded()
    })
}

/// Prefs tree Java `launch.sh` copies into each instance (`betaPreferences/` inside).
pub fn discover_profile_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("STS_PROFILE_FIXTURE") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(p) = std::env::var("STS_RUNTIME") {
        let p = PathBuf::from(p).join("profile-fixture");
        if p.exists() {
            return Some(p);
        }
    }
    let crate_rel =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime/profile-fixture");
    for cand in [
        PathBuf::from("exact-text-sim/runtime/profile-fixture"),
        PathBuf::from("../exact-text-sim/runtime/profile-fixture"),
        crate_rel,
    ] {
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

fn prefs_dir(profile: &Path) -> PathBuf {
    let beta = profile.join("betaPreferences");
    if beta.is_dir() {
        beta
    } else {
        profile.to_path_buf()
    }
}

fn load_profile_dir(profile: &Path) -> Option<Unlocks> {
    let prefs = prefs_dir(profile);
    let bosses = read_prefs(prefs.join("STSSeenBosses")).unwrap_or_default();
    let unlocks = read_prefs(prefs.join("STSUnlocks")).unwrap_or_default();
    let player = read_prefs(prefs.join("STSPlayer")).unwrap_or_default();

    let mut seen_bosses = HashSet::new();
    for (key, val) in &bosses {
        if pref_int(val) == 1 {
            seen_bosses.insert(key.clone());
        }
    }

    let locked_cards = DEFAULT_LOCKED_CARDS
        .iter()
        .filter(|id| pref_int(unlocks.get(**id).unwrap_or(&Value::Null)) != 2)
        .map(|s| (*s).to_string())
        .collect();
    let locked_relics = DEFAULT_LOCKED_RELICS
        .iter()
        .filter(|id| pref_int(unlocks.get(**id).unwrap_or(&Value::Null)) != 2)
        .map(|s| (*s).to_string())
        .collect();

    // Settings.setFinalActAvailability: Ironclad AND Silent AND Defect WIN.
    let final_act_available = pref_truthy(player.get("IRONCLAD_WIN").unwrap_or(&Value::Null))
        && pref_truthy(player.get("THE_SILENT_WIN").unwrap_or(&Value::Null))
        && pref_truthy(player.get("DEFECT_WIN").unwrap_or(&Value::Null));

    Some(Unlocks {
        locked_relics,
        locked_cards,
        seen_bosses,
        everything_unlocked: false,
        final_act_available,
    })
}

fn read_prefs(path: PathBuf) -> Option<HashMap<String, Value>> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

fn pref_int(v: &Value) -> i32 {
    match v {
        Value::Number(n) => n.as_i64().unwrap_or(0) as i32,
        Value::String(s) => {
            if s.eq_ignore_ascii_case("true") {
                1
            } else {
                s.parse().unwrap_or(0)
            }
        }
        Value::Bool(true) => 1,
        _ => 0,
    }
}

fn pref_truthy(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        Value::String(s) => s.eq_ignore_ascii_case("true") || s == "1",
        Value::Number(n) => n.as_i64().unwrap_or(0) != 0,
        _ => false,
    }
}

/// Used only when the Java profile tree is missing (engine-only checkout).
fn legacy_hardcoded() -> Unlocks {
    Unlocks {
        locked_relics: DEFAULT_LOCKED_RELICS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        locked_cards: DEFAULT_LOCKED_CARDS
            .iter()
            .map(|s| (*s).to_string())
            .collect(),
        seen_bosses: ["GUARDIAN", "CHAMP"]
            .into_iter()
            .map(str::to_string)
            .collect(),
        everything_unlocked: false,
        final_act_available: true,
    }
}
