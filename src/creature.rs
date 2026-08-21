use crate::ids::{MonsterId, PowerId};

#[derive(Clone, Debug)]
pub struct Power {
    pub id: PowerId,
    pub amount: i32,
    pub just_applied: bool,
    pub skip_first: bool,
    pub misc: i32,
}

#[derive(Clone, Debug)]
pub struct RelicInstance {
    pub id: crate::ids::RelicId,
    pub counter: i32,
    pub used_up: bool,
}

#[derive(Clone, Debug)]
pub struct PotionInstance {
    pub id: crate::ids::PotionId,
    pub slot: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrbKind {
    Lightning,
    Frost,
    Dark,
    Plasma,
}

#[derive(Clone, Copy, Debug)]
pub struct Orb {
    pub kind: OrbKind,
    /// Dark orbs accumulate evoke damage; other orbs ignore this.
    pub evoke: i32,
}

#[derive(Clone, Debug)]
pub struct Player {
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub gold: i32,
    pub energy: i32,
    pub energy_master: i32,
    pub potion_slots: i32,
    pub relics: Vec<RelicInstance>,
    pub potions: Vec<PotionInstance>,
    pub powers: Vec<Power>,
    pub deck: Vec<crate::card::Card>,
    pub draw: Vec<crate::card::Card>,
    pub hand: Vec<crate::card::Card>,
    pub discard: Vec<crate::card::Card>,
    pub exhaust: Vec<crate::card::Card>,
    pub duplication: i32,
    /// StaticDischargePower.onAttacked: Lightning channels queued during a hit.
    pub pending_static: i32,
    /// Lightning/Dark evoke amounts deferred from mid-hit Static Discharge
    /// channels. A Frost following deferred Lightning in the same one-hit
    /// action is deferred too, preserving the ChannelAction queue order.
    /// Flushed after take_turn so `channel_orb` can borrow Combat.
    pub pending_evoke_lightning: Vec<i32>,
    pub pending_evoke_frost: Vec<i32>,
    pub pending_evoke_dark: Vec<i32>,
    pub orbs: Vec<Orb>,
    /// Combat orb slots (`AbstractPlayer.maxOrbs`). Reset from `master_max_orbs` in
    /// `preBattlePrep`; Capacitor / Consume mutate this only for the current fight.
    pub max_orbs: i32,
    /// Persistent orb-slot cap (`AbstractPlayer.masterMaxOrbs`). Defect loadout is 3.
    pub master_max_orbs: i32,
}

impl Player {
    pub fn ironclad() -> Self {
        use crate::card::Card;
        use crate::ids::{CardId, PotionId, RelicId};
        Self {
            hp: 80,
            max_hp: 80,
            block: 0,
            gold: 99,
            energy: 0,
            energy_master: 3,
            potion_slots: 3,
            relics: vec![RelicInstance {
                id: RelicId::Burning_Blood,
                counter: -1,
                used_up: false,
            }],
            potions: (0..3)
                .map(|slot| PotionInstance {
                    id: PotionId::Slot,
                    slot,
                })
                .collect(),
            powers: Vec::new(),
            deck: vec![
                Card::new(CardId::Strike_R),
                Card::new(CardId::Strike_R),
                Card::new(CardId::Strike_R),
                Card::new(CardId::Strike_R),
                Card::new(CardId::Strike_R),
                Card::new(CardId::Defend_R),
                Card::new(CardId::Defend_R),
                Card::new(CardId::Defend_R),
                Card::new(CardId::Defend_R),
                Card::new(CardId::Bash),
            ],
            draw: Vec::new(),
            hand: Vec::new(),
            discard: Vec::new(),
            exhaust: Vec::new(),
            duplication: 0,
            pending_static: 0,
            pending_evoke_lightning: Vec::new(),
            pending_evoke_frost: Vec::new(),
            pending_evoke_dark: Vec::new(),
            orbs: Vec::new(),
            max_orbs: 0,
            master_max_orbs: 0,
        }
    }

