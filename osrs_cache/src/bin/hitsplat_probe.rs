//! Prints the hit splat definitions the game uses for the player's hits and blocks, and writes
//! their sprites to a PNG: hitsplat_probe [out.png]
use osrs_cache::config::Configs;
use osrs_cache::hitsplat::{kind, load_hitsplat};
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
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let all = configs.hitsplats()?;
    let mut sheet: Vec<(u32, osrs_cache::hitsplat::HitSplatArt)> = Vec::new();
    for k in [kind::BLOCK_ME, 13, kind::DAMAGE_ME, 17, 43, 65, 4, 5, 6] {
        let Some(def) = all.get(&k) else { println!("type {k}: none"); continue };
        let real = def.resolve(&all);
        println!("type {k} -> {} {:?}", real.id, real);
        if let Some(art) = load_hitsplat(&cache, &configs, k)? {
            println!("   sprite {}x{} colour {:06x} {:.2}s", art.sprite.width, art.sprite.height, art.text_color, art.seconds);
            sheet.push((k, art));
        }
    }
    if let Some(font) = osrs_cache::font::Font::load(&cache, "p11_full")? {
        println!("p11_full: line {} px, '4' advances {}", font.height, font.width("4"));
        let text = font.render("0123456789 Hit 14", 0xFFFFFF, true);
        sheet.push((0, osrs_cache::hitsplat::HitSplatArt { sprite: text, text_color: 0, offset_y: 0, seconds: 0.0 }));
    } else {
        println!("p11_full: not found");
    }
    if let Some(out) = std::env::args().nth(1) {
        let scale = 4usize;
        let cell = 40 * scale;
        let (w, h) = (cell * sheet.len().max(1), cell);
        let mut rgb = vec![90u8; w * h * 3];
        for (n, (_, art)) in sheet.iter().enumerate() {
            let t = &art.sprite;
            for y in 0..t.height as usize * scale {
                for x in 0..t.width as usize * scale {
                    let (sx, sy) = (x / scale, y / scale);
                    let p = (sy * t.width as usize + sx) * 4;
                    if t.rgba[p + 3] == 0 || x + n * cell >= w || y >= h {
                        continue;
                    }
                    let o = (y * w + x + n * cell) * 3;
                    rgb[o..o + 3].copy_from_slice(&t.rgba[p..p + 3]);
                }
            }
        }
        write_png(&out, w as u32, h as u32, &rgb)?;
    }
    Ok(())
}
