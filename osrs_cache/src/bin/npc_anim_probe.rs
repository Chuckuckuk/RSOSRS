//! How an NPC's model moves in each of some sequences, to tell attack animations apart without a
//! viewer: npc_anim_probe <npc id> <sequence id>...
//! For each sequence: whether it bakes against the NPC's rig, its frame count and length, and the
//! model's top height (in tiles) at its lowest and highest frame against the bind pose.
//! With STRIP=<out.png> it also draws each sequence as a row of side-on silhouettes (the model's
//! vertices seen from its right, eight frames evenly through the animation), to see the motion.
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::config::Configs;
use osrs_cache::npc::build_npc;
use osrs_cache::*;
use std::io::Write;

/// Pixels per silhouette cell, and frames drawn per sequence.
const CELL: usize = 96;
const STRIP_FRAMES: usize = 8;

/// Client units per tile. The rig is in client units with y pointing down.
const UNITS: f32 = 128.0;

/// Height of the highest vertex in tiles.
fn top(positions: &[[f32; 3]]) -> f32 {
    positions.iter().map(|p| -p[1] / UNITS).fold(f32::MIN, f32::max)
}

/// The furthest any vertex has moved from where it sits in the bind pose, in tiles.
fn max_shift(bind: &[[f32; 3]], posed: &[[f32; 3]]) -> f32 {
    bind.iter()
        .zip(posed)
        .map(|(a, b)| ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt())
        .fold(0.0, f32::max)
        / UNITS
}

fn main() -> Result<()> {
    let args: Vec<u32> = std::env::args().skip(1).filter_map(|a| a.parse().ok()).collect();
    let Some((&npc_id, sequences)) = args.split_first() else {
        eprintln!("usage: npc_anim_probe <npc id> <sequence id>...");
        return Ok(());
    };
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let defs = configs.npcs()?;
    let hsl = configs.texture_average_hsl()?;
    let Some(def) = defs.get(&npc_id) else {
        println!("no NPC {npc_id}");
        return Ok(());
    };
    let Some(mesh) = build_npc(&cache, def, &hsl) else {
        println!("NPC {npc_id} has no models");
        return Ok(());
    };
    let rig = mesh.rig.as_ref().expect("NPC meshes keep their rig");
    println!("{} ({npc_id}): bind-pose top {:.2} tiles", def.name, top(&rig.positions));
    let mut anims = Animations::new(&cache)?;
    let mut rows: Vec<Vec<Vec<[f32; 3]>>> = Vec::new();
    for &sequence in sequences {
        match Clip::bake(&mut anims, sequence, rig) {
            Ok(clip) => {
                let picks = (0..STRIP_FRAMES).map(|i| clip.frames[i * clip.frames.len() / STRIP_FRAMES].clone()).collect();
                rows.push(picks);
                let tops: Vec<f32> = clip.frames.iter().map(|f| top(f)).collect();
                if std::env::var("HEAD").is_ok() {
                    // where a monster's breath or fireball would start in each frame: the front
                    // of the vertices within 1.2 tiles of the top (as osrs_render's head_front)
                    for (i, f) in clip.frames.iter().enumerate() {
                        let t = top(f);
                        let head: Vec<&[f32; 3]> = f.iter().filter(|p| -p[1] / UNITS >= t - 1.2).collect();
                        let x = head.iter().map(|p| p[0] / UNITS).sum::<f32>() / head.len() as f32;
                        let front = head.iter().map(|p| -p[2] / UNITS).fold(f32::MIN, f32::max);
                        println!("    frame {i:2}: top {t:.2}, head x {x:.2}, mouth at y {:.2} z {front:.2}", t - 0.6);
                    }
                }
                let shift = clip.frames.iter().map(|f| max_shift(&rig.positions, f)).fold(0.0, f32::max);
                println!(
                    "  seq {sequence}: {} frames, {:.2} s, top {:.2}..{:.2} tiles, largest vertex move {:.2} tiles",
                    clip.frames.len(),
                    clip.duration() as f32 * 0.02,
                    tops.iter().copied().fold(f32::MAX, f32::min),
                    tops.iter().copied().fold(f32::MIN, f32::max),
                    shift
                );
            }
            Err(e) => {
                println!("  seq {sequence}: does not bake ({e})");
                rows.push(Vec::new());
            }
        }
    }
    if let Ok(path) = std::env::var("STRIP") {
        write_strip(&path, &rows, &rig.positions)?;
        println!("wrote {path}: one row per sequence, in the order given");
    }
    Ok(())
}

/// Side-on silhouettes: x across (the model's z, its front to the right), height up, all frames
/// on one scale taken from the bind pose.
fn write_strip(path: &str, rows: &[Vec<Vec<[f32; 3]>>], bind: &[[f32; 3]]) -> std::io::Result<()> {
    let (w, h) = (CELL * STRIP_FRAMES, CELL * rows.len().max(1));
    let mut rgb = vec![40u8; w * h * 3];
    let extent = bind.iter().map(|p| p[1].abs().max(p[2].abs())).fold(1.0f32, f32::max) * 1.4;
    for (r, frames) in rows.iter().enumerate() {
        for (c, frame) in frames.iter().enumerate() {
            for p in frame {
                let x = (CELL as f32 / 2.0 + p[2] / extent * CELL as f32 / 2.0) as isize;
                let y = (CELL as f32 * 0.9 + p[1] / extent * CELL as f32 * 0.8) as isize;
                if x < 0 || y < 0 || x >= CELL as isize || y >= CELL as isize {
                    continue;
                }
                let (px, py) = (c * CELL + x as usize, r * CELL + y as usize);
                let o = (py * w + px) * 3;
                rgb[o..o + 3].copy_from_slice(&[230, 210, 120]);
            }
        }
    }
    write_png(path, w as u32, h as u32, &rgb)
}

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
