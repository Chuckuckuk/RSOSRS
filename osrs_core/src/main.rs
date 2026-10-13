//! The game client: renders the game world and the player's character from the local game cache.
//! It never owns a `World` or a `TickDriver`: it logs in to the authoritative server, sends it
//! `ClientCommand`s and renders what it publishes. Models are drawn with the same pipeline
//! as `osrs_viewer` (game cache -> lit meshes in `osrs_cache`, Bevy materials in `osrs_render`).
//!
//! args:  --scenario <name | file.json>  start in a fight instead of Lumbridge: a bundled scenario
//!                                       (`jad`, ...) or a scenario file, played on the real map at
//!                                       its `location`. The fight waits until it is on screen.
//! env:   OSRS_SCREENSHOT=<path.png>   save a screenshot after a few seconds and exit
//!        OSRS_SCREENSHOT_DELAY=<seconds> (default 8) when to capture
//!        OSRS_ORBIT=yaw,pitch,distance   starting camera (radians, tiles)
//!        OSRS_WALK_TO=x,z                ask the server to walk the player to a world tile
//!        OSRS_WALK_RUN=0                 with OSRS_WALK_TO, walk instead of run
//!        OSRS_NAME=<name>                name to log in with (default "Player")
//!        OSRS_SPAWN=x,z                  world tile to enter the world on (default: Lumbridge square)
//!        OSRS_ATTACK=<entity id>         ask the server to attack that NPC once in the world
//!        OSRS_WIELD=0,1                  wield the items in these inventory slots once in the world
//!        OSRS_TAB=n                      open this interface tab first (0..13)
//!        OSRS_LAYOUT=classic|modern|fixed  interface layout (default classic: resizable)
//!        OSRS_UI_SCALE=n                 whole-number interface scale (default: largest that fits 765x503)
//!        OSRS_WINDOW=WxH                 window size in pixels (scale factor forced to 1), e.g. 900x600 to test
//!                                        the modern layout's narrow stone arrangement (< 948 px wide)
//!        OSRS_MAX_PLANE=n                hide terrain and scenery above plane n (to look under roofs)
//! controls: F10 cycles the interface layout (fixed, classic, modern); G toggles the tile grid; left-click ground to walk there; right-click ground for the "Walk here" menu;
//!           hold right mouse and drag to orbit the character, scroll to zoom
//!
//! The map is streamed: the squares around the player are decoded on a loading thread as he
//! walks and dropped when he has left them behind (`map_stream`), and the server loads its own
//! collision for the same squares.

mod chat;
mod combat_fx;
mod ground_click;
mod interface;
mod map_stream;
mod npcs;
mod panes;
mod overheads;
mod player_look;
mod projectiles;
mod route;
mod sound_fx;
mod tile_marks;

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use std::collections::VecDeque;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use osrs_cache::config::Configs;
use osrs_cache::kit::{build_player, Appearance};
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::scene::LocMesh;
use osrs_cache::squares::{MapSource, SQUARE};
use osrs_cache::textures::load_textures;
use osrs_cache::Cache;
use osrs_core::collision::{CollisionMap, CollisionMapData};
use osrs_core::data::{load_scenario, Registry, ScenarioDef};
use osrs_core::sim::scenarios::{build_scenario_on, scenario_start};
use osrs_core::sim::{
    spawn_server, ClientCommand, PlayerConfig, PlayerId, Server, ServerHandle, Snapshot, TickDriver, World,
};
use osrs_core::tiles::TileCoord;
use osrs_render::{spawn_animated, AnimatedModel, ModelAnimationPlugin};
use osrs_render::pick::GroundSurface;
use osrs_render::streaming::{square_of, SquareData, SquareLoader, SquareStream, StreamedWorld};
use osrs_render::world::{MaterialCache, TextureStore, TileGrid, WorldRenderPlugin};
use std::collections::{HashMap, HashSet};

/// The client's link to the authoritative server.
struct ServerLink {
    link: ServerHandle,
    latest: Option<Snapshot>,
    /// How many server ticks `latest` stands for: more than 1 when the client was too slow to
    /// look at some of them one by one (see `merge_snapshots`).
    ticks_in_latest: i32,
    /// Events from snapshots that have come in and not yet been drawn.
    new_events: Vec<osrs_core::sim::WorldEvent>,
}

/// A player in the world. The server owns the truth (its id and tile); this is the client's view
/// of one, carrying the entity that draws it. `id` stays `None` until the server has accepted the
/// login and reported the id it assigned.
#[derive(Component)]
struct Player {
    id: Option<PlayerId>,
    name: String,
    /// Where the server last said this player stands.
    tile: TileCoord,
    /// Last server tick folded into the fields below.
    seen_tick: Option<i32>,
    /// Tiles the server moved this player during the most recent tick that moved it (1 walking,
    /// 2 running). The gait and the pace of the visible movement both follow this, not the
    /// player's run toggle: a run with one step left is a walk.
    steps: i32,
    /// Tiles per second he is currently running along `route`.
    speed: f32,
    /// Tile centres (and the heading of the step into each) still to walk through, in order. The character visits each one, so a path
    /// that turns a corner is followed around it instead of cut across.
    route: VecDeque<(Vec3, f32)>,
    /// Seconds the character has been standing at the server tile.
    still_for: f32,
    /// Heading in radians about the up axis (0 faces south, the model front); eased toward the
    /// direction of travel at the game's turn rate rather than snapped.
    facing: f32,
    /// The heading `facing` is turning toward: the direction of the last step he took. It stays
    /// set after he stops, so a turn that was still in progress finishes while he stands.
    heading: f32,
    /// The turn controller easing `facing` toward `heading`.
    turn: Turn,
}

