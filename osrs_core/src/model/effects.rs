use serde::{Deserialize, Serialize};

/// The live multipliers the active prayers contribute (reference §3 prayer table), as percentages
/// where 100 means "no change". Unused fields stay at their neutral value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PrayerEffects {
    pub attack_percent: i32,
    pub strength_percent: i32,
    pub defence_percent: i32,
    pub ranged_attack_percent: i32,
    pub ranged_strength_percent: i32,
    pub magic_percent: i32,
    /// Added to the Magic Damage % sum (Augury +4% = 40).
    pub magic_damage_permille: i32,
}

impl Default for PrayerEffects {
    fn default() -> Self {
        Self {
            attack_percent: 100,
            strength_percent: 100,
            defence_percent: 100,
            ranged_attack_percent: 100,
            ranged_strength_percent: 100,
            magic_percent: 100,
            magic_damage_permille: 0,
        }
    }
}

impl PrayerEffects {
    /// Combines two sources. Boosting prayers of the same stat are mutually exclusive in the game,
    /// so where both set a stat the larger multiplier wins instead of compounding; the damage
    /// bonus is a plain sum.
    pub fn combine(self, other: PrayerEffects) -> PrayerEffects {
        PrayerEffects {
            attack_percent: self.attack_percent.max(other.attack_percent),
            strength_percent: self.strength_percent.max(other.strength_percent),
            defence_percent: self.defence_percent.max(other.defence_percent),
            ranged_attack_percent: self.ranged_attack_percent.max(other.ranged_attack_percent),
            ranged_strength_percent: self.ranged_strength_percent.max(other.ranged_strength_percent),
            magic_percent: self.magic_percent.max(other.magic_percent),
            magic_damage_permille: self.magic_damage_permille + other.magic_damage_permille,
        }
    }
}

/// The invisible per-stance bonuses added to the effective level (reference §3 stance table).
/// `ranged` applies to BOTH the ranged accuracy and ranged strength levels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StanceBonuses {
    pub attack: i32,
    pub strength: i32,
    pub defence: i32,
    pub ranged: i32,
    pub magic: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_changes_nothing() {
        let p = PrayerEffects::default();
        assert_eq!(p.attack_percent, 100);
        assert_eq!(p.magic_damage_permille, 0);
    }

    #[test]
    fn combine_takes_the_larger_multiplier_and_sums_damage() {
        let piety = PrayerEffects { attack_percent: 120, strength_percent: 123, defence_percent: 125, ..Default::default() };
        let augury = PrayerEffects { magic_percent: 125, defence_percent: 125, magic_damage_permille: 40, ..Default::default() };

        let both = piety.combine(augury);

        assert_eq!(both.attack_percent, 120);
        assert_eq!(both.magic_percent, 125);
        assert_eq!(both.defence_percent, 125);
        assert_eq!(both.magic_damage_permille, 40);
    }
}
