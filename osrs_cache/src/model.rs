//! Static model geometry (archive 7).
//!
//! Models come in several wire versions, distinguished by trailing marker bytes:
//! `..` (legacy), `ff ff` (v1), `fe ff` (v2), `fd ff` (v3). All are decoded into
//! one [`Model`] with per-face HSL colours. Animation skins are kept for later use.

use crate::buf::Buf;
use crate::{fmt_err, Result};

#[derive(Debug, Clone, Default)]
pub struct Model {
    pub id: u32,
    pub version: u8,
    /// Vertex positions in client units (y is negative-up).
    pub vx: Vec<i32>,
    pub vy: Vec<i32>,
    pub vz: Vec<i32>,
    pub fa: Vec<u16>,
    pub fb: Vec<u16>,
    pub fc: Vec<u16>,
    /// Packed 16-bit HSL colour per face (see [`hsl_to_rgb`]).
    pub colors: Vec<u16>,
    /// 0 = opaque .. 255 = fully transparent. Empty if the model has no alpha.
    pub alpha: Vec<u8>,
    /// Bit 0 set = flat shaded, else gouraud. Empty if absent.
    pub render_type: Vec<u8>,
    /// Texture id per face (-1 = none). Empty if the model has no textures.
    pub textures: Vec<i16>,
    pub priority: Vec<u8>,
    pub vertex_skin: Vec<u8>,
    pub face_skin: Vec<u8>,
    /// Texture-space triangles `[p, m, n]` (vertex indices): texture u runs p->m, v runs p->n.
    pub tex_tris: Vec<[u16; 3]>,
    /// Per face: index into `tex_tris`, or -1 to map from the face's own three vertices.
    /// Only meaningful for faces with `textures[f] >= 0`.
    pub face_coord: Vec<i16>,
}

impl Model {
    pub fn vertex_count(&self) -> usize {
        self.vx.len()
    }
    pub fn face_count(&self) -> usize {
        self.fa.len()
    }

    pub fn version_of(data: &[u8]) -> u8 {
        let n = data.len();
        if n >= 2 && data[n - 2] == 0xFF {
            match data[n - 1] {
                0xFF => 1,
                0xFE => 2,
                0xFD => 3,
                _ => 0,
            }
        } else {
            0
        }
    }

    pub fn decode(id: u32, data: &[u8]) -> Result<Model> {
        let mut m = match Model::version_of(data) {
            0 => decode_legacy(data)?,
            v => decode_new(data, v)?,
        };
        m.id = id;
        Ok(m)
    }

    /// Apply recolour tables (object/item definitions).
    pub fn recolor(&mut self, from: &[u16], to: &[u16]) {
        for c in &mut self.colors {
            for (f, t) in from.iter().zip(to) {
                if *c == *f {
                    *c = *t;
                    break;
                }
            }
        }
    }
}

struct Layout {
    vc: usize,
    fc: usize,
    tex_tris: usize,
    flags: u8,
    priority: u8,
    has_alpha: bool,
    has_face_skins: bool,
    has_vertex_skins: bool,
    has_textures: bool,
    x_len: usize,
    y_len: usize,
    z_len: usize,
    idx_len: usize,
    /// v3 header field after the index length: size of the textured-triangle section.
    extra_len: usize,
    /// v3: when the header's extra byte is 1, a further section of this many bytes follows.
    extra_section: usize,
}

fn decode_legacy(data: &[u8]) -> Result<Model> {
    if data.len() < 18 {
        return fmt_err("legacy model too short");
    }
    let mut h = Buf::at(data, data.len() - 18);
    let l = Layout {
        vc: h.u16() as usize,
        fc: h.u16() as usize,
        tex_tris: h.u8() as usize,
        flags: h.u8(),
        priority: h.u8(),
        has_alpha: h.u8() == 1,
        has_face_skins: h.u8() == 1,
        has_vertex_skins: h.u8() == 1,
        x_len: h.u16() as usize,
        y_len: h.u16() as usize,
        z_len: h.u16() as usize,
        idx_len: h.u16() as usize,
        has_textures: false,
        extra_len: 0,
        extra_section: 0,
    };
    build(data, &l, 0)
}

