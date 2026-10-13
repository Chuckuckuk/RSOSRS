use super::status_effect::StatusEffectDefinition;
use super::status_effect_tracker::StatusEffectTracker;
use crate::actors::Health;

#[derive(Debug, Clone, Copy)]
struct PendingHit {
    amount: i32,
    ticks_remaining: i32,
    on_land_effect: Option<StatusEffectDefinition>,
}

/// Rolled hits waiting out their projectile travel time before they touch `Health`. Mirrors
/// OSRS's real damage queue: `clear` wipes EVERYTHING pending (the Phoenix necklace mechanic),
/// not just the hit that triggered it.
#[derive(Debug, Clone, Default)]
pub struct DamageBuffer {
    pending: Vec<PendingHit>,
}

impl DamageBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Queues a hit that lands after `travel_ticks` further `advance` calls. Negative amounts are
    /// ignored; a hit of 0 (a block) is kept so it lands, and shows, like any other.
    pub fn enqueue(&mut self, amount: i32, travel_ticks: i32, on_land_effect: Option<StatusEffectDefinition>) {
        if amount < 0 {
            return;
        }
        self.pending.push(PendingHit { amount, ticks_remaining: travel_ticks, on_land_effect });
    }

    /// Lands every hit whose travel time is up, counts the rest down by one tick. A landing
    /// hit's on-land effect is applied to `status_effects`, or dropped if there is no tracker.
    /// Returns the amounts that landed, in order (0 for a block).
    pub fn advance(&mut self, target: &mut Health, mut status_effects: Option<&mut StatusEffectTracker>) -> Vec<i32> {
        let mut landed = Vec::new();
        self.pending.retain_mut(|hit| {
            if hit.ticks_remaining <= 0 {
                landed.push(hit.amount);
                target.damage(hit.amount);
                if let (Some(effect), Some(tracker)) = (hit.on_land_effect, status_effects.as_mut()) {
                    tracker.apply(effect);
                }
                false
            } else {
                hit.ticks_remaining -= 1;
                true
            }
        });
        landed
    }

    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enqueue_with_zero_travel_ticks_applies_on_the_very_next_advance_call() {
        let mut target = Health::new(20);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(5, 0, None);
        assert_eq!(target.current(), 20, "Enqueue alone must not touch Health - only advance does.");

        buffer.advance(&mut target, None);

        assert_eq!(target.current(), 15);
    }

    #[test]
    fn enqueue_with_travel_ticks_only_applies_after_that_many_advance_calls() {
        let mut target = Health::new(20);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(5, 2, None);

        buffer.advance(&mut target, None); // tick 1: still travelling
        assert_eq!(target.current(), 20);
        buffer.advance(&mut target, None); // tick 2: still travelling
        assert_eq!(target.current(), 20);
        buffer.advance(&mut target, None); // tick 3: lands
        assert_eq!(target.current(), 15);
    }

    #[test]
    fn multiple_hits_with_different_travel_times_land_on_different_ticks_independently() {
        let mut target = Health::new(100);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(10, 0, None); // lands immediately
        buffer.enqueue(20, 2, None); // lands on the 3rd advance

        buffer.advance(&mut target, None); // tick 1: the 0-delay hit lands
        assert_eq!(target.current(), 90);
        buffer.advance(&mut target, None); // tick 2: still nothing else due
        assert_eq!(target.current(), 90);
        buffer.advance(&mut target, None); // tick 3: the delayed hit lands
        assert_eq!(target.current(), 70);
    }

    #[test]
    fn clear_removes_every_pending_hit_even_already_rolled_ones_without_applying_them() {
        // The real Phoenix necklace mechanic: healing wipes the WHOLE queue, not just caps
        // the hit that triggered it - this is why an early sub-hit of a multi-hit special
        // (dragon claws, dark bow) cancels its own later sub-hits once it procs the necklace.
        let mut target = Health::new(100);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(30, 1, None);
        buffer.enqueue(40, 1, None);

        buffer.clear();
        buffer.advance(&mut target, None);
        buffer.advance(&mut target, None);

        assert_eq!(target.current(), 100, "Cleared hits must never apply, no matter how many advance calls follow.");
    }

    #[test]
    fn a_negative_amount_is_ignored_and_a_zero_lands_as_a_block_without_hurting() {
        let mut target = Health::new(20);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(-3, 0, None);
        assert_eq!(buffer.pending_count(), 0);
        buffer.enqueue(0, 0, None);
        assert_eq!(buffer.pending_count(), 1);

        let landed = buffer.advance(&mut target, None);

        assert_eq!(landed, vec![0]);
        assert_eq!(buffer.pending_count(), 0);
        assert_eq!(target.current(), 20);
    }

    #[test]
    fn advance_reports_what_landed_in_order() {
        let mut target = Health::new(50);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(4, 0, None);
        buffer.enqueue(9, 1, None);
        buffer.enqueue(2, 0, None);

        assert_eq!(buffer.advance(&mut target, None), vec![4, 2]);
        assert_eq!(buffer.advance(&mut target, None), vec![9]);
        assert!(buffer.advance(&mut target, None).is_empty());
    }

    #[test]
    fn pending_count_reflects_only_hits_not_yet_applied() {
        let mut target = Health::new(20);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(5, 1, None);
        assert_eq!(buffer.pending_count(), 1);

        buffer.advance(&mut target, None); // still travelling
        assert_eq!(buffer.pending_count(), 1);
        buffer.advance(&mut target, None); // lands
        assert_eq!(buffer.pending_count(), 0);
    }

    #[test]
    fn on_land_effect_is_applied_only_at_the_tick_the_hit_actually_lands_not_when_enqueued() {
        let mut target = Health::new(100);
        let mut buffer = DamageBuffer::new();
        let mut status_effects = StatusEffectTracker::new();
        buffer.enqueue(10, 2, Some(StatusEffectDefinition::burn(1, 5)));

        buffer.advance(&mut target, Some(&mut status_effects)); // still travelling
        assert!(!status_effects.is_stunned()); // sanity: tracker exists and is otherwise empty so far
        buffer.advance(&mut target, Some(&mut status_effects)); // still travelling
        buffer.advance(&mut target, Some(&mut status_effects)); // lands - Burn applies HERE
        status_effects.advance(&mut target, None); // Burn's own first pulse

        assert_eq!(target.current(), 89, "90 after the 10 damage landed, minus 1 more from Burn's pulse.");
    }

    #[test]
    fn on_land_effect_is_dropped_without_panicking_when_no_status_effect_tracker_is_given() {
        let mut target = Health::new(100);
        let mut buffer = DamageBuffer::new();
        buffer.enqueue(10, 0, Some(StatusEffectDefinition::stun(3)));

        buffer.advance(&mut target, None);

        assert_eq!(target.current(), 90);
    }
}
