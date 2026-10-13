//! Dump config group 35 (areas / map function definitions): area_probe [group] [max]
use osrs_cache::config::Configs;
use osrs_cache::*;
fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let group: u32 = a.get(1).map(|s| s.parse().unwrap()).unwrap_or(35);
    let max: usize = a.get(2).map(|s| s.parse().unwrap()).unwrap_or(40);
    let c = Cache::open_default()?;
    let files = Configs::new(&c)?.group_files(group)?;
    println!("{} files", files.len());
    for (id, d) in files.iter().take(max) {
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
        println!("{id}: {hex}");
    }
    Ok(())
}
