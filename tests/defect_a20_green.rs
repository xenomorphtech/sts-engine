//! Defect A20 GREEN registry: walk seeds listed in the JSONL file.
//!
//! The file `exact-text-sim/runtime/oracles/defect/a20/green_registry.jsonl`
//! is the source of truth. Do not keep a Rust array of seeds.

use std::path::PathBuf;
use sts_engine::green_registry::{GreenRegistry, GreenStatus};
use sts_engine::ids::Character;
use sts_engine::walk::{default_config, walk_oracle};
use sts_engine::Unlocks;

fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a20/green_registry.jsonl")
}

fn walk_a20(seed: &str) -> Result<sts_engine::walk::WalkOk, sts_engine::walk::WalkFail> {
    let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
    walk_oracle(&cfg)
}

const ORIGINAL: &[&str] = &["617755", "620036", "649580", "524210"];

#[test]
fn registry_file_lists_original_fixture_greens() {
    let reg = GreenRegistry::load(&registry_path()).expect("load registry jsonl");
    assert!(
        reg.green_count() >= ORIGINAL.len(),
        "registry_green={} is below the original fixture set",
        reg.green_count()
    );
    for seed in ORIGINAL {
        let rec = reg
            .seeds
            .get(*seed)
            .unwrap_or_else(|| panic!("{seed} missing from green_registry.jsonl"));
        assert_eq!(rec.status, GreenStatus::Green, "{seed} status");
    }
}

#[test]
fn registry_greens_still_walk() {
    let reg = GreenRegistry::load(&registry_path()).expect("load registry jsonl");
    let seeds: Vec<String> = reg.green_seeds().into_iter().map(str::to_string).collect();
    assert!(!seeds.is_empty(), "green_registry.jsonl has no GREEN seeds");
    for seed in &seeds {
        match walk_a20(seed) {
            Ok(ok) => {
                assert!(ok.snaps > 0, "{seed} empty walk");
            }
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => panic!("registry seed {seed} is not GREEN:\n{fail}"),
        }
    }
}

#[test]
fn regression_is_recorded_not_deleted() {
    let mut reg = GreenRegistry::new();
    reg.record_green("617755", 203, 198, 316940755);
    assert_eq!(reg.green_count(), 1);
    assert!(reg.seeds.contains_key("617755"));

    reg.record_regression("617755", 12, "first mismatch at seq 13 hp");
    assert!(
        reg.seeds.contains_key("617755"),
        "regression must not drop the seed"
    );
    assert_eq!(reg.seeds["617755"].status, GreenStatus::Regression);
    assert_eq!(reg.green_count(), 0);
    assert_eq!(reg.regressions.len(), 1);
    assert_eq!(reg.regressions[0].seed, "617755");
    assert_eq!(reg.regressions[0].detail, "first mismatch at seq 13 hp");
}
