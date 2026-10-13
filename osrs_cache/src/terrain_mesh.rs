//! Turns decoded terrain into a coloured triangle mesh, following the client's scene builder:
//! 11x11 underlay colour blending, per-vertex lighting from the height map normals, and the
//! 13 overlay tile shapes (tables extracted from the client bytecode).

use crate::config::{Overlay, Underlay};
use crate::model::{hsl_to_rgb, rgb_to_hsl_f32};
use crate::terrain::{Terrain, REGION};
use crate::{idx, locs, Cache, Result};
use std::collections::HashMap;

/// Vertex ids used per shape (1,3,5,7 = SW,SE,NE,NW corners; 2,4,6,8 = edge midpoints;
/// 9..12 = inset edge midpoints; 13..16 = inset corners).
const SHAPE_VERTICES: [&[u8]; 13] = [
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 6],
    &[1, 3, 5, 7, 2, 6],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 2, 8],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 11, 12],
    &[1, 3, 5, 7, 13, 14],
];

/// Triangles as `[flag, a, b, c]` quads of indices into the shape's vertex list.
/// flag 0 = overlay colour, flag 1 = underlay colour.
const SHAPE_TRIANGLES: [&[u8]; 13] = [
    &[0, 1, 2, 3, 0, 0, 1, 3],
    &[1, 1, 2, 3, 1, 0, 1, 3],
    &[0, 1, 2, 3, 1, 0, 1, 3],
    &[0, 0, 1, 2, 0, 0, 2, 4, 1, 0, 4, 3],
    &[0, 0, 1, 4, 0, 0, 4, 3, 1, 1, 2, 4],
    &[0, 0, 4, 3, 1, 0, 1, 2, 1, 0, 2, 4],
    &[0, 1, 2, 4, 1, 0, 1, 4, 1, 0, 4, 3],
    &[0, 4, 1, 2, 0, 4, 2, 5, 1, 0, 4, 5, 1, 0, 5, 3],
    &[0, 4, 1, 2, 0, 4, 2, 3, 0, 4, 3, 5, 1, 0, 4, 5],
    &[0, 0, 4, 5, 1, 4, 1, 2, 1, 4, 2, 3, 1, 4, 3, 5],
    &[0, 0, 1, 5, 0, 1, 4, 5, 0, 1, 2, 4, 1, 0, 5, 3, 1, 5, 4, 3, 1, 4, 2, 3],
    &[1, 0, 1, 5, 1, 1, 4, 5, 1, 1, 2, 4, 0, 0, 5, 3, 0, 5, 4, 3, 0, 4, 2, 3],
    &[1, 0, 5, 4, 1, 0, 1, 5, 0, 0, 4, 3, 0, 4, 5, 3, 0, 5, 2, 3, 0, 1, 2, 5],
];

pub const TILE: f32 = 128.0;

/// Terrain for a rectangle of map squares, stitched into one height/colour grid.
pub struct TerrainGrid {
    pub region_x: i32,
    pub region_y: i32,
    pub nx: usize,
    pub ny: usize,
    pub width: usize,
    pub depth: usize,
    /// `[plane][x][z]` over the whole rectangle.
    pub tiles: Vec<crate::terrain::Tile>,
    pub underlays: HashMap<u32, Underlay>,
    pub overlays: HashMap<u32, Overlay>,
    /// Map squares that were missing from the cache (rendered flat and empty).
    pub missing: Vec<(i32, i32)>,
}

impl TerrainGrid {
    pub fn load(
        cache: &Cache,
        region_x: i32,
        region_y: i32,
        nx: usize,
        ny: usize,
        underlays: HashMap<u32, Underlay>,
        overlays: HashMap<u32, Overlay>,
    ) -> Result<TerrainGrid> {
        let maps = cache.index(idx::MAPS)?;
        let width = nx * REGION;
        let depth = ny * REGION;
        let mut tiles = vec![crate::terrain::Tile::default(); 4 * width * depth];
        let mut missing = Vec::new();
        for ry in 0..ny {
            for rx in 0..nx {
                let (wx, wy) = (region_x + rx as i32, region_y + ry as i32);
                let gid = ((wx << 8) | wy) as u32;
                let terrain = match cache.read_files(&maps, gid) {
                    Ok(files) if !files.is_empty() => {
                        Terrain::decode(&files[0].1, wx * REGION as i32, wy * REGION as i32).ok()
                    }
                    _ => None,
                };
                let Some(t) = terrain else {
                    missing.push((wx, wy));
                    continue;
                };
                for p in 0..4 {
                    for x in 0..REGION {
                        for z in 0..REGION {
                            let gx = rx * REGION + x;
                            let gz = ry * REGION + z;
                            tiles[(p * width + gx) * depth + gz] = *t.tile(p, x, z);
                        }
                    }
                }
            }
        }
        Ok(TerrainGrid { region_x, region_y, nx, ny, width, depth, tiles, underlays, overlays, missing })
    }

