use super::attack_style::AttackStyle;
use crate::model::{AttributeSet, DamageType, EquipmentBonuses, PrayerEffects, StanceBonuses};
use crate::rules::{self, Ratio};
use std::fmt;

/// Mirrors C#'s `ArgumentOutOfRangeException` for `CombatStats::new` - a level below 1 is a
/// programmer error (bad NPC/player data), not a recoverable runtime condition, so callers are
/// expected to `unwrap()`/`expect()` this at construction time rather than handle it per-site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelBelowOne {
    pub field: &'static str,
    pub value: i32,
}

impl fmt::Display for LevelBelowOne {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} must be at least 1, was {}", self.field, self.value)
    }
}

impl std::error::Error for LevelBelowOne {}

/// Everything a roll reads off one combatant: the five combat levels (the CURRENT, boosted or
/// drained value - OSRS_COMBAT_REFERENCE.md §3 adds boosts before the prayer multiplier, so the
/// boosted level is all the pipeline needs; Hitpoints is `Health::max`, not here), the summed
/// equipment bonuses, the multipliers of the active prayers, the stance's invisible bonuses and
/// the monster attribute tags.
///
/// Every extra defaults to neutral (no bonuses, no prayer, no stance, no attributes), so a
/// `CombatStats::new` built from five levels behaves exactly like the original "equipment always
/// zero" scope: every `(bonus + 64)` term is a flat 64 and the effective level is `level + 8`.
/// Players rebuild theirs whenever gear, prayers, stance or boosts change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatStats {
    attack: i32,
    strength: i32,
    defence: i32,
    ranged: i32,
    magic: i32,
    bonuses: EquipmentBonuses,
    prayer: PrayerEffects,
    stance: StanceBonuses,
    attributes: AttributeSet,
}

impl CombatStats {
    pub fn new(attack: i32, strength: i32, defence: i32, ranged: i32, magic: i32) -> Result<Self, LevelBelowOne> {
        Self::check(attack, "attack")?;
        Self::check(strength, "strength")?;
        Self::check(defence, "defence")?;
        Self::check(ranged, "ranged")?;
        Self::check(magic, "magic")?;

        Ok(Self {
            attack,
            strength,
            defence,
            ranged,
            magic,
            bonuses: EquipmentBonuses::default(),
            prayer: PrayerEffects::default(),
            stance: StanceBonuses::default(),
            attributes: AttributeSet::default(),
        })
    }

    pub fn with_bonuses(mut self, bonuses: EquipmentBonuses) -> Self {
        self.bonuses = bonuses;
        self
    }

    pub fn with_prayer_effects(mut self, prayer: PrayerEffects) -> Self {
        self.prayer = prayer;
        self
    }

    pub fn with_stance(mut self, stance: StanceBonuses) -> Self {
        self.stance = stance;
        self
    }

    pub fn with_attributes(mut self, attributes: AttributeSet) -> Self {
        self.attributes = attributes;
        self
    }

    pub fn bonuses(&self) -> &EquipmentBonuses {
        &self.bonuses
    }

    pub fn prayer_effects(&self) -> &PrayerEffects {
        &self.prayer
    }

    pub fn stance(&self) -> &StanceBonuses {
        &self.stance
    }

    pub fn attributes(&self) -> AttributeSet {
        self.attributes
    }

    /// Effective Attack / Ranged / Magic level for the ACCURACY roll (the §3 pipeline, before any
    /// Void-style `AccuracyLevel` modifier).
    pub fn effective_accuracy_level(&self, style: AttackStyle) -> i32 {
        match style {
            AttackStyle::Melee => rules::effective_level(self.attack, self.prayer.attack_percent, self.stance.attack, Ratio::ONE),
            AttackStyle::Ranged => {
                rules::effective_level(self.ranged, self.prayer.ranged_attack_percent, self.stance.ranged, Ratio::ONE)
            }
            AttackStyle::Magic => rules::effective_level(self.magic, self.prayer.magic_percent, self.stance.magic, Ratio::ONE),
        }
    }

    /// The visible (current, boosted) level behind an accuracy roll - what powered staves scale from.
    pub fn accuracy_level_base(&self, style: AttackStyle) -> i32 {
        self.accuracy_level_for(style)
    }

    /// Effective Strength / Ranged level for the MAX HIT roll (Magic uses its own level only for
    /// the legacy "derived" max hit - real spells have a fixed base, see `MaxHitSource`).
    pub fn effective_damage_level(&self, style: AttackStyle) -> i32 {
        match style {
            AttackStyle::Melee => {
                rules::effective_level(self.strength, self.prayer.strength_percent, self.stance.strength, Ratio::ONE)
            }
            AttackStyle::Ranged => {
                rules::effective_level(self.ranged, self.prayer.ranged_strength_percent, self.stance.ranged, Ratio::ONE)
            }
            AttackStyle::Magic => rules::effective_level(self.magic, self.prayer.magic_percent, self.stance.magic, Ratio::ONE),
        }
    }

