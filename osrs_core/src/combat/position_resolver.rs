use std::rc::Rc;

use super::ground_target::GroundTarget;
use super::hazard::HazardDef;
use super::player_reaction_state::PlayerReactionState;
use super::protection_prayer::ProtectionPrayer;
use super::special_attack::{SpecialContext, SpecialEffect, SpecialResolver};
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

/// Where a position special leaves its hazard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HazardPlacement {
    /// On the tile the area is centred on, once.
    AnchorTile,
    /// On every tile the area covers.
    EveryCoveredTile,
}

/// What a position special leaves behind where it lands, whether or not it hurt anyone.
#[derive(Debug, Clone, PartialEq)]
pub struct LeftBehind {
    pub def: Rc<HazardDef>,
    pub placement: HazardPlacement,
}

/// A special that is decided by position: it covers the tiles of its `target` and, when its
/// wind-up ends, hurts whoever is standing on them then, and is gone. It has no style, no damage
/// type and never rolls accuracy. (Falling rocks, a boulder, an aftershock, a claw.)
#[derive(Debug, Clone, PartialEq)]
pub struct PositionResolver {
    pub target: GroundTarget,
    pub damage: GroundDamage,
    /// The one prayer that reduces (but never negates) the damage, if any.
    pub prayer_reduction: Option<PrayerReduction>,
    /// A hazard it leaves where it lands (the boulder a rock becomes).
    pub leaves: Option<LeftBehind>,
}

impl PositionResolver {
    pub fn new(target: GroundTarget, damage: GroundDamage) -> Self {
        match damage {
            GroundDamage::Fixed(amount) => assert!(amount >= 0, "ground damage must not be negative"),
            GroundDamage::Uniform { max } => assert!(max >= 0, "ground damage must not be negative"),
        }
        Self { target, damage, prayer_reduction: None, leaves: None }
    }

    /// The one protection prayer that cuts this attack's damage without negating it:
    /// `penetration` of the damage still gets through when it is active.
    pub fn with_prayer_reduction(mut self, prayer: ProtectionPrayer, penetration: f32) -> Self {
        assert!(prayer != ProtectionPrayer::None, "a prayer reduction needs a prayer");
        assert!((0.0..=1.0).contains(&penetration), "penetration must be within [0, 1]");
        self.prayer_reduction = Some(PrayerReduction { prayer, penetration });
        self
    }

    /// Leaves `def` behind where this lands.
    pub fn leaving(mut self, def: Rc<HazardDef>, placement: HazardPlacement) -> Self {
        self.leaves = Some(LeftBehind { def, placement });
        self
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

impl SpecialResolver for PositionResolver {
    fn resolve(&self, context: &SpecialContext, rng: &mut dyn Rng) -> Vec<SpecialEffect> {
        // the tiles are fixed from where both sides stood at selection; who is on them is now
        let tiles = self.target.tiles(context.self_tile, context.target_tile, rng);
        let mut effects = Vec::new();
        if context.player_alive && tiles.contains(&context.player_tile) {
            // no accuracy roll: standing there is what hurts, less what the one prayer cuts
            let rolled = self.damage.roll(rng);
            let multiplier = self.damage_multiplier(Some(context.player_reaction));
            let damage = ((rolled as f32 * multiplier) as f64).round() as i32;
            effects.push(SpecialEffect::HurtPlayer { damage, delay_ticks: 0 });
        }
        if let Some(left) = &self.leaves {
            let places = match left.placement {
                HazardPlacement::AnchorTile => vec![self.target.anchor_tile(context.self_tile, context.target_tile)],
                HazardPlacement::EveryCoveredTile => tiles,
            };
            effects.extend(places.into_iter().map(|tile| SpecialEffect::SpawnHazard { def: Rc::clone(&left.def), tile, armed_in: 0 }));
        }
        effects
    }

    fn as_position(&self) -> Option<&PositionResolver> {
        Some(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::HazardAnchor;
    use crate::rng::ScriptedRng;

    fn attack(prayer: Option<PrayerReduction>) -> PositionResolver {
        let mut attack = PositionResolver::new(GroundTarget::square(HazardAnchor::Target, 0), GroundDamage::Fixed(20));
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