    #[inline]
    pub fn tile(&self, plane: usize, x: usize, z: usize) -> &crate::terrain::Tile {
        &self.tiles[(plane * self.width + x) * self.depth + z]
    }

    /// Vertex height (client units, negative is up), clamped to the loaded area.
    pub fn height_at(&self, plane: usize, x: i32, z: i32) -> i32 {
        let x = x.clamp(0, self.width as i32 - 1) as usize;
        let z = z.clamp(0, self.depth as i32 - 1) as usize;
        self.tile(plane, x, z).height
    }

    /// Height (client units, like [`height_at`](Self::height_at)) of the ground surface at a point
    /// given in fractional tiles, blended between the four corners of the tile it falls in, as the
    /// client does when it bends a model onto the terrain.
    pub fn surface_height(&self, plane: usize, x: f32, z: f32) -> f32 {
        let (tx, tz) = (x.floor() as i32, z.floor() as i32);
        let (fx, fz) = (x - tx as f32, z - tz as f32);
        let h = |dx: i32, dz: i32| self.height_at(plane, tx + dx, tz + dz) as f32;
        let south = h(0, 0) * (1.0 - fx) + h(1, 0) * fx;
        let north = h(0, 1) * (1.0 - fx) + h(1, 1) * fx;
        south * (1.0 - fz) + north * fz
    }
}

#[derive(Default)]
pub struct TerrainMesh {
    pub positions: Vec<[f32; 3]>,
    /// Linear RGBA.
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Overlay triangles that use a texture image, by texture id (uv = tile coordinates).
    pub textured: HashMap<u32, crate::scene::MeshData>,
}

fn srgb_to_linear(c: f32) -> f32 {
    c.powf(2.2)
}

/// The client's saturation damping for bright colours, then packing to 16-bit HSL.
pub(crate) fn pack_hsl(hue: i32, sat: i32, light: i32) -> u16 {
    let mut sat = sat;
    if light > 179 {
        sat /= 2;
    }
    if light > 192 {
        sat /= 2;
    }
    if light > 217 {
        sat /= 2;
    }
    if light > 243 {
        sat /= 2;
    }
    (((sat / 32) << 7) + ((hue / 4) << 10) + light / 2) as u16
}

pub(crate) fn rgb_to_hsl_bytes(rgb: u32) -> (i32, i32, i32) {
    let (h, s, l) = rgb_to_hsl_f32(rgb);
    ((h * 256.0) as i32, (s * 256.0) as i32, (l * 256.0) as i32)
}

pub(crate) struct UnderlayHsl {
    pub hue: i32,
    pub sat: i32,
    pub light: i32,
    pub hue_mult: i32,
}

pub(crate) fn underlay_hsl(rgb: u32) -> UnderlayHsl {
    let (h, s, l) = rgb_to_hsl_f32(rgb);
    let mult = if l > 0.5 { (1.0 - l) * s * 512.0 } else { l * s * 512.0 } as i32;
    let mult = mult.max(1);
    UnderlayHsl {
        hue: (h * mult as f32) as i32,
        sat: (s * 256.0) as i32,
        light: (l * 256.0) as i32,
        hue_mult: mult,
    }
}

/// The overlay colour (magenta) that marks a tile as not drawn.
pub(crate) const INVISIBLE_RGB: u32 = 0xFF00FF;

const BRIGHTNESS: f32 = 0.8;
const AMBIENT: f32 = 96.0;

impl TerrainGrid {
    /// Build the mesh for one plane (0 = ground floor).
    ///
    /// Bridges: a tile whose plane-1 settings have bit 2 is a bridge deck. The client draws the
    /// plane-1 tile on plane 0 (above the real plane-0 tile, which stays as the underside).
    pub fn build_mesh(&self, plane: usize) -> TerrainMesh {
        self.build_mesh_window(plane, 0, 0, self.width, self.depth)
    }

