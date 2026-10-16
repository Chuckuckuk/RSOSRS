//! Small pictures of NPC models for the model picker, drawn on the CPU: the model in its bind pose,
//! seen from the front and a little to its left, its triangles filled with their lit vertex
//! colours over a depth buffer. Textured faces show their grey shading.

use bevy::prelude::*;
use bevy_egui::egui;
use osrs_cache::config::Configs;
use osrs_cache::npc::{build_npc, NpcDef};
use osrs_cache::scene::{LocMesh, MeshData};
use osrs_cache::Cache;
use std::collections::HashMap;

/// Pixels along a thumbnail's side.
pub const SIZE: usize = 96;

/// How many thumbnails are drawn per frame, so opening a picker of many models never stalls.
const PER_FRAME: usize = 3;

#[derive(Resource)]
pub struct NpcThumbs {
    cache: &'static Cache,
    texture_hsl: HashMap<u32, u16>,
    /// Drawn pictures by NPC id (`None`: the model could not be built). Kept for the whole run.
    pictures: HashMap<u32, Option<Vec<u8>>>,
    /// The pictures uploaded to the egui context showing them; a texture belongs to one context,
    /// so these go when the picker window does.
    textures: HashMap<u32, egui::TextureHandle>,
    /// Pictures that may still be drawn this frame.
    budget: usize,
}

impl NpcThumbs {
    pub fn new(cache: &'static Cache) -> NpcThumbs {
        let texture_hsl = Configs::new(cache).and_then(|c| c.texture_average_hsl()).unwrap_or_default();
        NpcThumbs { cache, texture_hsl, pictures: HashMap::new(), textures: HashMap::new(), budget: PER_FRAME }
    }

    /// Call once a frame, before [`NpcThumbs::get`]: a few more pictures may be drawn.
    pub fn begin_frame(&mut self) {
        self.budget = PER_FRAME;
    }

    /// Forgets the uploaded textures (their context is gone); the pictures stay.
    pub fn forget_textures(&mut self) {
        self.textures.clear();
    }

    /// The thumbnail of `def` in `ctx`, if it is drawn; `None` while it waits its turn or when the
    /// model cannot be built.
    pub fn get(&mut self, ctx: &egui::Context, def: &NpcDef) -> Option<egui::TextureHandle> {
        if let Some(texture) = self.textures.get(&def.id) {
            return Some(texture.clone());
        }
        if !self.pictures.contains_key(&def.id) {
            if self.budget == 0 {
                ctx.request_repaint();
                return None;
            }
            self.budget -= 1;
            let picture = build_npc(self.cache, def, &self.texture_hsl).map(|mesh| render(&mesh, SIZE));
            self.pictures.insert(def.id, picture);
        }
        let rgba = self.pictures.get(&def.id)?.as_ref()?;
        let texture =
            ctx.load_texture(format!("npc{}", def.id), egui::ColorImage::from_rgba_unmultiplied([SIZE, SIZE], rgba), egui::TextureOptions::LINEAR);
        self.textures.insert(def.id, texture.clone());
        Some(texture)
    }
}

/// Linear to sRGB, one channel, 0..=255.
fn srgb(c: f32) -> u8 {
    (Srgba::gamma_function_inverse(c.clamp(0.0, 1.0)) * 255.0 + 0.5) as u8
}

/// `mesh` drawn into a `size` x `size` RGBA picture (transparent where it is not).
pub fn render(mesh: &LocMesh, size: usize) -> Vec<u8> {
    // a three-quarter view: the model turned a little, so its depth shows
    let (sin, cos) = 0.45f32.sin_cos();
    let view = |p: [f32; 3]| -> [f32; 3] { [p[0] * cos - p[2] * sin, p[1], p[0] * sin + p[2] * cos] };
    let parts: Vec<&MeshData> = [&mesh.opaque, &mesh.translucent].into_iter().chain(mesh.textured.values()).collect();

    let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
    for part in &parts {
        for &p in &part.positions {
            let v = view(p);
            lo = [lo[0].min(v[0]), lo[1].min(v[1])];
            hi = [hi[0].max(v[0]), hi[1].max(v[1])];
        }
    }
    let mut rgba = vec![0u8; size * size * 4];
    if lo[0] > hi[0] {
        return rgba;
    }
    // fit the larger side into the picture with a small margin, centred
    let span = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1e-3);
    let scale = (size as f32 - 6.0) / span;
    let (cx, cy) = ((lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0);
    let to_screen = |v: [f32; 3]| -> [f32; 3] {
        [size as f32 / 2.0 + (v[0] - cx) * scale, size as f32 / 2.0 - (v[1] - cy) * scale, v[2]]
    };

    let mut depth = vec![f32::MIN; size * size];
    for part in &parts {
        for tri in part.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
            let (pa, pb, pc) = (to_screen(view(part.positions[a])), to_screen(view(part.positions[b])), to_screen(view(part.positions[c])));
            let (ca, cb, cc) = (part.colors[a], part.colors[b], part.colors[c]);
            let area = (pb[0] - pa[0]) * (pc[1] - pa[1]) - (pb[1] - pa[1]) * (pc[0] - pa[0]);
            if area.abs() < 1e-6 {
                continue;
            }
            let x0 = pa[0].min(pb[0]).min(pc[0]).floor().max(0.0) as usize;
            let x1 = (pa[0].max(pb[0]).max(pc[0]).ceil() as usize).min(size - 1);
            let y0 = pa[1].min(pb[1]).min(pc[1]).floor().max(0.0) as usize;
            let y1 = (pa[1].max(pb[1]).max(pc[1]).ceil() as usize).min(size - 1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((pb[0] - px) * (pc[1] - py) - (pb[1] - py) * (pc[0] - px)) / area;
                    let w1 = ((pc[0] - px) * (pa[1] - py) - (pc[1] - py) * (pa[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }
                    // nearer the viewer is larger z (the model faces +z)
                    let z = w0 * pa[2] + w1 * pb[2] + w2 * pc[2];
                    let i = y * size + x;
                    if z <= depth[i] {
                        continue;
                    }
                    let mix = |k: usize| w0 * ca[k] + w1 * cb[k] + w2 * cc[k];
                    let alpha = mix(3).clamp(0.0, 1.0);
                    let o = i * 4;
                    if alpha >= 0.99 {
                        depth[i] = z;
                        rgba[o..o + 4].copy_from_slice(&[srgb(mix(0)), srgb(mix(1)), srgb(mix(2)), 255]);
                    } else {
                        // a translucent face blends over what is behind it
                        for k in 0..3 {
                            let under = rgba[o + k] as f32;
                            rgba[o + k] = (under * (1.0 - alpha) + srgb(mix(k)) as f32 * alpha) as u8;
                        }
                        rgba[o + 3] = rgba[o + 3].max((alpha * 255.0) as u8);
                    }
                }
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_triangle_fills_the_middle_of_the_picture_and_leaves_the_corners_clear() {
        let tri = MeshData {
            positions: vec![[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 2.0, 0.0]],
            colors: vec![[1.0, 0.0, 0.0, 1.0]; 3],
            indices: vec![0, 1, 2],
            ..Default::default()
        };
        let mesh = LocMesh { rig: None, opaque: tri, translucent: MeshData::default(), textured: HashMap::new() };
        let px = render(&mesh, 32);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(at(16, 20), &[255, 0, 0, 255]);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(31, 0)[3], 0);
    }
}
