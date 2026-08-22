use sts_engine::action::{Action, PotionOp};
use sts_engine::card::Card;
use sts_engine::combat::{self, Combat};
use sts_engine::creature::{Orb, OrbKind, Player, PotionInstance, RelicInstance};
use sts_engine::game::{Game, NeowDrawback, NeowKind, NeowOption, Screen};
use sts_engine::ids::{Act, CardId, Character, EncounterId, MonsterId, PotionId, PowerId, RelicId, RoomType};
use sts_engine::rng::RngSet;
use sts_engine::Unlocks;

#[test]
fn potion_discard_remains_legal_on_card_reward_screen() {
    let mut game = Game::new(103370126172143121, Character::Defect, 20, Unlocks::fixture());
    game.player.potions[0].id = PotionId::EssenceOfDarkness;
    game.screen = Screen::CardReward;

    assert!(game.legal_actions().contains(&Action::Potion {
        action: PotionOp::Discard,
        slot: 0,
        target_index: None,
    }));
}

#[test]
fn smoke_bomb_can_be_discarded_but_not_used_in_a_boss_fight() {
    for (room, encounter) in [
        (RoomType::Boss, EncounterId::TimeEater),
        (RoomType::Event, EncounterId::TheGuardian),
    ] {
        let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
        game.current_room = room;
        game.player.potions[0].id = PotionId::SmokeBomb;
        game.combat = Some(Combat::start(
            encounter,
            &mut game.player,
            &mut game.rng,
            50,
            game.seed,
            20,
        ));
        game.screen = Screen::Combat;

        let actions = game.legal_actions();
        assert!(!actions.contains(&Action::Potion {
            action: PotionOp::Use,
            slot: 0,
            target_index: None,
        }));
        assert!(actions.contains(&Action::Potion {
            action: PotionOp::Discard,
            slot: 0,
            target_index: None,
        }));
    }
}

#[test]
fn smoke_bomb_suppresses_rewards_when_a_card_kills_before_escape() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Monster;
    game.player.potions[0].id = PotionId::SmokeBomb;
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    let combat = game.combat.as_mut().expect("combat");
    for monster in &mut combat.monsters {
        monster.hp = 0;
        monster.dead = true;
    }
    combat.monsters[0].hp = 1;
    combat.monsters[0].dead = false;
    game.player.hand = vec![Card::new(CardId::Strike_B)];
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    assert!(game.combat.as_ref().is_some_and(|combat| combat.smoked));
    let card_rng_before = game.rng.card.counter;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: Some(0),
    });

    assert_eq!(game.screen, Screen::CombatReward);
    assert!(game.rewards.is_empty());
    assert!(game.card_reward.is_empty());
    assert_eq!(game.rng.card.counter, card_rng_before);
    assert_eq!(game.legal_actions(), vec![Action::Proceed]);
}

#[test]
fn skill_potion_does_not_replace_tempests_x_cost_with_zero() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.player.potions[0].id = PotionId::Skill;
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    game.card_reward = vec![Card::new(CardId::Tempest)];
    game.step(&Action::Choose {
        index: 0,
        label: Some("Tempest".into()),
        x: None,
        y: None,
        room: None,
    });

    let tempest = game
        .player
        .hand
        .iter()
        .find(|card| card.id == CardId::Tempest)
        .expect("generated Tempest in hand");
    assert_eq!(tempest.cost_for_turn, -1);
}

#[test]
fn normality_disables_every_card_after_three_cards_are_played() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    game.player.hand = vec![Card::new(CardId::Normality), Card::new(CardId::Defend_B)];
    game.player.energy = 3;
    game.combat.as_mut().unwrap().cards_played_this_turn = 3;
    game.screen = Screen::Combat;

    let actions = game.legal_actions();
    assert!(!actions.iter().any(|action| matches!(action, Action::Play { .. })));
    assert!(actions.contains(&Action::EndTurn));
}

#[test]
fn letter_opener_mode_shift_block_resolves_before_end_turn_lightning() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Boss;
    game.combat = Some(Combat::start(
        EncounterId::TheGuardian,
        &mut game.player,
        &mut game.rng,
        16,
        game.seed,
        20,
    ));
    game.player.relics.push(RelicInstance {
        id: RelicId::Letter_Opener,
        counter: 2,
        used_up: false,
    });
    game.player.add_power(PowerId::Focus, 2);
    game.player.orbs = vec![Orb {
        kind: OrbKind::Lightning,
        evoke: 0,
    }];
    game.player.hand = vec![Card::new(CardId::Defend_B)];
    game.player.draw.clear();
    game.player.discard.clear();
    game.player.energy = 3;
    game.screen = Screen::Combat;

    let guardian = &mut game.combat.as_mut().unwrap().monsters[0];
    guardian.hp = 207;
    guardian.block = 0;
    guardian.split_triggered = false;
    guardian.stolen_gold = 0;
    guardian
        .powers
        .iter_mut()
        .find(|power| power.id == PowerId::ModeShift)
        .unwrap()
        .amount = 5;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });

    let guardian = &game.combat.as_ref().unwrap().monsters[0];
    assert_eq!(guardian.hp, 202);
    assert_eq!(guardian.block, 20);
    assert!(guardian.split_triggered);

    game.step(&Action::EndTurn);

    assert_eq!(game.combat.as_ref().unwrap().monsters[0].hp, 202);
}

#[test]
fn dualcast_letter_opener_resolves_before_mode_shift_queued_by_second_evoke() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Boss;
    game.combat = Some(Combat::start(
        EncounterId::TheGuardian,
        &mut game.player,
        &mut game.rng,
        16,
        game.seed,
        20,
    ));
    game.player.relics.push(RelicInstance {
        id: RelicId::Letter_Opener,
        counter: 2,
        used_up: false,
    });
    game.player.orbs = vec![
        Orb {
            kind: OrbKind::Lightning,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Lightning,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Lightning,
            evoke: 0,
        },
    ];
    game.player.hand = vec![Card::new(CardId::Dualcast)];
    game.player.draw.clear();
    game.player.discard.clear();
    game.player.energy = 1;
    game.screen = Screen::Combat;

    let guardian = &mut game.combat.as_mut().unwrap().monsters[0];
    guardian.hp = 26;
    guardian.block = 0;
    guardian.split_triggered = false;
    guardian
        .powers
        .iter_mut()
        .find(|power| power.id == PowerId::ModeShift)
        .unwrap()
        .amount = 12;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });

    let guardian = &game.combat.as_ref().unwrap().monsters[0];
    assert_eq!(guardian.hp, 5);
    assert_eq!(guardian.block, 20);
    assert!(guardian.split_triggered);
}

#[test]
fn thinking_ahead_requires_one_hand_choice_then_confirm() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    game.player.hand = vec![
        Card::new(CardId::Thinking_Ahead),
        Card::new(CardId::Defend_B),
        Card::new(CardId::Strike_B),
    ];
    game.player.draw.clear();
    game.player.discard.clear();
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::HandSelect);
    assert!(!game.legal_actions().contains(&Action::Proceed));

    game.step(&Action::Choose {
        index: 0,
        label: Some("Defend_B".into()),
        x: None,
        y: None,
        room: None,
    });
    assert_eq!(game.screen, Screen::HandSelect);
    assert_eq!(game.legal_actions(), [Action::Proceed]);
    assert_eq!(game.player.hand.iter().map(|card| card.id).collect::<Vec<_>>(), [CardId::Strike_B]);
    assert!(game.player.draw.is_empty());

    game.step(&Action::Proceed);

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.player.hand.iter().map(|card| card.id).collect::<Vec<_>>(), [CardId::Strike_B]);
    assert_eq!(game.player.draw.iter().map(|card| card.id).collect::<Vec<_>>(), [CardId::Defend_B]);
}

