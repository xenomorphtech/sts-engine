//! Zero-configuration native Rust frontend for procedural combat training.
//!
//! GPU:
//!     cargo run --release --features native-training-cuda --bin sts-hrm-train
//!
//! CPU:
//!     cargo run --release --features native-training --bin sts-hrm-train -- --device cpu

use std::env;
use std::path::{Path, PathBuf};
use sts_engine::combat_training::{train_native, NativeTrainingConfig, TrainingDevice};

fn usage() {
    println!(
        "Usage: sts-hrm-train [OPTIONS]\n\n\
Native procedural Defect elite/boss training. Simulation, exact forks, HTN\n\
continuations, feature encoding, autograd, AdamW, validation, and checkpointing\n\
all run in this Rust process.\n\n\
Options:\n  \
--seconds N                 wall-clock training budget (default: 600)\n  \
--device auto|cuda|cpu      tensor device (default: auto)\n  \
--output PATH               native safetensors checkpoint\n  \
--batch-scenarios N         fresh combats per optimizer batch (default: 96)\n  \
--validation-scenarios N    fresh final validation combats (default: 120)\n  \
--root-actions N            exact counterfactual actions per menu (default: 4)\n  \
--burn-in-actions N         maximum HTN prefix length (default: 16)\n  \
--seed N                    model/action-sampling seed\n  \
-h, --help                  show this help\n\n\
CUDA requires building with --features native-training-cuda. CPU-only builds\n\
use --features native-training. Most experiments need only --seconds."
    );
}

fn rooted(root: &Path, raw: String) -> PathBuf {
    let path = PathBuf::from(raw);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn positive_usize(value: String, flag: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("{flag} must be a positive integer"))?;
    if parsed == 0 {
        Err(format!("{flag} must be positive"))
    } else {
        Ok(parsed)
    }
}

fn parse_options(root: &Path) -> Result<NativeTrainingConfig, String> {
    let mut config = NativeTrainingConfig::default();
    config.output = root.join(&config.output);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                config.seconds = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "--seconds must be a number".to_string())?;
            }
            "--device" => {
                let value = next_value(&mut args, &arg)?;
                config.device = TrainingDevice::from_cli(&value)
                    .ok_or_else(|| "--device must be auto, cuda, or cpu".to_string())?;
            }
            "--output" => config.output = rooted(root, next_value(&mut args, &arg)?),
            "--batch-scenarios" => {
                config.batch_scenarios = positive_usize(next_value(&mut args, &arg)?, &arg)?;
            }
            "--validation-scenarios" => {
                config.final_validation_scenarios =
                    positive_usize(next_value(&mut args, &arg)?, &arg)?;
            }
            "--root-actions" => {
                config.root_actions = positive_usize(next_value(&mut args, &arg)?, &arg)?;
            }
            "--burn-in-actions" => {
                config.burn_in_actions = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "--burn-in-actions must be a nonnegative integer".to_string())?;
            }
            "--seed" => {
                config.seed = next_value(&mut args, &arg)?
                    .parse()
                    .map_err(|_| "--seed must be an unsigned integer".to_string())?;
            }
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    config.validate()?;
    Ok(config)
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let config = parse_options(root).unwrap_or_else(|error| {
        eprintln!("error: {error}\n");
        usage();
        std::process::exit(2);
    });
    if let Err(error) = train_native(&config) {
        eprintln!("native training failed: {error}");
        std::process::exit(1);
    }
}
