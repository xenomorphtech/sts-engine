use sts_engine::action::Action;
use sts_engine::card::Card;
use sts_engine::combat::{self, Combat};
use sts_engine::creature::{Player, RelicInstance};
use sts_engine::game::{Game, Screen};
use sts_engine::ids::{Act, CardId, Character, EncounterId, MonsterId, PowerId, RelicId, RoomType};
use sts_engine::rng::RngSet;
use sts_engine::Unlocks;

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