#[test]
fn reboot_clears_mummified_hand_costs_when_moving_hand_to_draw() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    let mut discounted = Card::new(CardId::Compile_Driver);
    discounted.cost_for_turn = 0;
    let mut discarded_discounted = Card::new(CardId::Compile_Driver);
    discarded_discounted.cost_for_turn = 0;
    game.player.hand = vec![Card::new(CardId::Reboot), discounted];
    game.player.draw.clear();
    game.player.discard = vec![discarded_discounted];
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });

    let compiles = game
        .player
        .hand
        .iter()
        .filter(|card| card.id == CardId::Compile_Driver)
        .collect::<Vec<_>>();
    assert_eq!(compiles.len(), 2);
    assert!(compiles
        .iter()
        .all(|card| card.cost_for_turn == card.cost && card.cost_for_turn == 1));
}

#[test]
fn lethal_compile_driver_cancels_queued_draw_and_abacus_shuffle() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.player.relics.push(RelicInstance {
        id: RelicId::TheAbacus,
        counter: -1,
        used_up: false,
    });
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    let combat = game.combat.as_mut().expect("combat");
    for monster in &mut combat.monsters {
        monster.hp = 0;
        monster.dead = true;
    }
    combat.monsters[0].hp = 1;
    combat.monsters[0].dead = false;
    game.player.orbs = vec![Orb {
        kind: OrbKind::Frost,
        evoke: 0,
    }];
    game.player.hand = vec![Card::new(CardId::Compile_Driver)];
    game.player.draw.clear();
    game.player.discard = vec![Card::new(CardId::Defend_B)];
    game.player.block = 0;
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: Some(0),
    });

    assert_eq!(game.screen, Screen::CombatReward);
    assert_eq!(game.player.block, 0);
    assert!(game.player.draw.is_empty());
}

#[test]
fn lethal_attack_resets_nunchaku_without_granting_queued_energy() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.player.relics.push(RelicInstance {
        id: RelicId::Nunchaku,
        counter: 9,
        used_up: false,
    });
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        1,
        game.seed,
        20,
    ));
    let combat = game.combat.as_mut().expect("combat");
    for monster in &mut combat.monsters {
        monster.hp = 0;
        monster.dead = true;
    }
    combat.monsters[0].hp = 1;
    combat.monsters[0].dead = false;
    game.player.hand = vec![Card::new(CardId::Strike_B)];
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: Some(0),
    });

    assert_eq!(game.screen, Screen::CombatReward);
    assert_eq!(game.player.energy, 2);
    assert_eq!(
        game.player
            .relics
            .iter()
            .find(|relic| relic.id == RelicId::Nunchaku)
            .expect("Nunchaku")
            .counter,
        0
    );
}

#[test]
fn gremlin_horn_draws_when_exploder_dies_during_its_monster_turn() {
    let mut player = Player::defect();
    player.relics.push(RelicInstance {
        id: RelicId::Gremlin_Horn,
        counter: -1,
        used_up: false,
    });
    player.relics.push(RelicInstance {
        id: RelicId::Runic_Pyramid,
        counter: -1,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(17);
    let mut combat = Combat::start(
        EncounterId::TwoLouse,
        &mut player,
        &mut rng,
        1,
        17,
        20,
    );
    combat.monsters[0].id = MonsterId::Exploder;
    combat.monsters[0].hp = 30;
    combat.monsters[0].max_hp = 30;
    combat.monsters[0].dead = false;
    combat.monsters[0].add_power(PowerId::Explosive, 1);
    player.hand.clear();
    player.draw = vec![
        Card::new(CardId::Defend_B),
        Card::new(CardId::Zap),
        Card::new(CardId::Strike_B),
        Card::new(CardId::Cold_Snap),
        Card::new(CardId::Ball_Lightning),
        Card::new(CardId::Dualcast),
    ];
    player.discard.clear();

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert!(combat.monsters[0].dead);
    assert!(combat.monsters[1].alive());
    assert_eq!(player.hand.len(), 6);
    assert!(player.draw.is_empty());
}

#[test]
fn gremlin_horn_does_not_draw_for_large_slime_split_suicide() {
    let mut player = Player::defect();
    player.relics.push(RelicInstance {
        id: RelicId::Gremlin_Horn,
        counter: -1,
        used_up: false,
    });
    player.relics.push(RelicInstance {
        id: RelicId::Runic_Pyramid,
        counter: -1,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(17);
    let mut combat = Combat::start(
        EncounterId::TwoLouse,
        &mut player,
        &mut rng,
        1,
        17,
        20,
    );
    combat.monsters[0].id = MonsterId::AcidSlimeL;
    combat.monsters[0].hp = 31;
    combat.monsters[0].max_hp = 62;
    combat.monsters[0].next_move = 3;
    combat.monsters[0].dead = false;
    combat.monsters[0].powers.clear();
    player.hand.clear();
    player.draw = vec![
        Card::new(CardId::Defend_B),
        Card::new(CardId::Zap),
        Card::new(CardId::Strike_B),
        Card::new(CardId::Cold_Snap),
        Card::new(CardId::Ball_Lightning),
        Card::new(CardId::Dualcast),
    ];
    player.discard.clear();

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert!(combat
        .monsters
        .iter()
        .any(|monster| monster.id == MonsterId::AcidSlimeM));
    assert!(combat.monsters.iter().filter(|monster| monster.alive()).count() >= 2);
    assert_eq!(player.hand.len(), 5);
    assert_eq!(player.draw.len(), 1);
}

#[test]
fn ball_lightning_channels_before_gremlin_leader_minions_escape() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Elite;
    game.combat = Some(Combat::start(
        EncounterId::GremlinLeader,
        &mut game.player,
        &mut game.rng,
        23,
        game.seed,
        20,
    ));
    let combat = game.combat.as_mut().expect("combat");
    let leader = combat
        .monsters
        .iter()
        .position(|monster| monster.id == MonsterId::GremlinLeader)
        .expect("Gremlin Leader");
    combat.monsters[leader].hp = 1;
    combat.monsters[leader].block = 0;
    game.player.orbs = vec![
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
        Orb {
            kind: OrbKind::Dark,
            evoke: 6,
        },
        Orb {
            kind: OrbKind::Frost,
            evoke: 0,
        },
    ];
    game.player.add_power(PowerId::Focus, 3);
    game.player.hand = vec![Card::new(CardId::Ball_Lightning)];
    game.player.block = 0;
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: Some(leader),
    });

    assert_eq!(game.screen, Screen::CombatReward);
    assert_eq!(game.player.block, 8);
}

#[test]
fn skipped_skill_potion_discovery_burns_the_unused_offer_rounds() {
    let seed = 5_053_207_210_280_065_480;
    let mut game = Game::new(seed, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Boss;
    game.combat = Some(Combat::start(
        EncounterId::SlimeBoss,
        &mut game.player,
        &mut game.rng,
        16,
        seed,
        20,
    ));
    game.player.potions = vec![PotionInstance {
        id: PotionId::Skill,
        slot: 0,
    }];
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::CardReward);
    assert_eq!(game.rng.card_random.counter, 3);

    game.step(&Action::Skip);

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.rng.card_random.counter, 51);
}

