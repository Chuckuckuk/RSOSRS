/// Current/max hitpoints with clamped damage/heal. Plain data - no death event, no
/// regen; callers check `is_dead` after a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    max: i32,
    current: i32,
}

impl Health {
    /// Panics if `max <= 0` (mirrors the C# `ArgumentOutOfRangeException` - bad entity data is a
    /// programmer error, not a runtime condition).
    pub fn new(max: i32) -> Self {
        assert!(max > 0, "max health must be positive, was {max}");
        Self { max, current: max }
    }

    pub fn max(&self) -> i32 {
        self.max
    }

    pub fn current(&self) -> i32 {
        self.current
    }

    pub fn is_dead(&self) -> bool {
        self.current <= 0
    }

    pub fn damage(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        self.current = (self.current - amount).max(0);
    }

    pub fn heal(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        self.current = (self.current + amount).min(self.max);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_health_starts_full_and_alive() {
        let health = Health::new(10);

        assert_eq!(health.max(), 10);
        assert_eq!(health.current(), 10);
        assert!(!health.is_dead());
    }

    #[test]
    fn damage_reduces_current_but_clamps_at_zero() {
        let mut health = Health::new(10);

        health.damage(4);
        assert_eq!(health.current(), 6);

        health.damage(100);
        assert_eq!(health.current(), 0);
        assert!(health.is_dead());
    }

    #[test]
    fn heal_restores_current_but_clamps_at_max() {
        let mut health = Health::new(10);
        health.damage(7);

        health.heal(2);
        assert_eq!(health.current(), 5);

        health.heal(100);
        assert_eq!(health.current(), 10);
    }

    #[test]
    fn non_positive_amounts_are_no_ops() {
        let mut health = Health::new(10);
        health.damage(3);

        health.damage(0);
        health.damage(-5);
        assert_eq!(health.current(), 7);

        health.heal(0);
        health.heal(-5);
        assert_eq!(health.current(), 7);
    }

    #[test]
    #[should_panic]
    fn non_positive_max_panics() {
        Health::new(0);
    }
}
