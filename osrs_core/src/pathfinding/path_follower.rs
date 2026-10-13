use super::bfs_pathfinder::BfsPathfinder;
use super::target_spec::TargetSpec;
use crate::collision::{CollisionMap, OccupancyMask};
use crate::tiles::TileCoord;

/// Holds and consumes a single BFS path. Separate from `Mover` so "what path am I on" and "how
/// do I apply steps to an Actor's tile/occupancy" aren't tangled together - this type knows
/// nothing about Actor, CollisionMap occupancy writes, or occupancy.
#[derive(Debug, Clone, Default)]
pub struct PathFollower {
    path: Vec<TileCoord>,
    path_index: usize,
    /// The tiles the most recent `advance` stepped through, in order.
    consumed: Vec<TileCoord>,
}

impl PathFollower {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn has_remaining_steps(&self) -> bool {
        self.path_index < self.path.len()
    }

    /// How many tiles of the active path are still to be walked.
    pub fn remaining_steps(&self) -> usize {
        self.path.len().saturating_sub(self.path_index)
    }

    /// Drops the rest of the active path.
    pub fn clear(&mut self) {
        self.path_index = self.path.len();
    }

    /// The tile the active path currently ends at, if any steps remain unconsumed.
    pub fn destination(&self) -> Option<TileCoord> {
        if self.path_index < self.path.len() {
            self.path.last().copied()
        } else {
            None
        }
    }

    /// Runs a fresh BFS search and replaces the active path. Returns the pathfinder's
    /// own success/fail result (false means no movement at all, not even a fallback).
    pub fn repath(
        &mut self,
        pathfinder: &mut BfsPathfinder,
        map: &CollisionMap,
        start: TileCoord,
        target: TargetSpec,
        footprint_size: i32,
        respects: OccupancyMask,
    ) -> bool {
        let found = pathfinder.find_path(map, start, target, footprint_size, respects, &mut self.path);
        self.path_index = 0;
        found
    }

    /// The tile `advance(steps)` would end on (the path's last tile if it is shorter), or `None`
    /// if it would not move.
    pub fn tile_after(&self, steps: i32) -> Option<TileCoord> {
        if steps <= 0 || self.path_index >= self.path.len() {
            return None;
        }
        let index = (self.path_index + steps as usize - 1).min(self.path.len() - 1);
        Some(self.path[index])
    }

    /// The tiles the last `advance` stepped through (empty if it consumed none).
    pub fn last_consumed(&self) -> &[TileCoord] {
        &self.consumed
    }

    /// Records a step taken outside the path (a greedy one) as this tick's, or none.
    pub fn note_step(&mut self, tile: Option<TileCoord>) {
        self.consumed.clear();
        self.consumed.extend(tile);
    }

    /// Consumes up to `steps` tiles. Returns the number of tiles actually consumed (0 if idle or
    /// the path is exhausted) and the tile reached (`TileCoord::default()` if none consumed).
    pub fn advance(&mut self, steps: i32) -> (i32, TileCoord) {
        let mut final_tile = TileCoord::default();
        let mut consumed = 0;
        self.consumed.clear();

        let mut i = 0;
        while i < steps && self.path_index < self.path.len() {
            final_tile = self.path[self.path_index];
            self.consumed.push(final_tile);
            self.path_index += 1;
            consumed += 1;
            i += 1;
        }

        (consumed, final_tile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::test_grid;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn follower_towards(x: i32) -> PathFollower {
        let map = test_grid::open(20, 3, 100);
        let mut pathfinder = BfsPathfinder::new();
        let mut follower = PathFollower::new();
        assert!(follower.repath(&mut pathfinder, &map, tc(0, 0), TargetSpec::exact(tc(x, 0)), 1, OccupancyMask::ALL));
        follower
    }

    #[test]
    fn new_follower_is_idle() {
        let mut follower = PathFollower::new();

        assert!(!follower.has_remaining_steps());
        assert_eq!(follower.destination(), None);
        assert_eq!(follower.advance(2).0, 0);
    }

    #[test]
    fn advance_consumes_up_to_the_requested_steps_and_reports_the_tile_reached() {
        let mut follower = follower_towards(5);
        assert_eq!(follower.destination(), Some(tc(5, 0)));

        assert_eq!(follower.advance(2), (2, tc(2, 0)));
        assert!(follower.has_remaining_steps());
        assert_eq!(follower.advance(2), (2, tc(4, 0)));
        // Only one step is left, so a request for two consumes one.
        assert_eq!(follower.advance(2), (1, tc(5, 0)));
        assert!(!follower.has_remaining_steps());
        assert_eq!(follower.destination(), None);
        assert_eq!(follower.advance(2).0, 0);
    }

    #[test]
    fn repath_replaces_the_active_path() {
        let map = test_grid::open(20, 3, 100);
        let mut pathfinder = BfsPathfinder::new();
        let mut follower = follower_towards(5);
        follower.advance(2);

        follower.repath(&mut pathfinder, &map, tc(2, 0), TargetSpec::exact(tc(8, 0)), 1, OccupancyMask::ALL);

        assert_eq!(follower.destination(), Some(tc(8, 0)));
        assert_eq!(follower.advance(1), (1, tc(3, 0)));
    }
}
