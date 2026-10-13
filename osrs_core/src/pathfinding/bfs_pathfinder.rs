use super::target_spec::{TargetKind, TargetSpec};
use crate::collision::{CollisionMap, OccupancyMask};
use crate::tiles::{Direction, TileCoord, EXPANSION_ORDER};

/// Half-width of the search window. The full window is `WINDOW_SIZE` x `WINDOW_SIZE`.
pub const WINDOW_RADIUS: i32 = 64;
pub const WINDOW_SIZE: i32 = WINDOW_RADIUS * 2;

/// Radius (in tiles) around the DESTINATION that the unreachable fallback scans for a
/// reachable tile to settle for. Modelled on OSRS's alternative-route behaviour; the exact
/// radius is not verified against the live game - treat 10 as a starting point to tune.
const FALLBACK_RADIUS: i32 = 10;

const CELL_COUNT: usize = (WINDOW_SIZE * WINDOW_SIZE) as usize;

/// Zero-allocation-per-query breadth-first pathfinder over a 128x128 window re-centred on the
/// actor for every search. BFS, not A*, is the deliberate choice here - it reproduces OSRS's
/// exact path shapes (unit cost per step, including diagonals), which a Chebyshev-heuristic
/// A* would not. One instance is shared per world; pathfinding happens inside a single tick
/// phase so there is no reentrancy concern.
#[derive(Debug, Clone)]
pub struct BfsPathfinder {
    // Breadcrumb state, reused across queries (never reallocated).
    visit_stamp: Vec<i32>,
    distances: Vec<u16>,
    came_from: Vec<Direction>, // the direction we arrived by
    // Ring buffer queue. Visited is marked ON ENQUEUE, so a cell is enqueued at most once and
    // the buffer never needs to hold more than CELL_COUNT entries - marking on dequeue instead
    // would let a cell be enqueued up to 8 times and silently overrun this.
    queue: Vec<usize>,
    queue_head: usize,
    queue_count: usize,
    // A plain int generation counter: wraparound would need ~2^31 queries in a single
    // process, which is not a realistic concern here.
    generation: i32,
}

impl Default for BfsPathfinder {
    fn default() -> Self {
        Self::new()
    }
}

impl BfsPathfinder {
    pub fn new() -> Self {
        Self {
            visit_stamp: vec![0; CELL_COUNT],
            distances: vec![0; CELL_COUNT],
            came_from: vec![Direction::None; CELL_COUNT],
            queue: vec![0; CELL_COUNT],
            queue_head: 0,
            queue_count: 0,
            generation: 0,
        }
    }

