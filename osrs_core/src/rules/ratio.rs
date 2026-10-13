use serde::{Deserialize, Serialize};

/// An exact multiplier (`num / den`). OSRS applies most gear multipliers as "multiply, then floor",
/// in a fixed order, so they are carried as fractions (7/6 for a Slayer helm, 6/5 for an imbued
/// Salve amulet, 11/10 for Void) instead of floats that could round the wrong way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ratio {
    pub num: i64,
    pub den: i64,
}

impl Ratio {
    pub const ONE: Ratio = Ratio { num: 1, den: 1 };

    pub const fn new(num: i64, den: i64) -> Self {
        assert!(den > 0, "a Ratio's denominator must be positive");
        Self { num, den }
    }

    /// `floor(value * num / den)`.
    pub fn apply(self, value: i64) -> i64 {
        (value * self.num).div_euclid(self.den)
    }

    pub fn is_one(self) -> bool {
        self.num == self.den
    }
}

impl Default for Ratio {
    fn default() -> Self {
        Self::ONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_floors() {
        assert_eq!(Ratio::new(7, 6).apply(37), 43); // 43.16
        assert_eq!(Ratio::new(6, 5).apply(10), 12);
        assert_eq!(Ratio::new(1, 2).apply(5), 2);
    }

    #[test]
    fn apply_floors_toward_negative_infinity() {
        assert_eq!(Ratio::new(1, 2).apply(-5), -3);
    }
}
