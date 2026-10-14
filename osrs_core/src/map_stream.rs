//! Keeps the map squares around the player loaded: asks the loading thread for the ones that are
//! missing, spawns them as they arrive and drops the ones left far behind.
//!
//! The server does the same for its collision map (see `main.rs`), around the tile the player
//! really stands on, with the same radius, so ground the client shows is ground the server can
//! walk on.

use super::ground_click::GroundPicker;
use super::scenery_swaps::SwapMeshes;
use super::{GridOverlay, Player, Textures};
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use osrs_render::streaming::{square_of, square_offset, Loaded, SquareData, SquareStream, StreamedWorld};
use osrs_render::pick::Picker;
use osrs_render::world::{spawn_parts, square_tile_grid_mesh, tile_under, MaterialCache, ObjectScene, ScrollingTextures, TileGrid};
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;

/// Squares kept loaded in each direction around the player's square: 1 is a 3x3 block, which is
/// 192 tiles across, always at least 64 tiles of map in every direction.
pub const RADIUS: i32 = 1;

/// The radius in use: `RADIUS` in the open world, 0 in a scenario that sits on a real place (the
/// Fight Cave fills its own square, and nothing around it belongs to the fight).
#[derive(Resource, Clone, Copy)]
pub struct StreamRadius(pub i32);

/// Everything needed to put a decoded square into the scene, or to take one out.
#[derive(SystemParam)]
pub struct SquareSpawner<'w, 's> {
    pub commands: Commands<'w, 's>,
    pub meshes: ResMut<'w, Assets<Mesh>>,
    pub materials: ResMut<'w, Assets<StandardMaterial>>,
    pub images: ResMut<'w, Assets<Image>>,
    scrolling: ResMut<'w, ScrollingTextures>,
    shared: ResMut<'w, MaterialCache>,
    textures: Res<'w, Textures>,
    pub world: ResMut<'w, StreamedWorld>,
    picker: ResMut<'w, GroundPicker>,
    grid: ResMut<'w, GridOverlay>,
}

impl SquareSpawner<'_, '_> {
    pub fn spawn(&mut self, data: &SquareData) {
        let offset = square_offset(data.square);
        self.world.spawn_square(
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials,
            &mut self.images,
            &mut self.scrolling,
            &mut self.shared,
            &self.textures.0,
            data,
        );
        self.picker.0.add(data.square, &data.objects, offset);

        let lines = self.meshes.add(square_tile_grid_mesh(data));
        let material = match &self.grid.material {
            Some(material) => material.clone(),
            None => {
                let material = self.materials.add(StandardMaterial {
                    base_color: Color::BLACK,
                    unlit: true,
                    ..default()
                });
                self.grid.material = Some(material.clone());
                material
            }
        };
        let visibility = if self.grid.shown { Visibility::Inherited } else { Visibility::Hidden };
        let entity = self
            .commands
            .spawn((Mesh3d(lines), MeshMaterial3d(material), Transform::from_translation(offset), TileGrid, visibility))
            .id();
        self.world.attach(data.square, entity);
    }

    /// Spawns the objects of `scene` (positions in world render space) as part of `square`, so
    /// they leave with it, clickable under `owner`. Returns their entities.
    pub fn spawn_objects(&mut self, square: (i32, i32), owner: (i32, i32), scene: &Arc<ObjectScene>) -> Vec<Entity> {
        let entities = spawn_parts(
            &mut self.commands,
            &mut self.meshes,
            &mut self.materials,
            &mut self.images,
            &mut self.scrolling,
            &mut self.shared,
            &self.textures.0,
            &[],
            scene,
            Vec3::ZERO,
        );
        for entity in &entities {
            self.world.attach(square, *entity);
        }
        self.picker.0.add(owner, scene, Vec3::ZERO);
        entities
    }

    pub fn picker(&mut self) -> &mut Picker {
        &mut self.picker.0
    }

    pub fn despawn(&mut self, square: (i32, i32)) {
        for entity in self.world.remove(square) {
            self.commands.entity(entity).despawn();
        }
        self.picker.0.remove(square);
    }
}

/// Squares requested and not yet back, and squares that are back and waiting to be spawned.
#[derive(Default)]
pub struct Pending {
    requested: HashSet<(i32, i32)>,
    ready: VecDeque<SquareData>,
}

/// The square of the world tile `transform` is standing on.
fn square_under(transform: &Transform) -> (i32, i32) {
    let (x, z) = tile_under(transform.translation);
    square_of(x, z)
}

fn near(square: (i32, i32), focus: (i32, i32), radius: i32) -> bool {
    (square.0 - focus.0).abs() <= radius && (square.1 - focus.1).abs() <= radius
}

/// Loads the squares around the player, and unloads the ones more than `RADIUS + 1` squares
/// away (one ring further than is loaded, so walking back and forth along a border does not
/// load and drop the same squares over and over).
pub fn stream_map(
    radius: Res<StreamRadius>,
    stream: NonSend<SquareStream>,
    mut pending: Local<Pending>,
    mut swaps: ResMut<SwapMeshes>,
    players: Query<&Transform, With<Player>>,
    mut spawner: SquareSpawner,
) {
    let Ok(player) = players.single() else { return };
    let focus = square_under(player);

    // nearest first, so the ground under his feet is never the last to arrive
    let mut wanted: Vec<(i32, i32)> = Vec::new();
    let radius = radius.0;
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            wanted.push((focus.0 + dx, focus.1 + dz));
        }
    }
    wanted.sort_by_key(|s| (s.0 - focus.0).abs() + (s.1 - focus.1).abs());
    for square in wanted {
        if !spawner.world.contains(square) && pending.requested.insert(square) {
            stream.request(square);
        }
    }

    for loaded in stream.poll() {
        match loaded {
            Loaded::Ready(data) => {
                pending.requested.remove(&data.square);
                pending.ready.push_back(data);
            }
            Loaded::Failed(square, why) => {
                pending.requested.remove(&square);
                eprintln!("[client] could not load map square ({}, {}): {why}", square.0, square.1);
            }
            // a replacement object's mesh (see `scenery_swaps`)
            Loaded::Object { id, kind, orientation, mesh, clip } => {
                if let Some(clip) = clip {
                    swaps.clips.insert((id, kind, orientation), clip);
                }
                swaps.ready.insert((id, kind, orientation), mesh);
            }
            // a ground item's mesh (see `ground_scene`)
            Loaded::Item { id, mesh } => {
                swaps.items.insert(id, mesh);
            }
        }
    }

    // One square into the scene per frame: spawning thousands of entities for several squares at
    // once would stall the frame, and the characters are drawn from server ticks that must not wait.
    while let Some(data) = pending.ready.pop_front() {
        // the player may have walked on while it was being decoded
        if near(data.square, focus, radius + 1) && !spawner.world.contains(data.square) {
            let started = std::time::Instant::now();
            spawner.spawn(&data);
            let took = started.elapsed();
            if took.as_millis() > 20 {
                eprintln!("[client] spawning map square ({}, {}) took {took:.2?}", data.square.0, data.square.1);
            }
            break;
        }
    }

    let gone: Vec<(i32, i32)> = spawner.world.squares().filter(|s| !near(*s, focus, radius + 1)).collect();
    for square in gone {
        spawner.despawn(square);
    }
}