    pub fn effective_defence_level(&self) -> i32 {
        rules::effective_level(self.defence, self.prayer.defence_percent, self.stance.defence, Ratio::ONE)
    }

    /// The strength bonus this combatant's own gear adds to a hit of `style`.
    pub fn strength_bonus_for(&self, style: AttackStyle) -> i32 {
        match style {
            AttackStyle::Melee => self.bonuses.melee_strength,
            AttackStyle::Ranged => self.bonuses.ranged_strength,
            AttackStyle::Magic => 0,
        }
    }

    /// This combatant's defence roll against an incoming hit of `damage_type`. A monster defends
    /// with raw level + 9 (its Magic level against magic); a player with the effective-level
    /// pipeline, defending magic with 70% Magic + 30% Defence (reference §4/§5).
    pub fn defence_roll_against(&self, damage_type: DamageType, defender_is_monster: bool) -> i64 {
        let bonus = self.bonuses.defence.get(damage_type);
        let magic = damage_type == DamageType::Magic;
        if defender_is_monster {
            let level = if magic { self.magic } else { self.defence };
            return rules::monster_defence_roll(level, bonus);
        }
        let level = if magic {
            let eff_magic = rules::effective_level(self.magic, self.prayer.magic_percent, self.stance.magic, Ratio::ONE);
            rules::player_magic_defence_level(eff_magic, self.effective_defence_level())
        } else {
            self.effective_defence_level()
        };
        rules::roll(level, bonus)
    }

    fn check(value: i32, field: &'static str) -> Result<(), LevelBelowOne> {
        if value < 1 {
            Err(LevelBelowOne { field, value })
        } else {
            Ok(())
        }
    }

    pub fn attack(self) -> i32 {
        self.attack
    }

    pub fn strength(self) -> i32 {
        self.strength
    }

    pub fn defence(self) -> i32 {
        self.defence
    }

    pub fn ranged(self) -> i32 {
        self.ranged
    }

    pub fn magic(self) -> i32 {
        self.magic
    }

    /// Which level an attack of this style rolls for ACCURACY - Attack for Melee,
    /// Ranged/Magic double as their own accuracy level (no separate stat, matching real OSRS).
    pub fn accuracy_level_for(self, style: AttackStyle) -> i32 {
        match style {
            AttackStyle::Melee => self.attack,
            AttackStyle::Ranged => self.ranged,
            AttackStyle::Magic => self.magic,
        }
    }

    /// Which level an attack of this style rolls for MAX HIT - Strength for Melee,
    /// Ranged/Magic again double as their own damage level (no Ranged Strength/Magic Damage%
    /// stat without equipment - see the struct doc).
    pub fn damage_level_for(self, style: AttackStyle) -> i32 {
        match style {
            AttackStyle::Melee => self.strength,
            AttackStyle::Ranged => self.ranged,
            AttackStyle::Magic => self.magic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_rejects_any_level_below_one() {
        assert!(CombatStats::new(0, 50, 50, 50, 50).is_err());
        assert!(CombatStats::new(50, 0, 50, 50, 50).is_err());
        assert!(CombatStats::new(50, 50, 0, 50, 50).is_err());
        assert!(CombatStats::new(50, 50, 50, 0, 50).is_err());
        assert!(CombatStats::new(50, 50, 50, 50, 0).is_err());
    }

    #[test]
    fn accuracy_level_for_picks_attack_for_melee_and_the_matching_stat_otherwise() {
        let stats = CombatStats::new(10, 20, 30, 40, 50).unwrap();

        assert_eq!(stats.accuracy_level_for(AttackStyle::Melee), 10);
        assert_eq!(stats.accuracy_level_for(AttackStyle::Ranged), 40);
        assert_eq!(stats.accuracy_level_for(AttackStyle::Magic), 50);
    }

    #[test]
    fn damage_level_for_picks_strength_for_melee_and_the_matching_stat_otherwise() {
        let stats = CombatStats::new(10, 20, 30, 40, 50).unwrap();

        assert_eq!(stats.damage_level_for(AttackStyle::Melee), 20);
        assert_eq!(stats.damage_level_for(AttackStyle::Ranged), 40);
        assert_eq!(stats.damage_level_for(AttackStyle::Magic), 50);
    }
}
