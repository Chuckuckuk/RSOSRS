use super::ground_target::GroundTarget;
use super::player_reaction_state::PlayerReactionState;
use super::protection_prayer::ProtectionPrayer;
use crate::rng::Rng;

/// How much a ground attack deals to each target standing on its tiles. There is no accuracy
/// roll: being on a covered tile when it lands is what hurts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroundDamage {
    /// Always exactly this much.
    Fixed(i32),
    /// Uniform on `0..=max`.
    Uniform { max: i32 },
}

impl GroundDamage {
    /// One target's damage, before any prayer reduction.
    pub fn roll(self, rng: &mut dyn Rng) -> i32 {
        match self {
            GroundDamage::Fixed(amount) => amount,
            GroundDamage::Uniform { max } => rng.next_range(0, max + 1),
        }
    }
}

/// A protection prayer that cuts a ground attack's damage but never negates it (Duke Sucellus'
/// aftershock): the correct prayer lets `penetration` of the damage through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PrayerReduction {
    pub prayer: ProtectionPrayer,
    /// Fraction of the damage that still gets through the correct prayer, in `(0, 1]`.
    pub penetration: f32,
}

/// An attack aimed at the ground: it covers the tiles of its `target` and, when its telegraph
/// ends, hits whoever is standing on them then, and is gone. It has no style, no damage type and
/// never rolls accuracy.
#[derive(Debug, Clone, PartialEq)]
pub struct GroundAttack {
    /// Ticks the tiles stay marked before the attack resolves; it lands, and is gone, when they
    /// end. 0 lands the tick it is chosen.
    pub telegraph_ticks: i32,
    pub target: GroundTarget,
    pub damage: GroundDamage,
    /// The one prayer that reduces (but never negates) the damage, if any.
    pub prayer_reduction: Option<PrayerReduction>,
}

impl GroundAttack {
    pub fn new(telegraph_ticks: i32, target: GroundTarget, damage: GroundDamage) -> Self {
        assert!(telegraph_ticks >= 0, "telegraph_ticks must not be negative");
        match damage {
            GroundDamage::Fixed(amount) => assert!(amount >= 0, "ground damage must not be negative"),
            GroundDamage::Uniform { max } => assert!(max >= 0, "ground damage must not be negative"),
        }
        Self { telegraph_ticks, target, damage, prayer_reduction: None }
    }

    /// The fraction of the damage that reaches a defender in this state: 1 unless the one prayer
    /// that reduces it is active.
    pub fn damage_multiplier(&self, reaction_state: Option<&PlayerReactionState>) -> f32 {
        match (self.prayer_reduction, reaction_state) {
            (Some(reduction), Some(state))
                if state.active_prayer != ProtectionPrayer::None && state.active_prayer == reduction.prayer =>
            {
                reduction.penetration
            }
            _ => 1.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::HazardAnchor;
    use crate::rng::ScriptedRng;

    fn attack(prayer: Option<PrayerReduction>) -> GroundAttack {
        let mut attack = GroundAttack::new(0, GroundTarget::square(HazardAnchor::Target, 0), GroundDamage::Fixed(20));
        attack.prayer_reduction = prayer;
        attack
    }

    fn praying(prayer: ProtectionPrayer) -> PlayerReactionState {
        PlayerReactionState { active_prayer: prayer, ..Default::default() }
    }

    #[test]
    fn fixed_damage_draws_nothing_and_uniform_damage_draws_once() {
        // an empty script panics on any draw
        assert_eq!(GroundDamage::Fixed(12).roll(&mut ScriptedRng::default()), 12);
        assert_eq!(GroundDamage::Uniform { max: 30 }.roll(&mut ScriptedRng::from_ints(vec![17])), 17);
    }

    #[test]
    fn with_no_prayer_reduction_nothing_changes_the_damage() {
        let attack = attack(None);

        assert_eq!(attack.damage_multiplier(Some(&praying(ProtectionPrayer::Melee))), 1.0);
        assert_eq!(attack.damage_multiplier(None), 1.0);
    }

    #[test]
    fn the_correct_prayer_lets_only_the_penetration_fraction_through() {
        let attack = attack(Some(PrayerReduction { prayer: ProtectionPrayer::Melee, penetration: 0.5 }));

        assert_eq!(attack.damage_multiplier(Some(&praying(ProtectionPrayer::Melee))), 0.5);
    }

    #[test]
    fn a_wrong_or_missing_prayer_takes_full_damage() {
        let attack = attack(Some(PrayerReduction { prayer: ProtectionPrayer::Melee, penetration: 0.5 }));

        assert_eq!(attack.damage_multiplier(Some(&praying(ProtectionPrayer::Magic))), 1.0);
        assert_eq!(attack.damage_multiplier(Some(&praying(ProtectionPrayer::None))), 1.0);
        assert_eq!(attack.damage_multiplier(None), 1.0);
    }
}
