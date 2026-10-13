//! Map squares with objects whose models use a scrolling texture: scan_scroll
use osrs_cache::config::Configs;
use osrs_cache::model::Model;
use osrs_cache::*;
use std::collections::{BTreeMap, HashSet};
fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    let anim = |t: i32| t >= 0 && matches!(t as u32, 17 | 24 | 34 | 40 | 56 | 57 | 59 | 99..=115);
    let models = c.index(idx::MODELS)?;
    let mut animated: HashSet<u32> = HashSet::new();
    for id in models.groups.keys() {
        let Ok(data) = c.read_group(idx::MODELS, *id) else { continue };
        if let Ok(m) = Model::decode(*id, &data) {
            if m.textures.iter().any(|t| anim(*t as i32)) { animated.insert(*id); }
        }
    }
    println!("{} models use scrolling textures", animated.len());
    let objs: HashSet<u32> = defs.iter().filter(|(_, d)| d.models.iter().any(|m| animated.contains(m))).map(|(i, _)| *i).collect();
    println!("{} object definitions", objs.len());
    let maps = c.index(idx::MAPS)?;
    let mut rows: Vec<(u32, u32, u32, BTreeMap<u32, u32>)> = Vec::new();
    for gid in maps.groups.keys() {
        let Ok(files) = c.read_files(&maps, *gid) else { continue };
        if files.len() < 2 { continue; }
        let mut by: BTreeMap<u32, u32> = BTreeMap::new();
        for p in locs::decode(&files[1].1) {
            if objs.contains(&p.id) { *by.entry(p.id).or_default() += 1; }
        }
        if !by.is_empty() {
            rows.push((by.values().sum(), gid >> 8, gid & 255, by));
        }
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    for (n, rx, ry, by) in rows.iter().take(15) {
        let names: Vec<String> = by.iter().map(|(i, c)| format!("{}x{} '{}'", i, c, defs[i].name)).collect();
        println!("region {rx} {ry}: {n} objects: {}", names.join(", "));
    }
    Ok(())
}
