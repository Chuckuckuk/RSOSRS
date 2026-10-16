use std::rc::Rc;

use crate::actors::{MovementPolicy, StationaryMovementPolicy};
use crate::combat::{AttackDefinition, AttackPattern};

/// A special a phase fires on its own clock, whatever its attack cycle is doing (a rock that falls
/// every few ticks while the boss fires).
#[derive(Clone)]
pub struct PeriodicSpecial {
    pub every_ticks: i32,
    /// A special with no wind-up of its own.
    pub attack: Rc<AttackDefinition>,
}

/// One stage of a boss fight: which attacks it picks from and how it moves. Phases are swapped
/// wholesale by `TriggerAction::SwapPhase`.
#[derive(Clone)]
pub struct Phase {
    pub name: String,
    pub attack_pattern: Rc<dyn AttackPattern>,
    pub movement_policy: Rc<dyn MovementPolicy>,
    /// Specials that go off on their own clock while this phase lasts.
    pub periodic: Vec<PeriodicSpecial>,
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
            periodic: Vec::new(),
        }
    }

    /// Fires `attack` (a special with no wind-up) every `every_ticks` ticks of the fight.
    pub fn with_periodic(mut self, every_ticks: i32, attack: Rc<AttackDefinition>) -> Self {
        assert!(every_ticks >= 1, "a periodic special needs a period of at least one tick");
        assert!(attack.special_part().is_some_and(|special| special.telegraph_ticks == 0), "a periodic attack is a special with no wind-up");
        self.periodic.push(PeriodicSpecial { every_ticks, attack });
        self
    }
}
