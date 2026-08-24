//! Walk later-act ExactTextSim oracles when they exist.

use sts_engine::ids::Character;
use sts_engine::walk::walk_from_runtime;
use sts_engine::Unlocks;

fn walk(name: &str, snaps_file: &str, cmds_file: &str, _min_act: i32) {
    walk_ex(
        name,
        snaps_file,
        cmds_file,
        Character::Ironclad,
        Unlocks::fixture(),
    );
}

fn walk_ex(name: &str, snaps_file: &str, cmds_file: &str, character: Character, unlocks: Unlocks) {
    match walk_from_runtime(name, snaps_file, cmds_file, character, unlocks) {
        Ok(ok) => {
            eprintln!(
                "{name} GREEN last_ok={} / {} snaps seed={}",
                ok.last_ok, ok.snaps, ok.seed
            );
            assert_eq!(
                ok.last_ok,
                ok.snaps.saturating_sub(1),
                "{name} incomplete walk"
            );
        }
        Err(fail) if fail.mismatched == ["io"] => {
            eprintln!("skipping {name}: {}", fail.boundary);
        }
        Err(fail) => panic!("{fail}"),
    }
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn later_acts_seed2_walk() {
    walk(
        "acts1-4",
        "acts1-4-seed2-pruned.jsonl",
        "acts1-4-seed2.commands.jsonl",
        3,
    );
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn walk_latest_autoplay() {
    walk(
        "latest",
        "a0-s2-latest.jsonl",
        "a0-s2-latest.commands.jsonl",
        2,
    );
}

fn walk_defect_oracle(seed: &str) {
    let states = format!("oracles/defect/a0/{seed}/states.jsonl");
    let cmds = format!("oracles/defect/a0/{seed}/commands.jsonl");
    walk_ex(
        &format!("defect-s{seed}"),
        &states,
        &cmds,
        Character::Defect,
        Unlocks::fixture(),
    );
}

#[test]
fn walk_defect_htn_s345425() {
    walk_defect_oracle("345425");
}

#[test]
fn walk_defect_oracle_s505936() {
    walk_defect_oracle("505936");
}

#[test]
fn walk_defect_oracle_s954894() {
    walk_defect_oracle("954894");
}

#[test]
fn walk_defect_oracle_s632706() {
    walk_defect_oracle("632706");
}

#[test]
fn walk_defect_oracle_s710850() {
    walk_defect_oracle("710850");
}

#[test]
fn walk_defect_oracle_s338612() {
    walk_defect_oracle("338612");
}

#[test]
fn walk_defect_oracle_s462984() {
    walk_defect_oracle("462984");
}

#[test]
fn walk_defect_oracle_s755902() {
    walk_defect_oracle("755902");
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn walk_batch2_s1() {
    walk(
        "batch2-s1",
        "batch2-s1.jsonl",
        "batch2-s1.commands.jsonl",
        1,
    );
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn walk_batch2_s3() {
    walk(
        "batch2-s3",
        "batch2-s3.jsonl",
        "batch2-s3.commands.jsonl",
        1,
    );
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn walk_batch2_s5() {
    walk(
        "batch2-s5",
        "batch2-s5.jsonl",
        "batch2-s5.commands.jsonl",
        1,
    );
}

#[test]
#[ignore = "stale ExactTextSim: published COMBAT_REWARD before FastCardObtainEffect; live GUI has the card in masterDeck"]
fn act2_seed2_walk() {
    walk(
        "act2",
        "act2-seed2-pruned.jsonl",
        "act2-seed2.commands.jsonl",
        2,
    );
}
