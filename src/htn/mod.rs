//! Hierarchical-task-network autoplayer, ported from `exact-text-sim/tools/htn`.
//!
//! The Python agent drove ExactTextSim snapshots. This port decides from a
//! live `Game` using the same task tree (WinRun → CompleteAct → room methods)
//! and the same map/reward/rest/shop/card-pick tables.

mod agent;
mod strategy;
mod turnplan;

pub use agent::HtnAgent;
