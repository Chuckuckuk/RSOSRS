/// The real OSRS accuracy-roll/max-hit formulas (see OSRS_COMBAT_REFERENCE.md §3/§5), reduced
/// to this project's current "core trio" fidelity scope: base level only, no stance bonus, no
/// prayer multiplier, and equipment bonus permanently 0 (so every `(bonus + 64)` term collapses
/// to a flat 64). `effective_level` below is therefore just "+8", not the full
/// potion/prayer/stance/void pipeline the reference doc describes - widen it here, not at any
/// call site, if a later pass adds stance/prayer/equipment.
pub fn effective_level(base_level: i32) -> i32 {
    base_level + 8
}

/// Equipment bonus is always 0 this pass, so this is just `effective_level * 64`.
pub fn attack_roll(accuracy_level: i32) -> i32 {
    effective_level(accuracy_level) * 64
}

/// Monsters (`ActorKind::Npc` defending) get OSRS's own flat +9 on their raw Defence
/// level instead of the player's `effective_level(+8)` pipeline - see OSRS_COMBAT_REFERENCE.md
/// §4's note on this asymmetry. Equipment bonus is always 0 this pass either way.
pub fn defence_roll(defence_level: i32, defender_is_monster: bool) -> i32 {
    let level = if defender_is_monster { defence_level + 9 } else { effective_level(defence_level) };
    level * 64
}

/// `floor(0.5 + effective_level * 64/640)`, i.e. the melee max-hit formula with
/// StrengthBonus fixed at 0 - see OSRS_COMBAT_REFERENCE.md §5 for the general form and why
/// it's equivalent to `floor((effective_level * (bonus+320))/640)` written the other way.
pub fn max_hit(damage_level: i32) -> i32 {
    (0.5 + effective_level(damage_level) as f64 * 64.0 / 640.0).floor() as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_level_is_base_level_plus_eight() {
        assert_eq!(effective_level(0), 8);
        assert_eq!(effective_level(99), 107);
    }

    #[test]
    fn attack_roll_is_effective_level_times_64_equipment_bonus_always_zero_this_pass() {
        assert_eq!(attack_roll(0), 8 * 64);
        assert_eq!(attack_roll(99), 107 * 64);
    }

    #[test]
    fn defence_roll_for_a_player_defender_uses_the_same_effective_level_pipeline_as_attack() {
        assert_eq!(defence_roll(0, false), 8 * 64);
        assert_eq!(defence_roll(99, false), 107 * 64);
    }

    #[test]
    fn defence_roll_for_a_monster_defender_uses_the_flat_plus_9_quirk_not_effective_level() {
        // OSRS_COMBAT_REFERENCE.md §4: monsters get (DefenceLevel + 9), not the player's
        // effective_level(+8) pipeline - a deliberate asymmetry, not a typo.
        assert_eq!(defence_roll(0, true), 9 * 64);
        assert_eq!(defence_roll(99, true), 108 * 64);
    }

    #[test]
    fn max_hit_matches_the_known_melee_formula() {
        // floor(0.5 + effective_level * 64 / 640) - StrengthBonus fixed at 0 this pass.
        assert_eq!(max_hit(0), 1); // floor(0.5 + 8*64/640)    = floor(1.3)  = 1
        assert_eq!(max_hit(42), 5); // floor(0.5 + 50*64/640)  = floor(5.5)  = 5
        assert_eq!(max_hit(180), 19); // floor(0.5 + 188*64/640) = floor(19.3) = 19
    }

    #[test]
    fn max_hit_never_goes_negative_for_any_non_negative_level() {
        let mut level = 0;
        while level <= 500 {
            assert!(max_hit(level) >= 0);
            level += 25;
        }
    }
}
