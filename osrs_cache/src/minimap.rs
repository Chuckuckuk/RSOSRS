//! The minimap picture of a map square, drawn the way the client builds its minimap image:
//! four pixels per tile, flat colours (no lighting or height shading), north up, x east.
//!
//! What goes into it, in the client's order:
//! 1. Every tile's colour. An underlay tile is the 11x11 blended underlay colour at a fixed
//!    lightness (96/128 of the colour's own lightness, where the 3D view applies per-vertex light).
//!    An overlay replaces it, using the overlay's *secondary* colour when it has one (the map
//!    makers' minimap colour) and otherwise its colour or texture average. Overlay shapes cover only
//!    their part of the tile: the same triangles as the 3D view, sampled at the 4x4 pixel centres.
//! 2. Walls as one-pixel lines on the tile edge (white, red when the object is interactive).
//! 3. Map scenes: the small "mapscene" sprite an object def names (trees, rocks, ...), centred on the
//!    object's footprint.
//!
//! Map function icons (bank, shops, altars...) are not part of the picture: the client draws them over
//! it at runtime, so [`Minimap::icons`] lists them for the caller to draw.
//!
//! Scenery that is, for now, something else (an opened door, a felled tree's stump) is drawn as it
//! stands by [`MinimapSource::render_changed`], which swaps those placements before drawing.
//!
//! Faithfulness notes are in the README ("Minimap").

use crate::config::{Configs, ObjectDef, Overlay, Underlay};
use crate::locs::Placement;
use crate::model::hsl_to_rgb;
use crate::terrain::REGION;
use crate::terrain_mesh::{
    pack_hsl, rgb_to_hsl_bytes, underlay_hsl, PlacedObjects, TerrainGrid, UnderlayHsl, INVISIBLE_RGB,
};
use crate::textures::Texture;
use crate::{idx, Cache, Result};
use std::collections::HashMap;

/// Pixels along one tile edge.
pub const PX_PER_TILE: usize = 4;
/// Pixels along one square edge.
pub const SIZE: usize = REGION * PX_PER_TILE;

/// Config group of area definitions (map function icons).
pub const AREA_GROUP: u32 = 35;

const WALL: [u8; 3] = [0xEE, 0xEE, 0xEE];
const DOOR: [u8; 3] = [0xEE, 0x00, 0x00];

/// A map function icon (a bank, an altar, ...) to draw over the minimap.
#[derive(Debug, Clone, PartialEq)]
pub struct MapIcon {
    /// South-west tile of the object, world coordinates.
    pub x: i32,
    pub z: i32,
    /// Tiles the object covers (after its rotation).
    pub size_x: u8,
    pub size_z: u8,
    /// The area definition (object def opcode 82) and its sprite group in the sprites archive.
    pub area: u32,
    pub sprite: u32,
    pub name: String,
}

/// A ground-level (plane 0) placement in world tiles: what a [`Swap`] takes out or puts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MapObject {
    pub id: u32,
    pub x: i32,
    pub z: i32,
    pub kind: u8,
    pub orientation: u8,
}

/// A placement shown as another for now: the one the map has, and what stands in for it (an
/// opened door, moved and turned).
pub type Swap = (MapObject, MapObject);

pub struct Minimap {
    /// 256 x 256 RGBA, north up, x east. Pixel (px, py) covers tile
    /// `x = sx * 64 + px / 4`, `z = sz * 64 + 63 - py / 4`. Tiles with nothing to draw are transparent.
    pub image: Texture,
    pub icons: Vec<MapIcon>,
}

/// The definitions every minimap needs, decoded once.
pub struct MinimapSource<'a> {
    cache: &'a Cache,
    underlays: HashMap<u32, Underlay>,
    overlays: HashMap<u32, Overlay>,
    objects: HashMap<u32, ObjectDef>,
    /// Area id -> (sprite group, name).
    areas: HashMap<u32, (u32, String)>,
    scenes: Vec<Option<Texture>>,
}

/// Render the minimap of map square (`sx`, `sz`). Decodes the definitions on every call; use
/// [`MinimapSource`] for several squares.
pub fn render_square(cache: &Cache, sx: i32, sz: i32) -> Result<Minimap> {
    MinimapSource::new(cache)?.render(sx, sz)
}

