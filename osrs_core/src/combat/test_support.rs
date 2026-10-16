//! Shared helpers for combat unit tests (the C# `FakeCombatTarget` and friends).

use std::rc::Rc;

use super::{AttackDefinition, AttackStyle, CombatStats, DamageBuffer};
use crate::actors::{ActorKind, CombatTargetView, Health};
use crate::tiles::TileCoord;

/// A bare-bones defender: a health pool, a damage buffer and the read-only view other systems
/// target it through. Fields are public so tests can borrow them separately.
pub(crate) struct FakeCombatTarget {
    pub health: Health,
    pub view: CombatTargetView,
    pub damage_buffer: DamageBuffer,
}

impl FakeCombatTarget {
    pub fn new(max_health: i32) -> Self {
        Self {
            health: Health::new(max_health),
            view: CombatTargetView {
                tile: TileCoord::default(),
                footprint_size: 1,
                stats: CombatStats::new(50, 50, 50, 50, 50).unwrap(),
                kind: ActorKind::Npc,
            },
            damage_buffer: DamageBuffer::new(),
        }
    }
}

/// A plain melee attack with the given telegraph, for tests that only care about identity/timing.
pub(crate) fn melee_attack(name: &str, telegraph_ticks: i32) -> Rc<AttackDefinition> {
    if telegraph_ticks == 0 {
        AttackDefinition::instant(name, None, AttackStyle::Melee).shared()
    } else {
        AttackDefinition::telegraphed(name, None, telegraph_ticks, AttackStyle::Melee).shared()
    }
}
