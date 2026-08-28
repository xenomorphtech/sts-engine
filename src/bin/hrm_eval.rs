//! Historical PyTorch/ONNX combat-HRM evaluator.
//!
//! This consumes the superseded 500-puzzle checkpoint format, not the native
//! procedural trainer's safetensors format.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

const SOURCE_FIXTURE: &str = "fixtures/htn/defect-a0-act3-boss-winning-entry-500.jsonl.xz";
const DEFAULT_CHECKPOINT: &str = "artifacts/hrm/combat-hrm-10m.pt";
const EXPORT_SCRIPT: &str = "tools/export_hrm_onnx.py";
const DEFAULT_OUTPUT_DIR: &str = "artifacts/hrm";
const PYTORCH_INDEX: &str = "https://download.pytorch.org/whl/cu128";
const RUNTIME_SCHEMA_VERSION: u64 = 2;

struct Options {
    checkpoint: PathBuf,
    device: String,
    output_dir: PathBuf,
    max_actions: usize,
    batch_size: Option<usize>,
    branches_output: Option<PathBuf>,
}

fn usage() {
    println!(
        "Usage: sts-hrm-eval [--checkpoint PATH] [--device auto|cuda|cpu] \
[--output-dir PATH] [--max-actions N] [--batch-size N] [--branches-output PATH]\n\n\
The legacy default checkpoint path is artifacts/hrm/combat-hrm-10m.pt. The \
generated file is no longer retained. The checked-in 500-puzzle Defect A0 Act 3 \
boss fixture, automatic CUDA selection, 1000 actions per puzzle, and reports \
under artifacts/hrm.\n\n\
Inference, exact rollouts, loop detection, and reporting run in Rust. If the \
trainer-neutral ONNX artifact is missing or stale, it is exported once first. \
--batch-size overrides dynamic runtimes for benchmarking; fixed runtimes reject mismatches. \
--branches-output instead scores every legal opening action for training-split \
states and writes outcome-aware training data."
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

fn parse_options(root: &Path) -> Result<Options, String> {
    let mut options = Options {
        checkpoint: root.join(DEFAULT_CHECKPOINT),
        device: "auto".to_string(),
        output_dir: root.join(DEFAULT_OUTPUT_DIR),
        max_actions: 1000,
        batch_size: None,
        branches_output: None,
    };
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--checkpoint" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--checkpoint requires a path".to_string())?;
                options.checkpoint = rooted(root, raw);
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
                options.output_dir = rooted(root, raw);
            }
            "--max-actions" => {
                options.max_actions = args
                    .next()
                    .ok_or_else(|| "--max-actions requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--max-actions must be a positive integer".to_string())?;
                if options.max_actions == 0 {
                    return Err("--max-actions must be positive".to_string());
                }
            }
            "--batch-size" => {
                let batch_size = args
                    .next()
                    .ok_or_else(|| "--batch-size requires a value".to_string())?
                    .parse()
                    .map_err(|_| "--batch-size must be a positive integer".to_string())?;
                if batch_size == 0 {
                    return Err("--batch-size must be positive".to_string());
                }
                options.batch_size = Some(batch_size);
            }
            "--branches-output" => {
                let raw = args
                    .next()
                    .ok_or_else(|| "--branches-output requires a path".to_string())?;
                options.branches_output = Some(rooted(root, raw));
            }
            "--help" | "-h" => {
                usage();
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }
    Ok(options)
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
        .ok_or_else(|| "evaluation binary has no parent directory".to_string())?
        .join(format!("sts-htn{}", env::consts::EXE_SUFFIX));
    if sibling.exists() {
        return Ok(sibling);
    }
    eprintln!("building the rollout engine once...");
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
            "cargo completed but rollout engine is absent at {}",
            sibling.display()
        ))
    }
}

