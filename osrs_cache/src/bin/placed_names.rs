//! Lists the distinct objects placed in a block of map squares as `id<TAB>name` lines.
//!
//! usage: placed_names <first square x> <first square y> <squares wide> <squares high>

use osrs_cache::squares::MapSource;
use std::collections::BTreeMap;

fn main() {
    let a: Vec<i32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let [x0, y0, w, h] = a[..] else {
        eprintln!("usage: placed_names <x> <y> <w> <h>");
        std::process::exit(2);
    };
    let source = MapSource::open().expect("open the game cache");
    let mut found: BTreeMap<u32, String> = BTreeMap::new();
    for sy in y0..y0 + h {
        for sx in x0..x0 + w {
            let Ok(around) = source.neighbourhood(sx, sy) else { continue };
            for (gx, gz, p) in &around.placed.placements {
                // only the middle square's own placements
                if (64..128).contains(gx) && (64..128).contains(gz) {
                    if let Some(def) = source.object_defs().get(&p.id) {
                        if !def.name.is_empty() && def.name != "null" {
                            found.insert(p.id, def.name.clone());
                        }
                    }
                }
            }
        }
    }
    for (id, name) in found {
        println!("{id}\t{name}");
    }
}
