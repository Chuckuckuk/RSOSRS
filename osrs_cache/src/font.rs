//! The game's bitmap fonts: one sprite group per font (a frame per character, in the sprites
//! archive) and a metrics file (in the fonts archive) whose first 256 bytes are the horizontal
//! advance of each character.

use crate::buf::Buf;
use crate::textures::Texture;
use crate::{idx, Cache, Result};

/// One frame of a multi-frame sprite: its pixels (palette index 0 is transparent) at `x`, `y`
/// inside a `canvas_width` x `canvas_height` canvas.
#[derive(Clone)]
pub struct Frame {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub canvas_width: usize,
    pub canvas_height: usize,
    /// RGBA8, `width` x `height`.
    pub rgba: Vec<u8>,
}

/// Every frame of a sprite.
pub fn decode_frames(data: &[u8]) -> Vec<Frame> {
    if data.len() < 10 {
        return Vec::new();
    }
    let frames = Buf::at(data, data.len() - 2).u16() as usize;
    if frames == 0 || data.len() < 7 + frames * 8 {
        return Vec::new();
    }
    let mut b = Buf::at(data, data.len() - 7 - frames * 8);
    let (canvas_width, canvas_height) = (b.u16() as usize, b.u16() as usize);
    let palette_len = b.u8() as usize + 1;
    let xs: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let ys: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let widths: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let heights: Vec<usize> = (0..frames).map(|_| b.u16() as usize).collect();
    let Some(palette_at) = data.len().checked_sub(7 + frames * 8 + (palette_len - 1) * 3) else {
        return Vec::new();
    };
    let mut p = Buf::at(data, palette_at);
    let mut palette = vec![0u32; palette_len];
    for colour in palette.iter_mut().skip(1) {
        let rgb = p.u24();
        *colour = if rgb == 0 { 1 } else { rgb };
    }

    let mut pixels = Buf::new(data);
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let (w, h) = (widths[f], heights[f]);
        let flags = pixels.u8();
        let mut index = vec![0u8; w * h];
        let read = |plane: &mut [u8], pixels: &mut Buf| {
            if flags & 1 == 0 {
                for v in plane.iter_mut() {
                    *v = pixels.u8();
                }
            } else {
                for x in 0..w {
                    for y in 0..h {
                        plane[y * w + x] = pixels.u8();
                    }
                }
            }
        };
        read(&mut index, &mut pixels);
        let mut alpha = vec![255u8; w * h];
        if flags & 2 != 0 {
            read(&mut alpha, &mut pixels);
        }
        let mut rgba = vec![0u8; w * h * 4];
        for (i, &palette_index) in index.iter().enumerate() {
            if palette_index == 0 {
                continue;
            }
            let colour = palette.get(palette_index as usize).copied().unwrap_or(0);
            rgba[i * 4] = (colour >> 16) as u8;
            rgba[i * 4 + 1] = (colour >> 8) as u8;
            rgba[i * 4 + 2] = colour as u8;
            rgba[i * 4 + 3] = alpha[i];
        }
        out.push(Frame { x: xs[f], y: ys[f], width: w, height: h, canvas_width, canvas_height, rgba });
    }
    out
}

#[derive(Clone)]
pub struct Font {
    glyphs: Vec<Frame>,
    advances: Vec<u8>,
    /// Height of a line of text, pixels (the glyph canvas height).
    pub height: usize,
    /// Topmost ink row of any glyph (client: the tallest glyph touches the text origin).
    pub top: usize,
    /// One past the lowest ink row of any glyph (descenders included).
    pub bottom: usize,
}

