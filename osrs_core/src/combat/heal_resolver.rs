use crate::actors::Health;

/// Heals `target` and returns the amount actually applied (clamped at max health).
pub fn resolve(heal_amount: i32, target: &mut Health) -> i32 {
    let before = target.current();
    target.heal(heal_amount);
    target.current() - before
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_heals_by_the_given_amount_clamped_to_max() {
        let mut target = Health::new(20);
        target.damage(15); // 5/20

        let healed = resolve(8, &mut target);

        assert_eq!(healed, 8);
        assert_eq!(target.current(), 13);
    }

    #[test]
    fn resolve_clamps_at_max_health_and_returns_the_actual_amount_applied() {
        let mut target = Health::new(20);
        target.damage(2); // 18/20

        let healed = resolve(10, &mut target);

        assert_eq!(healed, 2); // only 2 actually got applied before hitting the cap
        assert_eq!(target.current(), 20);
    }
}
