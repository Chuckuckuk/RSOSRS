//! Map editor: place and remove scenery, resource nodes, NPC spawns and minimap icons, then
//! compile the edits into a new game cache the game can be switched to. The installed game's
//! cache is never written to (see `osrs_cache::compile` and `osrs_cache::active`).
//!
//! usage: osrs_map_editor [--project <name>] [--base <cache dir>] [--at <x>,<z>]
//!                        [--compile <cache name>] [--activate] [--play]
//!   --project  the project to open, `~/.rsosrs/projects/<name>.json` (default: "default"; made
//!              if it does not exist)
//!   --base     the cache the edits are made on (default: the installed game's)
//!   --at       the tile to start at (default: Lumbridge); with --play, where the game starts
//!   --compile  no window: compile the project into `~/.rsosrs/caches/<cache name>` (it becomes
//!              the project's cache) and exit
//!   --activate no window: make the project's cache the one the game reads
//!   --play     no window: start the game on the project's cache, compiling one first (named
//!              after the project) if it has none
//!
//! controls: WASD move, Q/E down/up, hold right mouse to look, Shift = fast, wheel = speed;
//!           V/B/X select, place, erase; R rotate; 1-4 plane; T roofs; Delete removes the
//!           selection; Ctrl+Z / Ctrl+Y undo / redo; Ctrl+S save; F5 play.

mod camera;
mod catalog;
mod editor;
mod play;
mod preview;
mod tools;
mod ui;
mod world;

use bevy::prelude::*;
use bevy::window::WindowCloseRequested;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};
use osrs_cache::project::MapProject;
use osrs_cache::Cache;
use osrs_render::streaming::{SquareStream, StreamedWorld};
use osrs_render::world::{MaterialCache, TextureStore, WorldRenderPlugin};
use std::path::PathBuf;

struct Args {
    project: String,
    base: Option<PathBuf>,
    at: (i32, i32),
    /// Whether `--at` was given (rather than the default).
    at_given: bool,
    compile: Option<String>,
    activate: bool,
    play: bool,
}

fn args() -> Args {
    let mut out =
        Args { project: "default".into(), base: None, at: (3222, 3218), at_given: false, compile: None, activate: false, play: false };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--project" => out.project = it.next().unwrap_or_else(|| usage("--project needs a name")),
            "--base" => out.base = Some(PathBuf::from(it.next().unwrap_or_else(|| usage("--base needs a directory")))),
            "--at" => {
                let v = it.next().unwrap_or_else(|| usage("--at needs x,z"));
                let p: Vec<i32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
                match p[..] {
                    [x, z] => out.at = (x, z),
                    _ => usage("--at needs x,z"),
                }
                out.at_given = true;
            }
            "--compile" => out.compile = Some(it.next().unwrap_or_else(|| usage("--compile needs a cache name"))),
            "--activate" => out.activate = true,
            "--play" => out.play = true,
            "-h" | "--help" => usage(""),
            other => usage(&format!("unknown argument {other}")),
        }
    }
    out
}

fn usage(why: &str) -> ! {
    if !why.is_empty() {
        eprintln!("{why}");
    }
    eprintln!("usage: osrs_map_editor [--project <name>] [--base <cache dir>] [--at <x>,<z>] [--compile <cache name>] [--activate] [--play]");
    std::process::exit(2);
}

fn fail(what: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("{what}: {e}");
    std::process::exit(1);
}

