//! The door table against the game cache (skipped when the cache is absent): every door's closed
//! form is opened with "Open" and its open form closed with "Close", the two are the same kind of
//! door, and the open form blocks (the server gives an open door its wall).

use osrs_cache::config::Configs;
use osrs_cache::Cache;
use osrs_core::data::Registry;

#[test]
fn every_door_pairs_an_open_option_with_a_close_option() {
    let Some(defs) = Cache::open_default().ok().and_then(|cache| Configs::new(&cache).ok()?.objects().ok()) else { return };
    let registry = Registry::builtin();
    let options = |id: u32| -> Vec<String> {
        defs[&id].actions.iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect()
    };

    for door in registry.doors() {
        for leaf in &door.leaves {
            let (closed, open) = (&defs[&leaf.closed], &defs[&leaf.open]);
            assert_eq!(options(leaf.closed), vec!["open"], "door '{}': {} opens", door.id, leaf.closed);
            assert_eq!(options(leaf.open), vec!["close"], "door '{}': {} closes", door.id, leaf.open);
            assert!(closed.name.eq_ignore_ascii_case(&open.name), "door '{}': {} and {} are the same door", door.id, leaf.closed, leaf.open);
            assert_ne!(open.interact_type, 0, "door '{}': {} blocks while open", door.id, leaf.open);
        }
    }
}
