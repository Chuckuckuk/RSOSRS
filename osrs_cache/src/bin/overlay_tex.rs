//! Which overlay textures a map square uses: overlay_tex <rx> <ry>
use osrs_cache::config::Configs;
use osrs_cache::*;
use std::collections::BTreeMap;
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (rx, ry): (u32, u32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let c = Cache::open_default()?;
    let ov = Configs::new(&c)?.overlays()?;
    let maps = c.index(idx::MAPS)?;
    let files = c.read_files(&maps, (rx << 8) | ry)?;
    let t = terrain::Terrain::decode(&files[0].1, rx as i32 * 64, ry as i32 * 64)?;
    let mut count: BTreeMap<u16, u32> = BTreeMap::new();
    for x in 0..64 { for z in 0..64 { let o = t.tile(0, x, z).overlay; if o != 0 { *count.entry(o).or_default() += 1; } } }
    for (o, n) in count {
        let d = &ov[&(o as u32 - 1)];
        println!("overlay {} x{n}: texture {:?} rgb {:06x}", o - 1, d.texture, d.rgb);
    }
    Ok(())
}