fn main() {
    let args = args();
    let base_dir = match args.base.clone().or_else(osrs_cache::active::live_dir) {
        Some(dir) => dir,
        None => fail("no game cache", "install the game (RuneLite) or pass --base <cache dir>"),
    };
    if !osrs_cache::active::is_cache_dir(&base_dir) {
        fail("not a cache", base_dir.display());
    }
    // everything in this process (the square loader thread too) reads the base cache, plus the
    // overlay of edits, whichever cache the game has been switched to
    std::env::set_var(osrs_cache::active::ENV_CACHE_DIR, &base_dir);
    println!("[editor] editing on {}", base_dir.display());

    let path = MapProject::path_of(&args.project);
    let project = if path.exists() {
        MapProject::load(&path).unwrap_or_else(|e| fail(&format!("could not open {}", path.display()), e))
    } else {
        let mut project = MapProject::new(&args.project);
        project.npc_spawns = editor::default_spawns();
        project
    };
    println!("[editor] project {} ({})", project.name, path.display());

    if args.compile.is_some() || args.play || args.activate {
        let mut project = project;
        // a project that has a cache is played (or activated) as it is; --compile, or a project
        // without one, compiles first
        let existing = if args.compile.is_none() { project.compiled_cache() } else { None };
        let dir = match existing {
            Some(dir) => dir,
            None => {
                let name = args.compile.clone().unwrap_or_else(|| ui::clean_name(&project.name));
                let out = osrs_cache::active::cache_dir_of(&name);
                let started = std::time::Instant::now();
                let manifest = osrs_cache::compile::compile(&project, &base_dir, &out, |stage| println!("[editor] {stage:?}"))
                    .unwrap_or_else(|e| fail("the compile failed", e));
                println!(
                    "[editor] compiled {} in {:.1?}: {} squares, {} NPC spawns, {} icon objects",
                    out.display(),
                    started.elapsed(),
                    manifest.squares,
                    manifest.npc_spawns,
                    manifest.marker_defs
                );
                // the cache this project plays on from now on
                project.cache = Some(name);
                project.save(&path).unwrap_or_else(|e| fail("could not save the project", e));
                out
            }
        };
        if args.activate {
            osrs_cache::active::set_active(&osrs_cache::active::Selection::Dir(dir.clone()))
                .unwrap_or_else(|e| fail("could not make it the active cache", e));
            println!("[editor] the game now reads {}", dir.display());
        }
        if args.play {
            let how = play::launch(&dir, args.at_given.then_some(args.at)).unwrap_or_else(|e| fail("could not play", e));
            println!("[editor] started the game on {} ({how})", dir.display());
        }
        return;
    }

    let base = Cache::open(&base_dir).unwrap_or_else(|e| fail("could not open the cache", e)).ignoring_overlay();
    let catalog = catalog::Catalog::load(&base).unwrap_or_else(|e| fail("could not read the cache's definitions", e));
    let textures = osrs_cache::textures::load_textures(&base).unwrap_or_else(|e| fail("could not read the cache's textures", e));
    let picker = world::EditorPicker::new(&catalog);
    let mut editor = editor::Editor::new(project, path, base, base_dir.clone());
    editor.status = format!("{} objects, {} NPCs, {} icons", catalog.objects.len(), catalog.npcs.len(), catalog.areas.len());
    let shared = preview::leak_cache(&base_dir).unwrap_or_else(|e| fail("could not open the cache", e));

    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "OSRS map editor".into(), ..default() }),
        ..default()
    }))
    .add_plugins((EguiPlugin::default(), WorldRenderPlugin))
    .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
    .insert_resource(world::Textures(TextureStore(textures)))
    .insert_resource(catalog)
    .insert_resource(picker)
    .insert_resource(editor)
    .insert_resource(preview::MinimapPreview::new(shared))
    .insert_resource(preview::Sprites::new(shared))
    // OSRS_ROOFS=0 starts with the planes above the edited one hidden (T toggles)
    .insert_resource(camera::ViewState { roofs: std::env::var("OSRS_ROOFS").map_or(true, |v| v != "0") })
    .insert_resource(StartAt(args.at))
    .init_resource::<StreamedWorld>()
    .init_resource::<MaterialCache>()
    .init_resource::<world::Hover>()
    .init_resource::<world::Ghost>()
    .init_resource::<tools::IconsInView>()
    .init_resource::<ui::UiState>()
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (
            world::find_hover,
            camera::fly_camera,
            tools::shortcuts,
            tools::world_clicks,
            world::stream_world,
            world::update_ghost,
            world::plane_visibility,
            tools::track_icons,
            tools::draw_marks,
            save_on_close,
            screenshot_and_exit,
        )
            .chain(),
    )
    .add_systems(EguiPrimaryContextPass, ui::editor_ui);
    app.world_mut().insert_non_send(SquareStream::spawn());
    app.run();
}

#[derive(Resource)]
struct StartAt((i32, i32));

fn setup(mut commands: Commands, at: Res<StartAt>) {
    camera::spawn_camera(&mut commands, at.0);
}

/// `OSRS_SCREENSHOT=<path.png>`: save a screenshot after `OSRS_SCREENSHOT_DELAY` seconds (default
/// 10) and exit, as `osrs_viewer` does.
fn screenshot_and_exit(mut commands: Commands, time: Res<Time>, mut taken: Local<bool>, mut exit: MessageWriter<AppExit>) {
    use bevy::render::view::screenshot::{save_to_disk, Screenshot};
    let Ok(path) = std::env::var("OSRS_SCREENSHOT") else { return };
    let delay: f32 = std::env::var("OSRS_SCREENSHOT_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(10.0);
    let t = time.elapsed_secs();
    if !*taken && t > delay {
        *taken = true;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if *taken && t > delay + 3.0 {
        exit.write(AppExit::Success);
    }
}

/// Closing the window keeps the work.
fn save_on_close(mut closing: MessageReader<WindowCloseRequested>, mut editor: ResMut<editor::Editor>) {
    if closing.read().count() > 0 && editor.dirty {
        editor.save();
        println!("[editor] {}", editor.status);
    }
}
