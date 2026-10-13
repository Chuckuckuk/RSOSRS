use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_selection_context::AttackSelectionContext;
use crate::rng::Rng;
use crate::tiles::TileCoord;

/// Picks which `AttackDefinition` an NPC uses next. Implementations must be pure functions of
/// (context, rng) - no hidden mutable state - so the same pattern instance is safe to reuse and
/// its selections stay fully determined by the encounter's seeded RNG.
pub trait AttackPattern {
    fn select_next(&self, context: &AttackSelectionContext, rng: &mut dyn Rng) -> Rc<AttackDefinition>;

    /// Whether this pattern has an attack that can reach `target_tile` from `self_tile` right now,
    /// or `None` when it does not know about reach (every pattern except range-aware ones).
    fn reaches(&self, _self_tile: TileCoord, _target_tile: TileCoord) -> Option<bool> {
        None
    }

    /// Whether a new attack may start: one that reaches, or always for a pattern that does not
    /// know about reach.
    fn engages(&self, self_tile: TileCoord, target_tile: TileCoord) -> bool {
        self.reaches(self_tile, target_tile).unwrap_or(true)
    }
}
