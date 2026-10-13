//! Text drawing with the client's tags (`<col=rrggbb>`, `</col>`, `<br>`, `<lt>`, `<gt>`, `<img=n>` skipped,
//! other tags dropped) and word wrapping to the component width.

use super::layout::Rect;
use super::raster::Canvas;
use crate::font::Font;

#[derive(Clone, Copy)]
struct Glyph {
    c: u8,
    colour: Option<u32>,
}

fn to_byte(ch: char) -> u8 {
    match ch as u32 {
        v if v < 256 => v as u8,
        0x20ac => 0x80,
        0x2018 => 0x91,
        0x2019 => 0x92,
        0x201c => 0x93,
        0x201d => 0x94,
        0x2022 => 0x95,
        0x2013 => 0x96,
        0x2014 => 0x97,
        _ => b'?',
    }
}

/// Parse tags into glyph lines separated at `<br>`. Returns lines of glyphs.
fn parse(text: &str) -> Vec<Vec<Glyph>> {
    let mut lines = vec![Vec::new()];
    let mut colour: Option<u32> = None;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '<' {
            if let Some(end) = chars[i..].iter().position(|&c| c == '>') {
                let tag: String = chars[i + 1..i + end].iter().collect();
                let lower = tag.to_ascii_lowercase();
                i += end + 1;
                if lower == "br" {
                    lines.push(Vec::new());
                } else if lower == "lt" {
                    lines.last_mut().unwrap().push(Glyph { c: b'<', colour });
                } else if lower == "gt" {
                    lines.last_mut().unwrap().push(Glyph { c: b'>', colour });
                } else if lower == "/col" {
                    colour = None;
                } else if let Some(v) = lower.strip_prefix("col=") {
                    colour = u32::from_str_radix(v.trim_start_matches('#'), 16).ok();
                }
                continue;
            }
        }
        lines.last_mut().unwrap().push(Glyph { c: to_byte(ch), colour });
        i += 1;
    }
    lines
}

fn width_of(font: &Font, g: &[Glyph]) -> i32 {
    g.iter().map(|g| font.advance_of(g.c) as i32).sum()
}

/// Greedy word wrap of one parsed line to `max_w` (<= 0 disables wrapping).
fn wrap(font: &Font, line: Vec<Glyph>, max_w: i32) -> Vec<Vec<Glyph>> {
    if max_w <= 0 || width_of(font, &line) <= max_w {
        return vec![line];
    }
    let mut out = Vec::new();
    let mut cur: Vec<Glyph> = Vec::new();
    let mut word: Vec<Glyph> = Vec::new();
    let flush = |cur: &mut Vec<Glyph>, word: &mut Vec<Glyph>, out: &mut Vec<Vec<Glyph>>, space: Option<Glyph>| {
        let cw = width_of(font, cur);
        if !cur.is_empty() && cw + width_of(font, word) > max_w {
            // drop the trailing space of the finished line
            while cur.last().is_some_and(|g| g.c == b' ') {
                cur.pop();
            }
            out.push(std::mem::take(cur));
        }
        cur.append(word);
        if let Some(s) = space {
            cur.push(s);
        }
    };
    for g in line {
        if g.c == b' ' {
            flush(&mut cur, &mut word, &mut out, Some(g));
        } else {
            word.push(g);
        }
    }
    flush(&mut cur, &mut word, &mut out, None);
    out.push(cur);
    out
}

/// Number of pixels between consecutive baselines when `line_height` is 0.
pub fn default_line_height(font: &Font) -> i32 {
    (font.bottom - font.top) as i32
}

fn draw_line(canvas: &mut Canvas, font: &Font, line: &[Glyph], x: i32, y_top: i32, rgb: u32, shadow: bool) {
    // y_top is where the tallest glyph's top row lands
    let origin_y = y_top - font.top as i32;
    for pass in 0..2 {
        if pass == 0 && !shadow {
            continue;
        }
        let mut pen = x;
        for g in line {
            if let Some(f) = font.glyph(g.c) {
                let ink = if pass == 0 { 0 } else { g.colour.unwrap_or(rgb) };
                let (dx, dy) = if pass == 0 { (1, 1) } else { (0, 0) };
                for gy in 0..f.height {
                    for gx in 0..f.width {
                        if f.rgba[(gy * f.width + gx) * 4 + 3] > 0 {
                            canvas.put(pen + f.x as i32 + gx as i32 + dx, origin_y + f.y as i32 + gy as i32 + dy, ink, 255);
                        }
                    }
                }
            }
            pen += font.advance_of(g.c) as i32;
        }
    }
}

/// Draw `text` into `rect`. `x_align` 0 left / 1 centre / 2 right, `y_align` 0 top / 1 middle / 2 bottom.
/// `line_height` 0 uses the font's own. Text is not clipped to `rect` (only to the canvas clip), like the client.
#[allow(clippy::too_many_arguments)]
pub fn draw_text(
    canvas: &mut Canvas,
    font: &Font,
    text: &str,
    rect: Rect,
    rgb: u32,
    x_align: u8,
    y_align: u8,
    line_height: u8,
    shadow: bool,
) {
    let mut lines = Vec::new();
    for l in parse(text) {
        lines.extend(wrap(font, l, rect.w));
    }
    let lh = if line_height == 0 { default_line_height(font) } else { line_height as i32 };
    let glyph_h = (font.bottom - font.top) as i32;
    let block = (lines.len() as i32 - 1) * lh + glyph_h;
    let top = match y_align {
        0 => rect.y,
        1 => rect.y + (rect.h - block) / 2,
        _ => rect.y + rect.h - block,
    };
    for (n, line) in lines.iter().enumerate() {
        let w = width_of(font, line);
        let x = match x_align {
            1 => rect.x + (rect.w - w) / 2,
            2 => rect.x + rect.w - w,
            _ => rect.x,
        };
        draw_line(canvas, font, line, x, top + n as i32 * lh, rgb, shadow);
    }
}

/// Width of the widest line of `text` after tag removal (no wrapping).
pub fn text_width(font: &Font, text: &str) -> i32 {
    parse(text).iter().map(|l| width_of(font, l)).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        let l = parse("a<col=ff0000>b</col><br>c<lt>d<img=3><shad=0>");
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].iter().map(|g| g.c).collect::<Vec<_>>(), b"ab");
        assert_eq!(l[0][1].colour, Some(0xff0000));
        assert_eq!(l[0][0].colour, None);
        assert_eq!(l[1].iter().map(|g| g.c).collect::<Vec<_>>(), b"c<d");
    }
}
