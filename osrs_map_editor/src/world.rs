//! The world on screen: map squares streamed in around the camera (decoded on a thread by
//! `osrs_render`'s [`SquareStream`], through the cache overlay so edits show), squares decoded again
//! when an edit changes them, and what the mouse is over.

use crate::camera::FlyCamera;
use crate::catalog::{footprint, Catalog};
use crate::editor::{Brush, Editor, Tool};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use osrs_render::pick::{Click, Picker};
use osrs_render::streaming::{square_of, square_offset, Loaded, SquareData, SquareStream, StreamedWorld};
use osrs_render::world::{tile_under, MaterialCache, OnPlane, ScrollingTextures, TextureStore};
use std::collections::{HashMap, VecDeque};

/// Squares loaded in each direction around the one the camera looks at.
const RADIUS: i32 = 1;

/// Decoded texture images, shared by every square.
#[derive(Resource)]
pub struct Textures(pub TextureStore);

/// Click boxes of every object on the ground plane (unlike the game's, every object takes clicks:
/// any of them can be selected).
#[derive(Resource)]
pub struct EditorPicker(pub Picker);

impl EditorPicker {
    pub fn new(catalog: &Catalog) -> EditorPicker {
        let names: HashMap<u32, String> = catalog.defs.keys().map(|&id| (id, catalog.object_name(id))).collect();
        let all = catalog.defs.keys().map(|&id| (id, "Select".to_string())).collect();
        EditorPicker(Picker::streamed(0, all, names))
    }
}

/// What the mouse is over in the world.
#[derive(Resource, Default)]
pub struct Hover {
    pub click: Option<Click>,
    /// Whether the pointer is over the world rather than a panel.
    pub in_world: bool,
}

impl Hover {
    pub fn tile(&self) -> Option<(i32, i32)> {
        self.click.as_ref().map(|c| c.tile)
    }
}

/// Requests in flight, and squares back from the loader waiting to be put in the scene.
#[derive(Default)]
pub struct Pending {
    /// Requests sent per square and not yet answered; a square is only put in when its last
    /// request is back, so an edit made while it was loading is never overwritten by the older
    /// answer. A square stays listed, at 0, from its answer until it is put in the scene.
    in_flight: HashMap<(i32, i32), u32>,
    ready: VecDeque<SquareData>,
}

/// The tile the camera is looking at, worked out once a frame by [`stream_world`] (which streams
/// the squares around it); the minimap and Play read it.
#[derive(Resource, Default)]
pub struct Focus {
    pub tile: (i32, i32),
}

impl Focus {
    pub fn square(&self) -> (i32, i32) {
        square_of(self.tile.0, self.tile.1)
    }
}

/// Where the ray through the middle of the view meets the ground, else the tile under the camera.
fn focus_tile(camera: &Camera, transform: &GlobalTransform, world: &StreamedWorld, picker: &Picker) -> (i32, i32) {
    let middle = camera.logical_viewport_size().map(|s| s / 2.0);
    let hit = middle.and_then(|m| camera.viewport_to_world(transform, m).ok()).and_then(|ray| picker.pick_all(world, ray));
    hit.map_or_else(|| tile_under(transform.translation()), |c| c.tile)
}

#[allow(clippy::too_many_arguments)]
pub fn stream_world(
    stream: NonSend<SquareStream>,
    mut pending: Local<Pending>,
    mut editor: ResMut<Editor>,
    mut world: ResMut<StreamedWorld>,
    mut picker: ResMut<EditorPicker>,
    mut ghost: ResMut<Ghost>,
    mut focus_tile_now: ResMut<Focus>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut scrolling: ResMut<ScrollingTextures>,
    mut shared: ResMut<MaterialCache>,
    textures: Res<Textures>,
) {
    let Ok((camera, transform)) = cameras.single() else { return };
    focus_tile_now.tile = focus_tile(camera, transform, &world, &picker.0);
    let focus = focus_tile_now.square();
    let near = |s: (i32, i32), r: i32| (s.0 - focus.0).abs() <= r && (s.1 - focus.1).abs() <= r;

    let request = |square: (i32, i32), pending: &mut Pending| {
        *pending.in_flight.entry(square).or_default() += 1;
        stream.request(square);
    };
    // squares an edit changed: decoded again if they are in the scene or on their way
    // (taken only when there are some: writing to the editor marks it changed)
    if !editor.stale.is_empty() {
        for square in std::mem::take(&mut editor.stale) {
            if world.contains(square) || pending.in_flight.contains_key(&square) {
                request(square, &mut pending);
            }
        }
    }
    let mut wanted: Vec<(i32, i32)> = (-RADIUS..=RADIUS).flat_map(|dz| (-RADIUS..=RADIUS).map(move |dx| (focus.0 + dx, focus.1 + dz))).collect();
    wanted.sort_by_key(|s| (s.0 - focus.0).abs() + (s.1 - focus.1).abs());
    for square in wanted {
        if !world.contains(square) && !pending.in_flight.contains_key(&square) {
            request(square, &mut pending);
        }
    }

    for loaded in stream.poll() {
        match loaded {
            Loaded::Ready(data) => {
                if finished(&mut pending.in_flight, data.square) {
                    pending.ready.push_back(data);
                }
            }
            Loaded::Failed(square, why) => {
                if finished(&mut pending.in_flight, square) {
                    pending.in_flight.remove(&square);
                }
                eprintln!("[editor] could not load map square {}_{}: {why}", square.0, square.1);
            }
            Loaded::Object { id, kind, orientation, mesh, .. } => ghost.arrived((id, kind, orientation), mesh),
            Loaded::Item { .. } => {}
        }
    }

    // one square into the scene per frame, so the editor stays responsive while it fills in
    while let Some(data) = pending.ready.pop_front() {
        // asked for again since (an edit): the newer answer is on its way
        if pending.in_flight.get(&data.square).is_some_and(|&n| n > 0) {
            continue;
        }
        pending.in_flight.remove(&data.square);
        if !near(data.square, RADIUS + 1) {
            continue;
        }
        for entity in world.remove(data.square) {
            commands.entity(entity).despawn();
        }
        picker.0.remove(data.square);
        world.spawn_square(&mut commands, &mut meshes, &mut materials, &mut images, &mut scrolling, &mut shared, &textures.0, &data);
        picker.0.add(data.square, &data.objects, square_offset(data.square));
        break;
    }

    let gone: Vec<(i32, i32)> = world.squares().filter(|s| !near(*s, RADIUS + 1)).collect();
    for square in gone {
        for entity in world.remove(square) {
            commands.entity(entity).despawn();
        }
        picker.0.remove(square);
    }
}

