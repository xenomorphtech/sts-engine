use crate::card::Card;
use crate::game::Game;
use crate::ids::{Act, CardId, CardType, EncounterId, RelicId};

/// Long-lived deck-building tasks shared by rewards, shops, and boss relics.
/// Acquisitions are valued as package components rather than in isolation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeckTask {
    EstablishOrbCore,
    ConvertFrost,
    ConvertLightning,
    ScalePowers,
    ExploitZeroCost,
    FundExpensiveCards,
    PrepareForBoss,
}

#[derive(Clone, Copy, Debug, Default)]
struct DeckProfile {
    attacks: i32,
    strikes: i32,
    skills: i32,
    powers: i32,
    channel: i32,
    frost: i32,
    lightning: i32,
    dark: i32,
    aoe: i32,
    focus: i32,
    orb_slots: i32,
    zero_cost: i32,
    zero_cost_attacks: i32,
    expensive: i32,
    energy: i32,
    x_cost: i32,
    biased_cognition: i32,
    blizzard: i32,
    all_for_one: i32,
    scrape: i32,
    claw: i32,
    thunder_strike: i32,
    power_payoffs: i32,
}

impl DeckProfile {
    fn from_game(game: &Game) -> Self {
        let mut profile = Self::default();
        for card in &game.player.deck {
            profile.add_card(card);
        }
        profile
    }

    fn add_card(&mut self, card: &Card) {
        match card.card_type() {
            CardType::ATTACK => self.attacks += 1,
            CardType::SKILL => self.skills += 1,
            CardType::POWER => self.powers += 1,
            _ => {}
        }
        self.strikes += i32::from(matches!(
            card.id,
            CardId::Strike_R | CardId::Strike_G | CardId::Strike_B | CardId::Strike_P
        ));
        self.channel += i32::from(is_channel(card.id));
        self.frost += i32::from(is_frost_source(card.id));
        self.lightning += i32::from(is_lightning_source(card.id));
        self.dark += i32::from(is_dark_source(card.id));
        self.aoe += i32::from(is_aoe_source(card.id));
        self.focus += i32::from(matches!(
            card.id,
            CardId::Defragment | CardId::Biased_Cognition
        ));
        self.orb_slots += i32::from(card.id == CardId::Capacitor);
        if card.cost == 0 {
            self.zero_cost += 1;
            self.zero_cost_attacks += i32::from(card.card_type() == CardType::ATTACK);
        }
        self.expensive += i32::from(card.cost >= 2);
        self.x_cost += i32::from(card.cost < 0);
        self.energy += i32::from(is_energy_source(card.id));
        match card.id {
            CardId::Biased_Cognition => self.biased_cognition += 1,
            CardId::Blizzard => self.blizzard += 1,
            CardId::All_For_One => self.all_for_one += 1,
            CardId::Scrape => self.scrape += 1,
            CardId::Gash => self.claw += 1,
            CardId::Thunder_Strike => self.thunder_strike += 1,
            CardId::Heatsinks | CardId::Storm | CardId::Force_Field => self.power_payoffs += 1,
            _ => {}
        }
    }
}

struct DeckPlan<'a> {
    game: &'a Game,
    profile: DeckProfile,
}

impl<'a> DeckPlan<'a> {
    fn new(game: &'a Game) -> Self {
        Self {
            game,
            profile: DeckProfile::from_game(game),
        }
    }

    fn active_tasks(&self) -> Vec<DeckTask> {
        let p = self.profile;
        let mut tasks = vec![DeckTask::EstablishOrbCore];
        if p.frost > 0 || p.blizzard > 0 {
            tasks.push(DeckTask::ConvertFrost);
        }
        if p.lightning > 0 || p.thunder_strike > 0 {
            tasks.push(DeckTask::ConvertLightning);
        }
        if p.powers >= 2
            || p.power_payoffs > 0
            || self.has(RelicId::Mummified_Hand)
            || self.has(RelicId::Bird_Faced_Urn)
        {
            tasks.push(DeckTask::ScalePowers);
        }
        if p.zero_cost >= 2
            || p.all_for_one > 0
            || p.scrape > 0
            || p.claw > 0
            || self.attack_chain_relics() > 0
        {
            tasks.push(DeckTask::ExploitZeroCost);
        }
        if p.energy > 0
            || p.expensive >= 3
            || self.has(RelicId::Nuclear_Battery)
            || self.has(RelicId::Snecko_Eye)
        {
            tasks.push(DeckTask::FundExpensiveCards);
        }
        if matches!(self.game.dungeon.act, Act::Exordium | Act::City) {
            tasks.push(DeckTask::PrepareForBoss);
        }
        tasks
    }

