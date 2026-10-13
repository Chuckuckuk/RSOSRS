use super::actor::Actor;
use crate::collision::CollisionMap;
use crate::pathfinding::{BfsPathfinder, PathFollower, TargetSpec};
use crate::tiles::{Direction, TileCoord};

/// Walks an `Actor` along a path and keeps the collision map's occupancy overlay in step with
/// it. Unlike the C# `Mover`, it does not hold the `CollisionMap` - every method that touches
/// the map takes it as an argument, so the world stays the single owner (see `PARITY_PLAN.md`).
///
/// The actor occupies its whole footprint (a 5x5 Jad blocks 25 tiles). While it works out where
/// it may go, it lifts its own mark, so it never counts as standing in its own way.
#[derive(Debug, Clone)]
pub struct Mover {
    pub actor: Actor,
    path_follower: PathFollower,
    /// Whether its mark is on the map (false once it has left it: dead and lying there).
    on_map: bool,
}

impl Mover {
    /// Marks the actor's footprint occupied in `map`.
    pub fn new(actor: Actor, map: &mut CollisionMap) -> Self {
        let mover = Self { actor, path_follower: PathFollower::new(), on_map: true };
        mover.mark(map, mover.actor.current_tile(), true);
        mover
    }

    /// Adds (or removes) the actor's mark over the footprint whose south-west tile is `at`.
    fn mark(&self, map: &mut CollisionMap, at: TileCoord, occupied: bool) {
        if !self.on_map {
            return;
        }
        let size = self.actor.footprint_size();
        let who = self.actor.occupies_as();
        for dz in 0..size {
            for dx in 0..size {
                map.set_occupied(TileCoord::with_plane(at.x + dx, at.z + dz, at.plane), who, occupied);
            }
        }
    }

    /// For an NPC: makes it stand in players' way too, or stop doing so (it always blocks NPCs).
    pub fn set_blocks_players(&mut self, map: &mut CollisionMap, blocks_players: bool) {
        if self.actor.blocks_players() == blocks_players {
            return;
        }
        let tile = self.actor.current_tile();
        // clear with the mask it had, then mark with the new one
        self.mark(map, tile, false);
        self.actor.set_blocks_players_flag(blocks_players);
        self.mark(map, tile, true);
    }

    /// Takes the actor's mark off the map for good: it no longer blocks anyone (a body).
    pub fn leave_map(&mut self, map: &mut CollisionMap) {
        self.mark(map, self.actor.current_tile(), false);
        self.on_map = false;
    }

    pub fn destination(&self) -> Option<TileCoord> {
        self.path_follower.destination()
    }

    /// The tile `step(steps)` would leave the actor on, without stepping.
    pub fn tile_after_steps(&self, steps: i32) -> TileCoord {
        self.path_follower.tile_after(steps).unwrap_or_else(|| self.actor.current_tile())
    }

    /// The tiles stepped through by the most recent `step` (a run covers two), in order.
    pub fn last_step_tiles(&self) -> &[TileCoord] {
        self.path_follower.last_consumed()
    }

    pub fn request_move(&mut self, pathfinder: &mut BfsPathfinder, map: &mut CollisionMap, target: TargetSpec) -> bool {
        let here = self.actor.current_tile();
        self.mark(map, here, false);
        let found = self.path_follower.repath(pathfinder, map, here, target, self.actor.footprint_size(), self.actor.respects());
        self.mark(map, here, true);
        found
    }

    /// Consumes up to `steps_per_tick` tiles of the active path (0 = hold still this tick). Each
    /// step is checked against the map as it is now, since the path was planned: one that has
    /// become blocked (another NPC stepped onto it) stops the actor short, and the rest of the path
    /// is dropped so that the next request plans around it.
    pub fn step(&mut self, map: &mut CollisionMap, steps_per_tick: i32) {
        let before = self.actor.current_tile();
        self.mark(map, before, false);
        let (size, respects) = (self.actor.footprint_size(), self.actor.respects());
        let wanted = steps_per_tick.min(self.path_follower.remaining_steps() as i32);
        let mut allowed = 0;
        let mut at = before;
        while allowed < wanted {
            let Some(next) = self.path_follower.tile_after(allowed + 1) else { break };
            let direction = Direction::from_delta(next.x - at.x, next.z - at.z);
            if !map.can_traverse(at, direction, size, respects) {
                break;
            }
            at = next;
            allowed += 1;
        }
        let blocked = allowed < wanted;

        let (consumed, final_tile) = self.path_follower.advance(allowed);
        let end = if consumed == 0 { before } else { final_tile };
        self.actor.advance_tile(end);
        self.mark(map, end, true);
        if blocked {
            self.path_follower.clear();
        }
    }

