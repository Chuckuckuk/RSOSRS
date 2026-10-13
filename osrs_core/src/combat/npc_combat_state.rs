use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_pattern::AttackPattern;
use super::attack_selection_context::AttackSelectionContext;
use super::attack_style::AttackStyle;
use crate::rng::Rng;
use crate::tiles::TileCoord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CombatState {
    Idle,
    Telegraphing,
}

/// The telegraph-then-resolve state machine for one NPC, driven by an `AttackPattern`. Owns the
/// per-NPC selection counter the pattern reads via `AttackSelectionContext` - the pattern itself
/// stays a pure function, reusable across NPCs, while this (one instance per NPC) is where
/// "which attack number is this for ME" actually lives.
///
/// Deliberately ignorant of `AttackDefinition::impact_delay_ticks` ("Doom-style" attacks):
/// `advance` always resolves the instant a telegraph ends (an attack without one resolves the tick
/// it is chosen), full stop. Whether the caller then
/// applies the roll immediately or hands it off to the DEFENDER's `PendingAttackQueue` for a
/// deferred roll is the caller's decision, read off `current_attack().impact_delay_ticks` - that
/// delay belongs to the ATTACK, which outlives this NPC's own turn to act, not to this state
/// machine.
///
/// Once telegraphing starts it commits unconditionally: `advance` ignores `target_engaged` until
/// it resolves, and the attack selected at telegraph-start is final.
pub struct NpcCombatState {
    pattern: Rc<dyn AttackPattern>,
    ticks_remaining: i32,
    cooldown_remaining: i32,
    selection_counter: i32,
    state: CombatState,
    current_attack: Option<Rc<AttackDefinition>>,
    self_tile_at_selection: TileCoord,
    target_tile_at_selection: TileCoord,
}

impl NpcCombatState {
    pub fn new(pattern: Rc<dyn AttackPattern>) -> Self {
        Self {
            pattern,
            ticks_remaining: 0,
            cooldown_remaining: 0,
            selection_counter: 0,
            state: CombatState::Idle,
            current_attack: None,
            self_tile_at_selection: TileCoord::default(),
            target_tile_at_selection: TileCoord::default(),
        }
    }

    pub fn state(&self) -> CombatState {
        self.state
    }

    /// The attack selected for the current/most recent telegraph (`None` before the first one).
    pub fn current_attack(&self) -> Option<&Rc<AttackDefinition>> {
        self.current_attack.as_ref()
    }

    /// Snapshot of `self_tile` from the exact `advance` call that STARTED the current/most
    /// recent telegraph - never updated again until the next selection, even though the live
    /// tiles may drift during a multi-tick telegraph. Needed for any reaction anchored to a
    /// position fixed at cast time.
    pub fn self_tile_at_selection(&self) -> TileCoord {
        self.self_tile_at_selection
    }

    /// Same snapshot rule as `self_tile_at_selection`, for the target's tile.
    pub fn target_tile_at_selection(&self) -> TileCoord {
        self.target_tile_at_selection
    }

    /// Wholesale-replaces which pattern future selections draw from, without resetting
    /// state/ticks/current attack - a swap mid-telegraph still resolves whatever was already
    /// selected, only the NEXT selection sees the new phase's pattern.
    pub fn set_pattern(&mut self, new_pattern: Rc<dyn AttackPattern>) {
        self.pattern = new_pattern;
    }

