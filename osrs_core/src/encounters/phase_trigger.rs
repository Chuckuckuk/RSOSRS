use super::encounter_context::EncounterContext;

/// The condition that fires a `TriggerBinding`. The C# `PhaseTriggerKind` + payload fields
/// become one enum with the payload on each variant.
#[derive(Debug, Clone, PartialEq)]
pub enum PhaseTrigger {
    /// The tracked entity's health percent has dropped to or below the threshold.
    HpThreshold { health_percent_threshold: f32 },
    /// The encounter has run for at least this many ticks.
    ElapsedTicks { tick_threshold: i32 },
    /// A named encounter flag equals the expected value.
    EncounterFlag { flag_name: String, expected_value: bool },
    /// The tracked entity has died.
    EntityDied,
}

impl PhaseTrigger {
    pub fn hp_threshold(health_percent_threshold: f32) -> Self {
        Self::HpThreshold { health_percent_threshold }
    }

    pub fn at_elapsed_ticks(tick_threshold: i32) -> Self {
        Self::ElapsedTicks { tick_threshold }
    }

    pub fn when_flag(flag_name: impl Into<String>, expected_value: bool) -> Self {
        Self::EncounterFlag { flag_name: flag_name.into(), expected_value }
    }

    pub fn when_entity_died() -> Self {
        Self::EntityDied
    }
}

/// Whether `trigger` is currently satisfied (the C# `PhaseTriggerEvaluator.IsSatisfied`).
pub fn is_satisfied(
    trigger: &PhaseTrigger,
    encounter: &EncounterContext,
    tracked_health_percent: f32,
    tracked_entity_died: bool,
) -> bool {
    match trigger {
        PhaseTrigger::HpThreshold { health_percent_threshold } => tracked_health_percent <= *health_percent_threshold,
        PhaseTrigger::ElapsedTicks { tick_threshold } => encounter.elapsed_ticks() >= *tick_threshold,
        PhaseTrigger::EncounterFlag { flag_name, expected_value } => encounter.flag(flag_name) == *expected_value,
        PhaseTrigger::EntityDied => tracked_entity_died,
    }
}
