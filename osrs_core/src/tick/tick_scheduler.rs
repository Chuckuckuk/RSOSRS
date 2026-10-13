use super::tick_phase::TickPhase;

/// When true, NPC movement runs before player movement within a tick (the OSRS server order);
/// flipping it swaps just those two phases and nothing else in the schedule.
pub const NPCS_MOVE_BEFORE_PLAYERS: bool = true;

const PHASE_ORDER_NPC_FIRST: [TickPhase; TickPhase::COUNT] = [
    TickPhase::PlayerQueue,
    TickPhase::NpcAi,
    TickPhase::InteractionCheckPre,
    TickPhase::NpcMovement,
    TickPhase::PlayerMovement,
    TickPhase::InteractionCheckPost,
    TickPhase::Regen,
];

const PHASE_ORDER_PLAYER_FIRST: [TickPhase; TickPhase::COUNT] = [
    TickPhase::PlayerQueue,
    TickPhase::NpcAi,
    TickPhase::InteractionCheckPre,
    TickPhase::PlayerMovement,
    TickPhase::NpcMovement,
    TickPhase::InteractionCheckPost,
    TickPhase::Regen,
];

const PHASE_ORDER: [TickPhase; TickPhase::COUNT] =
    if NPCS_MOVE_BEFORE_PLAYERS { PHASE_ORDER_NPC_FIRST } else { PHASE_ORDER_PLAYER_FIRST };

#[derive(Debug, Clone, Copy)]
struct PendingChange<Id> {
    phase: TickPhase,
    id: Id,
    register: bool,
}

/// A buffer of register/unregister requests. `TickScheduler` owns one, and `run_tick` hands
/// another to the dispatch callback so a tickable can (un)register things mid-tick - the C#
/// tickables call back into the scheduler directly, which Rust's borrow rules don't allow while
/// the scheduler is iterating. Either way the change is deferred to the next phase boundary.
#[derive(Debug, Clone)]
pub struct TickRegistrar<Id> {
    pending: Vec<PendingChange<Id>>,
}

impl<Id> TickRegistrar<Id> {
    fn new() -> Self {
        Self { pending: Vec::new() }
    }

    pub fn register(&mut self, phase: TickPhase, id: Id) {
        self.pending.push(PendingChange { phase, id, register: true });
    }

    pub fn unregister(&mut self, phase: TickPhase, id: Id) {
        self.pending.push(PendingChange { phase, id, register: false });
    }
}

/// Runs registered tickables phase by phase. Where the C# scheduler holds `ITickable` objects,
/// this one holds opaque `Id`s and hands each `(phase, id, tick)` to a dispatch callback, so the
/// world that owns the entities stays the only owner (see `PARITY_PLAN.md`).
///
/// Registrations and unregistrations are deferred and applied at the start of the next phase,
/// never spliced into the list currently being iterated. Iteration order within a phase is
/// insertion-stable. Occupancy overlays are written sequentially per mover inside the callback,
/// so mover k sees movers <k already moved and movers >k stale - this matches the server's
/// per-index iteration and is intended, not nondeterminism.
#[derive(Debug, Clone)]
pub struct TickScheduler<Id> {
    registered: [Vec<Id>; TickPhase::COUNT],
    registrar: TickRegistrar<Id>,
    current_tick: i32,
}

impl<Id: Copy + PartialEq> TickScheduler<Id> {
    pub fn new() -> Self {
        Self {
            registered: std::array::from_fn(|_| Vec::new()),
            registrar: TickRegistrar::new(),
            current_tick: 0,
        }
    }

    pub fn phase_order() -> [TickPhase; TickPhase::COUNT] {
        PHASE_ORDER
    }

    pub fn current_tick(&self) -> i32 {
        self.current_tick
    }

    pub fn register(&mut self, phase: TickPhase, id: Id) {
        self.registrar.register(phase, id);
    }

    pub fn unregister(&mut self, phase: TickPhase, id: Id) {
        self.registrar.unregister(phase, id);
    }

    /// Advances the tick counter and runs every phase in order, calling
    /// `dispatch(phase, id, tick, registrar)` for each registered id.
    pub fn run_tick(&mut self, mut dispatch: impl FnMut(TickPhase, Id, i32, &mut TickRegistrar<Id>)) {
        self.current_tick += 1;
        let tick = self.current_tick;

        for phase in PHASE_ORDER {
            self.apply_pending_changes();

            // Taken out for the duration of the phase: changes made by the callback land in the
            // registrar and only touch the list at the next phase boundary, so it cannot change
            // while we iterate.
            let list = std::mem::take(&mut self.registered[phase.index()]);
            for &id in &list {
                dispatch(phase, id, tick, &mut self.registrar);
            }
            self.registered[phase.index()] = list;
        }
    }

    fn apply_pending_changes(&mut self) {
        if self.registrar.pending.is_empty() {
            return;
        }

        for change in std::mem::take(&mut self.registrar.pending) {
            let list = &mut self.registered[change.phase.index()];
            if change.register {
                if !list.contains(&change.id) {
                    list.push(change.id);
                }
            } else if let Some(pos) = list.iter().position(|existing| *existing == change.id) {
                list.remove(pos);
            }
        }
    }
}

