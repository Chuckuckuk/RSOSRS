//! Locks in a real OSRS mechanic (confirmed against the wiki's prayer-flicking material, and
//! refined into three named timing archetypes - see `AttackDefinition::impact_delay_ticks`): a
//! protection prayer is only ever checked at the exact tick an attack's accuracy/damage roll
//! happens, never averaged or sampled across the telegraph (or travel phase). A player can
//! stand there with the WRONG prayer for the entire telegraph and still take zero damage by
//! switching to the correct one exactly on the resolve tick ("flicking") - and, just as
//! destructively, can hold the correct prayer for the whole telegraph and still eat the full
//! hit by dropping it one tick early. These tests exercise the real production path
//! (`NpcCombatState::advance` + `get_damage_multiplier` together), not the resolver in
//! isolation, since the thing actually being guaranteed is that NOTHING reads
//! `PlayerReactionState` before the resolve tick, for ANY of the three archetypes:
//! "Jad-style" (multi-tick telegraph, no travel), "Traditional" (no telegraph, no travel) and
//! "Doom-style" (little/no telegraph, multi-tick travel).

use std::rc::Rc;

use super::test_support::FakeCombatTarget;
use super::*;
use crate::actors::EntityId;
use crate::rng::{DotNetRandom, ScriptedRng};
use crate::tiles::TileCoord;

fn tile() -> TileCoord {
    TileCoord::default()
}

fn make_melee_attack(telegraph_ticks: i32, impact_delay_ticks: i32) -> Rc<AttackDefinition> {
    AttackDefinition::new("Test Melee", AttackRangeKind::Melee, 0, telegraph_ticks, AttackStyle::Melee)
        .with_prayer_penetration(0.0)
        .with_reaction(ReactionRequirement::prayer_match(ProtectionPrayer::Melee))
        .with_impact_delay_ticks(impact_delay_ticks)
        .shared()
}

fn attacker_stats() -> CombatStats {
    CombatStats::new(50, 50, 50, 50, 50).unwrap()
}

fn prayer(active_prayer: ProtectionPrayer) -> PlayerReactionState {
    PlayerReactionState { active_prayer, ..Default::default() }
}

fn state_for(attack: Rc<AttackDefinition>) -> NpcCombatState {
    NpcCombatState::new(Rc::new(FixedRotationPattern::new(vec![attack])))
}

fn advance(state: &mut NpcCombatState) -> bool {
    state.advance(true, 1.0, 0, tile(), tile(), AttackStyle::Melee, &mut DotNetRandom::new(1))
}

fn current_multiplier(state: &NpcCombatState, reaction: &PlayerReactionState) -> f32 {
    get_damage_multiplier(state.current_attack().unwrap(), Some(reaction), tile(), tile())
}

fn alive(_: EntityId) -> bool {
    false
}

#[test]
fn switching_to_the_correct_prayer_only_on_the_resolve_tick_still_fully_blocks_the_hit() {
    let mut state = state_for(make_melee_attack(3, 0));
    let mut reaction = prayer(ProtectionPrayer::None); // wrong throughout the telegraph

    assert!(!advance(&mut state)); // tick 1: starts telegraphing
    assert!(!advance(&mut state)); // tick 2: still telegraphing - wrong prayer, irrelevant
    assert!(!advance(&mut state)); // tick 3: still telegraphing - wrong prayer, irrelevant
    reaction.active_prayer = ProtectionPrayer::Melee; // flicked on just before the resolve tick
    assert!(advance(&mut state)); // tick 4: resolves

    assert_eq!(current_multiplier(&state, &reaction), 0.0, "Only the resolve-tick prayer state should matter.");
}

#[test]
fn dropping_the_correct_prayer_one_tick_early_still_takes_full_damage() {
    let mut state = state_for(make_melee_attack(3, 0));
    let mut reaction = prayer(ProtectionPrayer::Melee); // correct throughout the telegraph

    assert!(!advance(&mut state)); // tick 1
    assert!(!advance(&mut state)); // tick 2
    assert!(!advance(&mut state)); // tick 3
    reaction.active_prayer = ProtectionPrayer::None; // dropped one tick too early
    assert!(advance(&mut state)); // tick 4: resolves

    assert_eq!(
        current_multiplier(&state, &reaction),
        1.0,
        "Holding the correct prayer for the whole telegraph counts for nothing if it isn't active on the exact resolve tick."
    );
}

