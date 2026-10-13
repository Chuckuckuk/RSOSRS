//! Tick-synchronised resources: HP regen, stat drift, prayer drain, run energy, special attack
//! energy and poison/venom (OSRS_COMBAT_REFERENCE.md §2, §6). Rates are in game ticks.

// ---- hitpoints ----

pub const HP_REGEN_BASE_INTERVAL_TICKS: i32 = 100;

/// Ticks per 1 HP of natural regen. Rapid Heal and the Hitpoints cape each halve it but do not
/// stack with one another; a Regeneration bracelet halves it again and does stack with either.
pub fn hp_regen_interval_ticks(rapid_heal_or_hitpoints_cape: bool, regen_bracelet: bool) -> i32 {
    let mut interval = HP_REGEN_BASE_INTERVAL_TICKS;
    if rapid_heal_or_hitpoints_cape {
        interval /= 2;
    }
    if regen_bracelet {
        interval /= 2;
    }
    interval
}

// ---- stat boosts and drains ----

pub const STAT_DRIFT_INTERVAL_TICKS: i32 = 100;
pub const STAT_DRIFT_INTERVAL_PRESERVE_TICKS: i32 = 150;

/// How often a stat that differs from its base moves one level back toward it. Preserve slows
/// only the decay of BOOSTED stats; lowered stats still recover at the normal rate.
pub fn stat_drift_interval_ticks(current: i32, base: i32, preserve_active: bool) -> i32 {
    if preserve_active && current > base {
        STAT_DRIFT_INTERVAL_PRESERVE_TICKS
    } else {
        STAT_DRIFT_INTERVAL_TICKS
    }
}

/// One drift step: a level toward `base`.
pub fn drift_toward_base(current: i32, base: i32) -> i32 {
    match current.cmp(&base) {
        std::cmp::Ordering::Greater => current - 1,
        std::cmp::Ordering::Less => current + 1,
        std::cmp::Ordering::Equal => current,
    }
}

/// A potion's boost to a stat: `flat + floor(base * percent / 100)` (Super strength is 5 + 15%).
pub fn potion_boost(base: i32, flat: i32, percent: i32) -> i32 {
    flat + (base.max(0) * percent).div_euclid(100)
}

// ---- prayer ----

/// `60 + 2 * prayer bonus`: how much "drain effect" must accumulate to cost one prayer point.
pub fn prayer_drain_resistance(prayer_bonus: i32) -> i32 {
    60 + 2 * prayer_bonus.max(0)
}

/// The internal counter behind prayer drain. Every tick it gains the summed drain effect of all
/// active prayers; whenever it exceeds the resistance, one point is removed and the resistance
/// is subtracted. (Piety, effect 24, costs a point every 60/24 = 2.5 ticks = 1.5s.)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrayerDrainCounter {
    counter: i32,
}

impl PrayerDrainCounter {
    pub fn counter(&self) -> i32 {
        self.counter
    }

    /// Advances one tick and returns the whole points to deduct.
    pub fn tick(&mut self, total_drain_effect: i32, prayer_bonus: i32) -> i32 {
        if total_drain_effect <= 0 {
            self.counter = 0;
            return 0;
        }
        let resistance = prayer_drain_resistance(prayer_bonus);
        self.counter += total_drain_effect;
        let mut points = 0;
        while self.counter > resistance {
            self.counter -= resistance;
            points += 1;
        }
        points
    }
}

/// Smite: the victim loses a quarter of the damage dealt as prayer points.
pub fn smite_drain(damage_dealt: i32) -> i32 {
    damage_dealt.max(0) / 4
}

/// Redemption: heals a quarter of the player's Prayer level when HP falls below 10%.
pub fn redemption_heal(prayer_level: i32) -> i32 {
    prayer_level.max(0) / 4
}

// ---- run energy ----

/// Run energy is tracked in hundredths of a percent.
pub const RUN_ENERGY_MAX: i32 = 10_000;

/// Units lost per tick while running: `floor(60 + 67 * clamp(weight, 0, 64) / 64) * (1 - agility / 300)`.
/// A Stamina potion cuts it to 30%; a charged Ring of endurance to 85%.
pub fn run_energy_drain_per_tick(weight_kg: f64, agility_level: i32, stamina_potion: bool, ring_of_endurance: bool) -> i32 {
    let weight = weight_kg.clamp(0.0, 64.0);
    let base = (60.0 + 67.0 * weight / 64.0).floor() as i64;
    let mut drain = base * (300 - agility_level.clamp(0, 300) as i64) / 300;
    if stamina_potion {
        drain = drain * 3 / 10;
    }
    if ring_of_endurance {
        drain = drain * 85 / 100;
    }
    drain as i32
}

