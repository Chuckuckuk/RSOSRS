//! Counts overlay tiles by colour on each plane over a block of squares:
//! overlay_census <first square x> <first square y> <wide> <high>
use osrs_cache::config::Configs;
use osrs_cache::squares::MapSource;
use osrs_cache::*;
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let a: Vec<i32> = std::env::args().skip(1).filter_map(|v| v.parse().ok()).collect();
    let [x0, y0, w, h] = a[..] else { panic!("usage: overlay_census <x> <y> <w> <h>") };
    let source = MapSource::open()?;
    let overlays = Configs::new(source.cache())?.overlays()?;
    // (plane, overlay id) -> count
    let mut counts: BTreeMap<(usize, u16), usize> = BTreeMap::new();
    for sy in y0..y0 + h {
        for sx in x0..x0 + w {
            let n = source.neighbourhood(sx, sy)?;
            for plane in 0..4 {
                for x in 64..128 {
                    for z in 64..128 {
                        let t = n.grid.tile(plane, x, z);
                        if t.overlay != 0 {
                            *counts.entry((plane, t.overlay)).or_default() += 1;
                        }
                    }
                }
            }
        }
    }
    for plane in 0..4 {
        println!("plane {plane}:");
        let mut rows: Vec<_> = counts.iter().filter(|((p, _), _)| *p == plane).collect();
        rows.sort_by_key(|(_, c)| std::cmp::Reverse(**c));
        for ((_, id), count) in rows.into_iter().take(8) {
            let o = overlays.get(&(*id as u32 - 1));
            println!(
                "  overlay {id}: {count} tiles, rgb {:06x}, texture {:?}",
                o.map(|o| o.rgb).unwrap_or(0),
                o.and_then(|o| o.texture)
            );
        }
    }
    Ok(())
}
