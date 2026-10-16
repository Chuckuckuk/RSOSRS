//! Find object definitions by name: obj_search <text> [<text>...]   (case-insensitive substring)
use osrs_cache::config::Configs;
use osrs_cache::*;
fn main() -> Result<()> {
    let needles: Vec<String> = std::env::args().skip(1).map(|s| s.to_lowercase()).collect();
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    let mut ids: Vec<_> = defs.keys().copied().collect();
    ids.sort_unstable();
    for id in ids {
        let d = &defs[&id];
        let name = d.name.to_lowercase();
        if !d.models.is_empty() && needles.iter().any(|n| name.contains(n)) {
            println!("{id}: '{}' size {}x{} models {:?}", d.name, d.size_x, d.size_y, d.models);
        }
    }
    Ok(())
}
