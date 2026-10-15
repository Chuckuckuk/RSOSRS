//! OSRS world viewer: loads map squares straight from the local game cache and renders the
//! terrain with the client's colour blending and lighting. An exploratory/debugging tool; the
//! world loading and rendering itself lives in `osrs_render` and is shared with the game client.
//!
//! usage: osrs_viewer [region_x region_y [nx ny]]      (defaults: Lumbridge, 3x3 squares)
//! env:   OSRS_SCREENSHOT=<path.png>   save a screenshot after a few frames and exit
//!
//! controls: WASD move, Q/E down/up, hold right mouse to look, Shift = fast, scroll = speed,
//!           1-4 = viewer plane (roofs hidden above it), R = toggle roofs / upper planes
//! env:   OSRS_SCREENSHOT_DELAY=<seconds> (default 8) when to capture
//! env:   OSRS_PLANE=0..3  OSRS_ROOFS=0|1   initial plane / roofs

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy::window::{CursorOptions, PrimaryWindow};
use osrs_render::fly::{grab_cursor, FlyCamera};
use osrs_render::world::{load_world, spawn_world, OnPlane, ScrollingTextures, WorldData, WorldRenderPlugin};

#[derive(Resource, Default)]
struct ScreenshotState {
    path: Option<String>,
    frames: u32,
}

/// Loaded once in `main`, consumed by `setup`.
#[derive(Resource)]
struct PendingWorld(Option<WorldData>);

/// Which planes are drawn: with `roofs` on everything up to plane 3 shows (viewer outside);
/// with it off only planes <= `plane` (viewer inside, like the client's roof removal).
#[derive(Resource)]
struct ViewState {
    plane: u8,
    roofs: bool,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let num = |i: usize, d: i32| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    let (rx, ry, nx, ny) = (num(0, 49), num(1, 49), num(2, 3), num(3, 3));

    let world = match load_world(rx, ry, nx as usize, ny as usize) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("failed to load terrain: {e}");
            std::process::exit(1);
        }
    };

    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "OSRS viewer".into(), ..default() }),
            ..default()
        }))
        .add_plugins(WorldRenderPlugin)
        .insert_resource(ClearColor(Color::srgb(0.02, 0.02, 0.03)))
        .insert_resource(PendingWorld(Some(world)))
        .insert_resource(ScreenshotState { path: std::env::var("OSRS_SCREENSHOT").ok(), frames: 0 })
        .insert_resource(ViewState {
            plane: std::env::var("OSRS_PLANE").ok().and_then(|v| v.parse().ok()).unwrap_or(0),
            roofs: std::env::var("OSRS_ROOFS").map(|v| v != "0").unwrap_or(true),
        })
        .add_systems(Startup, setup)
        .add_systems(Update, (fly_camera, view_controls, screenshot_and_exit))
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scrolling: ResMut<ScrollingTextures>,
    mut pending: ResMut<PendingWorld>,
) {
    let Some(world) = pending.0.take() else { return };
    spawn_world(&mut commands, &mut meshes, &mut materials, &mut images, &mut scrolling, &world);

    // Start above the middle of the loaded area, looking north-east and down.
    let (center, extent) = world.center_and_extent();
    let mut target = center;
    let mut eye = center + Vec3::new(-extent * 0.25, extent * 0.22, extent * 0.35);
    // OSRS_LOOK=x,z,dist: look at grid tile (x, z) from `dist` tiles away
    if let Ok(v) = std::env::var("OSRS_LOOK") {
        let p: Vec<f32> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        if p.len() == 3 {
            target = Vec3::new(p[0] + 0.5, 0.0, -(p[1] + 0.5));
            eye = target + Vec3::new(-p[2] * 0.3, p[2] * 0.8, p[2] * 0.5);
        }
    }
    let look = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    commands.spawn((Camera3d::default(), look, FlyCamera::facing(&look, extent * 0.15)));
}

fn view_controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<ViewState>,
    mut things: Query<(&OnPlane, &mut Visibility)>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut first: Local<bool>,
) {
    let mut changed = !*first;
    *first = true;
    for (i, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4].into_iter().enumerate() {
        if keys.just_pressed(key) {
            state.plane = i as u8;
            changed = true;
        }
    }
    if keys.just_pressed(KeyCode::KeyR) {
        state.roofs = !state.roofs;
        changed = true;
    }
    if !changed {
        return;
    }
    let max_plane = if state.roofs { 3 } else { state.plane };
    for (plane, mut vis) in &mut things {
        *vis = if plane.0 <= max_plane { Visibility::Inherited } else { Visibility::Hidden };
    }
    if let Ok(mut w) = windows.single_mut() {
        w.title = format!(
            "OSRS viewer - plane {} - roofs {}",
            state.plane + 1,
            if state.roofs { "on (all planes)" } else { "off" }
        );
    }
}

fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cams: Query<(&mut Transform, &mut FlyCamera)>,
) {
    let Ok((mut tf, mut cam)) = cams.single_mut() else { return };
    let looking = buttons.pressed(MouseButton::Right);
    if let Ok(mut cursor) = windows.single_mut() {
        grab_cursor(&mut cursor, looking);
    }
    if looking {
        cam.look(&mut tf, motion.delta);
    }
    cam.zoom(scroll.delta.y, 2000.0);
    cam.fly(&mut tf, &keys, time.delta_secs());
}

fn screenshot_and_exit(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<ScreenshotState>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(path) = state.path.clone() else { return };
    // Pipelines compile asynchronously, so wait on wall-clock time rather than frames.
    let t = time.elapsed_secs();
    let delay: f32 = std::env::var("OSRS_SCREENSHOT_DELAY").ok().and_then(|v| v.parse().ok()).unwrap_or(8.0);
    if state.frames == 0 && t > delay {
        state.frames = 1;
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
    }
    if state.frames == 1 && t > delay + 4.0 {
        exit.write(AppExit::Success);
    }
}
