//! Print the trailing 3 bytes of every texture definition, grouped by value.
use osrs_cache::*;
use std::collections::BTreeMap;
fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let tex = c.index(idx::TEXTURES)?;
    let mut by: BTreeMap<Vec<u8>, Vec<u32>> = BTreeMap::new();
    for g in tex.groups.keys() {
        for (id, d) in c.read_files(&tex, *g)? {
            by.entry(d[4..].to_vec()).or_default().push(id);
        }
    }
    for (k, v) in &by {
        println!("{:02x?}: {} textures, ids {:?}", k, v.len(), &v[..v.len().min(12)]);
    }
    Ok(())
}
