#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusEffectKind {
    Corruption,
    Burn,
    DisableRunning,
    Stun,
    /// Periodic damage every 30 ticks, `magnitude` = severity (see `rules::poison_damage`).
    Poison,
    /// Periodic damage every 30 ticks that climbs 6, 8, ... 20 and never wears off by itself.
    Venom,
    /// Cannot move (Bind/Snare/Entangle); attacking and prayer are unaffected.
    Freeze,
}

/// A status effect an attack applies when it lands. Constructed through the named constructors
/// below, which panic on a non-positive magnitude/duration (mirrors the C#
/// `ArgumentOutOfRangeException` - bad attack data is a programmer error).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusEffectDefinition {
    pub kind: StatusEffectKind,
    pub duration_ticks: i32,
    pub magnitude: i32,
}

impl StatusEffectDefinition {
    pub fn corruption(prayer_drain_per_tick: i32, duration_ticks: i32) -> Self {
        assert!(prayer_drain_per_tick > 0, "prayer_drain_per_tick must be positive");
        assert!(duration_ticks > 0, "duration_ticks must be positive");
        Self { kind: StatusEffectKind::Corruption, duration_ticks, magnitude: prayer_drain_per_tick }
    }

    pub fn burn(damage_per_tick: i32, duration_ticks: i32) -> Self {
        assert!(damage_per_tick > 0, "damage_per_tick must be positive");
        assert!(duration_ticks > 0, "duration_ticks must be positive");
        Self { kind: StatusEffectKind::Burn, duration_ticks, magnitude: damage_per_tick }
    }

    pub fn disable_running(duration_ticks: i32) -> Self {
        assert!(duration_ticks > 0, "duration_ticks must be positive");
        Self { kind: StatusEffectKind::DisableRunning, duration_ticks, magnitude: 0 }
    }

    /// A poison of the given starting severity (the wiki examples run from about 10 to 60).
    pub fn poison(severity: i32) -> Self {
        assert!(severity > 0, "severity must be positive");
        Self { kind: StatusEffectKind::Poison, duration_ticks: 0, magnitude: severity }
    }

    pub fn venom() -> Self {
        Self { kind: StatusEffectKind::Venom, duration_ticks: 0, magnitude: 0 }
    }

    pub fn freeze(duration_ticks: i32) -> Self {
        assert!(duration_ticks > 0, "duration_ticks must be positive");
        Self { kind: StatusEffectKind::Freeze, duration_ticks, magnitude: 0 }
    }

    pub fn stun(duration_ticks: i32) -> Self {
        assert!(duration_ticks > 0, "duration_ticks must be positive");
        Self { kind: StatusEffectKind::Stun, duration_ticks, magnitude: 0 }
    }
}
