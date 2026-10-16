use super::attack_style::AttackStyle;
use super::max_hit_source::MaxHitSource;
use super::mitigation::Mitigation;
use crate::model::{DamageType, Modifier};

/// What an attack that is rolled against a defender has beyond the common fields of an
/// `AttackDefinition`: a combat style and the defence bonus it rolls against, the dice that decide
/// whether it hits and for how much, and how the defender's prayers change the outcome. A ground-targeted attack has none of this.
#[derive(Debug, Clone)]
pub struct TargetedAttack {
    pub style: AttackStyle,
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
    /// Skips the accuracy roll entirely (and its RNG draw): the hit always connects.
    pub always_hits: bool,
    /// Fraction of damage that still gets through when the defender meets `mitigation` (0 =
    /// fully blocked, 1 = "ignore"); see `reaction_resolver`.
    pub prayer_penetration: f32,
    /// What lets the defender cut this attack down: the protection prayer for its style unless it
    /// says otherwise.
    pub mitigation: Mitigation,
}

impl TargetedAttack {
    pub fn new(style: AttackStyle) -> Self {
        Self {
            style,
            damage_type: DamageType::default_for(style),
            max_hit: MaxHitSource::Derived,
            modifiers: Vec::new(),
            flags: Vec::new(),
            hit_count: 1,
            always_hits: false,
            prayer_penetration: 0.0,
            mitigation: Mitigation::default(),
        }
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

    pub fn with_mitigation(mut self, mitigation: Mitigation) -> Self {
        self.mitigation = mitigation;
        self
    }
}
