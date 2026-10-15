//! Level pipeline, accuracy and max-hit formulas (OSRS_COMBAT_REFERENCE.md §3, §5).

use super::Ratio;

/// Global cap on a single hit, regardless of how many multipliers stack (reference §5).
pub const MAX_HIT_CAP: i32 = 200;

/// `floor( floor(level * prayer_percent / 100) + stance_bonus + 8 ) * void`, floored again.
///
/// `level` already includes any potion boost (boosts are additive and come before the prayer
/// multiplier). `prayer_percent` is 100 when no relevant prayer is active, 123 for Piety's
/// Strength, and so on.
pub fn effective_level(level: i32, prayer_percent: i32, stance_bonus: i32, void: Ratio) -> i32 {
    let boosted = (level.max(0) as i64 * prayer_percent as i64).div_euclid(100);
    void.apply(boosted + stance_bonus as i64 + 8) as i32
}

/// Attack or defence roll: `effective_level * (equipment_bonus + 64)`. A bonus below -64 would
/// make the second term negative; the game never rolls below zero, so it is clamped there.
pub fn roll(effective_level: i32, equipment_bonus: i32) -> i64 {
    effective_level as i64 * (equipment_bonus as i64 + 64).max(0)
}

/// A monster defender's roll: its raw Defence (or Magic, against magic) level gets a flat +9
/// instead of the player's `effective_level` pipeline.
pub fn monster_defence_roll(level: i32, equipment_bonus: i32) -> i64 {
    roll(level + 9, equipment_bonus)
}

/// Effective level a player defends against magic with: 70% of the effective Magic level plus 30%
/// of the effective Defence level, floored once.
pub fn player_magic_defence_level(effective_magic: i32, effective_defence: i32) -> i32 {
    ((7 * effective_magic as i64 + 3 * effective_defence as i64).div_euclid(10)) as i32
}

/// Melee and ranged base max hit: `floor((effective * (strength_bonus + 64) + 320) / 640)`.
/// For ranged, `strength_bonus` is the Ranged Strength bonus (ammo, or the thrown weapon itself).
pub fn base_max_hit(effective_strength: i32, strength_bonus: i32) -> i32 {
    let numerator = effective_strength as i64 * (strength_bonus as i64 + 64) + 320;
    numerator.div_euclid(640) as i32
}

/// Applies each multiplier in order, flooring after every one (the game does not combine them
/// first), and then the global cap.
pub fn apply_multipliers(value: i32, multipliers: &[Ratio]) -> i32 {
    let mut result = value as i64;
    for ratio in multipliers {
        result = ratio.apply(result);
    }
    result.clamp(0, MAX_HIT_CAP as i64) as i32
}

/// A powered staff's own base max hit, scaled by the player's (boosted) Magic level:
/// `floor(magic_level / divisor) + offset`. Trident of the seas is `(3, -5)`, swamp `(3, -2)`,
/// Sanguinesti `(3, -1)`, Tumeken's shadow `(3, 1)`.
pub fn powered_staff_base_max_hit(magic_level: i32, divisor: i32, offset: i32) -> i32 {
    (magic_level.max(0).div_euclid(divisor.max(1)) + offset).max(0)
}

/// Magic max hit: `floor(base * (1000 + damage_bonus_permille) / 1000)`. Every Magic Damage %
/// source (gear, prayers such as Augury, Salve) is summed into one permille figure first - they
/// add, they do not multiply - and the sum is floored once.
pub fn magic_max_hit(base: i32, damage_bonus_permille: i32) -> i32 {
    ((base as i64 * (1000 + damage_bonus_permille as i64)).div_euclid(1000)).max(0) as i32
}

/// The twisted bow's scaling against the target's Magic (the higher of its Magic level and its
/// Magic attack bonus), as a percentage: accuracy `140 + floor((3m - 10) / 100) - floor((3m/10 - 100)^2 / 100)`
/// capped at 140, damage `250 + floor((3m - 14) / 100) - floor((3m/10 - 140)^2 / 100)` capped at
/// 250. `m` is capped at 250 (350 in the Chambers of Xeric).
pub fn twisted_bow_accuracy_percent(target_magic: i32, magic_cap: i32) -> i32 {
    let m = target_magic.clamp(0, magic_cap) as i64;
    let square = (3 * m - 1000) * (3 * m - 1000) / 10_000;
    (140 + (3 * m - 10).div_euclid(100) - square).clamp(0, 140) as i32
}

pub fn twisted_bow_damage_percent(target_magic: i32, magic_cap: i32) -> i32 {
    let m = target_magic.clamp(0, magic_cap) as i64;
    let square = (3 * m - 1400) * (3 * m - 1400) / 10_000;
    (250 + (3 * m - 14).div_euclid(100) - square).clamp(0, 250) as i32
}

