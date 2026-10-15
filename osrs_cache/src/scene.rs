//! Builds lit, rotated meshes for placed objects (locs), following the client's `getModel` +
//! `light` pipeline: pick the model for the object type, recolour, scale/offset, rotate by the
//! placement orientation, then bake per-vertex lighting in world orientation.

use crate::config::ObjectDef;
use crate::model::{hsl_to_rgb, Model};
use std::collections::HashMap;

#[derive(Default, Clone)]
pub struct MeshData {
    /// Bevy space: x east, y up, z south; one unit = one tile. Origin is the object's anchor.
    pub positions: Vec<[f32; 3]>,
    /// Linear RGBA.
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Texture coordinates (tile-sized repeats); empty for untextured meshes.
    pub uvs: Vec<[f32; 2]>,
    /// Per output vertex: index of the merged model vertex it came from (see [`Rig`]).
    pub source: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

/// The merged model's vertices before triangles were split per face, kept so animation frames
/// can re-pose them. Positions are model units (x east, y down, z north), the space frames work in.
#[derive(Default, Clone)]
pub struct Rig {
    pub positions: Vec<[f32; 3]>,
    /// Animation label of each vertex.
    pub skin: Vec<u8>,
}

#[derive(Default, Clone)]
pub struct LocMesh {
    pub rig: Option<Rig>,
    pub opaque: MeshData,
    pub translucent: MeshData,
    /// Faces drawn with a texture image, grouped by texture id. Colours here are grey shading.
    pub textured: HashMap<u32, MeshData>,
}

impl LocMesh {
    pub fn is_empty(&self) -> bool {
        self.opaque.is_empty() && self.translucent.is_empty() && self.textured.is_empty()
    }
}

/// Bends a placed object's mesh onto the terrain, like the client's `contourGround`.
///
/// `contoured_ground` is [`ObjectDef::contoured_ground`](crate::config::ObjectDef). `ground(dx, dz)`
/// is how far the terrain surface at an offset (tiles, Bevy axes: x east, z south) from the anchor
/// sits above the placement's own height. With 0 every vertex rises by that amount, so a flat
/// decoration lies along a slope; with `n > 0` only vertices in the lowest `n / 65536` of the
/// model's height move, the lowest ones fully and fading out toward that line.
pub fn contour(mesh: &mut LocMesh, contoured_ground: i32, ground: impl Fn(f32, f32) -> f32) {
    let parts = || std::iter::once(&mesh.opaque).chain(std::iter::once(&mesh.translucent)).chain(mesh.textured.values());
    let height = parts().flat_map(|m| m.positions.iter()).fold(0.0f32, |top, p| top.max(p[1]));
    let limit = contoured_ground as f32 / 65536.0;
    let all = std::iter::once(&mut mesh.opaque).chain(std::iter::once(&mut mesh.translucent)).chain(mesh.textured.values_mut());
    for part in all {
        for p in part.positions.iter_mut() {
            let lift = ground(p[0], p[2]);
            if contoured_ground == 0 {
                p[1] += lift;
            } else if height > 0.0 {
                let up = p[1] / height;
                if up < limit {
                    p[1] += lift * (limit - up) / limit;
                }
            }
        }
    }
}

const LIGHT: (f32, f32, f32) = (-50.0, -10.0, -50.0);
const BRIGHTNESS: f32 = 0.8;

fn srgb_to_linear(c: f32) -> f32 {
    c.powf(2.2)
}

/// Model ids to draw for an object placed with the given `kind` (wall, scenery, ...).
pub fn model_ids_for(def: &ObjectDef, kind: u8) -> Vec<u32> {
    if def.model_types.is_empty() {
        // typeless models are only used for ordinary scenery (10) and diagonal scenery (11)
        if kind == 10 || kind == 11 {
            return def.models.clone();
        }
        return Vec::new();
    }
    // every wall decoration kind (4..=8) draws the type-4 model; 11 draws the scenery model
    let want = match kind {
        11 => 10,
        5..=8 => 4,
        k => k,
    };
    def.models
        .iter()
        .zip(&def.model_types)
        .filter(|(_, t)| **t == want)
        .map(|(m, _)| *m)
        .collect()
}

/// Footprint (x tiles, z tiles) after rotation.
pub fn footprint(def: &ObjectDef, orientation: u8) -> (u8, u8) {
    if orientation & 1 == 1 {
        (def.size_y.max(1), def.size_x.max(1))
    } else {
        (def.size_x.max(1), def.size_y.max(1))
    }
}

/// How far (model units, 128 to a tile) a face is lifted off its plane per rank among the faces
/// that share it (capped), and how close two planes must be to count as the same; see the lift in
/// `build_models`. A step must clear the graphics card's precision at world coordinates: at x of
/// 3200 a position in `f32` moves in steps of 0.00024 tile, about 0.03 model units.
const LIFT_PER_RANK: f32 = 0.15;
const MAX_LIFT_RANK: usize = 12;
const COPLANAR_WITHIN: f32 = 0.5;

/// Wall decoration stand-off the client uses when the tile has no wall to read it from.
pub const DEFAULT_DECOR_DISPLACEMENT: u8 = 16;

/// Unit steps for decorations that stand off from a straight wall, per orientation.
const DIR_X: [i32; 4] = [1, 0, -1, 0];
const DIR_Y: [i32; 4] = [0, -1, 0, 1];
/// Unit steps toward the facing side of a diagonal wall decoration, per orientation.
const DIAG_X: [i32; 4] = [1, -1, -1, 1];
const DIAG_Y: [i32; 4] = [-1, -1, 1, 1];

fn append(dst: &mut LocMesh, src: LocMesh) {
    fn push(dst: &mut MeshData, src: MeshData) {
        let base = dst.positions.len() as u32;
        dst.positions.extend(src.positions);
        dst.colors.extend(src.colors);
        dst.uvs.extend(src.uvs);
        dst.source.extend(src.source);
        dst.indices.extend(src.indices.into_iter().map(|i| i + base));
    }
    push(&mut dst.opaque, src.opaque);
    push(&mut dst.translucent, src.translucent);
    for (id, m) in src.textured {
        push(dst.textured.entry(id).or_default(), m);
    }
}

/// Mesh for an object placed with `kind` and `orientation` (0..=3).
///
/// L-shaped corner walls (kind 2) are two arms: the model mirrored at `orientation`, plus the
/// model unmirrored at `orientation + 1`, exactly like the client's two-model corner.
///
/// Wall decorations (4..=8) use the type-4 model; `wall_displacement` is the `decor_displacement`
/// of the wall object on the same tile (None when there is no wall).
pub fn build_loc(
    load_model: &mut dyn FnMut(u32) -> Option<std::rc::Rc<Model>>,
    def: &ObjectDef,
    kind: u8,
    orientation: u8,
    wall_displacement: Option<u8>,
    texture_hsl: &HashMap<u32, u16>,
) -> Option<LocMesh> {
    build_loc_with(load_model, def, kind, orientation, wall_displacement, texture_hsl, false)
}

/// [`build_loc`], keeping the mesh's [`Rig`] when `keep_rig` so an animation can pose it. Only
/// meaningful for a single-model placement (the rig of an object made of several arms is the
/// first one's).
pub fn build_loc_with(
    load_model: &mut dyn FnMut(u32) -> Option<std::rc::Rc<Model>>,
    def: &ObjectDef,
    kind: u8,
    orientation: u8,
    wall_displacement: Option<u8>,
    texture_hsl: &HashMap<u32, u16>,
    keep_rig: bool,
) -> Option<LocMesh> {
    let o = orientation & 3;
    // (model orientation 0..=7, offset in model units) for every model the client draws
    let parts: Vec<(u8, (i32, i32))> = match kind {
        2 => vec![(o + 4, (0, 0)), ((o + 1) & 3, (0, 0))],
        5 => {
            let d = wall_displacement.unwrap_or(DEFAULT_DECOR_DISPLACEMENT) as i32;
            vec![(o, (d * DIR_X[o as usize], d * DIR_Y[o as usize]))]
        }
        6 | 8 => {
            let d = wall_displacement.map_or(DEFAULT_DECOR_DISPLACEMENT as i32 / 2, |d| d as i32 / 2);
            let mut v = vec![(o + 4, (d * DIAG_X[o as usize], d * DIAG_Y[o as usize]))];
            if kind == 8 {
                v.push((((o + 2) & 3) + 4, (0, 0)));
            }
            v
        }
        7 => vec![(((o + 2) & 3) + 4, (0, 0))],
        _ => vec![(o, (0, 0))],
    };
    let mut out: Option<LocMesh> = None;
    for (model_orientation, offset) in parts {
        if let Some(m) = build_single(load_model, def, kind, model_orientation, offset, texture_hsl, keep_rig) {
            match &mut out {
                Some(acc) => append(acc, m),
                None => out = Some(m),
            }
        }
    }
    out
}

/// `orientation` is 0..=7: bit 2 requests a mirrored model (corner walls) or the 45-degree
/// diagonal form (wall decorations). `offset` shifts the result in model units (x east, z north).
fn build_single(
    load_model: &mut dyn FnMut(u32) -> Option<std::rc::Rc<Model>>,
    def: &ObjectDef,
    kind: u8,
    orientation: u8,
    offset: (i32, i32),
    texture_hsl: &HashMap<u32, u16>,
    keep_rig: bool,
) -> Option<LocMesh> {
    let flip = def.rotated ^ (kind == 2 && orientation > 3);
    let diagonal_decor = (4..=8).contains(&kind) && orientation > 3;
    let orientation = orientation & 3;
    let ids = model_ids_for(def, kind);
    let shape = Shape {
        recolor_from: &def.recolor_from,
        recolor_to: &def.recolor_to,
        size: [def.model_size_x, def.model_size_y, def.model_size_z],
        offset: [def.offset_x, def.offset_y, def.offset_z],
        ambient: 64.0 + def.ambient as f32,
        contrast: 768.0 + def.contrast as f32 * 5.0,
        light: LIGHT,
        keep_rig,
    };
    build_models(load_model, &ids, &shape, flip, diagonal_decor, orientation, kind, offset, texture_hsl)
}

/// How a set of models is recoloured, scaled and lit.
pub struct Shape<'a> {
    pub recolor_from: &'a [u16],
    pub recolor_to: &'a [u16],
    /// Scale in 1/128ths per axis.
    pub size: [u16; 3],
    /// Offset in model units.
    pub offset: [i16; 3],
    /// Final client ambient and contrast values (objects: 64 + a, 768 + 5c; players: 64, 850).
    pub ambient: f32,
    pub contrast: f32,
    pub light: (f32, f32, f32),
    /// Keep the merged vertices and labels in [`LocMesh::rig`] (for animated models).
    pub keep_rig: bool,
}

