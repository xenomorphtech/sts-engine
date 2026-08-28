//! Teacher-free full-run experiment frontend.
//!
//! This baseline samples legal actions from the first Neow decision through
//! death, the Act 3 boss, or the episode cap. It never constructs an HTN agent.

use rayon::prelude::*;
use serde::Serialize;
use std::env;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::PathBuf;
use std::time::Instant;
use sts_engine::{
    BatchRequest, BatchedTrainEnv, Character, RunMeasurements, RunOutcome, TrainEnv,
    TrainingObservation,
};

#[derive(Clone, Debug)]
struct Options {
    count: usize,
    seed_source: u64,
    ascension: i32,
    max_steps: usize,
    output: Option<PathBuf>,
    transitions: Option<PathBuf>,
    serve_jsonl: bool,
}

#[derive(Clone, Debug, Serialize)]
struct EpisodeResult {
    seed: i64,
    steps: usize,
    max_floor: i32,
    outcome: RunOutcome,
    terminal_score: i32,
    terminal: RunMeasurements,
}

#[derive(Clone, Debug, Serialize)]
struct TraceStep {
    step: usize,
    observation: TrainingObservation,
    action_index: usize,
    before: RunMeasurements,
    after: RunMeasurements,
    reward: f32,
    outcome: RunOutcome,
}

#[derive(Clone, Debug, Serialize)]
struct EpisodeTrace {
    schema_version: u32,
    result: EpisodeResult,
    transitions: Vec<TraceStep>,
}

#[derive(Clone, Copy)]
struct SplitMix64(u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E3779B97F4A7C15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94D049BB133111EB);
        value ^ (value >> 31)
    }
}

fn usage() {
    println!(
        "Usage: sts-selfplay [--count N] [--seed-source N] [--ascension 0..20] [--max-steps N]\n\
                    [--output-jsonl PATH] [--transitions-jsonl PATH]\n\n\
Runs a teacher-free random-policy baseline on Defect, starting at the first Neow choice.\n\
Defaults: 1000 episodes, seed source 1, and a 5000-step cap.\n\
--serve-jsonl exposes batched reset/step requests for neural self-play; it emits no HTN choice.\n\
The protocol accepts reset_combat with fresh procedural elite/boss scenario specs."
    );
}

fn parse_options() -> Result<Options, String> {
    let mut options = Options {
        count: 1_000,
        seed_source: 1,
        ascension: 0,
        max_steps: TrainEnv::DEFAULT_MAX_STEPS,
        output: None,
        transitions: None,
        serve_jsonl: false,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--count" => {
                options.count = args
                    .next()
                    .ok_or_else(|| "--count requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--count must be a positive integer".to_string())?;
                if options.count == 0 {
                    return Err("--count must be positive".to_string());
                }
            }
            "--seed-source" => {
                options.seed_source = args
                    .next()
                    .ok_or_else(|| "--seed-source requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--seed-source must be an unsigned integer".to_string())?;
            }
            "--ascension" => {
                options.ascension = args
                    .next()
                    .ok_or_else(|| "--ascension requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--ascension must be an integer from 0 through 20".to_string())?;
                if !(0..=20).contains(&options.ascension) {
                    return Err("--ascension must be between 0 and 20".to_string());
                }
            }
            "--max-steps" => {
                options.max_steps = args
                    .next()
                    .ok_or_else(|| "--max-steps requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--max-steps must be a positive integer".to_string())?;
                if options.max_steps == 0 {
                    return Err("--max-steps must be positive".to_string());
                }
            }
            "--output-jsonl" => {
                options.output =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "--output-jsonl requires a path".to_string()
                    })?));
            }
            "--transitions-jsonl" => {
                options.transitions =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        "--transitions-jsonl requires a path".to_string()
                    })?));
            }
            "--serve-jsonl" => options.serve_jsonl = true,
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(options)
}

