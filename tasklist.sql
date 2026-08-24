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
    );