    fn card_adjustment(&self, card: &Card) -> i32 {
        let mut value = self.orphan_payoff_adjustment(card.id);
        for task in self.active_tasks() {
            value += self.task_card_value(task, card);
        }
        value + self.relic_card_value(card)
    }

    fn task_card_value(&self, task: DeckTask, card: &Card) -> i32 {
        let p = self.profile;
        match task {
            DeckTask::EstablishOrbCore => {
                if matches!(card.id, CardId::Defragment | CardId::Biased_Cognition) {
                    5 * p.channel.min(7) + 3 * p.frost.min(5)
                } else if is_channel(card.id) {
                    8 * (p.focus + i32::from(self.has(RelicId::DataDisk))).min(4)
                        + 3 * (p.orb_slots + self.orb_slot_relics()).min(4)
                } else if card.id == CardId::Capacitor {
                    10 * (p.channel - 2).clamp(0, 6) + 8 * p.focus.min(3) - 35
                } else if card.id == CardId::Consume {
                    45 * (p.orb_slots + self.orb_slot_relics()).min(3) + 5 * p.channel.min(6)
                } else {
                    0
                }
            }
            DeckTask::ConvertFrost => {
                if is_frost_source(card.id) {
                    5 * p.focus.min(4) + 4 * (p.orb_slots + self.orb_slot_relics()).min(4)
                } else if card.id == CardId::Blizzard {
                    38 * p.frost.min(5) + 8 * p.focus.min(3)
                } else {
                    0
                }
            }
            DeckTask::ConvertLightning => {
                if is_lightning_source(card.id) {
                    5 * p.focus.min(4) + if p.thunder_strike > 0 { 14 } else { 0 }
                } else if card.id == CardId::Thunder_Strike {
                    20 * p.lightning.min(6)
                } else if card.id == CardId::Electrodynamics {
                    6 * p.lightning.min(5)
                } else {
                    0
                }
            }
            DeckTask::ScalePowers => {
                if card.card_type() == CardType::POWER {
                    8 * p.power_payoffs.min(3)
                        + if self.has(RelicId::Mummified_Hand) {
                            28
                        } else {
                            0
                        }
                        + if self.has(RelicId::Bird_Faced_Urn) {
                            16
                        } else {
                            0
                        }
                } else if matches!(
                    card.id,
                    CardId::Heatsinks | CardId::Storm | CardId::Force_Field
                ) {
                    13 * p.powers.min(7)
                } else {
                    0
                }
            }
            DeckTask::ExploitZeroCost => {
                if card.id == CardId::All_For_One {
                    16 * p.zero_cost.min(7)
                } else if card.id == CardId::Scrape {
                    10 * p.zero_cost.min(7)
                } else if card.id == CardId::Gash {
                    35 * p.claw.min(3)
                        + 45 * (p.all_for_one + p.scrape).min(2)
                        + 5 * self.attack_chain_relics()
                } else if card.cost == 0 {
                    18 * (p.all_for_one + p.scrape).min(3) + 5 * self.attack_chain_relics()
                } else {
                    0
                }
            }
            DeckTask::FundExpensiveCards => {
                if card.cost >= 2 {
                    9 * (p.energy + i32::from(self.has(RelicId::Nuclear_Battery))).min(4)
                } else if is_energy_source(card.id) {
                    5 * p.expensive.min(6)
                } else {
                    0
                }
            }
            DeckTask::PrepareForBoss => self.boss_card_value(card.id),
        }
    }

