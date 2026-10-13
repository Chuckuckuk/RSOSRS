//! Print an object's multiloc transforms: obj_transforms <id>...
use osrs_cache::config::Configs;
use osrs_cache::*;
fn main() -> Result<()> {
    let c = Cache::open_default()?;
    let defs = Configs::new(&c)?.objects()?;
    for a in std::env::args().skip(1) {
        let id: u32 = a.parse().unwrap();
        let d = &defs[&id];
        println!(
            "{id} '{}' varbit {:?} transforms {:?} models {:?} types {:?} size {}x{}",
            d.name, d.varbit, d.transforms, d.models, d.model_types, d.size_x, d.size_y
        );
        for t in d.transforms.iter().filter(|t| **t >= 0) {
            let t = &defs[&(*t as u32)];
            println!(
                "   -> {} '{}' models {:?} types {:?} size {}x{} offsets {} {} {}",
                t.id, t.name, t.models, t.model_types, t.size_x, t.size_y, t.offset_x, t.offset_y, t.offset_z
            );
        }
    }
    Ok(())
}
