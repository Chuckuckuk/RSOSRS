//! Prints the raw files of one config group: group_probe <group>
use osrs_cache::config::Configs;
use osrs_cache::*;

fn main() -> Result<()> {
    let group: u32 = std::env::args().nth(1).and_then(|g| g.parse().ok()).unwrap_or(33);
    let cache = Cache::open_default()?;
    for (id, data) in Configs::new(&cache)?.group_files(group)? {
        let hex: String = data.iter().map(|b| format!("{b:02x}")).collect();
        println!("{id}: {hex}");
    }
    Ok(())
}