#[test]
fn attack_potion_waits_for_gambling_brew_hand_selection() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        6,
        game.seed,
        20,
    ));
    game.player.hand = vec![
        Card::new(CardId::Defend_B),
        Card::new(CardId::Zap),
        Card::new(CardId::Ball_Lightning),
    ];
    game.player.potions = vec![
        PotionInstance {
            id: PotionId::GamblersBrew,
            slot: 0,
        },
        PotionInstance {
            id: PotionId::Attack,
            slot: 1,
        },
    ];
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::HandSelect);

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 1,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::HandSelect);
    assert!(game.card_reward.is_empty());

    game.step(&Action::Proceed);

    assert_eq!(game.screen, Screen::CardReward);
    assert_eq!(game.card_reward.len(), 3);
    assert!(game
        .card_reward
        .iter()
        .all(|card| card.card_type() == sts_engine::ids::CardType::ATTACK));
}

#[test]
fn fire_potion_waits_for_colorless_discovery_and_keeps_relic_action_order() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        14,
        game.seed,
        20,
    ));
    game.combat.as_mut().unwrap().monsters[0].hp = 60;
    game.combat.as_mut().unwrap().monsters[0].max_hp = 60;
    game.player.hp = 50;
    game.player.relics.push(RelicInstance {
        id: RelicId::Toy_Ornithopter,
        counter: -1,
        used_up: false,
    });
    game.player.potions = vec![
        PotionInstance {
            id: PotionId::Colorless,
            slot: 0,
        },
        PotionInstance {
            id: PotionId::Fire,
            slot: 1,
        },
    ];
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::CardReward);

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 1,
        target_index: Some(0),
    });
    assert_eq!(game.screen, Screen::CardReward);
    assert_eq!(game.player.hp, 50);
    assert_eq!(game.combat.as_ref().unwrap().monsters[0].hp, 60);
    assert_eq!(game.player.potions[1].id, PotionId::Slot);

    game.step(&Action::Choose {
        index: 0,
        label: None,
        x: None,
        y: None,
        room: None,
    });

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.player.hp, 60);
    assert_eq!(game.combat.as_ref().unwrap().monsters[0].hp, 40);
}

#[test]
fn block_potion_waits_for_power_potion_discovery() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        14,
        game.seed,
        20,
    ));
    game.player.potions = vec![
        PotionInstance {
            id: PotionId::Power,
            slot: 0,
        },
        PotionInstance {
            id: PotionId::Block,
            slot: 1,
        },
    ];
    game.screen = Screen::Combat;

    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 0,
        target_index: None,
    });
    game.step(&Action::Potion {
        action: PotionOp::Use,
        slot: 1,
        target_index: None,
    });

    assert_eq!(game.screen, Screen::CardReward);
    assert_eq!(game.player.block, 0);

    game.step(&Action::Choose {
        index: 0,
        label: None,
        x: None,
        y: None,
        room: None,
    });

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.player.block, 12);
}

#[test]
fn upgraded_seek_adds_selected_cards_to_hand_in_click_order() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.combat = Some(Combat::start(
        EncounterId::TwoLouse,
        &mut game.player,
        &mut game.rng,
        6,
        game.seed,
        20,
    ));
    let mut seek = Card::new(CardId::Seek);
    seek.upgrade();
    game.player.hand = vec![seek];
    game.player.draw = vec![
        Card::new(CardId::Defragment),
        Card::new(CardId::Ball_Lightning),
        Card::new(CardId::Defend_B),
    ];
    game.player.energy = 3;
    game.screen = Screen::Combat;

    game.step(&Action::Play {
        hand_index: 0,
        target_index: None,
    });
    assert_eq!(game.screen, Screen::Grid);

    let defragment = game
        .legal_actions()
        .into_iter()
        .find(|action| matches!(action, Action::Choose { label: Some(label), .. } if label == "Defragment"))
        .expect("Defragment grid choice");
    game.step(&defragment);
    let ball_lightning = game
        .legal_actions()
        .into_iter()
        .find(|action| matches!(action, Action::Choose { label: Some(label), .. } if label == "Ball Lightning"))
        .expect("Ball Lightning grid choice");
    game.step(&ball_lightning);

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(
        game.player.hand.iter().map(|card| card.id).collect::<Vec<_>>(),
        [CardId::Defragment, CardId::Ball_Lightning]
    );
}

#[test]
fn sling_of_courage_grants_strength_only_in_elite_combat() {
    let mut elite_player = Player::defect();
    elite_player.relics.push(RelicInstance {
        id: RelicId::Sling,
        counter: -1,
        used_up: false,
    });
    let mut elite_rng = RngSet::generate_seeds(17);
    let _elite = Combat::start(
        EncounterId::BookOfStabbing,
        &mut elite_player,
        &mut elite_rng,
        23,
        17,
        20,
    );
    assert_eq!(elite_player.power_amount(PowerId::Strength), 2);

    let mut hallway_player = Player::defect();
    hallway_player.relics.push(RelicInstance {
        id: RelicId::Sling,
        counter: -1,
        used_up: false,
    });
    let mut hallway_rng = RngSet::generate_seeds(17);
    let _hallway = Combat::start(
        EncounterId::TwoLouse,
        &mut hallway_player,
        &mut hallway_rng,
        23,
        17,
        20,
    );
    assert_eq!(hallway_player.power_amount(PowerId::Strength), 0);
}

