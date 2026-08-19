//! Defect A20 GREEN registry: record fixture walks and retest listed seeds.

use sts_engine::green_registry::{GreenRegistry, GreenStatus};
use sts_engine::ids::Character;
use sts_engine::walk::{default_config, walk_oracle};
use sts_engine::Unlocks;
use std::path::PathBuf;

fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a20/green_registry.json")
}

fn walk_a20(seed: &str) -> Result<sts_engine::walk::WalkOk, sts_engine::walk::WalkFail> {
    let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 20);
    walk_oracle(&cfg)
}

const KNOWN: &[&str] = &[
    "617755", "620036", "649580", "524210", "155525", "840291", "778899",
    "895058", "417034", "103554", "403978", "745998", "790949", "992980",
];

#[test]
fn known_a20_greens_walk_and_are_recorded() {
    let mut reg = GreenRegistry::load(&registry_path()).expect("load registry");
    for seed in KNOWN {
        match walk_a20(seed) {
            Ok(ok) => {
                // last_ok is sequence; snaps is compared envelopes. stall_diag
                // lines increment sequence without a compare, so they need not
                // be equal.
                assert!(ok.snaps > 0, "{seed} empty walk");
                reg.record_green(seed, ok.last_ok, ok.snaps, ok.seed);
            }
            Err(fail) if fail.mismatched == ["io"] => {
                panic!("{seed} oracle missing: {}", fail.boundary);
            }
            Err(fail) => panic!("{seed} is not GREEN:\n{fail}"),
        }
    }
    reg.save(&registry_path()).expect("save registry");

    let reloaded = GreenRegistry::load(&registry_path()).expect("reload");
    for seed in KNOWN {
        let rec = reloaded.seeds.get(*seed).unwrap_or_else(|| panic!("{seed} missing from registry"));
        assert_eq!(rec.status, GreenStatus::Green, "{seed} status");
    }

    for (seed, rec) in &reloaded.seeds {
        if rec.status != GreenStatus::Green {
            continue;
        }
        match walk_a20(seed) {
            Ok(_) => {}
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => panic!("registry seed {seed} regressed:\n{fail}"),
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
