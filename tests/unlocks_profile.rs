use sts_engine::ids::{CardId, EncounterId, RelicId};
use sts_engine::Unlocks;

#[test]
fn fixture_loads_java_profile_fixture() {
    let u = Unlocks::fixture();
    assert!(
        u.boss_seen(EncounterId::Hexaghost)
            && u.boss_seen(EncounterId::SlimeBoss)
            && u.boss_seen(EncounterId::AwakenedOne)
            && u.boss_seen(EncounterId::TimeEater),
        "profile-fixture STSSeenBosses marks all bosses seen, got {:?}",
        u.seen_bosses
    );
    assert!(!u.everything_unlocked);
    assert!(
        u.card_locked(CardId::Core_Surge),
        "Core Surge is not in STSUnlocks as 2, so Java still locks it"
    );
    assert!(
        u.relic_locked(RelicId::The_Courier),
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
    assert!(
        u.boss_seen(EncounterId::AwakenedOne)
            && u.boss_seen(EncounterId::DonuAndDeca)
            && u.boss_seen(EncounterId::TimeEater)
    );
    assert!(!u.card_locked(CardId::Core_Surge));
    assert!(!u.relic_locked(RelicId::The_Courier));
}
