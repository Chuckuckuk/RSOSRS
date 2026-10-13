use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_pattern::AttackPattern;
use super::attack_selection_context::AttackSelectionContext;
use crate::rng::Rng;

/// Hard-coded cycle through a fixed list of attacks, in order, wrapping forever (Vorkath's
/// deterministic rotation). Draws no randomness at all - `rng` is accepted only to satisfy
/// `AttackPattern`'s shape.
pub struct FixedRotationPattern {
    rotation: Vec<Rc<AttackDefinition>>,
}

impl FixedRotationPattern {
    /// Panics if `rotation` is empty (mirrors the C# `ArgumentException`).
    pub fn new(rotation: Vec<Rc<AttackDefinition>>) -> Self {
        assert!(!rotation.is_empty(), "FixedRotationPattern needs at least one attack.");
        Self { rotation }
    }
}

impl AttackPattern for FixedRotationPattern {
    fn select_next(&self, context: &AttackSelectionContext, _rng: &mut dyn Rng) -> Rc<AttackDefinition> {
        let index = context.counter.rem_euclid(self.rotation.len() as i32) as usize;
        Rc::clone(&self.rotation[index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_support::melee_attack;
    use crate::combat::AttackStyle;
    use crate::rng::DotNetRandom;
    use crate::tiles::TileCoord;

    fn context_with_counter(counter: i32) -> AttackSelectionContext {
        AttackSelectionContext::new(1.0, 0, counter, TileCoord::default(), TileCoord::new(0, 0), AttackStyle::Melee)
    }

    #[test]
    fn select_next_cycles_through_in_order_then_wraps() {
        let (a, b, c) = (melee_attack("A", 1), melee_attack("B", 1), melee_attack("C", 1));
        let pattern = FixedRotationPattern::new(vec![a.clone(), b.clone(), c.clone()]);
        let mut rng = DotNetRandom::new(1);

        assert!(Rc::ptr_eq(&a, &pattern.select_next(&context_with_counter(0), &mut rng)));
        assert!(Rc::ptr_eq(&b, &pattern.select_next(&context_with_counter(1), &mut rng)));
        assert!(Rc::ptr_eq(&c, &pattern.select_next(&context_with_counter(2), &mut rng)));
        assert!(Rc::ptr_eq(&a, &pattern.select_next(&context_with_counter(3), &mut rng))); // wraps
    }

    #[test]
    #[should_panic]
    fn constructor_with_no_attacks_panics() {
        FixedRotationPattern::new(Vec::new());
    }
}
