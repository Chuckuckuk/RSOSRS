//! Sprite contact sheet: sprite_sheet <out.png> <first> <last>
//! (FRAMES=1 lays out every frame of each sprite; COLS and CELL set the grid)
use osrs_cache::font::decode_frames;
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

fn write_png(path: &str, w: u32, h: u32, rgb: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::new();
    for y in 0..h as usize {
        raw.push(0);
        raw.extend_from_slice(&rgb[y * w as usize * 3..(y + 1) * w as usize * 3]);
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
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &idat);
    chunk(b"IEND", &[]);
    std::fs::write(path, out)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (first, last): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let cache = Cache::open_default()?;
    let sprites = cache.index(idx::SPRITES)?;
    let cell: usize = std::env::var("CELL").ok().and_then(|v| v.parse().ok()).unwrap_or(48);
    let ids: Vec<u32> = (first..=last).filter(|id| sprites.groups.contains_key(id)).collect();
    // every picture to lay out: the first frame of each sprite, or with FRAMES=1 every frame of
    // each (the frames of one group, such as the overhead icons, are often the interesting part)
    let all_frames = std::env::var("FRAMES").is_ok_and(|v| v == "1");
    let mut pictures = Vec::new();
    for id in &ids {
        let files = cache.read_files(&sprites, *id)?;
        let frames = files.first().map(|(_, f)| decode_frames(f)).unwrap_or_default();
        if let Some(f) = frames.first() {
            println!("{id}: {} frames, first {}x{} canvas {}x{}", frames.len(), f.width, f.height, f.canvas_width, f.canvas_height);
        }
        let take = if all_frames { frames.len() } else { 1 };
        pictures.extend(frames.into_iter().take(take));
    }
    let cols: usize = std::env::var("COLS").ok().and_then(|v| v.parse().ok()).unwrap_or(16);
    let rows = pictures.len().div_ceil(cols).max(1);
    let (w, h) = (cell * cols, cell * rows);
    let mut rgb = vec![90u8; w * h * 3];
    for (n, f) in pictures.iter().enumerate() {
        let (ox, oy) = ((n % cols) * cell, (n / cols) * cell);
        for y in 0..f.height.min(cell) {
            for x in 0..f.width.min(cell) {
                let p = (y * f.width + x) * 4;
                if f.rgba[p + 3] == 0 {
                    continue;
                }
                let o = ((oy + y) * w + ox + x) * 3;
                rgb[o..o + 3].copy_from_slice(&f.rgba[p..p + 3]);
            }
        }
    }
    write_png(&args[1], w as u32, h as u32, &rgb)?;
    Ok(())
}
