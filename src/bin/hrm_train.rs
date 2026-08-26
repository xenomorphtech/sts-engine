//! Zero-configuration frontend for the default Act 3 boss HRM experiment.
//!
//! The ordinary workflow is simply:
//!
//!     cargo run --release --bin sts-hrm-train
//!
//! It expands and validates the checked-in 500-puzzle fixture when needed,
//! provisions PyTorch through uv when no suitable Python exists, trains for five
//! wall-clock minutes, and writes the checkpoint, metrics, FP16 ONNX policy,
//! and Rust runtime manifest under artifacts/hrm.

use std::env;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::SystemTime;

const SOURCE_FIXTURE: &str = "fixtures/htn/defect-a0-act3-boss-winning-entry-500.jsonl.xz";
const DATASET_NAME: &str = "defect-a0-act3-boss-hrm-puzzles.jsonl";
const TRAIN_SCRIPT: &str = "tools/train_hrm_combat.py";
const DEFAULT_OUTPUT_DIR: &str = "artifacts/hrm";
const PYTORCH_INDEX: &str = "https://download.pytorch.org/whl/cu128";

struct Options {
    seconds: f64,
    device: String,
    output_dir: PathBuf,
    rebuild_data: bool,
}

fn usage() {
    println!(
        "Usage: sts-hrm-train [--seconds N] [--device auto|cuda|cpu] \
[--output-dir PATH] [--rebuild-data]\n\n\
Defaults: checked-in 500-puzzle Defect A0 Act 3 boss fixture, 300 seconds, \
automatic CUDA selection, model defaults from tools/train_hrm_combat.py, and \
outputs in artifacts/hrm.\n\n\
With no arguments this performs the complete standard experiment."
    );
}

fn parse_options(root: &Path) -> Result<Options, String> {
    let mut options = Options {
        seconds: 300.0,
        device: "auto".to_string(),
        output_dir: root.join(DEFAULT_OUTPUT_DIR),
        rebuild_data: false,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                options.seconds = args
                    .next()
                    .ok_or_else(|| "--seconds requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--seconds must be a number".to_string())?;
                if !options.seconds.is_finite() || options.seconds <= 0.0 {
                    return Err("--seconds must be positive".to_string());
                }
            }
            "--device" => {
                options.device = args
                    .next()
                    .ok_or_else(|| "--device requires auto, cuda, or cpu".to_string())?;
                if !matches!(options.device.as_str(), "auto" | "cuda" | "cpu") {
                    return Err("--device must be auto, cuda, or cpu".to_string());
                }
            }
            "--output-dir" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--output-dir requires a path".to_string())?;
                let path = PathBuf::from(raw);
                options.output_dir = if path.is_absolute() {
                    path
                } else {
                    root.join(path)
                };
            }
            "--rebuild-data" => options.rebuild_data = true,
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(options)
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn needs_rebuild(source: &Path, dataset: &Path, forced: bool) -> bool {
    forced
        || !dataset.exists()
        || match (modified(source), modified(dataset)) {
            (Some(source), Some(dataset)) => source > dataset,
            _ => true,
        }
}

fn run_checked(mut command: Command, description: &str) -> Result<ExitStatus, String> {
    let status = command
        .status()
        .map_err(|error| format!("could not {description}: {error}"))?;
    if !status.success() {
        return Err(format!("{description} failed with {status}"));
    }
    Ok(status)
}

fn sibling_htn(root: &Path) -> Result<PathBuf, String> {
    let current = env::current_exe().map_err(|error| error.to_string())?;
    let sibling = current
        .parent()
        .ok_or_else(|| "training binary has no parent directory".to_string())?
        .join(format!("sts-htn{}", env::consts::EXE_SUFFIX));
    if sibling.exists() {
        return Ok(sibling);
    }

    eprintln!("building the replay exporter once...");
    let profile = current
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str());
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(root)
        .arg("build")
        .arg("--bin")
        .arg("sts-htn");
    if profile == Some("release") {
        cargo.arg("--release");
    }
    run_checked(cargo, "build sts-htn")?;
    if sibling.exists() {
        Ok(sibling)
    } else {
        Err(format!(
            "cargo completed but replay exporter is absent at {}",
            sibling.display()
        ))
    }
}

