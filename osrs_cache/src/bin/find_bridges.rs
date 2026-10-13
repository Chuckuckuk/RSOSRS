//! List bridge deck tiles (plane-1 settings bit 2) of a map square: find_bridges <rx> <ry>
use osrs_cache::*;
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (rx, ry): (u32, u32) = (a[1].parse().unwrap(), a[2].parse().unwrap());
    let c = Cache::open_default()?;
    let maps = c.index(idx::MAPS)?;
    let files = c.read_files(&maps, (rx << 8) | ry)?;
    let t = terrain::Terrain::decode(&files[0].1, rx as i32 * 64, ry as i32 * 64)?;
    for z in (0..64).rev() {
        let row: String = (0..64).map(|x| if t.tile(1, x, z).settings & 2 != 0 { '#' } else { '.' }).collect();
        if row.contains('#') {
            println!("z={z:2} {row}");
        }
    }
    Ok(())
}
