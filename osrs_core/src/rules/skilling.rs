//! Gathering-skill rolls (woodcutting, mining, fishing...): the chance of a successful attempt and
//! the chance of a skilling pet. Pure integer maths; the caller draws the random numbers.

/// The chance of a successful skilling attempt, as a numerator over 256 (OSRS wiki, "Skilling
/// success rate"): a straight line from `low` at level 1 to `high` at level 99,
///
/// `numerator = 1 + floor(low * (99 - L) / 98 + high * (L - 1) / 98 + 0.5)`
///
/// clamped to `0..=256` (256 always succeeds). `level` is capped at 99 and floored at 1. Each
/// resource and tool has its own `low`/`high`; an attempt succeeds when a uniform draw from
/// `0..256` is below the numerator.
pub fn success_numerator(low: i32, high: i32, level: i32) -> i32 {
    let level = level.clamp(1, 99) as i64;
    let (low, high) = (low as i64, high as i64);
    // floor(x / 98 + 0.5) == floor((2x + 98) / 196) for the integer x = low*(99-L) + high*(L-1)
    let x = low * (99 - level) + high * (level - 1);
    let rounded = (2 * x + 98).div_euclid(196);
    (1 + rounded).clamp(0, 256) as i32
}

/// Whether an attempt succeeds, given a draw from `0..256`.
pub fn attempt_succeeds(numerator: i32, draw_0_to_256: i32) -> bool {
    draw_0_to_256 < numerator
}

/// The "1 in N" chance of a skilling pet: the activity's base chance shortened by 25 per level
/// (OSRS wiki, e.g. Beaver: 1/(317,647 - level x 25) from a regular tree). Never below 1.
pub fn pet_chance_denominator(base: i32, level: i32) -> i32 {
    (base - level.max(1) * 25).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wikis_monkfish_example() {
        // low 48, high 90 at level 74: floor(12.245 + 67.041 + 0.5) = 79, so 80/256
        assert_eq!(success_numerator(48, 90, 74), 80);
    }

    #[test]
    fn the_ends_of_the_line_are_low_plus_one_and_high_plus_one() {
        assert_eq!(success_numerator(64, 200, 1), 65);
        assert_eq!(success_numerator(64, 200, 99), 201);
        assert_eq!(success_numerator(64, 200, 120), 201, "levels above 99 count as 99");
    }

    /// A regular tree's chart on the wiki: the chance at level 1 for each axe, and the level at
    /// which it reaches 100%.
    #[test]
    fn regular_tree_rates_match_the_wiki_chart() {
        let axes = [
            ("bronze", 64, 200, 0.2539, None),
            ("iron", 96, 300, 0.3789, Some(78)),
            ("steel", 128, 400, 0.5039, Some(47)),
            ("black", 144, 450, 0.5664, Some(37)),
            ("mithril", 160, 500, 0.6289, Some(29)),
            ("adamant", 192, 600, 0.7539, Some(17)),
            ("rune", 224, 700, 0.8789, Some(8)),
            ("dragon", 240, 750, 0.9414, Some(4)),
            ("crystal", 250, 800, 0.9805, Some(2)),
        ];
        for (axe, low, high, at_one, full_at) in axes {
            let p = success_numerator(low, high, 1) as f64 / 256.0;
            assert!((p - at_one).abs() < 0.0001, "{axe} at level 1: {p}");
            match full_at {
                Some(level) => {
                    assert_eq!(success_numerator(low, high, level), 256, "{axe} at {level}");
                    assert!(success_numerator(low, high, level - 1) < 256, "{axe} at {}", level - 1);
                }
                None => assert_eq!(success_numerator(low, high, 99), 201, "{axe} never reaches 100%"),
            }
        }
    }

    #[test]
    fn a_draw_below_the_numerator_succeeds() {
        assert!(attempt_succeeds(65, 64));
        assert!(!attempt_succeeds(65, 65));
        assert!(attempt_succeeds(256, 255));
    }

    #[test]
    fn beaver_chance_from_a_regular_tree() {
        assert_eq!(pet_chance_denominator(317_647, 1), 317_622);
        assert_eq!(pet_chance_denominator(317_647, 99), 315_172);
        assert_eq!(pet_chance_denominator(10, 99), 1);
    }
}
