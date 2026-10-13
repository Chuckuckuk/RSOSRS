use std::rc::Rc;

use super::combat_stats::CombatStats;
use super::homing_pulse_orb_definition::HomingPulseOrbDefinition;
use crate::tiles::TileCoord;

/// A live Slicer instance - the one truly new shape of entity this encounter needs: something
/// that exists in the world on its own, independent of the NPC's attack-selection loop that
/// spawned it, moving and dealing damage on its own schedule until it expires. Deliberately
/// NOT built on Actor/Mover/CollisionMap - it levitates, so it never collides with anything
/// and never occupies a tile for anyone else's pathing, which a plain greedy step-toward-
/// target (no `can_traverse` check at all) captures correctly.
#[derive(Debug, Clone)]
pub struct HomingPulseOrb {
    definition: Rc<HomingPulseOrbDefinition>,
    current_tile: TileCoord,
    ticks_remaining: i32,
    /// The boss's own `CombatStats`, captured by value at the moment it conjured this orb - the
    /// orb has no Actor of its own to roll a pulse's accuracy/max-hit against, so whoever spawns
    /// it must hand over its stats explicitly.
    attacker_stats: CombatStats,
    ticks_since_last_pulse: i32,
}

impl HomingPulseOrb {
    pub fn new(definition: Rc<HomingPulseOrbDefinition>, start_tile: TileCoord, attacker_stats: CombatStats) -> Self {
        let ticks_remaining = definition.initial_duration_ticks;
        Self { definition, current_tile: start_tile, ticks_remaining, attacker_stats, ticks_since_last_pulse: 0 }
    }

    pub fn definition(&self) -> &Rc<HomingPulseOrbDefinition> {
        &self.definition
    }

    pub fn current_tile(&self) -> TileCoord {
        self.current_tile
    }

    pub fn ticks_remaining(&self) -> i32 {
        self.ticks_remaining
    }

    pub fn is_expired(&self) -> bool {
        self.ticks_remaining <= 0
    }

    pub fn attacker_stats(&self) -> CombatStats {
        self.attacker_stats
    }

    /// Must be called exactly once per tick. Ticks the orb's own lifetime down first (an orb
    /// that expires this tick does not also get to move or pulse one last time), then takes one
    /// greedy step toward `target_tile` with no obstacle-avoidance at all, then returns true
    /// exactly on a tick a pulse is due - the caller is responsible for rolling the actual hit
    /// (via `definition().pulse_attack`) and calling `extend_on_hit` if it lands, since this
    /// type has no access to an RNG or a defender's reaction state.
    pub fn advance(&mut self, target_tile: TileCoord) -> bool {
        self.ticks_remaining -= 1;
        if self.is_expired() {
            return false;
        }

        let dx = (target_tile.x - self.current_tile.x).signum();
        let dz = (target_tile.z - self.current_tile.z).signum();
        if dx != 0 || dz != 0 {
            self.current_tile = TileCoord::with_plane(self.current_tile.x + dx, self.current_tile.z + dz, self.current_tile.plane);
        }

        self.ticks_since_last_pulse += 1;
        if self.ticks_since_last_pulse < self.definition.pulse_interval_ticks {
            return false;
        }

        self.ticks_since_last_pulse = 0;
        true
    }

    pub fn is_within_pulse_radius(&self, tile: TileCoord) -> bool {
        tile.chebyshev_distance_to(self.current_tile) <= self.definition.pulse_radius
    }

    /// Called by the caller only when a pulse (the tick `advance` returned true) actually dealt
    /// nonzero damage - "gains duration when it lands a hit".
    pub fn extend_on_hit(&mut self) {
        self.ticks_remaining += self.definition.duration_bonus_on_hit;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::{AttackDefinition, AttackRangeKind, AttackStyle};

    fn make_definition(initial_duration: i32, pulse_interval: i32, pulse_radius: i32, duration_bonus_on_hit: i32) -> Rc<HomingPulseOrbDefinition> {
        let pulse_attack = AttackDefinition::new("Test Pulse", AttackRangeKind::Melee, 0, 0, AttackStyle::Magic).shared();
        Rc::new(HomingPulseOrbDefinition::new(
            "Test Orb",
            initial_duration,
            pulse_interval,
            pulse_radius,
            duration_bonus_on_hit,
            pulse_attack,
        ))
    }

    fn attacker_stats() -> CombatStats {
        CombatStats::new(50, 50, 50, 50, 50).unwrap()
    }

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    #[test]
    fn advance_moves_one_greedy_diagonal_step_toward_the_target_every_tick_it_has_left() {
        let mut orb = HomingPulseOrb::new(make_definition(3, 1, 1, 2), tc(0, 0), attacker_stats());
        let target = tc(5, 5);

        assert!(orb.advance(target)); // pulses every tick (interval 1)
        assert_eq!(orb.current_tile(), tc(1, 1));
        assert!(orb.advance(target));
        assert_eq!(orb.current_tile(), tc(2, 2));
        assert!(!orb.advance(target), "The 3rd call consumes the last tick of duration and expires instead of pulsing.");
        assert_eq!(orb.current_tile(), tc(2, 2), "An expiring orb does not also get to move.");
        assert!(orb.is_expired());
    }

    #[test]
    fn advance_only_pulses_once_pulse_interval_ticks_have_elapsed() {
        let mut orb = HomingPulseOrb::new(make_definition(10, 2, 1, 2), tc(0, 0), attacker_stats());
        let target = tc(20, 20);

        assert!(!orb.advance(target)); // 1 of 2
        assert!(orb.advance(target)); // 2 of 2 - pulses, resets
        assert!(!orb.advance(target)); // 1 of 2 again
        assert!(orb.advance(target)); // 2 of 2 - pulses again
    }

    #[test]
    fn is_within_pulse_radius_uses_chebyshev_distance_from_the_orbs_current_tile() {
        let orb = HomingPulseOrb::new(make_definition(5, 1, 2, 2), tc(10, 10), attacker_stats());

        assert!(orb.is_within_pulse_radius(tc(12, 10)));
        assert!(!orb.is_within_pulse_radius(tc(13, 10)));
    }

    #[test]
    fn extend_on_hit_adds_the_definitions_duration_bonus_to_ticks_remaining() {
        let mut orb = HomingPulseOrb::new(make_definition(3, 1, 1, 5), tc(0, 0), attacker_stats());
        orb.advance(tc(1, 1)); // ticks_remaining now 2

        orb.extend_on_hit();

        assert_eq!(orb.ticks_remaining(), 7);
    }
}
