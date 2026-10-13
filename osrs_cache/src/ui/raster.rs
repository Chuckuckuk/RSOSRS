//! A small software rasterizer: an RGBA canvas with a clip stack and "over" alpha blending.

use super::layout::Rect;
use crate::textures::Texture;

pub struct Canvas {
    pub width: i32,
    pub height: i32,
    /// RGBA8, row-major, starts fully transparent.
    pub rgba: Vec<u8>,
    clips: Vec<Rect>,
}

impl Canvas {
    pub fn new(width: i32, height: i32) -> Canvas {
        let (w, h) = (width.max(0), height.max(0));
        Canvas { width: w, height: h, rgba: vec![0; (w * h * 4) as usize], clips: vec![Rect::new(0, 0, w, h)] }
    }

    /// The current clip rectangle (always inside the canvas).
    pub fn clip(&self) -> Rect {
        *self.clips.last().unwrap()
    }

    /// Push `r` intersected with the current clip.
    pub fn push_clip(&mut self, r: Rect) {
        let c = self.clip().intersect(&r);
        self.clips.push(c);
    }

    pub fn pop_clip(&mut self) {
        if self.clips.len() > 1 {
            self.clips.pop();
        }
    }

    /// Blend one pixel (`rgb` = 0xRRGGBB, `alpha` 0..=255) respecting the clip.
    pub fn put(&mut self, x: i32, y: i32, rgb: u32, alpha: u8) {
        if alpha == 0 || !self.clip().contains(x, y) {
            return;
        }
        let o = ((y * self.width + x) * 4) as usize;
        let src = [(rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8];
        let d = &mut self.rgba[o..o + 4];
        if alpha == 255 || d[3] == 0 {
            d[..3].copy_from_slice(&src);
            d[3] = alpha;
            return;
        }
        let sa = alpha as u32;
        let da = d[3] as u32;
        let out_a = sa + da * (255 - sa) / 255;
        for i in 0..3 {
            let s = src[i] as u32 * sa * 255;
            let dd = d[i] as u32 * da * (255 - sa);
            d[i] = ((s + dd) / (out_a * 255).max(1)).min(255) as u8;
        }
        d[3] = out_a.min(255) as u8;
    }

    pub fn fill_rect(&mut self, r: Rect, rgb: u32, alpha: u8) {
        let r = r.intersect(&self.clip());
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                self.put(x, y, rgb, alpha);
            }
        }
    }

    /// One-pixel outline inside `r`.
    pub fn outline_rect(&mut self, r: Rect, rgb: u32, alpha: u8) {
        if r.is_empty() {
            return;
        }
        for x in r.x..r.x + r.w {
            self.put(x, r.y, rgb, alpha);
            self.put(x, r.y + r.h - 1, rgb, alpha);
        }
        for y in r.y + 1..r.y + r.h - 1 {
            self.put(r.x, y, rgb, alpha);
            self.put(r.x + r.w - 1, y, rgb, alpha);
        }
    }

    /// Bresenham line with a `width` x `width` square brush.
    pub fn line(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: u32, alpha: u8, width: i32) {
        let (dx, dy) = ((x1 - x0).abs(), -(y1 - y0).abs());
        let (sx, sy) = (if x0 < x1 { 1 } else { -1 }, if y0 < y1 { 1 } else { -1 });
        let (mut x, mut y, mut err) = (x0, y0, dx + dy);
        let w = width.max(1);
        loop {
            for oy in 0..w {
                for ox in 0..w {
                    self.put(x + ox - (w - 1) / 2, y + oy - (w - 1) / 2, rgb, alpha);
                }
            }
            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    fn sample(&mut self, t: &Texture, sx: i32, sy: i32, dx: i32, dy: i32, alpha: u8) {
        let o = ((sy * t.width as i32 + sx) * 4) as usize;
        let a = t.rgba[o + 3] as u32;
        if a == 0 {
            return;
        }
        let a = (a * alpha as u32 / 255) as u8;
        let rgb = (t.rgba[o] as u32) << 16 | (t.rgba[o + 1] as u32) << 8 | t.rgba[o + 2] as u32;
        self.put(dx, dy, rgb, a);
    }

    /// Draw `t` at its natural size with its top-left at (x, y).
    pub fn blit(&mut self, t: &Texture, x: i32, y: i32, alpha: u8, flip_h: bool, flip_v: bool) {
        let (tw, th) = (t.width as i32, t.height as i32);
        self.blit_scaled(t, Rect::new(x, y, tw, th), alpha, flip_h, flip_v);
    }

    /// Draw `t` stretched (nearest neighbour) to fill `r`.
    pub fn blit_scaled(&mut self, t: &Texture, r: Rect, alpha: u8, flip_h: bool, flip_v: bool) {
        let (tw, th) = (t.width as i32, t.height as i32);
        if r.is_empty() || tw == 0 || th == 0 {
            return;
        }
        let vis = r.intersect(&self.clip());
        for y in vis.y..vis.y + vis.h {
            let mut sy = (y - r.y) * th / r.h;
            if flip_v {
                sy = th - 1 - sy;
            }
            for x in vis.x..vis.x + vis.w {
                let mut sx = (x - r.x) * tw / r.w;
                if flip_h {
                    sx = tw - 1 - sx;
                }
                self.sample(t, sx, sy, x, y, alpha);
            }
        }
    }

    /// Repeat `t` over `r`, starting at its top-left corner.
    pub fn blit_tiled(&mut self, t: &Texture, r: Rect, alpha: u8) {
        let (tw, th) = (t.width as i32, t.height as i32);
        if r.is_empty() || tw == 0 || th == 0 {
            return;
        }
        let vis = r.intersect(&self.clip());
        for y in vis.y..vis.y + vis.h {
            let sy = (y - r.y).rem_euclid(th);
            for x in vis.x..vis.x + vis.w {
                self.sample(t, (x - r.x).rem_euclid(tw), sy, x, y, alpha);
            }
        }
    }
}

/// IF3 sprite decoration: `border` 1 = one-pixel black outline around the opaque pixels, 2 = white;
/// `shadow` (0xRRGGBB, 0 = none) draws the silhouette shifted one pixel down-right under the sprite.
/// (The client's `SpritePixels.setBorder` / `setShadow`; semantics from memory.)
/// Same size as the input: decoration pixels that fall outside the canvas are lost.
pub fn decorate(t: &Texture, border: u8, shadow: u32) -> Texture {
    let (w, h) = (t.width as i32, t.height as i32);
    let solid = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && t.rgba[((y * w + x) * 4 + 3) as usize] > 0;
    let mut out = t.rgba.clone();
    let border_rgb = if border == 1 { Some(0x000001u32) } else if border >= 2 { Some(0xffffff) } else { None };
    let set = |out: &mut Vec<u8>, x: i32, y: i32, rgb: u32| {
        let o = ((y * w + x) * 4) as usize;
        out[o] = (rgb >> 16) as u8;
        out[o + 1] = (rgb >> 8) as u8;
        out[o + 2] = rgb as u8;
        out[o + 3] = 255;
    };
    for y in 0..h {
        for x in 0..w {
            if solid(x, y) {
                continue;
            }
            if shadow != 0 && solid(x - 1, y - 1) {
                set(&mut out, x, y, shadow);
            }
        }
    }
    if let Some(b) = border_rgb {
        for y in 0..h {
            for x in 0..w {
                if solid(x, y) {
                    continue;
                }
                if solid(x - 1, y) || solid(x + 1, y) || solid(x, y - 1) || solid(x, y + 1) {
                    set(&mut out, x, y, b);
                }
            }
        }
    }
    Texture { width: t.width, height: t.height, rgba: out, has_alpha: true, scroll_dir: 0, scroll_speed: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_and_clip() {
        let mut c = Canvas::new(10, 10);
        c.push_clip(Rect::new(2, 2, 3, 3));
        c.fill_rect(Rect::new(0, 0, 10, 10), 0xff0000, 255);
        c.pop_clip();
        let a = |x: usize, y: usize| c.rgba[(y * 10 + x) * 4 + 3];
        assert_eq!((a(2, 2), a(4, 4), a(5, 5), a(1, 1)), (255, 255, 0, 0));
    }

    #[test]
    fn blend_over() {
        let mut c = Canvas::new(1, 1);
        c.fill_rect(Rect::new(0, 0, 1, 1), 0x000000, 255);
        c.fill_rect(Rect::new(0, 0, 1, 1), 0xffffff, 128);
        assert!((c.rgba[0] as i32 - 128).abs() <= 1 && c.rgba[3] == 255);
    }

    #[test]
    fn tiled_and_scaled() {
        let t = Texture { width: 2, height: 1, rgba: vec![255, 0, 0, 255, 0, 255, 0, 255], has_alpha: false, scroll_dir: 0, scroll_speed: 0 };
        let mut c = Canvas::new(4, 1);
        c.blit_tiled(&t, Rect::new(0, 0, 4, 1), 255);
        assert_eq!(&c.rgba[8..12], &[255, 0, 0, 255]);
        let mut c = Canvas::new(4, 1);
        c.blit_scaled(&t, Rect::new(0, 0, 4, 1), 255, false, false);
        assert_eq!(&c.rgba[4..8], &[255, 0, 0, 255]);
        assert_eq!(&c.rgba[8..12], &[0, 255, 0, 255]);
    }
}
