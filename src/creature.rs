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
    pub orbs: Vec<Orb>,
    pub max_orbs: i32,
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
            orbs: Vec::new(),
            max_orbs: 0,
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
            orbs: Vec::new(),
            max_orbs: 3,
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
        let debuff = matches!(id, PowerId::Vulnerable | PowerId::Weak | PowerId::Frail);
        if debuff {
            if let Some(art) = self.powers.iter_mut().find(|p| p.id == PowerId::Artifact) {
                if art.amount > 0 {
                    art.amount -= 1;
                    if art.amount <= 0 {
                        self.powers.retain(|p| p.id != PowerId::Artifact);
                    }
                    return;
                }
            }
        }
        add_power_to_flags(&mut self.powers, id, amount, from_monster);
    }

    pub fn has_relic(&self, id: crate::ids::RelicId) -> bool {
        self.relics.iter().any(|r| r.id == id)
    }
}

pub fn add_power_to(powers: &mut Vec<Power>, id: PowerId, amount: i32) {
    add_power_to_flags(powers, id, amount, false);
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
            misc: 0,
        });
    }
}

pub fn end_of_turn(powers: &mut Vec<Power>) {
    let lose_str = powers
        .iter()
        .find(|p| p.id == PowerId::LoseStrength)
        .map(|p| p.amount)
        .unwrap_or(0);
    if lose_str != 0 {
        if let Some(s) = powers.iter_mut().find(|p| p.id == PowerId::Strength) {
            s.amount -= lose_str;
        }
    }
    let lose_dex = powers
        .iter()
        .find(|p| p.id == PowerId::LoseDexterity)
        .map(|p| p.amount)
        .unwrap_or(0);
    if lose_dex != 0 {
        if let Some(s) = powers.iter_mut().find(|p| p.id == PowerId::Dexterity) {
            s.amount -= lose_dex;
        }
    }
    powers.retain(|p| {
        p.id != PowerId::Rage
            && p.id != PowerId::LoseStrength
            && p.id != PowerId::LoseDexterity
            && p.id != PowerId::Entangled
    });
    powers.retain(|p| (p.id != PowerId::Strength && p.id != PowerId::Dexterity) || p.amount > 0);
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
            PowerId::Shackled => {
                ritual_str += p.amount;
                p.amount = 0;
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
}

impl Monster {
    pub fn alive(&self) -> bool {
        !self.dead && !self.escaped && (self.hp > 0 || self.half_dead)
    }

    pub fn power_amount(&self, id: PowerId) -> i32 {
        self.powers.iter().find(|p| p.id == id).map(|p| p.amount).unwrap_or(0)
    }

    pub fn add_power(&mut self, id: PowerId, amount: i32) {
        let debuff = matches!(id, PowerId::Vulnerable | PowerId::Weak | PowerId::Frail);
        if debuff {
            if let Some(art) = self.powers.iter_mut().find(|p| p.id == PowerId::Artifact) {
                if art.amount > 0 {
                    art.amount -= 1;
                    if art.amount <= 0 {
                        self.powers.retain(|p| p.id != PowerId::Artifact);
                    }
                    return;
                }
            }
        }
        add_power_to(&mut self.powers, id, amount);
    }
}