    /// Advances the state machine by one tick. Returns true exactly on the tick the telegraph
    /// ends (for an attack with no telegraph, the tick it is chosen) - the caller reads `current_attack` and its `impact_delay_ticks` to decide whether
    /// to roll now or defer to a `PendingAttackQueue`. `target_engaged` gates whether a new
    /// telegraph may START.
    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        target_engaged: bool,
        health_percent: f32,
        elapsed_ticks: i32,
        self_tile: TileCoord,
        target_tile: TileCoord,
        target_combat_style: AttackStyle,
        rng: &mut dyn Rng,
    ) -> bool {
        if self.state == CombatState::Idle {
            // The attack-speed gap after the previous resolve: counts down whether or not anything
            // is in range, like the game's own attack timer.
            if self.cooldown_remaining > 0 {
                self.cooldown_remaining -= 1;
                return false;
            }
            if !target_engaged {
                return false;
            }

            let context = AttackSelectionContext::new(
                health_percent,
                elapsed_ticks,
                self.selection_counter,
                self_tile,
                target_tile,
                target_combat_style,
            );
            let attack = self.pattern.select_next(&context, rng);
            self.selection_counter += 1;
            self.self_tile_at_selection = self_tile;
            self.target_tile_at_selection = target_tile;
            // an attack without a telegraph (most melee) resolves on the tick it is chosen
            if attack.telegraph_ticks <= 0 {
                self.cooldown_remaining = attack.cooldown_ticks;
                self.current_attack = Some(attack);
                return true;
            }
            self.state = CombatState::Telegraphing;
            self.ticks_remaining = attack.telegraph_ticks;
            self.current_attack = Some(attack);
            return false;
        }

        // state == Telegraphing
        self.ticks_remaining -= 1;
        if self.ticks_remaining > 0 {
            return false;
        }

        self.state = CombatState::Idle;
        self.cooldown_remaining = self.current_attack.as_ref().map_or(0, |attack| attack.cooldown_ticks);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_support::melee_attack;
    use crate::combat::{AttackRangeKind, FixedRotationPattern};
    use crate::rng::DotNetRandom;

    fn state_with(attacks: Vec<Rc<AttackDefinition>>) -> NpcCombatState {
        NpcCombatState::new(Rc::new(FixedRotationPattern::new(attacks)))
    }

    fn advance(state: &mut NpcCombatState, target_in_range: bool) -> bool {
        state.advance(target_in_range, 1.0, 0, TileCoord::default(), TileCoord::new(0, 0), AttackStyle::Melee, &mut DotNetRandom::new(1))
    }

    #[test]
    fn advance_stays_idle_while_target_out_of_range() {
        let mut state = state_with(vec![melee_attack("A", 2)]);

        assert!(!advance(&mut state, false));
        assert!(!advance(&mut state, false));
        assert_eq!(state.state(), CombatState::Idle);
        assert!(state.current_attack().is_none());
    }

    #[test]
    fn advance_resolves_exactly_telegraph_ticks_after_starting() {
        // telegraph_ticks=3: the START call sets ticks_remaining=3 and returns false (the
        // telegraph has only just begun); it then takes 3 more calls to count down to the
        // resolve, so 4 calls total.
        let mut state = state_with(vec![melee_attack("A", 3)]);

        assert!(!advance(&mut state, true)); // starts telegraphing
        assert_eq!(state.state(), CombatState::Telegraphing);
        assert!(!advance(&mut state, true));
        assert!(!advance(&mut state, true));
        assert!(advance(&mut state, true)); // 4th call: resolves
        assert_eq!(state.state(), CombatState::Idle);
    }

    #[test]
    fn advance_once_telegraphing_commits_regardless_of_range() {
        // Deliberate simplification: once telegraphing starts, it resolves unconditionally -
        // target_engaged is only consulted while Idle.
        let mut state = state_with(vec![melee_attack("A", 1)]);
        advance(&mut state, true); // starts telegraphing

        assert!(advance(&mut state, false)); // resolves even though out of range
    }

    #[test]
    fn advance_after_resolving_can_telegraph_again() {
        let mut state = state_with(vec![melee_attack("A", 1)]);
        advance(&mut state, true); // starts
        assert!(advance(&mut state, true)); // resolves
        assert_eq!(state.state(), CombatState::Idle);

        assert!(!advance(&mut state, true)); // starts a new telegraph
        assert_eq!(state.state(), CombatState::Telegraphing);
    }

    #[test]
    fn advance_selects_from_the_pattern_and_exposes_it_as_current_attack() {
        let (first, second) = (melee_attack("First", 1), melee_attack("Second", 1));
        let mut state = state_with(vec![first.clone(), second.clone()]);

        advance(&mut state, true); // selects `first` (counter 0)
        assert!(Rc::ptr_eq(state.current_attack().unwrap(), &first));
        advance(&mut state, true); // resolves `first`, idle again
        advance(&mut state, true); // selects `second` (counter 1)
        assert!(Rc::ptr_eq(state.current_attack().unwrap(), &second));
    }

    #[test]
    fn set_pattern_swaps_what_future_selections_draw_from() {
        let (old_attack, new_attack) = (melee_attack("Old", 1), melee_attack("New", 1));
        let mut state = state_with(vec![old_attack]);
        advance(&mut state, true); // selects the old attack
        advance(&mut state, true); // resolves it, back to idle

        state.set_pattern(Rc::new(FixedRotationPattern::new(vec![new_attack.clone()])));
        advance(&mut state, true); // next selection comes from the new pattern

        assert!(Rc::ptr_eq(state.current_attack().unwrap(), &new_attack));
    }

    #[test]
    fn advance_resolves_the_instant_the_telegraph_ends_regardless_of_impact_delay_ticks() {
        // impact_delay_ticks ("Doom-style") is no longer this type's concern - honoring it is
        // entirely the caller's job, read off current_attack. This state machine never differs:
        // Idle -> Telegraphing -> resolve, whether the delay is 0 or 50.
        let attack = AttackDefinition::new("Test Doom-style", AttackRangeKind::Ranged, 10, 2, AttackStyle::Magic)
            .with_impact_delay_ticks(5)
            .shared();
        let mut state = state_with(vec![attack]);

        assert!(!advance(&mut state, true)); // starts telegraphing
        assert!(!advance(&mut state, true));
        assert!(advance(&mut state, true)); // resolves - no travel phase, ever

        assert_eq!(state.state(), CombatState::Idle);
        assert_eq!(
            state.current_attack().unwrap().impact_delay_ticks,
            5,
            "The caller still needs to read this off current_attack to know whether to defer the roll."
        );
    }

    #[test]
    fn self_and_target_tile_at_selection_are_snapshotted_only_at_telegraph_start() {
        // A target-anchored hazard needs to resolve against where the target WAS when the
        // telegraph started, not wherever either side has drifted to by resolve time - otherwise
        // moving away would never actually dodge it.
        let mut state = state_with(vec![melee_attack("A", 2)]);
        let (self_at_start, target_at_start) = (TileCoord::new(1, 1), TileCoord::new(9, 9));
        let mut rng = DotNetRandom::new(1);

        state.advance(true, 1.0, 0, self_at_start, target_at_start, AttackStyle::Melee, &mut rng); // starts telegraphing
        assert_eq!(state.self_tile_at_selection(), self_at_start);
        assert_eq!(state.target_tile_at_selection(), target_at_start);

        // Both sides have moved mid-telegraph - the snapshot must not follow them.
        state.advance(true, 1.0, 0, TileCoord::new(2, 2), TileCoord::new(3, 3), AttackStyle::Melee, &mut rng);
        assert_eq!(state.self_tile_at_selection(), self_at_start);
        assert_eq!(state.target_tile_at_selection(), target_at_start);

        let resolved = state.advance(true, 1.0, 0, TileCoord::new(4, 4), TileCoord::new(5, 5), AttackStyle::Melee, &mut rng);
        assert!(resolved);
        assert_eq!(state.self_tile_at_selection(), self_at_start, "Still the tile from telegraph-start, even on the resolve tick itself.");
        assert_eq!(state.target_tile_at_selection(), target_at_start);
    }

    #[test]
    fn attack_speed_spaces_consecutive_resolves_exactly_that_many_ticks_apart() {
        for (telegraph, speed) in [(0, 4), (2, 6), (1, 5), (0, 2)] {
            let attack = melee_attack("A", telegraph).as_ref().clone().with_attack_speed(speed).shared();
            let mut state = state_with(vec![attack]);
            let mut resolves = Vec::new();
            for call in 0..40 {
                if advance(&mut state, true) {
                    resolves.push(call);
                }
            }
            let gaps: Vec<i32> = resolves.windows(2).map(|w| w[1] - w[0]).collect();
            assert!(!gaps.is_empty());
            assert!(gaps.iter().all(|gap| *gap == speed), "telegraph {telegraph} speed {speed} gaps {gaps:?}");
        }
    }

    #[test]
    fn an_attack_without_a_telegraph_resolves_on_the_tick_it_is_chosen() {
        let attack = melee_attack("Punch", 0).as_ref().clone().with_attack_speed(4).shared();
        let mut state = state_with(vec![attack]);

        let resolves: Vec<i32> = (0..10).filter(|_| advance(&mut state, true)).collect();

        assert_eq!(resolves, vec![0, 4, 8], "the first swing comes on the very first tick in range");
        assert_eq!(state.state(), CombatState::Idle, "it never sits in a telegraph");
    }
}
