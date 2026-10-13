/// What an Actor is, for the purposes of deriving its collision semantics. Deliberately not a
/// raw `OccupancyMask` on the actor itself - that would let a caller mismatch "what I occupy
/// as" against "what I respect" (see `Actor::respects`/`occupies_as`), which OSRS never allows: a
/// player always respects NPCs only, an NPC always respects everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorKind {
    Player,
    Npc,
}
