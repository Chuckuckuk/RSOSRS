//! The OSRS combat rules as pure functions and small state machines: no entities, no world, no
//! RNG. Everything in `OSRS_COMBAT_REFERENCE.md` that is a formula or a rate lives here, in
//! integer arithmetic wherever the real game floors, so a result is exactly what the game would
//! compute. The sim and the data layer call into this module; it calls into neither.

mod formulas;
mod ratio;
mod resources;

pub use formulas::*;
pub use ratio::Ratio;
pub use resources::*;
