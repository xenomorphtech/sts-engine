use crate::generated::orders::{DEFAULT_LOCKED_CARDS, DEFAULT_LOCKED_RELICS};
use std::collections::HashSet;

#[derive(Clone, Debug)]
pub struct Unlocks {
    pub locked_relics: HashSet<String>,
    pub locked_cards: HashSet<String>,
    pub seen_bosses: HashSet<String>,
    pub everything_unlocked: bool,
}

impl Unlocks {
    /// Profile captured by ExactTextSim: Ironclad unlock level 0, Guardian seen.
    pub fn fixture() -> Self {
        Self {
            locked_relics: DEFAULT_LOCKED_RELICS.iter().map(|s| (*s).to_string()).collect(),
            locked_cards: DEFAULT_LOCKED_CARDS.iter().map(|s| (*s).to_string()).collect(),
            seen_bosses: ["GUARDIAN", "CHAMP"].into_iter().map(str::to_string).collect(),
            everything_unlocked: false,
        }
    }

    pub fn all() -> Self {
        Self {
            locked_relics: HashSet::new(),
            locked_cards: HashSet::new(),
            seen_bosses: ["GUARDIAN", "GHOST", "SLIME", "CHAMP", "AUTOMATON", "COLLECTOR", "AWAKENED", "TIME", "DONU"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            everything_unlocked: true,
        }
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