    fn boss_card_value(&self, id: CardId) -> i32 {
        let p = self.profile;
        match self.game.dungeon.boss {
            EncounterId::Hexaghost => match id {
                CardId::Glacier => 50,
                CardId::Defragment => 30,
                CardId::Go_for_the_Eyes | CardId::Ball_Lightning | CardId::Doom_and_Gloom => 30,
                CardId::Cold_Snap => 25,
                _ => 0,
            },
            EncounterId::TheGuardian => match id {
                CardId::Glacier => 60,
                CardId::Reinforced_Body => 45,
                CardId::Auto_Shields => 35,
                CardId::Cold_Snap | CardId::Leap => 30,
                CardId::Defragment => 25,
                CardId::Go_for_the_Eyes => 20,
                _ => 0,
            },
            EncounterId::SlimeBoss => match id {
                CardId::Hyperbeam => 70,
                CardId::Electrodynamics => 60,
                CardId::Sunder => 50,
                CardId::Sweeping_Beam => 45,
                CardId::Doom_and_Gloom => 35,
                CardId::Ball_Lightning => 25,
                _ => 0,
            },
            EncounterId::Collector => {
                let missing_aoe = (2 - p.aoe).max(0);
                match id {
                    CardId::Electrodynamics => 45 * missing_aoe,
                    CardId::Hyperbeam => 30 * missing_aoe,
                    CardId::Sweeping_Beam => 24 * missing_aoe,
                    CardId::Doom_and_Gloom => 18 * missing_aoe,
                    CardId::Chill => 20 * missing_aoe,
                    _ => 0,
                }
            }
            EncounterId::Automaton => match id {
                CardId::Buffer => 75,
                CardId::Reinforced_Body => 35,
                CardId::Glacier => 25,
                CardId::BootSequence => 20,
                CardId::Doom_and_Gloom | CardId::Darkness => 25,
                CardId::Echo_Form => 30,
                _ => 0,
            },
            EncounterId::Champ => match id {
                CardId::Echo_Form => 50,
                CardId::Defragment => 30,
                CardId::Doom_and_Gloom | CardId::Darkness => 35,
                CardId::Loop | CardId::Creative_AI => 25,
                CardId::Biased_Cognition => 20,
                _ => 0,
            },
            _ => 0,
        }
    }

    fn orphan_payoff_adjustment(&self, id: CardId) -> i32 {
        let p = self.profile;
        match id {
            CardId::Blizzard => -180,
            CardId::Consume => -105,
            CardId::Barrage => -55 + 10 * p.channel.min(6),
            CardId::Thunder_Strike => -90,
            CardId::All_For_One => -85,
            CardId::Scrape => -55,
            CardId::Heatsinks => -65,
            CardId::Storm => -55,
            CardId::Force_Field => -45,
            CardId::Meteor_Strike => -120 + 35 * p.energy.min(3),
            CardId::Gash => -50,
            CardId::Tempest => -70 + 20 * p.energy.min(3),
            CardId::Fusion => -45,
            _ => 0,
        }
    }

    fn relic_card_value(&self, card: &Card) -> i32 {
        let mut value = 0;
        if self.has(RelicId::Snecko_Eye) {
            value += match card.cost {
                3.. => 45,
                2 => 22,
                0 => -30,
                _ => 0,
            };
        }
        if self.has(RelicId::Velvet_Choker) {
            if card.cost == 0 {
                value -= 28;
            } else if card.cost >= 2 {
                value += 8;
            }
        }
        if self.has(RelicId::Chemical_X) && card.cost < 0 {
            value += 80;
        }
        if self.has(RelicId::OrangePellets) {
            if card.id == CardId::Biased_Cognition {
                value += 70;
            } else if card.card_type() == CardType::POWER {
                value += 8;
            }
        }
        if self.has(RelicId::Cables)
            && matches!(card.id, CardId::Loop | CardId::Darkness | CardId::Glacier)
        {
            value += 18;
        }
        if self.has(RelicId::Symbiotic_Virus)
            && matches!(
                card.id,
                CardId::Darkness | CardId::Doom_and_Gloom | CardId::Loop
            )
        {
            value += 20;
        }
        value
    }

    fn has(&self, id: RelicId) -> bool {
        self.game.player.has_relic(id)
    }

    fn orb_slot_relics(&self) -> i32 {
        i32::from(self.has(RelicId::Runic_Capacitor)) + i32::from(self.has(RelicId::Inserter))
    }

