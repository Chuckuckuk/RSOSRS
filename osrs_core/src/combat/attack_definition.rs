use std::rc::Rc;

use super::attack_range_kind::AttackRangeKind;
use super::attack_style::AttackStyle;
use super::homing_pulse_orb_definition::HomingPulseOrbDefinition;
use super::max_hit_source::MaxHitSource;
use super::reaction_requirement::ReactionRequirement;
use super::status_effect::StatusEffectDefinition;
use crate::model::{DamageType, Modifier};
use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// One attack an NPC can select. Shared (`Rc<AttackDefinition>`) between patterns, the combat
/// state that is currently telegraphing it and the pending-attack queue that later rolls it, so
/// "the same attack" stays pointer-identical like the C# reference type.
///
/// The C# constructor has many defaulted parameters; here `new` takes the required four plus the
/// style and the optional ones are set with the `with_*` builders (each validates like the C#
/// constructor and panics on bad data).
#[derive(Debug, Clone)]
pub struct AttackDefinition {
    pub name: String,
    pub range_kind: AttackRangeKind,
    pub range: i32,
    pub telegraph_ticks: i32,
    pub style: AttackStyle,
    /// Fraction of damage that gets through a CORRECT protection prayer (0 = fully blocked,
    /// 1 = "ignore"); see `reaction_resolver`.
    pub prayer_penetration: f32,
    pub reaction: ReactionRequirement,
    /// Ticks between the telegraph ending and the accuracy/damage roll ("Doom-style" attacks).
    pub impact_delay_ticks: i32,
    /// Ticks between the roll and the damage actually landing.
    pub projectile_travel_ticks: i32,
    pub on_hit_status_effect: Option<StatusEffectDefinition>,
    pub spawns_orb: Option<Rc<HomingPulseOrbDefinition>>,
    /// Which defence bonus this attack rolls against (stab/slash/crush for melee). Defaults to the
    /// style's usual type.
    pub damage_type: DamageType,
    pub max_hit: MaxHitSource,
    /// Conditional effects on this attack's rolls (gear, set, special-attack and monster effects).
    pub modifiers: Vec<Modifier>,
    /// Free-form flags that `Condition::Flag` reads (`on_task`, `special`, ...).
    pub flags: Vec<String>,
    /// Separate accuracy/damage rolls per use (dragon dagger special: 2, dragon claws: 4). The
    /// caller rolls and enqueues each hit on its own, so each lands in the damage buffer on its own.
    pub hit_count: i32,
    /// Ticks the attacker must wait after this attack resolves before it may begin its next
    /// telegraph. `with_attack_speed` derives it from an OSRS attack speed; 0 keeps the original
    /// back-to-back behaviour.
    pub cooldown_ticks: i32,
    /// Skips the accuracy roll entirely (and its RNG draw): the hit always connects.
    pub always_hits: bool,
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
}