    /// One greedy step toward `target`. Returns false (and holds still) if no step is legal.
    pub fn step_towards(&mut self, pathfinder: &BfsPathfinder, map: &mut CollisionMap, target: TileCoord) -> bool {
        let here = self.actor.current_tile();
        self.mark(map, here, false);
        let direction = pathfinder.try_step_towards(map, here, target, self.actor.footprint_size(), self.actor.respects());
        self.apply_greedy_step(map, direction)
    }

    /// One greedy step directly away from `target`. Returns false (and holds still) if no step is legal.
    pub fn step_away(&mut self, pathfinder: &BfsPathfinder, map: &mut CollisionMap, target: TileCoord) -> bool {
        let here = self.actor.current_tile();
        self.mark(map, here, false);
        let direction = pathfinder.try_step_away(map, here, target, self.actor.footprint_size(), self.actor.respects());
        self.apply_greedy_step(map, direction)
    }

    /// Takes a greedy step (or holds still) and puts the actor's mark, lifted by the caller, back
    /// where it now stands.
    fn apply_greedy_step(&mut self, map: &mut CollisionMap, direction: Option<Direction>) -> bool {
        let before = self.actor.current_tile();
        let next = match direction {
            Some(dir) => before + dir,
            None => before,
        };
        self.actor.advance_tile(next);
        // a greedy step is a step like any other: it is what `last_step_tiles` reports
        self.path_follower.note_step(direction.map(|_| next));
        self.mark(map, next, true);
        direction.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actors::ActorKind;
    use crate::collision::{test_grid, CollisionFlag, OccupancyMask};
    use crate::combat::CombatStats;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn make_npc_mover(map: &mut CollisionMap, start: TileCoord) -> Mover {
        let actor = Actor::new(ActorKind::Npc, start, 1, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        Mover::new(actor, map)
    }

    #[test]
    fn construction_marks_the_start_tile_occupied() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);

        make_npc_mover(&mut map, start);

        assert!(map.is_occupied(start, OccupancyMask::NPC));
    }

    #[test]
    fn step_with_no_active_path_keeps_actor_in_place_and_collapses_interpolation() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let mut mover = make_npc_mover(&mut map, start);

        mover.step(&mut map, 1);

