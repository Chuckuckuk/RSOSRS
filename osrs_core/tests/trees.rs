//! The gathering nodes against the game cache (skipped when the cache is absent): every resource
//! object offers the node's option, turns into an object of its own size that does not, and
//! every product is an item the cache names the same way (so its icon and model resolve).

use osrs_cache::config::Configs;
use osrs_cache::item::ids_by_name;
use osrs_cache::Cache;
use osrs_core::data::Registry;

#[test]
fn every_resource_offers_its_option_and_depletes_into_an_object_of_its_size() {
    let Some(defs) = Cache::open_default().ok().and_then(|cache| Configs::new(&cache).ok()?.objects().ok()) else { return };
    let registry = Registry::builtin();
    let offers = |id: u32, action: &str| defs[&id].actions.iter().any(|a| a.eq_ignore_ascii_case(action));

    for node in registry.gathering_nodes() {
        for loc in &node.locs {
            let (resource, depleted) = (&defs[&loc.loc], &defs[&loc.depleted]);
            assert!(offers(loc.loc, &node.action), "'{}': {} '{}' offers '{}'", node.id, loc.loc, resource.name, node.action);
            assert!(!offers(loc.depleted, &node.action), "'{}': {} is depleted", node.id, loc.depleted);
            assert_eq!(
                (resource.size_x, resource.size_y),
                (depleted.size_x, depleted.size_y),
                "'{}': {} and {} are the same size",
                node.id,
                loc.loc,
                loc.depleted
            );
        }
    }
}

#[test]
fn every_product_is_an_item_in_the_cache() {
    let Some(items) = Cache::open_default().ok().and_then(|cache| Configs::new(&cache).ok()?.items().ok()) else { return };
    let by_name = ids_by_name(&items);
    let registry = Registry::builtin();

    for node in registry.gathering_nodes() {
        let name = registry.item(&node.product).map(|item| item.name.to_lowercase()).expect("validated");
        assert!(by_name.contains_key(&name), "'{}': the cache has no item named '{name}'", node.id);
    }
}