    fn attack_chain_relics(&self) -> i32 {
        [
            RelicId::Kunai,
            RelicId::Shuriken,
            RelicId::Ornamental_Fan,
            RelicId::Nunchaku,
            RelicId::InkBottle,
        ]
        .into_iter()
        .filter(|id| self.has(*id))
        .count() as i32
    }
}

pub fn card_adjustment(game: &Game, card: &Card) -> i32 {
    DeckPlan::new(game).card_adjustment(card)
}

pub fn shop_relic_value(game: &Game, id: RelicId) -> i32 {
    let plan = DeckPlan::new(game);
    let p = plan.profile;
    let base = match id {
        RelicId::Strange_Spoon => 45,
        RelicId::PrismaticShard => 20,
        RelicId::HandDrill => 55,
        RelicId::TheAbacus | RelicId::Medical_Kit => 180,
        _ => 130,
    };
    base + match id {
        RelicId::Runic_Capacitor | RelicId::Inserter => {
            12 * p.channel.min(7) + 8 * p.focus.min(4) - if p.channel < 2 { 45 } else { 0 }
        }
        RelicId::DataDisk => 10 * p.channel.min(7) + 6 * p.frost.min(5),
        RelicId::Cables => 8 * p.channel.min(7) + 12 * (p.dark + p.frost).min(4),
        RelicId::Symbiotic_Virus => 20 * (p.dark + p.blizzard).min(3),
        RelicId::Mummified_Hand => 18 * p.powers.min(8),
        RelicId::Bird_Faced_Urn => 12 * p.powers.min(8),
        RelicId::OrangePellets => 8 * p.powers.min(7) + if p.biased_cognition > 0 { 70 } else { 0 },
        RelicId::Chemical_X => 75 * p.x_cost.min(3) - if p.x_cost == 0 { 60 } else { 0 },
        RelicId::Kunai | RelicId::Shuriken | RelicId::Ornamental_Fan => {
            12 * p.zero_cost_attacks.min(6)
        }
        RelicId::Nunchaku | RelicId::InkBottle | RelicId::Unceasing_Top => 8 * p.zero_cost.min(7),
        RelicId::Frozen_Egg_2 => 9 * p.powers.min(7),
        RelicId::Toxic_Egg_2 => 6 * p.skills.min(10),
        RelicId::Molten_Egg_2 => 6 * p.attacks.min(10),
        _ => 0,
    }
}

pub fn shop_purge_value(game: &Game) -> i32 {
    let curses = game
        .player
        .deck
        .iter()
        .filter(|card| card.card_type() == CardType::CURSE)
        .count() as i32;
    if curses > 0 {
        return 240;
    }

    let p = DeckProfile::from_game(game);
    if p.strikes == 0 {
        return 60;
    }
    let replacement_attacks = (p.attacks - p.strikes).max(0);
    let engine_ready = p.channel >= 4 && (p.focus > 0 || p.powers >= 3);
    130 + 12 * p.strikes + 8 * replacement_attacks.min(6) + if engine_ready { 30 } else { 0 }
        - if game.dungeon.act as i32 == 1 && replacement_attacks < 4 {
            45
        } else {
            0
        }
}

fn is_channel(id: CardId) -> bool {
    matches!(
        id,
        CardId::Zap
            | CardId::Ball_Lightning
            | CardId::Cold_Snap
            | CardId::Coolheaded
            | CardId::Glacier
            | CardId::Doom_and_Gloom
            | CardId::Rainbow
            | CardId::Darkness
            | CardId::Fusion
            | CardId::Tempest
            | CardId::Meteor_Strike
            | CardId::Chill
            | CardId::Electrodynamics
    )
}

fn is_aoe_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Electrodynamics
            | CardId::Hyperbeam
            | CardId::Sweeping_Beam
            | CardId::Doom_and_Gloom
            | CardId::Blizzard
            | CardId::Chill
    )
}

fn is_frost_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Cold_Snap | CardId::Coolheaded | CardId::Glacier | CardId::Rainbow | CardId::Chill
    )
}

fn is_lightning_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Zap
            | CardId::Ball_Lightning
            | CardId::Electrodynamics
            | CardId::Tempest
            | CardId::Rainbow
    )
}

