//! Contact sheet of texture images: tex_dump <out.png> [first] [count]
use osrs_cache::textures::load_textures;
use osrs_cache::*;
use std::io::Write;

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for n in 0..256u32 {
        let mut c = n;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[n as usize] = c;
    }
    let mut c = 0xFFFF_FFFFu32;
    for &b in data {
        c = table[((c ^ b as u32) & 255) as usize] ^ (c >> 8);
    }
    c ^ 0xFFFF_FFFF
}

fn write_png(path: &str, w: u32, h: u32, rgb: &[u8]) -> std::io::Result<()> {
    let mut raw = Vec::with_capacity((w * 3 + 1) as usize * h as usize);
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
    let a: Vec<String> = std::env::args().collect();
    let first: u32 = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(0);
    let count: u32 = a.get(3).map(|s| s.parse().unwrap()).unwrap_or(40);
    let cache = Cache::open_default()?;
    let tex = load_textures(&cache)?;
    println!("{} textures decoded", tex.len());
    let cols = 8u32;
    let rows = count.div_ceil(cols);
    let (w, h) = (cols * 128, rows * 128);
    let mut img = vec![255u8; (w * h * 3) as usize];
    for i in 0..count {
        let Some(t) = tex.get(&(first + i)) else { continue };
        let (ox, oy) = ((i % cols) * 128, (i / cols) * 128);
        for y in 0..t.height.min(128) {
            for x in 0..t.width.min(128) {
                let s = ((y * t.width + x) * 4) as usize;
                let o = (((oy + y) * w + ox + x) * 3) as usize;
                let al = t.rgba[s + 3] as f32 / 255.0;
                let bg = if (x / 8 + y / 8) % 2 == 0 { 255.0 } else { 60.0 };
                for k in 0..3 {
                    img[o + k] = (t.rgba[s + k] as f32 * al + bg * (1.0 - al)) as u8;
                }
            }
        }
    }
    write_png(&a[1], w, h, &img)?;
    Ok(())
}