#[test]
fn girya_lifts_grant_strength_at_battle_start() {
    let mut player = Player::for_character(Character::Defect);
    player.relics.push(RelicInstance {
        id: RelicId::Girya,
        counter: 2,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(8960835100198667916);

    let _combat = Combat::start(
        EncounterId::JawWorm,
        &mut player,
        &mut rng,
        41,
        8960835100198667916,
        20,
    );

    assert_eq!(player.power_amount(PowerId::Strength), 2);
}

#[test]
fn fruit_juice_and_entropic_brew_are_usable_out_of_combat() {
    let mut game = Game::new(103370126172143121, Character::Defect, 20, Unlocks::fixture());
    game.player.potions[0].id = PotionId::FruitJuice;
    game.player.potions[1].id = PotionId::EntropicBrew;
    game.screen = Screen::CombatReward;

    let actions = game.legal_actions();
    for slot in 0..=1 {
        assert!(actions.contains(&Action::Potion {
            action: PotionOp::Use,
            slot,
            target_index: None,
        }));
    }
}

#[test]
fn explosive_potion_is_one_untargeted_action_against_multiple_monsters() {
    let mut game = Game::new(103370126172143121, Character::Defect, 20, Unlocks::fixture());
    game.player.potions[0].id = PotionId::Explosive;
    let seed = game.seed;
    game.combat = Some(Combat::start(
        EncounterId::ThreeShapes,
        &mut game.player,
        &mut game.rng,
        40,
        seed,
        20,
    ));
    game.screen = Screen::Combat;

    let uses: Vec<_> = game
        .legal_actions()
        .into_iter()
        .filter(|action| {
            matches!(
                action,
                Action::Potion {
                    action: PotionOp::Use,
                    slot: 0,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        uses,
        [Action::Potion {
            action: PotionOp::Use,
            slot: 0,
            target_index: None,
        }]
    );
}

#[test]
fn map_legal_actions_do_not_duplicate_a_destination() {
    use std::collections::HashSet;

    let mut game = Game::new(103370126172143121, Character::Defect, 20, Unlocks::fixture());
    game.dungeon.first_room_chosen = true;
    game.screen = Screen::Map;

    for y in 0..14 {
        for x in 0..7 {
            game.current_x = x;
            game.current_y = y;
            let destinations: Vec<_> = game
                .legal_actions()
                .into_iter()
                .filter_map(|action| match action {
                    Action::Choose { x: Some(x), y: Some(y), .. } => Some((x, y)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                destinations.len(),
                destinations.iter().collect::<HashSet<_>>().len(),
                "duplicate map destination from ({x}, {y}): {destinations:?}"
            );
        }
    }
}

#[test]
fn winged_greaves_exposes_the_next_row_and_spends_only_on_a_jump() {
    use std::collections::HashSet;

    let mut game = Game::new(2877855328497827070, Character::Defect, 20, Unlocks::fixture());
    game.dungeon.first_room_chosen = true;
    game.screen = Screen::Map;
    game.player.relics.push(RelicInstance {
        id: RelicId::WingedGreaves,
        counter: 3,
        used_up: false,
    });

    let mut fixture = None;
    for y in 0..13 {
        for x in 0..7 {
            let node = game.dungeon.map.node(x, y);
            let Some(next_y) = node.edges.first().map(|edge| edge.dst_y) else {
                continue;
            };
            let normal: HashSet<_> = node
                .edges
                .iter()
                .map(|edge| (edge.dst_x, edge.dst_y))
                .collect();
            let all: Vec<_> = game.dungeon.map.nodes[next_y as usize]
                .iter()
                .filter(|dest| dest.has_edges())
                .map(|dest| (dest.x, dest.y))
                .collect();
            if let Some(&jump) = all.iter().find(|dest| !normal.contains(dest)) {
                fixture = Some((x, y, all, jump));
                break;
            }
        }
        if fixture.is_some() {
            break;
        }
    }
    let (x, y, expected, jump) = fixture.expect("map with a wing-only next-row node");
    game.current_x = x;
    game.current_y = y;

    let actions = game.legal_actions();
    let destinations: Vec<_> = actions
        .iter()
        .filter_map(|action| match action {
            Action::Choose { x: Some(x), y: Some(y), .. } => Some((*x, *y)),
            _ => None,
        })
        .collect();
    assert_eq!(destinations, expected);
    let action = actions
        .into_iter()
        .find(|action| {
            matches!(
                action,
                Action::Choose {
                    x: Some(x),
                    y: Some(y),
                    ..
                } if (*x, *y) == jump
            )
        })
        .expect("wing-only action");
    game.step(&action);

    assert_eq!(
        game.player
            .relics
            .iter()
            .find(|relic| relic.id == RelicId::WingedGreaves)
            .map(|relic| relic.counter),
        Some(2)
    );

    let mut boss_game =
        Game::new(2877855328497827070, Character::Defect, 20, Unlocks::fixture());
    boss_game.dungeon.first_room_chosen = true;
    boss_game.current_x = 0;
    boss_game.current_y = 14;
    boss_game.screen = Screen::Map;
    boss_game.player.relics.push(RelicInstance {
        id: RelicId::WingedGreaves,
        counter: 2,
        used_up: false,
    });
    let boss = boss_game
        .legal_actions()
        .into_iter()
        .find(|action| {
            matches!(
                action,
                Action::Choose {
                    x: Some(-1),
                    y: Some(15),
                    ..
                }
            )
        })
        .expect("boss map action");
    boss_game.step(&boss);
    assert_eq!(
        boss_game
            .player
            .relics
            .iter()
            .find(|relic| relic.id == RelicId::WingedGreaves)
            .map(|relic| relic.counter),
        Some(2)
    );
}

#[test]
fn first_room_of_a_new_act_ignores_the_previous_boss_coordinate() {
    let mut game = Game::new(2877855328497827070, Character::Defect, 20, Unlocks::fixture());
    game.dungeon.first_room_chosen = false;
    game.current_x = -1;
    game.current_y = 15;
    game.screen = Screen::Map;
    game.player.relics.push(RelicInstance {
        id: RelicId::WingedGreaves,
        counter: 3,
        used_up: false,
    });
    let first_room = game
        .legal_actions()
        .into_iter()
        .find(|action| matches!(action, Action::Choose { .. }))
        .expect("first-row map action");

    game.step(&first_room);

    assert_eq!(game.current_y, 0);
    assert_eq!(
        game.player
            .relics
            .iter()
            .find(|relic| relic.id == RelicId::WingedGreaves)
            .map(|relic| relic.counter),
        Some(3)
    );
}

#[test]
fn skip_on_grid_confirmation_cancels_the_preview() {
    let mut game = Game::new(7, Character::Defect, 20, Unlocks::fixture());
    game.neow_screen = 3;
    game.neow_options = vec![NeowOption {
        label: "Remove a card".into(),
        kind: NeowKind::RemoveCard,
        drawback: NeowDrawback::None,
    }];
    game.step(&Action::Choose {
        index: 0,
        label: Some("Remove a card".into()),
        x: None,
        y: None,
        room: None,
    });

    let deck_before = game.player.deck.clone();
    let card = game
        .legal_actions()
        .into_iter()
        .find(|action| matches!(action, Action::Choose { .. }))
        .expect("purge card choice");
    game.step(&card);
    assert!(game.legal_actions().contains(&Action::Proceed));
    assert!(game.legal_actions().contains(&Action::Skip));

    game.step(&Action::Skip);

    assert_eq!(game.screen, Screen::Grid);
    assert_eq!(game.player.deck, deck_before);
    assert!(game
        .legal_actions()
        .iter()
        .any(|action| matches!(action, Action::Choose { .. })));
    assert!(!game.legal_actions().contains(&Action::Proceed));
}

fn action_choose(index: usize) -> Action {
    Action::Choose {
        index,
        label: None,
        x: None,
        y: None,
        room: None,
    }
}

#[test]
fn neow_applies_curse_before_twenty_percent_max_hp_reward() {
    let mut game = Game::new(2696771490991422653, Character::Defect, 20, Unlocks::fixture());
    game.step(&action_choose(0));
    game.step(&action_choose(2));

    assert_eq!(game.player.hp, 78);
    assert_eq!(game.player.max_hp, 85);
    assert_eq!(game.player.deck.last().map(|card| card.id), Some(CardId::Regret));
    assert_eq!(game.rng.card.counter, 1);
}

#[test]
fn neow_applies_curse_before_two_hundred_fifty_gold_reward() {
    let mut game = Game::new(6999307915985924753, Character::Defect, 20, Unlocks::fixture());
    game.step(&action_choose(0));
    game.step(&action_choose(2));

    assert_eq!(game.player.gold, 349);
    assert_eq!(game.player.deck.last().map(|card| card.id), Some(CardId::Clumsy));
    assert_eq!(game.rng.card.counter, 1);
}

#[test]
fn neow_percent_damage_drawback_uses_thirty_percent_of_current_hp() {
    let mut game = Game::new(2419708263384732054, Character::Defect, 20, Unlocks::fixture());
    game.step(&action_choose(0));
    game.step(&action_choose(2));

    assert_eq!(game.player.hp, 46);
}

#[test]
fn neow_defers_curse_until_after_rare_card_reward_opens() {
    let mut game = Game::new(3696180478129188597, Character::Defect, 20, Unlocks::fixture());
    game.step(&action_choose(0));
    game.step(&action_choose(2));

    assert_eq!(game.screen, Screen::CardReward);
    assert_eq!(game.player.deck.len(), 11);
    assert_eq!(game.rng.card.counter, 0);
    assert_eq!(
        game.card_reward.iter().copied().map(Card::sts_id).collect::<Vec<_>>(),
        ["Creative AI", "Biased Cognition", "Rainbow"]
    );

    game.step(&action_choose(1));
    assert_eq!(game.screen, Screen::Neow);
    assert_eq!(game.rng.card.counter, 1);
    assert_eq!(
        game.player.deck.iter().copied().map(Card::sts_id).collect::<Vec<_>>().split_off(11),
        ["Clumsy", "Biased Cognition"]
    );
}

#[test]
fn neow_defers_curse_until_transform_two_grid_closes() {
    let mut game = Game::new(46, Character::Defect, 20, Unlocks::fixture());
    game.step(&action_choose(0));
    game.step(&action_choose(2));

    assert_eq!(game.screen, Screen::Grid);
    assert_eq!(game.player.deck.len(), 11);
    assert_eq!(game.rng.card.counter, 0);
    let choices: Vec<_> = game
        .legal_actions()
        .into_iter()
        .filter(|action| matches!(action, Action::Choose { .. }))
        .collect();
    assert_eq!(choices.len(), 10);

    game.step(&choices[0]);
    assert_eq!(game.player.deck.len(), 11);
    assert_eq!(game.rng.card.counter, 0);
    assert_eq!(game.legal_actions(), choices);

    game.step(&choices[1]);
    assert_eq!(game.screen, Screen::Neow);
    assert_eq!(game.rng.card.counter, 1);
    assert_eq!(
        game.player.deck.iter().copied().map(Card::sts_id).collect::<Vec<_>>().split_off(9),
        ["BootSequence", "Sweeping Beam", "Shame"]
    );
}

#[test]
fn unopened_treasure_exposes_proceed_without_rolling_chest_rewards() {
    let mut game = Game::new(1924666432788095156, Character::Defect, 20, Unlocks::fixture());
    game.screen = Screen::Treasure;
    game.current_room = RoomType::Treasure;
    let treasure_counter = game.rng.treasure.counter;
    let relic_counter = game.rng.relic.counter;

    assert!(game.legal_actions().contains(&Action::Proceed));
    game.step(&Action::Proceed);

    assert_eq!(game.screen, Screen::Map);
    assert!(game.rewards.is_empty());
    assert_eq!(game.rng.treasure.counter, treasure_counter);
    assert_eq!(game.rng.relic.counter, relic_counter);
}

#[test]
fn a18_gremlin_nob_does_not_skull_bash_within_two_moves() {
    let mut player = Player::for_character(Character::Defect);
    player.hp = 200;
    player.max_hp = 200;
    let mut rng = RngSet::generate_seeds(2696771490991422653);
    let mut combat = Combat::start(
        EncounterId::GremlinNob,
        &mut player,
        &mut rng,
        7,
        2696771490991422653,
        20,
    );

    assert_eq!(combat.monsters[0].next_move, 3);
    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].next_move, 2);
    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].next_move, 1);
    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].next_move, 1);
}

#[test]
fn darkling_killed_by_bronze_scales_counts_before_regrowing() {
    let mut player = Player::for_character(Character::Defect);
    player.hp = 200;
    player.max_hp = 200;
    player.relics.push(RelicInstance {
        id: RelicId::Bronze_Scales,
        counter: -1,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(969606797112288563);
    let mut combat = Combat::start(
        EncounterId::ThreeDarklings,
        &mut player,
        &mut rng,
        36,
        969606797112288563,
        20,
    );
    player.orbs.clear();
    combat.monsters[0].hp = 3;
    combat.monsters[0].block = 0;
    combat.monsters[0].next_move = 1;

    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].hp, 0);
    assert!(combat.monsters[0].half_dead);
    assert_eq!(combat.monsters[0].next_move, 4);

    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].hp, 0);
    assert!(combat.monsters[0].half_dead);
    assert_eq!(combat.monsters[0].next_move, 5);

    combat::end_turn(&mut player, &mut combat, &mut rng, None);
    assert_eq!(combat.monsters[0].hp, combat.monsters[0].max_hp / 2);
    assert!(!combat.monsters[0].half_dead);
}