    /// The mesh of just the `w` x `d` tiles whose south-west tile is (`x0`, `z0`), with positions
    /// relative to that corner. Colour blending and lighting still read the tiles around the
    /// window, so a window cut from a larger grid matches the same tiles of one big mesh and
    /// neighbouring windows meet without a seam.
    pub fn build_mesh_window(&self, plane: usize, x0: usize, z0: usize, w: usize, d: usize) -> TerrainMesh {
        let mut out = TerrainMesh::default();
        let window = (x0 as i32, z0 as i32, (x0 + w) as i32, (z0 + d) as i32);
        let bridge = |x: usize, z: usize| plane < 3 && self.tile(1, x, z).settings & 2 != 0;
        match plane {
            0 => {
                self.build_plane_into(&mut out, 0, window, &|_, _| true);
                self.build_plane_into(&mut out, 1, window, &|x, z| bridge(x, z));
            }
            1 => self.build_plane_into(&mut out, 1, window, &|x, z| !bridge(x, z)),
            p => self.build_plane_into(&mut out, p, window, &|_, _| true),
        }
        out
    }

    fn build_plane_into(
        &self,
        out: &mut TerrainMesh,
        plane: usize,
        window: (i32, i32, i32, i32),
        include: &dyn Fn(usize, usize) -> bool,
    ) {
        let (w, d) = (self.width as i32, self.depth as i32);
        let (wx0, wz0, wx1, wz1) = window;

        // --- 11x11 box-blended underlay colour per tile ---
        let ul: HashMap<u32, UnderlayHsl> =
            self.underlays.iter().map(|(&id, u)| (id, underlay_hsl(u.rgb))).collect();
        let mut blended = vec![None::<(i32, i32, i32)>; (w * d) as usize];
        // prefix sums would be faster; direct window sums keep this obviously correct.
        let r = 5i32;
        for x in wx0..wx1 {
            for z in wz0..wz1 {
                let t = self.tile(plane, x as usize, z as usize);
                if t.underlay == 0 {
                    continue;
                }
                let (mut hue, mut sat, mut light, mut mult, mut count) = (0i64, 0i64, 0i64, 0i64, 0i64);
                for xx in (x - r).max(0)..=(x + r).min(w - 1) {
                    for zz in (z - r).max(0)..=(z + r).min(d - 1) {
                        let n = self.tile(plane, xx as usize, zz as usize);
                        if n.underlay == 0 {
                            continue;
                        }
                        // underlay ids in the tile are 1-based
                        if let Some(u) = ul.get(&(n.underlay as u32 - 1)) {
                            hue += u.hue as i64;
                            sat += u.sat as i64;
                            light += u.light as i64;
                            mult += u.hue_mult as i64;
                            count += 1;
                        }
                    }
                }
                if count > 0 && mult > 0 {
                    let h = (hue * 256 / mult) as i32;
                    blended[(x * d + z) as usize] = Some((h, (sat / count) as i32, (light / count) as i32));
                }
            }
        }

        // --- per-vertex light intensity from height-map normals ---
        let light_vec = (-50.0f32, -10.0f32, -50.0f32);
        let divisor = 768.0 * (light_vec.0 * light_vec.0 + light_vec.1 * light_vec.1 + light_vec.2 * light_vec.2).sqrt() / 256.0;
        let vw = (w + 1) as usize;
        let mut intensity = vec![AMBIENT; vw * (d as usize + 1)];
        for x in wx0..=wx1.min(w) {
            for z in wz0..=wz1.min(d) {
                let dx = (self.height_at(plane, x + 1, z) - self.height_at(plane, x - 1, z)) as f32;
                let dz = (self.height_at(plane, x, z + 1) - self.height_at(plane, x, z - 1)) as f32;
                let len = (dx * dx + 256.0 * 256.0 + dz * dz).sqrt();
                let nx = dx * 256.0 / len;
                let ny = -65536.0 / len;
                let nz = dz * 256.0 / len;
                let dot = nx * light_vec.0 + ny * light_vec.1 + nz * light_vec.2;
                intensity[x as usize * (d as usize + 1) + z as usize] = AMBIENT + dot / divisor;
            }
        }
        let inten = |x: i32, z: i32| intensity[x as usize * (d as usize + 1) + z as usize];

        let shade = |hsl: u16, light: f32| -> [f32; 4] {
            let l7 = (hsl & 127) as f32;
            let l = (l7 * light / 128.0).clamp(2.0, 126.0) as u16;
            let c = (hsl & !127) | l;
            let rgb = hsl_to_rgb(c, BRIGHTNESS);
            [srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2]), 1.0]
        };

        for x in wx0..wx1 {
            for z in wz0..wz1 {
                if !include(x as usize, z as usize) {
                    continue;
                }
                let t = self.tile(plane, x as usize, z as usize);
                let under = blended[(x * d + z) as usize].map(|(h, s, l)| pack_hsl(h.clamp(0, 255), s.clamp(0, 255), l.clamp(0, 255)));
                let over = if t.overlay != 0 {
                    self.overlays.get(&(t.overlay as u32 - 1)).map(|o| {
                        if let Some(hsl) = o.texture_hsl {
                            return (hsl, o.hide_underlay, o.texture);
                        }
                        let (h, s, l) = rgb_to_hsl_bytes(o.rgb);
                        (pack_hsl(h, s, l), o.hide_underlay, None)
                    })
                } else {
                    None
                };
                // Magenta is the map makers' "draw nothing here" overlay (the upper-plane tiles of a
                // bridge, the gaps of a rooftop): the client skips those triangles rather than paint
                // them, and so do we.
                let see_through = t.overlay != 0
                    && self
                        .overlays
                        .get(&(t.overlay as u32 - 1))
                        .is_some_and(|o| o.rgb == INVISIBLE_RGB && o.texture_hsl.is_none());
                if under.is_none() && (over.is_none() || see_through) {
                    continue;
                }

                // corner heights / lights: SW, SE, NE, NW
                let hs = [
                    self.height_at(plane, x, z),
                    self.height_at(plane, x + 1, z),
                    self.height_at(plane, x + 1, z + 1),
                    self.height_at(plane, x, z + 1),
                ];
                let ls = [inten(x, z), inten(x + 1, z), inten(x + 1, z + 1), inten(x, z + 1)];
                let (bx, bz) = ((x - wx0) as f32 * TILE, (z - wz0) as f32 * TILE);
                let q = TILE / 4.0;
                let hlf = TILE / 2.0;
                let tq = TILE * 3.0 / 4.0;
                let avg = |a: usize, b: usize| ((hs[a] + hs[b]) >> 1, (ls[a] + ls[b]) * 0.5);

                // vertex id -> (x, z, height, light)
                let vertex = |id: u8| -> (f32, f32, i32, f32) {
                    match id {
                        1 => (bx, bz, hs[0], ls[0]),
                        2 => { let (h, l) = avg(0, 1); (bx + hlf, bz, h, l) }
                        3 => (bx + TILE, bz, hs[1], ls[1]),
                        4 => { let (h, l) = avg(1, 2); (bx + TILE, bz + hlf, h, l) }
                        5 => (bx + TILE, bz + TILE, hs[2], ls[2]),
                        6 => { let (h, l) = avg(2, 3); (bx + hlf, bz + TILE, h, l) }
                        7 => (bx, bz + TILE, hs[3], ls[3]),
                        8 => { let (h, l) = avg(3, 0); (bx, bz + hlf, h, l) }
                        9 => { let (h, l) = avg(0, 1); (bx + hlf, bz + q, h, l) }
                        10 => { let (h, l) = avg(1, 2); (bx + tq, bz + hlf, h, l) }
                        11 => { let (h, l) = avg(2, 3); (bx + hlf, bz + tq, h, l) }
                        12 => { let (h, l) = avg(3, 0); (bx + q, bz + hlf, h, l) }
                        13 => (bx + q, bz + q, hs[0], ls[0]),
                        14 => (bx + tq, bz + q, hs[1], ls[1]),
                        15 => (bx + tq, bz + tq, hs[2], ls[2]),
                        _ => (bx + q, bz + tq, hs[3], ls[3]),
                    }
                };

                let (shape, rot) = if over.is_some() { (t.shape as usize, t.rotation as i32) } else { (0, 0) };
                let shape = shape.min(12);
                let verts: Vec<(f32, f32, i32, f32)> = SHAPE_VERTICES[shape]
                    .iter()
                    .map(|&v| {
                        let mut v = v as i32;
                        if v & 1 == 0 && v <= 8 {
                            v = ((v - rot * 2 - 1) & 7) + 1;
                        }
                        if v > 8 && v <= 12 {
                            v = ((v - 9 - rot) & 3) + 9;
                        }
                        if v > 12 && v <= 16 {
                            v = ((v - 13 - rot) & 3) + 13;
                        }
                        vertex(v as u8)
                    })
                    .collect();

                let tris = SHAPE_TRIANGLES[shape];
                for tri in tris.chunks_exact(4) {
                    let flag = tri[0];
                    // overlay triangles of a see-through tile are not drawn, and neither is an
                    // underlay triangle that would take the overlay's colour instead
                    if see_through && (flag == 0 || under.is_none()) {
                        continue;
                    }
                    let hsl = match (&over, under, flag) {
                        (None, Some(u), _) => u,
                        (None, None, _) => continue,
                        (Some((o, _, _)), _, 0) => *o,
                        (Some((o, hide, _)), u, _) => match u {
                            Some(u) if !*hide || shape == 1 => u,
                            _ => *o,
                        },
                    };
                    if let (0, Some((_, _, Some(tex)))) = (flag, &over) {
                        let m = out.textured.entry(*tex as u32).or_default();
                        let base = m.positions.len() as u32;
                        for &ix in &tri[1..4] {
                            let mut i = ix as i32;
                            if i < 4 {
                                i = (i - rot) & 3;
                            }
                            let (vx, vz, vh, vl) = verts[i as usize];
                            m.positions.push([vx / TILE, -(vh as f32) / TILE, -(vz / TILE)]);
                            let shade = (vl / 128.0).clamp(0.05, 1.0).powf(BRIGHTNESS);
                            let lin = srgb_to_linear(shade);
                            m.colors.push([lin, lin, lin, 1.0]);
                            m.uvs.push([vx / TILE, vz / TILE]);
                        }
                        m.indices.extend_from_slice(&[base, base + 2, base + 1]);
                        continue;
                    }
                    let base = out.positions.len() as u32;
                    for &ix in &tri[1..4] {
                        let mut i = ix as i32;
                        if i < 4 {
                            i = (i - rot) & 3;
                        }
                        let (vx, vz, vh, vl) = verts[i as usize];
                        out.positions.push([vx / TILE, -(vh as f32) / TILE, -(vz / TILE)]);
                        out.colors.push(shade(hsl, vl));
                    }
                    // wind so the face points up (+Y) with z flipped to bevy's convention
                    out.indices.extend_from_slice(&[base, base + 2, base + 1]);
                }
            }
        }
    }
}

