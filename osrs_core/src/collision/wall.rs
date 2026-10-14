//! Wall pieces' collision: which edge or corner bits a wall object sets on its own tile and on
//! the neighbour across it. The same table as `osrs_cache::collision::wall_edges`, which bakes the
//! map's walls; this one lets the server move a wall (open a door) without the cache. A test in
//! `tests/lumbridge_collision.rs` holds the two to the same values.

use super::collision_flag::CollisionFlag;
use super::collision_map::CollisionMap;
use crate::tiles::TileCoord;

/// The wall bits a wall piece of shape `kind` (0 straight, 1 and 3 diagonal corners, 2 L-shaped
/// corner) turned `orientation` sets, as `(dx, dz, bits)` from its own tile. Other shapes set
/// none. Orientation 0 is the west edge or north-west corner, then clockwise.
pub fn wall_edges(kind: u8, orientation: u8) -> Vec<(i32, i32, u32)> {
    use CollisionFlag as F;
    match (kind, orientation & 3) {
        (0, 0) => vec![(0, 0, F::BLOCK_WEST), (-1, 0, F::BLOCK_EAST)],
        (0, 1) => vec![(0, 0, F::BLOCK_NORTH), (0, 1, F::BLOCK_SOUTH)],
        (0, 2) => vec![(0, 0, F::BLOCK_EAST), (1, 0, F::BLOCK_WEST)],
        (0, _) => vec![(0, 0, F::BLOCK_SOUTH), (0, -1, F::BLOCK_NORTH)],
        (1 | 3, 0) => vec![(0, 0, F::BLOCK_NORTH_WEST), (-1, 1, F::BLOCK_SOUTH_EAST)],
        (1 | 3, 1) => vec![(0, 0, F::BLOCK_NORTH_EAST), (1, 1, F::BLOCK_SOUTH_WEST)],
        (1 | 3, 2) => vec![(0, 0, F::BLOCK_SOUTH_EAST), (1, -1, F::BLOCK_NORTH_WEST)],
        (1 | 3, _) => vec![(0, 0, F::BLOCK_SOUTH_WEST), (-1, -1, F::BLOCK_NORTH_EAST)],
        (2, 0) => vec![(0, 0, F::BLOCK_WEST | F::BLOCK_NORTH), (-1, 0, F::BLOCK_EAST), (0, 1, F::BLOCK_SOUTH)],
        (2, 1) => vec![(0, 0, F::BLOCK_NORTH | F::BLOCK_EAST), (0, 1, F::BLOCK_SOUTH), (1, 0, F::BLOCK_WEST)],
        (2, 2) => vec![(0, 0, F::BLOCK_EAST | F::BLOCK_SOUTH), (1, 0, F::BLOCK_WEST), (0, -1, F::BLOCK_NORTH)],
        (2, _) => vec![(0, 0, F::BLOCK_SOUTH | F::BLOCK_WEST), (0, -1, F::BLOCK_NORTH), (-1, 0, F::BLOCK_EAST)],
        _ => Vec::new(),
    }
}

impl CollisionMap {
    /// Puts a wall piece's bits on the map (a door closing, or one opening into its new place).
    pub fn add_wall(&mut self, tile: TileCoord, kind: u8, orientation: u8) {
        for (dx, dz, bits) in wall_edges(kind, orientation) {
            self.add_flags(TileCoord::with_plane(tile.x + dx, tile.z + dz, tile.plane), bits);
        }
    }

    /// Takes a wall piece's bits off the map. Like the game's, this clears the bits whatever else
    /// set them.
    pub fn remove_wall(&mut self, tile: TileCoord, kind: u8, orientation: u8) {
        for (dx, dz, bits) in wall_edges(kind, orientation) {
            self.remove_flags(TileCoord::with_plane(tile.x + dx, tile.z + dz, tile.plane), bits);
        }
    }
}
