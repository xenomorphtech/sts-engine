//! Defect A0 GREEN registry: walk seeds listed in the JSONL file.
//!
//! The file `exact-text-sim/runtime/oracles/defect/a0/green_registry.jsonl`
//! is the source of truth. Do not keep a Rust array of seeds.

use sts_engine::green_registry::{GreenRegistry, GreenStatus};
use sts_engine::ids::Character;
use sts_engine::walk::{default_config, walk_oracle};
use sts_engine::Unlocks;
use std::path::PathBuf;

fn registry_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a0/green_registry.jsonl")
}

fn walk_a0(seed: &str) -> Result<sts_engine::walk::WalkOk, sts_engine::walk::WalkFail> {
    let cfg = default_config(Character::Defect, seed, Unlocks::fixture(), 0);
    walk_oracle(&cfg)
}

#[test]
fn registry_file_is_loadable() {
    let path = registry_path();
    if !path.exists() {
        eprintln!("skip: a0 green_registry.jsonl not written yet");
        return;
    }
    let reg = GreenRegistry::load(&path).expect("load a0 green_registry.jsonl");
    assert_eq!(reg.ascension, 0, "a0 registry meta.ascension");
}

#[test]
fn harvested_a0_oracles_walk_or_report() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../exact-text-sim/runtime/oracles/defect/a0");
    let mut seen = 0usize;
    let mut green = 0usize;
    for entry in std::fs::read_dir(&root).expect("a0 oracle dir") {
        let entry = entry.unwrap();
        if !entry.file_type().unwrap().is_dir() {
            continue;
        }
        let seed = entry.file_name();
        let seed = seed.to_string_lossy();
        if seed.contains(' ')
            || !(entry.path().join("states.jsonl").exists()
                || entry.path().join("states.jsonl.gz").exists())
        {
            continue;
        }
        seen += 1;
        match walk_a0(&seed) {
            Ok(ok) => {
                assert!(ok.snaps > 0, "{seed} empty GREEN walk");
                green += 1;
            }
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => {
                eprintln!(
                    "{seed} RED last_ok {} seq {} {}",
                    fail.last_ok, fail.seq, fail.boundary
                );
            }
        }
    }
    assert!(seen > 0, "no harvested a0 oracles under {}", root.display());
    eprintln!("a0 harvested {green} GREEN / {seen} oracles");
}

#[test]
fn registry_greens_still_walk() {
    let path = registry_path();
    if !path.exists() {
        eprintln!("skip: a0 green_registry.jsonl not written yet");
        return;
    }
    let reg = GreenRegistry::load(&path).expect("load a0 registry");
    let seeds: Vec<String> = reg.green_seeds().into_iter().map(str::to_string).collect();
    for seed in &seeds {
        match walk_a0(seed) {
            Ok(ok) => {
                assert!(ok.snaps > 0, "{seed} empty walk");
            }
            Err(fail) if fail.mismatched == ["io"] => {
                eprintln!("skip missing oracle {seed}");
            }
            Err(fail) => panic!("a0 registry seed {seed} is not GREEN:\n{fail}"),
        }
    }
}