/// Area definition: the sprite (opcode 1) and name (opcode 3). Everything we skip has a fixed size.
fn decode_area(d: &[u8]) -> Option<(Option<u32>, String)> {
    let mut b = crate::buf::Buf::new(d);
    let (mut sprite, mut name) = (None, String::new());
    while b.pos < d.len() {
        match b.u8() {
            0 => break,
            1 => sprite = Some(b.u16() as u32),
            2 => {
                b.u16();
            }
            3 => name = b.string(),
            4 | 5 => {
                b.u24();
            }
            6 | 7 | 8 => {
                b.u8();
            }
            10..=14 | 17 => {
                b.string();
            }
            19 => {
                b.u16();
            }
            _ => break,
        }
    }
    Some((sprite, name))
}

/// Every map function (area definition with an icon): area id -> (sprite group, name). An object
/// definition's map icon (opcode 82) names one of these.
pub fn map_functions(cache: &Cache) -> Result<HashMap<u32, (u32, String)>> {
    let configs = Configs::new(cache)?;
    let mut areas = HashMap::new();
    for (id, d) in configs.group_files(AREA_GROUP).unwrap_or_default() {
        if let Some((Some(sprite), name)) = decode_area(&d) {
            areas.insert(id, (sprite, name));
        }
    }
    Ok(areas)
}

