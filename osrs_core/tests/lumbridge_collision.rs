//! The server walking on the real Lumbridge map (skipped when the game cache is absent).

use osrs_cache::collision::{flag, CollisionGrid};
use osrs_cache::config::Configs;
use osrs_cache::terrain_mesh::{PlacedObjects, TerrainGrid};
use osrs_cache::Cache;
use osrs_core::collision::{CollisionFlag, CollisionMap, CollisionMapData};
use osrs_core::sim::{ClientCommand, PlayerConfig, Server, TickDriver, World};
use osrs_core::tiles::TileCoord;

const SPAWN: (i32, i32) = (3222, 3218);

fn lumbridge() -> Option<(CollisionGrid, Vec<(i32, i32)>)> {
    let cache = Cache::open_default().ok()?;
    let cfg = Configs::new(&cache).ok()?;
    let grid = TerrainGrid::load(&cache, 49, 49, 3, 3, cfg.underlays().ok()?, cfg.overlays().ok()?).ok()?;
    let placed = PlacedObjects::load(&cache, &grid).ok()?;
    let defs = cfg.objects().ok()?;
    let collision = CollisionGrid::build(&grid, &placed, &defs);
    let fountains = placed
        .placements
        .iter()
        .filter(|(_, _, p)| p.plane == 0 && p.kind == 10 && defs.get(&p.id).is_some_and(|d| d.name == "Fountain"))
        .map(|(x, z, _)| (x + collision.origin_x, z + collision.origin_z))
        .collect();
    Some((collision, fountains))
}

fn server_on(collision: &CollisionGrid) -> Server {
    let cells = (collision.width * collision.depth) as usize;
    let map = CollisionMap::new(CollisionMapData {
        origin_x: collision.origin_x,
        origin_z: collision.origin_z,
        width: collision.width,
        height: collision.depth,
        flags: collision.flags.clone(),
        heights: vec![0; cells],
        max_step_height: 32,
    })
    .unwrap();
    let world = World::new(map, 1, TileCoord::new(SPAWN.0, SPAWN.1), PlayerConfig::default());
    Server::new(world, TickDriver::default())
}

#[test]
fn the_player_never_stands_on_a_blocked_tile_and_never_crosses_walls() {
    let Some((collision, fountains)) = lumbridge() else { return };
    let map_flags = |x: i32, z: i32| collision.at(x, z);

    // a ring of targets around the spawn: some are open ground, some are scenery, some behind walls
    let mut targets: Vec<(i32, i32)> = fountains.clone();
    for r in [4, 8, 12] {
        for (dx, dz) in [(r, 0), (-r, 0), (0, r), (0, -r), (r, r), (-r, r), (r, -r), (-r, -r)] {
            targets.push((SPAWN.0 + dx, SPAWN.1 + dz));
        }
    }

    let mut crossed_checks = 0;
    for (tx, tz, running) in targets.iter().flat_map(|(x, z)| [(*x, *z, false), (*x, *z, true)]) {
        let mut server = server_on(&collision);
        server.advance(0.0);
        server.handle_command(ClientCommand::SetRunning(running));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(tx, tz), force_inverse: false });
        let mut prev = TileCoord::new(SPAWN.0, SPAWN.1);
        for _ in 0..60 {
            let Some(snapshot) = server.advance(0.6) else { continue };
            let t = snapshot.player.tile;
            assert_eq!(
                map_flags(t.x, t.z) & CollisionFlag::BLOCK_FULL,
                0,
                "player stepped onto blocked tile ({}, {}) heading for ({tx}, {tz})",
                t.x,
                t.z
            );
            // every tick moves at most two tiles (a run), each step legal on the collision map
            let (dx, dz) = (t.x - prev.x, t.z - prev.z);
            assert!(dx.abs() <= 2 && dz.abs() <= 2);
            // a single orthogonal step must not cross a wall edge
            if dx.abs() + dz.abs() == 1 {
                let (exit, enter) = match (dx, dz) {
                    (1, 0) => (flag::EAST, flag::WEST),
                    (-1, 0) => (flag::WEST, flag::EAST),
                    (0, 1) => (flag::NORTH, flag::SOUTH),
                    _ => (flag::SOUTH, flag::NORTH),
                };
                assert_eq!(map_flags(prev.x, prev.z) & exit, 0, "walked through a wall leaving ({}, {})", prev.x, prev.z);
                assert_eq!(map_flags(t.x, t.z) & enter, 0, "walked through a wall entering ({}, {})", t.x, t.z);
                crossed_checks += 1;
            }
            prev = t;
        }
    }
    assert!(crossed_checks > 100, "too few single steps were checked: {crossed_checks}");
}

#[test]
fn clicking_a_solid_object_does_not_put_the_player_on_it() {
    let Some((collision, fountains)) = lumbridge() else { return };
    let (fx, fz) = fountains[0];
    let mut server = server_on(&collision);
    server.advance(0.0);
    server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(fx, fz), force_inverse: false });
    let mut last = TileCoord::new(SPAWN.0, SPAWN.1);
    for _ in 0..40 {
        if let Some(s) = server.advance(0.6) {
            last = s.player.tile;
        }
    }
    assert_ne!((last.x, last.z), (fx, fz), "ended on the fountain tile");
}
