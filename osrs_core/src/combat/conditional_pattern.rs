use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_pattern::AttackPattern;
use super::attack_selection_context::AttackSelectionContext;
use crate::rng::Rng;

/// One `condition -> attack` rule of a `ConditionalPattern`.
pub struct Branch {
    pub condition: Box<dyn Fn(&AttackSelectionContext) -> bool>,
    pub attack: Rc<AttackDefinition>,
}

impl Branch {
    pub fn new(condition: impl Fn(&AttackSelectionContext) -> bool + 'static, attack: Rc<AttackDefinition>) -> Self {
        Self { condition: Box::new(condition), attack }
    }
}

/// Branches on elapsed time or the per-NPC counter rather than choosing randomly (Zulrah's
/// scripted phase cycle). Evaluated in order; the first satisfied branch wins. `rng` is
/// accepted only to satisfy `AttackPattern`'s shape - selection here is fully deterministic.
pub struct ConditionalPattern {
    branches: Vec<Branch>,
    fallback: Rc<AttackDefinition>,
}

impl ConditionalPattern {
    /// `fallback` is used if no branch's condition is satisfied - keep data-authoring
    /// mistakes (gaps in the condition coverage) from crashing a live fight.
    pub fn new(fallback: Rc<AttackDefinition>, branches: Vec<Branch>) -> Self {
        Self { branches, fallback }
    }
}

impl AttackPattern for ConditionalPattern {
    fn select_next(&self, context: &AttackSelectionContext, _rng: &mut dyn Rng) -> Rc<AttackDefinition> {
        for branch in &self.branches {
            if (branch.condition)(context) {
                return Rc::clone(&branch.attack);
            }
        }
        Rc::clone(&self.fallback)
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
    fn select_next_picks_the_first_satisfied_branch_zulrah_style_schedule() {
        let (phase1, phase2, phase3) = (melee_attack("Phase1", 1), melee_attack("Phase2", 1), melee_attack("Phase3", 1));
        let pattern = ConditionalPattern::new(
            phase3.clone(),
            vec![Branch::new(|ctx| ctx.counter < 2, phase1.clone()), Branch::new(|ctx| ctx.counter < 4, phase2.clone())],
        );
        let mut rng = DotNetRandom::new(1);

        assert!(Rc::ptr_eq(&phase1, &pattern.select_next(&context_with_counter(0), &mut rng)));
        assert!(Rc::ptr_eq(&phase1, &pattern.select_next(&context_with_counter(1), &mut rng)));
        assert!(Rc::ptr_eq(&phase2, &pattern.select_next(&context_with_counter(2), &mut rng)));
        assert!(Rc::ptr_eq(&phase2, &pattern.select_next(&context_with_counter(3), &mut rng)));
        assert!(Rc::ptr_eq(&phase3, &pattern.select_next(&context_with_counter(4), &mut rng))); // falls back
    }

    #[test]
    fn select_next_is_deterministic_no_randomness_involved() {
        let attack = melee_attack("Only", 1);
        let pattern = ConditionalPattern::new(attack, Vec::new());

        let picked1 = pattern.select_next(&context_with_counter(0), &mut DotNetRandom::new(1));
        let picked2 = pattern.select_next(&context_with_counter(0), &mut DotNetRandom::new(999));

        assert!(Rc::ptr_eq(&picked1, &picked2));
    }
}
