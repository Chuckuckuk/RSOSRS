use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_pattern::AttackPattern;
use super::attack_selection_context::AttackSelectionContext;
use crate::rng::Rng;

/// One candidate attack and the function that weights it from LIVE state.
pub struct Entry {
    pub attack: Rc<AttackDefinition>,
    pub weight: Box<dyn Fn(&AttackSelectionContext) -> f32>,
}

impl Entry {
    pub fn new(attack: Rc<AttackDefinition>, weight: impl Fn(&AttackSelectionContext) -> f32 + 'static) -> Self {
        Self { attack, weight: Box::new(weight) }
    }
}

/// Weighted-random attack choice where each entry's weight is a function of LIVE state, not
/// a fixed number - so an HP threshold can shift which attack is likely without swapping the
/// moveset itself (Vardorvis's HP-reactive special frequency). A weight function returning 0
/// effectively disables that entry for the current selection; all entries returning 0 falls
/// back to the first entry rather than panicking, since that is a data-authoring mistake, not
/// something that should crash a live fight.
pub struct WeightedRandomPattern {
    entries: Vec<Entry>,
}

impl WeightedRandomPattern {
    /// Panics if `entries` is empty (mirrors the C# `ArgumentException`).
    pub fn new(entries: Vec<Entry>) -> Self {
        assert!(!entries.is_empty(), "WeightedRandomPattern needs at least one entry.");
        Self { entries }
    }
}

impl AttackPattern for WeightedRandomPattern {
    fn select_next(&self, context: &AttackSelectionContext, rng: &mut dyn Rng) -> Rc<AttackDefinition> {
        // Weights are f32 and summed in f32, exactly as the C# does; only the roll is f64.
        let mut total = 0.0f32;
        for entry in &self.entries {
            total += (entry.weight)(context).max(0.0);
        }

        if total <= 0.0 {
            return Rc::clone(&self.entries[0].attack);
        }

        let roll = rng.next_double() * total as f64;
        let mut cumulative = 0.0f32;
        for entry in &self.entries {
            cumulative += (entry.weight)(context).max(0.0);
            if roll < cumulative as f64 {
                return Rc::clone(&entry.attack);
            }
        }

        Rc::clone(&self.entries[self.entries.len() - 1].attack) // float-rounding safety net
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_support::melee_attack;
    use crate::combat::AttackStyle;
    use crate::rng::DotNetRandom;
    use crate::tiles::TileCoord;

    fn context_with_health_percent(health_percent: f32) -> AttackSelectionContext {
        AttackSelectionContext::new(health_percent, 0, 0, TileCoord::default(), TileCoord::new(0, 0), AttackStyle::Melee)
    }

    #[test]
    fn select_next_with_one_zero_weight_entry_never_selects_it() {
        let (always, never) = (melee_attack("Always", 1), melee_attack("Never", 1));
        let pattern = WeightedRandomPattern::new(vec![Entry::new(always.clone(), |_| 1.0), Entry::new(never, |_| 0.0)]);
        let mut rng = DotNetRandom::new(7);

        for _ in 0..50 {
            assert!(Rc::ptr_eq(&always, &pattern.select_next(&context_with_health_percent(1.0), &mut rng)));
        }
    }

    #[test]
    fn select_next_weights_shift_with_live_state_not_just_a_fixed_roll() {
        // Vardorvis-style: the SAME entry's weight depends on the context passed in, not a
        // number baked in at construction time.
        let (high_hp, low_hp) = (melee_attack("HighHpOnly", 1), melee_attack("LowHpOnly", 1));
        let pattern = WeightedRandomPattern::new(vec![
            Entry::new(high_hp.clone(), |ctx| if ctx.health_percent > 0.5 { 1.0 } else { 0.0 }),
            Entry::new(low_hp.clone(), |ctx| if ctx.health_percent <= 0.5 { 1.0 } else { 0.0 }),
        ]);
        let mut rng = DotNetRandom::new(3);

        assert!(Rc::ptr_eq(&high_hp, &pattern.select_next(&context_with_health_percent(1.0), &mut rng)));
        assert!(Rc::ptr_eq(&low_hp, &pattern.select_next(&context_with_health_percent(0.2), &mut rng)));
    }

    #[test]
    fn select_next_is_deterministic_for_the_same_seed_and_sequence() {
        let (a, b) = (melee_attack("A", 1), melee_attack("B", 1));
        let make_pattern = || {
            WeightedRandomPattern::new(vec![Entry::new(a.clone(), |_| 1.0), Entry::new(b.clone(), |_| 1.0)])
        };
        let (pattern_a, pattern_b) = (make_pattern(), make_pattern());
        let (mut rng_a, mut rng_b) = (DotNetRandom::new(99), DotNetRandom::new(99));

        for _ in 0..20 {
            let context = context_with_health_percent(1.0);
            let picked_a = pattern_a.select_next(&context, &mut rng_a);
            let picked_b = pattern_b.select_next(&context, &mut rng_b);
            assert!(Rc::ptr_eq(&picked_a, &picked_b));
        }
    }

    #[test]
    fn select_next_all_weights_zero_falls_back_to_first_entry_rather_than_panicking() {
        let (first, second) = (melee_attack("First", 1), melee_attack("Second", 1));
        let pattern = WeightedRandomPattern::new(vec![Entry::new(first.clone(), |_| 0.0), Entry::new(second, |_| 0.0)]);

        let picked = pattern.select_next(&context_with_health_percent(1.0), &mut DotNetRandom::new(5));

        assert!(Rc::ptr_eq(&first, &picked));
    }

    #[test]
    #[should_panic]
    fn constructor_with_no_entries_panics() {
        WeightedRandomPattern::new(Vec::new());
    }
}