    pub fn defect() -> Self {
        use crate::card::Card;
        use crate::ids::{CardId, PotionId, RelicId};
        Self {
            hp: 75,
            max_hp: 75,
            block: 0,
            gold: 99,
            energy: 0,
            energy_master: 3,
            potion_slots: 3,
            relics: vec![RelicInstance {
                id: RelicId::Cracked_Core,
                counter: -1,
                used_up: false,
            }],
            potions: (0..3)
                .map(|slot| PotionInstance {
                    id: PotionId::Slot,
                    slot,
                })
                .collect(),
            powers: Vec::new(),
            deck: vec![
                Card::new(CardId::Strike_B),
                Card::new(CardId::Strike_B),
                Card::new(CardId::Strike_B),
                Card::new(CardId::Strike_B),
                Card::new(CardId::Defend_B),
                Card::new(CardId::Defend_B),
                Card::new(CardId::Defend_B),
                Card::new(CardId::Defend_B),
                Card::new(CardId::Zap),
                Card::new(CardId::Dualcast),
            ],
            draw: Vec::new(),
            hand: Vec::new(),
            discard: Vec::new(),
            exhaust: Vec::new(),
            duplication: 0,
            pending_static: 0,
            pending_evoke_lightning: Vec::new(),
            pending_evoke_frost: Vec::new(),
            pending_evoke_dark: Vec::new(),
            orbs: Vec::new(),
            max_orbs: 3,
            master_max_orbs: 3,
        }
    }

    pub fn for_character(character: crate::ids::Character) -> Self {
        match character {
            crate::ids::Character::Defect => Self::defect(),
            _ => Self::ironclad(),
        }
    }

    /// AbstractDungeon.initialize after floor 1 Exordium: A14 max HP, A6 start HP, A10 curse.
    pub fn apply_ascension(&mut self, character: crate::ids::Character, ascension: i32) {
        if ascension >= 14 {
            let loss = match character {
                crate::ids::Character::Ironclad => 5,
                _ => 4,
            };
            self.max_hp = (self.max_hp - loss).max(1);
            if self.hp > self.max_hp {
                self.hp = self.max_hp;
            }
        }
        if ascension >= 6 {
            // MathUtils.round(maxHealth * 0.9F)
            self.hp = (self.max_hp as f32 * 0.9 + 0.5).floor() as i32;
        }
        if ascension >= 10 {
            // Observed ExactTextSim master_deck lists AscendersBane first.
            self.deck.insert(0, crate::card::Card::new(crate::ids::CardId::AscendersBane));
        }
    }

    pub fn power_amount(&self, id: PowerId) -> i32 {
        self.powers.iter().find(|p| p.id == id).map(|p| p.amount).unwrap_or(0)
    }

    pub fn add_power(&mut self, id: PowerId, amount: i32) {
        self.apply_power(id, amount, false);
    }

    pub fn add_power_from_monster(&mut self, id: PowerId, amount: i32) {
        self.apply_power(id, amount, true);
    }

    /// ApplyPowerAction: Ginger blocks Weak, Turnip blocks Frail, Artifact blocks debuffs.
    fn apply_power(&mut self, id: PowerId, amount: i32, from_monster: bool) {
        if id == PowerId::Weak && self.has_relic(crate::ids::RelicId::Ginger) {
            return;
        }
        if id == PowerId::Frail && self.has_relic(crate::ids::RelicId::Turnip) {
            return;
        }
        absorb_or_add_power(&mut self.powers, id, amount, from_monster);
    }

