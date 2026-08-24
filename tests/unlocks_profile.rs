use sts_engine::Unlocks;

#[test]
fn fixture_loads_java_profile_fixture() {
    let u = Unlocks::fixture();
    assert!(
        u.boss_seen("GHOST")
            && u.boss_seen("SLIME")
            && u.boss_seen("CROW")
            && u.boss_seen("WIZARD"),
        "profile-fixture STSSeenBosses marks all bosses seen, got {:?}",
        u.seen_bosses
    );
    assert!(!u.everything_unlocked);
    assert!(
        u.card_locked("Core Surge"),
        "Core Surge is not in STSUnlocks as 2, so Java still locks it"
    );
    assert!(
        u.relic_locked("The Courier"),
        "The Courier is not in STSUnlocks as 2"
    );
    assert!(
        u.final_act_available,
        "profile-fixture has IRONCLAD+SILENT+DEFECT WIN"
    );
}

#[test]
fn all_uses_java_boss_pref_keys() {
    let u = Unlocks::all();
    assert!(u.boss_seen("CROW") && u.boss_seen("DONUT") && u.boss_seen("WIZARD"));
    assert!(!u.card_locked("Core Surge"));
    assert!(!u.relic_locked("The Courier"));
}