fn serve_jsonl(ascension: i32, max_steps: usize) -> Result<(), String> {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut writer = BufWriter::new(stdout.lock());
    let mut batch = BatchedTrainEnv::new(ascension, max_steps);
    for line in BufReader::new(stdin.lock()).lines() {
        let line = line.map_err(|error| error.to_string())?;
        if line.trim().is_empty() {
            continue;
        }
        let request: BatchRequest = serde_json::from_str(&line)
            .map_err(|error| format!("invalid self-play protocol request: {error}"))?;
        let rows = batch.apply(request)?;
        serde_json::to_writer(&mut writer, &rows).map_err(|error| error.to_string())?;
        writer.write_all(b"\n").map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn episode(
    seed: i64,
    ascension: i32,
    max_steps: usize,
    record_transitions: bool,
) -> EpisodeTrace {
    let mut env = TrainEnv::new_with_config(seed, Character::Defect, ascension, max_steps);
    let mut random = SplitMix64(seed as u64 ^ 0xD3FEC7A0);
    let mut max_floor = env.game.dungeon.floor;
    let mut transitions = Vec::new();
    loop {
        let legal = env.game.legal_actions();
        if legal.is_empty() {
            break;
        }
        let action_index = (random.next() as usize) % legal.len();
        let before = env.measurements();
        let observation = record_transitions.then(|| env.training_observation());
        let info = env.step(action_index);
        max_floor = max_floor.max(env.game.dungeon.floor);
        if let Some(observation) = observation {
            transitions.push(TraceStep {
                step: env.steps() - 1,
                observation,
                action_index,
                before,
                after: info.measurements.clone(),
                reward: info.reward,
                outcome: info.outcome,
            });
        }
        if info.done {
            return EpisodeTrace {
                schema_version: 1,
                result: EpisodeResult {
                    seed,
                    steps: env.steps(),
                    max_floor,
                    outcome: info.outcome,
                    terminal_score: info.terminal_score.unwrap_or(-1),
                    terminal: info.measurements,
                },
                transitions,
            };
        }
    }
    let terminal = env.measurements();
    EpisodeTrace {
        schema_version: 1,
        result: EpisodeResult {
            seed,
            steps: env.steps(),
            max_floor,
            outcome: env.outcome(),
            terminal_score: -terminal.enemy_hp.max(1),
            terminal,
        },
        transitions,
    }
}

fn write_jsonl<T: Serialize>(path: &PathBuf, rows: impl Iterator<Item = T>) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    let file = File::create(path).map_err(|error| format!("create {}: {error}", path.display()))?;
    let mut writer = BufWriter::new(file);
    for row in rows {
        serde_json::to_writer(&mut writer, &row).map_err(|error| error.to_string())?;
        writer.write_all(b"\n").map_err(|error| error.to_string())?;
    }
    writer.flush().map_err(|error| error.to_string())
}

fn run() -> Result<(), String> {
    let options = parse_options()?;
    if options.serve_jsonl {
        return serve_jsonl(options.ascension, options.max_steps);
    }
    let mut seed_rng = SplitMix64(options.seed_source);
    let seeds: Vec<i64> = (0..options.count).map(|_| seed_rng.next() as i64).collect();
    let started = Instant::now();
    let traces: Vec<_> = seeds
        .par_iter()
        .map(|seed| {
            episode(
                *seed,
                options.ascension,
                options.max_steps,
                options.transitions.is_some(),
            )
        })
        .collect();
    let elapsed = started.elapsed().as_secs_f64();
    let results: Vec<_> = traces.iter().map(|trace| &trace.result).collect();

    if let Some(path) = &options.output {
        write_jsonl(path, results.iter().copied())?;
    }
    if let Some(path) = &options.transitions {
        write_jsonl(path, traces.iter())?;
    }

    let wins = results
        .iter()
        .filter(|result| result.outcome == RunOutcome::Act3BossVictory)
        .count();
    let deaths = results
        .iter()
        .filter(|result| result.outcome == RunOutcome::PlayerDeath)
        .count();
    let capped = results
        .iter()
        .filter(|result| result.outcome == RunOutcome::StepLimit)
        .count();
    let mean_floor = results
        .iter()
        .map(|result| f64::from(result.max_floor))
        .sum::<f64>()
        / results.len() as f64;
    let mean_steps = results
        .iter()
        .map(|result| result.steps as f64)
        .sum::<f64>()
        / results.len() as f64;
    println!(
        "teacher_free_random defect_a{} episodes={} wins={} deaths={} capped={} win_rate={:.4} mean_floor={:.3} mean_steps={:.1} elapsed_seconds={:.3} episodes_per_second={:.1}",
        options.ascension,
        results.len(),
        wins,
        deaths,
        capped,
        wins as f64 / results.len() as f64,
        mean_floor,
        mean_steps,
        elapsed,
        results.len() as f64 / elapsed.max(f64::EPSILON),
    );
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("error: {message}");
        std::process::exit(2);
    }
}
