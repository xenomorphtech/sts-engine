use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    Choose {
        index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        label: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        x: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        y: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        room: Option<String>,
    },
    Play {
        hand_index: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_index: Option<usize>,
    },
    EndTurn,
    Potion {
        action: PotionOp,
        slot: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target_index: Option<usize>,
    },
    Proceed,
    Skip,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PotionOp {
    Use,
    Discard,
}

impl Action {
    pub fn choose(index: usize) -> Self {
        Action::Choose {
            index,
            label: None,
            x: None,
            y: None,
            room: None,
        }
    }
}