impl<'a> MinimapSource<'a> {
    pub fn new(cache: &'a Cache) -> Result<MinimapSource<'a>> {
        let configs = Configs::new(cache)?;
        let areas = map_functions(cache)?;
        let sprites = cache.index(idx::SPRITES)?;
        let mut scenes = Vec::new();
        if let Some(group) = sprites.group_by_name("mapscene") {
            // one file holding every frame
            if let Some((_, f)) = cache.read_files(&sprites, group.id)?.first() {
                scenes = crate::font::decode_frames(f)
                    .into_iter()
                    .map(|f| {
                        Some(Texture {
                            width: f.width as u32,
                            height: f.height as u32,
                            has_alpha: true,
                            rgba: f.rgba,
                            scroll_dir: 0,
                            scroll_speed: 0,
                        })
                    })
                    .collect();
            }
        }
        Ok(MinimapSource {
            cache,
            underlays: configs.underlays()?,
            overlays: configs.overlays()?,
            objects: configs.objects()?,
            areas,
            scenes,
        })
    }

    pub fn render(&self, sx: i32, sz: i32) -> Result<Minimap> {
        self.render_changed(sx, sz, &[])
    }

    /// Like [`Self::render`], with each swap's original placement taken out and its replacement
    /// put in, so walls, doors and map scenes are drawn where they stand now. A swap whose
    /// original is not on the map still puts its replacement in.
    pub fn render_changed(&self, sx: i32, sz: i32, swaps: &[Swap]) -> Result<Minimap> {
        let grid = TerrainGrid::load(
            self.cache,
            sx - 1,
            sz - 1,
            3,
            3,
            self.underlays.clone(),
            self.overlays.clone(),
        )?;
        let mut placed = PlacedObjects::load(self.cache, &grid)?;
        let (ox, oz) = (grid.region_x * 64, grid.region_y * 64);
        for (from, to) in swaps {
            let is_from = |(gx, gz, p): &(i32, i32, Placement)| {
                p.plane == 0
                    && (gx + ox, gz + oz) == (from.x, from.z)
                    && (p.id, p.kind, p.orientation) == (from.id, from.kind, from.orientation)
            };
            placed.placements.retain(|placement| !is_from(placement));
            let (gx, gz) = (to.x - ox, to.z - oz);
            let local = |g: i32| g.rem_euclid(REGION as i32) as u8;
            let p = Placement { id: to.id, plane: 0, x: local(gx), z: local(gz), kind: to.kind, orientation: to.orientation };
            placed.placements.push((gx, gz, p));
        }
        Ok(self.render_from(&grid, &placed))
    }

    /// Draws the middle square of a 3x3 grid (as built by [`crate::squares::MapSource`]).
    pub fn render_from(&self, grid: &TerrainGrid, placed: &PlacedObjects) -> Minimap {
        let mut img = vec![0u8; SIZE * SIZE * 4];
        let blend0 = blend_plane(grid, 0);
        let blend1 = blend_plane(grid, 1);
        let mid = REGION;
        for lx in 0..REGION {
            for lz in 0..REGION {
                let (x, z) = (mid + lx, mid + lz);
                // a bridge deck is the plane-1 tile, seen from plane 0
                let deck = grid.tile(1, x, z).settings & 2 != 0;
                let (plane, blend) = if deck { (1, &blend1) } else { (0, &blend0) };
                self.draw_tile(&mut img, grid, plane, blend, x, z, lx, lz);
            }
        }

        let mut icons = Vec::new();
        for (gx, gz, p) in &placed.placements {
            if p.plane != 0 {
                continue;
            }
            let Some(def) = self.objects.get(&p.id) else { continue };
            let (lx, lz) = (*gx - mid as i32, *gz - mid as i32);
            let inside = (0..REGION as i32).contains(&lx) && (0..REGION as i32).contains(&lz);
            if matches!(p.kind, 0..=3) && inside {
                draw_wall(&mut img, def, p.kind, p.orientation, lx as usize, lz as usize);
            }
            if inside && p.kind == 9 {
                draw_diagonal(&mut img, def, p.orientation, lx as usize, lz as usize);
            }
        }
        // map scenes go over the walls
        for (gx, gz, p) in &placed.placements {
            if p.plane != 0 || !matches!(p.kind, 0..=3 | 9..=22) {
                continue;
            }
            let Some(def) = self.objects.get(&p.id) else { continue };
            let (lx, lz) = (*gx - mid as i32, *gz - mid as i32);
            let (fx, fz) = footprint(def, p.orientation);
            if let Some(scene) = def.map_scene.and_then(|s| self.scenes.get(s as usize)).and_then(|s| s.as_ref()) {
                // client: top-left = tile origin + (footprint pixels - sprite) / 2, north edge first
                let px = lx * 4 + (fx as i32 * 4 - scene.width as i32) / 2;
                let py = (REGION as i32 - lz - fz as i32) * 4 + (fz as i32 * 4 - scene.height as i32) / 2;
                blit(&mut img, scene, px, py);
            }
        }
        for (gx, gz, p) in &placed.placements {
            if p.plane != 0 {
                continue;
            }
            let Some(def) = self.objects.get(&p.id) else { continue };
            let Some(area) = def.map_icon.filter(|&a| a != 0xFFFF) else { continue };
            let (lx, lz) = (*gx - mid as i32, *gz - mid as i32);
            if !((0..REGION as i32).contains(&lx) && (0..REGION as i32).contains(&lz)) {
                continue;
            }
            let Some((sprite, name)) = self.areas.get(&(area as u32)) else { continue };
            let (fx, fz) = footprint(def, p.orientation);
            icons.push(MapIcon {
                x: grid.region_x * 64 + *gx,
                z: grid.region_y * 64 + *gz,
                size_x: fx,
                size_z: fz,
                area: area as u32,
                sprite: *sprite,
                name: name.clone(),
            });
        }

        Minimap {
            image: Texture {
                width: SIZE as u32,
                height: SIZE as u32,
                rgba: img,
                has_alpha: true,
                scroll_dir: 0,
                scroll_speed: 0,
            },
            icons,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_tile(
        &self,
        img: &mut [u8],
        grid: &TerrainGrid,
        plane: usize,
        blend: &[Option<(i32, i32, i32)>],
        x: usize,
        z: usize,
        lx: usize,
        lz: usize,
    ) {
        let t = grid.tile(plane, x, z);
        let under = blend[x * grid.depth + z].map(|(h, s, l)| pack_hsl(h.clamp(0, 255), s.clamp(0, 255), l.clamp(0, 255)));
        let ov = if t.overlay != 0 { self.overlays.get(&(t.overlay as u32 - 1)) } else { None };
        let see_through = ov.is_some_and(|o| o.rgb == INVISIBLE_RGB && o.texture_hsl.is_none());
        // the overlay's minimap colour: secondary if it has one, else its colour or texture average
        let over = ov.map(|o| {
            let hsl = if let Some(sec) = o.secondary_rgb {
                let (h, s, l) = rgb_to_hsl_bytes(sec);
                pack_hsl(h, s, l)
            } else if let Some(h) = o.texture_hsl {
                h
            } else {
                let (h, s, l) = rgb_to_hsl_bytes(o.rgb);
                pack_hsl(h, s, l)
            };
            (hsl, o.hide_underlay)
        });
        if under.is_none() && (over.is_none() || see_through) {
            return;
        }
        let (shape, rot) = if over.is_some() { ((t.shape as usize).min(12), t.rotation as i32) } else { (0, 0) };
        let mask = shape_mask(shape, rot);
        for py in 0..PX_PER_TILE {
            for px in 0..PX_PER_TILE {
                let flag = mask[py * PX_PER_TILE + px];
                if flag == 2 {
                    continue;
                }
                if see_through && (flag == 0 || under.is_none()) {
                    continue;
                }
                let hsl = match (&over, under, flag) {
                    (None, Some(u), _) => u,
                    (None, None, _) => continue,
                    (Some((o, _)), _, 0) => *o,
                    (Some((o, hide)), u, _) => match u {
                        Some(u) if !*hide || shape == 1 => u,
                        _ => *o,
                    },
                };
                let rgb = minimap_rgb(hsl);
                put(img, lx * 4 + px, (REGION - 1 - lz) * 4 + py, rgb);
            }
        }
    }
}

/// Whether the client treats this object as interactive (and so colours its wall red): opcode 19
/// says so, else it has menu options, else it is a typed-10 model.
pub fn is_interactive(def: &ObjectDef) -> bool {
    match def.interactive_flag {
        -1 => {
            !def.actions.is_empty()
                || (!def.models.is_empty() && (def.model_types.is_empty() || def.model_types[0] == 10))
        }
        f => f == 1,
    }
}

fn footprint(def: &ObjectDef, orientation: u8) -> (u8, u8) {
    let (x, z) = (def.size_x.max(1), def.size_y.max(1));
    if orientation & 1 == 1 { (z, x) } else { (x, z) }
}

fn put(img: &mut [u8], x: usize, y: usize, rgb: [u8; 3]) {
    if x < SIZE && y < SIZE {
        let o = (y * SIZE + x) * 4;
        img[o..o + 4].copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
    }
}

fn put_i(img: &mut [u8], x: i32, y: i32, rgb: [u8; 3]) {
    if x >= 0 && y >= 0 {
        put(img, x as usize, y as usize, rgb);
    }
}

fn blit(img: &mut [u8], sprite: &Texture, x0: i32, y0: i32) {
    for sy in 0..sprite.height as i32 {
        for sx in 0..sprite.width as i32 {
            let o = (sy as usize * sprite.width as usize + sx as usize) * 4;
            if sprite.rgba[o + 3] == 0 {
                continue;
            }
            put_i(img, x0 + sx, y0 + sy, [sprite.rgba[o], sprite.rgba[o + 1], sprite.rgba[o + 2]]);
        }
    }
}

/// Walls: kind 0 one edge, 2 two edges (the edge and the next turn), 1 and 3 a corner pixel.
/// Orientation 0 = west edge / north-west corner, then clockwise.
fn draw_wall(img: &mut [u8], def: &ObjectDef, kind: u8, orientation: u8, lx: usize, lz: usize) {
    let colour = if is_interactive(def) { DOOR } else { WALL };
    let (px, py) = (lx as i32 * 4, (REGION - 1 - lz) as i32 * 4);
    let edge = |img: &mut [u8], o: u8| {
        for i in 0..4 {
            match o & 3 {
                0 => put_i(img, px, py + i, colour),
                1 => put_i(img, px + i, py, colour),
                2 => put_i(img, px + 3, py + i, colour),
                _ => put_i(img, px + i, py + 3, colour),
            }
        }
    };
    match kind {
        0 => edge(img, orientation),
        2 => {
            edge(img, orientation);
            edge(img, orientation + 1);
        }
        _ => {
            let (cx, cy) = match orientation & 3 {
                0 => (0, 0),
                1 => (3, 0),
                2 => (3, 3),
                _ => (0, 3),
            };
            put_i(img, px + cx, py + cy, colour);
        }
    }
}

/// Diagonal walls (kind 9): a four-pixel diagonal.
fn draw_diagonal(img: &mut [u8], def: &ObjectDef, orientation: u8, lx: usize, lz: usize) {
    let colour = if is_interactive(def) { DOOR } else { WALL };
    let (px, py) = (lx as i32 * 4, (REGION - 1 - lz) as i32 * 4);
    for i in 0..4 {
        if orientation & 1 == 0 {
            put_i(img, px + i, py + 3 - i, colour); // south-west to north-east
        } else {
            put_i(img, px + i, py + i, colour); // north-west to south-east
        }
    }
}

/// Pixel colour of a packed HSL colour at the minimap's fixed lightness.
fn minimap_rgb(hsl: u16) -> [u8; 3] {
    let l = ((96 * (hsl & 127) as i32) / 128).clamp(2, 126) as u16;
    let c = hsl_to_rgb((hsl & !127) | l, 0.8);
    [(c[0] * 256.0).min(255.0) as u8, (c[1] * 256.0).min(255.0) as u8, (c[2] * 256.0).min(255.0) as u8]
}

/// 11x11 box-blended underlay HSL per tile of `plane`, `[x * depth + z]`, for the middle square.
fn blend_plane(grid: &TerrainGrid, plane: usize) -> Vec<Option<(i32, i32, i32)>> {
    let ul: HashMap<u32, UnderlayHsl> = grid.underlays.iter().map(|(&id, u)| (id, underlay_hsl(u.rgb))).collect();
    let (w, d) = (grid.width as i32, grid.depth as i32);
    let mut out = vec![None; (w * d) as usize];
    let r = 5i32;
    let m = REGION as i32;
    for x in m..2 * m {
        for z in m..2 * m {
            let t = grid.tile(plane, x as usize, z as usize);
            if t.underlay == 0 {
                continue;
            }
            let (mut hue, mut sat, mut light, mut mult, mut count) = (0i64, 0i64, 0i64, 0i64, 0i64);
            for xx in (x - r).max(0)..=(x + r).min(w - 1) {
                for zz in (z - r).max(0)..=(z + r).min(d - 1) {
                    let n = grid.tile(plane, xx as usize, zz as usize);
                    if n.underlay == 0 {
                        continue;
                    }
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
                out[(x * d + z) as usize] = Some(((hue * 256 / mult) as i32, (sat / count) as i32, (light / count) as i32));
            }
        }
    }
    out
}

// --- overlay shape masks -------------------------------------------------------------------

/// Vertex ids per shape and triangles, as in `terrain_mesh` (see there for the numbering).
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

/// Tile-space position (0..128, x east, z north) of vertex id `v`.
fn vertex_pos(v: i32) -> (f32, f32) {
    let (q, h, tq, t) = (32.0, 64.0, 96.0, 128.0);
    match v {
        1 => (0.0, 0.0),
        2 => (h, 0.0),
        3 => (t, 0.0),
        4 => (t, h),
        5 => (t, t),
        6 => (h, t),
        7 => (0.0, t),
        8 => (0.0, h),
        9 => (h, q),
        10 => (tq, h),
        11 => (h, tq),
        12 => (q, h),
        13 => (q, q),
        14 => (tq, q),
        15 => (tq, tq),
        _ => (q, tq),
    }
}

/// Which colour each of a tile's 4x4 pixels (row 0 = north) takes: 0 overlay, 1 underlay, 2 nothing.
/// Pixels are sampled at their centres against the shape's triangles; on a shared edge the overlay wins.
fn shape_mask(shape: usize, rot: i32) -> [u8; 16] {
    let verts: Vec<(f32, f32)> = SHAPE_VERTICES[shape]
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
            vertex_pos(v)
        })
        .collect();
    let mut mask = [2u8; 16];
    for tri in SHAPE_TRIANGLES[shape].chunks_exact(4) {
        let pts: Vec<(f32, f32)> = tri[1..4]
            .iter()
            .map(|&ix| {
                let mut i = ix as i32;
                if i < 4 {
                    i = (i - rot) & 3;
                }
                verts[i as usize]
            })
            .collect();
        for py in 0..4 {
            for px in 0..4 {
                let p = ((px as f32 + 0.5) * 32.0, (3.5 - py as f32) * 32.0);
                if inside(p, &pts) {
                    let cell = &mut mask[py * 4 + px];
                    if tri[0] == 0 || *cell == 2 {
                        *cell = tri[0];
                    }
                }
            }
        }
    }
    mask
}

fn inside(p: (f32, f32), t: &[(f32, f32)]) -> bool {
    let e = |a: (f32, f32), b: (f32, f32)| (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0);
    let (d0, d1, d2) = (e(t[0], t[1]), e(t[1], t[2]), e(t[2], t[0]));
    (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_overlay_covers_the_whole_tile() {
        assert_eq!(shape_mask(0, 0), [0; 16]);
    }

    #[test]
    fn a_half_tile_shape_splits_the_pixels() {
        // shape 3 is an overlay triangle against an underlay triangle
        let m = shape_mask(3, 0);
        assert!(m.iter().any(|&p| p == 0) && m.iter().any(|&p| p == 1), "{m:?}");
        // rotating it by half a turn flips which corner holds the overlay
        assert_ne!(shape_mask(3, 0), shape_mask(3, 2));
    }

    #[test]
    fn colours_are_the_fixed_lightness_of_the_hsl() {
        // 96/128 of the lightness: a mid grey (lightness 64) is darker than the same grey at 100
        let a = minimap_rgb(64);
        let b = minimap_rgb(100);
        assert!(a[0] < b[0]);
    }
}
