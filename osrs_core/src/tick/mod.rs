mod tick_accumulator;
mod tick_phase;
mod tick_scheduler;

pub use tick_accumulator::TickAccumulator;
pub use tick_phase::TickPhase;
pub use tick_scheduler::{TickRegistrar, TickScheduler, NPCS_MOVE_BEFORE_PLAYERS};
