use super::combat_target_view::CombatTargetView;
use super::mover::Mover;
use crate::collision::CollisionMap;
use crate::combat::AttackStyle;
use crate::pathfinding::{BfsPathfinder, TargetSpec};

/// Decides how an NPC moves each tick relative to its target. Policies are stateless, so one
/// instance can be shared (via `Rc`) between phases and NPCs.
pub trait MovementPolicy {
    fn step(
        &self,
        mover: &mut Mover,
        pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        target: Option<&CombatTargetView>,
        target_style: AttackStyle,
    );

    /// Whether the NPC holds its ground while one of its attacks can reach its target (the base
    /// rule every monster follows: no walking up to a target it can already hit). Policies that
    /// back away on purpose (kiting) opt out.
    fn holds_when_in_reach(&self) -> bool {
        true
    }
}

/// Never moves, regardless of the target.
#[derive(Debug, Clone, Copy, Default)]
pub struct StationaryMovementPolicy;

impl MovementPolicy for StationaryMovementPolicy {
    fn step(
        &self,
        mover: &mut Mover,
        _pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        _target: Option<&CombatTargetView>,
        _target_style: AttackStyle,
    ) {
        mover.step(map, 0);
    }
}

/// Greedy-steps toward the target until orthogonally adjacent to its footprint, then holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct ChaseToMeleeRangeMovementPolicy;

impl MovementPolicy for ChaseToMeleeRangeMovementPolicy {
    fn step(
        &self,
        mover: &mut Mover,
        pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        target: Option<&CombatTargetView>,
        _target_style: AttackStyle,
    ) {
        let Some(target) = target else {
            mover.step(map, 0);
            return;
        };

        let adjacent = TargetSpec::adjacent(target.tile, target.footprint_size, target.footprint_size)
            .is_satisfied_by(mover.actor.current_tile());
        if adjacent {
            mover.step(map, 0);
        } else {
            mover.step_towards(pathfinder, map, target.tile);
        }
    }
}

/// Walks toward the target until adjacent even while one of its attacks can already reach (a boss
/// that advances on the player rather than firing from where it stands).
#[derive(Debug, Clone, Copy, Default)]
pub struct AdvanceMovementPolicy;

impl MovementPolicy for AdvanceMovementPolicy {
    fn step(
        &self,
        mover: &mut Mover,
        pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        target: Option<&CombatTargetView>,
        target_style: AttackStyle,
    ) {
        ChaseToMeleeRangeMovementPolicy.step(mover, pathfinder, map, target, target_style);
    }

    fn holds_when_in_reach(&self) -> bool {
        false
    }
}

/// Holds a preferred Chebyshev distance: backs off when closer, closes in when farther.
#[derive(Debug, Clone, Copy)]
pub struct MaintainDistanceMovementPolicy {
    preferred_distance: i32,
}

impl MaintainDistanceMovementPolicy {
    pub fn new(preferred_distance: i32) -> Self {
        Self { preferred_distance }
    }
}

impl MovementPolicy for MaintainDistanceMovementPolicy {
    fn holds_when_in_reach(&self) -> bool {
        false
    }

    fn step(
        &self,
        mover: &mut Mover,
        pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        target: Option<&CombatTargetView>,
        _target_style: AttackStyle,
    ) {
        let Some(target) = target else {
            mover.step(map, 0);
            return;
        };

        let distance = mover.actor.current_tile().chebyshev_distance_to(target.tile);
        if distance < self.preferred_distance {
            mover.step_away(pathfinder, map, target.tile);
        } else if distance > self.preferred_distance {
            mover.step_towards(pathfinder, map, target.tile);
        } else {
            mover.step(map, 0);
        }
    }
}

/// Chases a melee-fighting target, kites anything ranged or magic.
#[derive(Debug, Clone, Copy)]
pub struct StyleReactiveMovementPolicy {
    chase: ChaseToMeleeRangeMovementPolicy,
    kite: MaintainDistanceMovementPolicy,
}

impl StyleReactiveMovementPolicy {
    pub fn new(kite_distance: i32) -> Self {
        Self { chase: ChaseToMeleeRangeMovementPolicy, kite: MaintainDistanceMovementPolicy::new(kite_distance) }
    }
}