impl Font {
    /// The font called `name` ("p11_full", "p12_full", "b12_full", ...).
    pub fn load(cache: &Cache, name: &str) -> Result<Option<Font>> {
        let sprites = cache.index(idx::SPRITES)?;
        let metrics = cache.index(13)?;
        let (Some(sprite), Some(metric)) = (sprites.group_by_name(name), metrics.group_by_name(name)) else {
            return Ok(None);
        };
        let sprite_file = cache.read_files(&sprites, sprite.id)?;
        let metric_file = cache.read_files(&metrics, metric.id)?;
        let (Some((_, sprite_data)), Some((_, metric_data))) = (sprite_file.first(), metric_file.first()) else {
            return Ok(None);
        };
        let glyphs = decode_frames(sprite_data);
        if glyphs.is_empty() || metric_data.len() < 256 {
            return Ok(None);
        }
        let height = glyphs[0].canvas_height;
        let top = glyphs.iter().filter(|g| g.height > 0).map(|g| g.y).min().unwrap_or(0);
        let bottom = glyphs.iter().filter(|g| g.height > 0).map(|g| g.y + g.height).max().unwrap_or(height);
        Ok(Some(Font { glyphs, advances: metric_data[..256].to_vec(), height, top, bottom }))
    }

    /// Load by the sprite-group id that interface components store (494 = p11_full, ...). The
    /// metrics file in archive 13 has the same group id as the sprite group.
    pub fn load_id(cache: &Cache, sprite_group: u32) -> Result<Option<Font>> {
        let sprites = cache.index(idx::SPRITES)?;
        let metrics = cache.index(13)?;
        if !metrics.groups.contains_key(&sprite_group) || !sprites.groups.contains_key(&sprite_group) {
            return Ok(None);
        }
        let (sf, mf) = (cache.read_files(&sprites, sprite_group)?, cache.read_files(&metrics, sprite_group)?);
        let (Some((_, sd)), Some((_, md))) = (sf.first(), mf.first()) else { return Ok(None) };
        let glyphs = decode_frames(sd);
        if glyphs.is_empty() || md.len() < 256 {
            return Ok(None);
        }
        let height = glyphs[0].canvas_height;
        let top = glyphs.iter().filter(|g| g.height > 0).map(|g| g.y).min().unwrap_or(0);
        let bottom = glyphs.iter().filter(|g| g.height > 0).map(|g| g.y + g.height).max().unwrap_or(height);
        Ok(Some(Font { glyphs, advances: md[..256].to_vec(), height, top, bottom }))
    }

    /// The glyph frame of byte `c`, if it has one.
    pub fn glyph(&self, c: u8) -> Option<&Frame> {
        self.glyphs.get(c as usize)
    }

    /// Horizontal advance of byte `c`.
    pub fn advance_of(&self, c: u8) -> usize {
        self.advance(c)
    }

    fn advance(&self, c: u8) -> usize {
        self.advances[c as usize] as usize
    }

    /// How wide `text` is.
    pub fn width(&self, text: &str) -> usize {
        text.bytes().map(|c| self.advance(c)).sum()
    }

    /// `text` as an image, in `rgb` (0xRRGGBB) with a one-pixel black shadow when `shadow`.
    pub fn render(&self, text: &str, rgb: u32, shadow: bool) -> Texture {
        let extra = usize::from(shadow);
        let width = (self.width(text) + extra).max(1);
        let height = self.height + extra;
        let mut rgba = vec![0u8; width * height * 4];
        let mut put = |x: usize, y: usize, colour: [u8; 3], alpha: u8| {
            if x < width && y < height && alpha > 0 {
                let o = (y * width + x) * 4;
                // an ink pixel replaces whatever is under it (the shadow)
                rgba[o..o + 3].copy_from_slice(&colour);
                rgba[o + 3] = alpha;
            }
        };
        let ink = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
        for pass in 0..=extra {
            let (shift, colour) = if shadow && pass == 0 { (1, [0, 0, 0]) } else { (0, ink) };
            let mut pen = 0;
            for c in text.bytes() {
                if let Some(glyph) = self.glyphs.get(c as usize) {
                    for gy in 0..glyph.height {
                        for gx in 0..glyph.width {
                            let alpha = glyph.rgba[(gy * glyph.width + gx) * 4 + 3];
                            put(pen + glyph.x + gx + shift, glyph.y + gy + shift, colour, alpha);
                        }
                    }
                }
                pen += self.advance(c);
            }
        }
        Texture { width: width as u32, height: height as u32, rgba, has_alpha: true, scroll_dir: 0, scroll_speed: 0 }
    }
}