/// Orbit camera looking at a point on the character.
#[derive(Component)]
struct OrbitCamera {
    target: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

#[derive(Resource, Default)]
struct ScreenshotState {
    path: Option<String>,
    shot: bool,
}

/// World tile the character spawns on by default: the middle of Lumbridge square.
const DEFAULT_SPAWN_TILE: (i32, i32) = (3222, 3218);

/// The tile to spawn on: `OSRS_SPAWN=x,z`, else Lumbridge.
fn spawn_tile() -> (i32, i32) {
    let parsed: Option<Vec<i32>> = std::env::var("OSRS_SPAWN")
        .ok()
        .map(|v| v.split(',').filter_map(|s| s.trim().parse().ok()).collect());
    match parsed.as_deref() {
        Some([x, z]) => (*x, *z),
        _ => DEFAULT_SPAWN_TILE,
    }
}

/// What the client was started into: a scenario (`--scenario`), or the open world.
#[derive(Resource, Clone)]
struct Launch {
    scenario: Option<ScenarioDef>,
    /// The world tile the player enters on.
    spawn: (i32, i32),
}

/// Reads the command line. A bad argument or an unknown scenario ends the program with a message.
fn launch(bundled: bool) -> Launch {
    let fail = |message: String| -> ! {
        eprintln!("{message}");
        std::process::exit(2);
    };
    let mut scenario = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--scenario" => {
                let name = args.next().unwrap_or_else(|| fail("--scenario needs a scenario name or file".into()));
                scenario = Some(load_scenario(&name).unwrap_or_else(|e| fail(e)));
            }
            other => fail(format!("unknown argument '{other}' (usage: osrs_core [--scenario <name | file.json>])")),
        }
    }
    // a program with its own cache is the Jad fight, and starts in it
    if scenario.is_none() && bundled {
        scenario = Some(load_scenario("jad").unwrap_or_else(|e| fail(e)));
    }
    let spawn = match &scenario {
        Some(scenario) => {
            let start = scenario_start(scenario);
            (start.x, start.z)
        }
        None => spawn_tile(),
    };
    Launch { scenario, spawn }
}

impl Launch {
    /// How many map squares around the player are loaded and drawn: just the one the fight is in
    /// when it sits on a real place (the Fight Cave), else the open world's ring.
    fn stream_radius(&self) -> i32 {
        match &self.scenario {
            Some(scenario) if scenario.location.is_some() => 0,
            _ => map_stream::RADIUS,
        }
    }
}

/// Decoded texture images, shared by every square that is spawned.
#[derive(Resource)]
struct Textures(TextureStore);

/// The map squares around the spawn, decoded before the app starts and spawned by `setup`.
#[derive(Resource)]
struct FirstSquares(Vec<SquareData>);

/// Left-click options of the objects that have one, and what everything is called, for the whole game.
#[derive(Resource)]
struct ObjectInfo {
    left_click: HashMap<u32, String>,
    names: HashMap<u32, String>,
}

/// The tile grid overlay (G): whether it is showing, and the material its lines share.
#[derive(Resource)]
struct GridOverlay {
    shown: bool,
    material: Option<Handle<StandardMaterial>>,
}

/// Everything decoded from the cache before the app starts, spawned by `setup`.
#[derive(Resource)]
struct Scene {
    player: LocMesh,
    /// The player's animations baked against the character's vertices, in `PLAYER_CLIPS` order.
    player_clips: Vec<Clip>,
    /// The cache's hit splat pictures: a hit that did damage, and a block.
    damage_splat: osrs_cache::hitsplat::HitSplatArt,
    block_splat: osrs_cache::hitsplat::HitSplatArt,
    /// The font hit splat numbers are drawn in.
    splat_font: osrs_cache::font::Font,
    /// The standard health bar.
    health_bar: osrs_cache::healthbar::HealthBarArt,
}

/// Sequences the unarmed player plays, in the order of `Scene::player_clips`. Walking uses a
/// different one when the body is not facing the way it is going (side-steps and backing up), and
/// standing uses a turn-on-the-spot while the facing catches up.
const PLAYER_CLIPS: [(&str, u32); 10] = [
    ("idle", 808),
    ("walk", 819),
    ("run", 824),
    ("walk back", 820),
    ("walk left", 821),
    ("walk right", 822),
    ("turn on the spot", 823),
    ("punch", 422),
    ("block", 424),
    ("death", 836),
];
const CLIP_IDLE: usize = 0;
const CLIP_WALK: usize = 1;
const CLIP_RUN: usize = 2;
const CLIP_BACK: usize = 3;
const CLIP_LEFT: usize = 4;
const CLIP_RIGHT: usize = 5;
const CLIP_TURN: usize = 6;
pub(crate) const CLIP_ATTACK: usize = 7;
pub(crate) const CLIP_BLOCK: usize = 8;
pub(crate) const CLIP_DEATH: usize = 9;

fn load_scene(spawn: (i32, i32), radius: i32) -> osrs_cache::Result<(Scene, FirstSquares, Textures, ObjectInfo)> {
    // the squares around the spawn are decoded up front so the first frame has a world in it
    let mut loader = SquareLoader::open()?;
    let around = square_of(spawn.0, spawn.1);
    let mut first = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            first.push(loader.load(around.0 + dx, around.1 + dz)?);
        }
    }
    let (left_click, names) = loader.object_info();
    drop(loader);

    let cache = Cache::open_default()?;
    let textures = TextureStore(load_textures(&cache)?);
    let texture_hsl = Configs::new(&cache)?.texture_average_hsl()?;
    let player = build_player(&cache, &Appearance::default_male(), &[], &texture_hsl)?
        .ok_or_else(|| osrs_cache::Error::Missing("default character has no models".into()))?;
    let rig = player.rig.as_ref().expect("player meshes keep their rig");
    let mut animations = Animations::new(&cache)?;
    let mut player_clips = Vec::new();
    for (name, sequence) in PLAYER_CLIPS {
        let clip = Clip::bake(&mut animations, sequence, rig)?;
        eprintln!(
            "{name} animation {sequence}: {} frames, {:.2} s",
            clip.frames.len(),
            clip.duration() as f32 * 0.02
        );
        player_clips.push(clip);
    }
    let configs = Configs::new(&cache)?;
    let splat = |kind| {
        osrs_cache::hitsplat::load_hitsplat(&cache, &configs, kind)?
            .ok_or_else(|| osrs_cache::Error::Missing(format!("hit splat type {kind}")))
    };
    let damage_splat = splat(osrs_cache::hitsplat::kind::DAMAGE_ME)?;
    let block_splat = splat(osrs_cache::hitsplat::kind::BLOCK_ME)?;
    let splat_font = osrs_cache::font::Font::load(&cache, "p11_full")?
        .ok_or_else(|| osrs_cache::Error::Missing("font p11_full".into()))?;
    let health_bar = osrs_cache::healthbar::load_healthbar(&cache, &configs, 0)?
        .ok_or_else(|| osrs_cache::Error::Missing("health bar 0".into()))?;
    Ok((
        Scene { player, player_clips, damage_splat, block_splat, splat_font, health_bar },
        FirstSquares(first),
        Textures(textures),
        ObjectInfo { left_click, names },
    ))
}

