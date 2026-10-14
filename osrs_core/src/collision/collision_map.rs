use super::collision_flag::CollisionFlag;
use super::collision_map_data::{CollisionMapData, CollisionMapDataError};
use super::occupancy_mask::OccupancyMask;
use crate::tiles::{Direction, TileCoord};

const FULL_BLOCK_MASK: u32 = CollisionFlag::BLOCK_FULL;

/// A baked collision map plus runtime occupancy overlays. Owns the rules that define OSRS
/// movement feel: directional, two-sided walls; no corner-cutting on diagonals; per-edge step
/// height; and separate NPC/player occupancy (players never block other players).
///
/// `flags_at` is the hottest path in the project (called up to 8x per BFS node expansion) and
/// must never panic - out-of-bounds reads resolve to `BLOCK_FULL`.
#[derive(Debug, Clone)]
pub struct CollisionMap {
    origin_x: i32,
    origin_z: i32,
    width: i32,
    height: i32,
    max_step_height: i32,
    base_flags: Vec<u32>,
    heights: Vec<i16>,
    // Runtime overlays. Separate from base_flags so they can be rewritten every tick without
    // touching (or re-baking) the static map; separate from each other because players and
    // NPCs block differently (see `OccupancyMask`). Each counts the movers on a tile, so two
    // that overlap (spawned on one tile) do not wipe out each other's mark when one leaves.
    npc_occupancy: Vec<u16>,
    player_occupancy: Vec<u16>,
    blocks_players_occupancy: Vec<u16>,
}

impl CollisionMap {
    /// Default occupancy respected by a player: blocked only by NPCs that stand in players' way
    /// (`blocks_movement`), never by other players.
    pub const PLAYER_DEFAULT_RESPECTS: OccupancyMask = OccupancyMask::BLOCKS_PLAYERS;
    /// Default occupancy respected by an NPC-sized mover: blocked by NPCs and players alike.
    pub const NPC_DEFAULT_RESPECTS: OccupancyMask = OccupancyMask::ALL;

    pub fn new(data: CollisionMapData) -> Result<Self, CollisionMapDataError> {
        if data.width <= 0 || data.height <= 0 {
            return Err(CollisionMapDataError::NonPositiveSize);
        }
        let expected = (data.width * data.height) as usize;
        if data.flags.len() != expected || data.heights.len() != expected {
            return Err(CollisionMapDataError::ArrayLengthMismatch);
        }

        Ok(Self {
            origin_x: data.origin_x,
            origin_z: data.origin_z,
            width: data.width,
            height: data.height,
            max_step_height: data.max_step_height,
            base_flags: data.flags,
            heights: data.heights,
            npc_occupancy: vec![0; expected],
            player_occupancy: vec![0; expected],
            blocks_players_occupancy: vec![0; expected],
        })
    }

    /// Adds a block of tiles to the map, growing its bounds to take them in (a map that is
    /// streamed in square by square). Tiles of the new bounds that no block has covered yet read
    /// as fully blocked; existing flags, heights and occupancy are kept, and `data` overwrites the
    /// base flags and heights of the tiles it covers.
    pub fn merge(&mut self, data: CollisionMapData) -> Result<(), CollisionMapDataError> {
        if data.width <= 0 || data.height <= 0 {
            return Err(CollisionMapDataError::NonPositiveSize);
        }
        let incoming = (data.width * data.height) as usize;
        if data.flags.len() != incoming || data.heights.len() != incoming {
            return Err(CollisionMapDataError::ArrayLengthMismatch);
        }

        let min_x = self.origin_x.min(data.origin_x);
        let min_z = self.origin_z.min(data.origin_z);
        let max_x = (self.origin_x + self.width).max(data.origin_x + data.width);
        let max_z = (self.origin_z + self.height).max(data.origin_z + data.height);
        if (min_x, min_z, max_x - min_x, max_z - min_z) != (self.origin_x, self.origin_z, self.width, self.height) {
            let (width, height) = (max_x - min_x, max_z - min_z);
            let cells = (width * height) as usize;
            let mut flags = vec![FULL_BLOCK_MASK; cells];
            let mut heights = vec![0i16; cells];
            let mut npc = vec![0u16; cells];
            let mut player = vec![0u16; cells];
            let mut blocking = vec![0u16; cells];
            for z in 0..self.height {
                let from = (z * self.width) as usize;
                let to = ((z + self.origin_z - min_z) * width + (self.origin_x - min_x)) as usize;
                let row = self.width as usize;
                flags[to..to + row].copy_from_slice(&self.base_flags[from..from + row]);
                heights[to..to + row].copy_from_slice(&self.heights[from..from + row]);
                npc[to..to + row].copy_from_slice(&self.npc_occupancy[from..from + row]);
                player[to..to + row].copy_from_slice(&self.player_occupancy[from..from + row]);
                blocking[to..to + row].copy_from_slice(&self.blocks_players_occupancy[from..from + row]);
            }
            self.origin_x = min_x;
            self.origin_z = min_z;
            self.width = width;
            self.height = height;
            self.base_flags = flags;
            self.heights = heights;
            self.npc_occupancy = npc;
            self.player_occupancy = player;
            self.blocks_players_occupancy = blocking;
        }

        for z in 0..data.height {
            let from = (z * data.width) as usize;
            let to = ((z + data.origin_z - self.origin_z) * self.width + (data.origin_x - self.origin_x)) as usize;
            let row = data.width as usize;
            self.base_flags[to..to + row].copy_from_slice(&data.flags[from..from + row]);
            self.heights[to..to + row].copy_from_slice(&data.heights[from..from + row]);
        }
        Ok(())
    }

