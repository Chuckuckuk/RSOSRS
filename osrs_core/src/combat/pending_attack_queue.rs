use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::combat_stats::CombatStats;
use super::damage_buffer::DamageBuffer;
use super::damage_resolver;
use super::player_reaction_state::PlayerReactionState;
use super::reaction_resolver;
use crate::actors::{ActorKind, CombatTargetView, EntityId};
use crate::rng::Rng;
use crate::tiles::TileCoord;

struct PendingRoll {
    attack: Rc<AttackDefinition>,
    hazard_origin_tile: TileCoord,
    attacker: Option<EntityId>,
    attacker_stats: CombatStats,
    ticks_remaining: i32,
    /// The least a hit that gets past the accuracy roll deals (see `damage_resolver::roll_with_floor`).
    successful_floor: i32,
}

/// Attacks that have launched but not yet rolled ("Doom-style" impact delay), owned by the
/// DEFENDER rather than the attacker: once an attack launches, its in-flight state belongs to
/// whatever it is flying at, so it keeps counting down (and can still be dropped because its
/// caster died) regardless of what the attacker's own component does afterwards.
///
/// The C# entry holds the attacker's `Health` object to ask "is the caster dead?". Rust stores
/// the attacker's `EntityId` instead, and `advance` is told how to look up deadness.
#[derive(Default)]
pub struct PendingAttackQueue {
    pending: Vec<PendingRoll>,
}