fn python_can_export(candidate: &Path) -> bool {
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

fn modified(path: &Path) -> Option<std::time::SystemTime> {
    fs::metadata(path).ok()?.modified().ok()
}

fn runtime_paths(checkpoint: &Path) -> (PathBuf, PathBuf) {
    (
        checkpoint.with_extension("onnx"),
        checkpoint.with_extension("runtime.json"),
    )
}

fn runtime_needs_export(checkpoint: &Path, onnx: &Path, metadata: &Path) -> bool {
    if !onnx.is_file() || !metadata.is_file() {
        return true;
    }
    let current_schema = fs::read(metadata)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .and_then(|manifest| manifest.get("schema_version")?.as_u64());
    if current_schema != Some(RUNTIME_SCHEMA_VERSION) {
        return true;
    }
    let Some(checkpoint_time) = modified(checkpoint) else {
        return true;
    };
    modified(onnx).is_none_or(|time| time < checkpoint_time)
        || modified(metadata).is_none_or(|time| time < checkpoint_time)
}

fn exporter_command(
    root: &Path,
    checkpoint: &Path,
    onnx: &Path,
    metadata: &Path,
) -> Result<Command, String> {
    let script = root.join(EXPORT_SCRIPT);
    let mut command = if let Some(python) = python_candidates(root)
        .into_iter()
        .find(|candidate| python_can_export(candidate))
    {
        eprintln!("exporting the native runtime with {}", python.display());
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
                "runtime export is stale, and neither a Python ONNX exporter nor uv is available"
                    .to_string(),
            );
        }
        eprintln!("provisioning the one-time ONNX export environment with uv...");
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
        .arg(checkpoint)
        .arg("--onnx")
        .arg(onnx)
        .arg("--metadata")
        .arg(metadata);
    Ok(command)
}

fn ensure_runtime(
    root: &Path,
    checkpoint: &Path,
    onnx: &Path,
    metadata: &Path,
) -> Result<(), String> {
    if !runtime_needs_export(checkpoint, onnx, metadata) {
        return Ok(());
    }
    let command = exporter_command(root, checkpoint, onnx, metadata)?;
    run_checked(command, "export the combat HRM to ONNX")?;
    if !onnx.is_file() || !metadata.is_file() {
        return Err("ONNX exporter completed without both runtime artifacts".to_string());
    }
    Ok(())
}

fn decode_fixture(root: &Path, source: &Path) -> Result<PathBuf, String> {
    let decoded = env::temp_dir().join(format!(
        "sts-hrm-eval-checkpoints-{}.jsonl",
        std::process::id()
    ));
    let output = fs::File::create(&decoded)
        .map_err(|error| format!("create {}: {error}", decoded.display()))?;
    let mut xz = Command::new("xz");
    xz.current_dir(root)
        .arg("-dc")
        .arg(source)
        .stdout(Stdio::from(output));
    if let Err(error) = run_checked(xz, "decode the boss-entry fixture") {
        let _ = fs::remove_file(&decoded);
        return Err(error);
    }
    Ok(decoded)
}

fn run() -> Result<(), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let options = parse_options(&root)?;
    let source = root.join(SOURCE_FIXTURE);
    if !source.is_file() {
        return Err(format!(
            "evaluation fixture is missing: {}",
            source.display()
        ));
    }
    if !options.checkpoint.is_file() {
        return Err(format!(
            "legacy checkpoint is missing: {}; provide a preserved compatible .pt file explicitly",
            options.checkpoint.display()
        ));
    }
    fs::create_dir_all(&options.output_dir)
        .map_err(|error| format!("create {}: {error}", options.output_dir.display()))?;
    let (onnx, metadata) = runtime_paths(&options.checkpoint);
    ensure_runtime(&root, &options.checkpoint, &onnx, &metadata)?;
    let engine = sibling_htn(&root)?;
    let mode = if options.branches_output.is_some() {
        "branch scoring"
    } else {
        "500-puzzle evaluation"
    };
    eprintln!(
        "starting native Rust {mode}: model={}, device={}, cap={}",
        onnx.display(),
        options.device,
        options.max_actions
    );
    let decoded = decode_fixture(&root, &source)?;
    let mut command = Command::new(engine);
    command
        .current_dir(&root)
        .arg("--character")
        .arg("DEFECT")
        .arg("--a0");
    if let Some(output) = &options.branches_output {
        command
            .arg("--generate-hrm-branches")
            .arg(&metadata)
            .arg(&onnx)
            .arg(&decoded)
            .arg(output);
    } else {
        command
            .arg("--eval-hrm-onnx")
            .arg(&metadata)
            .arg(&onnx)
            .arg(&decoded)
            .arg(&options.output_dir);
    }
    command
        .arg("--hrm-device")
        .arg(&options.device)
        .arg("--rollout-max-actions")
        .arg(options.max_actions.to_string());
    if let Some(batch_size) = options.batch_size {
        command.arg("--hrm-batch-size").arg(batch_size.to_string());
    }
    let result = run_checked(command, "evaluate the combat HRM in Rust");
    let _ = fs::remove_file(&decoded);
    result.map(|_| ())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("sts-hrm-eval: {message}");
        std::process::exit(1);
    }
}
