//! Walkability of a block of map squares, built the way the game client builds its collision
//! maps from terrain flags and placed objects.
//!
//! Only plane 0 is produced (what the simulator walks on). Bits are the client's own flag values:
//! the eight edge/corner bits, `OBJECT` for scenery, `FLOOR_DECORATION` and `FLOOR`.
//! A set direction bit means "you cannot leave this tile through that edge/corner"; walls set
//! the bit on their own tile and the opposite bit on the tile beyond, so they block both ways.

use crate::config::ObjectDef;
use crate::scene::footprint;
use crate::terrain_mesh::{PlacedObjects, TerrainGrid};
use std::collections::HashMap;

pub mod flag {
    pub const NORTH_WEST: u32 = 0x1;
    pub const NORTH: u32 = 0x2;
    pub const NORTH_EAST: u32 = 0x4;
    pub const EAST: u32 = 0x8;
    pub const SOUTH_EAST: u32 = 0x10;
    pub const SOUTH: u32 = 0x20;
    pub const SOUTH_WEST: u32 = 0x40;
    pub const WEST: u32 = 0x80;
    /// Solid scenery.
    pub const OBJECT: u32 = 0x100;
    pub const FLOOR_DECORATION: u32 = 0x40000;
    /// Water, cliffs and other terrain flagged as unwalkable.
    pub const FLOOR: u32 = 0x200000;
}

/// Collision flags of `width` x `depth` tiles whose south-west corner is world tile
/// (`origin_x`, `origin_z`); index `z * width + x`.
#[derive(Debug, Clone)]
pub struct CollisionGrid {
    pub origin_x: i32,
    pub origin_z: i32,
    pub width: i32,
    pub depth: i32,
    pub flags: Vec<u32>,
}

/// The wall bits a wall piece of shape `kind` (0 straight, 1 and 3 diagonal corners, 2 L-shaped
/// corner) turned `orientation` sets, as `(dx, dz, bits)` from its own tile: its own side of the
/// edge or corner, and the neighbour's side of it. Other shapes set none. (Orientation 0 is the
/// west edge or north-west corner, then clockwise.)
pub fn wall_edges(kind: u8, orientation: u8) -> Vec<(i32, i32, u32)> {
    use flag::*;
    let o = orientation & 3;
    match (kind, o) {
        (0, 0) => vec![(0, 0, WEST), (-1, 0, EAST)],
        (0, 1) => vec![(0, 0, NORTH), (0, 1, SOUTH)],
        (0, 2) => vec![(0, 0, EAST), (1, 0, WEST)],
        (0, _) => vec![(0, 0, SOUTH), (0, -1, NORTH)],
        (1 | 3, 0) => vec![(0, 0, NORTH_WEST), (-1, 1, SOUTH_EAST)],
        (1 | 3, 1) => vec![(0, 0, NORTH_EAST), (1, 1, SOUTH_WEST)],
        (1 | 3, 2) => vec![(0, 0, SOUTH_EAST), (1, -1, NORTH_WEST)],
        (1 | 3, _) => vec![(0, 0, SOUTH_WEST), (-1, -1, NORTH_EAST)],
        (2, 0) => vec![(0, 0, WEST | NORTH), (-1, 0, EAST), (0, 1, SOUTH)],
        (2, 1) => vec![(0, 0, NORTH | EAST), (0, 1, SOUTH), (1, 0, WEST)],
        (2, 2) => vec![(0, 0, EAST | SOUTH), (1, 0, WEST), (0, -1, NORTH)],
        (2, _) => vec![(0, 0, SOUTH | WEST), (0, -1, NORTH), (-1, 0, EAST)],
        _ => Vec::new(),
    }
}

impl CollisionGrid {
    /// Flags of a world tile; outside the grid reads as fully blocked.
    pub fn at(&self, world_x: i32, world_z: i32) -> u32 {
        let (x, z) = (world_x - self.origin_x, world_z - self.origin_z);
        if x < 0 || z < 0 || x >= self.width || z >= self.depth {
            return flag::FLOOR;
        }
        self.flags[(z * self.width + x) as usize]
    }

    fn add(&mut self, gx: i32, gz: i32, bits: u32) {
        if gx >= 0 && gz >= 0 && gx < self.width && gz < self.depth {
            self.flags[(gz * self.width + gx) as usize] |= bits;
        }
    }

    pub fn build(grid: &TerrainGrid, placed: &PlacedObjects, defs: &HashMap<u32, ObjectDef>) -> CollisionGrid {
        let (width, depth) = (grid.width as i32, grid.depth as i32);
        let mut out = CollisionGrid {
            origin_x: grid.region_x * 64,
            origin_z: grid.region_y * 64,
            width,
            depth,
            flags: vec![0; (width * depth) as usize],
        };

        // Terrain: a tile flagged blocked on plane p blocks plane p, except that tiles under a
        // bridge deck (plane 1 flagged as bridge) drop one level: the deck's own flags apply to
        // the level the player walks on, and whatever is under the bridge is ignored.
        for plane in 0..2usize {
            for x in 0..width as usize {
                for z in 0..depth as usize {
                    if grid.tile(plane, x, z).settings & 1 == 0 {
                        continue;
                    }
                    let bridge = grid.tile(1, x, z).settings & 2 != 0;
                    let level = if bridge { plane as i32 - 1 } else { plane as i32 };
                    if level == 0 {
                        out.add(x as i32, z as i32, flag::FLOOR);
                    }
                }
            }
        }

        // Objects. `PlacedObjects` has already moved bridge-tile objects down a plane.
        for (gx, gz, p) in &placed.placements {
            if p.plane != 0 {
                continue;
            }
            let Some(def) = defs.get(&p.id) else { continue };
            let (x, z) = (*gx, *gz);
            match p.kind {
                0..=3 if def.interact_type != 0 => out.add_wall(x, z, p.kind, p.orientation),
                9..=21 if def.interact_type != 0 => {
                    let (fx, fz) = footprint(def, p.orientation);
                    for dx in 0..fx as i32 {
                        for dz in 0..fz as i32 {
                            out.add(x + dx, z + dz, flag::OBJECT);
                        }
                    }
                }
                22 if def.interactive_flag == 1 => out.add(x, z, flag::FLOOR_DECORATION),
                _ => {}
            }
        }
        out
    }

    /// Wall pieces: straight (0), diagonal corner pieces (1, 3) and L-shaped corners (2). The
    /// orientation says which edge or corner the piece sits on.
    fn add_wall(&mut self, x: i32, z: i32, kind: u8, orientation: u8) {
        for (dx, dz, bits) in wall_edges(kind, orientation) {
            self.add(x + dx, z + dz, bits);
        }
    }
}

