use std::rc::Rc;

use super::attack_definition::AttackDefinition;

/// Static data for a Slicer-style orb that homes on the target and pulses damage.
#[derive(Debug, Clone)]
pub struct HomingPulseOrbDefinition {
    pub name: String,
    pub initial_duration_ticks: i32,
    pub pulse_interval_ticks: i32,
    pub pulse_radius: i32,
    pub duration_bonus_on_hit: i32,
    pub pulse_attack: Rc<AttackDefinition>,
}

impl HomingPulseOrbDefinition {
    /// Panics on invalid data (mirrors the C# `ArgumentOutOfRangeException`s).
    pub fn new(
        name: impl Into<String>,
        initial_duration_ticks: i32,
        pulse_interval_ticks: i32,
        pulse_radius: i32,
        duration_bonus_on_hit: i32,
        pulse_attack: Rc<AttackDefinition>,
    ) -> Self {
        assert!(initial_duration_ticks > 0, "initial_duration_ticks must be positive");
        assert!(pulse_interval_ticks > 0, "pulse_interval_ticks must be positive");
        assert!(pulse_radius >= 0, "pulse_radius must not be negative");
        assert!(duration_bonus_on_hit >= 0, "duration_bonus_on_hit must not be negative");

        Self {
            name: name.into(),
            initial_duration_ticks,
            pulse_interval_ticks,
            pulse_radius,
            duration_bonus_on_hit,
            pulse_attack,
        }
    }
}