    pub fn origin_x(&self) -> i32 {
        self.origin_x
    }

    pub fn origin_z(&self) -> i32 {
        self.origin_z
    }

    pub fn width(&self) -> i32 {
        self.width
    }

    pub fn height(&self) -> i32 {
        self.height
    }

    fn in_bounds(&self, t: TileCoord) -> bool {
        if t.plane != 0 {
            return false; // v1: single plane only (see TileCoord remarks)
        }
        let local_x = t.x - self.origin_x;
        let local_z = t.z - self.origin_z;
        local_x >= 0 && local_x < self.width && local_z >= 0 && local_z < self.height
    }

    fn index_of(&self, t: TileCoord) -> usize {
        ((t.z - self.origin_z) * self.width + (t.x - self.origin_x)) as usize
    }

    /// Out-of-bounds tiles read as fully blocked, never panic.
    pub fn flags_at(&self, t: TileCoord) -> u32 {
        if self.in_bounds(t) {
            self.base_flags[self.index_of(t)]
        } else {
            FULL_BLOCK_MASK
        }
    }

    pub fn height_at(&self, t: TileCoord) -> i16 {
        if self.in_bounds(t) {
            self.heights[self.index_of(t)]
        } else {
            0
        }
    }

    pub fn add_flags(&mut self, t: TileCoord, flags: u32) {
        if !self.in_bounds(t) {
            return;
        }
        let idx = self.index_of(t);
        self.base_flags[idx] |= flags;
    }

    pub fn remove_flags(&mut self, t: TileCoord, flags: u32) {
        if !self.in_bounds(t) {
            return;
        }
        let idx = self.index_of(t);
        self.base_flags[idx] &= !flags;
    }

    pub fn is_blocked(&self, t: TileCoord, size: i32) -> bool {
        for dz in 0..size {
            for dx in 0..size {
                let tile = TileCoord::with_plane(t.x + dx, t.z + dz, t.plane);
                if !self.in_bounds(tile) {
                    return true;
                }
                if self.base_flags[self.index_of(tile)] & FULL_BLOCK_MASK != 0 {
                    return true;
                }
            }
        }
        false
    }

    /// Adds (`occupied`) or removes one mover of kind `who` on tile `t`. Every mover that adds
    /// itself must remove itself the same way; the tile reads occupied while any is there.
    pub fn set_occupied(&mut self, t: TileCoord, who: OccupancyMask, occupied: bool) {
        if !self.in_bounds(t) {
            return;
        }
        let idx = self.index_of(t);
        let count = |overlay: &mut Vec<u16>| {
            overlay[idx] = if occupied { overlay[idx].saturating_add(1) } else { overlay[idx].saturating_sub(1) };
        };
        if who.intersects(OccupancyMask::NPC) {
            count(&mut self.npc_occupancy);
        }
        if who.intersects(OccupancyMask::PLAYER) {
            count(&mut self.player_occupancy);
        }
        if who.intersects(OccupancyMask::BLOCKS_PLAYERS) {
            count(&mut self.blocks_players_occupancy);
        }
    }

    pub fn is_occupied(&self, t: TileCoord, respects: OccupancyMask) -> bool {
        self.in_bounds(t) && self.is_occupied_at(self.index_of(t), respects)
    }

