use std::rc::Rc;

use super::attack_definition::AttackDefinition;
use super::attack_pattern::AttackPattern;
use super::attack_selection_context::AttackSelectionContext;
use crate::rng::Rng;
use crate::tiles::TileCoord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationMode {
    /// Cycles through the usable attacks in list order.
    Fixed,
    /// Picks among the usable attacks by weight.
    Weighted,
}

/// How a data-defined monster chooses its next attack: only from the attacks that can currently
/// reach the target (a melee swing needs adjacency, a ranged or magic attack needs range), by
/// either a fixed cycle or weights. A monster with nothing in reach does not start an attack at
/// all - `engages` is how the world asks.
pub struct RangeAwarePattern {
    attacks: Vec<(Rc<AttackDefinition>, f32)>,
    mode: RotationMode,
    footprint: i32,
    /// The attacker's own attack range, which an attack without a range of its own inherits.
    attack_range: i32,
}

impl RangeAwarePattern {
    /// Panics if `attacks` is empty (a monster with no attacks should not have a pattern at all).
    pub fn new(
        attacks: Vec<(Rc<AttackDefinition>, f32)>,
        mode: RotationMode,
        footprint: i32,
        attack_range: i32,
    ) -> Self {
        assert!(!attacks.is_empty(), "RangeAwarePattern needs at least one attack.");
        Self { attacks, mode, footprint, attack_range }
    }

    fn usable(&self, self_tile: TileCoord, target_tile: TileCoord) -> Vec<usize> {
        (0..self.attacks.len())
            .filter(|&i| self.attacks[i].0.is_in_reach(self_tile, self.footprint, self.attack_range, target_tile))
            .collect()
    }
}

impl AttackPattern for RangeAwarePattern {
    fn reaches(&self, self_tile: TileCoord, target_tile: TileCoord) -> Option<bool> {
        Some(!self.usable(self_tile, target_tile).is_empty())
    }

    fn select_next(&self, context: &AttackSelectionContext, rng: &mut dyn Rng) -> Rc<AttackDefinition> {
        let mut candidates = self.usable(context.self_tile, context.target_tile);
        if candidates.is_empty() {
            candidates = (0..self.attacks.len()).collect(); // not reachable via `engages`; never panic mid-fight
        }

        let chosen = match self.mode {
            RotationMode::Fixed => candidates[context.counter.max(0) as usize % candidates.len()],
            RotationMode::Weighted => {
                let total: f32 = candidates.iter().map(|&i| self.attacks[i].1.max(0.0)).sum();
                if total <= 0.0 {
                    candidates[0]
                } else {
                    let roll = rng.next_double() * total as f64;
                    let mut cumulative = 0.0f32;
                    let mut pick = *candidates.last().expect("non-empty");
                    for &i in &candidates {
                        cumulative += self.attacks[i].1.max(0.0);
                        if roll < cumulative as f64 {
                            pick = i;
                            break;
                        }
                    }
                    pick
                }
            }
        };
        Rc::clone(&self.attacks[chosen].0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::AttackStyle;
    use crate::rng::DotNetRandom;

    fn melee() -> Rc<AttackDefinition> {
        AttackDefinition::targeted("Melee", None, 1, AttackStyle::Melee).shared()
    }

    fn magic() -> Rc<AttackDefinition> {
        AttackDefinition::targeted("Magic", Some(10), 1, AttackStyle::Magic).shared()
    }

    fn context(counter: i32, target: TileCoord) -> AttackSelectionContext {
        AttackSelectionContext::new(1.0, 0, counter, TileCoord::new(0, 0), target, AttackStyle::Melee)
    }

    #[test]
    fn melee_is_only_usable_when_adjacent_but_magic_reaches_further() {
        let pattern = RangeAwarePattern::new(vec![(melee(), 1.0), (magic(), 1.0)], RotationMode::Fixed, 1, 1);

        assert_eq!(pattern.usable(TileCoord::new(0, 0), TileCoord::new(1, 0)), vec![0, 1]);
        assert_eq!(pattern.usable(TileCoord::new(0, 0), TileCoord::new(6, 0)), vec![1]);
        assert!(!pattern.engages(TileCoord::new(0, 0), TileCoord::new(30, 0)));
    }

    #[test]
    fn a_min_range_hides_an_attack_up_close() {
        let lob = AttackDefinition::targeted("Lob", Some(10), 1, AttackStyle::Ranged).with_min_range(4).shared();
        let pattern = RangeAwarePattern::new(vec![(melee(), 1.0), (lob, 1.0)], RotationMode::Fixed, 1, 1);

        assert_eq!(pattern.usable(TileCoord::new(0, 0), TileCoord::new(1, 0)), vec![0]);
        assert_eq!(pattern.usable(TileCoord::new(0, 0), TileCoord::new(5, 0)), vec![1]);
    }

    #[test]
    fn fixed_mode_cycles_only_the_usable_attacks() {
        let pattern = RangeAwarePattern::new(vec![(melee(), 1.0), (magic(), 1.0)], RotationMode::Fixed, 1, 1);
        let mut rng = DotNetRandom::new(1);
        let far = TileCoord::new(6, 0);

        for counter in 0..4 {
            assert_eq!(pattern.select_next(&context(counter, far), &mut rng).name, "Magic");
        }

        let near = TileCoord::new(1, 0);
        assert_eq!(pattern.select_next(&context(0, near), &mut rng).name, "Melee");
        assert_eq!(pattern.select_next(&context(1, near), &mut rng).name, "Magic");
    }

    #[test]
    fn weighted_mode_honours_zero_weights() {
        let pattern = RangeAwarePattern::new(vec![(melee(), 0.0), (magic(), 1.0)], RotationMode::Weighted, 1, 1);
        let mut rng = DotNetRandom::new(5);

        for _ in 0..50 {
            assert_eq!(pattern.select_next(&context(0, TileCoord::new(1, 0)), &mut rng).name, "Magic");
        }
    }
}
