use std::rc::Rc;

use super::attack_style::AttackStyle;
use super::ground_attack::{GroundAttack, GroundDamage, PrayerReduction};
use super::ground_target::GroundTarget;
use super::max_hit_source::MaxHitSource;
use super::protection_prayer::ProtectionPrayer;
use super::reaction_requirement::ReactionRequirement;
use super::status_effect::StatusEffectDefinition;
use super::targeted_attack::TargetedAttack;
use crate::model::{DamageType, Modifier};
use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// One attack an NPC can select. Shared (`Rc<AttackDefinition>`) between patterns, the combat
/// state that is currently telegraphing it and the pending-attack queue that later rolls it, so
/// "the same attack" stays pointer-identical like the C# reference type.
///
/// This holds what every attack has: when it happens (telegraph, delay, cooldown), how far it
/// reaches, and how it looks and sounds. What it does is its `kind`: a `TargetedAttack` is rolled
/// against a defender (accuracy, style, prayers), a `GroundAttack` hits whoever is standing on a set
/// of tiles with a plain amount of damage.
///
/// The optional fields are set with the `with_*` builders (each validates like the C# constructor
/// and panics on bad data, including a builder for a field the attack's kind does not have).
#[derive(Debug, Clone)]
pub struct AttackDefinition {
    pub name: String,
    pub range: i32,
    pub telegraph_ticks: i32,
    /// Ticks between the telegraph ending and the roll or landing ("Doom-style" attacks, and every
    /// ground attack).
    pub impact_delay_ticks: i32,
    /// Ticks the attacker must wait after this attack resolves before it may begin its next
    /// telegraph. `with_attack_speed` derives it from an OSRS attack speed; 0 keeps the original
    /// back-to-back behaviour.
    pub cooldown_ticks: i32,
    pub on_hit_status_effect: Option<StatusEffectDefinition>,
    /// The cache sequence the attacker plays for this attack, if it has its own (presentation
    /// only: the rules never read it).
    pub animation: Option<u32>,
    /// Graphics (cache spot animations) for the client: one flying from attacker to target, one
    /// on the target as the attack is released, one on it as the hit lands. Presentation only.
    pub projectile: Option<u32>,
    pub target_graphic: Option<u32>,
    pub impact_graphic: Option<u32>,
    /// Cache sound effects (archive 4): one heard as the attack is thrown, one as it lands (a
    /// spell's impact). Presentation only.
    pub sound: Option<u32>,
    pub hit_sound: Option<u32>,
    pub kind: AttackKind,
}

/// What an attack does, and the data only that kind of attack has.
#[derive(Debug, Clone)]
pub enum AttackKind {
    /// Rolled against a defender.
    Targeted(TargetedAttack),
    /// Aimed at tiles; never rolls accuracy.
    Ground(GroundAttack),
}

impl AttackDefinition {
    /// An attack rolled against a defender, in the given combat style.
    pub fn targeted(name: impl Into<String>, range: i32, telegraph_ticks: i32, style: AttackStyle) -> Self {
        Self::with_kind(name.into(), range, telegraph_ticks, AttackKind::Targeted(TargetedAttack::new(style)))
    }

    /// An attack aimed at the ground: after `impact_delay_ticks` (set with
    /// `with_impact_delay_ticks`) it deals `damage` to whoever stands on the tiles `target` picks.
    pub fn ground(
        name: impl Into<String>,
        range: i32,
        telegraph_ticks: i32,
        target: GroundTarget,
        damage: GroundDamage,
    ) -> Self {
        Self::with_kind(name.into(), range, telegraph_ticks, AttackKind::Ground(GroundAttack::new(target, damage)))
    }

    fn with_kind(name: String, range: i32, telegraph_ticks: i32, kind: AttackKind) -> Self {
        Self {
            name,
            range,
            telegraph_ticks,
            impact_delay_ticks: 0,
            cooldown_ticks: 0,
            on_hit_status_effect: None,
            animation: None,
            projectile: None,
            target_graphic: None,
            impact_graphic: None,
            sound: None,
            hit_sound: None,
            kind,
        }
    }

    pub fn targeted_part(&self) -> Option<&TargetedAttack> {
        match &self.kind {
            AttackKind::Targeted(targeted) => Some(targeted),
            AttackKind::Ground(_) => None,
        }
    }

    /// The targeted part of an attack that is known to be one: a player's swing, or one waiting in
    /// a defender's queue. Panics for a ground attack, which can be neither.
    pub fn expect_targeted(&self) -> &TargetedAttack {
        self.targeted_part().unwrap_or_else(|| panic!("'{}' is a ground attack; it is never rolled against a defender", self.name))
    }

    pub fn ground_part(&self) -> Option<&GroundAttack> {
        match &self.kind {
            AttackKind::Ground(ground) => Some(ground),
            AttackKind::Targeted(_) => None,
        }
    }

