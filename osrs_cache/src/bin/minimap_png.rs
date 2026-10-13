//! Render the 3x3 map squares around a world tile as minimap pictures.
//! usage: minimap_png <x> <z> <out.png> [scale=1] [noicons]
//! Map function icons (from the area definitions) are drawn over the picture, centred on the object.

use osrs_cache::font::decode_frames;
use osrs_cache::minimap::{MinimapSource, SIZE};
use osrs_cache::*;
use std::io::Write;

fn crc32(data: &[u8]) -> u32 {
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c ^= b as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xFFFF_FFFF
}

fn write_png(path: &str, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((w * 4 + 1) as usize * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgba[y * w as usize * 4..(y + 1) * w as usize * 4]);
    }
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    z.write_all(&raw)?;
    let idat = z.finish()?;
    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &idat);
    chunk(b"IEND", &[]);
    std::fs::write(path, out)
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: minimap_png <x> <z> <out.png> [scale] [noicons]");
        return Ok(());
    }
    let (x, z): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let scale: usize = a.get(4).and_then(|s| s.parse().ok()).unwrap_or(1);
    let icons = !a.iter().any(|s| s == "noicons");
    let (sx, sz) = (x >> 6, z >> 6);
    let cache = Cache::open_default()?;
    let source = MinimapSource::new(&cache)?;
    let sprites = cache.index(idx::SPRITES)?;
    let side = SIZE * 3;
    let mut canvas = vec![0u8; side * side * 4];
    for i in 0..side * side {
        canvas[i * 4 + 3] = 255;
    }
    let mut all_icons = Vec::new();
    for dz in -1..=1 {
        for dx in -1..=1 {
            let m = source.render(sx + dx, sz + dz)?;
            let (ox, oy) = (((dx + 1) as usize) * SIZE, ((1 - dz) as usize) * SIZE);
            for py in 0..SIZE {
                for px in 0..SIZE {
                    let s = (py * SIZE + px) * 4;
                    if m.image.rgba[s + 3] != 0 {
                        let d = ((oy + py) * side + ox + px) * 4;
                        canvas[d..d + 4].copy_from_slice(&m.image.rgba[s..s + 4]);
                    }
                }
            }
            all_icons.extend(m.icons);
        }
    }
    println!("{} icons", all_icons.len());
    if icons {
        for ic in &all_icons {
            let Some(frame) = cache
                .read_files(&sprites, ic.sprite)
                .ok()
                .and_then(|f| f.first().map(|(_, d)| decode_frames(d)))
                .and_then(|f| f.into_iter().next())
            else {
                continue;
            };
            // centre of the footprint, in canvas pixels
            let cx = ((ic.x - (sx - 1) * 64) as usize * 4 + ic.size_x as usize * 2) as i32;
            let cy = (side as i32) - ((ic.z - (sz - 1) * 64) as usize * 4 + ic.size_z as usize * 2) as i32;
            let (x0, y0) = (cx - frame.width as i32 / 2, cy - frame.height as i32 / 2);
            for fy in 0..frame.height {
                for fx in 0..frame.width {
                    let s = (fy * frame.width + fx) * 4;
                    if frame.rgba[s + 3] == 0 {
                        continue;
                    }
                    let (px, py) = (x0 + fx as i32, y0 + fy as i32);
                    if px >= 0 && py >= 0 && (px as usize) < side && (py as usize) < side {
                        let d = (py as usize * side + px as usize) * 4;
                        canvas[d..d + 3].copy_from_slice(&frame.rgba[s..s + 3]);
                    }
                }
            }
        }
    }
    let ow = side * scale;
    let mut out = vec![0u8; ow * ow * 4];
    for y in 0..ow {
        for xx in 0..ow {
            let s = ((y / scale) * side + xx / scale) * 4;
            let d = (y * ow + xx) * 4;
            out[d..d + 4].copy_from_slice(&canvas[s..s + 4]);
        }
    }
    write_png(&a[3], ow as u32, ow as u32, &out)?;
    println!("wrote {} ({}x{})", a[3], ow, ow);
    Ok(())
}
