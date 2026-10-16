//! Loading the map one 64x64 square at a time, for a world that is streamed in as the player
//! moves through it.
//!
//! A square is decoded together with its eight neighbours. The terrain colour blends over 11x11
//! tiles, lighting reads the heights next to each vertex, and scenery in a neighbouring square
//! can stand across the border; looking only at one square would leave visible seams and miss
//! the walls and objects that spill over the edge.

use crate::collision::{flag, CollisionGrid};
use crate::config::{Configs, ObjectDef, Overlay, Underlay};
use crate::terrain::REGION;
use crate::terrain_mesh::{PlacedObjects, TerrainGrid};
use crate::{Cache, Result};
use std::collections::HashMap;

/// Tiles along one side of a map square.
pub const SQUARE: i32 = REGION as i32;

/// The group in the maps archive (5) that holds map square (`sx`, `sy`).
pub fn map_group(sx: i32, sy: i32) -> u32 {
    ((sx << 8) | sy) as u32
}

/// The cache plus the definitions every square needs, decoded once.
pub struct MapSource {
    cache: Cache,
    underlays: HashMap<u32, Underlay>,
    overlays: HashMap<u32, Overlay>,
    defs: HashMap<u32, ObjectDef>,
}

/// A square and the eight around it, decoded.
pub struct Neighbourhood {
    /// Map square coordinates of the middle square.
    pub square: (i32, i32),
    /// 3x3 squares of terrain; the middle square is tiles `64..128` in both directions.
    pub grid: TerrainGrid,
    /// Every placement in those nine squares, in `grid` tile coordinates.
    pub placed: PlacedObjects,
}

impl MapSource {
    pub fn open() -> Result<MapSource> {
        let cache = Cache::open_default()?;
        MapSource::from_cache(cache)
    }

    pub fn from_cache(cache: Cache) -> Result<MapSource> {
        let (underlays, overlays, defs) = {
            let configs = Configs::new(&cache)?;
            (configs.underlays()?, configs.overlays()?, configs.objects()?)
        };
        Ok(MapSource { cache, underlays, overlays, defs })
    }

    pub fn cache(&self) -> &Cache {
        &self.cache
    }

    pub fn object_defs(&self) -> &HashMap<u32, ObjectDef> {
        &self.defs
    }

    /// Decodes map square (`sx`, `sy`) and the squares around it.
    pub fn neighbourhood(&self, sx: i32, sy: i32) -> Result<Neighbourhood> {
        let grid = TerrainGrid::load(&self.cache, sx - 1, sy - 1, 3, 3, self.underlays.clone(), self.overlays.clone())?;
        let placed = PlacedObjects::load(&self.cache, &grid)?;
        Ok(Neighbourhood { square: (sx, sy), grid, placed })
    }

    /// Collision flags of square (`sx`, `sy`) alone: `64 * 64` flags, index `z * 64 + x`, ground
    /// level, for walking.
    pub fn collision(&self, sx: i32, sy: i32) -> Result<Vec<u32>> {
        Ok(self.neighbourhood(sx, sy)?.collision(&self.defs))
    }
}

impl Neighbourhood {
    /// Whether the middle square has map data at all. Where it has none (open sea past the coast)
    /// there is nothing to stand on.
    pub fn exists(&self) -> bool {
        !self.grid.missing.contains(&self.square)
    }

    /// The placements of the middle square alone, each with its world tile `(x, z)`.
    pub fn middle_placements(&self) -> impl Iterator<Item = ((i32, i32), &crate::locs::Placement)> {
        let (wx0, wz0) = (self.square.0 * SQUARE, self.square.1 * SQUARE);
        self.placed.placements.iter().filter_map(move |(gx, gz, p)| {
            let (lx, lz) = (gx - SQUARE, gz - SQUARE);
            ((0..SQUARE).contains(&lx) && (0..SQUARE).contains(&lz)).then_some(((wx0 + lx, wz0 + lz), p))
        })
    }

    /// Collision flags of the middle square: `64 * 64`, index `z * 64 + x`. A square that is not
    /// in the cache is solid.
    pub fn collision(&self, defs: &HashMap<u32, ObjectDef>) -> Vec<u32> {
        let cells = (SQUARE * SQUARE) as usize;
        if !self.exists() {
            return vec![flag::FLOOR; cells];
        }
        let all = CollisionGrid::build(&self.grid, &self.placed, defs);
        let mut out = Vec::with_capacity(cells);
        for z in 0..SQUARE {
            for x in 0..SQUARE {
                out.push(all.at(self.grid.region_x * SQUARE + SQUARE + x, self.grid.region_y * SQUARE + SQUARE + z));
            }
        }
        out
    }
}
