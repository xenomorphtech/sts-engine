use crate::generated::card_catalog::{CardDef, CARDS};
use crate::ids::{CardId, CardRarity, CardTarget, CardType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Card {
    pub id: CardId,
    pub upgraded: bool,
    pub times_upgraded: u8,
    pub cost: i16,
    pub cost_for_turn: i16,
    pub base_damage: i16,
    pub base_block: i16,
    pub base_magic: i16,
    pub misc: i16,
    pub free_to_play_once: bool,
    pub exhaust: bool,
    pub ethereal: bool,
    pub retain: bool,
    pub innate: bool,
}

impl Card {
    pub fn new(id: CardId) -> Self {
        let def = id.def();
        let stats = crate::content::card_stats(id, false);
        Self {
            id,
            upgraded: false,
            times_upgraded: 0,
            cost: def.cost,
            cost_for_turn: def.cost,
            base_damage: stats.damage,
            base_block: stats.block,
            base_magic: stats.magic,
            misc: 0,
            free_to_play_once: false,
            exhaust: stats.exhaust,
            ethereal: stats.ethereal,
            retain: false,
            innate: stats.innate,
        }
    }

    pub fn upgrade(&mut self) {
        if self.upgraded && self.id != CardId::Searing_Blow {
            return;
        }
        self.upgraded = true;
        self.times_upgraded = self.times_upgraded.saturating_add(1);
        let stats = crate::content::card_stats(self.id, true);
        self.cost = stats.cost;
        self.cost_for_turn = stats.cost;
        self.base_damage = stats.damage;
        self.base_block = stats.block;
        self.base_magic = stats.magic;
        self.exhaust = stats.exhaust;
        self.ethereal = stats.ethereal;
        self.innate = stats.innate;
    }

    pub fn def(self) -> &'static CardDef {
        self.id.def()
    }

    pub fn card_type(self) -> CardType {
        self.id.def().card_type
    }

    pub fn target(self) -> CardTarget {
        self.id.def().target
    }

    pub fn rarity(self) -> CardRarity {
        self.id.def().rarity
    }

    pub fn needs_target(self) -> bool {
        matches!(self.target(), CardTarget::ENEMY | CardTarget::SELF_AND_ENEMY)
    }

    pub fn sts_id(self) -> &'static str {
        self.id.sts_id()
    }

    pub fn can_upgrade(self) -> bool {
        if matches!(self.card_type(), CardType::STATUS | CardType::CURSE) {
            return false;
        }
        if self.upgraded && self.id != CardId::Searing_Blow {
            return false;
        }
        true
    }
}

impl CardId {
    pub fn def(self) -> &'static CardDef {
        &CARDS[self as usize]
    }

    /// Java `AbstractCard.hasTag(CardTags.STRIKE)`.
    pub fn has_strike_tag(self) -> bool {
        matches!(
            self,
            CardId::Strike_R
                | CardId::Strike_G
                | CardId::Strike_B
                | CardId::Strike_P
                | CardId::Perfected_Strike
                | CardId::Pommel_Strike
                | CardId::Twin_Strike
                | CardId::Wild_Strike
                | CardId::Swift_Strike
                | CardId::Meteor_Strike
                | CardId::Thunder_Strike
                | CardId::WindmillStrike
        )
    }

    /// Java `AbstractCard.hasTag(CardTags.HEALING)`.
    pub fn has_healing_tag(self) -> bool {
        matches!(
            self,
            CardId::Feed
                | CardId::Reaper
                | CardId::Self_Repair
                | CardId::Bandage_Up
                | CardId::Bite
                | CardId::Wish
                | CardId::LessonLearned
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CardStats {
    pub cost: i16,
    pub damage: i16,
    pub block: i16,
    pub magic: i16,
    pub exhaust: bool,
    pub ethereal: bool,
    pub innate: bool,
}

impl CardStats {
    pub const fn attack(cost: i16, damage: i16) -> Self {
        Self {
            cost,
            damage,
            block: -1,
            magic: -1,
            exhaust: false,
            ethereal: false,
            innate: false,
        }
    }

    pub const fn skill(cost: i16, block: i16, magic: i16) -> Self {
        Self {
            cost,
            damage: -1,
            block,
            magic,
            exhaust: false,
            ethereal: false,
            innate: false,
        }
    }
}
