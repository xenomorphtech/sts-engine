CREATE TABLE IF NOT EXISTS tasklist (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL,
    details TEXT NOT NULL,
    source_refs TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'open'
        CHECK (status IN ('open', 'in_progress', 'done'))
);

INSERT INTO tasklist (id, title, details, source_refs, status) VALUES
    (
        'engine-audit-meteor-strike',
        'Implement Meteor Strike',
        'Meteor Strike is present in the generated catalog but has no card stats or combat effect, so it deals no damage and channels no Plasma.',
        'src/generated/card_catalog.rs:211; src/content.rs:990; src/combat.rs:6775',
        'done'
    ),
    (
        'engine-audit-hyperbeam',
        'Implement Hyperbeam',
        'Hyperbeam is present in the generated catalog but has neither its all-enemy damage nor its Focus loss implemented.',
        'src/generated/card_catalog.rs:205',
        'done'
    ),
    (
        'engine-audit-loop-plasma',
        'Make Loop trigger front Plasma',
        'Loop triggers the front-orb passive path, but Plasma contributes no Loop energy while normal and Gold-Plated Cables Plasma energy are handled separately.',
        'src/combat.rs:7539; src/combat.rs:7941; src/combat.rs:7678',
        'done'
    ),
    (
        'engine-audit-x-plasma-ordering',
        'Preserve Plasma energy from Tempest and Multi-Cast',
        'Tempest and Multi-Cast currently resolve channels or evocations before setting energy to zero, erasing Plasma energy generated during their own resolution. Match STS action ordering and retain that generated energy.',
        'src/combat.rs:5676; src/combat.rs:5845',
        'done'
    ),
    (
        'engine-audit-echo-form',
        'Implement Echo Form card duplication',
        'Echo Form records a power but does not duplicate the first card or cards played each turn, eliminating double-card queue lines.',
        'src/combat.rs:6063',
        'done'
    ),
    (
        'queue-intelligence-front-orb-scheduling',
        'Value multi-turn front-orb scheduling',
        'Extend ordered-orb continuation value so Loop, Recursion, and future channel cards reward keeping the right orb at the front beyond the next exactly simulated Loop trigger.',
        'src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-plasma-accessibility',
        'Distinguish Plasma accessibility and energy demand',
        'Value Plasma by queue position and release schedule so a front Plasma, a Plasma one channel from eviction, and a buried Plasma are not interchangeable; join that accessibility with next-hand energy demand from Fusion, Recursion, Multi-Cast, Fission+, and Meteor Strike.',
        'src/htn/turnplan.rs:1097; src/htn/turnplan.rs:690',
        'done'
    ),
    (
        'queue-intelligence-capacity-option',
        'Value orb capacity as a queue option',
        'Represent the future safe-channel capacity and delayed-evocation value created by Capacitor, plus the option for Consume to intentionally reduce capacity and accelerate queue cycling; immediate insertions and deletions are already simulated exactly.',
        'src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-multiple-dark-banks',
        'Schedule and protect multiple Dark banks',
        'Assign distinct targets, release schedules, queue-distance value, and protection requirements to every Dark orb produced by Darkness, Doom and Gloom, Rainbow, or Recursion instead of applying target-aware reasoning only to the first bank.',
        'src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-electrodynamics-lightning',
        'Value retained Lightning under Electrodynamics',
        'When Electrodynamics is active, value future Lightning triggers from Zap, Tempest, Storm, and retained Lightning as all-enemy damage against living enemies rather than single-target damage; the next passive trigger is already simulated exactly.',
        'src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-orb-card-joins',
        'Join next-hand cards with the orb queue',
        'Extend post-End Turn hand valuation beyond ordinary printed damage and block so combinations such as front Dark plus Multi-Cast, a diverse row plus Compile Driver, and a full row plus Barrage receive continuation value alongside Fission and Recursion.',
        'src/htn/turnplan.rs:690; src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-persistent-channel-engines',
        'Value persistent channel engines across turns',
        'Add continuation value for future trigger opportunities from Storm and Loop, comparable to the explicit Static Discharge bonus, rather than relying mainly on the single trigger realized by End Turn simulation.',
        'src/htn/turnplan.rs:544; src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-queue-tool-preservation',
        'Preserve queue tools for planned future turns',
        'Reward Hologram, Seek, Rebound, and Equilibrium lines that retain or return Recursion and Multi-Cast for a planned future queue; same-turn fetching is searchable, but Rebound currently gives these queue tools only its minimum fallback value.',
        'src/htn/turnplan.rs:454; src/htn/turnplan.rs:478; src/htn/turnplan.rs:1097',
        'done'
    ),
    (
        'queue-intelligence-multi-turn-dark-growth',
        'Model repeated front-position Dark growth',
        'Represent repeated Dark growth from Darkness+ while the orb remains at the front under Loop or Gold-Plated Cables, beyond the ordinary growth estimate and the one immediate extra trigger simulated exactly.',
        'src/htn/turnplan.rs:1097',
        'done'
    );
