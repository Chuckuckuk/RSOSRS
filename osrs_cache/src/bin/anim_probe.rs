//! Probe animation storage: anim_probe <sequence id> [frame group file]
use osrs_cache::*;

fn hex(d: &[u8], n: usize) -> String {
    d.iter().take(n).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let id: u32 = a.get(1).and_then(|s| s.parse().ok()).unwrap_or(808);
    let c = Cache::open_default()?;
    let cfgi = c.index(idx::CONFIG)?;
    let files = c.read_files(&cfgi, cfg::SEQUENCE)?;
    println!("{} sequences", files.len());
    if let Some((_, d)) = files.iter().find(|(i, _)| *i == id) {
        println!("seq {id} ({} bytes): {}", d.len(), hex(d, 200));
    }
    for archive in [idx::ANIM_FRAMES, idx::SKELETONS] {
        let ix = c.index(archive)?;
        let n = ix.groups.len();
        println!("archive {archive}: {n} groups");
        for (g, grp) in ix.groups.iter().take(3) {
            println!("  group {g}: {} files", grp.files.len());
        }
    }
    if a.len() >= 4 {
        let (g, f): (u32, u32) = (a[2].parse().unwrap(), a[3].parse().unwrap());
        let ix = c.index(idx::ANIM_FRAMES)?;
        let d = c.read_file(&ix, g, f)?;
        println!("frame {g}/{f}: {} bytes: {}", d.len(), hex(&d, 120));
    }
    Ok(())
}