#[test]
fn static_discharge_kills_maw_before_remaining_multiattack_hits() {
    let mut player = Player::for_character(Character::Defect);
    player.relics.push(RelicInstance {
        id: RelicId::Lizard_Tail,
        counter: -1,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(8360976353793871823);
    let mut combat = Combat::start(
        EncounterId::Maw,
        &mut player,
        &mut rng,
        39,
        8360976353793871823,
        20,
    );
    player.hp = 35;
    player.max_hp = 76;
    player.block = 22;
    player.add_power(PowerId::Focus, 1);
    player.add_power(PowerId::StaticDischarge, 1);
    player.add_power(PowerId::Electro, 1);
    player.max_orbs = 3;
    player.orbs = vec![
        Orb { kind: OrbKind::Lightning, evoke: 0 },
        Orb { kind: OrbKind::Lightning, evoke: 0 },
        Orb { kind: OrbKind::Lightning, evoke: 0 },
    ];
    combat.monsters[0].hp = 29;
    combat.monsters[0].block = 0;
    combat.monsters[0].next_move = 5;
    combat.monsters[0].extra = 12;
    combat.monsters[0].add_power(PowerId::Strength, 5);

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert!(combat.monsters[0].dead);
    assert_eq!(player.hp, 17);
    assert_eq!(
        player
            .relics
            .iter()
            .find(|relic| relic.id == RelicId::Lizard_Tail)
            .map(|relic| relic.counter),
        Some(-1)
    );
}

#[test]
fn mercury_hourglass_finishes_an_all_half_dead_darkling_group() {
    let mut player = Player::for_character(Character::Defect);
    player.hp = 200;
    player.max_hp = 200;
    player.relics.push(RelicInstance {
        id: RelicId::Mercury_Hourglass,
        counter: -1,
        used_up: false,
    });
    let mut rng = RngSet::generate_seeds(8945191795714220528);
    let mut combat = Combat::start(
        EncounterId::ThreeDarklings,
        &mut player,
        &mut rng,
        41,
        8945191795714220528,
        20,
    );
    player.orbs.clear();
    for monster in &mut combat.monsters[..2] {
        monster.hp = 0;
        monster.dead = false;
        monster.half_dead = true;
        monster.next_move = 4;
    }
    combat.monsters[2].hp = 1;
    combat.monsters[2].block = 0;
    combat.monsters[2].next_move = 4;

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert!(combat.all_dead());
    assert!(combat.monsters.iter().all(|monster| monster.dead));
}

#[test]
fn a20_beyond_proceed_starts_second_boss_without_healing() {
    let mut game = Game::new(7, Character::Defect, 20, Unlocks::fixture());
    game.dungeon.act = Act::Beyond;
    game.dungeon.floor = 50;
    game.dungeon.boss = "Awakened One".into();
    game.dungeon.boss_list.clear();
    game.dungeon.boss_list.extend(["Donu and Deca", "Time Eater"].map(str::to_string));
    game.current_room = RoomType::Boss;
    game.screen = Screen::CombatReward;
    game.player.hp = 17;

    assert_eq!(game.legal_actions(), vec![Action::Proceed]);
    game.step(&Action::Proceed);

    assert_eq!(game.screen, Screen::Combat);
    assert_eq!(game.current_room, RoomType::Boss);
    assert_eq!(game.current_x, -1);
    assert_eq!(game.current_y, 15);
    assert_eq!(game.dungeon.floor, 51);
    assert_eq!(game.player.hp, 17);
    assert_eq!(game.dungeon.boss, "Donu and Deca");
    assert_eq!(game.dungeon.boss_list.as_ref(), &["Time Eater"]);
    assert_eq!(game.combat.as_ref().unwrap().encounter, EncounterId::DonuAndDeca);
}

#[test]
fn a19_beyond_proceed_goes_to_spire_heart() {
    let mut game = Game::new(7, Character::Defect, 19, Unlocks::fixture());
    game.dungeon.act = Act::Beyond;
    game.dungeon.floor = 50;
    game.dungeon.boss_list.clear();
    game.dungeon.boss_list.extend(["Time Eater", "Donu and Deca"].map(str::to_string));
    game.current_room = RoomType::Boss;
    game.screen = Screen::CombatReward;

    game.step(&Action::Proceed);

    assert_eq!(game.screen, Screen::Event);
    assert_eq!(game.current_room, RoomType::Victory);
    assert_eq!(game.dungeon.floor, 51);
    assert!(game.combat.is_none());
}

#[test]
fn time_eater_has_a20_hp_haste_and_head_slam_effects() {
    let mut rng = RngSet::generate_seeds(11);
    let mut player = Player::defect();
    let mut monster = combat::spawn_monster(MonsterId::TimeEater, &mut rng, 20);
    assert_eq!(monster.hp, 480);

    monster.hp = 239;
    monster.add_power(PowerId::Weak, 2);
    monster.roll_move(&mut rng);
    assert_eq!(monster.next_move, 5);
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(monster.hp, 240);
    assert_eq!(monster.block, 32);
    assert_eq!(monster.power_amount(PowerId::Weak), 0);

    monster.next_move = 4;
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(player.power_amount(PowerId::DrawReduction), 1);
    assert_eq!(player.discard.iter().filter(|card| card.id == CardId::Slimed).count(), 2);
}

#[test]
fn expiring_draw_reduction_still_reduces_the_already_queued_draw() {
    let mut rng = RngSet::generate_seeds(29);
    let mut player = Player::defect();
    let mut combat = Combat::start(EncounterId::TimeEater, &mut player, &mut rng, 51, 29, 20);
    player.hand.clear();
    player.discard.clear();
    player.exhaust.clear();
    player.draw = vec![Card::new(CardId::Defend_B); 6];
    player.add_power_from_monster(PowerId::DrawReduction, 1);
    player.powers.iter_mut().find(|power| power.id == PowerId::DrawReduction).unwrap().just_applied = false;
    combat.monsters[0].next_move = 3;

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert_eq!(player.hand.len(), 4);
    assert_eq!(player.power_amount(PowerId::DrawReduction), 0);
}

#[test]
fn spheric_guardian_uses_its_hard_block_and_damage_values() {
    let mut rng = RngSet::generate_seeds(13);
    let mut player = Player::defect();
    let mut monster = combat::spawn_monster(MonsterId::SphericGuardian, &mut rng, 20);

    monster.block = 0;
    monster.next_move = 2;
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(monster.block, 35);

    let hp = player.hp;
    monster.block = 0;
    monster.next_move = 3;
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(monster.block, 15);
    assert_eq!(player.hp, hp - 11);
}

#[test]
fn spheric_guardian_slam_resolves_abacus_after_both_damage_actions() {
    let mut rng = RngSet::generate_seeds(13);
    let mut player = Player::defect();
    player.hp = 57;
    player.block = 10;
    player.hand.clear();
    player.draw = vec![Card::new(CardId::Ball_Lightning)];
    player.discard = vec![Card::new(CardId::Strike_B), Card::new(CardId::Defend_B)];
    player.relics.push(RelicInstance {
        id: RelicId::Centennial_Puzzle,
        counter: -1,
        used_up: false,
    });
    player.relics.push(RelicInstance {
        id: RelicId::TheAbacus,
        counter: -1,
        used_up: false,
    });
    let mut monster = combat::spawn_monster(MonsterId::SphericGuardian, &mut rng, 20);
    monster.next_move = 1;

    monster.take_turn(&mut player, &mut rng, 20, None);

    assert_eq!(player.hp, 45);
    assert_eq!(player.block, 6);
    assert_eq!(player.hand.len(), 3);
}

#[test]
fn bronze_orb_uses_its_ascension_nine_hp_range() {
    let mut saw_upper_bound = false;
    for seed in 0..100 {
        let mut rng = RngSet::generate_seeds(seed);
        let monster = combat::spawn_monster(MonsterId::BronzeOrb, &mut rng, 20);
        assert!((54..=60).contains(&monster.hp), "seed {seed} rolled {} HP", monster.hp);
        saw_upper_bound |= monster.hp == 60;
    }
    assert!(saw_upper_bound);
}

#[test]
fn hourglass_returns_stasis_card_between_turn_draw_and_gremlin_horn_draw() {
    let mut rng = RngSet::generate_seeds(31);
    let mut player = Player::defect();
    player.relics.push(RelicInstance {
        id: RelicId::Mercury_Hourglass,
        counter: -1,
        used_up: false,
    });
    player.relics.push(RelicInstance {
        id: RelicId::Gremlin_Horn,
        counter: -1,
        used_up: false,
    });
    let mut combat = Combat::start(EncounterId::TwoLouse, &mut player, &mut rng, 33, 31, 20);
    player.orbs.clear();
    player.hand.clear();
    player.discard.clear();
    player.exhaust.clear();
    player.draw = [
        CardId::Doom_and_Gloom,
        CardId::Barrage,
        CardId::Defend_B,
        CardId::AscendersBane,
        CardId::Strike_B,
        CardId::Zap,
    ]
    .into_iter()
    .map(Card::new)
    .collect();

    combat.monsters = vec![
        combat::spawn_monster(MonsterId::BronzeOrb, &mut rng, 20),
        combat::spawn_monster(MonsterId::BronzeAutomaton, &mut rng, 20),
    ];
    let stasis_orb = &mut combat.monsters[0];
    stasis_orb.hp = 3;
    stasis_orb.block = 0;
    stasis_orb.next_move = 2;
    stasis_orb.stasis_card = Some(Card::new(CardId::Glacier));
    for monster in &mut combat.monsters {
        if monster.id == MonsterId::BronzeAutomaton {
            monster.next_move = 99;
        }
    }

    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    assert_eq!(
        player.hand.iter().map(|card| card.id).collect::<Vec<_>>(),
        [
            CardId::Zap,
            CardId::Strike_B,
            CardId::AscendersBane,
            CardId::Defend_B,
            CardId::Barrage,
            CardId::Glacier,
            CardId::Doom_and_Gloom,
        ]
    );
    assert_eq!(player.energy, player.energy_master + 1);
}

#[test]
fn a20_transient_has_six_turns_and_starts_at_forty_damage() {
    let mut rng = RngSet::generate_seeds(17);
    let mut player = Player::defect();
    let combat = Combat::start(EncounterId::Transient, &mut player, &mut rng, 1, 17, 20);
    let monster = &combat.monsters[0];

    assert_eq!(monster.power_amount(PowerId::Fading), 6);
    assert_eq!(monster.next_move, 1);
    assert_eq!(monster.intent_damage, 40);
}

#[test]
fn awakened_one_rebirth_resolves_during_the_monster_phase() {
    let mut rng = RngSet::generate_seeds(19);
    let mut player = Player::defect();
    let mut combat = Combat::start(EncounterId::AwakenedOne, &mut player, &mut rng, 50, 19, 20);
    for monster in &mut combat.monsters {
        if monster.id == MonsterId::Cultist {
            monster.hp = 0;
            monster.dead = true;
        }
    }
    let awakened = combat.monsters.iter_mut().find(|monster| monster.id == MonsterId::AwakenedOne).unwrap();
    awakened.hp = 1;
    combat::damage_monster(awakened, &mut player, &mut rng, 1, 1);
    assert!(awakened.half_dead);
    assert_eq!(awakened.next_move, 3);

    let ai_before_rebirth = rng.ai.counter;
    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    let awakened = combat.monsters.iter().find(|monster| monster.id == MonsterId::AwakenedOne).unwrap();
    assert!(!awakened.half_dead);
    assert_eq!(awakened.hp, 320);
    assert_eq!(awakened.next_move, 5);
    assert_eq!(rng.ai.counter, ai_before_rebirth + 1);

    let mut rng = RngSet::generate_seeds(23);
    let mut player = Player::defect();
    player.relics.push(RelicInstance { id: RelicId::Bronze_Scales, counter: -1, used_up: false });
    let mut combat = Combat::start(EncounterId::AwakenedOne, &mut player, &mut rng, 50, 23, 20);
    for monster in &mut combat.monsters {
        if monster.id == MonsterId::Cultist {
            monster.hp = 0;
            monster.dead = true;
        }
    }
    let awakened = combat.monsters.iter_mut().find(|monster| monster.id == MonsterId::AwakenedOne).unwrap();
    awakened.hp = 9;
    awakened.next_move = 2;

    let ai_before_reactive_death = rng.ai.counter;
    combat::end_turn(&mut player, &mut combat, &mut rng, None);

    let awakened = combat.monsters.iter().find(|monster| monster.id == MonsterId::AwakenedOne).unwrap();
    assert!(awakened.half_dead);
    assert_eq!(awakened.next_move, 3);
    assert_eq!(rng.ai.counter, ai_before_reactive_death + 1);
}

#[test]
fn time_warp_forces_turn_after_twelve_cards_and_grants_strength() {
    let mut game = Game::new(11, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Boss;
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::TimeEater,
        &mut game.player,
        &mut game.rng,
        50,
        game.seed,
        20,
    ));

    for play in 1..=12 {
        let mut strike = Card::new(CardId::Strike_B);
        strike.free_to_play_once = true;
        game.player.hand.insert(0, strike);
        game.step(&Action::Play { hand_index: 0, target_index: Some(0) });
        if play < 12 {
            assert_eq!(game.combat.as_ref().unwrap().turn, 1);
        }
    }

    let combat = game.combat.as_ref().unwrap();
    assert_eq!(combat.encounter, EncounterId::TimeEater);
    assert_eq!(combat.turn, 2);
    assert!(!combat.force_end_turn);
    assert_eq!(combat.monsters[0].extra, 0);
    assert_eq!(combat.monsters[0].power_amount(PowerId::Strength), 2);
}

#[test]
fn spire_growth_uses_a20_hp_and_opens_with_constrict() {
    let mut rng = RngSet::generate_seeds(17);
    let mut player = Player::defect();
    let combat = Combat::start(EncounterId::SpireGrowth, &mut player, &mut rng, 40, 17, 20);
    let mut monster = combat.monsters.into_iter().next().unwrap();

    assert_eq!(EncounterId::from_sts_key("Spire Growth"), Some(EncounterId::SpireGrowth));
    assert_eq!(monster.id, MonsterId::SpireGrowth);
    assert_eq!(monster.hp, 190);
    assert_eq!(monster.next_move, 2);

    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(player.power_amount(PowerId::Constricted), 12);
}

#[test]
fn constricted_deals_blockable_end_of_turn_damage() {
    let mut game = Game::new(17, Character::Defect, 20, Unlocks::fixture());
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::SpireGrowth,
        &mut game.player,
        &mut game.rng,
        40,
        game.seed,
        20,
    ));
    game.player.hp = 50;
    game.player.block = 5;
    game.player.add_power(PowerId::Constricted, 12);
    let monster = &mut game.combat.as_mut().unwrap().monsters[0];
    monster.next_move = 99;
    monster.intent_damage = 0;

    game.step(&Action::EndTurn);

    assert_eq!(game.player.hp, 43);
    assert_eq!(game.player.power_amount(PowerId::Constricted), 12);
}

