//! Walking from Lumbridge to Draynor Village with the map loaded square by square (skipped when
//! the game cache is absent).

use osrs_cache::squares::{MapSource, SQUARE};
use osrs_core::collision::{CollisionFlag, CollisionMap, CollisionMapData};
use osrs_core::sim::{ClientCommand, PlayerConfig, Server, TickDriver, World};
use osrs_core::tiles::TileCoord;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

const LUMBRIDGE: (i32, i32) = (3222, 3218);
/// The road into Draynor, by the bank.
const DRAYNOR: (i32, i32) = (3100, 3245);

fn square_of(x: i32, z: i32) -> (i32, i32) {
    (x.div_euclid(SQUARE), z.div_euclid(SQUARE))
}

fn block(source: &MapSource, square: (i32, i32)) -> CollisionMapData {
    let cells = (SQUARE * SQUARE) as usize;
    CollisionMapData {
        origin_x: square.0 * SQUARE,
        origin_z: square.1 * SQUARE,
        width: SQUARE,
        height: SQUARE,
        flags: source.collision(square.0, square.1).unwrap(),
        heights: vec![0; cells],
        max_step_height: 32,
    }
}

/// A server that starts with only its own square and streams in what the player walks into, the
/// way the client's server does. Also returns the squares it has loaded so far.
fn streaming_server(source: MapSource) -> (Server, Rc<RefCell<HashSet<(i32, i32)>>>) {
    let start = square_of(LUMBRIDGE.0, LUMBRIDGE.1);
    let map = CollisionMap::new(block(&source, start)).unwrap();
    let world = World::new(map, 1, TileCoord::new(LUMBRIDGE.0, LUMBRIDGE.1), PlayerConfig::default());
    let mut server = Server::new(world, TickDriver::default());
    let loaded = Rc::new(RefCell::new(HashSet::from([start])));
    let seen = Rc::clone(&loaded);
    server.set_map_streamer(move |map, tile| {
        let (cx, cy) = square_of(tile.x, tile.z);
        for dz in -1..=1 {
            for dx in -1..=1 {
                let square = (cx + dx, cy + dz);
                if seen.borrow_mut().insert(square) {
                    map.merge(block(&source, square)).unwrap();
                }
            }
        }
    });
    (server, loaded)
}

#[test]
fn the_player_can_walk_from_lumbridge_to_draynor_across_streamed_squares() {
    let Ok(source) = MapSource::open() else { return };
    let (mut server, loaded) = streaming_server(source);
    server.advance(0.0);

    let mut tile = TileCoord::new(LUMBRIDGE.0, LUMBRIDGE.1);
    let mut crossed_squares = HashSet::from([square_of(tile.x, tile.z)]);
    let mut ticks = 0;
    let mut stuck_for = 0;
    while (tile.x - DRAYNOR.0).abs() > 3 || (tile.z - DRAYNOR.1).abs() > 3 {
        // click as a player would: a point part of the way there, within the loaded squares
        let (dx, dz) = (DRAYNOR.0 - tile.x, DRAYNOR.1 - tile.z);
        let reach = 40;
        let scale = (reach as f32 / dx.abs().max(dz.abs()).max(1) as f32).min(1.0);
        let click = TileCoord::new(tile.x + (dx as f32 * scale) as i32, tile.z + (dz as f32 * scale) as i32);
        server.handle_command(ClientCommand::WalkTo { tile: click, force_inverse: false });

        let before = tile;
        for _ in 0..20 {
            if let Some(snapshot) = server.advance(0.6) {
                tile = snapshot.player.tile;
                ticks += 1;
                crossed_squares.insert(square_of(tile.x, tile.z));
            }
        }
        stuck_for = if tile == before { stuck_for + 1 } else { 0 };
        assert!(stuck_for < 3, "stuck at {tile:?} on the way to {DRAYNOR:?} after {ticks} ticks");
        assert!(ticks < 2000, "still at {tile:?} after {ticks} ticks");
    }

    // the walk went through more than one square, and the server had loaded each as it was needed
    assert!(crossed_squares.len() >= 2, "{crossed_squares:?}");
    assert!(crossed_squares.iter().all(|s| loaded.borrow().contains(s)));
    // and the far side of the map was never loaded up front
    assert!(!loaded.borrow().contains(&(45, 50)));
    let map = &server.world().map;
    assert_eq!(map.flags_at(tile) & CollisionFlag::BLOCK_FULL, 0, "ended on a solid tile");
}

#[test]
fn ground_beyond_the_streamed_squares_is_not_walkable_until_the_player_is_near() {
    let Ok(source) = MapSource::open() else { return };
    let (mut server, _) = streaming_server(source);
    server.advance(0.0);

    // Draynor is two squares west of Lumbridge's square: nothing there is loaded yet
    let far = TileCoord::new(DRAYNOR.0, DRAYNOR.1);
    assert_ne!(server.world().map.flags_at(far) & CollisionFlag::BLOCK_FULL, 0);
}
