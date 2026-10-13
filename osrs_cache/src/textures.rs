//! Texture images: texture definitions (archive 9) point at paletted sprites (archive 8).

use crate::buf::Buf;
use crate::{idx, Cache, Result};
use std::collections::HashMap;

#[derive(Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    /// RGBA8, row-major, top row first.
    pub rgba: Vec<u8>,
    /// True if any pixel is fully transparent (cut-out textures).
    pub has_alpha: bool,
    /// Scroll direction 1..=4 (0 = static) and speed from the definition's last two bytes.
    pub scroll_dir: u8,
    pub scroll_speed: u8,
}

/// First frame of a sprite, composed on its full canvas. Palette index 0 is transparent.
pub fn decode_sprite(data: &[u8]) -> Option<Texture> {
    if data.len() < 10 {
        return None;
    }
    let mut b = Buf::at(data, data.len() - 2);
    let frames = b.u16() as usize;
    if frames == 0 || data.len() < 7 + frames * 8 {
        return None;
    }
    let mut b = Buf::at(data, data.len() - 7 - frames * 8);
    let (max_w, max_h) = (b.u16() as usize, b.u16() as usize);
    let pal_len = b.u8() as usize + 1;
    let off_x: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let off_y: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let widths: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let heights: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let pal_pos = data.len().checked_sub(7 + frames * 8 + (pal_len - 1) * 3)?;
    let mut pb = Buf::at(data, pal_pos);
    let mut palette = vec![0u32; pal_len];
    for p in palette.iter_mut().skip(1) {
        let rgb = pb.u24();
        *p = if rgb == 0 { 1 } else { rgb };
    }
    let mut pix = Buf::new(data);
    let flags = pix.u8();
    let (w, h) = (widths[0], heights[0]);
    if w == 0 || h == 0 || max_w == 0 || max_h == 0 {
        return None;
    }
    let mut idxs = vec![0u8; w * h];
    if flags & 1 == 0 {
        for v in idxs.iter_mut() {
            *v = pix.u8();
        }
    } else {
        for x in 0..w {
            for y in 0..h {
                idxs[y * w + x] = pix.u8();
            }
        }
    }
    let mut alpha = vec![255u8; w * h];
    if flags & 2 != 0 {
        if flags & 1 == 0 {
            for a in alpha.iter_mut() {
                *a = pix.u8();
            }
        } else {
            for x in 0..w {
                for y in 0..h {
                    alpha[y * w + x] = pix.u8();
                }
            }
        }
    }
    // 21 of the 37 sprites with an alpha plane (stone highlights 1027-1030, ...) store all zeros: the
    // plane is unused there and the sprite is opaque (the client shows them).
    if flags & 2 != 0 && alpha.iter().all(|&a| a == 0) {
        alpha.fill(255);
    }
    let mut rgba = vec![0u8; max_w * max_h * 4];
    let mut has_alpha = false;
    for y in 0..h {
        for x in 0..w {
            let (cx, cy) = (x + off_x[0], y + off_y[0]);
            if cx >= max_w || cy >= max_h {
                continue;
            }
            let i = idxs[y * w + x] as usize;
            let o = (cy * max_w + cx) * 4;
            if i == 0 {
                has_alpha = true;
                continue;
            }
            let c = palette.get(i).copied().unwrap_or(0);
            rgba[o] = (c >> 16) as u8;
            rgba[o + 1] = (c >> 8) as u8;
            rgba[o + 2] = c as u8;
            rgba[o + 3] = alpha[y * w + x];
            if alpha[y * w + x] < 255 {
                has_alpha = true;
            }
        }
    }
    // fully transparent canvas margin counts as alpha too
    if w < max_w || h < max_h {
        has_alpha = true;
    }
    Some(Texture { width: max_w as u32, height: max_h as u32, rgba, has_alpha, scroll_dir: 0, scroll_speed: 0 })
}

/// Decode every texture. Definition layout: `[sprite u16][average hsl u16][transparent u8]
/// [scroll direction u8][scroll speed u8]`.
pub fn load_textures(cache: &Cache) -> Result<HashMap<u32, Texture>> {
    let defs = cache.index(idx::TEXTURES)?;
    let sprites = cache.index(idx::SPRITES)?;
    let mut out = HashMap::new();
    for group in defs.groups.keys() {
        for (id, d) in cache.read_files(&defs, *group)? {
            if d.len() < 4 {
                continue;
            }
            let sprite = u16::from_be_bytes([d[0], d[1]]) as u32;
            let Ok(files) = cache.read_files(&sprites, sprite) else { continue };
            if let Some(mut tex) = files.first().and_then(|(_, f)| decode_sprite(f)) {
                // definition tail: [transparent flag][scroll direction][scroll speed]
                if d.len() >= 7 {
                    tex.scroll_dir = d[5];
                    tex.scroll_speed = d[6];
                }
                out.insert(id, tex);
            }
        }
    }
    Ok(out)
}

/// The first frame of sprite group `group` of the sprites archive, on its full canvas.
pub fn load_sprite(cache: &Cache, sprites: &crate::Index, group: u32) -> Option<Texture> {
    let files = cache.read_files(sprites, group).ok()?;
    files.first().and_then(|(_, f)| decode_sprite(f))
}