/// Units regained per tick while not running: `floor(agility / 10) + 15`, x1.3 (floored) in the
/// full Graceful outfit.
pub fn run_energy_regen_per_tick(agility_level: i32, graceful: bool) -> i32 {
    let regen = agility_level.max(0) / 10 + 15;
    if graceful {
        regen * 13 / 10
    } else {
        regen
    }
}

// ---- special attack energy ----

pub const SPEC_ENERGY_MAX: i32 = 100;
pub const SPEC_ENERGY_REGEN_AMOUNT: i32 = 10;

/// 10% every 50 ticks (30 seconds); a Lightbearer doubles the rate.
pub fn spec_regen_interval_ticks(lightbearer: bool) -> i32 {
    if lightbearer {
        25
    } else {
        50
    }
}

// ---- poison and venom ----

pub const DAMAGE_OVER_TIME_INTERVAL_TICKS: i32 = 30;

/// Poison deals `floor((severity + 4) / 5)`; severity drops by one per hit, so the damage steps
/// down every five hits and the poison ends when severity reaches zero.
pub fn poison_damage(severity: i32) -> i32 {
    if severity <= 0 {
        0
    } else {
        (severity + 4) / 5
    }
}

pub const VENOM_START_DAMAGE: i32 = 6;
pub const VENOM_STEP: i32 = 2;
pub const VENOM_MAX_DAMAGE: i32 = 20;