#[test]
fn nemesis_has_a20_hp_burns_and_alternating_intangible() {
    let mut game = Game::new(23, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Elite;
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::Nemesis,
        &mut game.player,
        &mut game.rng,
        40,
        game.seed,
        20,
    ));
    game.player.hp = 500;
    game.player.max_hp = 500;

    let monster = &mut game.combat.as_mut().unwrap().monsters[0];
    assert_eq!(EncounterId::from_sts_key("Nemesis"), Some(EncounterId::Nemesis));
    assert_eq!(monster.id, MonsterId::Nemesis);
    assert_eq!(monster.hp, 200);
    monster.next_move = 4;
    monster.intent_damage = 0;

    game.step(&Action::EndTurn);
    let combat = game.combat.as_ref().unwrap();
    assert_eq!(game.player.discard.iter().filter(|card| card.id == CardId::Burn).count(), 5);
    assert_eq!(combat.monsters[0].power_amount(PowerId::Intangible), 1);

    game.step(&Action::EndTurn);
    assert_eq!(game.combat.as_ref().unwrap().monsters[0].power_amount(PowerId::Intangible), 0);
}

#[test]
fn reptomancer_starts_with_two_daggers_and_summons_two_more_at_a20() {
    let mut game = Game::new(29, Character::Defect, 20, Unlocks::fixture());
    game.current_room = RoomType::Elite;
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::Reptomancer,
        &mut game.player,
        &mut game.rng,
        40,
        game.seed,
        20,
    ));
    game.player.hp = 500;
    game.player.max_hp = 500;

    let combat = game.combat.as_ref().unwrap();
    assert_eq!(
        combat.monsters.iter().map(|monster| monster.id).collect::<Vec<_>>(),
        [MonsterId::Dagger, MonsterId::Reptomancer, MonsterId::Dagger]
    );
    assert!((190..=200).contains(&combat.monsters[1].hp));
    assert_eq!(combat.monsters[1].next_move, 2);
    assert!(combat
        .monsters
        .iter()
        .filter(|monster| monster.id == MonsterId::Dagger)
        .all(|monster| monster.next_move == 1));

    game.step(&Action::EndTurn);

    let combat = game.combat.as_ref().unwrap();
    assert_eq!(
        combat
            .monsters
            .iter()
            .filter(|monster| monster.id == MonsterId::Dagger && monster.alive())
            .count(),
        4
    );
    assert_eq!(
        game.player.discard.iter().filter(|card| card.id == CardId::Wound).count(),
        2
    );
}