/// v2 (`ff fe`) and v3 (`ff fd`): header is read backwards from the marker.
///   v2: vc fc tc flags prio alpha fskin vskin tex | x y z idx L | marker        (23 bytes)
///   v3: vc fc tc flags prio alpha fskin vskin tex extra | x y z idx L M | marker (26 bytes)
fn decode_new(data: &[u8], version: u8) -> Result<Model> {
    let hdr = if version == 3 { 26 } else { 23 };
    if data.len() < hdr {
        return fmt_err("model too short");
    }
    let mut h = Buf::at(data, data.len() - hdr);
    let vc = h.u16() as usize;
    let fc = h.u16() as usize;
    let tex_tris = h.u8() as usize;
    let flags = h.u8();
    let priority = h.u8();
    let has_alpha = h.u8() == 1;
    let has_face_skins = h.u8() == 1;
    // v2: ... alpha fskin vskin tex       v3: ... alpha fskin tex vskin extra
    let b3 = h.u8() == 1;
    let b4 = h.u8() == 1;
    let (has_vertex_skins, has_textures) = if version == 3 { (b4, b3) } else { (b3, b4) };
    let extra_flag = if version == 3 { h.u8() } else { 0 };
    let mut l = Layout {
        vc,
        fc,
        tex_tris,
        flags,
        priority,
        has_alpha,
        has_face_skins,
        has_vertex_skins,
        has_textures,
        x_len: h.u16() as usize,
        y_len: h.u16() as usize,
        z_len: h.u16() as usize,
        idx_len: h.u16() as usize,
        extra_len: h.u16() as usize,
        extra_section: 0,
    };
    if version == 3 {
        let m = h.u16() as usize;
        if extra_flag == 1 {
            // when the vertex-skin array is also present, the field counts it too
            l.extra_section = m.saturating_sub(if l.has_vertex_skins { l.vc } else { 0 });
        }
    }
    build(data, &l, version)
}

/// Bytes consumed by `n` signed smarts starting at `pos`.
fn smart_run(data: &[u8], pos: usize, n: usize) -> usize {
    let mut p = pos;
    for _ in 0..n {
        p += if data.get(p).copied().unwrap_or(0) < 128 { 1 } else { 2 };
    }
    p - pos
}

fn build(data: &[u8], l: &Layout, version: u8) -> Result<Model> {
    // Textured-triangle records have a variable encoding that is not decoded yet (they only
    // matter for textures), so try a few plausible section sizes until the vertex streams line up.
    // v3: one type byte per textured triangle comes first, and the textured-triangle section
    // between the colours and the vertex streams is exactly `extra_len` bytes.
    // v2: the section is 6 bytes (3 x u16) per textured triangle.
    let mut candidates: Vec<usize> = Vec::new();
    if version == 3 {
        candidates.push(l.extra_len);
    }
    candidates.push(l.tex_tris * 6);
    if l.tex_tris > 0 {
        for k in (0..=8).rev() {
            if k != 6 {
                candidates.push(l.tex_tris * k);
            }
        }
    }
    let mut last_err = None;
    for tex_bytes in candidates {
        match layout_with(data, l, version, tex_bytes) {
            Ok(Some(p)) => return decode_streams(data, l, version, p),
            Ok(None) => {}
            Err(e) => last_err = Some(e),
        }
    }
    match last_err {
        Some(e) => Err(e),
        None => fmt_err(format!("v{version} vertex streams do not match header lengths")),
    }
}

struct Positions {
    vflags: usize,
    fcompress: usize,
    prio: usize,
    fskin: usize,
    frender: usize,
    vskin: usize,
    alpha: usize,
    idx: usize,
    ftex: usize,
    color: usize,
    texsec: usize,
    x: usize,
    y: usize,
    z: usize,
}

fn layout_with(data: &[u8], l: &Layout, version: u8, tex_bytes: usize) -> Result<Option<Positions>> {
    let new = version != 0;
    let mut off = 0usize;
    let mut take = |n: usize| {
        let s = off;
        off += n;
        s
    };
    if version == 3 {
        take(l.tex_tris);
    }
    let vflags = take(l.vc);
    // v3 stores the per-face render types before the face compress types.
    let early_render = version == 3;
    let mut frender = 0;
    if early_render {
        frender = take(if l.flags & 1 != 0 { l.fc } else { 0 });
    }
    let fcompress = take(l.fc);
    let prio = take(if l.priority == 255 { l.fc } else { 0 });
    let fskin = take(if l.has_face_skins { l.fc } else { 0 });
    if !early_render {
        frender = take(if l.flags & 1 != 0 { l.fc } else { 0 });
    }
    let vskin = take(if l.has_vertex_skins { l.vc } else { 0 });
    let alpha = take(if l.has_alpha { l.fc } else { 0 });
    // v3 models flagged with the extra byte carry an opaque section ahead of the face indices
    take(l.extra_section);
    let idx = take(l.idx_len);
    let ftex = take(if new && l.has_textures { l.fc * 2 } else { 0 });
    // v3 puts the textured-triangle section before the face colours; v2 after them.
    let mut texsec = 0;
    if version == 3 {
        texsec = take(tex_bytes);
    }
    let color = take(l.fc * 2);
    if version != 3 {
        texsec = take(tex_bytes);
    }
    let x = take(l.x_len);
    let y = take(l.y_len);
    let z = take(l.z_len);
    if off > data.len() {
        return fmt_err(format!("v{version} layout overruns: need {off}, have {}", data.len()));
    }
    let flag_bytes = &data[vflags..vflags + l.vc];
    let nx = flag_bytes.iter().filter(|f| *f & 1 != 0).count();
    let ny = flag_bytes.iter().filter(|f| *f & 2 != 0).count();
    let nz = flag_bytes.iter().filter(|f| *f & 4 != 0).count();
    if smart_run(data, x, nx) != l.x_len || smart_run(data, y, ny) != l.y_len || smart_run(data, z, nz) != l.z_len {
        return Ok(None);
    }
    Ok(Some(Positions { vflags, fcompress, prio, fskin, frender, vskin, alpha, idx, ftex, color, texsec, x, y, z }))
}