    /// Searches for a path from `start` to `target`, writing it to `out_path` (cleared first;
    /// the start tile itself is not included). Returns false if there is no movement to make.
    pub fn find_path(
        &mut self,
        map: &CollisionMap,
        start: TileCoord,
        target: TargetSpec,
        size: i32,
        respects: OccupancyMask,
        out_path: &mut Vec<TileCoord>,
    ) -> bool {
        out_path.clear();

        if start.plane != target.origin.plane {
            return false; // pathfinder never crosses planes
        }
        if target.is_satisfied_by(start) {
            return true; // already there: success, empty path
        }

        self.generation += 1;

        let origin_x = start.x - WINDOW_RADIUS;
        let origin_z = start.z - WINDOW_RADIUS;
        let plane = start.plane;

        self.reset_queue();

        let start_idx = local_index(WINDOW_RADIUS, WINDOW_RADIUS); // start is always at the window centre
        self.visit_stamp[start_idx] = self.generation;
        self.distances[start_idx] = 0;
        self.came_from[start_idx] = Direction::None;
        self.enqueue(start_idx);

        let mut found_idx: Option<usize> = None;

        while self.queue_count > 0 && found_idx.is_none() {
            let current_idx = self.dequeue();
            let current_local_x = current_idx as i32 % WINDOW_SIZE;
            let current_local_z = current_idx as i32 / WINDOW_SIZE;
            let current_tile = TileCoord::with_plane(origin_x + current_local_x, origin_z + current_local_z, plane);
            let current_dist = self.distances[current_idx];

            for dir in EXPANSION_ORDER {
                if !map.can_traverse(current_tile, dir, size, respects) {
                    continue;
                }

                let next_local_x = current_local_x + dir.delta_x();
                let next_local_z = current_local_z + dir.delta_z();
                if !in_window(next_local_x, next_local_z) {
                    continue; // beyond the window: unreachable by construction
                }

                let next_idx = local_index(next_local_x, next_local_z);
                if self.visit_stamp[next_idx] == self.generation {
                    continue; // already visited this query
                }

                self.visit_stamp[next_idx] = self.generation;
                self.distances[next_idx] = current_dist + 1;
                self.came_from[next_idx] = dir;
                self.enqueue(next_idx);

                let next_tile = TileCoord::with_plane(origin_x + next_local_x, origin_z + next_local_z, plane);
                if target.is_satisfied_by(next_tile) {
                    found_idx = Some(next_idx);
                    break;
                }
            }
        }

        if let Some(idx) = found_idx {
            self.reconstruct_path(origin_x, origin_z, plane, idx, out_path);
            return true;
        }

        // Unreachable fallback is gated to Exact destinations only, as OSRS's useAlternative
        // flag does - Rect/Adjacent/WithinRange targets simply fail with no movement.
        if target.kind == TargetKind::Exact {
            return self.try_fallback(origin_x, origin_z, plane, target.origin, out_path);
        }

        false
    }

    /// Cheap single-step greedy move toward a target tile, used for following a moving target
    /// instead of re-running BFS every tick (that's where OSRS's trailing-behind follow feel
    /// comes from). This is OSRS's "dumb" NPC pathing: try the diagonal toward the target, then
    /// the x axis (east/west) alone, then the z axis (north/south) alone, and never detour.
    ///
    /// One exception makes the corner trap: a mover **diagonally adjacent** to the target (some
    /// tile of its body touches the target only at a corner) tries its east/west step and nothing
    /// else. If that is blocked it stays put, even with the north/south step free, and a melee
    /// monster there never gets a side to attack from.
    ///
    /// The direction is taken from the mover's whole footprint, not its south-west tile: a 5x5 NPC
    /// with the target level with any of its rows moves straight across, not diagonally.
    pub fn try_step_towards(
        &self,
        map: &CollisionMap,
        from: TileCoord,
        target: TileCoord,
        size: i32,
        respects: OccupancyMask,
    ) -> Option<Direction> {
        if from.plane != target.plane {
            return None;
        }

        // which way the target lies from the footprint: 0 when it is level with some row/column
        let toward = |target: i32, low: i32| {
            if target < low {
                -1
            } else if target > low + size - 1 {
                1
            } else {
                0
            }
        };
        let (dx, dz) = (toward(target.x, from.x), toward(target.z, from.z));
        if dx == 0 && dz == 0 {
            return None; // already there (or under the mover)
        }

        // touching only at a corner: the target is one tile off the footprint on both axes
        let touches = |target: i32, low: i32| target == low - 1 || target == low + size;
        if dx != 0 && dz != 0 && touches(target.x, from.x) && touches(target.z, from.z) {
            let east_west = if dx > 0 { Direction::East } else { Direction::West };
            return map.can_traverse(from, east_west, size, respects).then_some(east_west);
        }

        step_greedy(map, from, dx, dz, size, respects)
    }