/// The primary window; `OSRS_WINDOW=WxH` fixes its size in physical pixels.
fn window_from_env(title: String) -> Window {
    let mut window = Window { title, ..default() };
    let size = std::env::var("OSRS_WINDOW").ok().and_then(|v| {
        let (w, h) = v.split_once('x')?;
        Some((w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?))
    });
    if let Some((w, h)) = size {
        window.resolution = bevy::window::WindowResolution::new(w, h).with_scale_factor_override(1.0);
    }
    window
}

fn main() {
    // an executable carrying its own cache (see `osrs_cache::bundle`) runs without the game
    // installed, and with nothing asked for it goes straight into the Jad fight
    let bundled = osrs_cache::bundle::install_from_exe();
    if !bundled && std::env::var("OSRS_RECORD_BUNDLE").is_ok() {
        osrs_cache::bundle::start_recording();
    }
    let launch = launch(bundled);
    let radius = launch.stream_radius();
    // the tile grid is for the open world; a fight shows only the floor (G still toggles it, and
    // OSRS_GRID=1 starts with it on)
    let grid_shown = launch.scenario.is_none() || std::env::var("OSRS_GRID").is_ok_and(|v| v == "1");
    let (scene, first, textures, info) = match load_scene(launch.spawn, radius) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("failed to load the game cache: {e}");
            std::process::exit(1);
        }
    };

    // The mock server runs on its own thread for the life of the client; the client talks to it
    // only through `ServerLink`. The world is built on the server thread (it holds `Rc`s, so it
    // can't cross threads). Its collision comes from the game cache, so the server's pathfinder
    // walks around walls, water and scenery, and is loaded square by square around the player as
    // he walks (the same squares the client draws).
    let spawn = launch.spawn;
    let scenario = launch.scenario.clone();
    let link = spawn_server(move || {
        let spawn = TileCoord::new(spawn.0, spawn.1);
        let source = MapSource::open().expect("the game cache opens on the server thread too");
        let (sx, sy) = square_of(spawn.x, spawn.z);
        // the squares around the spawn are loaded up front, so whatever starts near the player
        // stands on ground the server knows; the rest stream in as he walks
        let mut map = CollisionMap::new(square_collision(&source, (sx, sy))).expect("a square's arrays match its size");
        let mut loaded: HashSet<(i32, i32)> = HashSet::from([(sx, sy)]);
        for dz in -radius..=radius {
            for dx in -radius..=radius {
                let square = (sx + dx, sy + dz);
                if loaded.insert(square) {
                    if let Err(e) = map.merge(square_collision(&source, square)) {
                        eprintln!("[server] map square ({}, {}) rejected: {e}", square.0, square.1);
                    }
                }
            }
        }
        let registry = std::rc::Rc::new(Registry::builtin());

        let mut server = match &scenario {
            Some(scenario) => {
                let world = build_scenario_on(registry, scenario, map).unwrap_or_else(|e| {
                    eprintln!("[server] the scenario \"{}\" cannot be set up: {e}", scenario.name);
                    std::process::exit(1);
                });
                println!("[server] scenario \"{}\" is set up and waits for the client", scenario.name);
                // held still until the client has the fight on screen (`start_scenario`)
                let mut driver = TickDriver::default();
                driver.set_paused(true);
                Server::new(world, driver)
            }
            None => {
                let mut world = World::new(map, 12345, spawn, PlayerConfig::new_account());
                world.set_registry(registry);
                // a man (level 2) stands about in Lumbridge square, a few tiles from where you arrive
                let man_at = nearest_open_tile(&world.map, TileCoord::new(spawn.x + 4, spawn.z - 3));
                match man_at.map(|at| world.spawn_npc_def("man", at)) {
                    Some(Ok(id)) => println!("[server] a man (entity {}) stands at ({}, {})", id.0, man_at.unwrap().x, man_at.unwrap().z),
                    Some(Err(e)) => eprintln!("[server] could not place the man: {e}"),
                    None => eprintln!("[server] no open tile near the spawn for the man"),
                }
                // a starter kit for the mock server, so the inventory has something in it
                for (item, quantity) in [
                    ("bronze_sword", 1),
                    ("bronze_full_helm", 1),
                    ("abyssal_whip", 1),
                    ("magic_shortbow", 1),
                    ("shark", 3),
                    ("cooked_karambwan", 2),
                    ("prayer_potion_4", 2),
                    ("bronze_arrow", 150),
                ] {
                    if let Err(e) = world.give_item(item, quantity) {
                        eprintln!("[server] starter kit: {e}");
                    }
                }
                Server::new(world, TickDriver::default())
            }
        };

        // what the scenery is called, for the server's examine texts
        server.set_object_names(
            source.object_defs().values().filter(|d| !d.name.is_empty()).map(|d| (d.id, d.name.clone())).collect(),
        );

        server.set_map_streamer(move |map, tile| {
            let (cx, cy) = square_of(tile.x, tile.z);
            for dz in -radius..=radius {
                for dx in -radius..=radius {
                    let square = (cx + dx, cy + dz);
                    if loaded.insert(square) {
                        let started = std::time::Instant::now();
                        match map.merge(square_collision(&source, square)) {
                            Ok(()) => println!(
                                "[server] map square ({}, {}) loaded in {:.2?}",
                                square.0,
                                square.1,
                                started.elapsed()
                            ),
                            Err(e) => eprintln!("[server] map square ({}, {}) rejected: {e}", square.0, square.1),
                        }
                    }
                }
            }
        });
        server
    });

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(window_from_env(window_title(&launch))),
        ..default()
    }))
    .add_plugins((WorldRenderPlugin, ModelAnimationPlugin, ground_click::GroundClickPlugin, chat::ChatPlugin, combat_fx::CombatFxPlugin, interface::InterfacePlugin, overheads::OverheadPlugin, player_look::PlayerLookPlugin, projectiles::ProjectilePlugin, sound_fx::SoundPlugin, tile_marks::TileMarksPlugin))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
    .insert_resource(ScreenshotState { path: std::env::var("OSRS_SCREENSHOT").ok(), shot: false })
    .insert_resource(scene)
    .insert_resource(launch)
    .insert_resource(first)
    .insert_resource(textures)
    .insert_resource(ground_click::GroundPicker::new(&info))
    .insert_resource(info)
    .insert_resource(GridOverlay { shown: grid_shown, material: None })
    .insert_resource(map_stream::StreamRadius(radius))
    .init_resource::<StreamedWorld>()
    .init_resource::<MaterialCache>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (scripted_input, poll_server, start_scenario, sync_player, npcs::sync_npcs, map_stream::stream_map, limit_planes, orbit_camera, toggle_grid, screenshot_and_exit)
            .chain(),
    );
    app.world_mut().insert_non_send(ServerLink { link, latest: None, ticks_in_latest: 1, new_events: Vec::new() });
    app.world_mut().insert_non_send(SquareStream::spawn());
    app.world_mut().insert_non_send(npcs::NpcViews::spawn());
    app.run();
}

