//! The 36x32 inventory picture of an item: its model, lit like any scene object, seen through the
//! definition's own camera (distance, pitch, yaw, roll, offsets), with the game's black outline and
//! dark drop shadow.

use crate::item::ItemDef;
use crate::model::Model;
use crate::scene::{build_models, LocMesh, MeshData, Shape};
use crate::textures::Texture;
use crate::{idx, Cache};
use std::collections::HashMap;
use std::rc::Rc;

pub const ICON_WIDTH: usize = 36;
pub const ICON_HEIGHT: usize = 32;
/// The screen point the model's centre lands on, in icon pixels.
const CENTRE: (f32, f32) = (16.0, 16.0);
/// The scene's projection constant.
const FOCAL: f32 = 512.0;
const SHADOW: [u8; 3] = [0x30, 0x20, 0x20];

/// Builds the lit mesh the icon is drawn from.
pub fn icon_mesh(cache: &Cache, def: &ItemDef, texture_hsl: &HashMap<u32, u16>) -> Option<LocMesh> {
    let model = def.inventory_model;
    if model == 0 {
        return None;
    }
    let mut load = |id: u32| -> Option<Rc<Model>> {
        let data = cache.read_group(idx::MODELS, id).ok()?;
        Model::decode(id, &data).ok().map(Rc::new)
    };
    let shape = Shape {
        recolor_from: &def.recolor_from,
        recolor_to: &def.recolor_to,
        size: def.resize,
        offset: [0, 0, 0],
        ambient: 64.0 + def.ambient as f32,
        contrast: 768.0 + def.contrast as f32,
        light: (-50.0, -10.0, -50.0),
        keep_rig: false,
    };
    build_models(&mut load, &[model], &shape, false, false, 0, 10, (0, 0), texture_hsl)
}

fn linear_to_srgb8(c: f32) -> u8 {
    (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0 + 0.5) as u8
}

/// `degrees` in the game's 2048-unit turn to (sin, cos).
fn angle(units: i32) -> (f32, f32) {
    let radians = units as f32 * std::f32::consts::TAU / 2048.0;
    radians.sin_cos()
}

struct Canvas {
    rgba: Vec<u8>,
    depth: Vec<f32>,
}

