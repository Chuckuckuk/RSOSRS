//! The minimap (content type 1338) and the compass (1339).
//!
//! The picture of a map square (`osrs_cache::minimap`, 4 pixels a tile) is decoded on a worker
//! thread, the first time its square comes near, and kept. Each time the interface is drawn the
//! minimap component is filled by looking up, for every pixel inside its mask sprite, the map
//! pixel it stands for after turning the map by the camera's yaw around the player's exact
//! position. (The mask sprite is opaque outside the round window and transparent inside it.) Map icons (banks, altars...) and the dots (NPCs yellow, the player white in the
//! middle) go on top, upright. The compass is the cache's compass sprite turned by the same yaw.
//!
//! Turning: with the camera at yaw `y` the screen's up is the world direction (-sin y, cos y)
//! (x east, z north) and the screen's right is (cos y, sin y).

use osrs_cache::font::decode_frames;
use osrs_cache::minimap::{Minimap, MinimapSource, SIZE};
use osrs_cache::textures::Texture;
use osrs_cache::ui::{Assets, Canvas, Rect};
use osrs_cache::{idx, Cache};
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

/// Pixels along one tile edge.
const PX: f32 = 4.0;
/// Tiles of map around the player that are kept ready: the minimap shows 19 tiles each way.
const REACH: f32 = 22.0;
/// The compass sprite (group "compass").
const COMPASS_SPRITE: i32 = 169;
/// Frames of the `mapdots` sprite.
const DOT_NPC: usize = 1;
const DOT_PLAYER: usize = 2;
/// Where the compass sprite's picture starts being used: its size is 51.
const MISSING: u32 = 0x000000;

/// Where the minimap is centred and which way it is turned.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MapParams {
    /// World position in tiles (x east, z north), fractional.
    pub x: f32,
    pub z: f32,
    pub yaw: f32,
}

impl MapParams {
    /// The position and angle in the steps that make a visible difference (a pixel, half a degree).
    pub fn quantised(&self) -> (i32, i32, i32) {
        ((self.x * PX) as i32, (self.z * PX) as i32, (self.yaw * 114.6) as i32)
    }

    /// The world tile under a minimap pixel `dx` right and `dy` down of the centre.
    pub fn tile_at(&self, dx: f32, dy: f32) -> (f32, f32) {
        let (s, c) = self.yaw.sin_cos();
        (self.x + (dx * c + dy * s) / PX, self.z + (dx * s - dy * c) / PX)
    }

    /// The pixel offset from the centre of a world position (the inverse of `tile_at`).
    pub fn offset_of(&self, x: f32, z: f32) -> (f32, f32) {
        let (s, c) = self.yaw.sin_cos();
        let (wx, wz) = (x - self.x, z - self.z);
        (PX * (wx * c + wz * s), PX * (wx * s - wz * c))
    }
}

type Reply = ((i32, i32), Option<Minimap>);

/// The minimap pictures, and what is drawn on them.
pub struct MinimapView {
    requests: Sender<(i32, i32)>,
    results: Receiver<Reply>,
    /// Squares that came back (`None`: could not be drawn).
    squares: HashMap<(i32, i32), Option<Minimap>>,
    requested: HashSet<(i32, i32)>,
    /// Frames of the `mapdots` sprite (item, NPC, player, ...).
    dots: Vec<Texture>,
    pub params: MapParams,
    pub npc_dots: Vec<(f32, f32)>,
}