/// Ends a recording run (`OSRS_RECORD_BUNDLE=<file>`, see `osrs_cache::bundle`): everything the
/// game read, plus what it would read later (every item picture, every sound the data names),
/// written as a payload for `bundle_pack`.
fn finish_recording() {
    let Ok(path) = std::env::var("OSRS_RECORD_BUNDLE") else { return };
    if !osrs_cache::bundle::is_recording() {
        return;
    }
    let registry = Registry::builtin();
    // items that turn up later (a potion's lower doses, the vial): their pictures
    let names: Vec<String> =
        registry.items().map(|i| i.name.clone()).chain(registry.consumables().map(|c| c.name.clone())).collect();
    interface::touch_icons(&names);
    // every sound the data can ask for
    let cache = Cache::open_default().expect("the cache opens");
    for sound in registry.named_sounds() {
        osrs_cache::bundle::record(osrs_cache::sound::ARCHIVE, sound);
    }
    let _ = cache.index(osrs_cache::sound::ARCHIVE);
    let keys = osrs_cache::bundle::recorded();
    let (payload, skipped) = osrs_cache::bundle::write_payload(&cache, &keys);
    std::fs::write(&path, &payload).expect("the bundle file is writable");
    println!("[bundle] {} groups ({skipped} unreadable), {} MB, written to {path}", keys.len(), payload.len() / 1_000_000);
}

fn window_title(launch: &Launch) -> String {
    match &launch.scenario {
        Some(scenario) if !scenario.name.is_empty() => format!("RSOSRS - {}", scenario.name),
        _ => "RSOSRS".into(),
    }
}

/// Seconds a scenario waits, once everything in it is on screen, before it starts: the first
/// frames after a model appears can still be blank while its shaders compile.
const SCENARIO_GRACE: f32 = 3.0;
/// A scenario starts after this long even if something in it could not be drawn.
const SCENARIO_LONGEST_WAIT: f32 = 30.0;

/// A scenario's server starts paused (see `main`). This lets it go once the client has the fight
/// on screen: every monster the server reports is drawn and `SCENARIO_GRACE` has passed.
fn start_scenario(
    launch: Res<Launch>,
    time: Res<Time>,
    server: NonSend<ServerLink>,
    views: NonSend<npcs::NpcViews>,
    mut drawn_since: Local<Option<f32>>,
    mut started: Local<bool>,
) {
    if *started || launch.scenario.is_none() {
        return;
    }
    let Some(snapshot) = &server.latest else { return };
    let now = time.elapsed_secs();
    let all_drawn = snapshot.entities.iter().filter(|e| e.appearance.is_some()).all(|e| views.entity_of(e.id).is_some());
    if all_drawn {
        drawn_since.get_or_insert(now);
    }
    let ready = drawn_since.is_some_and(|since| now - since >= SCENARIO_GRACE);
    if ready || now >= SCENARIO_LONGEST_WAIT {
        server.link.send(ClientCommand::SetPaused(false));
        *started = true;
        println!("[client] the scenario starts");
    }
}

/// The open tile nearest `wanted` (searching outwards a few tiles), if there is one.
fn nearest_open_tile(map: &CollisionMap, wanted: TileCoord) -> Option<TileCoord> {
    use osrs_core::collision::CollisionFlag;
    (0..=6)
        .flat_map(|r: i32| (-r..=r).flat_map(move |dx| (-r..=r).map(move |dz| (r, dx, dz))))
        .filter(|(r, dx, dz)| dx.abs().max(dz.abs()) == *r)
        .map(|(_, dx, dz)| TileCoord::new(wanted.x + dx, wanted.z + dz))
        .find(|t| map.flags_at(*t) & CollisionFlag::BLOCK_FULL == 0)
}

/// The collision of one map square, as the arrays a `CollisionMap` takes. A square that cannot be
/// decoded is solid, so nobody is ever walked onto ground nobody has seen.
fn square_collision(source: &MapSource, square: (i32, i32)) -> CollisionMapData {
    let cells = (SQUARE * SQUARE) as usize;
    let flags = source.collision(square.0, square.1).unwrap_or_else(|e| {
        eprintln!("[server] map square ({}, {}) could not be read: {e}", square.0, square.1);
        vec![osrs_cache::collision::flag::FLOOR; cells]
    });
    CollisionMapData {
        origin_x: square.0 * SQUARE,
        origin_z: square.1 * SQUARE,
        width: SQUARE,
        height: SQUARE,
        flags,
        // heights are not used for movement here: no step limit on real terrain
        heights: vec![0; cells],
        max_step_height: 32,
    }
}

