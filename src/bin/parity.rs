//! Walk an ExactTextSim oracle against rust and print the first mismatch.
//!
//! ```sh
//! cargo run --release --bin sts-parity -- --character DEFECT --seed 338612
//! cargo run --release --bin sts-parity -- --states path/states.jsonl --commands path/commands.jsonl
//! ```

use sts_engine::ids::Character;
use sts_engine::walk::{default_config, walk_oracle, WalkConfig};
use sts_engine::Unlocks;
use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return ExitCode::SUCCESS;
    }
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    let character = flag(args, "--character")
        .or_else(|| flag(args, "-c"))
        .and_then(|s| Character::from_cli(&s))
        .unwrap_or(Character::Defect);
    let unlocks = match flag(args, "--unlocks").unwrap_or_else(|| "fixture".into()).as_str() {
        "all" => Unlocks::all(),
        _ => Unlocks::fixture(),
    };
    let ascension = flag(args, "--ascension")
        .or_else(|| flag(args, "-a"))
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let cfg = if let (Some(states), Some(commands)) = (flag(args, "--states"), flag(args, "--commands")) {
        WalkConfig {
            name: flag(args, "--name").unwrap_or_else(|| "parity".into()),
            states: PathBuf::from(states),
            commands: PathBuf::from(commands),
            character,
            unlocks,
            ascension,
        }
    } else {
        let seed = flag(args, "--seed")
            .or_else(|| flag(args, "-s"))
            .or_else(|| positional_seed(args))
            .ok_or_else(|| "need --seed FOLDER or --states/--commands".to_string())?;
        default_config(character, &seed, unlocks, ascension)
    };
    eprintln!(
        "sts-parity {} {} unlocks={} states={}",
        cfg.character.sts_name(),
        cfg.name,
        if cfg.unlocks.everything_unlocked {
            "all"
        } else {
            "fixture"
        },
        cfg.states.display()
    );
    match walk_oracle(&cfg) {
        Ok(ok) => {
            println!(
                "{} GREEN last_ok={} / {} snaps seed={}",
                cfg.name, ok.last_ok, ok.snaps, ok.seed
            );
            Ok(())
        }
        Err(fail) => {
            print!("{fail}");
            Err(format!(
                "{} RED seq {} last_ok {}",
                fail.name, fail.seq, fail.last_ok
            ))
        }
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

fn positional_seed(args: &[String]) -> Option<String> {
    args.iter()
        .filter(|a| !a.starts_with('-'))
        .find(|a| a.chars().all(|c| c.is_ascii_digit()))
        .cloned()
}

fn print_help() {
    eprintln!(
        "\
sts-parity — lockstep rust vs ExactTextSim oracle

Usage:
  sts-parity --character DEFECT --seed 338612
  sts-parity --states FILE --commands FILE [--character DEFECT]

Options:
  -c, --character IRONCLAD|SILENT|DEFECT|WATCHER   (default DEFECT)
  -s, --seed FOLDER                                oracles/<char>/a<asc>/<seed>/
  -a, --ascension N                                default 0
      --unlocks fixture|all                        default fixture
      --states PATH --commands PATH                explicit JSONL pair
      --name LABEL                                 report name
  STS_RUNTIME  override exact-text-sim/runtime"
    );
}