/// Merge `ids` into one lit mesh (the second half of the client's `getModel`): recolour,
/// scale/offset, mirror, rotate by `orientation` quarter turns, bake gouraud lighting.
/// `diagonal_decor` and `kind` select the wall-decoration / diagonal-scenery turns.
pub fn build_models(
    load_model: &mut dyn FnMut(u32) -> Option<std::rc::Rc<Model>>,
    ids: &[u32],
    shape: &Shape,
    flip: bool,
    diagonal_decor: bool,
    orientation: u8,
    kind: u8,
    offset: (i32, i32),
    texture_hsl: &HashMap<u32, u16>,
) -> Option<LocMesh> {
    if ids.is_empty() {
        return None;
    }
    let def = shape;

    // merge models
    let mut vx: Vec<f32> = Vec::new();
    let mut vy: Vec<f32> = Vec::new();
    let mut vz: Vec<f32> = Vec::new();
    struct Face {
        a: usize,
        b: usize,
        c: usize,
        hsl: u16,
        alpha: u8,
        flat: bool,
        /// texture id, or -1
        tex: i32,
        /// texture-space triangle (P, M, N) as indices into the merged vertex arrays
        pmn: [usize; 3],
        /// the model's draw priority for this face: higher is painted over lower
        priority: u8,
    }
    let mut faces: Vec<Face> = Vec::new();
    let mut skin: Vec<u8> = Vec::new();
    for &id in ids {
        let m = load_model(id)?;
        let base = vx.len();
        skin.extend((0..m.vertex_count()).map(|i| m.vertex_skin.get(i).copied().unwrap_or(0)));
        for i in 0..m.vertex_count() {
            vx.push(m.vx[i] as f32);
            vy.push(m.vy[i] as f32);
            vz.push(m.vz[i] as f32);
        }
        for f in 0..m.face_count() {
            let mut hsl = m.colors[f];
            if !def.recolor_from.is_empty() {
                for (from, to) in def.recolor_from.iter().zip(def.recolor_to) {
                    if hsl == *from {
                        hsl = *to;
                        break;
                    }
                }
            }
            if let Some(tex) = m.textures.get(f).copied().filter(|t| *t >= 0) {
                if let Some(avg) = texture_hsl.get(&(tex as u32)) {
                    hsl = *avg;
                }
            }
            if hsl == 65535 {
                continue; // hidden face
            }
            // v3 faces with render-type bit 1 but no texture are placeholder faces (colour is
            // always the white marker 0x007f); the game does not draw them.
            if m.render_type.get(f).is_some_and(|r| r & 2 != 0)
                && m.textures.get(f).is_some_and(|t| *t < 0)
                && m.version == 3
            {
                continue;
            }
            let (a, b, c) = (base + m.fa[f] as usize, base + m.fb[f] as usize, base + m.fc[f] as usize);
            let tex = m.textures.get(f).copied().unwrap_or(-1) as i32;
            let coord = m.face_coord.get(f).copied().unwrap_or(-1);
            let pmn = match m.tex_tris.get(coord.max(0) as usize) {
                Some(t) if coord >= 0 && t.iter().all(|v| (*v as usize) < m.vertex_count()) => {
                    [base + t[0] as usize, base + t[1] as usize, base + t[2] as usize]
                }
                _ => [a, b, c],
            };
            faces.push(Face {
                a,
                b,
                c,
                hsl,
                alpha: m.alpha.get(f).copied().unwrap_or(0),
                flat: m.render_type.get(f).map(|r| r & 1 == 1).unwrap_or(false),
                tex,
                pmn,
                priority: m.priority.get(f).copied().unwrap_or(0),
            });
        }
    }
    if faces.is_empty() {
        return None;
    }

    // scale and offset
    let (sx, sy, sz) = (def.size[0] as f32 / 128.0, def.size[1] as f32 / 128.0, def.size[2] as f32 / 128.0);
    let scaled = (sx - 1.0).abs() > 1e-6 || (sy - 1.0).abs() > 1e-6 || (sz - 1.0).abs() > 1e-6;
    for i in 0..vx.len() {
        if scaled {
            vx[i] *= sx;
            vy[i] *= sy;
            vz[i] *= sz;
        }
        vx[i] += def.offset[0] as f32;
        vy[i] += def.offset[1] as f32;
        vz[i] += def.offset[2] as f32;
    }

    // mirror (object flag or corner arm): negate z and swap face winding
    if flip {
        for z in vz.iter_mut() {
            *z = -*z;
        }
        for f in faces.iter_mut() {
            std::mem::swap(&mut f.b, &mut f.c);
        }
    }

    // Diagonal wall decorations: turn the model 45 degrees and slide it onto the tile centre
    // (a model hugging the west edge lands on the diagonal), before the quarter turns.
    let turn_45 = |vx: &mut [f32], vz: &mut [f32]| {
        let (s, c) = (std::f32::consts::FRAC_PI_4.sin(), std::f32::consts::FRAC_PI_4.cos());
        for i in 0..vx.len() {
            let (x, z) = (vx[i], vz[i]);
            vx[i] = x * c + z * s;
            vz[i] = -x * s + z * c;
        }
    };
    if diagonal_decor {
        turn_45(&mut vx, &mut vz);
        for i in 0..vx.len() {
            vx[i] += 45.0;
            vz[i] -= 45.0;
        }
    }

    // rotation: quarter turns, plus 45 degrees for diagonal scenery. Diagonal walls (kind 9) are
    // modelled along the diagonal already, so they only get the quarter turns.
    for _ in 0..(orientation & 3) {
        for i in 0..vx.len() {
            let x = vx[i];
            vx[i] = vz[i];
            vz[i] = -x;
        }
    }
    if kind == 11 {
        turn_45(&mut vx, &mut vz);
    }
    if offset != (0, 0) {
        for i in 0..vx.len() {
            vx[i] += offset.0 as f32;
            vz[i] += offset.1 as f32;
        }
    }

    // vertex normals (sum of face normals, unnormalised like the client) for gouraud shading
    let mut nx = vec![0.0f32; vx.len()];
    let mut ny = vec![0.0f32; vx.len()];
    let mut nz = vec![0.0f32; vx.len()];
    let face_normal = |f: &Face| -> (f32, f32, f32) {
        let (ax, ay, az) = (vx[f.b] - vx[f.a], vy[f.b] - vy[f.a], vz[f.b] - vz[f.a]);
        let (bx, by, bz) = (vx[f.c] - vx[f.a], vy[f.c] - vy[f.a], vz[f.c] - vz[f.a]);
        (ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx)
    };
    for f in &faces {
        let n = face_normal(f);
        for v in [f.a, f.b, f.c] {
            nx[v] += n.0;
            ny[v] += n.1;
            nz[v] += n.2;
        }
    }

    let (ambient, contrast, light) = (def.ambient, def.contrast, def.light);
    let llen = (light.0 * light.0 + light.1 * light.1 + light.2 * light.2).sqrt();
    let divisor = contrast * llen / 256.0;
    let intensity = |n: (f32, f32, f32)| -> f32 {
        let len = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
        if len < 1e-6 {
            return ambient;
        }
        let dot = light.0 * n.0 + light.1 * n.1 + light.2 * n.2;
        ambient + dot / (divisor * len / 256.0)
    };
    let color = |hsl: u16, light: f32, alpha: f32| -> [f32; 4] {
        let l7 = (hsl & 127) as f32;
        let l = (l7 * light / 128.0).clamp(2.0, 126.0) as u16;
        let rgb = hsl_to_rgb((hsl & !127) | l, BRIGHTNESS);
        [srgb_to_linear(rgb[0]), srgb_to_linear(rgb[1]), srgb_to_linear(rgb[2]), alpha]
    };

    // planar texture mapping: u runs P->M, v runs P->N (least-squares in the face plane)
    let uv_of = |f: &Face, v: usize| -> [f32; 2] {
        let p = f.pmn[0];
        let a = (vx[f.pmn[1]] - vx[p], vy[f.pmn[1]] - vy[p], vz[f.pmn[1]] - vz[p]);
        let b = (vx[f.pmn[2]] - vx[p], vy[f.pmn[2]] - vy[p], vz[f.pmn[2]] - vz[p]);
        let d = (vx[v] - vx[p], vy[v] - vy[p], vz[v] - vz[p]);
        let dot = |u: (f32, f32, f32), w: (f32, f32, f32)| u.0 * w.0 + u.1 * w.1 + u.2 * w.2;
        let (aa, bb, ab) = (dot(a, a), dot(b, b), dot(a, b));
        let det = aa * bb - ab * ab;
        if det.abs() < 1e-3 {
            return [0.0, 0.0];
        }
        let (da, db) = (dot(d, a), dot(d, b));
        [(da * bb - db * ab) / det, (db * aa - da * ab) / det]
    };

    // The client paints faces in order (higher priority over lower, later over earlier), so a
    // face lying exactly on another one wins by order. A depth buffer has no order and would
    // fight, so a face that shares its plane (and facing) with others is lifted off it by its
    // rank among them: priority first, then position in the model. Faces alone on their plane
    // stay put. The lifts are far below what a pixel resolves, but above the depth precision.
    let planes: Vec<Option<([f32; 3], f32)>> = faces
        .iter()
        .map(|f| {
            let (x, y, z) = face_normal(f);
            let len = (x * x + y * y + z * z).sqrt();
            (len > 1e-6).then(|| {
                let n = [x / len, y / len, z / len];
                (n, n[0] * vx[f.a] + n[1] * vy[f.a] + n[2] * vz[f.a])
            })
        })
        .collect();
    let lifts: Vec<[f32; 3]> = (0..faces.len())
        .map(|i| {
            let Some((n, d)) = planes[i] else { return [0.0; 3] };
            // faces of one colour need no order among themselves: count the distinct
            // looks painted before this one on the same plane
            let mut below: Vec<(u16, i32, u8)> = Vec::new();
            for j in 0..faces.len() {
                let Some((m, e)) = planes[j] else { continue };
                if j != i
                    && (faces[j].priority, j) < (faces[i].priority, i)
                    && n[0] * m[0] + n[1] * m[1] + n[2] * m[2] > 0.999
                    && (d - e).abs() < COPLANAR_WITHIN
                {
                    let look = (faces[j].hsl, faces[j].tex, faces[j].alpha);
                    if !below.contains(&look) {
                        below.push(look);
                    }
                }
            }
            let rank = below.len();
            let k = rank.min(MAX_LIFT_RANK) as f32 * LIFT_PER_RANK / 128.0;
            // model space has y and z the other way round from render space
            [n[0] * k, -n[1] * k, -n[2] * k]
        })
        .collect();

    let mut out = LocMesh::default();
    if shape.keep_rig {
        out.rig = Some(Rig { positions: (0..vx.len()).map(|i| [vx[i], vy[i], vz[i]]).collect(), skin });
    }
    for (index, f) in faces.iter().enumerate() {
        let up = lifts[index];
        if f.tex >= 0 {
            let lights = if f.flat {
                let l = intensity(face_normal(f));
                [l, l, l]
            } else {
                [
                    intensity((nx[f.a], ny[f.a], nz[f.a])),
                    intensity((nx[f.b], ny[f.b], nz[f.b])),
                    intensity((nx[f.c], ny[f.c], nz[f.c])),
                ]
            };
            let target = out.textured.entry(f.tex as u32).or_default();
            let base = target.positions.len() as u32;
            for (k, v) in [f.a, f.b, f.c].into_iter().enumerate() {
                target.positions.push([vx[v] / 128.0 + up[0], -vy[v] / 128.0 + up[1], -vz[v] / 128.0 + up[2]]);
                target.source.push(v as u32);
                let shade = (lights[k] / 128.0).clamp(0.05, 1.0).powf(BRIGHTNESS);
                let lin = srgb_to_linear(shade);
                target.colors.push([lin, lin, lin, 1.0]);
                target.uvs.push(uv_of(f, v));
            }
            target.indices.extend_from_slice(&[base, base + 1, base + 2]);
            continue;
        }
        let target = if f.alpha > 0 { &mut out.translucent } else { &mut out.opaque };
        let alpha = 1.0 - f.alpha as f32 / 255.0;
        let lights = if f.flat {
            let l = intensity(face_normal(f));
            [l, l, l]
        } else {
            [
                intensity((nx[f.a], ny[f.a], nz[f.a])),
                intensity((nx[f.b], ny[f.b], nz[f.b])),
                intensity((nx[f.c], ny[f.c], nz[f.c])),
            ]
        };
        let base = target.positions.len() as u32;
        for (k, v) in [f.a, f.b, f.c].into_iter().enumerate() {
            target.positions.push([vx[v] / 128.0 + up[0], -vy[v] / 128.0 + up[1], -vz[v] / 128.0 + up[2]]);
            target.source.push(v as u32);
            target.colors.push(color(f.hsl, lights[k], alpha));
        }
        target.indices.extend_from_slice(&[base, base + 1, base + 2]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_tile() -> LocMesh {
        let mut mesh = LocMesh::default();
        mesh.opaque.positions = vec![[-0.5, 0.0, -0.5], [0.5, 0.0, -0.5], [0.5, 0.2, 0.5], [-0.5, 0.0, 0.5]];
        mesh
    }

    #[test]
    fn a_fully_contoured_mesh_lies_along_the_slope() {
        // ground rising one tile to the east
        let mut mesh = flat_tile();
        contour(&mut mesh, 0, |dx, _| dx);
        let ys: Vec<f32> = mesh.opaque.positions.iter().map(|p| p[1]).collect();
        assert_eq!(ys, vec![-0.5, 0.5, 0.7, -0.5]);
    }

    #[test]
    fn a_partly_contoured_mesh_only_bends_its_foot() {
        // contour the lowest half: the top vertex (at the model's full height) stays put
        let mut mesh = flat_tile();
        contour(&mut mesh, 32768, |_, _| 1.0);
        let ys: Vec<f32> = mesh.opaque.positions.iter().map(|p| p[1]).collect();
        assert_eq!(ys, vec![1.0, 1.0, 0.2, 1.0]);
    }
}
