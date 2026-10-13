use serde::{Deserialize, Serialize};

use crate::combat::AttackStyle;

/// The five equipment-bonus categories (reference §3): the three melee attack types plus Ranged
/// and Magic. An attack is rolled against the defender's defence bonus of the SAME category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageType {
    Stab,
    Slash,
    Crush,
    Ranged,
    Magic,
}

impl DamageType {
    pub fn style(self) -> AttackStyle {
        match self {
            DamageType::Stab | DamageType::Slash | DamageType::Crush => AttackStyle::Melee,
            DamageType::Ranged => AttackStyle::Ranged,
            DamageType::Magic => AttackStyle::Magic,
        }
    }

    /// The type a style defaults to when nothing more specific is given.
    pub fn default_for(style: AttackStyle) -> Self {
        match style {
            AttackStyle::Melee => DamageType::Slash,
            AttackStyle::Ranged => DamageType::Ranged,
            AttackStyle::Magic => DamageType::Magic,
        }
    }
}

/// One number per damage type: used for both the attack and the defence bonus tables.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StyleBonuses {
    pub stab: i32,
    pub slash: i32,
    pub crush: i32,
    pub magic: i32,
    pub ranged: i32,
}

impl StyleBonuses {
    pub fn get(&self, damage_type: DamageType) -> i32 {
        match damage_type {
            DamageType::Stab => self.stab,
            DamageType::Slash => self.slash,
            DamageType::Crush => self.crush,
            DamageType::Magic => self.magic,
            DamageType::Ranged => self.ranged,
        }
    }

    pub fn add(&mut self, other: &StyleBonuses) {
        self.stab += other.stab;
        self.slash += other.slash;
        self.crush += other.crush;
        self.magic += other.magic;
        self.ranged += other.ranged;
    }
}

/// Everything an item (or a monster's stat block) contributes to the five-way bonus tables plus
/// the "other" bonuses (reference §3). Equipping is summing these.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EquipmentBonuses {
    pub attack: StyleBonuses,
    pub defence: StyleBonuses,
    pub melee_strength: i32,
    pub ranged_strength: i32,
    /// Magic Damage %, in permille (a +10% bonus is 100). Summed with prayer/set bonuses before the
    /// single floor in `rules::magic_max_hit`.
    pub magic_damage_permille: i32,
    pub prayer: i32,
}

impl EquipmentBonuses {
    pub fn add(&mut self, other: &EquipmentBonuses) {
        self.attack.add(&other.attack);
        self.defence.add(&other.defence);
        self.melee_strength += other.melee_strength;
        self.ranged_strength += other.ranged_strength;
        self.magic_damage_permille += other.magic_damage_permille;
        self.prayer += other.prayer;
    }

    pub fn sum<'a>(items: impl IntoIterator<Item = &'a EquipmentBonuses>) -> Self {
        let mut total = Self::default();
        for item in items {
            total.add(item);
        }
        total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_map_to_their_style() {
        assert_eq!(DamageType::Stab.style(), AttackStyle::Melee);
        assert_eq!(DamageType::Crush.style(), AttackStyle::Melee);
        assert_eq!(DamageType::Ranged.style(), AttackStyle::Ranged);
        assert_eq!(DamageType::Magic.style(), AttackStyle::Magic);
    }

    #[test]
    fn summing_equipment_adds_every_field() {
        let helm = EquipmentBonuses { melee_strength: 3, prayer: 1, ..Default::default() };
        let mut body = EquipmentBonuses { melee_strength: 8, magic_damage_permille: 20, ..Default::default() };
        body.defence.stab = 100;

        let total = EquipmentBonuses::sum([&helm, &body]);

        assert_eq!(total.melee_strength, 11);
        assert_eq!(total.prayer, 1);
        assert_eq!(total.magic_damage_permille, 20);
        assert_eq!(total.defence.get(DamageType::Stab), 100);
    }

    #[test]
    fn sparse_json_fills_the_rest_with_zero() {
        let parsed: EquipmentBonuses = serde_json::from_str(r#"{"attack":{"slash":67},"melee_strength":66}"#).unwrap();
        assert_eq!(parsed.attack.slash, 67);
        assert_eq!(parsed.attack.stab, 0);
        assert_eq!(parsed.melee_strength, 66);
    }
}
