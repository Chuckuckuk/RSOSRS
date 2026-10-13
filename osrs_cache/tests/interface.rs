//! Interface component decoding against the locally installed cache (skipped when absent).

use osrs_cache::interface::{self, Component};
use osrs_cache::*;

#[test]
fn all_interface_components_decode_exactly() {
    let Ok(c) = Cache::open_default() else { return };
    let index = c.index(idx::INTERFACES).unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for &g in index.groups.keys() {
        for (f, d) in c.read_files(&index, g).unwrap() {
            match Component::decode(g, f, &d) {
                Ok(_) => ok += 1,
                Err(e) => bad.push(e.to_string()),
            }
        }
    }
    assert!(bad.is_empty(), "{} of {} components failed: {:?}", bad.len(), ok + bad.len(), &bad[..bad.len().min(5)]);
    assert!(ok > 20_000);
}

#[test]
fn toplevel_161_spot_checks() {
    let Ok(c) = Cache::open_default() else { return };
    let comps = interface::load_group(&c, 161).unwrap();
    let first = &comps[0];
    assert!(first.is_if3);
    assert_eq!(first.kind, 0, "component 0 is a layer");
    assert_eq!(first.id, 161 << 16);
    assert!(first.parent.is_none());
    // a sprite and a text component exist, and children point at a parent inside the group
    assert!(comps.iter().any(|c| c.kind == 5 && c.sprite_id >= 0));
    assert!(comps.iter().any(|c| c.parent.is_some()));
    assert!(comps.iter().any(|c| c.has_listener()));
    assert!(interface::group_ids(&c).unwrap().contains(&161));
}
