use super::attack_style::AttackStyle;
use crate::tiles::TileCoord;

/// The live state an `AttackPattern` may base its choice on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttackSelectionContext {
    pub health_percent: f32,
    pub elapsed_ticks: i32,
    /// How many attacks this NPC has selected so far (owned by `NpcCombatState`).
    pub counter: i32,
    pub self_tile: TileCoord,
    pub target_tile: TileCoord,
    pub target_combat_style: AttackStyle,
}

impl AttackSelectionContext {
    pub fn new(
        health_percent: f32,
        elapsed_ticks: i32,
        counter: i32,
        self_tile: TileCoord,
        target_tile: TileCoord,
        target_combat_style: AttackStyle,
    ) -> Self {
        Self { health_percent, elapsed_ticks, counter, self_tile, target_tile, target_combat_style }
    }
}
