//! Render an interface with the software UI renderer:
//! if_render <fixed|classic|modern|group id> <W>x<H> <out.png> [--tab N] [--mount comp=group]...
//! `comp` is a full component id (decimal or 0x..) or `group:file`.
use osrs_cache::interface::load_group;
use osrs_cache::ui::{self, standard_state, CacheAssets, Layout};
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

fn comp_id(s: &str) -> u32 {
    if let Some((g, f)) = s.split_once(':') {
        return g.parse::<u32>().unwrap() << 16 | f.parse::<u32>().unwrap();
    }
    match s.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16).unwrap(),
        None => s.parse().unwrap(),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: if_render <fixed|classic|modern|group> <W>x<H> <out.png> [--tab N] [--mount comp=group]...");
        std::process::exit(2);
    }
    let (w, h) = args[2].split_once('x').map(|(a, b)| (a.parse::<i32>().unwrap(), b.parse::<i32>().unwrap())).unwrap();
    let mut tab = None;
    let mut mounts = Vec::new();
    let mut crop: Option<(i32, i32, i32, i32, i32)> = None;
    let mut i = 4;
    while i < args.len() {
        match args[i].as_str() {
            "--tab" => {
                tab = Some(args[i + 1].parse::<usize>().unwrap());
                i += 1;
            }
            "--mount" => {
                let (c, g) = args[i + 1].split_once('=').unwrap();
                mounts.push((comp_id(c), g.parse::<u32>().unwrap()));
                i += 1;
            }
            "--crop" => {
                let v: Vec<i32> = args[i + 1].split(',').map(|v| v.parse().unwrap()).collect();
                crop = Some((v[0], v[1], v[2], v[3], v[4]));
                i += 1;
            }
            o => panic!("unknown option {o}"),
        }
        i += 1;
    }
    let cache = Cache::open_default()?;
    let mut state = match args[1].as_str() {
        "fixed" => standard_state(&cache, Layout::Fixed, tab)?,
        "classic" => standard_state(&cache, Layout::Classic, tab)?,
        "modern" => standard_state(&cache, Layout::Modern, tab)?,
        g => {
            let g: u32 = g.parse().expect("layout name or group id");
            let mut s = ui::UiState { top: g, ..Default::default() };
            s.groups.insert(g, load_group(&cache, g)?);
            s
        }
    };
    for (c, g) in mounts {
        state.groups.insert(g, load_group(&cache, g)?);
        state.mounts.insert(c, g);
    }
    let mut assets = CacheAssets::new(&cache)?;
    let (canvas, hits) = ui::draw(&state, &mut assets, w, h);
    let mut rgb = vec![0u8; (w * h * 3) as usize];
    for p in 0..(w * h) as usize {
        let a = canvas.rgba[p * 4 + 3] as u32;
        for k in 0..3 {
            let bg = 0x30u32;
            rgb[p * 3 + k] = ((canvas.rgba[p * 4 + k] as u32 * a + bg * (255 - a)) / 255) as u8;
        }
    }
    if let Some((cx, cy, cw, ch, k)) = crop {
        // x, y, w, h, integer zoom
        let (ow, oh) = (cw * k, ch * k);
        let mut out = vec![0u8; (ow * oh * 3) as usize];
        for y in 0..oh {
            for x in 0..ow {
                let (sx, sy) = ((cx + x / k).min(w - 1), (cy + y / k).min(h - 1));
                let o = ((y * ow + x) * 3) as usize;
                let s = ((sy * w + sx) * 3) as usize;
                out[o..o + 3].copy_from_slice(&rgb[s..s + 3]);
            }
        }
        write_png(&args[3], ow as u32, oh as u32, &out)?;
    } else {
        write_png(&args[3], w as u32, h as u32, &rgb)?;
    }
    println!("wrote {} ({} hits)", args[3], hits.len());
    Ok(())
}