fn is_dark_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Darkness | CardId::Doom_and_Gloom | CardId::Rainbow
    )
}

fn is_energy_source(id: CardId) -> bool {
    matches!(
        id,
        CardId::Turbo
            | CardId::Double_Energy
            | CardId::Fusion
            | CardId::Recycle
            | CardId::Conserve_Battery
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creature::RelicInstance;
    use crate::ids::Character;
    use crate::unlocks::Unlocks;

    fn add_relic(game: &mut Game, id: RelicId) {
        game.player.relics.push(RelicInstance {
            id,
            counter: -1,
            used_up: false,
        });
    }

    #[test]
    fn payoff_cards_gain_value_only_after_their_package_exists() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let blizzard = Card::new(CardId::Blizzard);
        let unsupported = card_adjustment(&game, &blizzard);
        game.player.deck.push(Card::new(CardId::Cold_Snap));
        game.player.deck.push(Card::new(CardId::Coolheaded));
        game.player.deck.push(Card::new(CardId::Glacier));
        assert!(card_adjustment(&game, &blizzard) > unsupported + 100);
    }

    #[test]
    fn owned_relics_change_the_card_plan() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.player.deck.push(Card::new(CardId::Defragment));
        game.player.deck.push(Card::new(CardId::Machine_Learning));
        let heatsinks = Card::new(CardId::Heatsinks);
        let without = card_adjustment(&game, &heatsinks);
        add_relic(&mut game, RelicId::Mummified_Hand);
        assert!(card_adjustment(&game, &heatsinks) > without);
    }

    #[test]
    fn shop_relics_are_scored_against_the_current_deck() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let empty = shop_relic_value(&game, RelicId::Runic_Capacitor);
        game.player.deck.push(Card::new(CardId::Cold_Snap));
        game.player.deck.push(Card::new(CardId::Coolheaded));
        game.player.deck.push(Card::new(CardId::Ball_Lightning));
        assert!(shop_relic_value(&game, RelicId::Runic_Capacitor) > empty + 30);
    }

    #[test]
    fn orphan_cards_wait_for_their_enabling_package() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let claw = Card::new(CardId::Gash);
        let unsupported = card_adjustment(&game, &claw);
        game.player.deck.push(Card::new(CardId::All_For_One));
        assert!(card_adjustment(&game, &claw) > unsupported + 40);

        let tempest = Card::new(CardId::Tempest);
        let unsupported = card_adjustment(&game, &tempest);
        game.player.deck.push(Card::new(CardId::Turbo));
        game.player.deck.push(Card::new(CardId::Double_Energy));
        assert!(card_adjustment(&game, &tempest) > unsupported + 30);
    }

    #[test]
    fn strike_purge_waits_until_replacement_damage_and_engine_exist() {
        use crate::ids::Act;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let early = shop_purge_value(&game);
        game.dungeon.act = Act::City;
        game.player.deck.push(Card::new(CardId::Cold_Snap));
        game.player.deck.push(Card::new(CardId::Ball_Lightning));
        game.player.deck.push(Card::new(CardId::Sweeping_Beam));
        game.player.deck.push(Card::new(CardId::Doom_and_Gloom));
        game.player.deck.push(Card::new(CardId::Defragment));
        assert!(shop_purge_value(&game) > early + 80);
    }

    #[test]
    fn known_act_two_boss_changes_the_acquisition_task() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        let electrodynamics = Card::new(CardId::Electrodynamics);
        game.dungeon.boss = EncounterId::Champ;
        let champ = card_adjustment(&game, &electrodynamics);
        game.dungeon.boss = EncounterId::Collector;
        assert!(card_adjustment(&game, &electrodynamics) >= champ + 90);
    }

    #[test]
    fn known_act_one_boss_changes_the_acquisition_task() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::Exordium;
        let glacier = Card::new(CardId::Glacier);
        game.dungeon.boss = EncounterId::SlimeBoss;
        let slime = card_adjustment(&game, &glacier);
        game.dungeon.boss = EncounterId::TheGuardian;
        assert!(card_adjustment(&game, &glacier) >= slime + 60);
    }
}