/// Objects placed on the terrain, ready for rendering.
pub struct PlacedObjects {
    pub placements: Vec<(i32, i32, locs::Placement)>,
}

impl PlacedObjects {
    /// Collect all placements for the grid's map squares (world tile coordinates).
    pub fn load(cache: &Cache, grid: &TerrainGrid) -> Result<PlacedObjects> {
        let maps = cache.index(idx::MAPS)?;
        let mut placements = Vec::new();
        for ry in 0..grid.ny {
            for rx in 0..grid.nx {
                let (wx, wy) = (grid.region_x + rx as i32, grid.region_y + ry as i32);
                let gid = ((wx << 8) | wy) as u32;
                let Ok(files) = cache.read_files(&maps, gid) else { continue };
                if files.len() < 2 {
                    continue;
                }
                for mut p in locs::decode(&files[1].1) {
                    let (gx, gz) = ((rx * REGION) as i32 + p.x as i32, (ry * REGION) as i32 + p.z as i32);
                    // Objects on a bridge tile move down one plane; ones below it are dropped.
                    if grid.tile(1, gx as usize, gz as usize).settings & 2 != 0 {
                        if p.plane == 0 {
                            continue;
                        }
                        p.plane -= 1;
                    }
                    placements.push((gx, gz, p));
                }
            }
        }
        Ok(PlacedObjects { placements })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::Tile;

    /// A 64x64 grid, flat, with one tile on plane 0 carrying `overlay` (overlay id 0, rgb `rgb`).
    fn grid_with_overlay(rgb: u32) -> TerrainGrid {
        let mut tiles = vec![Tile::default(); 4 * 64 * 64];
        tiles[(0 * 64 + 10) * 64 + 10] = Tile { overlay: 1, ..Tile::default() };
        let overlay = Overlay { rgb, ..Default::default() };
        TerrainGrid {
            region_x: 0,
            region_y: 0,
            nx: 1,
            ny: 1,
            width: 64,
            depth: 64,
            tiles,
            underlays: HashMap::new(),
            overlays: HashMap::from([(0, overlay)]),
            missing: Vec::new(),
        }
    }

    #[test]
    fn a_coloured_overlay_tile_is_drawn() {
        let mesh = grid_with_overlay(0x00FF00).build_mesh(0);
        assert_eq!(mesh.indices.len(), 6, "one tile is two triangles");
    }

    #[test]
    fn a_magenta_overlay_tile_is_the_map_makers_do_not_draw_marker() {
        // the upper-plane tiles of a bridge, or a gap in a rooftop, are painted #FF00FF in the map
        // data; the game draws nothing there (we used to paint them a pale lilac sheet)
        let mesh = grid_with_overlay(0xFF00FF).build_mesh(0);
        assert!(mesh.indices.is_empty(), "{} triangles drawn", mesh.indices.len() / 3);
    }
}