    /// Applies `change` to the targeted part. Panics for a ground attack, which has none: setting
    /// an accuracy modifier on a boulder is a data bug, not something to ignore.
    fn change_targeted(mut self, builder: &str, change: impl FnOnce(TargetedAttack) -> TargetedAttack) -> Self {
        match self.kind {
            AttackKind::Targeted(targeted) => self.kind = AttackKind::Targeted(change(targeted)),
            AttackKind::Ground(_) => panic!("`{builder}` only applies to a targeted attack, and '{}' is a ground attack", self.name),
        }
        self
    }

    /// Makes consecutive uses of this attack resolve exactly `speed_ticks` apart (OSRS attack
    /// speed). An attack without a telegraph resolves on the tick it is chosen; one with a
    /// telegraph spends `telegraph_ticks` winding up first, so the fastest speed this can express
    /// is `telegraph_ticks + 1`.
    pub fn with_attack_speed(mut self, speed_ticks: i32) -> Self {
        let busy = self.telegraph_ticks.max(0);
        self.cooldown_ticks = (speed_ticks - 1 - busy).max(0);
        self
    }

    pub fn with_impact_delay_ticks(mut self, impact_delay_ticks: i32) -> Self {
        assert!(impact_delay_ticks >= 0, "impact_delay_ticks must not be negative");
        self.impact_delay_ticks = impact_delay_ticks;
        self
    }

    pub fn with_on_hit_status_effect(mut self, effect: StatusEffectDefinition) -> Self {
        self.on_hit_status_effect = Some(effect);
        self
    }

    pub fn with_animation(mut self, animation: Option<u32>) -> Self {
        self.animation = animation;
        self
    }

    pub fn with_graphics(mut self, projectile: Option<u32>, target_graphic: Option<u32>, impact_graphic: Option<u32>) -> Self {
        self.projectile = projectile;
        self.target_graphic = target_graphic;
        self.impact_graphic = impact_graphic;
        self
    }

    pub fn with_sounds(mut self, sound: Option<u32>, hit_sound: Option<u32>) -> Self {
        self.sound = sound;
        self.hit_sound = hit_sound;
        self
    }

    // ---- targeted-only builders ----

    pub fn with_damage_type(self, damage_type: DamageType) -> Self {
        self.change_targeted("with_damage_type", |t| t.with_damage_type(damage_type))
    }

    pub fn with_max_hit(self, max_hit: MaxHitSource) -> Self {
        self.change_targeted("with_max_hit", |t| t.with_max_hit(max_hit))
    }

    pub fn with_modifiers(self, modifiers: Vec<Modifier>) -> Self {
        self.change_targeted("with_modifiers", |t| t.with_modifiers(modifiers))
    }

    pub fn with_flags(self, flags: Vec<String>) -> Self {
        self.change_targeted("with_flags", |t| t.with_flags(flags))
    }

    pub fn with_hit_count(self, hit_count: i32) -> Self {
        self.change_targeted("with_hit_count", |t| t.with_hit_count(hit_count))
    }

    pub fn with_always_hits(self, always_hits: bool) -> Self {
        self.change_targeted("with_always_hits", |t| t.with_always_hits(always_hits))
    }

    pub fn with_prayer_penetration(self, prayer_penetration: f32) -> Self {
        self.change_targeted("with_prayer_penetration", |t| t.with_prayer_penetration(prayer_penetration))
    }

    pub fn with_reaction(self, reaction: ReactionRequirement) -> Self {
        self.change_targeted("with_reaction", |t| t.with_reaction(reaction))
    }

    pub fn with_projectile_travel_ticks(self, projectile_travel_ticks: i32) -> Self {
        self.change_targeted("with_projectile_travel_ticks", |t| t.with_projectile_travel_ticks(projectile_travel_ticks))
    }

    // ---- ground-only builders ----

    /// The one protection prayer that cuts this ground attack's damage without negating it:
    /// `penetration` of the damage still gets through when it is active.
    pub fn with_prayer_reduction(mut self, prayer: ProtectionPrayer, penetration: f32) -> Self {
        assert!(prayer != ProtectionPrayer::None, "a prayer reduction needs a prayer");
        assert!((0.0..=1.0).contains(&penetration), "penetration must be within [0, 1]");
        match &mut self.kind {
            AttackKind::Ground(ground) => ground.prayer_reduction = Some(PrayerReduction { prayer, penetration }),
            AttackKind::Targeted(_) => {
                panic!("`with_prayer_reduction` only applies to a ground attack, and '{}' is a targeted attack", self.name)
            }
        }
        self
    }

    /// Wraps the finished definition for sharing.
    pub fn shared(self) -> Rc<Self> {
        Rc::new(self)
    }

