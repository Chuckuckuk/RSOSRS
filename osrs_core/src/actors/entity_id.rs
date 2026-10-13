/// Stable handle for an entity owned by the world. Replaces the shared object references the C#
/// code passes around (a queued attack's attacker, an encounter's participants, a healer's
/// target): anything that needs to point at another entity stores one of these and the world
/// resolves it. See `PARITY_PLAN.md`, design decision 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntityId(pub u32);