    pub fn has_relic(&self, id: crate::ids::RelicId) -> bool {
        self.relics.iter().any(|r| r.id == id)
    }
}

/// ApplyPowerAction: Artifact absorbs `powerToApply.type == DEBUFF`.
/// Strength/Dexterity/Focus are DEBUFF when the applied amount is negative
/// (DexterityPower.updateDescription). Lagavulin siphon is Dex then Strength;
/// Ancient Potion Artifact 1 eats the Dexterity (seed 213 Defend 5 not 4).
pub fn power_is_debuff(id: PowerId, amount: i32) -> bool {
    match id {
        PowerId::Weak
        | PowerId::Vulnerable
        | PowerId::Frail
        | PowerId::FrailPlayer
        | PowerId::Poison
        | PowerId::Constricted
        | PowerId::Entangled
        | PowerId::Hex
        | PowerId::NoDraw
        | PowerId::LockOn
        | PowerId::Slow
        | PowerId::Bias
        | PowerId::LoseStrength
        | PowerId::LoseDexterity
        | PowerId::NoBlock
        | PowerId::Shackled
        | PowerId::Confusion => true,
        PowerId::Strength | PowerId::Dexterity | PowerId::Focus => amount < 0,
        _ => false,
    }
}

pub fn add_power_to(powers: &mut Vec<Power>, id: PowerId, amount: i32) {
    add_power_to_flags(powers, id, amount, false);
}

/// ApplyPowerAction body after relic-specific blocks (Ginger/Turnip).
/// ArtifactPower.onApplyPower absorbs `type == DEBUFF` (including Strength/
/// Dexterity/Focus with a negative amount) and skips the apply.
pub fn absorb_or_add_power(powers: &mut Vec<Power>, id: PowerId, amount: i32, from_monster: bool) {
    if power_is_debuff(id, amount) {
        if let Some(art) = powers.iter_mut().find(|p| p.id == PowerId::Artifact) {
            if art.amount > 0 {
                art.amount -= 1;
                if art.amount <= 0 {
                    powers.retain(|p| p.id != PowerId::Artifact);
                }
                return;
            }
        }
    }
    add_power_to_flags(powers, id, amount, from_monster);
}

pub fn add_power_to_flags(powers: &mut Vec<Power>, id: PowerId, amount: i32, from_monster: bool) {
    // TheBombPower uses a unique ID per play (TheBomb0, TheBomb1, …) so later
    // bombs do not stack onto an earlier fuse.
    if id == PowerId::TheBomb {
        if amount != 0 {
            powers.push(Power {
                id,
                amount,
                just_applied: from_monster,
                skip_first: false,
                misc: 0,
            });
        }
        return;
    }
    if let Some(p) = powers.iter_mut().find(|p| p.id == id) {
        p.amount += amount;
        if p.amount == 0 {
            powers.retain(|x| x.id != id);
        }
    } else if amount != 0 {
        powers.push(Power {
            id,
            amount,
            just_applied: from_monster,
            skip_first: id == PowerId::Ritual,
            // FlightPower.storedAmount: atStartOfTurn restores amount to this.
            misc: if id == PowerId::Flight { amount } else { 0 },
        });
    }
}

pub fn end_of_turn(powers: &mut Vec<Power>) {
    // LoseStrengthPower / LoseDexterityPower.atEndOfTurn queue
    // ApplyPowerAction(Strength/Dexterity, -amount), so Artifact absorbs
    // the debuff (seed 357 Speed Potion Dex 5 + Ancient Potion Artifact 1:
    // Glacier+ stays 15). Direct subtraction bypassed Artifact.
    let lose_str = powers
        .iter()
        .find(|p| p.id == PowerId::LoseStrength)
        .map(|p| p.amount)
        .unwrap_or(0);
    if lose_str != 0 {
        absorb_or_add_power(powers, PowerId::Strength, -lose_str, false);
    }
    let lose_dex = powers
        .iter()
        .find(|p| p.id == PowerId::LoseDexterity)
        .map(|p| p.amount)
        .unwrap_or(0);
    if lose_dex != 0 {
        absorb_or_add_power(powers, PowerId::Dexterity, -lose_dex, false);
    }
    // RitualPower.atEndOfTurn when onPlayer: ApplyPower Strength (no skipFirst).
    let ritual = powers
        .iter()
        .find(|p| p.id == PowerId::Ritual)
        .map(|p| p.amount)
        .unwrap_or(0);
    if ritual != 0 {
        add_power_to(powers, PowerId::Strength, ritual);
    }
    powers.retain(|p| {
        p.id != PowerId::Rage
            && p.id != PowerId::LoseStrength
            && p.id != PowerId::LoseDexterity
            && p.id != PowerId::Entangled
            && p.id != PowerId::Amplify
    });
    // Java keeps negative Strength/Dexterity (Lagavulin siphon is -1); drop only 0.
    powers.retain(|p| (p.id != PowerId::Strength && p.id != PowerId::Dexterity) || p.amount != 0);
}

pub fn end_of_round(powers: &mut Vec<Power>) -> i32 {
    let mut ritual_str = 0;
    for p in powers.iter_mut() {
        match p.id {
            PowerId::Ritual => {
                if p.skip_first {
                    p.skip_first = false;
                } else {
                    ritual_str += p.amount;
                }
            }
            PowerId::Vulnerable | PowerId::Weak | PowerId::Frail => {
                if p.just_applied {
                    p.just_applied = false;
                } else {
                    p.amount -= 1;
                }
            }
            // IntangiblePlayerPower.atEndOfRound: always ReducePower, no justApplied.
            PowerId::Intangible => {
                p.amount -= 1;
            }
            // LockOnPower.atEndOfRound always ReducePower(1); no justApplied skip.
            PowerId::LockOn => {
                p.amount -= 1;
            }
            PowerId::NoBlock => {
                if p.just_applied {
                    p.just_applied = false;
                } else {
                    p.amount -= 1;
                }
            }
            PowerId::Shackled => {
                ritual_str += p.amount;
                p.amount = 0;
            }
            // GenericStrengthUpPower.atEndOfRound: ApplyPower Strength(amount).
            PowerId::StrengthUp => {
                ritual_str += p.amount;
            }
            PowerId::Slow => {
                p.amount = 0;
            }
            _ => {}
        }
    }
    powers.retain(|p| p.amount != 0 || p.id == PowerId::Slow);
    ritual_str
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Attack,
    AttackBuff,
    AttackDebuff,
    AttackDefend,
    Buff,
    Debuff,
    StrongDebuff,
    Defend,
    DefendBuff,
    DefendDebuff,
    Escape,
    Sleep,
    Stun,
    Unknown,
    Debug,
}

#[derive(Clone, Debug)]
pub struct Monster {
    pub id: MonsterId,
    pub hp: i32,
    pub max_hp: i32,
    pub block: i32,
    pub powers: Vec<Power>,
    pub intent: Intent,
    pub intent_damage: i32,
    pub intent_base_damage: i32,
    pub intent_hits: i32,
    pub next_move: i32,
    pub move_history: Vec<i32>,
    pub dead: bool,
    pub escaped: bool,
    pub first_move: bool,
    pub extra: i32,
    pub stolen_gold: i32,
    pub split_triggered: bool,
    pub stasis_card: Option<crate::card::Card>,
    pub half_dead: bool,
    pub ascension: i32,
    /// ReactivePower queues RollMoveAction behind the card's existing actions.
    pub pending_reactive: i32,
    /// CurlUpPower GainBlockAction is addToBot, so later hits of the same card
    /// (Rip and Tear, Barrage) land before the block appears.
    pub pending_curl: i32,
    /// HandDrill.onBlockBroken queues Vulnerable behind the card's existing
    /// actions, so block breaks are applied after the whole card resolves.
    pub pending_hand_drill: i32,
    /// Constructor `hb_x` used by SpawnMonsterAction smart insert (`drawX` order).
    pub offset_x: i32,
    /// Spawned this round (split); skip takeTurn until the next monster phase.
    pub just_spawned: bool,
}

impl Monster {
    pub fn alive(&self) -> bool {
        !self.dead && !self.escaped && (self.hp > 0 || self.half_dead)
    }

    pub fn power_amount(&self, id: PowerId) -> i32 {
        self.powers.iter().find(|p| p.id == id).map(|p| p.amount).unwrap_or(0)
    }

    pub fn add_power(&mut self, id: PowerId, amount: i32) {
        absorb_or_add_power(&mut self.powers, id, amount, false);
    }
}