impl<Id: Copy + PartialEq> Default for TickScheduler<Id> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NAMES: [&str; 6] = ["queueA", "queueB", "npcMove", "playerMove", "regen", "once"];

    fn run_and_log(scheduler: &mut TickScheduler<usize>, log: &mut Vec<&'static str>) {
        scheduler.run_tick(|_, id, _, _| log.push(NAMES[id]));
    }

    #[test]
    fn phase_order_matches_the_documented_schedule_with_npcs_before_players() {
        let order = TickScheduler::<usize>::phase_order();

        assert_eq!(order[0], TickPhase::PlayerQueue);
        assert_eq!(order[1], TickPhase::NpcAi);
        assert_eq!(order[2], TickPhase::InteractionCheckPre);
        // NPCS_MOVE_BEFORE_PLAYERS is true by default - flipping that constant should flip
        // these two without touching anything else in the schedule.
        if NPCS_MOVE_BEFORE_PLAYERS {
            assert_eq!(order[3], TickPhase::NpcMovement);
            assert_eq!(order[4], TickPhase::PlayerMovement);
        } else {
            assert_eq!(order[3], TickPhase::PlayerMovement);
            assert_eq!(order[4], TickPhase::NpcMovement);
        }
        assert_eq!(order[5], TickPhase::InteractionCheckPost);
        assert_eq!(order[6], TickPhase::Regen);
    }

    #[test]
    fn run_tick_invokes_phases_in_order_and_tickables_in_registration_order() {
        let mut scheduler = TickScheduler::new();
        let mut log = Vec::new();
        // Register out of phase order to confirm the scheduler - not registration order -
        // determines phase sequencing.
        scheduler.register(TickPhase::Regen, 4);
        scheduler.register(TickPhase::PlayerQueue, 0);
        scheduler.register(TickPhase::PlayerQueue, 1);
        scheduler.register(TickPhase::NpcMovement, 2);
        scheduler.register(TickPhase::PlayerMovement, 3);

        run_and_log(&mut scheduler, &mut log);

        assert_eq!(log, vec!["queueA", "queueB", "npcMove", "playerMove", "regen"]);
    }

    #[test]
    fn run_tick_increments_current_tick() {
        let mut scheduler = TickScheduler::<usize>::new();
        assert_eq!(scheduler.current_tick(), 0);

        scheduler.run_tick(|_, _, _, _| {});
        assert_eq!(scheduler.current_tick(), 1);

        scheduler.run_tick(|_, _, _, _| {});
        assert_eq!(scheduler.current_tick(), 2);
    }

    #[test]
    fn dispatch_receives_the_current_tick_number() {
        let mut scheduler = TickScheduler::new();
        scheduler.register(TickPhase::NpcAi, 0usize);
        let mut seen = Vec::new();

        scheduler.run_tick(|_, _, tick, _| seen.push(tick));
        scheduler.run_tick(|_, _, tick, _| seen.push(tick));

        assert_eq!(seen, vec![1, 2]);
    }

    #[test]
    fn unregister_stops_further_ticks() {
        let mut scheduler = TickScheduler::new();
        let mut log = Vec::new();
        scheduler.register(TickPhase::PlayerQueue, 5);

        run_and_log(&mut scheduler, &mut log);
        assert_eq!(log.len(), 1);

        scheduler.unregister(TickPhase::PlayerQueue, 5);
        run_and_log(&mut scheduler, &mut log);
        assert_eq!(log.len(), 1, "Tickable should not have run again after unregistering.");
    }

    #[test]
    fn register_during_on_tick_does_not_affect_the_current_phase_iteration() {
        // id 0 is the "registrar": the first time it runs it registers id 1 into the SAME phase
        // it's currently in. That new registration must be buffered until the next phase
        // boundary, never spliced into the list currently being iterated.
        let mut scheduler = TickScheduler::new();
        scheduler.register(TickPhase::PlayerQueue, 0usize);
        let mut log: Vec<usize> = Vec::new();

        let mut registered_once = false;
        let mut run = |scheduler: &mut TickScheduler<usize>, log: &mut Vec<usize>| {
            scheduler.run_tick(|_, id, _, registrar| {
                log.push(id);
                if id == 0 && !registered_once {
                    registered_once = true;
                    registrar.register(TickPhase::PlayerQueue, 1);
                }
            });
        };

        run(&mut scheduler, &mut log);
        assert_eq!(log, vec![0]);

        log.clear();
        run(&mut scheduler, &mut log);
        assert_eq!(log, vec![0, 1]);
    }

    #[test]
    fn registering_the_same_id_twice_in_a_phase_runs_it_once() {
        let mut scheduler = TickScheduler::new();
        scheduler.register(TickPhase::Regen, 4usize);
        scheduler.register(TickPhase::Regen, 4usize);
        let mut log = Vec::new();

        run_and_log(&mut scheduler, &mut log);

        assert_eq!(log, vec!["regen"]);
    }
}
