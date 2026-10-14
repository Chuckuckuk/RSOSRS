use serde::{Deserialize, Serialize};

use crate::model::Skill;

/// One thing a food or potion does when used.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsumableEffect {
    /// Heals `flat + floor(max_hp * percent / 100)` (a Saradomin brew style percent heal).
    Heal { flat: i32, percent: i32 },
    /// Raises a stat to `base + flat + floor(base * percent / 100)`, never lowering it.
    Boost { skill: Skill, flat: i32, percent: i32 },
    /// Lowers a stat by `flat + floor(base * percent / 100)` (the downside of a brew), floored at 1.
    Drain { skill: Skill, flat: i32, percent: i32 },
    /// Restores a stat by `flat + floor(base * percent / 100)`, up to its base (Super restore: 8 + 25%).
    Restore { skill: Skill, flat: i32, percent: i32 },
    RestoreRunEnergy { percent: i32 },
    RestoreSpecialEnergy { percent: i32 },
    CurePoison,
    CureVenom,
    PoisonImmunity { ticks: i32 },
    /// A Stamina potion: cuts run energy drain for this many ticks.
    StaminaBoost { ticks: i32 },
    /// Kills the user outright (Humble pie): hitpoints go to 0 and death plays out as from any
    /// other lethal hit.
    Die,
}

/// A food or potion. Using one starts a cooldown for its `delay_group`; foods and potions are
/// separate groups, so a food and a potion can be used on the same tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsumableDef {
    pub id: String,
    pub name: String,
    pub delay_group: String,
    /// Ticks before another consumable of the same group may be used.
    pub delay_ticks: i32,
    /// Ticks this adds to the user's attack timer (3 for most food, 0 for potions).
    pub attack_delay_ticks: i32,
    pub effects: Vec<ConsumableEffect>,
    /// The item that takes its place once it is used, if any: each dose of a potion is its own
    /// consumable (`prayer_potion_4`, `prayer_potion_3`, ...) that leaves the next dose down, and
    /// the last dose leaves an empty `vial`. Food leaves nothing.
    pub leaves: Option<String>,
    /// The cache sound effect heard as it is used; `None` is the game's usual one for its
    /// `delay_group` (eating for food, drinking for potions).
    pub sound: Option<u32>,
    /// The name of the cache item whose picture stands for it, when the cache has none of its
    /// own under `name` (Humble pie is not in this cache).
    pub icon: Option<String>,
    /// Free-form provenance (wiki page, whether the numbers are verified).
    pub notes: String,
}

impl Default for ConsumableDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            delay_group: "food".into(),
            delay_ticks: 3,
            attack_delay_ticks: 0,
            effects: Vec::new(),
            leaves: None,
            sound: None,
            icon: None,
            notes: String::new(),
        }
    }
}
