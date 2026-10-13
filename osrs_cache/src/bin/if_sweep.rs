//! Decodes every interface component and reports failures: if_sweep [max failures to print]
use osrs_cache::interface::Component;
use osrs_cache::*;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let index = cache.index(idx::INTERFACES)?;
    let (mut ok, mut bad, mut if1, mut if3) = (0, 0, 0, 0);
    let max: usize = std::env::args().nth(1).and_then(|v| v.parse().ok()).unwrap_or(20);
    for &g in index.groups.keys() {
        let files = match cache.read_files(&index, g) {
            Ok(f) => f,
            Err(e) => {
                println!("group {g}: {e}");
                continue;
            }
        };
        for (f, d) in files {
            match Component::decode(g, f, &d) {
                Ok(c) => {
                    ok += 1;
                    if c.is_if3 { if3 += 1 } else { if1 += 1 }
                }
                Err(e) => {
                    bad += 1;
                    if bad <= max {
                        let hex: String = d.iter().take(100).map(|b| format!("{b:02x}")).collect();
                        println!("{e}\n  {hex}");
                    }
                }
            }
        }
    }
    println!("groups {} ok {ok} (if3 {if3}, if1 {if1}) failed {bad}", index.groups.len());
    Ok(())
}
