//! Draws item inventory pictures to a PNG sheet: item_probe <out.png> <name or id>...
use osrs_cache::config::Configs;
use osrs_cache::item::ids_by_name;
use osrs_cache::item_icon::item_icon;
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
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let items = configs.items()?;
    let names = ids_by_name(&items);
    println!("{} items, {} names", items.len(), names.len());
    let texture_hsl = configs.texture_average_hsl()?;
    let scale = 4usize;
    let wanted: Vec<u32> = args[2..]
        .iter()
        .filter_map(|a| a.parse::<u32>().ok().or_else(|| names.get(&a.to_lowercase()).copied()))
        .collect();
    let (cell_w, cell_h) = (40 * scale, 36 * scale);
    let (w, h) = (cell_w * wanted.len().max(1), cell_h);
    let mut rgb = vec![110u8; w * h * 3];
    for (n, id) in wanted.iter().enumerate() {
        let Some(def) = items.get(id) else { continue };
        println!("{id} {:?} zoom {} pitch {} yaw {} roll {} off ({}, {}) options {:?}", def.name, def.zoom, def.pitch, def.yaw, def.roll, def.offset_x, def.offset_y, def.options);
        if let Some(mesh) = osrs_cache::item_icon::icon_mesh(&cache, def, &texture_hsl) {
            println!("   {}", osrs_cache::item_icon::describe(&mesh));
        }
        let Some(icon) = item_icon(&cache, def, &texture_hsl) else {
            println!("   no picture");
            continue;
        };
        for y in 0..icon.height as usize * scale {
            for x in 0..icon.width as usize * scale {
                let p = ((y / scale) * icon.width as usize + x / scale) * 4;
                if icon.rgba[p + 3] == 0 {
                    continue;
                }
                let o = (y * w + n * cell_w + x) * 3;
                rgb[o..o + 3].copy_from_slice(&icon.rgba[p..p + 3]);
            }
        }
    }
    write_png(&args[1], w as u32, h as u32, &rgb)?;
    Ok(())
}