fn setup(
    textures: Res<Textures>,
    scene: Res<Scene>,
    first: Res<FirstSquares>,
    mut spawner: map_stream::SquareSpawner,
    server: NonSend<ServerLink>,
    launch: Res<Launch>,
) {
    let spawn = launch.spawn;
    // Enter the world. The id comes back in a snapshot, once the server has assigned it.
    let name = std::env::var("OSRS_NAME").unwrap_or_else(|_| "Player".into());
    server.link.send(ClientCommand::Login { name: name.clone() });
    // OSRS_WALK_TO=x,z asks the server to walk the player to a world tile (no click-to-walk yet)
    if let Ok(v) = std::env::var("OSRS_WALK_TO") {
        let p: Vec<i32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if let [x, z] = p[..] {
            // OSRS_WALK_RUN=0 walks instead of running (the server defaults to running)
            if std::env::var("OSRS_WALK_RUN").is_ok_and(|v| v == "0") {
                server.link.send(ClientCommand::SetRunning(false));
            }
            server.link.send(ClientCommand::WalkTo { tile: TileCoord::new(x, z), force_inverse: false });
        }
    }

    for square in &first.0 {
        spawner.spawn(square);
    }

    // OSRS_WIELD=0,1 wields the items in those inventory slots straight away (for looking at the
    // equipment tab without clicking)
    if let Ok(slots) = std::env::var("OSRS_WIELD") {
        for slot in slots.split(',').filter_map(|s| s.trim().parse::<usize>().ok()) {
            server.link.send(ClientCommand::InventoryOption { slot, option: "Wield".to_owned() });
        }
    }

    // OSRS_ATTACK=<entity id> has the player attack that NPC straight away (for trying combat
    // without a click)
    if let Some(target) = std::env::var("OSRS_ATTACK").ok().and_then(|v| v.parse::<u32>().ok()) {
        server.link.send(ClientCommand::Attack { target: osrs_core::actors::EntityId(target) });
    }

    let feet = spawner
        .world
        .ground_at(spawn.0, spawn.1)
        .expect("the spawn square is part of the first squares");
    let character = spawn_animated(
        &mut spawner.commands,
        &mut spawner.meshes,
        &mut spawner.materials,
        &mut spawner.images,
        &textures.0 .0,
        &scene.player,
        scene.player_clips.clone(),
        Transform::from_translation(feet),
    );
    spawner.commands.entity(character).insert(combat_fx::Fighter::default());
    spawner.commands.entity(character).insert(Player {
        id: None,
        name,
        tile: TileCoord::new(spawn.0, spawn.1),
        seen_tick: None,
        steps: 0,
        speed: 0.0,
        route: VecDeque::new(),
        still_for: 0.0,
        facing: 0.0,
        heading: 0.0,
        turn: Turn::default(),
    });

    // Camera aimed at the middle of the character (about 1.5 tiles tall), seen from the front.
    let mut orbit = OrbitCamera { target: feet + Vec3::Y * 0.75, yaw: 0.45, pitch: 0.2, distance: 4.0 };
    // OSRS_ORBIT=yaw,pitch,distance overrides the starting camera (radians, tiles)
    if let Ok(v) = std::env::var("OSRS_ORBIT") {
        let p: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if let [yaw, pitch, distance] = p[..] {
            (orbit.yaw, orbit.pitch, orbit.distance) = (yaw, pitch, distance);
        }
    }
    spawner.commands.spawn((
        Camera3d::default(),
        orbit_transform(&orbit),
        orbit,
        bevy::camera::visibility::RenderLayers::from_layers(&[0, tile_marks::GIZMO_LAYER]),
    ));
}

fn orbit_transform(o: &OrbitCamera) -> Transform {
    let rot = Quat::from_euler(EulerRot::YXZ, o.yaw, -o.pitch, 0.0);
    Transform::from_translation(o.target + rot * Vec3::new(0.0, 0.0, o.distance)).looking_at(o.target, Vec3::Y)
}

fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    players: Query<&Transform, (With<Player>, Without<OrbitCamera>)>,
    mut cams: Query<(&mut Transform, &mut OrbitCamera), Without<Player>>,
) {
    let Ok((mut tf, mut cam)) = cams.single_mut() else { return };
    // follow the character (about 1.5 tiles tall) as the server moves it
    if let Ok(player) = players.single() {
        cam.target = player.translation + Vec3::Y * 0.75;
    }
    // the middle button turns the camera; the right one is the context menu's
    if buttons.pressed(MouseButton::Middle) {
        cam.yaw -= motion.delta.x * 0.005;
        cam.pitch = (cam.pitch + motion.delta.y * 0.005).clamp(-0.4, 1.4);
    }
    if scroll.delta.y != 0.0 {
        cam.distance = (cam.distance * (1.0 - scroll.delta.y * 0.1)).clamp(1.0, 40.0);
    }
    *tf = orbit_transform(&cam);
}

/// Keeps the newest state the server has published (ticking itself happens server-side) and
/// shows the messages of every snapshot that came in, not just the newest.
fn poll_server(mut server: NonSendMut<ServerLink>, mut chat: ResMut<chat::ChatLog>, time: Res<Time>) {
    let mut snapshots = server.link.drain_snapshots();
    for snapshot in &mut snapshots {
        server.new_events.extend(std::mem::take(&mut snapshot.events));
        for message in std::mem::take(&mut snapshot.messages) {
            println!("[game] {message}");
            chat.push(message, time.elapsed_secs());
        }
    }
    if let Some((snapshot, ticks)) = merge_snapshots(snapshots) {
        server.latest = Some(snapshot);
        server.ticks_in_latest = ticks;
    }
}

/// Folds snapshots (oldest first) into the newest one, keeping the tiles every earlier one said
/// the player and each entity stepped through, in order. A frame that took longer than a server
/// tick would otherwise only see the newest tick's steps, and the characters would jump over the
/// ones in between. Returns the folded snapshot and how many ticks it covers.
fn merge_snapshots(mut snapshots: Vec<Snapshot>) -> Option<(Snapshot, i32)> {
    let mut newest = snapshots.pop()?;
    let ticks = snapshots.len() as i32 + 1;
    if !snapshots.is_empty() {
        let mut player: Vec<TileCoord> = snapshots.iter().flat_map(|s| s.player.step_tiles.iter().copied()).collect();
        player.append(&mut newest.player.step_tiles);
        newest.player.step_tiles = player;

        for entity in &mut newest.entities {
            let mut steps: Vec<TileCoord> = snapshots
                .iter()
                .filter_map(|s| s.entities.iter().find(|e| e.id == entity.id))
                .flat_map(|e| e.step_tiles.iter().copied())
                .collect();
            steps.append(&mut entity.step_tiles);
            entity.step_tiles = steps;
        }
    }
    Some((newest, ticks))
}

