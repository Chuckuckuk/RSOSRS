use super::attack_style::AttackStyle;
use crate::rules;

/// How fast an attack's projectile crosses the gap between attacker and target, for attacks that
/// roll when they are released and land some ticks later.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProjectileSpeed {
    /// Chebyshev tiles per tick: a hit `d` tiles away is in the air `ceil(d / speed)` ticks, and
    /// never less than one.
    TilesPerTick(f32),
    /// The game's own flight time for a player's ranged weapon: `1 + floor((3 + d) / 6)`.
    OsrsRanged,
    /// The game's own flight time for a player's spell: `1 + floor((1 + d) / 3)`.
    OsrsMagic,
}

impl ProjectileSpeed {
    /// What an attack flies at when it does not say: three tiles a tick, about a spell's pace.
    pub const DEFAULT: ProjectileSpeed = ProjectileSpeed::TilesPerTick(3.0);

    pub fn tiles_per_tick(speed: f32) -> Self {
        assert!(speed > 0.0 && speed.is_finite(), "a projectile speed must be a positive number of tiles per tick");
        ProjectileSpeed::TilesPerTick(speed)
    }

    /// Ticks a projectile released `distance` tiles away spends in the air.
    pub fn flight_ticks(self, distance: i32) -> i32 {
        let distance = distance.max(0);
        match self {
            ProjectileSpeed::TilesPerTick(speed) => ((distance as f32 / speed).ceil() as i32).max(1),
            ProjectileSpeed::OsrsRanged => rules::projectile_travel_ticks(AttackStyle::Ranged, distance),
            ProjectileSpeed::OsrsMagic => rules::projectile_travel_ticks(AttackStyle::Magic, distance),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_per_tick_rounds_the_flight_up_and_never_goes_below_one_tick() {
        let speed = ProjectileSpeed::tiles_per_tick(3.0);

        assert_eq!(speed.flight_ticks(0), 1);
        assert_eq!(speed.flight_ticks(1), 1);
        assert_eq!(speed.flight_ticks(3), 1);
        assert_eq!(speed.flight_ticks(4), 2);
        assert_eq!(speed.flight_ticks(9), 3);
        assert_eq!(speed.flight_ticks(10), 4);
    }

    #[test]
    fn a_faster_projectile_arrives_sooner() {
        assert_eq!(ProjectileSpeed::tiles_per_tick(6.0).flight_ticks(12), 2);
        assert_eq!(ProjectileSpeed::tiles_per_tick(1.0).flight_ticks(12), 12);
    }

    #[test]
    fn the_players_formulas_are_kept_as_they_are() {
        assert_eq!(ProjectileSpeed::OsrsRanged.flight_ticks(3), 2);
        assert_eq!(ProjectileSpeed::OsrsRanged.flight_ticks(9), 3);
        assert_eq!(ProjectileSpeed::OsrsMagic.flight_ticks(2), 2);
        assert_eq!(ProjectileSpeed::OsrsMagic.flight_ticks(10), 4);
    }

    #[test]
    #[should_panic]
    fn a_zero_speed_is_rejected() {
        ProjectileSpeed::tiles_per_tick(0.0);
    }
}
