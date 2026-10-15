//! Outlines on the ground: a thin blue one on the tile the server has the player on, and a lighter
//! blue one on the tile under the cursor.

use super::ground_click::GroundPicker;
use super::interface::UiHits;
use super::{OrbitCamera, ServerLink};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use osrs_render::streaming::StreamedWorld;

/// The render layer the outlines are on.
pub const GIZMO_LAYER: usize = 7;

/// The player's server-side tile.
const PLAYER_TILE: Color = Color::srgb(0.15, 0.4, 1.0);
/// The tile under the cursor.
const HOVER_TILE: Color = Color::srgb(0.6, 0.78, 1.0);
/// How far above the ground the lines float, tiles (so the terrain does not cut them).
const LIFT: f32 = 0.02;

pub struct TileMarksPlugin;

impl Plugin for TileMarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HoverTile>().add_systems(Startup, thin_lines).add_systems(Update, (find_hover_tile, draw_marks).chain());
    }
}

/// The world tile under the cursor, if the cursor is over the world.
#[derive(Resource, Default)]
struct HoverTile(Option<(i32, i32)>);

fn thin_lines(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 1.5;
    // drawn in front of the terrain it lies on
    config.depth_bias = -1.0;
    // only the 3D camera draws them (the fixed layout's UI camera must not)
    config.render_layers = bevy::camera::visibility::RenderLayers::layer(GIZMO_LAYER);
}

fn find_hover_tile(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    world: Res<StreamedWorld>,
    picker: Res<GroundPicker>,
    ui: Res<UiHits>,
    mut hover: ResMut<HoverTile>,
) {
    hover.0 = None;
    let Ok(window) = windows.single() else { return };
    let Ok((camera, camera_transform)) = cameras.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    // the interface covers the world
    if ui.target_at(cursor).is_some() {
        return;
    }
    let origin = camera.logical_viewport_rect().map_or(Vec2::ZERO, |r| r.min);
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor - origin) else { return };
    hover.0 = picker.0.pick_all(&*world, ray).map(|click| click.tile);
}

fn draw_marks(mut gizmos: Gizmos, world: Res<StreamedWorld>, server: NonSend<ServerLink>, hover: Res<HoverTile>) {
    let outline = |gizmos: &mut Gizmos, tile: (i32, i32), colour: Color| {
        if let Some([a, b, c, d]) = world.block_corners(tile.0, tile.1, 1, 1, LIFT) {
            gizmos.linestrip([a, b, c, d, a], colour);
        }
    };
    if let Some(snapshot) = &server.latest {
        let tile = snapshot.player.tile;
        outline(&mut gizmos, (tile.x, tile.z), PLAYER_TILE);
    }
    if let Some(tile) = hover.0 {
        outline(&mut gizmos, tile, HOVER_TILE);
    }
}
