//! Graphics-free Slay the Spire engine for high-throughput AI training.
//!
//! The original desktop JAR remains the logic authority. This crate ports the
//! gameplay-relevant RNG and rules so a seeded command transcript can be
//! replayed without LibGDX, a GPU, or the 60 Hz action queue.

pub mod action;
pub mod card;
pub mod combat;
pub mod content;
pub mod creature;
pub mod dungeon;
pub mod env;
pub mod game;
pub mod generated;
pub mod htn;
pub mod ids;
pub mod java_util;
pub mod map;
pub mod parity;
pub mod replay;
pub mod rewards;
pub mod rng;
pub mod unlocks;
pub mod walk;

pub use action::Action;
pub use env::TrainEnv;
pub use game::{Game, Screen};
pub use ids::Character;
pub use replay::{load_commands, replay_seed};
pub use rng::{seed_from_string, RngSet, StsRandom};
pub use unlocks::Unlocks;