fn prepare_dataset(
    root: &Path,
    source: &Path,
    dataset: &Path,
    rebuild: bool,
) -> Result<(), String> {
    if !needs_rebuild(source, dataset, rebuild) {
        eprintln!("using cached replay-expanded dataset {}", dataset.display());
        return Ok(());
    }
    fs::create_dir_all(
        dataset
            .parent()
            .ok_or_else(|| "dataset path has no parent".to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let process_id = std::process::id();
    let decoded = env::temp_dir().join(format!("sts-hrm-source-{process_id}.jsonl"));
    let pending = dataset.with_extension(format!("jsonl.pending-{process_id}"));
    eprintln!("decoding {}...", source.display());
    let decoded_file =
        File::create(&decoded).map_err(|error| format!("create {}: {error}", decoded.display()))?;
    let mut xz = Command::new("xz");
    xz.arg("-dc")
        .arg(source)
        .stdout(Stdio::from(decoded_file))
        .current_dir(root);
    if let Err(error) = run_checked(xz, "decode the training fixture") {
        let _ = fs::remove_file(&decoded);
        return Err(error);
    }

    let exporter = sibling_htn(root)?;
    eprintln!("replaying and expanding all 500 boss puzzles...");
    let mut export = Command::new(exporter);
    export
        .current_dir(root)
        .arg("--a0")
        .arg("--export-hrm-boss-jsonl")
        .arg(&decoded)
        .arg(&pending);
    let result = run_checked(export, "export HRM boss trajectories");
    let _ = fs::remove_file(&decoded);
    if let Err(error) = result {
        let _ = fs::remove_file(&pending);
        return Err(error);
    }
    fs::rename(&pending, dataset).map_err(|error| {
        format!(
            "move {} to {}: {error}",
            pending.display(),
            dataset.display()
        )
    })?;
    Ok(())
}

fn python_has_training_dependencies(candidate: &Path) -> bool {
    Command::new(candidate)
        .args(["-c", "import torch, onnx, onnxscript"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn python_candidates(root: &Path) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(override_path) = env::var_os("STS_HRM_PYTHON") {
        candidates.push(PathBuf::from(override_path));
    }
    candidates.push(root.join(".venv").join("bin").join("python"));
    candidates.push(PathBuf::from("python3"));
    candidates
}

fn training_command(
    root: &Path,
    options: &Options,
    source: &Path,
    dataset: &Path,
) -> Result<Command, String> {
    let script = root.join(TRAIN_SCRIPT);
    let mut command = if let Some(python) = python_candidates(root)
        .into_iter()
        .find(|candidate| python_has_training_dependencies(candidate))
    {
        eprintln!("using PyTorch from {}", python.display());
        let mut command = Command::new(python);
        command.arg(&script);
        command
    } else {
        let uv = PathBuf::from("uv");
        let uv_available = Command::new(&uv)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success());
        if !uv_available {
            return Err(
                "no Python with PyTorch and no uv executable; install uv or set STS_HRM_PYTHON"
                    .to_string(),
            );
        }
        eprintln!("provisioning the CUDA PyTorch environment with uv...");
        let mut command = Command::new(uv);
        command
            .arg("run")
            .arg("--python")
            .arg("3.12")
            .arg("--with")
            .arg("torch")
            .arg("--with")
            .arg("numpy")
            .arg("--with")
            .arg("onnx")
            .arg("--with")
            .arg("onnxscript")
            .arg("--extra-index-url")
            .arg(PYTORCH_INDEX)
            .arg(&script);
        command
    };
    command
        .current_dir(root)
        .arg("--dataset")
        .arg(dataset)
        .arg("--source-fixture")
        .arg(source)
        .arg("--output-dir")
        .arg(&options.output_dir)
        .arg("--train-seconds")
        .arg(options.seconds.to_string())
        .arg("--device")
        .arg(&options.device);
    Ok(command)
}

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let options = parse_options(&root)?;
    let source = root.join(SOURCE_FIXTURE);
    if !source.is_file() {
        return Err(format!("training fixture is missing: {}", source.display()));
    }
    fs::create_dir_all(&options.output_dir)
        .map_err(|error| format!("create {}: {error}", options.output_dir.display()))?;
    let dataset = options.output_dir.join(DATASET_NAME);
    prepare_dataset(&root, &source, &dataset, options.rebuild_data)?;

    eprintln!(
        "starting combat HRM training: {:.1}s, device={}, output={}",
        options.seconds,
        options.device,
        options.output_dir.display()
    );
    let command = training_command(&root, &options, &source, &dataset)?;
    run_checked(command, "train the combat HRM")?;
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("sts-hrm-train: {message}");
        std::process::exit(1);
    }
}