#[test]
fn traditional_attack_no_telegraph_no_travel_still_only_checks_prayer_on_the_resolve_tick() {
    // "Traditional": telegraph_ticks 0, impact_delay_ticks 0 - the vast majority of OSRS
    // monsters. The attack is chosen and resolved on the same tick, so there is no window to
    // flick through at all: the prayer on that one tick is all that matters.
    let mut state = state_for(make_melee_attack(0, 0));
    let mut reaction = prayer(ProtectionPrayer::Melee);
    reaction.active_prayer = ProtectionPrayer::None; // wrong on the tick it swings

    assert!(advance(&mut state)); // chosen and resolved this same tick

    assert_eq!(current_multiplier(&state, &reaction), 1.0);
}

#[test]
fn doom_style_attack_late_switch_during_the_travel_phase_still_fully_blocks_the_hit() {
    // "Doom-style": little/no telegraph (1, as small as a telegraph can be while still being
    // distinct from Traditional's 0), but a multi-tick travel/impact delay during which the roll
    // hasn't happened yet - a FULL late reaction is possible right up through the final travel
    // tick, same as Jad-style's telegraph. The delay itself lives on the DEFENDER's
    // PendingAttackQueue, so this test drives both pieces together, same as the real call sites.
    let mut state = state_for(make_melee_attack(1, 2));
    let mut queue = PendingAttackQueue::new();
    let mut target = FakeCombatTarget::new(100);
    let mut reaction = prayer(ProtectionPrayer::None);

    assert!(!advance(&mut state)); // starts the (brief) telegraph
    assert!(advance(&mut state)); // telegraph ends - resolves immediately; honoring the delay is the caller's job
    let attack = Rc::clone(state.current_attack().unwrap());
    queue.enqueue(Rc::clone(&attack), tile(), Some(EntityId(7)), attacker_stats(), attack.impact_delay_ticks);

    let run = |queue: &mut PendingAttackQueue, reaction: &PlayerReactionState, target: &mut FakeCombatTarget| {
        queue.advance(&target.view, &mut target.damage_buffer, Some(reaction), &mut DotNetRandom::new(1), &alive);
    };

    run(&mut queue, &reaction, &mut target); // travelling - wrong prayer, irrelevant
    assert_eq!(target.health.current(), 100);
    run(&mut queue, &reaction, &mut target); // still travelling - still irrelevant
    assert_eq!(target.health.current(), 100);
    reaction.active_prayer = ProtectionPrayer::Melee; // switched correct on the very last travel tick

    // Correct prayer + 0 penetration forces the multiplier to exactly 0, zeroing the result
    // regardless of hit/miss.
    run(&mut queue, &reaction, &mut target); // impact - rolls now, reads the LIVE state
    target.damage_buffer.advance(&mut target.health, None);

    assert_eq!(
        target.health.current(),
        100,
        "A Doom-style attack's long travel phase must give a full late-reaction window, not just the telegraph."
    );
}

#[test]
fn doom_style_attack_correct_prayer_dropped_before_impact_still_takes_full_damage() {
    let mut state = state_for(make_melee_attack(1, 2));
    let mut queue = PendingAttackQueue::new();
    let mut target = FakeCombatTarget::new(100);
    let mut reaction = prayer(ProtectionPrayer::Melee);

    assert!(!advance(&mut state)); // starts telegraphing
    assert!(advance(&mut state)); // telegraph ends - resolves immediately
    let attack = Rc::clone(state.current_attack().unwrap());
    queue.enqueue(Rc::clone(&attack), tile(), Some(EntityId(7)), attacker_stats(), attack.impact_delay_ticks);

    queue.advance(&target.view, &mut target.damage_buffer, Some(&reaction), &mut DotNetRandom::new(1), &alive); // travelling
    queue.advance(&target.view, &mut target.damage_buffer, Some(&reaction), &mut DotNetRandom::new(1), &alive); // still travelling
    reaction.active_prayer = ProtectionPrayer::None; // dropped one tick before impact
    let mut guaranteed_hit = ScriptedRng::new(vec![0.0], vec![10]);
    queue.advance(&target.view, &mut target.damage_buffer, Some(&reaction), &mut guaranteed_hit, &alive); // impact
    target.damage_buffer.advance(&mut target.health, None);

    assert_eq!(
        target.health.current(),
        90,
        "Holding the correct prayer for the whole travel phase counts for nothing if it isn't active on the exact impact tick."
    );
}