#[test]
fn reptomancer_daggers_do_not_keep_the_encounter_open_after_summoner_death() {
    let mut rng = RngSet::generate_seeds(31);
    let mut player = Player::defect();
    let mut combat = Combat::start(EncounterId::Reptomancer, &mut player, &mut rng, 40, 31, 20);

    combat.monsters[1].hp = 0;
    combat.monsters[1].dead = true;

    assert!(combat.all_dead());
}

#[test]
fn writhing_mass_has_a20_stats_and_rerolls_after_a_nonlethal_attack() {
    let mut game = Game::new(37, Character::Defect, 20, Unlocks::fixture());
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::WrithingMass,
        &mut game.player,
        &mut game.rng,
        40,
        game.seed,
        20,
    ));

    let monster = &game.combat.as_ref().unwrap().monsters[0];
    assert_eq!(EncounterId::from_sts_key("Writhing Mass"), Some(EncounterId::WrithingMass));
    assert_eq!(monster.id, MonsterId::WrithingMass);
    assert_eq!(monster.hp, 175);
    assert_eq!(monster.pending_reactive, 0);
    assert_eq!(monster.power_amount(PowerId::Malleable), 3);
    assert!(matches!(monster.next_move, 1..=3));
    assert_eq!(monster.move_history.len(), 1);

    let mut strike = Card::new(CardId::Strike_B);
    strike.free_to_play_once = true;
    game.player.hand.insert(0, strike);
    game.step(&Action::Play { hand_index: 0, target_index: Some(0) });

    let monster = &game.combat.as_ref().unwrap().monsters[0];
    assert_eq!(monster.hp, 169);
    assert_eq!(monster.block, 3);
    assert_eq!(monster.power_amount(PowerId::Malleable), 4);
    assert_eq!(monster.pending_reactive, 0);
    assert_eq!(monster.move_history.len(), 2);
}

