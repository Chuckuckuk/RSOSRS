/// The phases of one game tick, in declaration order. The actual run order is decided by
/// `TickScheduler` (the NPC/player movement phases swap with `NPCS_MOVE_BEFORE_PLAYERS`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TickPhase {
    PlayerQueue,
    NpcAi,
    InteractionCheckPre,
    NpcMovement,
    PlayerMovement,
    InteractionCheckPost,
    Regen,
}

impl TickPhase {
    pub(crate) const COUNT: usize = 7;

    pub(crate) fn index(self) -> usize {
        self as usize
    }
}
