//! Top-down raster of the terrain mesh, for checking decoding without a 3D viewer.
//! usage: terrain_png <region_x> <region_y> <nx> <ny> <out.png> [px_per_tile=8]

use osrs_cache::config::Configs;
use osrs_cache::terrain_mesh::TerrainGrid;
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
    if a.len() < 6 {
        eprintln!("usage: terrain_png <region_x> <region_y> <nx> <ny> <out.png> [px_per_tile]");
        return Ok(());
    }
    let (rx, ry): (i32, i32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let (nx, ny): (usize, usize) = (a[3].parse().unwrap(), a[4].parse().unwrap());
    let px: f32 = a.get(6).map(|s| s.parse().unwrap()).unwrap_or(8.0);
    let cache = Cache::open_default()?;
    let cfg = Configs::new(&cache)?;
    let grid = TerrainGrid::load(&cache, rx, ry, nx, ny, cfg.underlays()?, cfg.overlays()?)?;
    if !grid.missing.is_empty() {
        eprintln!("missing squares: {:?}", grid.missing);
    }
    let mesh = grid.build_mesh(0);
    let w = (grid.width as f32 * px) as usize;
    let h = (grid.depth as f32 * px) as usize;
    let mut img = vec![20u8; w * h * 3];
    let to_srgb = |c: f32| (c.clamp(0.0, 1.0).powf(1.0 / 2.2) * 255.0) as u8;
    for tri in mesh.indices.chunks_exact(3) {
        let p: Vec<(f32, f32)> = tri
            .iter()
            .map(|&i| {
                let v = mesh.positions[i as usize];
                (v[0] * px, (grid.depth as f32 + v[2]) * px) // z is -north; image y grows southwards
            })
            .collect();
        let c: Vec<[f32; 4]> = tri.iter().map(|&i| mesh.colors[i as usize]).collect();
        let minx = p.iter().map(|q| q.0).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let maxx = (p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as usize).min(w.saturating_sub(1));
        let miny = p.iter().map(|q| q.1).fold(f32::MAX, f32::min).floor().max(0.0) as usize;
        let maxy = (p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as usize).min(h.saturating_sub(1));
        let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
        if area.abs() < 1e-6 {
            continue;
        }
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((p[1].0 - fx) * (p[2].1 - fy) - (p[2].0 - fx) * (p[1].1 - fy)) / area;
                let w1 = ((p[2].0 - fx) * (p[0].1 - fy) - (p[0].0 - fx) * (p[2].1 - fy)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < -0.001 || w1 < -0.001 || w2 < -0.001 {
                    continue;
                }
                let o = (y * w + x) * 3;
                for k in 0..3 {
                    img[o + k] = to_srgb(c[0][k] * w0 + c[1][k] * w1 + c[2][k] * w2);
                }
            }
        }
    }
    write_png(&a[5], w as u32, h as u32, &img)?;
    println!("wrote {} ({}x{}), {} triangles", a[5], w, h, mesh.indices.len() / 3);
    Ok(())
}
