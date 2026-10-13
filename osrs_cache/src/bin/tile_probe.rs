//! Prints the terrain data of a world tile on every plane: tile_probe <x> <z>
use osrs_cache::config::Configs;
use osrs_cache::squares::MapSource;
use osrs_cache::*;

fn main() -> Result<()> {
    let a: Vec<i32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let [x, z] = a[..] else { panic!("usage: tile_probe <x> <z>") };
    let source = MapSource::open()?;
    let (sx, sy) = (x.div_euclid(64), z.div_euclid(64));
    let around = source.neighbourhood(sx, sy)?;
    let overlays = Configs::new(source.cache())?.overlays()?;
    let (gx, gz) = ((x - sx * 64 + 64) as usize, (z - sy * 64 + 64) as usize);
    for dz in -2i32..=2 {
        for dx in -2i32..=2 {
            let (tx, tz) = ((gx as i32 + dx) as usize, (gz as i32 + dz) as usize);
            print!("({:+},{:+}) ", dx, dz);
            for p in 0..4 {
                let t = around.grid.tile(p, tx, tz);
                let ov = if t.overlay != 0 {
                    overlays.get(&(t.overlay as u32 - 1)).map(|o| format!("rgb={:06x} tex={:?} hide={}", o.rgb, o.texture, o.hide_underlay)).unwrap_or_default()
                } else {
                    String::new()
                };
                print!("| p{p}: h={} ul={} ov={} set={} {ov} ", t.height, t.underlay, t.overlay, t.settings);
            }
            println!();
        }
    }
    Ok(())
}