/// Combat level (reference §3), in exact integer arithmetic. `base` is x1000 of the 0.25 term and
/// each style is x1000 of its 0.325 term, then the sum is floored.
pub fn combat_level(attack: i32, strength: i32, defence: i32, hitpoints: i32, prayer: i32, ranged: i32, magic: i32) -> i32 {
    let base = 250 * (defence + hitpoints + prayer / 2) as i64;
    let melee = 325 * (attack + strength) as i64;
    let range = 325 * (ranged * 3 / 2) as i64;
    let mage = 325 * (magic * 3 / 2) as i64;
    ((base + melee.max(range).max(mage)) / 1000) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_level_applies_prayer_then_stance_then_eight_then_void() {
        // 99 str + 19 (super strength) = 118, Piety x1.23 = 145.14 -> 145, aggressive +3, +8.
        assert_eq!(effective_level(118, 123, 3, Ratio::ONE), 156);
        // Void multiplies the whole thing and floors: 156 * 1.1 = 171.6.
        assert_eq!(effective_level(118, 123, 3, Ratio::new(11, 10)), 171);
    }

    #[test]
    fn max_hit_matches_a_well_known_maxed_melee_setup() {
        // The effective level above with +86 strength bonus.
        assert_eq!(base_max_hit(156, 86), 37);
        // Both forms of the formula in the reference doc agree.
        for bonus in [0, 10, 64, 120] {
            for eff in [9, 50, 107, 150] {
                let alt = ((0.5 + eff as f64 * (bonus as f64 + 64.0) / 640.0).floor()) as i32;
                assert_eq!(base_max_hit(eff, bonus), alt, "eff={eff} bonus={bonus}");
            }
        }
    }

    #[test]
    fn monster_defence_uses_the_flat_nine_not_the_effective_level() {
        assert_eq!(monster_defence_roll(100, 20), 109 * 84);
        assert_eq!(roll(effective_level(100, 100, 0, Ratio::ONE), 20), 108 * 84);
    }

    #[test]
    fn negative_bonuses_never_produce_a_negative_roll() {
        assert_eq!(roll(100, -200), 0);
    }

    #[test]
    fn multipliers_floor_between_steps_and_the_result_is_capped() {
        // Sequentially: 5 * 3/2 = 7, then 7 * 3/2 = 10. Combined first (9/4) it would be 11.
        assert_eq!(apply_multipliers(5, &[Ratio::new(3, 2), Ratio::new(3, 2)]), 10);
        assert_eq!(apply_multipliers(5, &[Ratio::new(9, 4)]), 11);
        assert_eq!(apply_multipliers(190, &[Ratio::new(2, 1)]), MAX_HIT_CAP);
        assert_eq!(apply_multipliers(10, &[]), 10);
    }

    #[test]
    fn magic_bonuses_add_before_the_single_floor() {
        assert_eq!(magic_max_hit(30, 0), 30); // Ice Barrage, no gear
        assert_eq!(magic_max_hit(30, 100), 33); // +10%
        assert_eq!(magic_max_hit(30, 40), 31); // +4% (Augury alone): 31.2 -> 31
        assert_eq!(magic_max_hit(16, 40), 16); // 16.64 -> 16
    }

    #[test]
    fn powered_staves_scale_from_the_magic_level() {
        assert_eq!(powered_staff_base_max_hit(99, 3, -5), 28); // trident of the seas
        assert_eq!(powered_staff_base_max_hit(99, 3, -2), 31); // trident of the swamp
        assert_eq!(powered_staff_base_max_hit(99, 3, -1), 32); // sanguinesti
        assert_eq!(powered_staff_base_max_hit(99, 3, 1), 34); // tumeken's shadow
        assert_eq!(powered_staff_base_max_hit(1, 3, -5), 0);
    }

    #[test]
    fn player_magic_defence_is_seventy_percent_magic_thirty_percent_defence() {
        assert_eq!(player_magic_defence_level(100, 50), 85);
        assert_eq!(player_magic_defence_level(99, 1), 69); // 69.6
    }

    #[test]
    fn the_twisted_bow_accuracy_climbs_to_its_cap_by_about_240_magic() {
        assert_eq!(twisted_bow_accuracy_percent(200, 250), 129);
        assert_eq!(twisted_bow_accuracy_percent(230, 250), 137);
        assert_eq!(twisted_bow_accuracy_percent(250, 250), 140);
        assert_eq!(twisted_bow_accuracy_percent(300, 250), 140, "magic above the cap counts as the cap");
        assert!(twisted_bow_accuracy_percent(1, 250) < 140, "a very low Magic target is not scaled up");
    }

    #[test]
    fn the_twisted_bow_damage_scales_up_with_magic_and_xerician_raises_the_cap() {
        assert_eq!(twisted_bow_damage_percent(250, 250), 215);
        assert_eq!(twisted_bow_damage_percent(350, 350), 248);
        assert!(twisted_bow_damage_percent(150, 250) < twisted_bow_damage_percent(250, 250));
        assert!(twisted_bow_damage_percent(350, 350) > twisted_bow_damage_percent(350, 250));
    }

    #[test]
    fn combat_level_matches_known_values() {
        assert_eq!(combat_level(1, 1, 1, 10, 1, 1, 1), 3); // fresh account
        assert_eq!(combat_level(99, 99, 99, 99, 99, 99, 99), 126); // maxed melee
        // Pure ranged/magic caps lower than melee: 0.25*(99+99+49) + 0.325*148 = 109.8
        assert_eq!(combat_level(1, 1, 99, 99, 99, 99, 1), 109);
    }
}