/// Venom starts at 6 and climbs by 2 per hit to a ceiling of 20; it never wears off by itself.
pub fn next_venom_damage(previous: i32) -> i32 {
    if previous < VENOM_START_DAMAGE {
        VENOM_START_DAMAGE
    } else {
        (previous + VENOM_STEP).min(VENOM_MAX_DAMAGE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_regen_halves_for_rapid_heal_or_cape_and_again_for_the_bracelet() {
        assert_eq!(hp_regen_interval_ticks(false, false), 100);
        assert_eq!(hp_regen_interval_ticks(true, false), 50);
        assert_eq!(hp_regen_interval_ticks(false, true), 50);
        assert_eq!(hp_regen_interval_ticks(true, true), 25); // 4 HP per minute
    }

    #[test]
    fn stats_drift_one_level_toward_base_and_preserve_only_slows_boost_decay() {
        assert_eq!(drift_toward_base(110, 99), 109);
        assert_eq!(drift_toward_base(90, 99), 91);
        assert_eq!(drift_toward_base(99, 99), 99);
        assert_eq!(stat_drift_interval_ticks(110, 99, true), 150);
        assert_eq!(stat_drift_interval_ticks(90, 99, true), 100);
        assert_eq!(stat_drift_interval_ticks(110, 99, false), 100);
    }

    #[test]
    fn potion_boosts_use_flat_plus_percent_of_base() {
        assert_eq!(potion_boost(99, 5, 15), 19); // super strength
        assert_eq!(potion_boost(99, 3, 10), 12); // ranging / magic potion style
    }

    #[test]
    fn drain_resistance_grows_two_per_prayer_bonus() {
        assert_eq!(prayer_drain_resistance(0), 60);
        assert_eq!(prayer_drain_resistance(30), 120);
    }

    #[test]
    fn piety_costs_a_point_every_one_and_a_half_seconds() {
        // 60 ticks of effect-24 drain = 1440 / 60 = 24 points per 60 ticks? No: 1440 effect
        // units / 60 resistance = 24 points in 60 ticks -> one point per 2.5 ticks = 1.5s.
        let mut counter = PrayerDrainCounter::default();
        let drained: i32 = (0..60).map(|_| counter.tick(24, 0)).sum();
        assert_eq!(drained, 23); // the counter must EXCEED resistance, so the last unit is held
    }

    #[test]
    fn the_lowest_tier_prayer_lasts_thirty_six_seconds_per_point() {
        let mut counter = PrayerDrainCounter::default();
        let mut ticks_to_first_point = 0;
        loop {
            ticks_to_first_point += 1;
            if counter.tick(1, 0) > 0 {
                break;
            }
        }
        assert_eq!(ticks_to_first_point, 61); // resistance 60, drain must exceed it
    }

    #[test]
    fn prayer_bonus_extends_the_drain_time() {
        let mut plain = PrayerDrainCounter::default();
        let mut geared = PrayerDrainCounter::default();
        let plain_points: i32 = (0..600).map(|_| plain.tick(12, 0)).sum();
        let geared_points: i32 = (0..600).map(|_| geared.tick(12, 30)).sum();
        assert!(geared_points < plain_points);
    }

    #[test]
    fn no_active_prayers_resets_the_counter() {
        let mut counter = PrayerDrainCounter::default();
        counter.tick(24, 0);
        assert_eq!(counter.tick(0, 0), 0);
        assert_eq!(counter.counter(), 0);
    }

    #[test]
    fn smite_and_redemption_are_quarter_rules() {
        assert_eq!(smite_drain(41), 10);
        assert_eq!(redemption_heal(99), 24);
    }

    #[test]
    fn run_energy_drain_and_regen_follow_the_wiki_formulas() {
        // 0 kg, 1 agility: floor(60) * (299/300) = 59 (integer maths floors 59.8)
        assert_eq!(run_energy_drain_per_tick(0.0, 1, false, false), 59);
        // 64+ kg is clamped at 64: floor(60 + 67) = 127
        assert_eq!(run_energy_drain_per_tick(200.0, 0, false, false), 127);
        assert_eq!(run_energy_drain_per_tick(0.0, 99, false, false), 40);
        assert_eq!(run_energy_drain_per_tick(0.0, 99, true, false), 12);
        assert_eq!(run_energy_drain_per_tick(-5.0, 0, false, false), 60); // negative weight = 0 kg

        assert_eq!(run_energy_regen_per_tick(99, false), 24);
        assert_eq!(run_energy_regen_per_tick(99, true), 31);
        assert_eq!(run_energy_regen_per_tick(1, false), 15);
    }

    #[test]
    fn spec_energy_regens_ten_percent_per_fifty_ticks() {
        assert_eq!(spec_regen_interval_ticks(false), 50);
        assert_eq!(spec_regen_interval_ticks(true), 25);
    }

    #[test]
    fn poison_damage_steps_down_every_five_hits() {
        assert_eq!(poison_damage(30), 6); // wiki's worked example
        assert_eq!(poison_damage(26), 6);
        assert_eq!(poison_damage(25), 5);
        assert_eq!(poison_damage(1), 1);
        assert_eq!(poison_damage(0), 0);
    }

    #[test]
    fn venom_climbs_by_two_to_a_ceiling_of_twenty() {
        let mut damage = 0;
        let mut seen = Vec::new();
        for _ in 0..10 {
            damage = next_venom_damage(damage);
            seen.push(damage);
        }
        assert_eq!(seen, vec![6, 8, 10, 12, 14, 16, 18, 20, 20, 20]);
    }
}

// ---- projectile timing ----

/// Ticks a hit spends in the air before it lands: none for melee (it lands on the attack tick),
/// `1 + floor((3 + distance) / 6)` for ranged and `1 + floor((1 + distance) / 3)` for magic.
pub fn projectile_travel_ticks(style: crate::combat::AttackStyle, distance: i32) -> i32 {
    use crate::combat::AttackStyle;
    let distance = distance.max(0);
    match style {
        AttackStyle::Melee => 0,
        AttackStyle::Ranged => 1 + (3 + distance) / 6,
        AttackStyle::Magic => 1 + (1 + distance) / 3,
    }
}

#[cfg(test)]
mod projectile_tests {
    use super::*;
    use crate::combat::AttackStyle;

    #[test]
    fn travel_time_grows_with_distance_by_style() {
        assert_eq!(projectile_travel_ticks(AttackStyle::Melee, 5), 0);
        assert_eq!(projectile_travel_ticks(AttackStyle::Ranged, 1), 1);
        assert_eq!(projectile_travel_ticks(AttackStyle::Ranged, 3), 2);
        assert_eq!(projectile_travel_ticks(AttackStyle::Ranged, 9), 3);
        assert_eq!(projectile_travel_ticks(AttackStyle::Magic, 1), 1);
        assert_eq!(projectile_travel_ticks(AttackStyle::Magic, 2), 2);
        assert_eq!(projectile_travel_ticks(AttackStyle::Magic, 10), 4);
    }
}