impl MovementPolicy for StyleReactiveMovementPolicy {
    fn holds_when_in_reach(&self) -> bool {
        false
    }

    fn step(
        &self,
        mover: &mut Mover,
        pathfinder: &BfsPathfinder,
        map: &mut CollisionMap,
        target: Option<&CombatTargetView>,
        target_style: AttackStyle,
    ) {
        if target_style == AttackStyle::Melee {
            self.chase.step(mover, pathfinder, map, target, target_style);
        } else {
            self.kite.step(mover, pathfinder, map, target, target_style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::{Actor, ActorKind};
    use crate::collision::test_grid;
    use crate::combat::CombatStats;
    use crate::tiles::TileCoord;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn make_npc_mover(map: &mut CollisionMap, start: TileCoord) -> Mover {
        let actor = Actor::new(ActorKind::Npc, start, 1, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        Mover::new(actor, map)
    }

    fn target_at(x: i32, z: i32) -> CombatTargetView {
        CombatTargetView {
            tile: tc(x, z),
            footprint_size: 1,
            stats: CombatStats::new(50, 50, 50, 50, 50).unwrap(),
            kind: ActorKind::Npc,
        }
    }

    /// Runs `policy` once from (5,5) on an open grid and returns where the mover ended up.
    fn run(policy: &dyn MovementPolicy, target: Option<CombatTargetView>, style: AttackStyle) -> TileCoord {
        let mut map = test_grid::open(20, 20, 100);
        let mut mover = make_npc_mover(&mut map, tc(5, 5));
        policy.step(&mut mover, &BfsPathfinder::new(), &mut map, target.as_ref(), style);
        mover.actor.current_tile()
    }

    #[test]
    fn stationary_never_moves_even_with_a_living_target_far_away() {
        let end = run(&StationaryMovementPolicy, Some(target_at(15, 15)), AttackStyle::Melee);

        assert_eq!(end, tc(5, 5));
    }

    #[test]
    fn chase_to_melee_range_steps_towards_when_not_yet_adjacent() {
        let end = run(&ChaseToMeleeRangeMovementPolicy, Some(target_at(8, 8)), AttackStyle::Melee);

        assert_eq!(end, tc(6, 6));
    }

    #[test]
    fn chase_to_melee_range_holds_still_once_adjacent() {
        let end = run(&ChaseToMeleeRangeMovementPolicy, Some(target_at(6, 5)), AttackStyle::Melee);

        assert_eq!(end, tc(5, 5));
    }

    #[test]
    fn chase_to_melee_range_holds_still_when_there_is_no_living_target() {
        let end = run(&ChaseToMeleeRangeMovementPolicy, None, AttackStyle::Melee);

        assert_eq!(end, tc(5, 5));
    }

    #[test]
    fn maintain_distance_steps_away_when_closer_than_preferred() {
        let end = run(&MaintainDistanceMovementPolicy::new(5), Some(target_at(6, 6)), AttackStyle::Ranged);

        assert_eq!(end, tc(4, 4));
    }

    #[test]
    fn maintain_distance_steps_towards_when_farther_than_preferred() {
        let end = run(&MaintainDistanceMovementPolicy::new(5), Some(target_at(15, 15)), AttackStyle::Ranged);

        assert_eq!(end, tc(6, 6));
    }

    #[test]
    fn maintain_distance_holds_still_at_exactly_the_preferred_distance() {
        let end = run(&MaintainDistanceMovementPolicy::new(5), Some(target_at(10, 5)), AttackStyle::Ranged);

        assert_eq!(end, tc(5, 5));
    }

    #[test]
    fn style_reactive_chases_a_melee_target() {
        let end = run(&StyleReactiveMovementPolicy::new(5), Some(target_at(8, 8)), AttackStyle::Melee);

        assert_eq!(end, tc(6, 6));
    }

    #[test]
    fn style_reactive_kites_a_ranged_or_magic_target() {
        let end = run(&StyleReactiveMovementPolicy::new(5), Some(target_at(6, 6)), AttackStyle::Magic);

        assert_eq!(end, tc(4, 4));
    }
}
