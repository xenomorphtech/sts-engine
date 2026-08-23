use crate::generated::orders::{DEFAULT_LOCKED_CARDS, DEFAULT_LOCKED_RELICS};
use crate::ids::{CardId, EncounterId, RelicId};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Unlocks {
    pub locked_relics: HashSet<RelicId>,
    pub locked_cards: HashSet<CardId>,
    pub seen_bosses: HashSet<EncounterId>,
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
        discover_profile_dir()
            .and_then(|dir| load_profile_dir(&dir))
            .unwrap_or_else(legacy_hardcoded)
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
                EncounterId::TheGuardian,
                EncounterId::Hexaghost,
                EncounterId::SlimeBoss,
                EncounterId::Champ,
                EncounterId::Automaton,
                EncounterId::Collector,
                EncounterId::AwakenedOne,
                EncounterId::DonuAndDeca,
                EncounterId::TimeEater,
            ]
            .into_iter()
            .collect(),
            everything_unlocked: true,
            final_act_available: true,
        }
    }

    pub fn from_profile_dir(dir: impl AsRef<Path>) -> Option<Self> {
        load_profile_dir(dir.as_ref())
    }

    pub fn relic_locked(&self, id: RelicId) -> bool {
        !self.everything_unlocked && self.locked_relics.contains(&id)
    }

    pub fn card_locked(&self, id: CardId) -> bool {
        !self.everything_unlocked && self.locked_cards.contains(&id)
    }

    pub fn boss_seen(&self, id: EncounterId) -> bool {
        self.seen_bosses.contains(&id)
    }
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
    let crate_rel = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../exact-text-sim/runtime/profile-fixture");
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
            if let Some(id) = EncounterId::from_sts_key(key) {
                seen_bosses.insert(id);
            }
        }
    }

    let locked_cards = DEFAULT_LOCKED_CARDS
        .iter()
        .filter(|id| pref_int(unlocks.get(id.sts_id()).unwrap_or(&Value::Null)) != 2)
        .copied()
        .collect();
    let locked_relics = DEFAULT_LOCKED_RELICS
        .iter()
        .filter(|id| pref_int(unlocks.get(id.sts_id()).unwrap_or(&Value::Null)) != 2)
        .copied()
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
        locked_relics: DEFAULT_LOCKED_RELICS.iter().copied().collect(),
        locked_cards: DEFAULT_LOCKED_CARDS.iter().copied().collect(),
        seen_bosses: [EncounterId::TheGuardian, EncounterId::Champ]
            .into_iter()
            .collect(),
        everything_unlocked: false,
        final_act_available: true,
    }
}