impl PendingAttackQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// `attacker` is `None` for attacks with no caster that can die (the C# null `Health`).
    pub fn enqueue(
        &mut self,
        attack: Rc<AttackDefinition>,
        hazard_origin_tile: TileCoord,
        attacker: Option<EntityId>,
        attacker_stats: CombatStats,
        impact_delay_ticks: i32,
    ) {
        self.enqueue_with_floor(attack, hazard_origin_tile, attacker, attacker_stats, impact_delay_ticks, 0);
    }

    /// `enqueue`, for an attacker whose successful hits deal at least `successful_floor`.
    pub fn enqueue_with_floor(
        &mut self,
        attack: Rc<AttackDefinition>,
        hazard_origin_tile: TileCoord,
        attacker: Option<EntityId>,
        attacker_stats: CombatStats,
        impact_delay_ticks: i32,
        successful_floor: i32,
    ) {
        self.pending.push(PendingRoll {
            attack,
            hazard_origin_tile,
            attacker,
            attacker_stats,
            ticks_remaining: impact_delay_ticks,
            successful_floor,
        });
    }

    /// Counts every pending attack down one tick; any whose delay is up rolls NOW (reading the
    /// player's reaction state live) and hands its damage to `damage_buffer`. An attack whose
    /// caster died before it landed is consumed without rolling at all.
    pub fn advance(
        &mut self,
        defender: &CombatTargetView,
        damage_buffer: &mut DamageBuffer,
        reaction_state: Option<&PlayerReactionState>,
        rng: &mut dyn Rng,
        attacker_is_dead: &dyn Fn(EntityId) -> bool,
    ) {
        let mut i = 0;
        while i < self.pending.len() {
            if self.pending[i].ticks_remaining <= 0 {
                let roll = self.pending.remove(i);

                // The attack's caster died before this landed - the shot dies with it,
                // deliberately, rather than hitting from beyond the grave.
                if roll.attacker.is_some_and(attacker_is_dead) {
                    continue;
                }

                let multiplier = match reaction_state {
                    Some(_) => {
                        reaction_resolver::get_damage_multiplier(&roll.attack, reaction_state, roll.hazard_origin_tile, defender.tile)
                    }
                    None => 1.0,
                };
                let damage = damage_resolver::roll_with_floor(
                    &roll.attack,
                    &roll.attacker_stats,
                    &defender.stats,
                    defender.kind == ActorKind::Npc,
                    rng,
                    multiplier,
                    roll.successful_floor,
                );
                damage_buffer.enqueue(damage, roll.attack.projectile_travel_ticks, roll.attack.on_hit_status_effect);
            } else {
                self.pending[i].ticks_remaining -= 1;
                i += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;
    use crate::actors::Health;
    use crate::combat::test_support::FakeCombatTarget;
    use crate::combat::{AttackRangeKind, AttackStyle, ProtectionPrayer, ReactionRequirement};
    use crate::rng::{DotNetRandom, ScriptedRng};

    const ATTACKER: EntityId = EntityId(1);
    const ATTACKER_TILE: TileCoord = TileCoord { x: 5, z: 5, plane: 0 };

    fn make_attack(projectile_travel_ticks: i32, reaction: ReactionRequirement) -> Rc<AttackDefinition> {
        AttackDefinition::new("Test Doom-style", AttackRangeKind::Ranged, 10, 0, AttackStyle::Magic)
            .with_reaction(reaction)
            .with_projectile_travel_ticks(projectile_travel_ticks)
            .shared()
    }

    fn attacker_stats() -> CombatStats {
        CombatStats::new(50, 50, 50, 50, 50).unwrap()
    }

    /// Guarantees a hit (0.0 is less than any positive hit chance) and a scripted damage amount.
    fn guaranteed_hit_of(amount: i32) -> ScriptedRng {
        ScriptedRng::new(vec![0.0], vec![amount])
    }

    fn alive(_: EntityId) -> bool {
        false
    }

    fn advance(queue: &mut PendingAttackQueue, target: &mut FakeCombatTarget, reaction: Option<&PlayerReactionState>, rng: &mut dyn Rng) {
        queue.advance(&target.view, &mut target.damage_buffer, reaction, rng, &alive);
    }

    #[test]
    fn enqueue_then_advance_does_not_touch_health_until_the_delay_elapses() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        queue.enqueue(make_attack(0, ReactionRequirement::none()), ATTACKER_TILE, Some(ATTACKER), attacker_stats(), 2);

        advance(&mut queue, &mut target, None, &mut DotNetRandom::new(1)); // still travelling
        assert_eq!(target.health.current(), 100);
        advance(&mut queue, &mut target, None, &mut DotNetRandom::new(1)); // still travelling
        assert_eq!(target.health.current(), 100);
        advance(&mut queue, &mut target, None, &mut guaranteed_hit_of(10)); // delay elapsed - rolls and forwards into DamageBuffer

        assert_eq!(target.health.current(), 100, "The roll lands in DamageBuffer first, not Health directly.");
        assert_eq!(target.damage_buffer.pending_count(), 1);
        target.damage_buffer.advance(&mut target.health, None);
        assert_eq!(target.health.current(), 90);
    }

    #[test]
    fn advance_reads_reaction_state_live_at_the_tick_it_rolls_not_when_enqueued() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        let mut reaction = PlayerReactionState { active_prayer: ProtectionPrayer::None, ..Default::default() };
        queue.enqueue(
            make_attack(0, ReactionRequirement::prayer_match(ProtectionPrayer::Magic)),
            ATTACKER_TILE,
            Some(ATTACKER),
            attacker_stats(),
            1,
        );

        advance(&mut queue, &mut target, Some(&reaction), &mut DotNetRandom::new(1)); // still travelling - wrong prayer is irrelevant
        reaction.active_prayer = ProtectionPrayer::Magic; // switched correct right before it rolls

        // A correct PrayerMatch with 0 penetration forces the multiplier to exactly 0, which
        // zeroes the result whether the accuracy roll hit or missed.
        advance(&mut queue, &mut target, Some(&reaction), &mut DotNetRandom::new(1));
        target.damage_buffer.advance(&mut target.health, None);

        assert_eq!(target.health.current(), 100, "A prayer switched correct before the roll tick must fully block it.");
    }

    #[test]
    fn advance_with_no_reaction_state_always_rolls_full_damage() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        queue.enqueue(
            make_attack(0, ReactionRequirement::prayer_match(ProtectionPrayer::Magic)),
            ATTACKER_TILE,
            Some(ATTACKER),
            attacker_stats(),
            0,
        );

        advance(&mut queue, &mut target, None, &mut guaranteed_hit_of(10)); // no prayer book to check - e.g. an NPC defender
        target.damage_buffer.advance(&mut target.health, None);

        assert_eq!(target.health.current(), 90);
    }

    #[test]
    fn advance_forwards_into_damage_buffer_with_the_attacks_own_projectile_travel_ticks() {
        // Proves composition: a Doom-style roll-delay followed by its own post-roll travel
        // delay still ends up resolved by DamageBuffer, unaffected by PendingAttackQueue.
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        queue.enqueue(make_attack(2, ReactionRequirement::none()), ATTACKER_TILE, Some(ATTACKER), attacker_stats(), 1);

        advance(&mut queue, &mut target, None, &mut DotNetRandom::new(1)); // still travelling
        advance(&mut queue, &mut target, None, &mut guaranteed_hit_of(10)); // rolls, hands off to DamageBuffer with 2 travel ticks

        assert_eq!(target.damage_buffer.pending_count(), 1);
        target.damage_buffer.advance(&mut target.health, None);
        assert_eq!(target.health.current(), 100);
        target.damage_buffer.advance(&mut target.health, None);
        assert_eq!(target.health.current(), 100);
        target.damage_buffer.advance(&mut target.health, None);
        assert_eq!(target.health.current(), 90);
    }

    #[test]
    fn pending_count_reflects_only_unrolled_attacks() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        queue.enqueue(make_attack(0, ReactionRequirement::none()), ATTACKER_TILE, Some(ATTACKER), attacker_stats(), 1);
        assert_eq!(queue.pending_count(), 1);

        advance(&mut queue, &mut target, None, &mut DotNetRandom::new(1)); // still travelling
        assert_eq!(queue.pending_count(), 1);
        advance(&mut queue, &mut target, None, &mut DotNetRandom::new(1)); // rolls (hit or miss) and leaves this queue entirely
        assert_eq!(queue.pending_count(), 0);
    }

    #[test]
    fn advance_when_the_attacker_died_before_the_delay_elapsed_never_rolls_shot_dies_with_it() {
        // The deliberately-preserved behaviour: an attacker killed between launching a
        // Doom-style attack and that attack's impact takes its unresolved shot down with it -
        // the projectile never lands, rather than hitting from beyond the grave.
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        let attacker_dead = Cell::new(false);
        let is_dead = |_: EntityId| attacker_dead.get();
        queue.enqueue(make_attack(0, ReactionRequirement::none()), ATTACKER_TILE, Some(ATTACKER), attacker_stats(), 2);

        queue.advance(&target.view, &mut target.damage_buffer, None, &mut DotNetRandom::new(1), &is_dead); // attacker alive
        assert_eq!(queue.pending_count(), 1);
        attacker_dead.set(true); // attacker dies mid-flight
        queue.advance(&target.view, &mut target.damage_buffer, None, &mut DotNetRandom::new(1), &is_dead); // still counting down
        assert_eq!(queue.pending_count(), 1);
        queue.advance(&target.view, &mut target.damage_buffer, None, &mut DotNetRandom::new(1), &is_dead); // would resolve, but attacker is dead

        assert_eq!(queue.pending_count(), 0, "The entry is still consumed/removed even when dropped.");
        assert_eq!(target.damage_buffer.pending_count(), 0, "A dead attacker's shot must never reach DamageBuffer at all.");
        target.damage_buffer.advance(&mut target.health, None);
        assert_eq!(target.health.current(), 100);
    }

    #[test]
    fn advance_when_the_attacker_was_already_dead_at_enqueue_time_never_rolls() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        let mut attacker_health = Health::new(10);
        attacker_health.damage(attacker_health.max());
        let is_dead = |_: EntityId| attacker_health.is_dead();
        queue.enqueue(make_attack(0, ReactionRequirement::none()), ATTACKER_TILE, Some(ATTACKER), attacker_stats(), 0);

        queue.advance(&target.view, &mut target.damage_buffer, None, &mut DotNetRandom::new(1), &is_dead);

        assert_eq!(target.damage_buffer.pending_count(), 0);
    }

    #[test]
    fn an_attack_with_no_attacker_handle_always_rolls() {
        let mut target = FakeCombatTarget::new(100);
        let mut queue = PendingAttackQueue::new();
        queue.enqueue(make_attack(0, ReactionRequirement::none()), ATTACKER_TILE, None, attacker_stats(), 0);

        queue.advance(&target.view, &mut target.damage_buffer, None, &mut guaranteed_hit_of(4), &|_| true);

        assert_eq!(target.damage_buffer.pending_count(), 1);
    }
}