    /// Mirror of `try_step_towards` for the MaintainDistance policy: one greedy step
    /// directly AWAY from `target` (the towards-direction negated), falling back to each
    /// cardinal component alone if the preferred retreat direction is blocked, same as
    /// `try_step_towards`. Used by kiting NPCs (ranged/magic variants that never close distance).
    pub fn try_step_away(
        &self,
        map: &CollisionMap,
        from: TileCoord,
        target: TileCoord,
        size: i32,
        respects: OccupancyMask,
    ) -> Option<Direction> {
        if from.plane != target.plane {
            return None;
        }

        let dx = (from.x - target.x).signum();
        let dz = (from.z - target.z).signum();
        if dx == 0 && dz == 0 {
            return None; // standing on top of the target - no tile is "away"
        }

        step_greedy(map, from, dx, dz, size, respects)
    }

    fn try_fallback(
        &self,
        origin_x: i32,
        origin_z: i32,
        plane: i32,
        destination: TileCoord,
        out_path: &mut Vec<TileCoord>,
    ) -> bool {
        // Clamp the destination into the search window first. If the real destination is
        // entirely outside the window (a click far beyond pathfinding range), this scans near
        // the window's edge closest to it instead of scanning near an out-of-window point that
        // could never have been reached - this is what produces OSRS's "walk partway, click
        // again" behaviour for distant clicks rather than a flat refusal to move at all.
        let dest_local_x = (destination.x - origin_x).clamp(0, WINDOW_SIZE - 1);
        let dest_local_z = (destination.z - origin_z).clamp(0, WINDOW_SIZE - 1);

        let min_x = (dest_local_x - FALLBACK_RADIUS).max(0);
        let max_x = (dest_local_x + FALLBACK_RADIUS).min(WINDOW_SIZE - 1);
        let min_z = (dest_local_z - FALLBACK_RADIUS).max(0);
        let max_z = (dest_local_z + FALLBACK_RADIUS).min(WINDOW_SIZE - 1);

        let mut best_idx: Option<usize> = None;
        let mut best_sq_dist = i32::MAX;
        let mut best_cost = u16::MAX;

        for lz in min_z..=max_z {
            for lx in min_x..=max_x {
                let idx = local_index(lx, lz);
                if self.visit_stamp[idx] != self.generation {
                    continue; // not reached by this search
                }

                let dx = lx - dest_local_x;
                let dz = lz - dest_local_z;
                let sq_dist = dx * dx + dz * dz;
                let cost = self.distances[idx];

                if sq_dist < best_sq_dist || (sq_dist == best_sq_dist && cost < best_cost) {
                    best_sq_dist = sq_dist;
                    best_cost = cost;
                    best_idx = Some(idx);
                }
            }
        }

        match best_idx {
            None => false, // nothing reachable near the destination: no movement at all
            Some(idx) => {
                self.reconstruct_path(origin_x, origin_z, plane, idx, out_path);
                true
            }
        }
    }

    fn reconstruct_path(&self, origin_x: i32, origin_z: i32, plane: i32, target_idx: usize, out_path: &mut Vec<TileCoord>) {
        out_path.clear();

        let mut idx = target_idx;
        loop {
            let dir = self.came_from[idx];
            if dir == Direction::None {
                break; // reached the start cell
            }

            let local_x = idx as i32 % WINDOW_SIZE;
            let local_z = idx as i32 / WINDOW_SIZE;
            out_path.push(TileCoord::with_plane(origin_x + local_x, origin_z + local_z, plane));

            idx = local_index(local_x - dir.delta_x(), local_z - dir.delta_z());
        }

        // Built target-to-start by walking the breadcrumbs back; the C# inserts at the front.
        out_path.reverse();
    }

    fn reset_queue(&mut self) {
        self.queue_head = 0;
        self.queue_count = 0;
    }

    fn enqueue(&mut self, idx: usize) {
        let write_pos = (self.queue_head + self.queue_count) % CELL_COUNT;
        self.queue[write_pos] = idx;
        self.queue_count += 1;
    }

    fn dequeue(&mut self) -> usize {
        let value = self.queue[self.queue_head];
        self.queue_head = (self.queue_head + 1) % CELL_COUNT;
        self.queue_count -= 1;
        value
    }
}

