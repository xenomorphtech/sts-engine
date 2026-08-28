//! Ordered, parallel batches of independent training environments.
//!
//! The API is usable directly by an in-process Rust trainer. The JSONL
//! frontend is only an adapter around this type, so transport changes cannot
//! create a second implementation of reset/step/fork semantics.

use crate::{
    Character, CompactStepInfo, ProceduralCombatScenario, ProceduralCombatSpec, RunMeasurements,
    RunOutcome, TrainEnv, TrainingObservation,
};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum BatchRequest {
    Reset {
        seeds: Vec<i64>,
    },
    ResetCombat {
        scenarios: Vec<ProceduralCombatSpec>,
    },
    Step {
        actions: Vec<Option<usize>>,
    },
    Fork {
        branches: Vec<BatchForkRequest>,
    },
    BranchFork {
        branches: Vec<BatchForkRequest>,
    },
    BranchStep {
        actions: Vec<Option<usize>>,
    },
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct BatchForkRequest {
    pub environment: usize,
    pub action: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct BatchStepRow {
    pub index: usize,
    pub steps: usize,
    pub outcome: RunOutcome,
    pub reward: f32,
    pub terminal_score: Option<i32>,
    pub measurements: RunMeasurements,
    pub scenario: Option<ProceduralCombatScenario>,
    pub observation: Option<TrainingObservation>,
}

/// The transition fields consumed by native continuation rollouts. Root menus
/// still use [`BatchStepRow`]; branches avoid rebuilding measurements and
/// observations after every action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BatchCompactStepRow {
    pub index: usize,
    pub outcome: RunOutcome,
    pub terminal_score: Option<i32>,
}

impl BatchCompactStepRow {
    fn from_info(index: usize, info: CompactStepInfo) -> Self {
        Self {
            index,
            outcome: info.outcome,
            terminal_score: info.terminal_score,
        }
    }
}

impl BatchStepRow {
    fn from_env(index: usize, env: &TrainEnv, reward: f32, terminal_score: Option<i32>) -> Self {
        let outcome = env.outcome();
        Self {
            index,
            steps: env.steps(),
            outcome,
            reward,
            terminal_score,
            measurements: env.measurements(),
            scenario: env.scenario().cloned(),
            observation: (!outcome.done()).then(|| env.training_observation()),
        }
    }
}

/// Stateful batch executor shared by in-process trainers and protocol adapters.
///
/// Every operation preserves input ordering even though independent engine
/// transitions and observation construction run on Rayon's bounded global
/// pool. A request either succeeds completely or returns before replacing the
/// corresponding environment vector.
pub struct BatchedTrainEnv {
    ascension: i32,
    max_steps: usize,
    envs: Vec<TrainEnv>,
    branch_envs: Vec<TrainEnv>,
}

impl BatchedTrainEnv {
    pub fn new(ascension: i32, max_steps: usize) -> Self {
        Self {
            ascension,
            max_steps,
            envs: Vec::new(),
            branch_envs: Vec::new(),
        }
    }

    pub fn reset(&mut self, seeds: Vec<i64>) -> Vec<BatchStepRow> {
        self.envs = seeds
            .into_par_iter()
            .map(|seed| {
                TrainEnv::new_with_config(seed, Character::Defect, self.ascension, self.max_steps)
            })
            .collect();
        self.branch_envs.clear();
        rows(&self.envs)
    }

    pub fn reset_combat(
        &mut self,
        scenarios: Vec<ProceduralCombatSpec>,
    ) -> Result<Vec<BatchStepRow>, String> {
        let envs = scenarios
            .into_par_iter()
            .map(|scenario| {
                TrainEnv::procedural_defect_combat(scenario, self.ascension, self.max_steps)
            })
            .collect::<Result<Vec<_>, String>>()?;
        self.envs = envs;
        self.branch_envs.clear();
        Ok(rows(&self.envs))
    }

    pub fn step(&mut self, actions: Vec<Option<usize>>) -> Result<Vec<BatchStepRow>, String> {
        step_environments(&mut self.envs, actions, "step")
    }

    pub fn fork(&mut self, branches: Vec<BatchForkRequest>) -> Result<Vec<BatchStepRow>, String> {
        let (branch_envs, rows) = fork_environments(&self.envs, branches, "fork")?;
        self.branch_envs = branch_envs;
        Ok(rows)
    }

    /// Fork roots without constructing model observations for the resulting
    /// branches. Native continuation policies can read `branch_environments`
    /// directly and avoid candidate-transition cloning on every branch step.
    pub fn fork_compact(
        &mut self,
        branches: Vec<BatchForkRequest>,
    ) -> Result<Vec<BatchCompactStepRow>, String> {
        let (branch_envs, rows) = fork_environments_compact(&self.envs, branches, "compact fork")?;
        self.branch_envs = branch_envs;
        Ok(rows)
    }

    pub fn branch_fork(
        &mut self,
        branches: Vec<BatchForkRequest>,
    ) -> Result<Vec<BatchStepRow>, String> {
        let (branch_envs, rows) = fork_environments(&self.branch_envs, branches, "branch fork")?;
        self.branch_envs = branch_envs;
        Ok(rows)
    }

    pub fn branch_step(
        &mut self,
        actions: Vec<Option<usize>>,
    ) -> Result<Vec<BatchStepRow>, String> {
        step_environments(&mut self.branch_envs, actions, "branch step")
    }

    /// Advance native-policy branches without constructing unused observations.
    pub fn branch_step_compact(
        &mut self,
        actions: Vec<Option<usize>>,
    ) -> Result<Vec<BatchCompactStepRow>, String> {
        step_environments_compact(&mut self.branch_envs, actions, "compact branch step")
    }

    pub fn apply(&mut self, request: BatchRequest) -> Result<Vec<BatchStepRow>, String> {
        match request {
            BatchRequest::Reset { seeds } => Ok(self.reset(seeds)),
            BatchRequest::ResetCombat { scenarios } => self.reset_combat(scenarios),
            BatchRequest::Step { actions } => self.step(actions),
            BatchRequest::Fork { branches } => self.fork(branches),
            BatchRequest::BranchFork { branches } => self.branch_fork(branches),
            BatchRequest::BranchStep { actions } => self.branch_step(actions),
        }
    }

    /// Current root environments, for an in-process continuation policy.
    pub fn environments(&self) -> &[TrainEnv] {
        &self.envs
    }

    /// Current forked environments, for an in-process continuation policy.
    pub fn branch_environments(&self) -> &[TrainEnv] {
        &self.branch_envs
    }
}

fn rows(envs: &[TrainEnv]) -> Vec<BatchStepRow> {
    envs.par_iter()
        .enumerate()
        .map(|(index, env)| BatchStepRow::from_env(index, env, 0.0, None))
        .collect()
}

fn step_environments(
    envs: &mut [TrainEnv],
    actions: Vec<Option<usize>>,
    operation: &str,
) -> Result<Vec<BatchStepRow>, String> {
    if actions.len() != envs.len() {
        return Err(format!(
            "{operation} action count {} does not match environment count {}",
            actions.len(),
            envs.len()
        ));
    }
    Ok(envs
        .par_iter_mut()
        .zip(actions.into_par_iter())
        .enumerate()
        .map(|(index, (env, action))| {
            if let Some(action) = action.filter(|_| !env.outcome().done()) {
                let info = env.step(action);
                BatchStepRow::from_env(index, env, info.reward, info.terminal_score)
            } else {
                BatchStepRow::from_env(index, env, 0.0, None)
            }
        })
        .collect())
}

fn fork_environments(
    parents: &[TrainEnv],
    branches: Vec<BatchForkRequest>,
    operation: &str,
) -> Result<(Vec<TrainEnv>, Vec<BatchStepRow>), String> {
    let forked = branches
        .into_par_iter()
        .enumerate()
        .map(|(index, branch)| {
            let mut fork = parents
                .get(branch.environment)
                .ok_or_else(|| {
                    format!(
                        "{operation} environment {} is out of range",
                        branch.environment
                    )
                })?
                .clone();
            if fork.outcome().done() {
                return Err(format!(
                    "cannot {operation} terminal environment {}",
                    branch.environment
                ));
            }
            let info = fork.step(branch.action);
            let row = BatchStepRow::from_env(index, &fork, info.reward, info.terminal_score);
            Ok((fork, row))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(forked.into_iter().unzip())
}

fn step_environments_compact(
    envs: &mut [TrainEnv],
    actions: Vec<Option<usize>>,
    operation: &str,
) -> Result<Vec<BatchCompactStepRow>, String> {
    if actions.len() != envs.len() {
        return Err(format!(
            "{operation} action count {} does not match environment count {}",
            actions.len(),
            envs.len()
        ));
    }
    Ok(envs
        .par_iter_mut()
        .zip(actions.into_par_iter())
        .enumerate()
        .map(|(index, (env, action))| {
            let info = if let Some(action) = action.filter(|_| !env.outcome().done()) {
                env.step_compact(action)
            } else {
                CompactStepInfo {
                    outcome: env.outcome(),
                    terminal_score: None,
                }
            };
            BatchCompactStepRow::from_info(index, info)
        })
        .collect())
}

fn fork_environments_compact(
    parents: &[TrainEnv],
    branches: Vec<BatchForkRequest>,
    operation: &str,
) -> Result<(Vec<TrainEnv>, Vec<BatchCompactStepRow>), String> {
    let forked = branches
        .into_par_iter()
        .enumerate()
        .map(|(index, branch)| {
            let mut fork = parents
                .get(branch.environment)
                .ok_or_else(|| {
                    format!(
                        "{operation} environment {} is out of range",
                        branch.environment
                    )
                })?
                .clone();
            if fork.outcome().done() {
                return Err(format!(
                    "cannot {operation} terminal environment {}",
                    branch.environment
                ));
            }
            let info = fork.step_compact(branch.action);
            Ok((fork, BatchCompactStepRow::from_info(index, info)))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(forked.into_iter().unzip())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_json(row: &BatchStepRow) -> serde_json::Value {
        serde_json::to_value(row).unwrap()
    }

    #[test]
    fn parallel_reset_and_step_match_independent_environments_in_order() {
        let seeds = vec![17, -9, 42, i64::MAX, i64::MIN + 1];
        let mut batch = BatchedTrainEnv::new(20, 500);
        let reset_rows = batch.reset(seeds.clone());
        let mut independent = seeds
            .into_iter()
            .map(|seed| TrainEnv::new_with_config(seed, Character::Defect, 20, 500))
            .collect::<Vec<_>>();
        for (index, (row, env)) in reset_rows.iter().zip(&independent).enumerate() {
            assert_eq!(row.index, index);
            assert_eq!(
                row_json(row),
                row_json(&BatchStepRow::from_env(index, env, 0.0, None))
            );
        }

        let actions = vec![Some(0); independent.len()];
        let batch_rows = batch.step(actions.clone()).unwrap();
        for (index, ((row, env), action)) in batch_rows
            .iter()
            .zip(&mut independent)
            .zip(actions)
            .enumerate()
        {
            let info = env.step(action.unwrap());
            assert_eq!(
                row_json(row),
                row_json(&BatchStepRow::from_env(
                    index,
                    env,
                    info.reward,
                    info.terminal_score,
                ))
            );
        }
    }

    #[test]
    fn parallel_forks_preserve_request_order_and_exact_state() {
        let seeds = vec![101, 202];
        let requests = vec![
            BatchForkRequest {
                environment: 1,
                action: 0,
            },
            BatchForkRequest {
                environment: 0,
                action: 1,
            },
            BatchForkRequest {
                environment: 1,
                action: 2,
            },
        ];
        let mut batch = BatchedTrainEnv::new(20, 500);
        batch.reset(seeds.clone());
        let rows = batch.fork(requests.clone()).unwrap();
        let parents = seeds
            .into_iter()
            .map(|seed| TrainEnv::new_with_config(seed, Character::Defect, 20, 500))
            .collect::<Vec<_>>();

        let mut independent_branches = Vec::new();
        for (index, (row, request)) in rows.iter().zip(requests).enumerate() {
            let mut fork = parents[request.environment].clone();
            let info = fork.step(request.action);
            assert_eq!(row.index, index);
            assert_eq!(
                row_json(row),
                row_json(&BatchStepRow::from_env(
                    index,
                    &fork,
                    info.reward,
                    info.terminal_score,
                ))
            );
            independent_branches.push(fork);
        }

        let actions = vec![Some(0); independent_branches.len()];
        let rows = batch.branch_step(actions.clone()).unwrap();
        for (index, ((row, env), action)) in rows
            .iter()
            .zip(&mut independent_branches)
            .zip(actions)
            .enumerate()
        {
            let info = env.step(action.unwrap());
            assert_eq!(
                row_json(row),
                row_json(&BatchStepRow::from_env(
                    index,
                    env,
                    info.reward,
                    info.terminal_score,
                ))
            );
        }

        let requests = vec![
            BatchForkRequest {
                environment: 2,
                action: 0,
            },
            BatchForkRequest {
                environment: 0,
                action: 0,
            },
        ];
        let rows = batch.branch_fork(requests.clone()).unwrap();
        for (index, (row, request)) in rows.iter().zip(requests).enumerate() {
            let mut fork = independent_branches[request.environment].clone();
            let info = fork.step(request.action);
            assert_eq!(
                row_json(row),
                row_json(&BatchStepRow::from_env(
                    index,
                    &fork,
                    info.reward,
                    info.terminal_score,
                ))
            );
        }
    }

    #[test]
    fn compact_branches_only_omit_observation_construction() {
        let seeds = vec![303, 404];
        let requests = vec![
            BatchForkRequest {
                environment: 0,
                action: 0,
            },
            BatchForkRequest {
                environment: 1,
                action: 1,
            },
        ];
        let mut full = BatchedTrainEnv::new(20, 500);
        let mut compact = BatchedTrainEnv::new(20, 500);
        full.reset(seeds);
        // Start from literal clones so randomized HashSet iteration order in
        // debug output cannot masquerade as a transition difference.
        compact.envs = full.envs.clone();

        let full_rows = full.fork(requests.clone()).unwrap();
        let compact_rows = compact.fork_compact(requests).unwrap();
        for (expected, actual) in full_rows.into_iter().zip(&compact_rows) {
            assert!(expected.observation.is_some());
            assert_eq!(expected.index, actual.index);
            assert_eq!(expected.outcome, actual.outcome);
            assert_eq!(expected.terminal_score, actual.terminal_score);
        }
        for (full_env, compact_env) in full
            .branch_environments()
            .iter()
            .zip(compact.branch_environments())
        {
            assert_eq!(
                format!("{:?}", full_env.game),
                format!("{:?}", compact_env.game)
            );
        }

        let actions = vec![Some(0), Some(0)];
        let full_rows = full.branch_step(actions.clone()).unwrap();
        let compact_rows = compact.branch_step_compact(actions).unwrap();
        for (expected, actual) in full_rows.into_iter().zip(&compact_rows) {
            assert_eq!(expected.index, actual.index);
            assert_eq!(expected.outcome, actual.outcome);
            assert_eq!(expected.terminal_score, actual.terminal_score);
        }
        for (full_env, compact_env) in full
            .branch_environments()
            .iter()
            .zip(compact.branch_environments())
        {
            assert_eq!(
                format!("{:?}", full_env.game),
                format!("{:?}", compact_env.game)
            );
        }
    }
}