impl MinimapView {
    /// Starts the thread that draws squares. It opens its own cache.
    pub fn spawn(cache: &Cache) -> MinimapView {
        let (request_tx, request_rx) = mpsc::channel::<(i32, i32)>();
        let (result_tx, result_rx) = mpsc::channel::<Reply>();
        thread::spawn(move || {
            let cache = Cache::open_default();
            let source = cache.as_ref().ok().and_then(|c| match MinimapSource::new(c) {
                Ok(s) => Some(s),
                Err(e) => {
                    eprintln!("[minimap] cannot start: {e}");
                    None
                }
            });
            for (sx, sz) in request_rx {
                let picture = source.as_ref().and_then(|s| match s.render(sx, sz) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        eprintln!("[minimap] square ({sx}, {sz}) cannot be drawn: {e}");
                        None
                    }
                });
                if result_tx.send(((sx, sz), picture)).is_err() {
                    return;
                }
            }
        });
        MinimapView {
            requests: request_tx,
            results: result_rx,
            squares: HashMap::new(),
            requested: HashSet::new(),
            dots: load_dots(cache),
            params: MapParams::default(),
            npc_dots: Vec::new(),
        }
    }

    /// Collects finished squares and asks for the ones the player is near. Returns whether
    /// anything arrived (the picture changed).
    pub fn update(&mut self, params: MapParams, npc_dots: &[(f32, f32)]) -> bool {
        self.params = params;
        self.npc_dots = npc_dots.to_vec();
        let mut arrived = false;
        while let Ok((square, picture)) = self.results.try_recv() {
            self.squares.insert(square, picture);
            arrived = true;
        }
        let (lo_x, hi_x) = (((params.x - REACH) / 64.0).floor() as i32, ((params.x + REACH) / 64.0).floor() as i32);
        let (lo_z, hi_z) = (((params.z - REACH) / 64.0).floor() as i32, ((params.z + REACH) / 64.0).floor() as i32);
        for sz in lo_z..=hi_z {
            for sx in lo_x..=hi_x {
                if self.requested.insert((sx, sz)) {
                    let _ = self.requests.send((sx, sz));
                }
            }
        }
        arrived
    }

    /// How many squares are drawn (for the fingerprint: a new one changes the picture).
    pub fn ready(&self) -> usize {
        self.squares.len()
    }

    /// The map pixel at world position (`wx`, `wz`), or `None` where there is nothing.
    fn map_pixel(&self, wx: f32, wz: f32) -> Option<[u8; 3]> {
        let (sx, sz) = ((wx / 64.0).floor() as i32, (wz / 64.0).floor() as i32);
        let picture = self.squares.get(&(sx, sz))?.as_ref()?;
        let px = (((wx - (sx * 64) as f32) * PX) as usize).min(SIZE - 1);
        let py = ((((sz * 64 + 64) as f32 - wz) * PX) as usize).min(SIZE - 1);
        let o = (py * SIZE + px) * 4;
        let rgba = &picture.image.rgba;
        (rgba[o + 3] != 0).then(|| [rgba[o], rgba[o + 1], rgba[o + 2]])
    }

    /// Draws the minimap into `rect`, only where `mask` is not empty.
    pub fn draw_map(&mut self, rect: Rect, mask: Option<&Texture>, canvas: &mut Canvas, assets: &mut dyn Assets) {
        let p = self.params;
        let (cx, cy) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
        let inside = |ox: i32, oy: i32| inside_mask(mask, rect, ox, oy);
        for oy in 0..rect.h {
            for ox in 0..rect.w {
                if !inside(ox, oy) {
                    continue;
                }
                let (wx, wz) = p.tile_at(ox as f32 + 0.5 - cx, oy as f32 + 0.5 - cy);
                let rgb = self.map_pixel(wx, wz).map_or(MISSING, |c| (c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32);
                canvas.put(rect.x + ox, rect.y + oy, rgb, 255);
            }
        }
        // map function icons, upright, centred on their object
        let mut icons = Vec::new();
        for picture in self.squares.values().flatten() {
            for ic in &picture.icons {
                let (ix, iz) = (ic.x as f32 + ic.size_x as f32 / 2.0, ic.z as f32 + ic.size_z as f32 / 2.0);
                let (dx, dy) = p.offset_of(ix, iz);
                if dx.abs() < cx + 8.0 && dy.abs() < cy + 8.0 {
                    icons.push((ic.sprite, dx, dy));
                }
            }
        }
        for (sprite, dx, dy) in icons {
            let (ox, oy) = ((cx + dx) as i32, (cy + dy) as i32);
            if !inside(ox, oy) {
                continue;
            }
            if let Some(tex) = assets.sprite(sprite as i32) {
                canvas.blit(tex, rect.x + ox - tex.width as i32 / 2, rect.y + oy - tex.height as i32 / 2, 255, false, false);
            }
        }
        for (x, z) in &self.npc_dots {
            let (dx, dy) = p.offset_of(*x, *z);
            let (ox, oy) = ((cx + dx) as i32, (cy + dy) as i32);
            if inside(ox, oy) {
                self.dot(canvas, DOT_NPC, rect.x + ox, rect.y + oy, 0xffff00);
            }
        }
        self.dot(canvas, DOT_PLAYER, rect.x + rect.w / 2, rect.y + rect.h / 2, 0xffffff);
    }

    /// A dot centred on (`x`, `y`): the `mapdots` frame, else a 3x3 square.
    fn dot(&self, canvas: &mut Canvas, frame: usize, x: i32, y: i32, fallback: u32) {
        match self.dots.get(frame) {
            Some(t) => canvas.blit(t, x - t.width as i32 / 2, y - t.height as i32 / 2, 255, false, false),
            None => canvas.fill_rect(Rect::new(x - 1, y - 1, 3, 3), fallback, 255),
        }
    }

    /// Draws the compass: the compass sprite turned by the camera's yaw, inside `mask`.
    pub fn draw_compass(&mut self, rect: Rect, mask: Option<&Texture>, canvas: &mut Canvas, assets: &mut dyn Assets) {
        let Some(src) = assets.sprite(COMPASS_SPRITE) else { return };
        let (s, c) = self.params.yaw.sin_cos();
        let (cx, cy) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
        let (sw, sh) = (src.width as f32, src.height as f32);
        for oy in 0..rect.h {
            for ox in 0..rect.w {
                if !inside_mask(mask, rect, ox, oy) {
                    continue;
                }
                // north is turned clockwise by yaw on screen: look the pixel up counter-clockwise
                let (dx, dy) = (ox as f32 + 0.5 - cx, oy as f32 + 0.5 - cy);
                let (u, v) = (dx * c + dy * s + sw / 2.0, -dx * s + dy * c + sh / 2.0);
                if u < 0.0 || v < 0.0 || u >= sw || v >= sh {
                    continue;
                }
                let o = ((v as usize * src.width as usize) + u as usize) * 4;
                let a = src.rgba[o + 3];
                if a != 0 {
                    let rgb = (src.rgba[o] as u32) << 16 | (src.rgba[o + 1] as u32) << 8 | src.rgba[o + 2] as u32;
                    canvas.put(rect.x + ox, rect.y + oy, rgb, a);
                }
            }
        }
    }
}