/// Shared body of `try_step_towards` / `try_step_away`: try the (possibly diagonal) preferred
/// direction first, then each cardinal component alone.
fn step_greedy(map: &CollisionMap, from: TileCoord, dx: i32, dz: i32, size: i32, respects: OccupancyMask) -> Option<Direction> {
    let preferred = direction_from_sign(dx, dz);
    if preferred != Direction::None && map.can_traverse(from, preferred, size, respects) {
        return Some(preferred);
    }

    if dx != 0 {
        let cardinal_x = if dx > 0 { Direction::East } else { Direction::West };
        if cardinal_x != preferred && map.can_traverse(from, cardinal_x, size, respects) {
            return Some(cardinal_x);
        }
    }

    if dz != 0 {
        let cardinal_z = if dz > 0 { Direction::North } else { Direction::South };
        if cardinal_z != preferred && map.can_traverse(from, cardinal_z, size, respects) {
            return Some(cardinal_z);
        }
    }

    None
}

fn direction_from_sign(dx: i32, dz: i32) -> Direction {
    match (dx.signum(), dz.signum()) {
        (0, 0) => Direction::None,
        (-1, 0) => Direction::West,
        (1, 0) => Direction::East,
        (0, -1) => Direction::South,
        (0, 1) => Direction::North,
        (-1, -1) => Direction::SouthWest,
        (1, -1) => Direction::SouthEast,
        (-1, 1) => Direction::NorthWest,
        _ => Direction::NorthEast,
    }
}

fn local_index(local_x: i32, local_z: i32) -> usize {
    (local_z * WINDOW_SIZE + local_x) as usize
}

