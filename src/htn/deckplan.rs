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
    ConvertDark,
    ScalePowers,
    ExploitZeroCost,
    FundExpensiveCards,
    PrepareForBoss,
}

/// Mutually exclusive scaling packages.  A reward can contribute to several
/// low-level tasks, but prior correction should follow one coherent plan
/// instead of adding every plausible archetype together.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeckPackage {
    OrbFocus,
    Dark,
    Powers,
    ZeroCost,
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
    loop_cards: i32,
    recursion: i32,
    multi_cast: i32,
    hologram: i32,
    seek: i32,
    echo_form: i32,
    creative_ai: i32,
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
            CardId::Loop => self.loop_cards += 1,
            // Slay the Spire's internal id for Recursion is "Redo".
            CardId::Redo => self.recursion += 1,
            CardId::Multi_Cast => self.multi_cast += 1,
            CardId::Hologram => self.hologram += 1,
            CardId::Seek => self.seek += 1,
            CardId::Echo_Form => self.echo_form += 1,
            CardId::Creative_AI => self.creative_ai += 1,
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
        if self.game.dungeon.act != Act::Exordium
            && matches!(self.primary_package(), Some(DeckPackage::Dark))
        {
            tasks.push(DeckTask::ConvertDark);
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
        if matches!(
            self.game.dungeon.act,
            Act::Exordium | Act::City | Act::Beyond
        ) {
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

    /// Make the deck plan authoritative only when the deck supplies evidence
    /// for it. `contextual_score` contains the learned prior and immediate
    /// deck-need bonuses; `stable_prior` is the conservative static value used
    /// solely for cards that complete the committed package.
    fn planned_card_score(&self, card: &Card, contextual_score: i32, stable_prior: i32) -> i32 {
        let adjustment = self.card_adjustment(card);
        let mut score = contextual_score + adjustment;

        // Act 1 is primarily a tempo test.  Do not let a speculative package
        // suppress the attacks needed to reach the first boss.
        if self.game.dungeon.act == Act::Exordium {
            return score;
        }

        let commitment = self.primary_package();
        let package_bonus =
            commitment.and_then(|package| self.package_completion_bonus(package, card.id));

        if let Some(bonus) = package_bonus {
            score = score.max(stable_prior + adjustment + bonus);
        }
        score
    }

    fn primary_package(&self) -> Option<DeckPackage> {
        let p = self.profile;
        let orb_relics = i32::from(self.has(RelicId::DataDisk))
            + self.orb_slot_relics()
            + i32::from(self.has(RelicId::FrozenCore));
        let dark_relics =
            i32::from(self.has(RelicId::Cables)) + i32::from(self.has(RelicId::Symbiotic_Virus));
        let power_relics = i32::from(self.has(RelicId::Mummified_Hand))
            + i32::from(self.has(RelicId::Bird_Faced_Urn))
            + i32::from(self.has(RelicId::OrangePellets));

        let candidates = [
            (
                DeckPackage::OrbFocus,
                p.channel + 3 * p.frost + 2 * p.lightning + 5 * p.focus + 3 * orb_relics,
            ),
            (
                DeckPackage::Dark,
                8 * p.dark
                    + 5 * (p.loop_cards + p.recursion + p.multi_cast)
                    + 2 * (p.hologram + p.seek)
                    + 5 * dark_relics,
            ),
            (
                DeckPackage::Powers,
                2 * p.powers
                    + 5 * p.power_payoffs
                    + 4 * (p.echo_form + p.creative_ai)
                    + 6 * power_relics,
            ),
            (
                DeckPackage::ZeroCost,
                3 * p.zero_cost
                    + 8 * (p.all_for_one + p.scrape)
                    + 4 * p.claw
                    + 3 * self.attack_chain_relics(),
            ),
        ];
        let (package, strength) = candidates
            .into_iter()
            .max_by_key(|(_, strength)| *strength)?;
        (strength >= 8).then_some(package)
    }

    fn package_completion_bonus(&self, package: DeckPackage, id: CardId) -> Option<i32> {
        let p = self.profile;
        match package {
            DeckPackage::OrbFocus => match id {
                CardId::Defragment | CardId::Biased_Cognition if p.focus == 0 && p.channel >= 3 => {
                    Some(45)
                }
                CardId::Glacier | CardId::Coolheaded | CardId::Cold_Snap | CardId::Chill
                    if p.focus > 0 && p.frost < 2 =>
                {
                    Some(25)
                }
                CardId::Capacitor
                    if p.focus > 0
                        && p.channel >= 3
                        && p.orb_slots + self.orb_slot_relics() == 0 =>
                {
                    Some(65)
                }
                CardId::Consume if p.focus == 0 && p.orb_slots + self.orb_slot_relics() > 0 => {
                    Some(55)
                }
                _ => None,
            },
            DeckPackage::Dark => match id {
                CardId::Darkness | CardId::Doom_and_Gloom | CardId::Rainbow if p.dark == 0 => {
                    Some(80)
                }
                CardId::Multi_Cast if p.dark > 0 && p.multi_cast + p.recursion == 0 => Some(140),
                CardId::Redo if p.dark > 0 && p.multi_cast + p.recursion == 0 => Some(105),
                _ => None,
            },
            DeckPackage::Powers => match id {
                CardId::Heatsinks | CardId::Storm | CardId::Force_Field
                    if p.powers >= 3 && p.power_payoffs == 0 =>
                {
                    Some(195)
                }
                CardId::Creative_AI if p.power_payoffs > 0 && p.creative_ai + p.echo_form == 0 => {
                    Some(130)
                }
                CardId::Echo_Form if p.power_payoffs > 0 && p.echo_form == 0 => Some(55),
                _ => None,
            },
            DeckPackage::ZeroCost => match id {
                CardId::All_For_One if p.zero_cost >= 3 && p.all_for_one + p.scrape == 0 => {
                    Some(155)
                }
                CardId::Scrape if p.zero_cost >= 3 && p.all_for_one + p.scrape == 0 => Some(65),
                CardId::Gash if p.all_for_one + p.scrape > 0 && p.claw < 3 => {
                    Some(25 + 35 * (p.all_for_one + p.scrape).min(2))
                }
                _ => None,
            },
        }
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
            DeckTask::ConvertDark => {
                let release = p.recursion + p.multi_cast;
                if is_dark_source(card.id) && p.dark == 0 && release > 0 {
                    30 * release.min(2)
                } else if card.id == CardId::Multi_Cast && p.dark > 0 && release == 0 {
                    45 * p.dark.min(3)
                } else if card.id == CardId::Redo && p.dark > 0 && release == 0 {
                    35 * p.dark.min(3)
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
            EncounterId::TimeEater => match id {
                // Prefer cards that compress a turn's output into a few plays
                // and preserve answers across the twelve-card boundary.
                CardId::Echo_Form => 55,
                CardId::Glacier | CardId::Buffer => 45,
                CardId::Reinforced_Body => 40,
                CardId::Doom_and_Gloom | CardId::Darkness => 35,
                CardId::Defragment => 35,
                CardId::Hologram => 25,
                _ => 0,
            },
            EncounterId::AwakenedOne => match id {
                // Non-Power scaling can be established in phase one without
                // permanently increasing every remaining multi-hit attack.
                CardId::Doom_and_Gloom | CardId::Darkness => 50,
                CardId::Glacier => 45,
                CardId::Reinforced_Body => 40,
                CardId::Core_Surge | CardId::Genetic_Algorithm => 35,
                CardId::Buffer => 25,
                CardId::Creative_AI | CardId::Storm | CardId::Machine_Learning => -25,
                _ => 0,
            },
            EncounterId::DonuAndDeca => match id {
                // Focused banks remove Donu before repeated Strength buffs;
                // efficient two-target output and durable block then handle
                // Deca's Dazed/block cycle.
                CardId::Doom_and_Gloom | CardId::Darkness => 55,
                CardId::Electrodynamics => 50,
                CardId::Glacier => 45,
                CardId::Reinforced_Body | CardId::Buffer => 35,
                CardId::Sunder => 30,
                CardId::Defragment => 25,
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

/// Resolve the learned reward score through the currently committed deck
/// package.  The stable prior is not a global floor: it is considered only
/// when this exact card fills a missing role in the selected package.
pub fn planned_card_score(
    game: &Game,
    card: &Card,
    contextual_score: i32,
    stable_prior: i32,
) -> i32 {
    DeckPlan::new(game).planned_card_score(card, contextual_score, stable_prior)
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
        - if !strike_purge_ready(game) { 45 } else { 0 }
}

/// Whether removing a basic Strike no longer leaves the Act 1 deck without a
/// reliable damage floor. Later acts may purge normally; before the first boss
/// retain at least two Strikes until replacement attacks or a real orb engine
/// exist.
pub(crate) fn strike_purge_ready(game: &Game) -> bool {
    let p = DeckProfile::from_game(game);
    if p.strikes == 0 || game.dungeon.act != Act::Exordium {
        return true;
    }
    if p.strikes > 2 {
        return true;
    }
    let replacement_attacks = (p.attacks - p.strikes).max(0);
    let engine_ready = p.channel >= 4 && (p.focus > 0 || p.powers >= 3);
    replacement_attacks >= 4 || engine_ready
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
    fn act_one_keeps_learned_tempo_authoritative() {
        let game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let hologram = Card::new(CardId::Hologram);
        let learned = 2;
        assert_eq!(
            planned_card_score(&game, &hologram, learned, 60),
            learned + card_adjustment(&game, &hologram)
        );
    }

    #[test]
    fn dark_commitment_can_override_a_bad_operator_prior() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        game.player.deck.push(Card::new(CardId::Doom_and_Gloom));
        game.player.deck.push(Card::new(CardId::Loop));

        let multi_cast = Card::new(CardId::Multi_Cast);
        let filler = Card::new(CardId::Go_for_the_Eyes);
        assert!(
            planned_card_score(&game, &multi_cast, 0, 40)
                > planned_card_score(&game, &filler, 211, 115)
        );
    }

    #[test]
    fn orb_commitment_recovers_its_missing_focus_without_a_global_floor() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        game.player.deck.push(Card::new(CardId::Cold_Snap));
        game.player.deck.push(Card::new(CardId::Coolheaded));
        game.player.deck.push(Card::new(CardId::Glacier));

        let focus = Card::new(CardId::Biased_Cognition);
        let filler = Card::new(CardId::Go_for_the_Eyes);
        assert!(
            planned_card_score(&game, &focus, 0, 150)
                > planned_card_score(&game, &filler, 211, 115)
        );
    }

    #[test]
    fn focused_orb_package_seeks_capacity_once_channels_fill_the_row() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        for id in [
            CardId::Cold_Snap,
            CardId::Coolheaded,
            CardId::Ball_Lightning,
            CardId::Defragment,
        ] {
            game.player.deck.push(Card::new(id));
        }

        let plan = DeckPlan::new(&game);
        assert_eq!(plan.primary_package(), Some(DeckPackage::OrbFocus));
        assert_eq!(
            plan.package_completion_bonus(DeckPackage::OrbFocus, CardId::Capacitor),
            Some(65)
        );
    }

    #[test]
    fn completed_orb_role_does_not_create_a_global_floor() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        for id in [
            CardId::Cold_Snap,
            CardId::Coolheaded,
            CardId::Glacier,
            CardId::Defragment,
        ] {
            game.player.deck.push(Card::new(id));
        }

        let focus = Card::new(CardId::Biased_Cognition);
        assert_eq!(
            planned_card_score(&game, &focus, 0, 150),
            card_adjustment(&game, &focus)
        );
    }

    #[test]
    fn power_commitment_recovers_its_missing_payoff() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        for id in [
            CardId::Machine_Learning,
            CardId::Buffer,
            CardId::Static_Discharge,
            CardId::Capacitor,
        ] {
            game.player.deck.push(Card::new(id));
        }

        let payoff = Card::new(CardId::Heatsinks);
        let filler = Card::new(CardId::Go_for_the_Eyes);
        assert!(
            planned_card_score(&game, &payoff, 0, 90)
                > planned_card_score(&game, &filler, 211, 115)
        );
    }

    #[test]
    fn zero_cost_commitment_recovers_all_for_one() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::City;
        for id in [CardId::Beam_Cell, CardId::FTL, CardId::Gash] {
            game.player.deck.push(Card::new(id));
        }

        let payoff = Card::new(CardId::All_For_One);
        let filler = Card::new(CardId::Go_for_the_Eyes);
        assert!(
            planned_card_score(&game, &payoff, 0, 100)
                > planned_card_score(&game, &filler, 211, 115)
        );
    }

    #[test]
    fn strike_purge_waits_until_replacement_damage_and_engine_exist() {
        use crate::ids::Act;

        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        let early = shop_purge_value(&game);
        assert!(strike_purge_ready(&game));
        let mut removed = 0;
        game.player.deck.retain(|card| {
            if card.id == CardId::Strike_B && removed < 2 {
                removed += 1;
                false
            } else {
                true
            }
        });
        assert!(!strike_purge_ready(&game));
        game.player.deck.push(Card::new(CardId::Cold_Snap));
        game.player.deck.push(Card::new(CardId::Ball_Lightning));
        game.player.deck.push(Card::new(CardId::Sweeping_Beam));
        game.player.deck.push(Card::new(CardId::Doom_and_Gloom));
        game.player.deck.push(Card::new(CardId::Defragment));
        assert!(strike_purge_ready(&game));
        game.dungeon.act = Act::City;
        assert!(shop_purge_value(&game) > early);
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

    #[test]
    fn known_act_three_boss_changes_the_acquisition_task() {
        let mut game = Game::new(2, Character::Defect, 0, Unlocks::fixture());
        game.dungeon.act = Act::Beyond;

        let electrodynamics = Card::new(CardId::Electrodynamics);
        game.dungeon.boss = EncounterId::AwakenedOne;
        let awakened = card_adjustment(&game, &electrodynamics);
        game.dungeon.boss = EncounterId::DonuAndDeca;
        assert!(card_adjustment(&game, &electrodynamics) >= awakened + 50);

        let creative_ai = Card::new(CardId::Creative_AI);
        game.dungeon.boss = EncounterId::TimeEater;
        let time_eater = card_adjustment(&game, &creative_ai);
        game.dungeon.boss = EncounterId::AwakenedOne;
        assert!(card_adjustment(&game, &creative_ai) <= time_eater - 5);

        let glacier = Card::new(CardId::Glacier);
        game.dungeon.boss = EncounterId::TimeEater;
        assert!(card_adjustment(&game, &glacier) >= 45);
        game.dungeon.boss = EncounterId::AwakenedOne;
        assert!(card_adjustment(&game, &glacier) >= 45);
    }
}