    fn is_occupied_at(&self, idx: usize, respects: OccupancyMask) -> bool {
        (respects.intersects(OccupancyMask::NPC) && self.npc_occupancy[idx] != 0)
            || (respects.intersects(OccupancyMask::PLAYER) && self.player_occupancy[idx] != 0)
            || (respects.intersects(OccupancyMask::BLOCKS_PLAYERS) && self.blocks_players_occupancy[idx] != 0)
    }

    /// Can a mover of the given footprint size step from `from` in `dir`? Encodes directional
    /// walls, no-corner-cutting, per-edge step height, full blocks and occupancy.
    ///
    /// Size > 1 footprints are anchored at their SOUTH-WEST tile. For an east move of size N,
    /// the leading (newly entered) edge is column `from.x + N` over rows `from.z ..= from.z + N - 1`;
    /// for a north-east move it's column `from.x + N` over rows `from.z ..= from.z + N`, AND row
    /// `from.z + N` over columns `from.x ..= from.x + N`. This is the single most bug-prone
    /// function in the project - do not "simplify" the per-row/column edge checks below.
    ///
    /// (The C# defaults are `size = 1` and `respects = OccupancyMask.All`; Rust has no default
    /// arguments, so callers pass them explicitly.)
    pub fn can_traverse(&self, from: TileCoord, dir: Direction, size: i32, respects: OccupancyMask) -> bool {
        if dir == Direction::None || size < 1 {
            return false;
        }

        let to = from + dir;
        if to.plane != from.plane {
            return false; // v1: pathfinder/mover never cross planes
        }

        if dir.is_diagonal() {
            let (cardinal_a, cardinal_b) = dir.cardinal_components();

            // No corner cutting: a diagonal step is legal only if both adjacent cardinal
            // steps (from the SAME current position) are also legal.
            if !self.can_traverse(from, cardinal_a, size, respects) {
                return false;
            }
            if !self.can_traverse(from, cardinal_b, size, respects) {
                return false;
            }
            // Nor cutting past the destination's own walls: each L's second leg, from the tile
            // beside the start into the destination, must cross open edges too.
            if !self.can_cross_cardinal_edge(from + cardinal_a, cardinal_b, size)
                || !self.can_cross_cardinal_edge(from + cardinal_b, cardinal_a, size)
            {
                return false;
            }
            if !self.can_enter_footprint(to, size, respects) {
                return false;
            }

            // Explicit diagonal block flags are rarely baked but are respected when present.
            let diag_exit = diagonal_flag(dir);
            let diag_enter = diagonal_flag(dir.opposite());
            if self.flags_at(from) & diag_exit != 0 {
                return false;
            }
            if self.flags_at(to) & diag_enter != 0 {
                return false;
            }
            return true;
        }

        if !self.can_cross_cardinal_edge(from, dir, size) {
            return false;
        }
        self.can_enter_footprint(to, size, respects)
    }

    fn can_enter_footprint(&self, to: TileCoord, size: i32, respects: OccupancyMask) -> bool {
        for dz in 0..size {
            for dx in 0..size {
                let t = TileCoord::with_plane(to.x + dx, to.z + dz, to.plane);
                if !self.in_bounds(t) {
                    return false;
                }
                let idx = self.index_of(t);
                if self.base_flags[idx] & FULL_BLOCK_MASK != 0 {
                    return false;
                }
                if self.is_occupied_at(idx, respects) {
                    return false;
                }
            }
        }
        true
    }

    fn can_cross_cardinal_edge(&self, from: TileCoord, dir: Direction, size: i32) -> bool {
        let plane = from.plane;
        let tile = |x, z| TileCoord::with_plane(x, z, plane);

        // Each cardinal move checks the exit edge of the footprint's trailing side against the
        // matching entry edge one tile beyond it, once per row (east/west) or column (north/south).
        if !dir.is_cardinal() {
            return false;
        }
        let (exit_flag, enter_flag) = (edge_flag(dir), edge_flag(dir.opposite()));

        for i in 0..size {
            let (exit_tile, enter_tile) = match dir {
                Direction::East => (tile(from.x + size - 1, from.z + i), tile(from.x + size, from.z + i)),
                Direction::West => (tile(from.x, from.z + i), tile(from.x - 1, from.z + i)),
                Direction::North => (tile(from.x + i, from.z + size - 1), tile(from.x + i, from.z + size)),
                Direction::South => (tile(from.x + i, from.z), tile(from.x + i, from.z - 1)),
                _ => return false,
            };

            if !self.check_edge_pair(exit_tile, enter_tile, exit_flag, enter_flag) {
                return false;
            }
        }
        true
    }

