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

/// The server on the real map around the diagonal door `id` at (`x`, `z`), the door in its
/// scenery index, the player standing at `at`.
fn world_around_diagonal_door(id: u32, x: i32, z: i32, at: (i32, i32)) -> Option<osrs_core::sim::World> {
    use osrs_cache::collision::CollisionGrid;
    use osrs_cache::terrain_mesh::{PlacedObjects, TerrainGrid};
    use osrs_core::collision::{CollisionMap, CollisionMapData};
    use osrs_core::sim::{Loc, PlayerConfig, World};
    use osrs_core::tiles::TileCoord;

    let cache = Cache::open_default().ok()?;
    let cfg = Configs::new(&cache).ok()?;
    let (sx, sz) = (x.div_euclid(64), z.div_euclid(64));
    let grid = TerrainGrid::load(&cache, sx - 1, sz - 1, 3, 3, cfg.underlays().ok()?, cfg.overlays().ok()?).ok()?;
    let placed = PlacedObjects::load(&cache, &grid).ok()?;
    let collision = CollisionGrid::build(&grid, &placed, &cfg.objects().ok()?);
    let (ox, oz) = (collision.origin_x, collision.origin_z);
    let door = placed
        .placements
        .iter()
        .find(|(gx, gz, p)| p.plane == 0 && p.id == id && (gx + ox, gz + oz) == (x, z))
        .map(|(_, _, p)| Loc { id, tile: TileCoord::new(x, z), kind: p.kind, orientation: p.orientation, size: (1, 1) })
        .expect("the door is on the map");
    assert_eq!(door.kind, 9, "a door on a diagonal wall");
    let cells = (collision.width * collision.depth) as usize;
    let map = CollisionMap::new(CollisionMapData {
        origin_x: ox,
        origin_z: oz,
        width: collision.width,
        height: collision.depth,
        flags: collision.flags.clone(),
        heights: vec![0; cells],
        max_step_height: 32,
    })
    .unwrap();
    let mut world = World::new(map, 1, TileCoord::new(at.0, at.1), PlayerConfig::default());
    world.set_registry(std::rc::Rc::new(Registry::builtin()));
    world.add_scenery(door);
    Some(world)
}

/// Opens the diagonal door `id` at (`x`, `z`) from the tile east of it and walks straight
/// through to the tile west of it, which the shut door does not let the player do.
fn a_diagonal_door_on_the_real_map_opens_and_lets_you_through(id: u32, x: i32, z: i32) {
    use osrs_core::tiles::TileCoord;
    let (east, west) = (TileCoord::new(x + 1, z), TileCoord::new(x - 1, z));
    let walk_through = |world: &mut osrs_core::sim::World| {
        world.player_request_walk_to(west, false);
        for _ in 0..3 {
            world.run_tick();
        }
        world.player.tile() == west
    };
    let Some(mut world) = world_around_diagonal_door(id, x, z, (east.x, east.z)) else { return };
    assert!(!walk_through(&mut world), "{id}: shut, the doorway is not the way through");

    let Some(mut world) = world_around_diagonal_door(id, x, z, (east.x, east.z)) else { return };
    world.player_interact_object(id, TileCoord::new(x, z), "Open").unwrap();
    for _ in 0..5 {
        world.run_tick();
    }
    assert_eq!(world.scenery_changes().count(), 1, "{id}: it opened");
    assert!(walk_through(&mut world), "{id}: open, the doorway is the way through");
}

#[test]
fn faladors_diagonal_door_opens() {
    a_diagonal_door_on_the_real_map_opens_and_lets_you_through(24050, 3046, 3348);
}

#[test]
fn east_ardougnes_diagonal_door_opens() {
    a_diagonal_door_on_the_real_map_opens_and_lets_you_through(1535, 2650, 3297);
}
