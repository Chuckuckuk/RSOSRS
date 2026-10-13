/// What happens when a `TriggerBinding` fires. The C# `TriggerActionKind` + payload fields
/// become one enum with the payload on each variant.
#[derive(Debug, Clone, PartialEq)]
pub enum TriggerAction {
    SwapPhase { phase_name: String },
    SpawnEntities { spawn_id: String },
    SetFlag { flag_name: String, value: bool },
    ModifyStats { stat_name: String, delta: f32 },
}

impl TriggerAction {
    pub fn swap_phase(phase_name: impl Into<String>) -> Self {
        Self::SwapPhase { phase_name: phase_name.into() }
    }

    pub fn spawn_entities(spawn_id: impl Into<String>) -> Self {
        Self::SpawnEntities { spawn_id: spawn_id.into() }
    }

    pub fn set_flag(flag_name: impl Into<String>, value: bool) -> Self {
        Self::SetFlag { flag_name: flag_name.into(), value }
    }

    pub fn modify_stat(stat_name: impl Into<String>, delta: f32) -> Self {
        Self::ModifyStats { stat_name: stat_name.into(), delta }
    }
}