    fn check_edge_pair(&self, exit_tile: TileCoord, enter_tile: TileCoord, exit_flag: u32, enter_flag: u32) -> bool {
        if !self.in_bounds(exit_tile) || !self.in_bounds(enter_tile) {
            return false;
        }

        let exit_idx = self.index_of(exit_tile);
        let enter_idx = self.index_of(enter_tile);

        if self.base_flags[exit_idx] & exit_flag != 0 {
            return false;
        }
        if self.base_flags[enter_idx] & enter_flag != 0 {
            return false;
        }

        let delta = (self.heights[exit_idx] as i32 - self.heights[enter_idx] as i32).abs();
        delta <= self.max_step_height
    }
}

/// The wall bit on a tile's edge facing `dir` (a cardinal direction; 0 for any other).
pub(crate) fn edge_flag(dir: Direction) -> u32 {
    match dir {
        Direction::East => CollisionFlag::BLOCK_EAST,
        Direction::West => CollisionFlag::BLOCK_WEST,
        Direction::North => CollisionFlag::BLOCK_NORTH,
        Direction::South => CollisionFlag::BLOCK_SOUTH,
        _ => 0,
    }
}

fn diagonal_flag(dir: Direction) -> u32 {
    match dir {
        Direction::SouthWest => CollisionFlag::BLOCK_SOUTH_WEST,
        Direction::SouthEast => CollisionFlag::BLOCK_SOUTH_EAST,
        Direction::NorthWest => CollisionFlag::BLOCK_NORTH_WEST,
        Direction::NorthEast => CollisionFlag::BLOCK_NORTH_EAST,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_grid;
    use super::*;

    const ALL: OccupancyMask = OccupancyMask::ALL;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    // --- CollisionMapTests ---

    #[test]
    fn directional_wall_blocks_one_way_only() {
        // "." at (0,0) with a wall on its east edge, "." at (1,0) open beyond it.
        let map = test_grid::parse("|.", 100);

        // Leaving (0,0) eastward is blocked by the wall.
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
        // Leaving (1,0) westward is blocked by the same wall (two-sided bake).
        assert!(!map.can_traverse(tc(1, 0), Direction::West, 1, ALL));

        // No wall on north/south, so those remain open.
        let open_map = test_grid::open(2, 2, 100);
        assert!(open_map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
    }

    #[test]
    fn full_block_blocks_all_entry_directions() {
        // ASCII row 0 is the north-most row (max Z), so this places the block at (1,0) with
        // open tiles at (0,0), (2,0), and (1,1) to its north.
        let map = test_grid::parse("...\n.#.", 100);

        assert!(!map.can_traverse(tc(1, 1), Direction::South, 1, ALL)); // into blocked from north
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, ALL)); // into blocked from west
        assert!(!map.can_traverse(tc(2, 0), Direction::West, 1, ALL)); // into blocked from east
        assert!(map.is_blocked(tc(1, 0), 1));
    }

    #[test]
    fn overlay_add_and_remove_flags_changes_traversability() {
        let mut map = test_grid::open(3, 3, 100);
        let a = tc(0, 0);
        assert!(map.can_traverse(a, Direction::East, 1, ALL));

        map.add_flags(a + Direction::East, CollisionFlag::BLOCK_OBJECT);
        assert!(!map.can_traverse(a, Direction::East, 1, ALL));

        map.remove_flags(a + Direction::East, CollisionFlag::BLOCK_OBJECT);
        assert!(map.can_traverse(a, Direction::East, 1, ALL));
    }

    #[test]
    fn out_of_bounds_reads_as_fully_blocked_never_panics() {
        let map = test_grid::open(2, 2, 100);

        assert_eq!(map.flags_at(tc(99, 99)), CollisionFlag::BLOCK_FULL);
        assert!(map.is_blocked(tc(-5, -5), 1));
        assert!(!map.can_traverse(tc(0, 0), Direction::West, 1, ALL)); // would step out of bounds
    }

    #[test]
    fn step_height_blocks_edge_beyond_max_delta_allows_within_it() {
        // Tile (0,0) height 0, tile (1,0) height 9 - with max_step_height 2 the edge is blocked.
        let blocked_map = test_grid::parse("09", 2);
        assert!(!blocked_map.can_traverse(tc(0, 0), Direction::East, 1, ALL));

        // Same heights, but a generous max_step_height allows it.
        let allowed_map = test_grid::parse("09", 20);
        assert!(allowed_map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
    }

    #[test]
    fn size_2_footprint_blocked_by_any_tile_in_leading_edge() {
        // ASCII row 0 (north-most) places a block at (2,1); row 1 (z=0) is fully open.
        // A size-2 mover occupying (0,0)-(1,1) moving east has a leading edge at column 2,
        // rows 0-1 - row 1's block makes the whole footprint move illegal even though row 0
        // alone would be fine.
        let map = test_grid::parse("..#.\n....", 100);

        assert!(!map.can_traverse(tc(0, 0), Direction::East, 2, ALL));
        // A size-1 mover using only row 0 (z = 0) never touches the blocked tile at z = 1.
        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
    }

    // --- OccupancyTests ---

    #[test]
    fn player_occupancy_does_not_block_another_player() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::PLAYER, true);

        // A player mover's default respects mask is Npc-only - it must not be blocked by
        // another player's occupancy.
        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::PLAYER_DEFAULT_RESPECTS));
    }

    #[test]
    fn an_npc_that_blocks_players_blocks_a_player() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::NPC | OccupancyMask::BLOCKS_PLAYERS, true);

        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::PLAYER_DEFAULT_RESPECTS));
    }

    #[test]
    fn a_player_walks_through_an_ordinary_npc_that_still_blocks_npcs() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::NPC, true);

        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::PLAYER_DEFAULT_RESPECTS));
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::NPC_DEFAULT_RESPECTS));
    }

    #[test]
    fn a_tile_stays_occupied_until_everyone_on_it_has_left() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::NPC, true);
        map.set_occupied(tc(1, 0), OccupancyMask::NPC, true);

        map.set_occupied(tc(1, 0), OccupancyMask::NPC, false);
        assert!(map.is_occupied(tc(1, 0), OccupancyMask::NPC), "one of the two is still there");
        map.set_occupied(tc(1, 0), OccupancyMask::NPC, false);
        assert!(!map.is_occupied(tc(1, 0), OccupancyMask::NPC));
    }

    #[test]
    fn player_occupancy_blocks_an_npc() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::PLAYER, true);

        // An NPC mover's default respects mask is All - a player standing in its way blocks it.
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::NPC_DEFAULT_RESPECTS));
    }

    #[test]
    fn npc_occupancy_blocks_another_npc() {
        let mut map = test_grid::open(2, 2, 100);
        map.set_occupied(tc(1, 0), OccupancyMask::NPC, true);

        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::NPC_DEFAULT_RESPECTS));
    }

    #[test]
    fn clearing_occupancy_restores_traversability() {
        let mut map = test_grid::open(2, 2, 100);
        let destination = tc(1, 0);
        map.set_occupied(destination, OccupancyMask::NPC, true);
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::NPC_DEFAULT_RESPECTS));

        map.set_occupied(destination, OccupancyMask::NPC, false);
        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, CollisionMap::NPC_DEFAULT_RESPECTS));
    }

    // --- CornerCuttingTests ---

    #[test]
    fn diagonal_past_wall_corner_is_rejected_when_one_cardinal_component_is_blocked() {
        let mut map = test_grid::open(2, 2, 100);
        map.add_flags(tc(0, 0), CollisionFlag::BLOCK_NORTH);

        // North component of the NE diagonal is blocked, so the diagonal shortcut must be
        // rejected even though the diagonal's destination tile (1,1) is itself open.
        assert!(!map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
        assert!(!map.can_traverse(tc(0, 0), Direction::NorthEast, 1, ALL));
    }

    #[test]
    fn diagonal_past_wall_corner_is_rejected_when_other_cardinal_component_is_blocked() {
        let mut map = test_grid::open(2, 2, 100);
        map.add_flags(tc(0, 0), CollisionFlag::BLOCK_EAST);

        assert!(map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
        assert!(!map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
        assert!(!map.can_traverse(tc(0, 0), Direction::NorthEast, 1, ALL));
    }

    #[test]
    fn diagonal_into_a_tile_walled_on_an_edge_facing_the_mover_is_rejected() {
        // A wall on (1,1)'s south edge or on its west edge: neither is on (0,0)'s edges, so
        // both cardinal steps from (0,0) are open, but each closes one L into (1,1).
        for (beside, beside_flag, dest_flag) in [
            (tc(1, 0), CollisionFlag::BLOCK_NORTH, CollisionFlag::BLOCK_SOUTH),
            (tc(0, 1), CollisionFlag::BLOCK_EAST, CollisionFlag::BLOCK_WEST),
        ] {
            let mut map = test_grid::open(2, 2, 100);
            map.add_flags(beside, beside_flag);
            map.add_flags(tc(1, 1), dest_flag);

            assert!(map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
            assert!(map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
            assert!(!map.can_traverse(tc(0, 0), Direction::NorthEast, 1, ALL), "wall {dest_flag:#x} on the destination");
            assert!(!map.can_traverse(tc(1, 1), Direction::SouthWest, 1, ALL), "nor back out past it");
        }
    }

    #[test]
    fn diagonal_through_open_gap_is_accepted() {
        let map = test_grid::open(2, 2, 100);

        assert!(map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
        assert!(map.can_traverse(tc(0, 0), Direction::East, 1, ALL));
        assert!(map.can_traverse(tc(0, 0), Direction::NorthEast, 1, ALL));
    }

    #[test]
    fn diagonal_into_fully_blocked_destination_is_rejected_even_with_open_cardinals() {
        // Row 0 (north) = "#." -> (0,1) blocked, (1,1) open. Row 1 (south) = ".." open.
        // From (0,0): North component goes to (0,1), which is the blocked tile itself -
        // that's a full block, not a corner-cut, but should still reject the diagonal.
        let map = test_grid::parse("#.\n..", 100);

        assert!(!map.can_traverse(tc(0, 0), Direction::North, 1, ALL));
        assert!(!map.can_traverse(tc(0, 0), Direction::NorthEast, 1, ALL));
    }

    // --- streaming ---

    fn block(origin_x: i32, origin_z: i32, size: i32, flags: u32) -> CollisionMapData {
        let cells = (size * size) as usize;
        CollisionMapData { origin_x, origin_z, width: size, height: size, flags: vec![flags; cells], heights: vec![0; cells], max_step_height: 100 }
    }

    #[test]
    fn merging_a_block_grows_the_map_and_keeps_what_was_there() {
        let mut map = CollisionMap::new(block(0, 0, 4, 0)).unwrap();
        map.add_flags(tc(1, 1), CollisionFlag::BLOCK_OBJECT);
        map.set_occupied(tc(2, 2), OccupancyMask::NPC, true);

        // a block to the west and south of the first, not touching it
        map.merge(block(-8, -8, 4, 0)).unwrap();

        assert_eq!((map.origin_x(), map.origin_z(), map.width(), map.height()), (-8, -8, 12, 12));
        assert_eq!(map.flags_at(tc(1, 1)), CollisionFlag::BLOCK_OBJECT, "old flags survive");
        assert!(map.is_occupied(tc(2, 2), OccupancyMask::NPC), "old occupancy survives");
        assert_eq!(map.flags_at(tc(-7, -7)), 0, "the new block is open");
        // the gap between the two blocks has never been loaded, so it is solid
        assert_eq!(map.flags_at(tc(-2, -2)), CollisionFlag::BLOCK_FULL);
    }

    #[test]
    fn merging_a_block_inside_the_bounds_overwrites_it() {
        let mut map = CollisionMap::new(block(0, 0, 4, CollisionFlag::BLOCK_FULL)).unwrap();
        assert!(map.is_blocked(tc(1, 1), 1));
        map.merge(block(0, 0, 2, 0)).unwrap();
        assert!(!map.is_blocked(tc(1, 1), 1));
        assert!(map.is_blocked(tc(3, 3), 1));
        assert_eq!((map.width(), map.height()), (4, 4));
    }

    #[test]
    fn a_player_can_walk_from_one_merged_block_into_the_next() {
        let mut map = CollisionMap::new(block(0, 0, 4, 0)).unwrap();
        assert!(!map.can_traverse(tc(3, 0), Direction::East, 1, ALL), "nothing loaded to the east yet");
        map.merge(block(4, 0, 4, 0)).unwrap();
        assert!(map.can_traverse(tc(3, 0), Direction::East, 1, ALL));
    }

    // --- constructor validation ---

    #[test]
    fn constructor_rejects_mismatched_array_lengths_and_non_positive_sizes() {
        let bad_len = CollisionMapData { width: 2, height: 2, flags: vec![0; 3], heights: vec![0; 4], ..Default::default() };
        assert_eq!(CollisionMap::new(bad_len).unwrap_err(), CollisionMapDataError::ArrayLengthMismatch);

        let bad_size = CollisionMapData { width: 0, height: 2, ..Default::default() };
        assert_eq!(CollisionMap::new(bad_size).unwrap_err(), CollisionMapDataError::NonPositiveSize);
    }
}