/// Top turn speed (rad/s), about 515 degrees a second. The old client turned 32/2048 per 20 ms
/// cycle (281 deg/s) but the live game was observed to turn a full 180 and still arrive on the
/// tile within the tick it is clicked in (0.6 s), and never as fast as a quick snap; this is sized
/// so a reversal lands in about 0.55 s, which is the observed part.
const MAX_TURN_SPEED: f32 = 9.0;
/// How quickly angular speed builds and falls (rad/s^2): about 0.09 s from rest to top speed.
const TURN_ACCELERATION: f32 = MAX_TURN_SPEED / 0.09;
/// How quickly the acceleration itself may change (rad/s^3), so a turn starts and ends gently
/// instead of jumping to full acceleration: it takes about 0.045 s to reach it.
const TURN_JERK: f32 = TURN_ACCELERATION / 0.045;

/// A turn with limited speed, acceleration and jerk: angular speed builds in a smooth S, holds at
/// `MAX_TURN_SPEED` on a long turn, and is brought down so it lands on the target at rest. It is a
/// small controller, so re-aiming mid-turn simply carries on from the current speed.
#[derive(Debug, Default, Clone)]
pub(crate) struct Turn {
    /// Angular speed now (rad/s, signed).
    velocity: f32,
    /// Angular acceleration now (rad/s^2, signed).
    acceleration: f32,
}

impl Turn {
    #[cfg(test)]
    fn is_at_rest(&self) -> bool {
        self.velocity == 0.0 && self.acceleration == 0.0
    }

    /// Advance by `dt` toward `target`; returns the new angle (continuing from `current`).
    fn step(&mut self, current: f32, target: f32, dt: f32) -> f32 {
        use std::f32::consts::{PI, TAU};
        let difference = (target - current + PI).rem_euclid(TAU) - PI;
        let distance = difference.abs();
        if distance < 1e-3 && self.velocity.abs() < 0.05 {
            *self = Turn::default();
            return target;
        }
        let direction = difference.signum();

        // The speed from which the turn can still stop in the distance left. The stopping
        // acceleration assumed is a fraction of the maximum, because the jerk limit spends time
        // getting the deceleration going and the speed must already be low by the end.
        let braking = (2.0 * 0.35 * TURN_ACCELERATION * distance).sqrt();
        // and in the last few degrees the speed also tapers with the distance left, so it lands
        // gently instead of at the end of the braking curve
        let taper = 8.0 * distance + 0.1;
        let wanted_velocity = direction * MAX_TURN_SPEED.min(braking).min(taper);
        let wanted_acceleration =
            ((wanted_velocity - self.velocity) / 0.05).clamp(-TURN_ACCELERATION, TURN_ACCELERATION);
        let change = (wanted_acceleration - self.acceleration).clamp(-TURN_JERK * dt, TURN_JERK * dt);
        self.acceleration += change;
        self.velocity = (self.velocity + self.acceleration * dt).clamp(-MAX_TURN_SPEED, MAX_TURN_SPEED);

        let step = self.velocity * dt;
        // arrived (or would pass it): land on the target at rest
        if step.abs() >= distance && step.signum() == direction {
            *self = Turn::default();
            return target;
        }
        current + step
    }
}

/// How long a character keeps its walking pose after it stops advancing, only so the animation
/// does not flicker to idle for a frame or two when a tick lands slightly late.
const WALK_LINGER: f32 = 0.08;

