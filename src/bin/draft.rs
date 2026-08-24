//! JSONL bridge for the compressed deck/relic-picking curriculum.
//!
//! Requests (one JSON object per line):
//! `{"op":"reset","seed":7,"character":"DEFECT"}`
//! `{"op":"step","action_index":0}`
//! `{"op":"evaluate","max_steps_per_boss":2000}`
//! `{"op":"batch_reset","seeds":[7,8,9],"character":"DEFECT"}`
//! `{"op":"batch_step","action_indices":[0,0,0]}`
//! `{"op":"batch_baseline","max_decisions":200}`

use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use sts_engine::{
    seed_from_string, BossCombatBatch, BossDraftBatch, BossDraftEnv, Character, DraftConfig,
    Unlocks,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Request {
    Reset {
        seed: Value,
        #[serde(default = "default_character")]
        character: String,
        #[serde(default)]
        config: Option<DraftConfig>,
    },
    Observe,
    Step {
        action_index: usize,
    },
    Evaluate {
        #[serde(default = "default_max_steps")]
        max_steps_per_boss: usize,
    },
    BatchReset {
        seeds: Vec<Value>,
        #[serde(default = "default_character")]
        character: String,
        #[serde(default)]
        config: Option<DraftConfig>,
    },
    BatchObserve,
    BatchStep {
        action_indices: Vec<Option<usize>>,
    },
    BatchBaseline {
        #[serde(default = "default_max_decisions")]
        max_decisions: usize,
    },
    BatchEvaluate {
        #[serde(default = "default_max_steps")]
        max_steps_per_boss: usize,
    },
    BatchFightReset {
        #[serde(default = "default_max_steps")]
        max_steps_per_fight: usize,
    },
    BatchFightObserve,
    BatchFightBaselineActions,
    BatchFightStep {
        action_indices: Vec<Option<usize>>,
    },
    BatchFightResults,
}

#[derive(Default)]
struct Session {
    single: Option<BossDraftEnv>,
    batch: Option<BossDraftBatch>,
    fights: Option<BossCombatBatch>,
}

fn default_character() -> String {
    "DEFECT".into()
}

fn default_max_steps() -> usize {
    2_000
}

fn default_max_decisions() -> usize {
    200
}

fn parse_seed(value: &Value) -> Result<i64, String> {
    if let Some(seed) = value.as_i64() {
        return Ok(seed);
    }
    let raw = value
        .as_str()
        .ok_or_else(|| "seed must be an i64 or STS seed string".to_string())?;
    raw.parse::<i64>().or_else(|_| Ok(seed_from_string(raw)))
}

fn handle(request: Request, session: &mut Session) -> Result<Value, String> {
    match request {
        Request::Reset {
            seed,
            character,
            config,
        } => {
            let seed = parse_seed(&seed)?;
            let character = Character::from_cli(&character)
                .ok_or_else(|| format!("unsupported character {character:?}"))?;
            let next = BossDraftEnv::new(
                seed,
                character,
                config.unwrap_or_default(),
                Unlocks::fixture(),
            )?;
            let observation = next.observation();
            session.single = Some(next);
            session.batch = None;
            session.fights = None;
            Ok(json!({"ok": true, "observation": observation}))
        }
        Request::Observe => {
            let env = session
                .single
                .as_ref()
                .ok_or_else(|| "reset the environment first".to_string())?;
            Ok(json!({"ok": true, "observation": env.observation()}))
        }
        Request::Step { action_index } => {
            let env = session
                .single
                .as_mut()
                .ok_or_else(|| "reset the environment first".to_string())?;
            let observation = env.step(action_index)?;
            Ok(json!({"ok": true, "observation": observation}))
        }
        Request::Evaluate { max_steps_per_boss } => {
            let env = session
                .single
                .as_ref()
                .ok_or_else(|| "reset the environment first".to_string())?;
            if !env.ready_for_bosses() {
                return Err("finish the formation schedule before evaluation".into());
            }
            Ok(json!({
                "ok": true,
                "evaluation": env.evaluate_htn(max_steps_per_boss),
            }))
        }
        Request::BatchReset {
            seeds,
            character,
            config,
        } => {
            let seeds = seeds
                .iter()
                .enumerate()
                .map(|(index, seed)| {
                    parse_seed(seed).map_err(|error| format!("seed {index}: {error}"))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let character = Character::from_cli(&character)
                .ok_or_else(|| format!("unsupported character {character:?}"))?;
            let batch = BossDraftBatch::new(
                &seeds,
                character,
                config.unwrap_or_default(),
                Unlocks::fixture(),
            )?;
            let observations = batch.observations();
            session.batch = Some(batch);
            session.single = None;
            session.fights = None;
            Ok(json!({"ok": true, "observations": observations}))
        }
        Request::BatchObserve => {
            let batch = session
                .batch
                .as_ref()
                .ok_or_else(|| "batch_reset the environments first".to_string())?;
            Ok(json!({"ok": true, "observations": batch.observations()}))
        }
        Request::BatchStep { action_indices } => {
            let batch = session
                .batch
                .as_mut()
                .ok_or_else(|| "batch_reset the environments first".to_string())?;
            let observations = batch.step(&action_indices)?;
            session.fights = None;
            Ok(json!({"ok": true, "observations": observations}))
        }
        Request::BatchBaseline { max_decisions } => {
            let batch = session
                .batch
                .as_mut()
                .ok_or_else(|| "batch_reset the environments first".to_string())?;
            let observations = batch.complete_with_htn_baseline(max_decisions)?;
            session.fights = None;
            Ok(json!({"ok": true, "observations": observations}))
        }
        Request::BatchEvaluate { max_steps_per_boss } => {
            let batch = session
                .batch
                .as_ref()
                .ok_or_else(|| "batch_reset the environments first".to_string())?;
            Ok(json!({
                "ok": true,
                "evaluations": batch.evaluate_htn(max_steps_per_boss)?,
            }))
        }
        Request::BatchFightReset {
            max_steps_per_fight,
        } => {
            let batch = session
                .batch
                .as_ref()
                .ok_or_else(|| "batch_reset the environments first".to_string())?;
            let fights = batch.start_boss_combats(max_steps_per_fight)?;
            let observations = fights.observations();
            session.fights = Some(fights);
            Ok(json!({"ok": true, "observations": observations}))
        }
        Request::BatchFightObserve => {
            let fights = session
                .fights
                .as_ref()
                .ok_or_else(|| "batch_fight_reset first".to_string())?;
            Ok(json!({"ok": true, "observations": fights.observations()}))
        }
        Request::BatchFightBaselineActions => {
            let fights = session
                .fights
                .as_mut()
                .ok_or_else(|| "batch_fight_reset first".to_string())?;
            Ok(json!({
                "ok": true,
                "action_indices": fights.baseline_action_indices(),
            }))
        }
        Request::BatchFightStep { action_indices } => {
            let fights = session
                .fights
                .as_mut()
                .ok_or_else(|| "batch_fight_reset first".to_string())?;
            let observations = fights.step(&action_indices)?;
            Ok(json!({"ok": true, "observations": observations}))
        }
        Request::BatchFightResults => {
            let fights = session
                .fights
                .as_ref()
                .ok_or_else(|| "batch_fight_reset first".to_string())?;
            Ok(json!({"ok": true, "evaluations": fights.results()?}))
        }
    }
}

fn main() {
    if std::env::args().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "sts-draft reads JSONL on stdin. Operations: reset, observe, step, evaluate,\n\
             batch_reset, batch_observe, batch_step, batch_baseline, batch_evaluate,\n\
             batch_fight_reset, batch_fight_observe, batch_fight_baseline_actions,\n\
             batch_fight_step, batch_fight_results.\n\
             reset defaults to DEFECT A20 and the 18-card/6-elite/3-shop average curriculum."
        );
        return;
    }

    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut session = Session::default();
    for line in stdin.lock().lines() {
        let response = match line {
            Err(error) => json!({"ok": false, "error": error.to_string()}),
            Ok(line) if line.trim().is_empty() => continue,
            Ok(line) => serde_json::from_str::<Request>(&line)
                .map_err(|error| format!("invalid request: {error}"))
                .and_then(|request| handle(request, &mut session))
                .unwrap_or_else(|error| json!({"ok": false, "error": error})),
        };
        serde_json::to_writer(&mut stdout, &response).expect("serialize response");
        writeln!(&mut stdout).expect("write response");
        stdout.flush().expect("flush response");
    }
}
