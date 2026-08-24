use sts_engine::card::Card;
use sts_engine::combat::{end_turn, play_owned_card, Combat};
use sts_engine::creature::{Orb, OrbKind, Player, RelicInstance};
use sts_engine::ids::{CardId, EncounterId, PowerId, RelicId};
use sts_engine::rng::RngSet;

fn combat_fixture(encounter: EncounterId) -> (Player, Combat, RngSet) {
    let mut player = Player::defect();
    let mut rng = RngSet::generate_seeds(2);
    let mut combat = Combat::start(encounter, &mut player, &mut rng, 1, 2, 0);

    player.hand.clear();
    player.draw.clear();
    player.discard.clear();
    player.exhaust.clear();
    player.orbs.clear();
    player.relics.clear();
    player.energy = 10;
    for monster in &mut combat.monsters {
        monster.hp = 500;
        monster.max_hp = 500;
        monster.block = 0;
        monster.intent_damage = 0;
        monster.intent_base_damage = 0;
        monster.intent_hits = 0;
    }

    (player, combat, rng)
}

fn play(
    player: &mut Player,
    combat: &mut Combat,
    rng: &mut RngSet,
    card: Card,
    target: Option<usize>,
) {
    assert!(!play_owned_card(player, combat, card, target, rng, None,));
}

#[test]
fn meteor_strike_deals_damage_and_channels_three_plasma() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::Cultist);
    let card = Card::new(CardId::Meteor_Strike);
    assert_eq!(card.cost, 5);
    assert_eq!(card.base_damage, 24);
    let mut upgraded = card;
    upgraded.upgrade();
    assert_eq!(upgraded.base_damage, 30);

    let hp_before = combat.monsters[0].hp;
    play(&mut player, &mut combat, &mut rng, card, Some(0));

    assert_eq!(combat.monsters[0].hp, hp_before - 24);
    assert_eq!(player.orbs.len(), 3);
    assert!(player.orbs.iter().all(|orb| orb.kind == OrbKind::Plasma));
}

#[test]
fn hyperbeam_damages_every_enemy_and_loses_three_focus() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::CultistAndChosen);
    let card = Card::new(CardId::Hyperbeam);
    assert_eq!(card.cost, 2);
    assert_eq!(card.base_damage, 26);
    assert_eq!(card.base_magic, 3);
    let mut upgraded = card;
    upgraded.upgrade();
    assert_eq!(upgraded.base_damage, 34);

    player.add_power(PowerId::Focus, 2);
    let hp_before: Vec<i32> = combat.monsters.iter().map(|monster| monster.hp).collect();
    play(&mut player, &mut combat, &mut rng, card, None);

    for (monster, hp) in combat.monsters.iter().zip(hp_before) {
        assert_eq!(monster.hp, hp - 26);
    }
    assert_eq!(player.power_amount(PowerId::Focus), -1);
}

#[test]
fn loop_triggers_front_plasma_in_addition_to_normal_and_cables_energy() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::Cultist);
    player.energy_master = 3;
    player.energy = 0;
    player.orbs = vec![
        Orb {
            kind: OrbKind::Plasma,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Plasma,
            evoke: 0,
        },
    ]
    .into();
    player.add_power(PowerId::Loop, 2);
    player.relics.push(RelicInstance {
        id: RelicId::Cables,
        counter: -1,
        used_up: false,
    });

    end_turn(&mut player, &mut combat, &mut rng, None);

    // 3 base + 2 normal Plasma + 1 Cables + 2 Loop triggers.
    assert_eq!(player.energy, 8);
}

#[test]
fn tempest_spends_x_before_its_forced_plasma_evoke() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::Cultist);
    player.energy = 1;
    player.max_orbs = 1;
    player.orbs = vec![Orb {
        kind: OrbKind::Plasma,
        evoke: 0,
    }]
    .into();

    play(
        &mut player,
        &mut combat,
        &mut rng,
        Card::new(CardId::Tempest),
        None,
    );

    assert_eq!(player.energy, 2);
    assert_eq!(player.orbs.len(), 1);
    assert_eq!(player.orbs[0].kind, OrbKind::Lightning);
}

#[test]
fn multi_cast_spends_x_before_evoking_plasma() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::Cultist);
    player.energy = 1;
    player.max_orbs = 1;
    player.orbs = vec![Orb {
        kind: OrbKind::Plasma,
        evoke: 0,
    }]
    .into();

    play(
        &mut player,
        &mut combat,
        &mut rng,
        Card::new(CardId::Multi_Cast),
        None,
    );

    assert_eq!(player.energy, 2);
    assert!(player.orbs.is_empty());
}

#[test]
fn stacked_echo_form_duplicates_the_first_card_for_each_stack_each_turn() {
    let (mut player, mut combat, mut rng) = combat_fixture(EncounterId::Cultist);
    player.add_power(PowerId::EchoForm, 2);
    let starting_hp = combat.monsters[0].hp;

    play(
        &mut player,
        &mut combat,
        &mut rng,
        Card::new(CardId::Strike_B),
        Some(0),
    );
    assert_eq!(combat.monsters[0].hp, starting_hp - 12);
    assert_eq!(combat.echo_cards_duplicated_this_turn, 1);

    play(
        &mut player,
        &mut combat,
        &mut rng,
        Card::new(CardId::Strike_B),
        Some(0),
    );
    assert_eq!(combat.monsters[0].hp, starting_hp - 24);
    assert_eq!(combat.echo_cards_duplicated_this_turn, 2);

    play(
        &mut player,
        &mut combat,
        &mut rng,
        Card::new(CardId::Strike_B),
        Some(0),
    );
    assert_eq!(combat.monsters[0].hp, starting_hp - 30);

    end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.echo_cards_duplicated_this_turn, 0);
}