impl AttackDefinition {
    pub fn new(
        name: impl Into<String>,
        range_kind: AttackRangeKind,
        range: i32,
        telegraph_ticks: i32,
        style: AttackStyle,
    ) -> Self {
        Self {
            name: name.into(),
            range_kind,
            range,
            telegraph_ticks,
            style,
            prayer_penetration: 0.0,
            reaction: ReactionRequirement::default(),
            impact_delay_ticks: 0,
            projectile_travel_ticks: 0,
            on_hit_status_effect: None,
            spawns_orb: None,
            damage_type: DamageType::default_for(style),
            max_hit: MaxHitSource::Derived,
            modifiers: Vec::new(),
            flags: Vec::new(),
            hit_count: 1,
            always_hits: false,
            cooldown_ticks: 0,
            animation: None,
            projectile: None,
            target_graphic: None,
            impact_graphic: None,
            sound: None,
            hit_sound: None,
        }
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

    pub fn with_damage_type(mut self, damage_type: DamageType) -> Self {
        assert_eq!(damage_type.style(), self.style, "damage type must belong to the attack's style");
        self.damage_type = damage_type;
        self
    }

    pub fn with_max_hit(mut self, max_hit: MaxHitSource) -> Self {
        self.max_hit = max_hit;
        self
    }

    pub fn with_modifiers(mut self, modifiers: Vec<Modifier>) -> Self {
        self.modifiers = modifiers;
        self
    }

    pub fn with_flags(mut self, flags: Vec<String>) -> Self {
        self.flags = flags;
        self
    }

    pub fn with_hit_count(mut self, hit_count: i32) -> Self {
        assert!(hit_count >= 1, "hit_count must be at least 1");
        self.hit_count = hit_count;
        self
    }

    pub fn with_always_hits(mut self, always_hits: bool) -> Self {
        self.always_hits = always_hits;
        self
    }

    pub fn with_prayer_penetration(mut self, prayer_penetration: f32) -> Self {
        assert!((0.0..=1.0).contains(&prayer_penetration), "prayer_penetration must be within [0, 1]");
        self.prayer_penetration = prayer_penetration;
        self
    }

    pub fn with_reaction(mut self, reaction: ReactionRequirement) -> Self {
        self.reaction = reaction;
        self
    }

    pub fn with_impact_delay_ticks(mut self, impact_delay_ticks: i32) -> Self {
        assert!(impact_delay_ticks >= 0, "impact_delay_ticks must not be negative");
        self.impact_delay_ticks = impact_delay_ticks;
        self
    }

    pub fn with_projectile_travel_ticks(mut self, projectile_travel_ticks: i32) -> Self {
        assert!(projectile_travel_ticks >= 0, "projectile_travel_ticks must not be negative");
        self.projectile_travel_ticks = projectile_travel_ticks;
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

    pub fn with_spawns_orb(mut self, orb: Rc<HomingPulseOrbDefinition>) -> Self {
        self.spawns_orb = Some(orb);
        self
    }

    /// Wraps the finished definition for sharing.
    pub fn shared(self) -> Rc<Self> {
        Rc::new(self)
    }

    /// The target spec an attacker standing at `attacker_tile` with the given footprint must
    /// satisfy for this attack to be in range of a tile.
    pub fn build_range_check(&self, attacker_tile: TileCoord, attacker_footprint: i32) -> TargetSpec {
        match self.range_kind {
            AttackRangeKind::Melee => TargetSpec::adjacent(attacker_tile, attacker_footprint, attacker_footprint),
            AttackRangeKind::Ranged => TargetSpec::within_range_of(attacker_tile, self.range),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_the_optional_fields() {
        let attack = AttackDefinition::new("Test", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee);

        assert_eq!(attack.prayer_penetration, 0.0);
        assert_eq!(attack.reaction, ReactionRequirement::none());
        assert_eq!(attack.impact_delay_ticks, 0);
        assert_eq!(attack.projectile_travel_ticks, 0);
        assert!(attack.on_hit_status_effect.is_none());
        assert!(attack.spawns_orb.is_none());
    }

    #[test]
    #[should_panic]
    fn prayer_penetration_above_one_panics() {
        AttackDefinition::new("Test", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee).with_prayer_penetration(1.5);
    }

    #[test]
    #[should_panic]
    fn negative_impact_delay_panics() {
        AttackDefinition::new("Test", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee).with_impact_delay_ticks(-1);
    }

    #[test]
    fn melee_range_check_is_adjacency_to_the_attackers_footprint() {
        let attack = AttackDefinition::new("Test", AttackRangeKind::Melee, 0, 1, AttackStyle::Melee);

        let check = attack.build_range_check(TileCoord::new(5, 5), 2);

        assert!(check.is_satisfied_by(TileCoord::new(4, 5)));
        assert!(check.is_satisfied_by(TileCoord::new(7, 6)));
        assert!(!check.is_satisfied_by(TileCoord::new(5, 5)));
    }

    #[test]
    fn ranged_range_check_is_chebyshev_distance_from_the_attacker_tile() {
        let attack = AttackDefinition::new("Test", AttackRangeKind::Ranged, 3, 1, AttackStyle::Magic);

        let check = attack.build_range_check(TileCoord::new(5, 5), 1);

        assert!(check.is_satisfied_by(TileCoord::new(8, 8)));
        assert!(!check.is_satisfied_by(TileCoord::new(9, 5)));
    }
}
