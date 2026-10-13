/// Current/max prayer points with clamped drain/restore. Plain data, same shape as `Health`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrayerPool {
    max: i32,
    current: i32,
}

impl PrayerPool {
    /// Panics if `max <= 0` (mirrors the C# `ArgumentOutOfRangeException`).
    pub fn new(max: i32) -> Self {
        assert!(max > 0, "max prayer must be positive, was {max}");
        Self { max, current: max }
    }

    pub fn max(&self) -> i32 {
        self.max
    }

    pub fn current(&self) -> i32 {
        self.current
    }

    pub fn drain(&mut self, amount: i32) {
        if amount <= 0 {
            return;
        }
        self.current = (self.current - amount).max(0);
    }

    pub fn restore(&mut self, amount: i32) {
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
    fn new_prayer_pool_starts_full() {
        let prayer = PrayerPool::new(99);

        assert_eq!(prayer.max(), 99);
        assert_eq!(prayer.current(), 99);
    }

    #[test]
    fn drain_reduces_current_but_clamps_at_zero() {
        let mut prayer = PrayerPool::new(99);

        prayer.drain(40);
        assert_eq!(prayer.current(), 59);

        prayer.drain(1000);
        assert_eq!(prayer.current(), 0);
    }

    #[test]
    fn restore_increases_current_but_clamps_at_max() {
        let mut prayer = PrayerPool::new(99);
        prayer.drain(50);

        prayer.restore(10);
        assert_eq!(prayer.current(), 59);

        prayer.restore(1000);
        assert_eq!(prayer.current(), 99);
    }

    #[test]
    fn non_positive_amounts_are_no_ops() {
        let mut prayer = PrayerPool::new(99);
        prayer.drain(20);

        prayer.drain(0);
        prayer.drain(-5);
        assert_eq!(prayer.current(), 79);

        prayer.restore(0);
        prayer.restore(-5);
        assert_eq!(prayer.current(), 79);
    }
}