/// Notes that one request for `square` came back; whether it was the last one. The square stays
/// listed (at 0) until it is put in the scene, so it is not asked for again meanwhile.
fn finished(in_flight: &mut HashMap<(i32, i32), u32>, square: (i32, i32)) -> bool {
    let n = in_flight.entry(square).or_default();
    *n = n.saturating_sub(1);
    *n == 0
}

/// Hides the planes above the one being edited, unless roofs are shown.
pub fn plane_visibility(
    view: Res<crate::camera::ViewState>,
    editor: Res<Editor>,
    mut things: Query<(Ref<OnPlane>, &mut Visibility)>,
    mut last: Local<Option<(u8, bool)>>,
) {
    let max_plane = if view.roofs { 3 } else { editor.plane };
    let changed = *last != Some((editor.plane, view.roofs));
    *last = Some((editor.plane, view.roofs));
    for (plane, mut vis) in &mut things {
        if changed || plane.is_added() {
            *vis = if plane.0 <= max_plane { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

pub fn find_hover(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<FlyCamera>>,
    world: Res<StreamedWorld>,
    picker: Res<EditorPicker>,
    mut contexts: bevy_egui::EguiContexts,
    mut hover: ResMut<Hover>,
) {
    hover.click = None;
    hover.in_world = false;
    let over_ui = contexts.ctx_mut().is_ok_and(|ctx| ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input());
    if over_ui {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let Ok((camera, transform)) = cameras.single() else { return };
    let Some(cursor) = window.cursor_position() else { return };
    hover.in_world = true;
    let Ok(ray) = camera.viewport_to_world(transform, cursor) else { return };
    hover.click = picker.0.pick_all(&*world, ray);
}

/// The object about to be placed, drawn where it would go.
#[derive(Resource, Default)]
pub struct Ghost {
    /// `(id, kind, orientation)` shown, or asked for.
    want: Option<(u32, u8, u8)>,
    mesh: Option<osrs_cache::scene::LocMesh>,
    entity: Option<Entity>,
    /// A mesh arrived and needs spawning.
    fresh: bool,
}

impl Ghost {
    fn arrived(&mut self, key: (u32, u8, u8), mesh: Option<osrs_cache::scene::LocMesh>) {
        if self.want == Some(key) {
            self.mesh = mesh;
            self.fresh = true;
        }
    }
}

/// The `(id, kind, orientation)` the Place tool would put down, if it places an object.
pub fn placing_object(editor: &Editor, catalog: &Catalog) -> Option<(u32, u8, u8)> {
    match (&editor.tool, &editor.brush) {
        (Tool::Place, Some(Brush::Object(id))) => {
            let kind = editor.kind_override.unwrap_or_else(|| catalog.defs.get(id).map_or(10, crate::catalog::default_kind));
            Some((*id, kind, editor.rotation))
        }
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn update_ghost(
    stream: NonSend<SquareStream>,
    mut ghost: ResMut<Ghost>,
    editor: Res<Editor>,
    catalog: Res<Catalog>,
    hover: Res<Hover>,
    world: Res<StreamedWorld>,
    textures: Res<Textures>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    let want = placing_object(&editor, &catalog);
    if want != ghost.want {
        ghost.want = want;
        ghost.mesh = None;
        ghost.fresh = false;
        if let Some(entity) = ghost.entity.take() {
            commands.entity(entity).despawn();
        }
        if let Some((id, kind, orientation)) = want {
            stream.request_object(id, kind, orientation);
        }
    }
    if ghost.fresh {
        ghost.fresh = false;
        if let Some(mesh) = &ghost.mesh {
            let entity = osrs_render::spawn_loc_mesh(&mut commands, &mut meshes, &mut materials, &mut images, &textures.0 .0, mesh, Transform::default());
            commands.entity(entity).insert(Visibility::Hidden);
            ghost.entity = Some(entity);
        }
    }
    let Some(entity) = ghost.entity else { return };
    let Ok((mut transform, mut visibility)) = transforms.get_mut(entity) else { return };
    let spot = want.zip(hover.tile()).and_then(|((id, _, orientation), (x, z))| {
        let (w, h) = footprint(catalog.defs.get(&id), orientation);
        world.object_origin(x, z, w, h)
    });
    match spot {
        Some(at) => {
            transform.translation = at;
            *visibility = Visibility::Inherited;
        }
        None => *visibility = Visibility::Hidden,
    }
}
