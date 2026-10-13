//! List placements of a given kind in a map square: find_locs <rx> <ry> <kind|255=any> [x z radius]
use osrs_cache::config::Configs;
use osrs_cache::*;
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (rx, ry, kind): (u32, u32, u8) = (a[1].parse().unwrap(), a[2].parse().unwrap(), a[3].parse().unwrap());
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    let maps = c.index(idx::MAPS)?;
    let files = c.read_files(&maps, (rx << 8) | ry)?;
    let near: Option<(i32, i32, i32)> = (a.len() >= 7).then(|| (a[4].parse().unwrap(), a[5].parse().unwrap(), a[6].parse().unwrap()));
    let found = locs::decode(&files[1].1).into_iter().filter(|p| {
        (kind == 255 || p.kind == kind)
            && (std::env::var("ALLPLANES").is_ok() || p.plane == 0)
            && near.map_or(true, |(x, z, r)| (p.x as i32 - x).abs() <= r && (p.z as i32 - z).abs() <= r)
    });
    for p in found.take(if near.is_some() { 100000 } else { 12 }) {
        println!("plane {} tile ({}, {}) kind {} id {} '{}' orientation {}", p.plane, p.x, p.z, p.kind, p.id, defs[&p.id].name, p.orientation);
    }
    Ok(())
}
