use super::actor_kind::ActorKind;
use crate::combat::CombatStats;
use crate::tiles::TileCoord;

/// What the rest of the core needs to know about something that can be attacked or chased - the
/// read-only half of the C# `ICombatTarget`. The other half (its `DamageBuffer` and
/// `PendingAttackQueue`) is passed separately as `&mut` where needed, so an entity that owns all
/// of these can lend out a snapshot of its identity without freezing its own buffers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatTargetView {
    pub tile: TileCoord,
    pub footprint_size: i32,
    pub stats: CombatStats,
    pub kind: ActorKind,
}