#[test]
fn masked_bandits_enters_the_three_bandit_combat() {
    // Seed 340 chooses Fight at Masked Bandits. The event must seed gold +
    // Red Mask rewards and enter the Pointy/Leader/Bear combat immediately.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 179,
                "340 still fails before Masked Bandits combat last_ok={} want > 179: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sadistic_nature_damages_after_player_debuffs() {
    // Seed 279 plays Sadistic Nature, then Go for the Eyes against Guardian.
    // Weak must apply first and queue Sadistic's 5 THORNS damage (43 -> 38).
    let cfg = default_config(Character::Defect, "279", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 185,
                "279 still fails at Sadistic Nature last_ok={} want > 185: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn inserter_adds_an_orb_slot_every_second_turn() {
    // Seed 935 owns Inserter in the first Act 2 fight. Its second turn must
    // reset the relic counter and run IncreaseMaxOrbAction before card play.
    let cfg = default_config(Character::Defect, "935", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 181,
                "935 still fails at Inserter last_ok={} want > 181: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn nest_waits_for_the_reward_choice() {
    // Seed 416 enters Nest and chooses Smash and Grab. Continue must expose
    // the reward choice; taking it grants 99 gold and leaves the event open.
    let cfg = default_config(Character::Defect, "416", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 196,
                "416 still fails at Nest last_ok={} want > 196: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn addict_leave_opens_the_map_immediately() {
    // Seed 128 leaves Addict. Java opens the map in the same buttonEffect;
    // delaying it consumes the following Vampires map click as an event exit.
    let cfg = default_config(Character::Defect, "128", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 265,
                "128 still fails after Addict last_ok={} want > 265: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn gremlin_leader_encounter_has_minions_and_leader() {
    // Seed 128 reaches the Act 2 Gremlin Leader elite after Addict. The
    // encounter is two miscRng-selected gremlins plus GremlinLeader, not Nob.
    let cfg = default_config(Character::Defect, "128", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 271,
                "128 still fails at Gremlin Leader last_ok={} want > 271: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn slavers_encounter_includes_taskmaster_and_honors_red_mask() {
    // Seed 340 reaches Slavers after Masked Bandits and Nest. Java constructs
    // Blue Slaver, Taskmaster (SlaverBoss), then Red Slaver. The Red Mask from
    // Bandits weakens all three at battle start, preventing HP loss on turn 1.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 296,
                "340 still fails at Slavers/Red Mask last_ok={} want > 296: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn buffer_absorbs_jax_hp_loss() {
    // Seed 340 plays Buffer immediately before J.A.X. in the Slavers fight.
    // Java's Buffer hook consumes the charge and prevents J.A.X.'s 3 HP loss.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 308,
                "340 still fails at Buffer/J.A.X. last_ok={} want > 308: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn bronze_automaton_spawns_orbs_around_boss() {
    // Seed 340 reaches Bronze Automaton. Java's SpawnMonsterAction sorts the
    // -300 X and +200 X orbs around the boss instead of prepending both.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 351,
                "340 still fails at Bronze Automaton formation last_ok={} want > 351: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_fire_potion_damage() {
    // Seed 340 uses Fire Potion against Orb Walker while holding Sacred Bark.
    // Java's AbstractPotion.getPotency doubles the potion's damage to 40.
    let cfg = default_config(Character::Defect, "340", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 438,
                "340 still fails at Sacred Bark/Fire Potion last_ok={} want > 438: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sozu_blocks_potion_rewards_before_we_meet_again() {
    // Seed 393 obtains Sozu after Act 1. Later potion rewards are consumed but
    // not added, so We Meet Again offers gold first and charges Java's amount.
    let cfg = default_config(Character::Defect, "393", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 391,
                "393 still fails at Sozu/We Meet Again last_ok={} want > 391: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn coffee_dripper_skips_rest_option() {
    // Seed 628 owns Coffee Dripper at an Act 2 campfire. Java leaves Rest
    // unusable, so the headless driver's compact choice index 0 opens Smith.
    let cfg = default_config(Character::Defect, "628", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 307,
                "628 still fails at Coffee Dripper campfire last_ok={} want > 307: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn amplify_queues_the_next_power_twice() {
    // Seed 89 plays Amplify, then Electrodynamics against Bronze Automaton.
    // The purged copy counts as another Power play and channels two more orbs.
    let cfg = default_config(Character::Defect, "89", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 435,
                "89 still fails at Amplify/Electrodynamics last_ok={} want > 435: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn plasma_grants_energy_at_turn_start() {
    // Seed 786 starts Act 2 with Nuclear Battery. Plasma's +1 energy lets an
    // unupgraded Tempest channel a fourth Lightning and evoke for 8 damage.
    let cfg = default_config(Character::Defect, "786", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 183,
                "786 still fails at Plasma/Tempest last_ok={} want > 183: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn gremlin_horn_draws_after_killing_card_is_discarded() {
    // Seed 786 later plays Barrage into Looter with an empty draw pile.
    // UseCardAction discards Barrage before Horn's queued draw reshuffles, so
    // Barrage participates in the shuffle and Java draws Strike_B.
    let cfg = default_config(Character::Defect, "786", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 207,
                "786 still fails at Gremlin Horn/UseCardAction order last_ok={} want > 207: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn thorns_stops_later_hits_after_killing_attacker() {
    // Seed 943 ends the Hexaghost fight with the boss at 6 HP and Thorns 3.
    // Its two-hit Tackle queues separate DamageActions; hit two is cancelled
    // after the first hit's Thorns damage makes Hexaghost start dying.
    let cfg = default_config(Character::Defect, "943", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 184,
                "943 still fails at lethal Thorns multi-hit last_ok={} want > 184: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_resets_mode_shift_after_static_discharge_evokes() {
    // Seed 490's Static Discharge evokes Lightning during Twin Slam. Java
    // resolves that damage before Offensive Mode applies a fresh Mode Shift
    // 40; applying it afterward incorrectly starts the next cycle at 32.
    let cfg = default_config(Character::Defect, "490", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 186,
                "490 still fails at Guardian/Static Discharge order last_ok={} want > 186: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn barrage_finishes_before_queued_flight_reductions() {
    // Seed 835 plays a three-orb Barrage into a Byrd with Flight 2. Java puts
    // all three DamageActions ahead of Flight's ReducePowerActions, so every
    // 4-damage hit is halved before the Byrd is grounded (6 total, not 8).
    let cfg = default_config(Character::Defect, "835", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 186,
                "835 still fails at Barrage/Flight queue order last_ok={} want > 186: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn snecko_eye_confuses_before_drawing_seven() {
    // Seed 719 enters its first Act 2 fight with Snecko Eye. atPreBattle must
    // install Confusion before the opening seven-card draw so cardRandomRng
    // assigns costs to all seven cards in Java order.
    let cfg = default_config(Character::Defect, "719", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 189,
                "719 still fails at Snecko Eye pre-battle last_ok={} want > 189: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn magnetism_generates_colorless_card_before_turn_draw() {
    // Seed 719 plays a Colorless Potion Magnetism. At the next turn start,
    // cardRandomRng picks Metamorphosis and puts it into hand before Snecko
    // Eye's seven-card draw consumes its Confusion cost rolls.
    let cfg = default_config(Character::Defect, "719", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 258,
                "719 still fails at Magnetism turn start last_ok={} want > 258: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn temporary_card_costs_reset_at_end_of_turn() {
    // Seed 48 creates a 0-cost Rip and Tear with Attack Potion, plays it, and
    // redraws it two turns later. AbstractRoom.endTurn resetAttributes makes
    // it cost 1 again; otherwise Reinforced Body incorrectly retains 1 energy.
    let cfg = default_config(Character::Defect, "48", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 192,
                "48 still fails at temporary cost reset last_ok={} want > 192: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn the_bomb_damage_triggers_slime_boss_split() {
    // Seed 887 ends a turn with Slime Boss at 74 HP. Lightning deals 3, then
    // The Bomb's THORNS damage deals 40; SlimeBoss.damage must replace its
    // weakened Slam with Split before the monster phase.
    let cfg = default_config(Character::Defect, "887", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 193,
                "887 still fails at The Bomb/Slime Boss split last_ok={} want > 193: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn preserved_insect_caps_after_emerald_max_hp_buff() {
    // Seed 79 enters the burning Gremlin Nob elite with Preserved Insect.
    // IncreaseMaxHpAction first raises 84/84 to 105/105; the later relic
    // atBattleStart hook caps current HP to floor(105 * 0.75) = 78.
    let cfg = default_config(Character::Defect, "79", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 195,
                "79 still fails at Preserved Insect/emerald HP order last_ok={} want > 195: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn city_first_strong_encounter_honors_weak_exclusions() {
    // Seed 833's second weak Act 2 encounter is Chosen. The first strong roll
    // is Cultist and Chosen, which TheCity excludes after Chosen; rerolling
    // produces 3 Cultists and keeps monster RNG aligned.
    let cfg = default_config(Character::Defect, "833", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 196,
                "833 still fails at Act 2 first-strong exclusion last_ok={} want > 196: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn player_death_wins_simultaneous_card_kill() {
    // Seed 760 plays Rip and Tear at 1 HP into Guardian's Sharp Hide. The
    // first hit's reactive damage kills the player and the second hit kills
    // Guardian; Java opens DEATH rather than awarding boss rewards.
    let cfg = default_config(Character::Defect, "760", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 197,
                "760 still fails at simultaneous player/monster death last_ok={} want > 197: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn static_discharge_evoke_can_cancel_guardian_second_hit() {
    // Seed 572 ends Guardian's first Twin Slam hit with a full orb row.
    // Static Discharge channels Lightning at the top of Java's action queue;
    // Electrodynamics makes the evoked Lightning kill Guardian before its
    // separately queued second hit, preserving 9 HP rather than 1.
    let cfg = default_config(Character::Defect, "572", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 199,
                "572 still fails at Static Discharge/Guardian queue order last_ok={} want > 199: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sharp_hide_can_kill_before_ftl_deferred_damage() {
    // FTLAction adds its DamageAction behind UseCardAction's queued power
    // hooks. On seed 350, Guardian's Sharp Hide therefore kills the 1-HP
    // player and post-death cleanup drops FTL's pending 5 damage.
    let cfg = default_config(Character::Defect, "350", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 203,
                "350 still fails at Sharp Hide/FTL queue order last_ok={} want > 203: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn thorns_killed_monster_still_rolls_when_an_enemy_remains() {
    // Seed 535 has an Acid Slime M die to Thorns during its own attack while
    // Spike Slime L remains alive. Java's already-queued RollMoveAction still
    // burns aiRng; preserving that burn keeps the large slime's next move in
    // sync on the following turn.
    let cfg = default_config(Character::Defect, "535", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 203,
                "535 still fails at dead-monster RollMoveAction last_ok={} want > 203: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn knowing_skull_leave_costs_hp_before_final_leave() {
    // Seed 535 reaches Knowing Skull at 44 HP. Its fourth ASK option costs 6
    // HP and leaves the event on its COMPLETE screen before the final Leave.
    let cfg = default_config(Character::Defect, "535", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 304,
                "535 still fails at Knowing Skull leave last_ok={} want > 304: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn the_joust_bet_roll_and_payout_follow_java_screens() {
    // Seed 21 bets 50 gold on the murderer, then miscRng rolls ownerWins=false.
    // Java pays 100 gold on the JOUST screen before presenting final Leave.
    let cfg = default_config(Character::Defect, "21", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 408,
                "21 still fails in The Joust last_ok={} want > 408: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn wheel_decay_fires_darkstone_periapt_on_obtain() {
    // Seed 100's Wheel result grants Decay while Darkstone Periapt is owned.
    // The curse obtain raises max HP and current HP by 6 before final Leave.
    let cfg = default_config(Character::Defect, "100", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 311,
                "100 still fails at Wheel curse obtain last_ok={} want > 311: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_liquid_bronze_thorns() {
    // Seed 4 uses Liquid Bronze with Sacred Bark before the Collector. Java's
    // AbstractPotion.getPotency doubles the base 3 to 6 Thorns.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 472,
                "4 still fails at Sacred Bark Liquid Bronze last_ok={} want > 472: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn collector_spawn_initializes_torch_head_moves_and_ai_rng() {
    // Seed 4 reaches the Collector with both Torch Heads dead. Each summon is
    // initialized by SpawnMonsterAction in Java, which rolls its fixed move and
    // consumes aiRng before the Collector's next move selection.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 489,
                "4 still fails after Collector summon initialization last_ok={} want > 489: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn collector_revives_dead_torch_head_slots() {
    // Seed 4 kills both original Torch Heads. Java's <=25 AI roll selects
    // REVIVE, creates one replacement for each dead enemy slot, initializes
    // both moves, and only then rolls the Collector's next move.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 511,
                "4 still fails at Collector Torch Head revive last_ok={} want > 511: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn collector_death_suicides_surviving_summons() {
    // Seed 4 kills the Collector with FTL while two revived Torch Heads live.
    // TheCollector.die queues SuicideAction for both before boss rewards open.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 535,
                "4 still fails at Collector summon cleanup last_ok={} want > 535: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn winding_halls_retrace_returns_to_map() {
    // Seed 4 takes Winding Halls' third choice. Java loses 5% max HP, shows
    // the event's Leave screen, then returns to the same-floor map.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 577,
                "4 still over-advances after Winding Halls last_ok={} want > 577: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn artifact_absorbs_lagavulin_siphon_dexterity() {
    // 213: Ancient Potion Artifact 1 eats Dexterity -1; Strength -1 still lands.
    // Defend then stays 5 block (rust had Dexterity -1 → 4, 9 vs Java 10).
    let cfg = default_config(Character::Defect, "213", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 67,
                "213 still fails at Artifact/siphon last_ok={} want > 67: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn whetstone_upgrades_attacks_at_obtain() {
    // Seed 1: Whetstone onEquip must use miscRng at instantObtain so Rip and
    // Tear is + (9x2) vs Slime Boss, not unupgraded 7x2 (mons 123 vs 119).
    let cfg = default_config(Character::Defect, "1", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 205,
                "1 still fails at lethal thorns last_ok={} want > 205: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn slime_boss_split_inserts_by_draw_x() {
    // Seed 8: slime split, Cursed Key, Champ, then Shame EOT Frail on Defend.
    let cfg = default_config(Character::Defect, "8", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 689,
                "8 still fails at Act 3 event last_ok={} want > 689: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn acid_slime_l_split_survives_roll_move() {
    // Seed 776: Bronze Scales thorns during AcidSlime_L tackle crosses 50% HP.
    // Java queues SetMoveAction(SPLIT) after RollMoveAction so getMove cannot
    // overwrite the split (player 29 vs 13, two AcidSlime_M kids).
    let cfg = default_config(Character::Defect, "776", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 73,
                "776 still fails at AcidSlime_L split last_ok={} want > 73: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn clumsy_is_ethereal_at_end_of_turn() {
    // Seed 944: Clumsy in hand at EOT must ExhaustSpecificCardAction, not
    // discard-and-reshuffle (hand Compile Driver vs Strike_B at seq 56).
    let cfg = default_config(Character::Defect, "944", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 54,
                "944 still fails at Clumsy ethereal last_ok={} want > 54: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn pantograph_heals_at_boss() {
    // Seed 5: Pantograph boss heal, then Byrd FlightPower.atStartOfTurn
    // restores storedAmount so Sweeping Beam still halves (10→7 not 10→4).
    let cfg = default_config(Character::Defect, "5", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 257,
                "5 still fails at Byrd Flight restore last_ok={} want > 257: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn runic_pyramid_keeps_hand_at_end_of_turn() {
    // 213: Runic Pyramid skips DiscardAtEndOfTurnAction. Rust discarded
    // Hologram/Consume/Defend_B; Java kept them (hand 8 vs 5 after the draw).
    let cfg = default_config(Character::Defect, "213", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 429,
                "213 still fails after Distilled Chaos Dazed autoplay last_ok={} want > 429: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn hex_does_not_insert_dazed_on_cold_snap() {
    // 213 seq 429: Cold Snap is an Attack; HexPower.onUseCard skips ATTACK.
    // Extra Dazed in hand meant rust treated the play as a non-attack.
    use sts_engine::card::Card;
    use sts_engine::combat::{play_card, Combat};
    use sts_engine::creature::{Orb, OrbKind, Player, RelicInstance};
    use sts_engine::ids::{CardId, EncounterId, PowerId, RelicId};
    use sts_engine::rng::RngSet;

    let mut rng = RngSet::generate_seeds(213);
    let mut player = Player::defect();
    player.energy = 3;
    player.max_orbs = 2;
    player.orbs = vec![
        Orb { kind: OrbKind::Frost, evoke: 9 },
        Orb { kind: OrbKind::Lightning, evoke: 12 },
    ];
    player.add_power(PowerId::Hex, 1);
    player.add_power(PowerId::Focus, 4);
    player.relics.push(RelicInstance {
        id: RelicId::InkBottle,
        counter: 8,
        used_up: false,
    });
    let mut combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 29, 213, 0);
    player.orbs = vec![
        Orb { kind: OrbKind::Frost, evoke: 9 },
        Orb { kind: OrbKind::Lightning, evoke: 12 },
    ];
    player.max_orbs = 2;
    player.hand = vec![
        Card::new(CardId::Skim),
        Card::new(CardId::Capacitor),
        Card::new(CardId::Buffer),
        Card::new(CardId::Capacitor),
        Card::new(CardId::Dazed),
        Card::new(CardId::Dazed),
        Card::new(CardId::Dazed),
        Card::new(CardId::Cold_Snap),
    ];
    player.draw.clear();
    player.discard.clear();
    play_card(&mut player, &mut combat, 7, Some(0), &mut rng, None);
    let dazed_hand = player.hand.iter().filter(|c| c.id == CardId::Dazed).count();
    let dazed_draw = player.draw.iter().filter(|c| c.id == CardId::Dazed).count();
    assert_eq!(
        dazed_hand, 3,
        "Cold Snap+Hex must not add Dazed; hand_dazed={dazed_hand} draw_dazed={dazed_draw} hand={:?}",
        player.hand.iter().map(|c| c.sts_id()).collect::<Vec<_>>()
    );
    assert_eq!(dazed_draw, 0, "Hex must not insert Dazed on an Attack");
}

#[test]
fn autoplay_dazed_skips_hex_and_ink_bottle() {
    // PlayTopCard of Dazed: canUse is false, UseCardAction.dontTriggerOnUseCard.
    // Rust used to Hex-insert and tick InkBottle, leaving an extra Dazed for
    // seed 213 Cold Snap to draw.
    use sts_engine::card::Card;
    use sts_engine::combat::{play_owned_card, Combat};
    use sts_engine::creature::{Player, RelicInstance};
    use sts_engine::ids::{CardId, EncounterId, PowerId, RelicId};
    use sts_engine::rng::RngSet;

    let mut rng = RngSet::generate_seeds(213);
    let mut player = Player::defect();
    player.add_power(PowerId::Hex, 1);
    player.relics.push(RelicInstance {
        id: RelicId::InkBottle,
        counter: 9,
        used_up: false,
    });
    let mut combat = Combat::start(EncounterId::Cultist, &mut player, &mut rng, 1, 213, 0);
    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    play_owned_card(
        &mut player,
        &mut combat,
        Card::new(CardId::Dazed),
        None,
        &mut rng,
        None,
    );
    let ink = player
        .relics
        .iter()
        .find(|r| r.id == RelicId::InkBottle)
        .map(|r| r.counter)
        .unwrap_or(-1);
    assert_eq!(ink, 9, "unplayable Dazed must not tick InkBottle");
    assert_eq!(
        player.draw.iter().filter(|c| c.id == CardId::Dazed).count(),
        0,
        "Hex must not fire on unplayable Dazed autoplay"
    );
    assert_eq!(
        player.discard.iter().filter(|c| c.id == CardId::Dazed).count(),
        1,
        "Dazed autoplay still discards via UseCardAction"
    );
    assert!(
        !player.discard.iter().any(|c| c.id == CardId::Dazed && c.free_to_play_once),
        "UseCardAction clears freeToPlayOnce so All For One does not retrieve Dazed"
    );
}

#[test]
fn storm_channels_after_the_power_applies() {
    // 169: Defragment +1 Focus then Storm channels Lightning, evoking Frost at 6 not 5.
    let cfg = default_config(Character::Defect, "169", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 111,
                "169 still fails at Storm+Focus last_ok={} want > 111: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn act1_boss_still_rolls_potion_chance() {
    // 169: Hexaghost addPotionToRewards uses 40+blizzard (MonsterRoomBoss
    // instanceof MonsterRoom). Rust forced chance 0, skipped SteroidPotion,
    // then Choose 0 claimed CARD instead of the potion.
    let cfg = default_config(Character::Defect, "169", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 335,
                "169 still fails at Hex Dazed timing last_ok={} want > 335: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn colorless_potion_discovery_matches_src_pool() {
    // 28: ColorlessPotion DiscoveryAction reads srcColorlessCardPool. Rust shuffled
    // colorlessCardPool in place for returnColorlessCard, then reversed that.
    let cfg = default_config(Character::Defect, "28", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 101,
                "28 still fails at Colorless discovery last_ok={} want > 101: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_shuffles_without_in_flight_cards() {
    // 38: Dualcast is still in limbo when the next PlayTopCard shuffles.
    let cfg = default_config(Character::Defect, "38", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 92,
                "38 still fails at Distilled Chaos last_ok={} want > 92: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn note_for_yourself_leave_stays_in_event() {
    // 49: NoteForYourself Leave must not consume the COMPLETE click as a map node.
    let cfg = default_config(Character::Defect, "49", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 75,
                "49 still fails at NoteForYourself last_ok={} want > 75: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn monster_weak_vulnerable_floors_once() {
    // 34: SlaverRed Weak * player Vulnerable must chain then floor (14, not 13).
    let cfg = default_config(Character::Defect, "34", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 66,
                "34 still fails at Weak+Vulnerable last_ok={} want > 66: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn golden_shrine_pray_is_100_gold_on_a0() {
    // 18: GoldShrine pray is 100 below A15 (rust had hardcoded 50).
    let cfg = default_config(Character::Defect, "18", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 21,
                "18 still fails at Golden Shrine last_ok={} want > 21: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn bird_faced_urn_heals_on_power() {
    // 12: Dualcast/Defragment/Electrodynamics with Bird Faced Urn; rust skipped the +2 heal.
    let cfg = default_config(Character::Defect, "12", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 109,
                "12 still fails at Bird Faced Urn last_ok={} want > 109: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn shuriken_gives_strength_every_third_attack() {
    // 20: Shuriken leftover Strength 1, Melter hits SlaverBlue 11 not 12.
    let cfg = default_config(Character::Defect, "20", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 107,
                "20 still fails at Shuriken last_ok={} want > 107: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn potion_belt_adds_two_slots() {
    // 41: Potion Belt onEquip +2 slots; Fruit Juice in slot 3 is +5 HP at the shop.
    let cfg = default_config(Character::Defect, "41", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 138,
                "41 still fails at Potion Belt last_ok={} want > 138: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn lagavulin_siphon_keeps_negative_strength() {
    // 713578: Lagavulin move 1 applies Strength -1; rust used to drop amount<=0 at EOT.
    let cfg = default_config(Character::Defect, "713578", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 117,
                "713578 still fails at Lagavulin siphon last_ok={} want > 117: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn gfte_weak_snapshots_intent_before_mode_shift() {
    // 54: Blind.use applies Weak 2 (Java WeakPower). Without it Guardian
    // ROLL_ATTACK 9 vs 8 frost is 1 HP; Weak 9*0.75=6 is fully blocked.
    // ForTheEyesAction snapshots getIntentBaseDmg before Mode Shift.
    let cfg = default_config(Character::Defect, "54", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 152,
                "54 still fails at GftE Weak last_ok={} want > 152: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn speed_potion_is_combat_only() {
    // 357: SpeedPotion is COMBAT-only (canUse). LoseDexterityPower.atEndOfTurn
    // is ApplyPowerAction(Dexterity, -amount), so Ancient Potion Artifact 1
    // eats the -5 and Glacier+ stays 15 (10 + Dex 5).
    let cfg = default_config(Character::Defect, "357", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "357 still fails at Speed Potion last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn eot_orbs_killing_skip_burn_autoplay() {
    // 968: EOT lightning kills Hexaghost; cleanCardQueue drops queued Burn
    // autoplays. Playing them anyway Fairy-revives (22 vs 8).
    let cfg = default_config(Character::Defect, "968", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 164,
                "968 still fails at EOT Burn after Hexaghost death last_ok={} want > 164: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn jax_loses_hp_and_gains_strength() {
    // 32: JAX.use LoseHP 3 then Strength 2 (seq 207 65 vs 62, no Strength).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 207,
                "32 still fails at JAX last_ok={} want > 207: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn nuclear_battery_channels_plasma_at_prebattle() {
    // 32: NuclearBattery.atPreBattle channels Plasma after Cracked Core.
    // Without it turn-1 Cold Snap does not evoke Lightning (Spheric 15 vs 18).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 187,
                "32 still fails at Nuclear Battery Plasma last_ok={} want > 187: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_twin_slam_then_mode_shift() {
    // 32: Twin Slam queues ChangeState Offensive then 8x2. ApplyPower(Mode
    // Shift) / Reset Threshold are addToBottom of ChangeState, so player
    // Thorns 3x2 do not stick on the new Mode Shift. Rust applied Mode Shift
    // first (24 leftover, Sweeping Beam hit 20 block: 97 vs 91).
    let cfg = default_config(Character::Defect, "32", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 161,
                "32 still fails at Guardian Twin Slam/Mode Shift last_ok={} want > 161: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn tiny_house_opens_combat_reward() {
    // 906: TinyHouse.onEquip increaseMaxHp(5) and CombatRewardScreen.open
    // (gold 50, miscRng potion, CARD). Rust stayed on BossRelic at 36 HP.
    let cfg = default_config(Character::Defect, "906", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 160,
                "906 still fails at Tiny House last_ok={} want > 160: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn explosive_potion_triggers_gremlin_horn() {
    // 773: ExplosivePotion DamageAllEnemiesAction kills AcidSlime_M.
    // GremlinHorn.onMonsterDeath addToBot Draw+Energy while the other M lives
    // (hand 6 with Strike_B, energy 4). Rust dealt the 10 and skipped Horn.
    let cfg = default_config(Character::Defect, "773", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 160,
                "773 still fails at Explosive/Gremlin Horn last_ok={} want > 160: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn duplication_tempest_keeps_energy_on_use() {
    // 991: DuplicationPower queues CardQueueItem(tmp, m, card.energyOnUse).
    // The copy channels X=3 Lightning after the original spent the energy.
    let cfg = default_config(Character::Defect, "991", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "991 still fails at Duplication Tempest last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_mode_shift_block_applies_before_next_card() {
    // 149: Fire/Explosive trip Mode Shift; GainBlock 20 is addToBottom of that
    // potion DamageAction. Sweeping Beam must hit the 20 block (200 vs 194).
    let cfg = default_config(Character::Defect, "149", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "149 still fails at Guardian Mode Shift block last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn static_discharge_channels_before_next_divider_hit() {
    // 389: StaticDischargePower.onAttacked addToTop(ChannelAction). Frost
    // evoke block lands before remaining Hexaghost Divider hits (56 vs 41).
    let cfg = default_config(Character::Defect, "389", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 153,
                "389 still fails at Static Discharge/Divider last_ok={} want > 153: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn empty_cage_opens_purge_grid_once() {
    // 723: EmptyCage.onEquip opens a 2-card purge GRID. Staying on BossRelic
    // treated the next Choose as another relic pick (two Empty Cages).
    let cfg = default_config(Character::Defect, "723", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 148,
                "723 still fails at Empty Cage last_ok={} want > 148: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn regen_skips_when_eot_orbs_end_combat() {
    // 714: RegenPower.atEndOfTurn is AbstractRoom.endTurn after orbs.
    // Lethal EOT lightning skips RegenAction (Java hp 14 Regen 2, rust ticked).
    let cfg = default_config(Character::Defect, "714", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 142,
                "714 still fails at EOT Regen last_ok={} want > 142: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sharp_hide_hits_after_lethal_attack() {
    // 723: SharpHide onUseCard queues THORNS before Channel/evoke resolve.
    // Ball Lightning+ evoke killed Guardian; rust skipped hide (block 6 vs 3).
    let cfg = default_config(Character::Defect, "723", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 141,
                "723 still fails at Sharp Hide last_ok={} want > 141: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_drops_wait_after_lethal() {
    // 211: DistilledChaosPotion queues PlayTopCardAction (WAIT). After a
    // lethal top card, clearPostCombatActions drops the rest so Glacier
    // never grants 7 block.
    let cfg = default_config(Character::Defect, "211", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 141,
                "211 still fails at Distilled Chaos last_ok={} want > 141: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn rainbow_upgrade_stops_exhaust() {
    // 935: Java Rainbow.upgrade sets exhaust=false (cost stays 2). Rust kept
    // exhaust so EndTurn shuffled Rainbow into draw vs exhaust pile.
    let cfg = default_config(Character::Defect, "935", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 143,
                "935 still fails at Rainbow+ exhaust last_ok={} want > 143: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn blizzard_applies_strength_with_zero_frost() {
    // 840: Blizzard.use calculateCardDamage after baseDamage = frostCount*2;
    // frostCount 0 still gets Strength 2 (Java SlaverRed 18, rust skipped the hit).
    let cfg = default_config(Character::Defect, "840", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 140,
                "840 still fails at Blizzard last_ok={} want > 140: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn chrysalis_shuffles_zero_cost_skills_into_draw() {
    // 63: Chrysalis use() rolls 3 SKILLs then queues random-spot inserts
    // (interleaving pick/insert drew Steam Power). Calipers loseBlock(15).
    let cfg = default_config(Character::Defect, "63", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 176,
                "63 still fails at Chrysalis/Calipers last_ok={} want > 176: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mind_blast_damage_is_draw_pile_size() {
    // 543: Mind Blast applyPowers baseDamage = drawPile.size(); rust dealt 0
    // so Looter lived (18) while Java ended combat.
    let cfg = default_config(Character::Defect, "543", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 137,
                "543 still fails at Mind Blast last_ok={} want > 137: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn apotheosis_keeps_gash_combat_bonus() {
    // 270: GashAction +2 this combat, then Apotheosis upgradeDamage(2)
    // on the mutated base. Catalog overwrite reset Gash+ to 5 so
    // Hexaghost took 5 (hp 158) instead of 7 (hp 156).
    let cfg = default_config(Character::Defect, "270", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 177,
                "270 still fails at Gash+/Apotheosis last_ok={} want > 177: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn master_of_strategy_draws_and_exhausts() {
    // 193: Master of Strategy DrawCardAction(3) + exhaust. Rust discarded
    // it and skipped the draws (hand 4 vs 7).
    let cfg = default_config(Character::Defect, "193", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 175,
                "193 still fails at Master of Strategy last_ok={} want > 175: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn abacus_block_on_empty_deck_shuffle() {
    // 443: EmptyDeckShuffleAction during turn-start draw fires Abacus
    // GainBlock 6 after loseBlock (block 6 vs 0). draw_cards_rng shuffled
    // without onShuffle.
    let cfg = default_config(Character::Defect, "443", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "443 still fails at Abacus shuffle block last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn astrolabe_transforms_three_cards() {
    // 133: Astrolabe.onEquip GRID 3, transformCard(c, true, miscRng).
    // Choose 13/11/7 = Ball Lightning, Melter, Compile Driver → Gash+/White
    // Noise+/Steam+. Missing the GRID left the old deck.
    let cfg = default_config(Character::Defect, "133", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "133 still fails at Astrolabe last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn astrolabe_transforms_in_selection_order() {
    // 45 selects Skim, Scrape, then Hologram. Java rolls replacements in that
    // click order; sorting deck indices changes the temporarily excluded card
    // and resolves the second identical miscRng roll to Skim, not White Noise.
    let cfg = default_config(Character::Defect, "45", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 184,
                "45 still fails at Astrolabe selection order last_ok={} want > 184: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn maw_bank_gold_on_boss_treasure_enter() {
    // 683: MawBank.onEnterRoom TreasureRoomBoss +12 after boss rewards Proceed.
    let cfg = default_config(Character::Defect, "683", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "683 still fails at MawBank boss chest last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn duplication_does_not_leave_original_free() {
    // 423: DuplicationPotion copy is a separate CardQueueItem with
    // freeToPlayOnce; the original Zap is discarded without it. Scrape then
    // discards the redrawn Zap (cost 1). Rust kept Zap in hand.
    let cfg = default_config(Character::Defect, "423", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 172,
                "423 still fails at Duplication/Scrape Zap last_ok={} want > 172: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn seek_upgraded_grid_picks_two() {
    // 96: Apotheosis upgrades Seek to magic 2. BetterDrawPileToHandAction
    // GRID stays open after the first Choose (Sweeping Beam); second is
    // Melter. Rust closed after one pick (hand 3 vs 2).
    let cfg = default_config(Character::Defect, "96", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 173,
                "96 still fails at Seek+ GRID last_ok={} want > 173: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn trip_applies_vulnerable() {
    // 496: Trip.use Vulnerable 2 on Guardian. Ball Lightning then hits 7*1.5
    // plus Lightning evoke 8 (214 not 217).
    let cfg = default_config(Character::Defect, "496", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 171,
                "496 still fails at Trip Vulnerable last_ok={} want > 171: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn philosophers_stone_gives_enemies_strength() {
    // 645: PhilosopherStone.atBattleStart Strength 1 on Looter+Mugger
    // (intent 11 not 10). Missing it, EndTurn is 60 HP vs Java 58.
    let cfg = default_config(Character::Defect, "645", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "645 still fails at Philosopher's Stone last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn frozen_core_channels_frost_on_empty_eot() {
    // 870: FrozenCore.onPlayerEndTurn channels Frost if hasEmptyOrb, before
    // TriggerEndOfTurnOrbsAction. Missing it, Parasite 6x2 is 12 not 10.
    let cfg = default_config(Character::Defect, "870", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "870 still fails at Frozen Core EOT Frost last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn secret_weapon_opens_attack_from_deck_grid() {
    // 509: Secret Weapon AttackFromDeckToHandAction GRID; Choose 1 is Cold Snap,
    // exhaust. Rust discarded Secret Weapon and never pulled the attack.
    let cfg = default_config(Character::Defect, "509", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 169,
                "509 still fails at Secret Weapon last_ok={} want > 169: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn secret_technique_opens_skill_from_deck_grid() {
    // 806: Secret Technique GRID of skills; Choose 3 is Glacier, exhaust.
    let cfg = default_config(Character::Defect, "806", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 146,
                "806 still fails at Secret Technique last_ok={} want > 146: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn forethought_puts_card_on_bottom_of_draw() {
    // 277: Colorless Forethought HAND_SELECT; Choose 6 then Proceed.
    let cfg = default_config(Character::Defect, "277", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 135,
                "277 still fails at Forethought last_ok={} want > 135: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn unceasing_top_draws_on_empty_hand() {
    // 114: last card of the turn empties the hand; Top draws Strike_B.
    let cfg = default_config(Character::Defect, "114", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 147,
                "114 still fails at Unceasing Top last_ok={} want > 147: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn stone_calendar_hits_all_on_turn_seven() {
    // 342: Stone Calendar counter 7 deals 52 THORNS (Guardian 97 vs 45).
    let cfg = default_config(Character::Defect, "342", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 147,
                "342 still fails at Stone Calendar last_ok={} want > 147: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ornithopter_defers_heal_on_gambling_overlay() {
    // 287: Gambler's Brew HAND_SELECT snapshot is still pre-Ornithopter heal.
    let cfg = default_config(Character::Defect, "287", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 148,
                "287 still fails at Ornithopter gambling heal last_ok={} want > 148: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn distilled_chaos_hologram_opens_discard_grid() {
    // 610: Distilled Chaos autoplays Hologram; GRID must open so Choose 1
    // returns Cold Snap and exhausts Hologram.
    let cfg = default_config(Character::Defect, "610", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 111,
                "610 still fails at Distilled Chaos Hologram last_ok={} want > 111: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn creative_ai_rolls_before_loop_lightning() {
    // 937: CreativeAI atStartOfTurn RNG is immediate; Loop only queues
    // LightningOrbPassiveAction. Rust Loop roll stole the POWER pick.
    let cfg = default_config(Character::Defect, "937", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 121,
                "937 still fails at CreativeAI/Loop RNG last_ok={} want > 121: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn purity_opens_exhaust_hand_select() {
    // 241: Colorless Potion Purity must HAND_SELECT (Choose 2 is Strike).
    let cfg = default_config(Character::Defect, "241", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 122,
                "241 still fails at Purity HAND_SELECT last_ok={} want > 122: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn toy_ornithopter_heals_after_discovery_overlay() {
    // 45: Colorless/typed potion DiscoveryAction is on the queue before
    // ToyOrnithopter HealAction, so the CARD_REWARD snapshot is still 56 HP.
    let cfg = default_config(Character::Defect, "45", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 127,
                "45 still fails at Ornithopter discovery heal last_ok={} want > 127: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn gremlin_horn_draws_when_eot_orbs_kill() {
    // 906: Electro lightning kills AcidSlime_M at EOT; Gremlin Horn
    // DrawCardAction resolves before DiscardAtEndOfTurn. SlimeBoss slam
    // must not overwrite a thorns-triggered SPLIT (last_ok 97 → split).
    let cfg = default_config(Character::Defect, "906", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 139,
                "906 still fails at Horn/SlimeBoss split last_ok={} want > 139: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn odd_mushroom_cuts_vulnerable_to_25_percent() {
    // 89/958: FungiBeast + Odd Mushroom. Vulnerable is 1.25× not 1.5×
    // (hp 40 vs rust 38 / 56 vs 54 after EndTurn).
    let cfg = default_config(Character::Defect, "89", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 128,
                "89 still fails at Odd Mushroom last_ok={} want > 128: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn guardian_thorns_mode_shift_beats_vent_steam() {
    // 275: Fierce Bash setMove(Vent Steam) before DamageAction resolves.
    // Player Thorns 3 crosses Mode Shift; CLOSE_UP must stick (Sharp Hide,
    // no Weak/Vuln). Rust overwrote CLOSE_UP (hp 45 vs 42, Guardian 178 vs 173).
    let cfg = default_config(Character::Defect, "275", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 168,
                "275 still fails at Fierce Bash Mode Shift last_ok={} want > 168: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ornamental_fan_block_before_sharp_hide() {
    // 872: UseCardAction ctor relics-then-monster-powers after card.use.
    // Fan GainBlock 4 then Sharp Hide 3 (hp 58 block 1, not 55/4).
    let cfg = default_config(Character::Defect, "872", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 166,
                "872 still fails at Fan vs Sharp Hide last_ok={} want > 166: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mayhem_plays_top_card_at_turn_start() {
    // 533: MayhemPower.atStartOfTurn queues PlayTopCard after DrawCardAction.
    // Missing the power left Genetic Algorithm in the deck (hand/block diverge).
    let cfg = default_config(Character::Defect, "533", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 168,
                "533 still fails at Mayhem last_ok={} want > 168: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn incense_burner_intangible_cuts_sharp_hide() {
    // 604: Incense Burner Intangible 1; Guardian Sharp Hide 3. Compile Driver
    // THORNS is 1 not 3 (Java hp 43 vs rust 41 at seq 166).
    let cfg = default_config(Character::Defect, "604", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 165,
                "604 still fails at Intangible Sharp Hide last_ok={} want > 165: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn incense_burner_applies_intangible_every_six_turns() {
    // 493: Incense Burner counter ticks atTurnStart; at 6 apply Intangible
    // so FungiBeast hits for 1 not 9 (hp 24 vs rust 16).
    let cfg = default_config(Character::Defect, "493", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 112,
                "493 still fails at Incense Burner last_ok={} want > 112: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sphere_and_two_shapes_uses_independent_ancient_shape_rolls() {
    // 4: Act 3 strong hallway "Sphere and 2 Shapes" rolls Spiker then
    // Repulsor independently via miscRng before constructing the Guardian.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 661,
                "4 still fails at Sphere and 2 Shapes last_ok={} want > 661: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn donu_and_deca_boss_uses_paired_alternating_moves() {
    // 4: the Act 3 boss is Deca then Donu. Both start with Artifact; Deca
    // opens with Beam while Donu opens with Circle of Protection.
    let cfg = default_config(Character::Defect, "4", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 796,
                "4 still fails at Donu and Deca last_ok={} want > 796: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn deferred_ftl_uses_pre_relic_damage_snapshot() {
    // 539: FTLAction's DamageInfo is built while Pen Nib is at 8. Sharp Hide
    // resolves first, but Pen Nib 9 is applied only after FTL's 9 damage.
    let cfg = default_config(Character::Defect, "539", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 163,
                "539 still fails at deferred FTL/Pen Nib last_ok={} want > 163: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn duplicated_overclock_discards_original_between_burns() {
    // 123: DuplicationPower's copied Overclock is a later CardQueueItem.
    // Original Burn, original Steam Power discard, then copied Burn is the
    // input order for the next shuffle (Burn is drawn, not Steam Power).
    let cfg = default_config(Character::Defect, "123", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 206,
                "123 still fails at Duplication/Overclock queue order last_ok={} want > 206: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn astrolabe_transforms_colorless_from_colorless_pool() {
    // 31: Astrolabe selects Defragment, Cold Snap, then colorless Hand of
    // Greed. The third transform uses srcColorlessCardPool and yields upgraded
    // Sadistic Nature; the colored pool incorrectly yielded Skim.
    let cfg = default_config(Character::Defect, "31", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 206,
                "31 still fails at Astrolabe colorless transform last_ok={} want > 206: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn all_for_one_returns_cards_after_ink_bottle_draw() {
    // 157: All For One is played with Ink Bottle at 9. Java draws Defend,
    // discards All For One, then returns Go for the Eyes and Boot Sequence.
    let cfg = default_config(Character::Defect, "157", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 207,
                "157 still fails at All For One/Ink Bottle queue order last_ok={} want > 207: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn zero_damage_lightning_still_advances_card_random_rng() {
    // 595: Bias leaves Lightning passives at zero. Java still selects one
    // target per passive; four such selections determine which slime the
    // later Cold Snap channel hits when it evokes Lightning.
    let cfg = default_config(Character::Defect, "595", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 207,
                "595 still fails at zero-damage Lightning RNG last_ok={} want > 207: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn lethal_sweeping_beam_drops_draw_and_stays_in_limbo_on_player_death() {
    // 158: Sweeping Beam kills Guardian, clearing its queued draw. Sharp Hide
    // then kills the player before UseCardAction discards the played card.
    let cfg = default_config(Character::Defect, "158", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 208,
                "158 still fails at lethal Sweeping Beam queue cleanup last_ok={} want > 208: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn hex_inserts_dazed_before_ink_bottle_draws() {
    // 444: Coolheaded triggers both Hex and Ink Bottle. Player powers run
    // before relics, so Dazed enters the six-card draw pile before Ink draws.
    let cfg = default_config(Character::Defect, "444", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 211,
                "444 still fails at Hex/Ink Bottle order last_ok={} want > 211: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn fusion_hammer_increases_master_energy_on_equip() {
    // 178: Fusion Hammer supplies the fourth energy in Act 2. After Biased
    // Cognition and Ball Lightning, Tempest still channels twice and evokes
    // both nine-block Frost orbs.
    let cfg = default_config(Character::Defect, "178", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 212,
                "178 still fails at Fusion Hammer energy last_ok={} want > 212: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ectoplasm_consumes_gold_rewards_without_adding_gold() {
    // 465: the first Act 2 combat reward is claimed while Ectoplasm is owned.
    // Java removes GOLD(13) but leaves gold at 149.
    let cfg = default_config(Character::Defect, "465", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 214,
                "465 still gains gold through Ectoplasm last_ok={} want > 214: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn velvet_choker_increases_master_energy_on_equip() {
    // 233: Velvet Choker supplies the fourth energy in Act 2, leaving two
    // energy for Tempest after Zap and Ball Lightning and evoking twice.
    let cfg = default_config(Character::Defect, "233", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 215,
                "233 still misses Velvet Choker energy last_ok={} want > 215: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn jack_of_all_trades_exhausts_and_creates_a_colorless_card() {
    // 68: Jack of All Trades rolls Madness from the non-healing source
    // colorless pool, adds it to hand, and exhausts itself.
    let cfg = default_config(Character::Defect, "68", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 222,
                "68 still misses Jack of All Trades last_ok={} want > 222: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn seek_resolves_hex_after_draw_pile_selection() {
    // 96: Seek removes Coolheaded before Hex inserts its Dazed. Inserting the
    // Dazed before the GRID changes its random position and the next turn's hand.
    let cfg = default_config(Character::Defect, "96", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 230,
                "96 still resolves Hex before Seek GRID last_ok={} want > 230: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn cauldron_opens_five_potion_rewards_on_equip() {
    // 883: buying Cauldron at the Act 2 shop opens five potion rewards before
    // the driver proceeds to the next map node. Its reward-screen open also
    // burns the automatic CARD roll before removing that reward, keeping the
    // following combat reward aligned.
    let cfg = default_config(Character::Defect, "883", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 249,
                "883 still misses Cauldron reward RNG last_ok={} want > 249: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn orrery_opens_five_independent_card_rewards_on_equip() {
    // 163: Orrery creates five eagerly rolled CARD reward items. Choosing the
    // first opens Bullseye/Capacitor/Claw+ without buying the first shop card.
    let cfg = default_config(Character::Defect, "163", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 253,
                "163 still misses Orrery card rewards last_ok={} want > 253: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn designer_grid_returns_to_done_screen() {
    // 69: Designer enters DONE before its upgrade GRID opens. Closing the GRID
    // must expose Leave immediately so the following map click enters floor 22.
    let cfg = default_config(Character::Defect, "69", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 254,
                "69 still rewinds Designer after GRID last_ok={} want > 254: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_colorless_potion_discovery_copies() {
    // 15: Sacred Bark makes Colorless Potion create two copies of the chosen
    // Jack of All Trades instead of one.
    let cfg = default_config(Character::Defect, "15", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 238,
                "15 still misses Sacred Bark potion potency last_ok={} want > 238: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ectoplasm_increases_master_energy_on_equip() {
    // 930: Ectoplasm supplies the fourth energy, so Tempest channels three
    // Lightning orbs after Defend and Beam Cell instead of two.
    let cfg = default_config(Character::Defect, "930", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 240,
                "930 still misses Ectoplasm energy last_ok={} want > 240: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_block_potion() {
    // 781: Sacred Bark doubles Block Potion from 12 to 24 block.
    let cfg = default_config(Character::Defect, "781", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 245,
                "781 still misses doubled Block Potion last_ok={} want > 245: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn slavers_collar_temporarily_increases_energy_in_elite_combat() {
    // 382: Slaver's Collar supplies a fourth energy in the Slavers elite
    // fight. After Defragment, Multi-Cast therefore evokes Lightning three
    // times and kills both outer slavers.
    let cfg = default_config(Character::Defect, "382", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 247,
                "382 still misses Slaver's Collar energy last_ok={} want > 247: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mugger_death_advances_ai_rng_before_later_monster_rolls() {
    // 359: a passive Lightning hit kills Mugger before Looter acts. Mugger's
    // death SFX consumes aiRng.random(2), changing Looter's post-attack move
    // from Lunge to Smoke Bomb.
    let cfg = default_config(Character::Defect, "359", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 258,
                "359 still misses Mugger death AI RNG last_ok={} want > 258: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_focus_flex_and_explosive_potions() {
    // 280: Entropic Brew creates Focus, Flex, and Explosive potions. Sacred
    // Bark doubles their potency to 4 Focus, 10 temporary Strength, and 20
    // damage respectively.
    let cfg = default_config(Character::Defect, "280", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 262,
                "280 still misses Sacred Bark potion potency last_ok={} want > 262: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_strength_and_dexterity_potions() {
    // 280 later drinks Strength and Dexterity potions before the Slavers.
    // AbstractPotion.getPotency doubles both from 2 to 4 with Sacred Bark.
    let cfg = default_config(Character::Defect, "280", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 319,
                "280 still misses Sacred Bark stat potion potency last_ok={} want > 319: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn colosseum_starts_two_slaver_event_fight() {
    // 369: Colosseum's second button starts the event-only Blue + Red Slaver
    // encounter rather than leaving for the map.
    let cfg = default_config(Character::Defect, "369", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 264,
                "369 still misses Colosseum Slavers last_ok={} want > 264: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn ectoplasm_blocks_claimed_stolen_gold() {
    // 930: killing Looter and Mugger creates a 60-gold stolen reward. Claiming
    // it removes the reward, but Ectoplasm keeps player gold unchanged.
    let cfg = default_config(Character::Defect, "930", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 270,
                "930 still gains stolen gold with Ectoplasm last_ok={} want > 270: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_fruit_juice() {
    // 729: Fruit Juice is used from the combat reward screen. Sacred Bark
    // doubles its max/current HP increase from 5 to 10.
    let cfg = default_config(Character::Defect, "729", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 271,
                "729 still misses doubled Fruit Juice last_ok={} want > 271: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn seek_grid_sorts_upgraded_duplicate_by_display_name() {
    // 348: the draw pile contains Defragment and Defragment+. Java's grid
    // sorts by displayed name, so Choose 2 must take the upgraded copy.
    let cfg = default_config(Character::Defect, "348", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 272,
                "348 still selects the wrong Defragment copy last_ok={} want > 272: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn toolbox_choice_resolves_before_the_opening_draw() {
    // 600: Toolbox rolls Swift Strike / Trip / Magnetism (four cardRandomRng
    // calls after one duplicate), adds Swift Strike, then draws five cards.
    let cfg = default_config(Character::Defect, "600", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 274,
                "600 still skips the Toolbox pre-draw reward last_ok={} want > 274: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn tempest_hex_inserts_dazed_before_nested_channel_actions() {
    // 75: TempestAction queues its three ChannelActions after UseCardAction's
    // Hex reaction. Dazed insertion therefore consumes cardRandomRng before
    // the full orb slots evoke Lightning into Byrd.
    let cfg = default_config(Character::Defect, "75", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 278,
                "75 still resolves Tempest before Hex last_ok={} want > 278: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_energy_potion() {
    // 197: Sacred Bark doubles Energy Potion from 2 to 4 energy. The extra
    // two energy remain for Reinforced Body after three preceding cards.
    let cfg = default_config(Character::Defect, "197", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 283,
                "197 still misses doubled Energy Potion last_ok={} want > 283: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn lethal_life_suck_does_not_resolve_queued_heal() {
    // 283: Shelled Parasite's Life Suck kills the player for 8 after Frost
    // block. Java stops before its queued HealAction, leaving the Parasite at
    // 15 HP instead of healing it to 23.
    let cfg = default_config(Character::Defect, "283", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 290,
                "283 still heals after lethal Life Suck last_ok={} want > 290: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn mercury_hourglass_resolves_before_loop_lightning() {
    // 742: Byrd has 3 HP at turn start. Mercury Hourglass kills it before
    // Loop selects a random target, forcing Loop's Lightning into Chosen.
    let cfg = default_config(Character::Defect, "742", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 293,
                "742 still resolves Loop before Hourglass last_ok={} want > 293: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn hex_inserts_dazed_before_multicast_evokes() {
    // 722: Multi-Cast's wrapper queues its evoke actions behind Hex. Inserting
    // Dazed after those Lightning target rolls changes the hidden draw order.
    let cfg = default_config(Character::Defect, "722", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 296,
                "722 still resolves Multi-Cast before Hex last_ok={} want > 296: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn life_suck_heals_before_queued_thorns() {
    // 59: Shelled Parasite starts Life Suck at full HP. Its queued heal is a
    // no-op before Bronze Scales deals 3, so the reflected damage must remain.
    let cfg = default_config(Character::Defect, "59", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 297,
                "59 still heals Life Suck after Thorns last_ok={} want > 297: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn loop_frost_block_survives_lethal_hourglass() {
    // 742: Hourglass kills the final monster, but Java's post-combat clear
    // preserves Loop's queued GainBlockAction from the front Frost orb.
    let cfg = default_config(Character::Defect, "742", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 303,
                "742 still drops Loop Frost block last_ok={} want > 303: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_fairy_potion_revive() {
    // 987: Fairy in a Bottle revives for 60% with Sacred Bark, then one
    // remaining Snake Plant hit resolves before the next command boundary.
    let cfg = default_config(Character::Defect, "987", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 303,
                "987 still uses base Fairy potency last_ok={} want > 303: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn black_star_adds_second_elite_relic_reward() {
    // 641: after taking Happy Flower, the next reward is Black Star's second
    // relic (Question Card), not the card reward overlay.
    let cfg = default_config(Character::Defect, "641", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 305,
                "641 still omits Black Star reward last_ok={} want > 305: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn hand_of_greed_does_not_reward_gremlin_leader_minions() {
    // 43: Hand of Greed kills a Gremlin Leader minion, which has MinionPower
    // in Java and therefore must not award gold.
    let cfg = default_config(Character::Defect, "43", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 306,
                "43 still rewards gold for a minion last_ok={} want > 306: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_speed_potion() {
    // 43: Speed Potion with Sacred Bark applies 10 Dexterity and 10 matching
    // LoseDexterity, so the following Glacier gains 5 more block.
    let cfg = default_config(Character::Defect, "43", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 308,
                "43 still uses base Speed Potion potency last_ok={} want > 308: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn orrery_uses_shop_room_card_rarity_odds() {
    // 43: Orrery's fifth reward is rolled while the current room is ShopRoom.
    // Adjusted roll 40 is uncommon under ShopRoom's 9/37 odds, producing FTL.
    let cfg = default_config(Character::Defect, "43", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 347,
                "43 still uses hallway rarity odds in the shop last_ok={} want > 347: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn upgraded_chaos_rolls_all_orbs_before_channel_actions() {
    // 903: upgraded Chaos rolls Dark then Frost before the first channel
    // evokes Lightning and consumes cardRandomRng for its target.
    let cfg = default_config(Character::Defect, "903", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 306,
                "903 still interleaves Chaos rolls and channels last_ok={} want > 306: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn monster_regeneration_waits_for_the_enemy_turn_to_finish() {
    // 245: Taskmaster acts first at 32 HP, then Red Slaver kills the player.
    // Java never reaches the group end-of-turn phase that would heal it to 37.
    let cfg = default_config(Character::Defect, "245", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 307,
                "245 still heals a monster before lethal damage last_ok={} want > 307: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_liquid_memories_selection() {
    // 270: Sacred Bark raises Liquid Memories to two cards. The first click
    // selects Compile Driver but leaves GRID open for Glacier as the second.
    let cfg = default_config(Character::Defect, "270", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 308,
                "270 still resolves Liquid Memories after one pick last_ok={} want > 308: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn sacred_bark_doubles_swift_potion_draw() {
    // 696: Swift Potion with Sacred Bark draws six cards, filling the hand
    // from five to ten rather than stopping after the base three cards.
    let cfg = default_config(Character::Defect, "696", Unlocks::fixture(), 0);
    match walk_oracle(&cfg) {
        Ok(_) => {}
        Err(fail) if fail.mismatched == ["io"] => {}
        Err(fail) => {
            assert!(
                fail.last_ok > 309,
                "696 still uses base Swift Potion draw last_ok={} want > 309: {fail}",
                fail.last_ok
            );
        }
    }
}

#[test]
fn regression_is_recorded_not_deleted() {
    let mut reg = GreenRegistry::new();
    reg.record_green("407258", 250, 242, 210390258);
    assert_eq!(reg.green_count(), 1);
    assert!(reg.seeds.contains_key("407258"));

    reg.record_regression("407258", 12, "first mismatch at seq 13 hp");
    assert!(
        reg.seeds.contains_key("407258"),
        "regression must not drop the seed"
    );
    assert_eq!(reg.seeds["407258"].status, GreenStatus::Regression);
    assert_eq!(reg.green_count(), 0);
    assert_eq!(reg.regressions.len(), 1);
    assert_eq!(reg.regressions[0].seed, "407258");
}