fn in_window(local_x: i32, local_z: i32) -> bool {
    local_x >= 0 && local_x < WINDOW_SIZE && local_z >= 0 && local_z < WINDOW_SIZE
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::{test_grid, CollisionFlag};

    const ALL: OccupancyMask = OccupancyMask::ALL;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn find(map: &CollisionMap, start: TileCoord, target: TargetSpec, path: &mut Vec<TileCoord>) -> bool {
        BfsPathfinder::new().find_path(map, start, target, 1, ALL, path)
    }

    fn seal_room(map: &mut CollisionMap, min_x: i32, min_z: i32, max_x: i32, max_z: i32) {
        let (ring_min_x, ring_max_x) = (min_x - 1, max_x + 1);
        let (ring_min_z, ring_max_z) = (min_z - 1, max_z + 1);

        for x in ring_min_x..=ring_max_x {
            map.add_flags(tc(x, ring_min_z), CollisionFlag::BLOCK_OBJECT);
            map.add_flags(tc(x, ring_max_z), CollisionFlag::BLOCK_OBJECT);
        }
        for z in ring_min_z..=ring_max_z {
            map.add_flags(tc(ring_min_x, z), CollisionFlag::BLOCK_OBJECT);
            map.add_flags(tc(ring_max_x, z), CollisionFlag::BLOCK_OBJECT);
        }
    }

    #[test]
    fn open_grid_shortest_path_uses_diagonal_shortcut() {
        let map = test_grid::open(20, 20, 100);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), TargetSpec::exact(tc(5, 5)), &mut path);

        assert!(found);
        // Diagonal steps cost the same as cardinal ones, so the shortest path to a point 5
        // tiles away on both axes is 5 pure-diagonal steps, not 10.
        assert_eq!(path.len(), 5);
        assert_eq!(path[path.len() - 1], tc(5, 5));
    }

    #[test]
    fn expansion_order_determines_path_shape_when_diagonal_shortcut_is_blocked() {
        let mut map = test_grid::open(5, 5, 100);
        // Block the direct NE diagonal from the start, forcing a 2-step cardinal detour with
        // two equal-length options: East-then-North, or North-then-East.
        map.add_flags(tc(0, 0), CollisionFlag::BLOCK_NORTH_EAST);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), TargetSpec::exact(tc(1, 1)), &mut path);

        assert!(found);
        assert_eq!(path.len(), 2);
        // EXPANSION_ORDER puts East before North, so East-then-North is the deterministic shape.
        assert_eq!(path[0], tc(1, 0));
        assert_eq!(path[1], tc(1, 1));
    }

    #[test]
    fn path_routes_around_a_wall_rather_than_through_it() {
        let map = test_grid::parse(".....\n.###.\n.....", 100);
        let mut path = Vec::new();

        let found = find(&map, tc(2, 0), TargetSpec::exact(tc(2, 2)), &mut path);

        assert!(found);
        for tile in &path {
            let is_wall_tile = tile.z == 1 && tile.x >= 1 && tile.x <= 3;
            assert!(!is_wall_tile, "Path passed through wall tile {tile}");
        }
    }

    #[test]
    fn rect_target_stops_on_nearest_corner_of_the_footprint() {
        let map = test_grid::open(20, 20, 100);
        let target = TargetSpec::rect(tc(5, 5), 2, 2);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), target, &mut path);

        assert!(found);
        assert_eq!(path.len(), 5); // diagonal straight to the near corner (5,5)
        assert!(target.is_satisfied_by(path[path.len() - 1]));
    }

    #[test]
    fn adjacent_target_stops_beside_the_footprint_never_inside_it() {
        let map = test_grid::open(20, 20, 100);
        let target = TargetSpec::adjacent(tc(5, 5), 2, 2);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), target, &mut path);

        assert!(found);
        let last = path[path.len() - 1];
        assert!(target.is_satisfied_by(last));
        assert!(!(last.x >= 5 && last.x < 7 && last.z >= 5 && last.z < 7), "Adjacent target must not stand on the footprint itself");
    }

    #[test]
    fn within_range_target_stops_as_soon_as_in_range() {
        let map = test_grid::open(20, 5, 100);
        let target = TargetSpec::within_range_of(tc(10, 0), 3);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), target, &mut path);

        assert!(found);
        assert_eq!(path.len(), 7); // walks to x=7, which is exactly 3 away from x=10
        assert_eq!(path[path.len() - 1], tc(7, 0));
    }

    #[test]
    fn destination_beyond_window_walks_to_the_edge_of_reach_click_again_would_continue() {
        let map = test_grid::open(200, 10, 100);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), TargetSpec::exact(tc(500, 0)), &mut path);

        assert!(found);
        // The window radius is 64, so the farthest reachable tile in that direction is +63.
        assert_eq!(path[path.len() - 1], tc(63, 0));
    }

    #[test]
    fn small_sealed_room_falls_back_to_the_nearest_reachable_tile() {
        let mut map = test_grid::open(60, 60, 100);
        seal_room(&mut map, 10, 10, 10, 10);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), TargetSpec::exact(tc(10, 10)), &mut path);

        assert!(found, "A small sealed room should still produce a partial path up to its wall.");
        assert!(!path.is_empty());
        let last = path[path.len() - 1];
        assert!(last.squared_distance_to(tc(10, 10)) <= 10 * 10);
    }

    #[test]
    fn large_sealed_room_nothing_reachable_near_the_destination_produces_no_movement() {
        let mut map = test_grid::open(140, 140, 100);
        // Interior radius 11 from the room's centre to its nearest wall - bigger than the
        // fallback's +/-10 scan, so nothing within that scan of the target is ever reached.
        seal_room(&mut map, 44, 44, 66, 66);
        let mut path = Vec::new();

        let found = find(&map, tc(0, 0), TargetSpec::exact(tc(55, 55)), &mut path);

        assert!(!found, "Nothing within the fallback window of a large sealed room's centre is reachable - the character should not move at all.");
        assert!(path.is_empty());
    }

    #[test]
    fn rect_and_adjacent_targets_have_no_unreachable_fallback() {
        let mut map = test_grid::open(60, 60, 100);
        seal_room(&mut map, 10, 10, 10, 10);
        let mut path = Vec::new();

        let found_rect = find(&map, tc(0, 0), TargetSpec::rect(tc(10, 10), 1, 1), &mut path);
        assert!(!found_rect, "Rect targets must not use the Exact-only unreachable fallback.");

        let found_adjacent = find(&map, tc(0, 0), TargetSpec::adjacent(tc(10, 10), 1, 1), &mut path);
        // Adjacent to the sealed single tile means the ring tiles themselves - which are
        // full-blocked and therefore never reachable either.
        assert!(!found_adjacent, "Adjacent targets must not use the Exact-only unreachable fallback.");
    }

    #[test]
    fn size_3_mover_cannot_pass_a_2_wide_gap_while_size_1_can() {
        let map = test_grid::parse("............\n#####..#####\n............", 100);
        let south = tc(5, 0);

        assert!(map.can_traverse(south, Direction::North, 1, ALL));
        assert!(!map.can_traverse(south, Direction::North, 3, ALL));
    }

    #[test]
    fn step_towards_prefers_diagonal_falls_back_to_cardinal_when_blocked() {
        let pathfinder = BfsPathfinder::new();
        let open_map = test_grid::open(10, 10, 100);

        let dir = pathfinder.try_step_towards(&open_map, tc(0, 0), tc(3, 3), 1, ALL);
        assert_eq!(dir, Some(Direction::NorthEast));

        let mut blocked_map = test_grid::open(10, 10, 100);
        blocked_map.add_flags(tc(0, 0), CollisionFlag::BLOCK_NORTH_EAST);
        let fallback = pathfinder.try_step_towards(&blocked_map, tc(0, 0), tc(3, 3), 1, ALL);
        assert!(fallback == Some(Direction::East) || fallback == Some(Direction::North));
    }

    #[test]
    fn step_away_prefers_the_direct_opposite_diagonal() {
        let pathfinder = BfsPathfinder::new();
        let open_map = test_grid::open(10, 10, 100);

        let dir = pathfinder.try_step_away(&open_map, tc(5, 5), tc(3, 3), 1, ALL);

        assert_eq!(dir, Some(Direction::NorthEast)); // retreating from the SW, so away is NE
    }

    #[test]
    fn step_away_falls_back_to_a_cardinal_when_the_direct_retreat_is_blocked() {
        let pathfinder = BfsPathfinder::new();
        let mut blocked_map = test_grid::open(10, 10, 100);
        blocked_map.add_flags(tc(5, 5), CollisionFlag::BLOCK_NORTH_EAST);

        let dir = pathfinder.try_step_away(&blocked_map, tc(5, 5), tc(3, 3), 1, ALL);

        assert!(dir == Some(Direction::East) || dir == Some(Direction::North));
    }

    #[test]
    fn step_away_returns_none_when_already_standing_on_the_target() {
        let pathfinder = BfsPathfinder::new();
        let open_map = test_grid::open(10, 10, 100);

        let dir = pathfinder.try_step_away(&open_map, tc(5, 5), tc(5, 5), 1, ALL);

        assert_eq!(dir, None);
    }

    #[test]
    fn already_at_the_target_succeeds_with_an_empty_path() {
        let map = test_grid::open(5, 5, 100);
        let mut path = vec![tc(9, 9)];

        let found = find(&map, tc(2, 2), TargetSpec::exact(tc(2, 2)), &mut path);

        assert!(found);
        assert!(path.is_empty());
    }

    #[test]
    fn the_pathfinder_can_be_reused_across_queries() {
        let map = test_grid::open(20, 20, 100);
        let mut pathfinder = BfsPathfinder::new();
        let mut path = Vec::new();

        for _ in 0..3 {
            assert!(pathfinder.find_path(&map, tc(0, 0), TargetSpec::exact(tc(5, 5)), 1, ALL, &mut path));
            assert_eq!(path.len(), 5);
        }
    }
}