    /// Whether this is a melee attack at base reach (range 1): it must be orthogonally adjacent
    /// to the attacker's footprint. A melee attack with a longer reach (a halberd's 2), and every
    /// ground attack, is measured like any other: within `range` tiles.
    pub fn is_melee_reach(&self) -> bool {
        self.targeted_part().is_some_and(|targeted| targeted.style == AttackStyle::Melee) && self.range <= 1
    }

    /// The target spec an attacker standing at `attacker_tile` with the given footprint must
    /// satisfy for this attack to be in range of a tile.
    pub fn build_range_check(&self, attacker_tile: TileCoord, attacker_footprint: i32) -> TargetSpec {
        if self.is_melee_reach() {
            TargetSpec::adjacent(attacker_tile, attacker_footprint, attacker_footprint)
        } else {
            TargetSpec::within_range_of(attacker_tile, self.range)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::HazardAnchor;

    fn boulder() -> AttackDefinition {
        AttackDefinition::ground("Boulder", 10, 2, GroundTarget::square(HazardAnchor::Target, 1), GroundDamage::Fixed(30))
    }

    #[test]
    fn a_targeted_attack_defaults_the_optional_fields() {
        let attack = AttackDefinition::targeted("Test", 0, 1, AttackStyle::Melee);
        let targeted = attack.targeted_part().expect("targeted");

        assert_eq!(targeted.prayer_penetration, 0.0);
        assert_eq!(targeted.reaction, ReactionRequirement::none());
        assert_eq!(attack.impact_delay_ticks, 0);
        assert_eq!(targeted.projectile_travel_ticks, 0);
        assert!(attack.on_hit_status_effect.is_none());
        assert!(attack.ground_part().is_none());
    }

    #[test]
    fn a_ground_attack_has_no_targeted_part() {
        let attack = boulder();

        assert!(attack.targeted_part().is_none());
        assert_eq!(attack.ground_part().expect("ground").damage, GroundDamage::Fixed(30));
    }

    #[test]
    #[should_panic]
    fn prayer_penetration_above_one_panics() {
        AttackDefinition::targeted("Test", 0, 1, AttackStyle::Melee).with_prayer_penetration(1.5);
    }

    #[test]
    #[should_panic]
    fn negative_impact_delay_panics() {
        AttackDefinition::targeted("Test", 0, 1, AttackStyle::Melee).with_impact_delay_ticks(-1);
    }

    #[test]
    #[should_panic(expected = "ground attack")]
    fn an_accuracy_builder_on_a_ground_attack_panics() {
        boulder().with_always_hits(true);
    }

    #[test]
    #[should_panic(expected = "targeted attack")]
    fn a_prayer_reduction_on_a_targeted_attack_panics() {
        AttackDefinition::targeted("Test", 0, 1, AttackStyle::Melee).with_prayer_reduction(ProtectionPrayer::Melee, 0.5);
    }

    #[test]
    fn a_ground_attack_takes_a_prayer_reduction() {
        let attack = boulder().with_prayer_reduction(ProtectionPrayer::Melee, 0.5);

        let reduction = attack.ground_part().unwrap().prayer_reduction.expect("reduction");
        assert_eq!(reduction.prayer, ProtectionPrayer::Melee);
        assert_eq!(reduction.penetration, 0.5);
    }

    #[test]
    fn melee_range_check_is_adjacency_to_the_attackers_footprint() {
        let attack = AttackDefinition::targeted("Test", 0, 1, AttackStyle::Melee);

        let check = attack.build_range_check(TileCoord::new(5, 5), 2);

        assert!(check.is_satisfied_by(TileCoord::new(4, 5)));
        assert!(check.is_satisfied_by(TileCoord::new(7, 6)));
        assert!(!check.is_satisfied_by(TileCoord::new(5, 5)));
    }

    #[test]
    fn ranged_range_check_is_chebyshev_distance_from_the_attacker_tile() {
        let attack = AttackDefinition::targeted("Test", 3, 1, AttackStyle::Magic);

        let check = attack.build_range_check(TileCoord::new(5, 5), 1);

        assert!(check.is_satisfied_by(TileCoord::new(8, 8)));
        assert!(!check.is_satisfied_by(TileCoord::new(9, 5)));
    }

    #[test]
    fn a_ground_attack_is_measured_by_range_never_by_adjacency() {
        let attack = AttackDefinition::ground("Slam", 1, 1, GroundTarget::square(HazardAnchor::SelfTile, 1), GroundDamage::Fixed(5));

        let check = attack.build_range_check(TileCoord::new(5, 5), 1);

        assert!(check.is_satisfied_by(TileCoord::new(6, 6)), "range 1 reaches the diagonal: it is not melee adjacency");
    }
}