/// Draws `mesh` through the camera of `def` into a fresh icon canvas.
fn draw(mesh: &LocMesh, def: &ItemDef) -> Canvas {
    let mut canvas = Canvas { rgba: vec![0; ICON_WIDTH * ICON_HEIGHT * 4], depth: vec![f32::MAX; ICON_WIDTH * ICON_HEIGHT] };

    // the model's highest point, for centring (the game's model height)
    let mut height: f32 = 0.0;
    for part in [&mesh.opaque, &mesh.translucent] {
        for p in &part.positions {
            height = height.max(p[1] * 128.0);
        }
    }

    let (sin_yaw, cos_yaw) = angle(def.yaw);
    let (sin_pitch, cos_pitch) = angle(def.pitch);
    let (sin_roll, cos_roll) = angle(def.roll);
    let zoom = def.zoom.max(1) as f32;
    let tx = def.offset_x as f32;
    let ty = zoom * sin_pitch + height / 2.0 + def.offset_y as f32;
    let tz = zoom * cos_pitch + def.offset_y as f32;

    // model units, y down
    let to_screen = |p: [f32; 3]| -> [f32; 3] {
        let (mut x, mut y, mut z) = (p[0] * 128.0, -p[1] * 128.0, -p[2] * 128.0);
        // roll, yaw, pitch
        let (rx, ry) = (x * cos_roll + y * sin_roll, y * cos_roll - x * sin_roll);
        x = rx;
        y = ry;
        let (yx, yz) = (x * cos_yaw + z * sin_yaw, z * cos_yaw - x * sin_yaw);
        x = yx;
        z = yz;
        // the camera offset is applied before the pitch, which turns it into a pure distance
        let (x, y, z) = (x + tx, y + ty, z + tz);
        let (py, pz) = (y * cos_pitch - z * sin_pitch, y * sin_pitch + z * cos_pitch);
        let (y, z) = (py, pz);
        let z = z.max(1.0);
        [CENTRE.0 + x * FOCAL / z, CENTRE.1 + y * FOCAL / z, z]
    };

    let mut fill = |part: &MeshData, blend: bool| {
        for tri in part.indices.chunks_exact(3) {
            let v: Vec<[f32; 3]> = tri.iter().map(|i| to_screen(part.positions[*i as usize])).collect();
            let c: Vec<[f32; 4]> = tri.iter().map(|i| part.colors[*i as usize]).collect();
            // winding as the game does: only faces turned toward the camera are drawn
            let area = (v[0][0] - v[1][0]) * (v[2][1] - v[1][1]) - (v[0][1] - v[1][1]) * (v[2][0] - v[1][0]);
            if area <= 0.0 {
                continue;
            }
            let (min_x, max_x) = (
                v.iter().map(|p| p[0]).fold(f32::MAX, f32::min).floor().max(0.0) as usize,
                v.iter().map(|p| p[0]).fold(f32::MIN, f32::max).ceil().min(ICON_WIDTH as f32 - 1.0).max(0.0) as usize,
            );
            let (min_y, max_y) = (
                v.iter().map(|p| p[1]).fold(f32::MAX, f32::min).floor().max(0.0) as usize,
                v.iter().map(|p| p[1]).fold(f32::MIN, f32::max).ceil().min(ICON_HEIGHT as f32 - 1.0).max(0.0) as usize,
            );
            for y in min_y..=max_y {
                for x in min_x..=max_x {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    // barycentric weights
                    let d = (v[1][1] - v[2][1]) * (v[0][0] - v[2][0]) + (v[2][0] - v[1][0]) * (v[0][1] - v[2][1]);
                    if d.abs() < 1e-6 {
                        continue;
                    }
                    let w0 = ((v[1][1] - v[2][1]) * (px - v[2][0]) + (v[2][0] - v[1][0]) * (py - v[2][1])) / d;
                    let w1 = ((v[2][1] - v[0][1]) * (px - v[2][0]) + (v[0][0] - v[2][0]) * (py - v[2][1])) / d;
                    let w2 = 1.0 - w0 - w1;
                    const EDGE: f32 = -0.02;
                    if w0 < EDGE || w1 < EDGE || w2 < EDGE {
                        continue;
                    }
                    let z = w0 * v[0][2] + w1 * v[1][2] + w2 * v[2][2];
                    let at = y * ICON_WIDTH + x;
                    if z >= canvas.depth[at] {
                        continue;
                    }
                    let mut rgb = [0f32; 3];
                    let mut alpha = 0.0;
                    for k in 0..3 {
                        let w = [w0, w1, w2][k];
                        for ch in 0..3 {
                            rgb[ch] += c[k][ch] * w;
                        }
                        alpha += c[k][3] * w;
                    }
                    let o = at * 4;
                    if blend && alpha < 0.999 {
                        for ch in 0..3 {
                            let src = linear_to_srgb8(rgb[ch]) as f32;
                            let dst = canvas.rgba[o + ch] as f32;
                            canvas.rgba[o + ch] = (src * alpha + dst * (1.0 - alpha)) as u8;
                        }
                        canvas.rgba[o + 3] = canvas.rgba[o + 3].max((alpha * 255.0) as u8);
                    } else {
                        for ch in 0..3 {
                            canvas.rgba[o + ch] = linear_to_srgb8(rgb[ch]);
                        }
                        canvas.rgba[o + 3] = 255;
                        canvas.depth[at] = z;
                    }
                }
            }
        }
    };
    fill(&mesh.opaque, false);
    fill(&mesh.translucent, true);
    canvas
}

/// The finished icon: the model with a black outline and, below and to the left of it, the shadow.
pub fn render_icon(mesh: &LocMesh, def: &ItemDef) -> Texture {
    let canvas = draw(mesh, def);
    let (w, h) = (ICON_WIDTH, ICON_HEIGHT);
    let solid = |x: i32, y: i32| -> bool {
        x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && canvas.rgba[(y as usize * w + x as usize) * 4 + 3] > 0
    };
    let mut out = canvas.rgba.clone();
    // outline: empty pixels touching the model
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            if solid(x, y) {
                continue;
            }
            let o = (y as usize * w + x as usize) * 4;
            let touching = solid(x - 1, y) || solid(x + 1, y) || solid(x, y - 1) || solid(x, y + 1);
            if touching {
                out[o..o + 4].copy_from_slice(&[0, 0, 0, 255]);
            } else if solid(x - 1, y - 1) {
                // drop shadow, one pixel down and to the right of the model
                out[o..o + 3].copy_from_slice(&SHADOW);
                out[o + 3] = 255;
            }
        }
    }
    Texture { width: w as u32, height: h as u32, rgba: out, has_alpha: true, scroll_dir: 0, scroll_speed: 0 }
}

/// Convenience: mesh and picture in one go.
pub fn item_icon(cache: &Cache, def: &ItemDef, texture_hsl: &HashMap<u32, u16>) -> Option<Texture> {
    icon_mesh(cache, def, texture_hsl).map(|mesh| render_icon(&mesh, def))
}

/// What the mesh holds, for debugging a picture that comes out empty.
pub fn describe(mesh: &LocMesh) -> String {
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for p in &mesh.opaque.positions {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    format!(
        "opaque {} tris, translucent {}, textured groups {}, bounds {:?}..{:?}, first colour {:?}",
        mesh.opaque.indices.len() / 3,
        mesh.translucent.indices.len() / 3,
        mesh.textured.len(),
        lo,
        hi,
        mesh.opaque.colors.first()
    )
}
