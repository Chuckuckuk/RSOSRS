use serde::{Deserialize, Serialize};

use crate::combat::MaxHitSource;
use crate::model::{DamageType, EquipmentBonuses, Modifier, StanceBonuses};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipSlot {
    Head,
    Cape,
    Neck,
    Ammo,
    Weapon,
    Body,
    Shield,
    Legs,
    Hands,
    Feet,
    Ring,
}

/// What a stance is called in the combat options tab; informational (the numbers that matter are
/// on the `StanceDef`), but it drives XP routing and UI labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StanceKind {
    Accurate,
    Aggressive,
    Defensive,
    Controlled,
    Rapid,
    Longrange,
    /// Casting a spell (autocast or manual): no invisible bonus.
    Spell,
    DefensiveSpell,
}

/// One entry of a weapon's combat-style menu (reference §3 stance table).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StanceDef {
    pub name: String,
    pub kind: StanceKind,
    /// The damage type rolled with this stance (melee weapons differ per stance: a whip is slash
    /// when Accurate/Aggressive but still slash when Controlled; a spear stabs/slashes/crushes
    /// across its three). `None` keeps the weapon's default.
    pub damage_type: Option<DamageType>,
    pub bonuses: StanceBonuses,
    /// Added to the weapon's attack speed (Rapid is -1; Accurate/Longrange ranged are +0 here
    /// because the weapon's listed speed already includes the slower base).
    pub speed_delta: i32,
    /// Added to the weapon's attack range (Longrange is +2).
    pub range_delta: i32,
    /// The swing's cache sound effect when this stance has its own (a scimitar's stab is not its
    /// slash); `None` keeps the weapon's `attack_sound`.
    pub sound: Option<u32>,
}

impl Default for StanceDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: StanceKind::Accurate,
            damage_type: None,
            bonuses: StanceBonuses::default(),
            speed_delta: 0,
            range_delta: 0,
            sound: None,
        }
    }
}

/// A special attack (reference §2/§7 spec energy). Replaces the normal attack for one use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpecialAttackDef {
    pub name: String,
    /// Percent of the 100% bar consumed.
    pub energy_cost: i32,
    /// Extra modifiers for this one attack (accuracy and damage multipliers, defence reduction).
    pub modifiers: Vec<Modifier>,
    pub hit_count: i32,
    pub attack_speed: Option<i32>,
    pub always_hits: bool,
    pub damage_type: Option<DamageType>,
    pub max_hit: Option<MaxHitSource>,
}

impl Default for SpecialAttackDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            energy_cost: 50,
            modifiers: Vec::new(),
            hit_count: 1,
            attack_speed: None,
            always_hits: false,
            damage_type: None,
            max_hit: None,
        }
    }
}

/// The combat side of a weapon.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WeaponDef {
    pub category: String,
    /// Ticks between attacks (reference §7: 4 for scimitars, 5 for longswords, ...).
    pub attack_speed: i32,
    /// Tiles. 1 means melee (adjacent); anything larger is rolled from a distance.
    pub attack_range: i32,
    pub stances: Vec<StanceDef>,
    pub special: Option<SpecialAttackDef>,
    /// Set for powered staves (Trident, Sanguinesti, Shadow): the staff supplies its own max hit.
    pub powered: Option<MaxHitSource>,
    /// Whether the weapon can cast from the spellbook (regular staves and wands).
    pub can_cast: bool,
    /// Which ammunition family this weapon fires (`arrow`, `bolt`, ...); `None` for melee,
    /// thrown weapons and magic.
    pub ammo_family: Option<String>,
    /// The attack is ranged but the weapon itself supplies the Ranged Strength (darts, knives).
    pub thrown: bool,
    /// The cache sound effect (archive 4) heard as it attacks.
    pub attack_sound: Option<u32>,
}

impl Default for WeaponDef {
    fn default() -> Self {
        Self {
            category: "unarmed".into(),
            attack_speed: 4,
            attack_range: 1,
            stances: Vec::new(),
            special: None,
            powered: None,
            can_cast: false,
            ammo_family: None,
            thrown: false,
            attack_sound: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AmmoWeight {
    Light,
    Standard,
    Heavy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AmmoDef {
    pub family: String,
    pub weight: AmmoWeight,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemDef {
    pub id: String,
    pub name: String,
    /// Where it is worn; `None` for an item that is only ever carried (an empty vial).
    pub slot: Option<EquipSlot>,
    pub bonuses: EquipmentBonuses,
    pub weight_kg: f64,
    /// Always-on conditional effects while worn (Slayer helm, Salve amulet, Dragon hunter gear...).
    pub modifiers: Vec<Modifier>,
    /// Free tags other systems key on: `regen_bracelet`, `lightbearer`, `graceful`,
    /// `hitpoints_cape`, `ring_of_endurance`, ...
    pub tags: Vec<String>,
    pub weapon: Option<WeaponDef>,
    pub ammo: Option<AmmoDef>,
    /// For a pet: the monster (`npcs.json`) it is while it follows the player.
    pub follower: Option<String>,
    /// Free-form provenance (wiki page, whether the numbers are verified).
    pub notes: String,
}

impl Default for ItemDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            slot: None,
            bonuses: EquipmentBonuses::default(),
            weight_kg: 0.0,
            modifiers: Vec::new(),
            tags: Vec::new(),
            weapon: None,
            ammo: None,
            follower: None,
            notes: String::new(),
        }
    }
}

impl ItemDef {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

/// A set of items that grants extra bonuses/modifiers only when all pieces are worn (Void,
/// Dharok's, Barrows sets). Each entry of `pieces` lists the acceptable items for one piece, so
/// "any void helm" is a single entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SetDef {
    pub id: String,
    pub name: String,
    pub pieces: Vec<Vec<String>>,
    pub bonuses: EquipmentBonuses,
    pub modifiers: Vec<Modifier>,
}

impl Default for SetDef {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), pieces: Vec::new(), bonuses: EquipmentBonuses::default(), modifiers: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_whip_parses_with_sparse_json() {
        let item: ItemDef = serde_json::from_str(
            r#"{
                "id": "abyssal_whip", "name": "Abyssal whip", "slot": "weapon",
                "bonuses": {"attack": {"slash": 82}, "melee_strength": 82},
                "weapon": {
                    "category": "whip", "attack_speed": 4,
                    "stances": [
                        {"name": "Flick", "kind": "accurate", "damage_type": "slash", "bonuses": {"attack": 3}},
                        {"name": "Lash", "kind": "controlled", "bonuses": {"attack": 1, "strength": 1, "defence": 1}}
                    ],
                    "special": {"name": "Energy Drain", "energy_cost": 50,
                        "modifiers": [{"target": "attack_roll", "ratio": {"num": 5, "den": 4}}]}
                }
            }"#,
        )
        .unwrap();

        let weapon = item.weapon.unwrap();
        assert_eq!(weapon.attack_speed, 4);
        assert_eq!(weapon.attack_range, 1);
        assert_eq!(weapon.stances.len(), 2);
        assert_eq!(weapon.stances[0].damage_type, Some(DamageType::Slash));
        assert_eq!(weapon.stances[1].bonuses.defence, 1);
        assert_eq!(weapon.special.unwrap().modifiers.len(), 1);
    }
}