/// Applies what the server says about the player: its assigned id once logged in, and its
/// movement. Each tick the server lists the tiles the player stepped through; the character walks
/// through those tile centres in order (so it goes around corners and obstacles exactly as the
/// path did), at a pace that covers them within one tick. The walk or run animation is chosen from
/// how many tiles the server moved the player.
fn sync_player(
    server: NonSend<ServerLink>,
    streamed: Res<StreamedWorld>,
    time: Res<Time>,
    mut players: Query<
        (&mut Player, &mut Transform, &mut AnimatedModel, &mut combat_fx::Fighter),
        Without<npcs::NpcView>,
    >,
    npc_views: NonSend<npcs::NpcViews>,
    npc_transforms: Query<&Transform, With<npcs::NpcView>>,
    mut logged_clip: Local<Option<usize>>,
) {
    let Some(snapshot) = &server.latest else { return };
    // ground at the middle of a world tile, if its square is loaded
    let ground = |tile: TileCoord| -> Option<Vec3> { streamed.ground_at(tile.x, tile.z) };

    for (mut player, mut transform, mut model, mut fighter) in &mut players {
        let Some(id) = snapshot.player_id else { continue };
        fighter.health = Some((snapshot.player.health, snapshot.player.max_health));
        if snapshot.player.is_dead {
            fighter.dead = true;
        }
        if let Some(heading) = fighter.face.take() {
            player.heading = heading;
        }
        if player.id != Some(id) {
            println!("[client] logged in as \"{}\": the server assigned player id {id}", player.name);
            player.id = Some(id);
        }

        // A new tick: lay out his run to the end of the server's planned path.
        if player.seen_tick != Some(snapshot.tick) {
            let steps = &snapshot.player.step_tiles;
            let new = snapshot.player.tile;
            if steps.is_empty() {
                if new != player.tile {
                    // moved without stepping (a teleport): be there, no gait
                    if let Some(p) = ground(new) {
                        transform.translation = p;
                    }
                    player.route.clear();
                }
            } else {
                player.steps = steps.len().min(2) as i32;

                // He runs through the tile centres the server stepped over, in order, exactly as
                // the game draws it (see `route`).
                let (route, length) = route::lay_route(steps, player.tile, transform.translation, ground);
                player.route = route;

                // He draws one tick behind the server: he covers this tick's move in the next
                // tick, arriving on the server's tile just as the next move begins.
                player.speed = length / (snapshot.tick_seconds.max(0.05) * server.ticks_in_latest.max(1) as f32);
            }
            player.seen_tick = Some(snapshot.tick);
            player.tile = new;
        }

        // Run along the route.
        let budget = player.speed * time.delta_secs();
        let moving = !player.route.is_empty();
        let heading = route::follow_route(&mut player.route, &mut transform, budget, |x, z| streamed.surface_y(0, x, z));
        if let Some(heading) = heading {
            // the compass point of the step he is on (model front is +z at rotation zero)
            player.heading = heading;
        }
        // While interacting with an NPC the body is turned toward it however he is moving (so a
        // walk to it can go sideways or backwards); otherwise toward the way he is going. Either
        // way it eases in and out, even once he has stopped, so a turn in progress finishes.
        let toward_target = snapshot
            .player
            .face_target
            .and_then(|id| npc_views.entity_of(id))
            .and_then(|entity| npc_transforms.get(entity).ok())
            .map(|target| combat_fx::heading_between(transform.translation, target.translation));
        let (heading, facing, dt) = (toward_target.unwrap_or(player.heading), player.facing, time.delta_secs());
        player.facing = player.turn.step(facing, heading, dt);
        transform.rotation = Quat::from_rotation_y(player.facing);

        if moving {
            player.still_for = 0.0;
        } else {
            player.still_for += time.delta_secs();
        }

        // How far the body still has to turn to face the way he is going, to his right positive,
        // in the game's 2048-unit angle: the walk animation depends on it.
        let moving_on = player.steps != 0 && player.still_for <= WALK_LINGER;
        // (walking: against the way he is going, so facing a target sideways plays the strafe;
        // standing: against whatever he is turning to)
        let to_turn = {
            use std::f32::consts::{PI, TAU};
            let wanted = if moving_on { player.heading } else { heading };
            let diff = (wanted - player.facing + PI).rem_euclid(TAU) - PI;
            -diff * 2048.0 / TAU
        };
        let clip = if !moving_on {
            // standing: turn on the spot while the facing catches up, else idle
            if player.steps != 0 && to_turn.abs() > 32.0 { CLIP_TURN } else { CLIP_IDLE }
        } else if to_turn.abs() <= 256.0 {
            if player.steps >= 2 { CLIP_RUN } else { CLIP_WALK }
        } else if to_turn > 0.0 && to_turn < 768.0 {
            CLIP_RIGHT
        } else if to_turn < 0.0 && to_turn > -768.0 {
            CLIP_LEFT
        } else {
            CLIP_BACK
        };
        // said once per change of pose, not every frame a one-shot animation keeps it from showing
        if *logged_clip != Some(clip) {
            *logged_clip = Some(clip);
            println!("[client] tick {}: {} (server moved {} tile(s) last tick)", snapshot.tick, PLAYER_CLIPS[clip].0, player.steps);
        }
        // a fight's attack, block and death animations take over from walking and standing
        match fighter.clip(time.elapsed_secs(), CLIP_DEATH) {
            Some((one_shot, first, _)) => {
                if first {
                    model.play_once(one_shot);
                }
            }
            None => model.play(clip),
        }
    }
}

/// G shows or hides the tile grid.
/// Hides terrain and objects above the plane in `OSRS_MAX_PLANE` (a debugging aid for telling
/// what belongs to which plane).
fn limit_planes(mut planes: Query<(&osrs_render::world::OnPlane, &mut Visibility), Added<osrs_render::world::OnPlane>>) {
    let Some(max) = std::env::var("OSRS_MAX_PLANE").ok().and_then(|v| v.parse::<u8>().ok()) else { return };
    for (plane, mut visibility) in &mut planes {
        if plane.0 > max {
            *visibility = Visibility::Hidden;
        }
    }
}

fn toggle_grid(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlay: ResMut<GridOverlay>,
    mut grid: Query<&mut Visibility, With<TileGrid>>,
) {
    if keys.just_pressed(KeyCode::KeyG) {
        overlay.shown = !overlay.shown;
        for mut visibility in &mut grid {
            *visibility = match *visibility {
                Visibility::Hidden => Visibility::Inherited,
                _ => Visibility::Hidden,
            };
        }
    }
}

/// TODO(remove): temporary debug hook, slated for removal once it is no longer needed (and not to be
/// used without the user asking: it injects input into the live window).
///
/// Debug input script, `OSRS_INPUT="t:move:x,y;t:press:left;t:release:left;..."` (seconds since
/// start, logical window pixels): drives the real click code paths without a human at the mouse.
fn scripted_input(
    time: Res<Time>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
    mut script: Local<Option<Vec<(f32, String, String)>>>,
) {
    let steps = script.get_or_insert_with(|| {
        let text = std::env::var("OSRS_INPUT").unwrap_or_default();
        text.split(';')
            .filter_map(|step| {
                let mut p = step.trim().splitn(3, ':');
                Some((p.next()?.parse().ok()?, p.next()?.to_owned(), p.next()?.to_owned()))
            })
            .collect()
    });
    let now = time.elapsed_secs();
    while steps.first().is_some_and(|(at, _, _)| *at <= now) {
        let (_, action, arg) = steps.remove(0);
        let button = |name: &str| match name {
            "right" => MouseButton::Right,
            _ => MouseButton::Left,
        };
        match action.as_str() {
            "move" => {
                let xy: Vec<f32> = arg.split(',').filter_map(|v| v.parse().ok()).collect();
                if let (Ok(mut window), [x, y]) = (windows.single_mut(), &xy[..]) {
                    // set directly: moving the OS cursor needs window focus
                    let scale = window.scale_factor() as f64;
                    window.set_physical_cursor_position(Some(bevy::math::DVec2::new(*x as f64 * scale, *y as f64 * scale)));
                }
            }
            "press" => buttons.press(button(&arg)),
            "release" => buttons.release(button(&arg)),
            _ => {}
        }
    }
}

