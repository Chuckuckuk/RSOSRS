use serde::{Deserialize, Serialize};

use super::OnHitDef;
use crate::model::{Attribute, Modifier};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LevelScaling {
    pub divisor: i32,
    pub offset: i32,
}

/// A combat spell. Its max hit is a fixed per-spell base (reference §5), not derived from the
/// Magic level; only Magic Damage % bonuses scale it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SpellDef {
    pub id: String,
    pub name: String,
    /// `standard`, `ancient`, `arceuus`.
    pub spellbook: String,
    pub level: i32,
    pub base_max_hit: i32,
    pub range: i32,
    /// Ticks between casts (5 for standard spells).
    pub cast_speed: i32,
    pub modifiers: Vec<Modifier>,
    pub on_hit: Option<OnHitDef>,
    /// Magic Dart style: the base max hit is `floor(Magic level / divisor) + offset` instead of a
    /// fixed number.
    pub level_scaling: Option<LevelScaling>,
    /// The spell can only be cast at targets with this attribute (Crumble Undead).
    pub requires_target_attribute: Option<Attribute>,
    /// Cache sound effects: heard as it is cast, and as it lands (a splash is the game's own
    /// failed-spell sound instead).
    pub cast_sound: Option<u32>,
    pub hit_sound: Option<u32>,
    pub tags: Vec<String>,
    /// Free-form provenance (wiki page, whether the numbers are verified).
    pub notes: String,
}

impl Default for SpellDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            spellbook: "standard".into(),
            level: 1,
            base_max_hit: 1,
            range: 10,
            cast_speed: 5,
            modifiers: Vec::new(),
            on_hit: None,
            level_scaling: None,
            requires_target_attribute: None,
            cast_sound: None,
            hit_sound: None,
            tags: Vec::new(),
            notes: String::new(),
        }
    }
}