#[test]
fn writhing_mass_implant_adds_a_permanent_parasite() {
    let mut game = Game::new(41, Character::Defect, 20, Unlocks::fixture());
    game.screen = Screen::Combat;
    game.combat = Some(Combat::start(
        EncounterId::WrithingMass,
        &mut game.player,
        &mut game.rng,
        40,
        game.seed,
        20,
    ));
    game.player.hp = 500;
    let monster = &mut game.combat.as_mut().unwrap().monsters[0];
    monster.next_move = 4;
    monster.intent_damage = 0;

    game.step(&Action::EndTurn);

    assert_eq!(game.player.deck.iter().filter(|card| card.id == CardId::Parasite).count(), 1);
}

#[test]
fn bronze_automaton_uses_a20_hp_damage_boost_and_post_beam_move() {
    let mut rng = RngSet::generate_seeds(43);
    let mut player = Player::defect();
    let mut monster = combat::spawn_monster(MonsterId::BronzeAutomaton, &mut rng, 20);
    assert_eq!(monster.hp, 320);

    monster.next_move = 5;
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(monster.block, 12);
    assert_eq!(monster.power_amount(PowerId::Strength), 4);

    monster.first_move = false;
    monster.move_history.clear();
    monster.move_history.push(2);
    monster.roll_move(&mut rng);
    assert_eq!(monster.next_move, 5);

    let hp_before = player.hp;
    monster.next_move = 2;
    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(hp_before - player.hp, 54);
}

#[test]
fn awakened_one_uses_a20_powers_and_requires_both_320_hp_forms() {
    let mut rng = RngSet::generate_seeds(47);
    let mut player = Player::defect();
    let mut combat = Combat::start(EncounterId::AwakenedOne, &mut player, &mut rng, 50, 47, 20);
    let awakened_index = combat
        .monsters
        .iter()
        .position(|monster| monster.id == MonsterId::AwakenedOne)
        .unwrap();

    let monster = &mut combat.monsters[awakened_index];
    assert_eq!(monster.hp, 320);
    assert_eq!(monster.power_amount(PowerId::Regen), 15);
    assert_eq!(monster.power_amount(PowerId::Curiosity), 2);
    assert_eq!(monster.power_amount(PowerId::Strength), 2);
    monster.add_power(PowerId::Weak, 2);

    // A multi-hit lethal must stop at the phase boundary rather than spilling
    // into the reborn form.
    combat::damage_monster(monster, &mut player, &mut rng, 500, 2);
    assert_eq!(monster.hp, 0);
    assert!(monster.half_dead);
    assert!(!monster.dead);
    assert_eq!(monster.next_move, 3);
    assert_eq!(monster.power_amount(PowerId::Weak), 0);
    assert_eq!(monster.power_amount(PowerId::Curiosity), 0);
    assert_eq!(monster.power_amount(PowerId::Regen), 15);
    assert_eq!(monster.power_amount(PowerId::Strength), 2);

    monster.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(monster.hp, 320);
    assert!(!monster.half_dead);
    monster.roll_move(&mut rng);
    assert_eq!(monster.next_move, 5);

    combat::damage_monster(monster, &mut player, &mut rng, 500, 1);
    assert!(monster.dead);
    assert!(!monster.half_dead);
    assert!(combat.all_dead());
}

#[test]
fn a20_hp_tiers_cover_late_hallways_elites_and_heart() {
    let variable_ranges = [
        (MonsterId::Chosen, 98, 103),
        (MonsterId::SnakePlant, 78, 82),
        (MonsterId::ShelledParasite, 70, 75),
        (MonsterId::Centurion, 78, 83),
        (MonsterId::Healer, 50, 58),
        (MonsterId::Darkling, 50, 59),
        (MonsterId::Spiker, 44, 60),
        (MonsterId::Repulsor, 31, 38),
        (MonsterId::BookOfStabbing, 168, 172),
    ];
    for (id, expected_min, expected_max) in variable_ranges {
        let mut rng = RngSet::generate_seeds(53);
        let hp: Vec<i32> = (0..500)
            .map(|_| combat::spawn_monster(id, &mut rng, 20).hp)
            .collect();
        assert_eq!(hp.iter().copied().min(), Some(expected_min), "{id:?}");
        assert_eq!(hp.iter().copied().max(), Some(expected_max), "{id:?}");
    }

    let mut rng = RngSet::generate_seeds(59);
    assert_eq!(combat::spawn_monster(MonsterId::GiantHead, &mut rng, 20).hp, 520);
    assert_eq!(combat::spawn_monster(MonsterId::CorruptHeart, &mut rng, 20).hp, 800);
}

#[test]
fn a20_exploder_uses_its_ascension_seven_hp_roll_and_ascension_two_damage() {
    let seed = 8_159_705_357_625_746_691;
    let mut rng = RngSet::generate_seeds(seed);
    let mut player = Player::defect();
    let mut combat = Combat::start(
        EncounterId::ThreeShapes,
        &mut player,
        &mut rng,
        35,
        seed,
        20,
    );
    let exploder = combat
        .monsters
        .iter_mut()
        .find(|monster| monster.id == MonsterId::Exploder)
        .expect("seeded ancient-shape encounter should contain an Exploder");

    assert_eq!(exploder.hp, 34);
    assert_eq!(exploder.intent_damage, 11);
    let hp_before = player.hp;
    exploder.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(player.hp, hp_before - 11);
}

#[test]
fn late_monsters_use_their_a20_damage_and_hard_move_branches() {
    let mut rng = RngSet::generate_seeds(61);
    let mut player = Player::defect();
    player.hp = 1_000;
    player.max_hp = 1_000;

    let mut chosen = combat::spawn_monster(MonsterId::Chosen, &mut rng, 20);
    chosen.roll_move(&mut rng);
    assert_eq!(chosen.next_move, 4);

    let mut parasite = combat::spawn_monster(MonsterId::ShelledParasite, &mut rng, 20);
    parasite.roll_move(&mut rng);
    assert_eq!(parasite.next_move, 1);
    let before = player.hp;
    parasite.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(before - player.hp, 21);

    let mut darkling = combat::spawn_monster(MonsterId::Darkling, &mut rng, 20);
    assert!((9..=13).contains(&darkling.extra));
    darkling.next_move = 2;
    darkling.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(darkling.block, 12);
    assert_eq!(darkling.power_amount(PowerId::Strength), 2);

    let mut book = combat::spawn_monster(MonsterId::BookOfStabbing, &mut rng, 20);
    book.next_move = 2;
    let before = player.hp;
    book.take_turn(&mut player, &mut rng, 20, None);
    assert_eq!(before - player.hp, 24);

    let mut shapes = Combat::start(EncounterId::ThreeShapes, &mut player, &mut rng, 40, 61, 20);
    let spiker = shapes
        .monsters
        .iter_mut()
        .find(|monster| monster.id == MonsterId::Spiker)
        .unwrap();
    assert_eq!(spiker.power_amount(PowerId::Thorns), 7);

    let mut giant = combat::spawn_monster(MonsterId::GiantHead, &mut rng, 20);
    giant.extra = 1;
    giant.roll_move(&mut rng);
    assert_eq!(giant.intent_damage, 40);
}