fn decode_streams(data: &[u8], l: &Layout, version: u8, p: Positions) -> Result<Model> {
    let new = version != 0;
    let Positions {
        vflags: p_vflags,
        fcompress: p_fcompress,
        prio: p_prio,
        fskin: p_fskin,
        frender: p_frender,
        vskin: p_vskin,
        alpha: p_alpha,
        idx: p_idx,
        ftex: p_ftex,
        color: p_color,
        texsec: p_texsec,
        x: p_x,
        y: p_y,
        z: p_z,
    } = p;
    let mut m = Model { version, ..Default::default() };

    // vertices
    let mut fl = Buf::at(data, p_vflags);
    let mut bx = Buf::at(data, p_x);
    let mut by = Buf::at(data, p_y);
    let mut bz = Buf::at(data, p_z);
    let (mut x, mut y, mut z) = (0i32, 0i32, 0i32);
    for _ in 0..l.vc {
        let f = fl.u8();
        if f & 1 != 0 {
            x += bx.smart_s();
        }
        if f & 2 != 0 {
            y += by.smart_s();
        }
        if f & 4 != 0 {
            z += bz.smart_s();
        }
        m.vx.push(x);
        m.vy.push(y);
        m.vz.push(z);
    }
    if l.has_vertex_skins {
        let mut b = Buf::at(data, p_vskin);
        m.vertex_skin = (0..l.vc).map(|_| b.u8()).collect();
    }

    // faces
    let mut ty = Buf::at(data, p_fcompress);
    let mut ix = Buf::at(data, p_idx);
    let (mut a, mut b_, mut c) = (0i32, 0i32, 0i32);
    let mut last = 0i32;
    for _ in 0..l.fc {
        match ty.u8() {
            1 => {
                a = ix.smart_s() + last;
                last = a;
                b_ = ix.smart_s() + last;
                last = b_;
                c = ix.smart_s() + last;
                last = c;
            }
            2 => {
                b_ = c;
                c = ix.smart_s() + last;
                last = c;
            }
            3 => {
                a = c;
                c = ix.smart_s() + last;
                last = c;
            }
            4 => {
                std::mem::swap(&mut a, &mut b_);
                c = ix.smart_s() + last;
                last = c;
            }
            t => return fmt_err(format!("bad face compress type {t}")),
        }
        if a < 0 || b_ < 0 || c < 0 || a as usize >= l.vc || b_ as usize >= l.vc || c as usize >= l.vc {
            return fmt_err("face index out of range");
        }
        m.fa.push(a as u16);
        m.fb.push(b_ as u16);
        m.fc.push(c as u16);
    }
    if ix.pos - p_idx != l.idx_len {
        return fmt_err("face index stream length mismatch");
    }

    let mut cb = Buf::at(data, p_color);
    m.colors = (0..l.fc).map(|_| cb.u16()).collect();
    if l.flags & 1 != 0 {
        let mut b = Buf::at(data, p_frender);
        m.render_type = (0..l.fc).map(|_| b.u8()).collect();
    }
    m.priority = if l.priority == 255 {
        let mut b = Buf::at(data, p_prio);
        (0..l.fc).map(|_| b.u8()).collect()
    } else {
        vec![l.priority; l.fc]
    };
    if l.has_alpha {
        let mut b = Buf::at(data, p_alpha);
        m.alpha = (0..l.fc).map(|_| b.u8()).collect();
    }
    if l.has_face_skins {
        let mut b = Buf::at(data, p_fskin);
        m.face_skin = (0..l.fc).map(|_| b.u8()).collect();
    }
    if new && l.has_textures {
        let mut b = Buf::at(data, p_ftex);
        m.textures = (0..l.fc).map(|_| b.u16() as i16 - 1).collect();
    } else if !m.render_type.is_empty() && m.render_type.iter().any(|r| r & 2 != 0) {
        // Older encoding: a face with render type bit 1 set stores its texture id in the colour.
        m.textures = (0..l.fc)
            .map(|f| if m.render_type[f] & 2 != 0 { m.colors[f] as i16 } else { -1 })
            .collect();
    }

    // textured triangles and per-face coordinate indices
    let tc = l.tex_tris;
    m.face_coord = vec![-1; l.fc];
    if tc > 0 {
        if version == 3 {
            // types come first; the triangle vertices trail the z stream (type != 0 carries 13
            // more bytes each, not interpreted); per-face indices (1-based, 0 = own vertices)
            // sit in the section ahead of the colours, one per textured face in face order.
            let mut b = Buf::at(data, p_z + l.z_len);
            for i in 0..tc {
                let tri = [b.u16(), b.u16(), b.u16()];
                if data.get(i).copied().unwrap_or(0) != 0 {
                    b.pos += 13;
                }
                m.tex_tris.push(tri);
            }
            let mut cb = Buf::at(data, p_texsec);
            for f in 0..l.fc {
                if m.textures.get(f).is_some_and(|t| *t >= 0) {
                    m.face_coord[f] = cb.u8() as i16 - 1;
                }
            }
        } else {
            let mut b = Buf::at(data, p_texsec);
            for _ in 0..tc {
                m.tex_tris.push([b.u16(), b.u16(), b.u16()]);
            }
            for f in 0..l.fc {
                if m.render_type.get(f).is_some_and(|r| r & 2 != 0) {
                    m.face_coord[f] = (m.render_type[f] >> 2) as i16;
                }
            }
        }
    }
    Ok(m)
}