/// Whether the mask sprite lets the map show (its pixel is transparent) at (`ox`, `oy`) of `rect`. Without a
/// mask the whole rectangle is inside.
fn inside_mask(mask: Option<&Texture>, rect: Rect, ox: i32, oy: i32) -> bool {
    if ox < 0 || oy < 0 || ox >= rect.w || oy >= rect.h {
        return false;
    }
    let Some(m) = mask else { return true };
    let (mx, my) = (ox * m.width as i32 / rect.w, oy * m.height as i32 / rect.h);
    let o = ((my as usize * m.width as usize) + mx as usize) * 4;
    // the mask is drawn the way the client's clip masks are: the transparent part is the window
    m.rgba[o + 3] == 0
}

fn load_dots(cache: &Cache) -> Vec<Texture> {
    let Ok(sprites) = cache.index(idx::SPRITES) else { return Vec::new() };
    let Some(group) = sprites.group_by_name("mapdots") else { return Vec::new() };
    let Ok(files) = cache.read_files(&sprites, group.id) else { return Vec::new() };
    files
        .first()
        .map(|(_, f)| {
            decode_frames(f)
                .into_iter()
                .map(|f| Texture {
                    width: f.width as u32,
                    height: f.height as u32,
                    rgba: f.rgba,
                    has_alpha: true,
                    scroll_dir: 0,
                    scroll_speed: 0,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_map_turns_with_the_camera() {
        // facing north: up on the minimap is north, right is east
        let p = MapParams { x: 100.0, z: 200.0, yaw: 0.0 };
        let (x, z) = p.tile_at(0.0, -8.0);
        assert!((x - 100.0).abs() < 1e-4 && (z - 202.0).abs() < 1e-4);
        let (x, z) = p.tile_at(8.0, 0.0);
        assert!((x - 102.0).abs() < 1e-4 && (z - 200.0).abs() < 1e-4);
        // facing east (yaw -90 degrees): up is east
        let p = MapParams { yaw: -std::f32::consts::FRAC_PI_2, ..p };
        let (x, z) = p.tile_at(0.0, -8.0);
        assert!((x - 102.0).abs() < 1e-3 && (z - 200.0).abs() < 1e-3);
    }

    #[test]
    fn offset_of_undoes_tile_at() {
        let p = MapParams { x: 3222.5, z: 3218.5, yaw: 0.45 };
        let (wx, wz) = p.tile_at(13.0, -21.0);
        let (dx, dy) = p.offset_of(wx, wz);
        assert!((dx - 13.0).abs() < 1e-3 && (dy + 21.0).abs() < 1e-3);
    }
}
