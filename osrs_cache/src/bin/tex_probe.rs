use osrs_cache::*;
use std::collections::BTreeMap;
fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let tex = c.index(idx::TEXTURES)?;
    let files = c.read_files(&tex, 0)?;
    let mut sizes: BTreeMap<usize, u32> = BTreeMap::new();
    for (_, d) in &files { *sizes.entry(d.len()).or_default() += 1; }
    println!("texture def sizes: {sizes:?}");
    for (id, d) in files.iter().filter(|(i, d)| *i < 4 || d.len() != 7).take(8) {
        println!("tex {id}: {:02x?}", d);
    }
    let sprites = c.index(idx::SPRITES)?;
    println!("sprite groups {}", sprites.groups.len());
    let g = sprites.groups.get(&447).unwrap();
    println!("group 447: files {:?}", g.files.iter().map(|f| f.id).collect::<Vec<_>>());
    let d = c.read_files(&sprites, 447)?;
    let b = &d[0].1;
    println!("sprite 447: {} bytes, tail {:02x?}", b.len(), &b[b.len().saturating_sub(24)..]);
    Ok(())
}
