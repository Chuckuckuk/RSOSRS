use std::rc::Rc;

use crate::actors::{MovementPolicy, StationaryMovementPolicy};
use crate::combat::AttackPattern;

/// One stage of a boss fight: which attacks it picks from and how it moves. Phases are swapped
/// wholesale by `TriggerAction::SwapPhase`.
#[derive(Clone)]
pub struct Phase {
    pub name: String,
    pub attack_pattern: Rc<dyn AttackPattern>,
    pub movement_policy: Rc<dyn MovementPolicy>,
}

impl Phase {
    /// `movement_policy` defaults to stationary when `None`.
    pub fn new(
        name: impl Into<String>,
        attack_pattern: Rc<dyn AttackPattern>,
        movement_policy: Option<Rc<dyn MovementPolicy>>,
    ) -> Self {
        Self {
            name: name.into(),
            attack_pattern,
            movement_policy: movement_policy.unwrap_or_else(|| Rc::new(StationaryMovementPolicy)),
        }
    }
}