/// Jagex 16-bit HSL (6-bit hue, 3-bit saturation, 7-bit lightness) to linear-ish RGB in 0..1.
/// `brightness` is the client gamma (0.8 by default; lower is brighter).
pub fn hsl_to_rgb(hsl: u16, brightness: f32) -> [f32; 3] {
    let h = ((hsl >> 10) & 63) as f32 / 64.0 + 0.0078125;
    let s = ((hsl >> 7) & 7) as f32 / 8.0 + 0.0625;
    let l = (hsl & 127) as f32 / 128.0;
    hsl_f32_to_rgb(h, s, l, brightness)
}

pub fn hsl_f32_to_rgb(h: f32, s: f32, l: f32, brightness: f32) -> [f32; 3] {
    let (mut r, mut g, mut b) = (l, l, l);
    if s != 0.0 {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        let channel = |mut t: f32| -> f32 {
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            if 6.0 * t < 1.0 {
                p + (q - p) * 6.0 * t
            } else if 2.0 * t < 1.0 {
                q
            } else if 3.0 * t < 2.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            }
        };
        r = channel(h + 1.0 / 3.0);
        g = channel(h);
        b = channel(h - 1.0 / 3.0);
    }
    [r.powf(brightness), g.powf(brightness), b.powf(brightness)]
}

/// Pack an `0xRRGGBB` colour into Jagex 16-bit HSL.
pub fn rgb_to_hsl16(rgb: u32) -> u16 {
    let (h, s, l) = rgb_to_hsl_f32(rgb);
    let hh = ((h * 64.0) as i32).clamp(0, 63);
    let ss = ((s * 8.0) as i32).clamp(0, 7);
    let ll = ((l * 128.0) as i32).clamp(0, 127);
    ((hh << 10) | (ss << 7) | ll) as u16
}

/// `0xRRGGBB` to (hue, saturation, lightness), each 0..1.
pub fn rgb_to_hsl_f32(rgb: u32) -> (f32, f32, f32) {
    let r = ((rgb >> 16) & 255) as f32 / 256.0;
    let g = ((rgb >> 8) & 255) as f32 / 256.0;
    let b = (rgb & 255) as f32 / 256.0;
    let min = r.min(g).min(b);
    let max = r.max(g).max(b);
    let l = (min + max) / 2.0;
    let (mut h, mut s) = (0.0, 0.0);
    if min != max {
        s = if l < 0.5 { (max - min) / (max + min) } else { (max - min) / (2.0 - max - min) };
        h = if r == max {
            (g - b) / (max - min)
        } else if g == max {
            2.0 + (b - r) / (max - min)
        } else {
            4.0 + (r - g) / (max - min)
        };
    }
    h /= 6.0;
    if h < 0.0 {
        h += 1.0;
    }
    (h, s, l)
}
