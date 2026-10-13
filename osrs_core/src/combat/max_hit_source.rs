use serde::{Deserialize, Serialize};

/// Where an attack's base max hit comes from (reference §5). Real monsters and spells do not
/// derive their max hit from levels the way a player's melee does, so the source is data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaxHitSource {
    /// Melee: `floor((effective Strength * (Strength bonus + 64) + 320) / 640)`; Ranged: the same
    /// with effective Ranged and the Ranged Strength bonus. (Magic under this source keeps the
    /// project's original stand-in: the same formula on the Magic level with no bonus.)
    #[default]
    Derived,
    /// A monster's listed max hit (or any hard-coded hit such as a fixed-damage hazard).
    Fixed(i32),
    /// A standard spell: a fixed base per spell, scaled only by Magic Damage % bonuses.
    Spell { base: i32 },
    /// A powered staff: `floor(Magic level / divisor) + offset`, then Magic Damage % bonuses.
    /// `gear_bonus_scale` multiplies the visible gear bonus first (Tumeken's shadow: 3, or 4 in
    /// the Tombs of Amascut); the combined gear bonus is capped at +100%.
    PoweredStaff { divisor: i32, offset: i32, gear_bonus_scale: i32 },
}

impl MaxHitSource {
    pub fn powered_staff(divisor: i32, offset: i32) -> Self {
        MaxHitSource::PoweredStaff { divisor, offset, gear_bonus_scale: 1 }
    }
}