fn screenshot_and_exit(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<ScreenshotState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = state.path.clone() else { return };
    // Pipelines compile asynchronously, so wait on wall-clock time rather than frames.
    let delay: f32 = std::env::var("OSRS_SCREENSHOT_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0);
    let t = time.elapsed_secs();
    if !state.shot && t > delay {
        state.shot = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if state.shot && t > delay + 4.0 {
        finish_recording();
        exit.write(AppExit::Success);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f32::consts::PI;

    /// Runs a turn at 60 Hz until it lands; returns (angle, seconds, speeds in rad/s).
    fn run_turn(from: f32, to: f32) -> (f32, f32, Vec<f32>) {
        let mut turn = Turn::default();
        let (mut angle, dt) = (from, 1.0 / 60.0);
        let mut speeds = Vec::new();
        let mut t = 0.0;
        while t < 4.0 {
            angle = turn.step(angle, to, dt);
            speeds.push(turn.velocity.abs());
            t += dt;
            if turn.is_at_rest() {
                break;
            }
        }
        (angle, t, speeds)
    }

    #[test]
    fn a_turn_never_exceeds_the_top_speed() {
        for target in [0.3, 1.0, PI / 2.0, 2.5, PI] {
            let (_, _, speeds) = run_turn(0.0, target);
            let peak = speeds.iter().cloned().fold(0.0, f32::max);
            assert!(peak <= MAX_TURN_SPEED + 1e-4, "turn of {target} peaked at {peak} rad/s");
        }
    }

    #[test]
    fn a_reversal_is_finished_within_one_server_tick() {
        // clicking back and forth once a tick: the model must be fully turned by the time the
        // tick (0.6 s) is up, so it can finish the move onto the tile
        let (angle, seconds, speeds) = run_turn(0.0, PI);
        assert!((angle - PI).cos() > 0.99999, "ended at {angle}");
        assert!(seconds < 0.6, "a 180 took {seconds}s");
        let peak = speeds.iter().cloned().fold(0.0, f32::max);
        assert!(peak > MAX_TURN_SPEED * 0.97, "peak {peak}");
    }

    #[test]
    fn it_starts_gradually_and_ends_at_rest() {
        let (_, _, speeds) = run_turn(0.0, PI);
        let peak = speeds.iter().cloned().fold(0.0, f32::max);
        // no jump to speed: the first frame is a sliver of top speed
        assert!(speeds[0] < peak * 0.10, "first frame {} of peak {peak}", speeds[0]);
        // and speed changes smoothly frame to frame, both ways
        // (the last frame is the landing: from a few tens of degrees a second to rest)
        let ramp = &speeds[..speeds.len() - 1];
        let biggest_step = ramp.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f32::max);
        // (never more than the acceleration limit allows in one 60 Hz frame)
        assert!(biggest_step <= TURN_ACCELERATION / 60.0 + 1e-3, "speed jumped by {biggest_step} (peak {peak})");
        // it lands slowly (a small fraction of top speed) rather than running flat out into the target
        assert!(speeds[speeds.len() - 2] < peak * 0.2, "arrived at {}", speeds[speeds.len() - 2]);
    }

    #[test]
    fn smaller_turns_are_quicker_but_not_instant() {
        let (_, quarter, _) = run_turn(0.0, PI / 2.0);
        let (_, eighth, _) = run_turn(0.0, PI / 4.0);
        let (_, half, _) = run_turn(0.0, PI);
        assert!(eighth < quarter && quarter < half, "{eighth} {quarter} {half}");
        assert!(eighth > 0.15, "{eighth}");
    }

    #[test]
    fn turns_take_the_short_way_round_across_the_wrap() {
        let from = 350f32.to_radians();
        let mut turn = Turn::default();
        let mut angle = from;
        for _ in 0..10 {
            angle = turn.step(angle, 10f32.to_radians(), 1.0 / 60.0);
        }
        assert!(angle > from && turn.velocity > 0.0, "turned the long way: {angle}");
    }

    #[test]
    fn re_aiming_mid_turn_never_jumps_the_angular_speed() {
        let mut turn = Turn::default();
        let (mut angle, dt) = (0.0f32, 1.0 / 60.0);
        for _ in 0..8 {
            angle = turn.step(angle, 2.0, dt);
        }
        let before = turn.velocity;
        assert!(before > 2.0, "should be well into the turn: {before}");
        // aim the other way (still the short arc): speed must change continuously, within the
        // acceleration limit, through zero and out the other side
        let mut last = before;
        let mut reversed = false;
        for _ in 0..20 {
            angle = turn.step(angle, -1.0, dt);
            assert!((turn.velocity - last).abs() <= TURN_ACCELERATION * dt + 1e-3, "{last} -> {}", turn.velocity);
            reversed |= turn.velocity < 0.0;
            last = turn.velocity;
        }
        assert!(reversed, "it should have swung round the other way");
    }

    #[test]
    fn a_slow_frame_still_sees_every_tile_stepped_through() {
        use osrs_core::sim::scenarios::dev_open_floor_world;
        use osrs_core::sim::{PlayerConfig, Server, TickDriver};

        let world = dev_open_floor_world(15, 1, TileCoord::new(0, 0), PlayerConfig::default());
        let mut server = Server::new(world, TickDriver::default());
        server.advance(0.0);
        server.handle_command(ClientCommand::SetRunning(true));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(8, 0), force_inverse: false });
        let snapshots: Vec<Snapshot> = (0..3).filter_map(|_| server.advance(0.6)).collect();
        let stepped: Vec<TileCoord> = snapshots.iter().flat_map(|s| s.player.step_tiles.clone()).collect();
        assert!(stepped.len() >= 3, "{stepped:?}");

        let (merged, ticks) = merge_snapshots(snapshots).unwrap();
        assert_eq!(ticks, 3);
        assert_eq!(merged.player.step_tiles, stepped, "every step, in order");
        assert_eq!(merged.player.step_tiles.last(), Some(&merged.player.tile));
        assert!(merge_snapshots(Vec::new()).is_none());
    }

    #[test]
    fn heading_convention_matches_the_render_space() {
        // east is +x in both; north (+z in tiles) is -z in the render space, so the model, whose
        // front is +z at rotation zero, must turn half way round to face north
        let heading = |dx: f32, dz: f32| dx.atan2(-dz);
        assert!((heading(0.0, 1.0).abs() - PI).abs() < 1e-5, "north");
        assert!(heading(0.0, -1.0).abs() < 1e-5, "south is the model front");
        assert!((heading(1.0, 0.0) - PI / 2.0).abs() < 1e-5, "east");
        assert!((heading(-1.0, 0.0) + PI / 2.0).abs() < 1e-5, "west");
    }
}
