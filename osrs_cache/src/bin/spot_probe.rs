//! Describes graphics (spot animations): spot_probe <id>...
use osrs_cache::config::Configs;
use osrs_cache::item_icon::describe;
use osrs_cache::spotanim::build_spotanim;
use osrs_cache::*;

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let configs = Configs::new(&cache)?;
    let defs = configs.spotanims()?;
    let hsl = configs.texture_average_hsl()?;
    for id in std::env::args().skip(1).filter_map(|a| a.parse::<u32>().ok()) {
        let Some(def) = defs.get(&id) else { println!("{id}: none"); continue };
        println!("{id}: {def:?}");
        if let Some(mesh) = build_spotanim(&cache, def, &hsl) {
            println!("   {}", describe(&mesh));
            // where along z each colour sits (to tell the head from the feathers)
            let mut by_colour: std::collections::BTreeMap<String, (f32, usize)> = Default::default();
            for (p, c) in mesh.opaque.positions.iter().zip(&mesh.opaque.colors) {
                let key = format!("{:.2}/{:.2}/{:.2}", c[0], c[1], c[2]);
                let e = by_colour.entry(key).or_insert((0.0, 0));
                e.0 += p[2];
                e.1 += 1;
            }
            for (colour, (sum, n)) in by_colour.iter().take(6) {
                println!("      colour {colour}: {n} vertices, mean z {:.3}", sum / *n as f32);
            }
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn unused() {}
