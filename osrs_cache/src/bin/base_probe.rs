//! base_probe <group>: raw frame-base file of an archive-1 group
use osrs_cache::*;
fn main() -> Result<()> {
    let g: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
    let c = Cache::open_default()?;
    let ix = c.index(idx::SKELETONS)?;
    let grp = ix.groups.get(&g).ok_or_else(|| Error::Missing(format!("group {g}")))?;
    println!("files {:?}", grp.files);
    let d = c.read_group(idx::SKELETONS, g)?;
    println!("{} bytes: {}", d.len(), d.iter().take(160).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" "));
    Ok(())
}
