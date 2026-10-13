//! Map terrain (file 0 of a map group): 4 planes x 64 x 64 tiles.
//!
//! This revision stores opcodes as u16 and overlay ids as u16 (older caches used u8).

use crate::buf::Buf;
use crate::{fmt_err, Result};

pub const REGION: usize = 64;
pub const PLANES: usize = 4;

#[derive(Debug, Clone, Copy, Default)]
pub struct Tile {
    /// Vertex height in client units (negative is up; 1 tile = 128 units horizontally).
    pub height: i32,
    /// 1-based overlay id (0 = none).
    pub overlay: u16,
    /// Overlay shape 0..=12.
    pub shape: u8,
    /// 0..=3 quarter turns.
    pub rotation: u8,
    /// Tile flag bits (1 = blocked, 2 = bridge, 4 = roof/no-render-above...).
    pub settings: u8,
    /// 1-based underlay id (0 = none).
    pub underlay: u16,
}

pub struct Terrain {
    pub tiles: Vec<Tile>, // [plane][x][z] flattened
}

impl Terrain {
    #[inline]
    pub fn idx(plane: usize, x: usize, z: usize) -> usize {
        (plane * REGION + x) * REGION + z
    }
    #[inline]
    pub fn tile(&self, plane: usize, x: usize, z: usize) -> &Tile {
        &self.tiles[Self::idx(plane, x, z)]
    }

    /// `base_x`/`base_z` are absolute world tile coordinates of the region corner
    /// (needed for the procedural noise heights of tiles with no explicit height).
    pub fn decode(data: &[u8], base_x: i32, base_z: i32) -> Result<Terrain> {
        let mut b = Buf::new(data);
        let mut tiles = vec![Tile::default(); PLANES * REGION * REGION];
        for plane in 0..PLANES {
            for x in 0..REGION {
                for z in 0..REGION {
                    let mut t = Tile::default();
                    let mut explicit: Option<i32> = None;
                    loop {
                        if b.pos >= data.len() {
                            return fmt_err("terrain data ended mid-tile");
                        }
                        let op = b.u16();
                        match op {
                            0 => break,
                            1 => {
                                let mut h = b.u8() as i32;
                                if h == 1 {
                                    h = 0;
                                }
                                explicit = Some(h * 8);
                                break;
                            }
                            2..=49 => {
                                t.overlay = b.u16();
                                t.shape = ((op - 2) / 4) as u8;
                                t.rotation = ((op - 2) & 3) as u8;
                            }
                            50..=81 => t.settings = (op - 49) as u8,
                            _ => t.underlay = op - 81,
                        }
                    }
                    t.height = match (explicit, plane) {
                        (Some(h), 0) => -h,
                        (Some(h), p) => tiles[Self::idx(p - 1, x, z)].height - h,
                        (None, 0) => -noise_height(base_x + x as i32 + 932731, base_z + z as i32 + 556238) * 8,
                        (None, p) => tiles[Self::idx(p - 1, x, z)].height - 240,
                    };
                    tiles[Self::idx(plane, x, z)] = t;
                }
            }
        }
        if b.pos + 1 < data.len() {
            return fmt_err(format!("terrain parse consumed {} of {} bytes", b.pos, data.len()));
        }
        Ok(Terrain { tiles })
    }
}

// --- procedural heights (client "perlin" noise) ---------------------------------------------

fn cosine(i: i32) -> i32 {
    (65536.0 * (i as f64 * std::f64::consts::TAU / 2048.0).cos()) as i32
}

fn interpolate(a: i32, b: i32, x: i32, scale: i32) -> i32 {
    let f = (65536 - cosine(x * 1024 / scale)) >> 1;
    ((a as i64 * (65536 - f) as i64) >> 16) as i32 + ((b as i64 * f as i64) >> 16) as i32
}

fn noise(x: i32, y: i32) -> i32 {
    let mut n = x.wrapping_add(y.wrapping_mul(57));
    n = (n << 13) ^ n;
    let t = n
        .wrapping_mul(n)
        .wrapping_mul(15731)
        .wrapping_add(789221)
        .wrapping_mul(n)
        .wrapping_add(1376312589)
        & 0x7fff_ffff;
    (t >> 19) & 255
}

fn smooth(x: i32, y: i32) -> i32 {
    let corners = noise(x - 1, y - 1) + noise(x + 1, y - 1) + noise(x - 1, y + 1) + noise(x + 1, y + 1);
    let sides = noise(x - 1, y) + noise(x + 1, y) + noise(x, y - 1) + noise(x, y + 1);
    corners / 16 + sides / 8 + noise(x, y) / 4
}

fn interp_noise(x: i32, y: i32, scale: i32) -> i32 {
    let xi = x / scale;
    let xf = x & (scale - 1);
    let yi = y / scale;
    let yf = y & (scale - 1);
    let a = smooth(xi, yi);
    let b = smooth(xi + 1, yi);
    let c = smooth(xi, yi + 1);
    let d = smooth(xi + 1, yi + 1);
    let i1 = interpolate(a, b, xf, scale);
    let i2 = interpolate(c, d, xf, scale);
    interpolate(i1, i2, yf, scale)
}

fn noise_height(x: i32, y: i32) -> i32 {
    let mut h = interp_noise(x + 45365, y + 91923, 4) - 128
        + ((interp_noise(x + 10294, y + 37821, 2) - 128) >> 1)
        + ((interp_noise(x, y, 1) - 128) >> 2);
    h = (h as f64 * 0.3) as i32 + 35;
    h.clamp(10, 60)
}