        assert_eq!(mover.actor.current_tile(), start);
        assert_eq!(mover.actor.previous_tile(), start);
    }

    #[test]
    fn request_move_then_step_advances_one_tile_per_step_on_an_open_grid() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let destination = tc(8, 5);
        let mut mover = make_npc_mover(&mut map, start);
        let mut pathfinder = BfsPathfinder::new();

        let found = mover.request_move(&mut pathfinder, &mut map, TargetSpec::exact(destination));
        assert!(found);

        mover.step(&mut map, 1);
        assert_eq!(mover.actor.current_tile(), tc(6, 5));
        assert_eq!(mover.actor.previous_tile(), start);

        mover.step(&mut map, 1);
        assert_eq!(mover.actor.current_tile(), tc(7, 5));

        mover.step(&mut map, 1);
        assert_eq!(mover.actor.current_tile(), destination);

        // Path exhausted: further steps keep the actor at the destination with no further
        // "movement" for the view to interpolate.
        mover.step(&mut map, 1);
        assert_eq!(mover.actor.current_tile(), destination);
        assert_eq!(mover.actor.previous_tile(), destination);
    }

    #[test]
    fn step_with_multiple_steps_per_tick_consumes_that_many_tiles_in_one_tick() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let destination = tc(8, 5);
        let mut mover = make_npc_mover(&mut map, start);
        let mut pathfinder = BfsPathfinder::new();
        mover.request_move(&mut pathfinder, &mut map, TargetSpec::exact(destination));

        // "Run": 2 tiles in one step call. previous_tile must be the pre-tick tile, not an
        // intermediate tile from mid-step.
        mover.step(&mut map, 2);
        assert_eq!(mover.actor.current_tile(), tc(7, 5));
        assert_eq!(mover.actor.previous_tile(), start);

        mover.step(&mut map, 2); // only 1 tile remains - must not overshoot
        assert_eq!(mover.actor.current_tile(), destination);
    }

    #[test]
    fn step_updates_occupancy_clearing_the_old_tile_and_marking_the_new() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let destination = tc(6, 5);
        let mut mover = make_npc_mover(&mut map, start);
        let mut pathfinder = BfsPathfinder::new();
        mover.request_move(&mut pathfinder, &mut map, TargetSpec::exact(destination));

        mover.step(&mut map, 1);

        assert!(!map.is_occupied(start, OccupancyMask::NPC));
        assert!(map.is_occupied(destination, OccupancyMask::NPC));
    }

    #[test]
    fn step_towards_takes_one_greedy_step_and_updates_occupancy() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let target = tc(8, 8); // due north-east: diagonal is unobstructed here
        let mut mover = make_npc_mover(&mut map, start);
        let pathfinder = BfsPathfinder::new();

        let moved = mover.step_towards(&pathfinder, &mut map, target);

        assert!(moved);
        assert_eq!(mover.actor.current_tile(), tc(6, 6));
        assert_eq!(mover.actor.previous_tile(), start);
        assert!(!map.is_occupied(start, OccupancyMask::NPC));
        assert!(map.is_occupied(tc(6, 6), OccupancyMask::NPC));
    }

    #[test]
    fn step_towards_corner_trap_stalls_in_place_when_diagonal_and_both_cardinals_are_blocked() {
        // A wall on the NPC's own north and east edges: the no-corner-cutting rule denies the
        // diagonal across that corner, and both cardinal fallbacks are individually blocked by
        // the same walls. Real OSRS NPCs have no route-search fallback here - they simply do
        // not move, tick after tick, for as long as the player holds the corner. This is the
        // actual mechanism behind "corner trap" / pillar safespots, not a bug.
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let target = tc(8, 8);
        map.add_flags(start, CollisionFlag::BLOCK_NORTH | CollisionFlag::BLOCK_EAST);
        let mut mover = make_npc_mover(&mut map, start);
        let pathfinder = BfsPathfinder::new();

        let moved = mover.step_towards(&pathfinder, &mut map, target);

        assert!(!moved);
        assert_eq!(mover.actor.current_tile(), start);
        assert_eq!(mover.actor.previous_tile(), start);
        assert!(map.is_occupied(start, OccupancyMask::NPC)); // never released
    }

    #[test]
    fn step_towards_falls_back_to_a_cardinal_when_only_the_diagonal_is_blocked() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let target = tc(8, 8);
        map.add_flags(start, CollisionFlag::BLOCK_NORTH_EAST);
        let mut mover = make_npc_mover(&mut map, start);
        let pathfinder = BfsPathfinder::new();

        let moved = mover.step_towards(&pathfinder, &mut map, target);

        assert!(moved);
        let reached = mover.actor.current_tile();
        assert!(reached == tc(6, 5) || reached == tc(5, 6));
    }

    #[test]
    fn step_away_takes_one_greedy_step_in_the_opposite_direction_and_updates_occupancy() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let target = tc(3, 3); // due south-west: retreating north-east is unobstructed
        let mut mover = make_npc_mover(&mut map, start);
        let pathfinder = BfsPathfinder::new();

        let moved = mover.step_away(&pathfinder, &mut map, target);

        assert!(moved);
        assert_eq!(mover.actor.current_tile(), tc(6, 6));
        assert_eq!(mover.actor.previous_tile(), start);
        assert!(!map.is_occupied(start, OccupancyMask::NPC));
        assert!(map.is_occupied(tc(6, 6), OccupancyMask::NPC));
    }

    #[test]
    fn step_away_stays_in_place_when_already_standing_on_the_target() {
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let mut mover = make_npc_mover(&mut map, start);
        let pathfinder = BfsPathfinder::new();

        let moved = mover.step_away(&pathfinder, &mut map, start);

        assert!(!moved);
        assert_eq!(mover.actor.current_tile(), start);
    }

    #[test]
    fn a_multi_tile_mover_occupies_its_whole_footprint_and_never_blocks_itself() {
        // The C# Mover marked only the south-west tile, and so a 2x2+ mover stepping south or west
        // was refused by its own marker. Here it marks its whole footprint and lifts its own mark
        // while it works out a step, so it can go every way.
        let mut map = test_grid::open(20, 20, 100);
        let start = tc(5, 5);
        let actor = Actor::new(ActorKind::Npc, start, 3, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut mover = Mover::new(actor, &mut map);
        let pathfinder = BfsPathfinder::new();

        for x in 5..8 {
            for z in 5..8 {
                assert!(map.is_occupied(tc(x, z), OccupancyMask::NPC), "({x}, {z}) is part of the footprint");
            }
        }
        assert!(mover.step_towards(&pathfinder, &mut map, tc(5, 0)), "south");
        assert!(mover.step_towards(&pathfinder, &mut map, tc(0, 4)), "west");
        assert!(mover.step_towards(&pathfinder, &mut map, tc(4, 15)), "north");
        assert_eq!(mover.actor.current_tile(), tc(4, 5));
        assert!(!map.is_occupied(tc(7, 7), OccupancyMask::NPC), "the tiles it left are free again");
        assert!(map.is_occupied(tc(6, 7), OccupancyMask::NPC));
    }

    #[test]
    fn a_diagonally_adjacent_npc_steps_east_or_west_to_reach_the_targets_side() {
        let mut map = test_grid::open(10, 10, 100);
        let actor = Actor::new(ActorKind::Npc, tc(1, 1), 1, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut mover = Mover::new(actor, &mut map);

        assert!(mover.step_towards(&BfsPathfinder::new(), &mut map, tc(0, 0)));

        assert_eq!(mover.actor.current_tile(), tc(0, 1), "west, never south, when both are open");
    }

    #[test]
    fn a_diagonally_adjacent_npc_never_tries_north_or_south_when_east_west_is_blocked() {
        // any tile of the body counts: this 2x2 touches the target at (0, 0) only by its corner
        // tile (1, 1); its west step is blocked by the block at (0, 2), and though south is open
        // it stays where it is
        let mut map = test_grid::parse(
            "\
.....
#....
.....
.....",
            100,
        );
        let actor = Actor::new(ActorKind::Npc, tc(1, 1), 2, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut mover = Mover::new(actor, &mut map);
        let pathfinder = BfsPathfinder::new();
        assert!(map.can_traverse(tc(1, 1), Direction::South, 2, OccupancyMask::NONE), "south would be a legal step");

        assert!(!mover.step_towards(&pathfinder, &mut map, tc(0, 0)));

        assert_eq!(mover.actor.current_tile(), tc(1, 1));
    }

    #[test]
    fn a_large_npc_heads_straight_for_a_target_level_with_its_side() {
        // the direction comes from the footprint: a target east of it and level with one of its
        // rows is due east, though it is north-east of the south-west tile
        let mut map = test_grid::open(20, 20, 100);
        let actor = Actor::new(ActorKind::Npc, tc(5, 5), 3, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut mover = Mover::new(actor, &mut map);

        assert!(mover.step_towards(&BfsPathfinder::new(), &mut map, tc(12, 7)));

        assert_eq!(mover.actor.current_tile(), tc(6, 5));
    }

    #[test]
    fn a_large_npc_is_snagged_on_a_corner_it_could_easily_walk_round() {
        // "Larger monsters ... may also end up snagged on a corner": the target is due west, a
        // block sits in line with the 2x2 NPC's upper row, and greedy stepping never sidesteps.
        let mut map = test_grid::parse(
            "\
........
...#....
........",
            100,
        );
        let actor = Actor::new(ActorKind::Npc, tc(4, 0), 2, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut mover = Mover::new(actor, &mut map);

        assert!(!mover.step_towards(&BfsPathfinder::new(), &mut map, tc(0, 0)));
        assert_eq!(mover.actor.current_tile(), tc(4, 0));
    }

    #[test]
    fn npcs_cannot_walk_through_one_another() {
        let mut map = test_grid::open(20, 20, 100);
        let npc = |x| Actor::new(ActorKind::Npc, tc(x, 5), 1, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let _wall = Mover::new(npc(6), &mut map);
        let mut walker = Mover::new(npc(5), &mut map);
        let pathfinder = BfsPathfinder::new();

        // straight east is the other NPC: the greedy step goes nowhere
        assert!(!walker.step_towards(&pathfinder, &mut map, tc(10, 5)));
        assert_eq!(walker.actor.current_tile(), tc(5, 5));
    }

    #[test]
    fn a_path_that_another_npc_steps_onto_stops_short_and_is_dropped() {
        let mut map = test_grid::open(20, 20, 100);
        let npc = |x, z| Actor::new(ActorKind::Npc, tc(x, z), 1, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut walker = Mover::new(npc(2, 5), &mut map);
        let mut pathfinder = BfsPathfinder::new();
        assert!(walker.request_move(&mut pathfinder, &mut map, TargetSpec::exact(tc(8, 5))));

        // after the path was planned, something arrives two tiles ahead
        let _blocker = Mover::new(npc(4, 5), &mut map);
        walker.step(&mut map, 2);

        assert_eq!(walker.actor.current_tile(), tc(3, 5), "it walks up to the newcomer, not into it");
        assert!(walker.destination().is_none(), "and plans again next time");
    }

    #[test]
    fn a_body_that_has_left_the_map_blocks_nobody() {
        let mut map = test_grid::open(20, 20, 100);
        let actor = Actor::new(ActorKind::Npc, tc(5, 5), 2, 10, CombatStats::new(1, 1, 1, 1, 1).unwrap());
        let mut body = Mover::new(actor, &mut map);

        body.leave_map(&mut map);
        body.leave_map(&mut map); // twice is harmless

        assert!(!map.is_occupied(tc(5, 5), OccupancyMask::ALL));
        assert!(!map.is_occupied(tc(6, 6), OccupancyMask::ALL));
    }
}
