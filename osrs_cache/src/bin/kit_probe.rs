//! Dump raw identity-kit definitions (config archive group 3): kit_probe [max_id]
use osrs_cache::*;

fn main() -> Result<()> {
    let max: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(60);
    let c = Cache::open_default()?;
    let index = c.index(idx::CONFIG)?;
    let files = c.read_files(&index, cfg::IDENTKIT)?;
    println!("{} identity kits", files.len());
    for (id, d) in files.iter().filter(|(id, _)| *id <= max) {
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
        println!("kit {id}: {hex}");
    }
    Ok(())
}
