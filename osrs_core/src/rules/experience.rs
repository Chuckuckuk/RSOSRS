//! The experience table. The game keeps experience in tenths of a point (an oak log is 37.5 XP,
//! stored as 375), so everything here is in tenths too.

use std::sync::LazyLock;

/// The most experience a skill can hold: 200,000,000 XP.
pub const MAX_XP_TENTHS: i32 = 2_000_000_000;

/// The highest real level.
pub const MAX_LEVEL: i32 = 99;

/// The experience (whole points) a skill needs to reach `level`: the sum, over every level below
/// it, of `floor(l + 300 * 2^(l / 7))`, quartered and floored. Level 2 needs 83, level 99 needs
/// 13,034,431. Levels below 1 count as 1 and above 99 as 99.
pub fn xp_for_level(level: i32) -> i32 {
    let mut points = 0i64;
    for l in 1..level.clamp(1, MAX_LEVEL) {
        points += (l as f64 + 300.0 * 2f64.powf(l as f64 / 7.0)).floor() as i64;
    }
    (points / 4) as i32
}

/// Every level's threshold in tenths, index `level - 1`, worked out once.
static THRESHOLDS: LazyLock<[i32; MAX_LEVEL as usize]> =
    LazyLock::new(|| std::array::from_fn(|i| xp_for_level(i as i32 + 1) * 10));

/// [`xp_for_level`] in tenths of a point.
pub fn xp_tenths_for_level(level: i32) -> i32 {
    THRESHOLDS[(level.clamp(1, MAX_LEVEL) - 1) as usize]
}

/// The level `xp_tenths` of experience gives: the highest level whose threshold it has reached.
pub fn level_for_xp_tenths(xp_tenths: i32) -> i32 {
    // level 1's threshold is 0, so at least one threshold is always reached
    THRESHOLDS.partition_point(|&threshold| threshold <= xp_tenths).max(1) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_match_the_wiki_experience_table() {
        assert_eq!(xp_for_level(1), 0);
        assert_eq!(xp_for_level(2), 83);
        assert_eq!(xp_for_level(3), 174);
        assert_eq!(xp_for_level(10), 1_154);
        assert_eq!(xp_for_level(50), 101_333);
        assert_eq!(xp_for_level(92), 6_517_253);
        assert_eq!(xp_for_level(99), 13_034_431);
    }

    #[test]
    fn levels_are_the_highest_threshold_reached() {
        assert_eq!(level_for_xp_tenths(0), 1);
        assert_eq!(level_for_xp_tenths(829), 1);
        assert_eq!(level_for_xp_tenths(830), 2);
        assert_eq!(level_for_xp_tenths(130_344_310), 99);
        assert_eq!(level_for_xp_tenths(MAX_XP_TENTHS), 99);
    }
}
