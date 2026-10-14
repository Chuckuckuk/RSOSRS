//! Things the server has put in the world that the map does not have: fires players lit
//! (`Snapshot::spawned`) and items lying on the ground (`Snapshot::ground_items`).
//!
//! Like `scenery_swaps` this is a diff of what the server lists against what is on screen, done
//! when a tick's snapshot arrives or a mesh comes back. Each thing is spawned as a one-object
//! scene attached to its map square, so it leaves with the square and is put back (the entities
//! are noticed gone) when the square returns. Meshes are built on the loading thread, once per
//! object (a fire) or cache item.
//!
//! Ground items are not click boxes: they are found by the tile a click ends on (`ground_click`).

use super::interface::ItemIcons;
use super::map_stream::SquareSpawner;
use super::scenery_swaps::SwapMeshes;
use super::{ServerLink, Textures};
use bevy::prelude::*;
use osrs_core::sim::LocKey;
use osrs_render::streaming::{square_of, SquareStream};
use osrs_render::spawn_animated;
use osrs_render::world::ObjectScene;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Item meshes are keyed apart from object meshes by this much in the object id.
const ITEM_KEY_BASE: u32 = 1_000_000;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Thing {
    Fire(LocKey),
    Item(u32),
}

impl Thing {
    /// The click-box owner key of the thing: its own, so it can be dropped alone. Map squares own
    /// theirs by square coordinates and swaps by `i32::MIN / 2 - x`; these are far from both.
    fn owner(&self) -> (i32, i32) {
        match self {
            Thing::Fire(key) => (i32::MIN / 4 - key.x, key.z),
            Thing::Item(uid) => (i32::MIN / 4 + 1, *uid as i32),
        }
    }
}

/// What the last pass saw, so a frame where nothing changed costs nothing.
#[derive(Default)]
pub struct Seen {
    tick: Option<i32>,
    meshes: usize,
}

/// Brings the scene in line with the server's fires and ground items.
#[allow(clippy::too_many_arguments)]
pub fn show_ground_things(
    server: NonSend<ServerLink>,
    stream: NonSend<SquareStream>,
    icons: Option<NonSend<ItemIcons>>,
    mut swaps: ResMut<SwapMeshes>,
    textures: Res<Textures>,
    mut shown: Local<BTreeMap<Thing, Vec<Entity>>>,
    mut seen: Local<Seen>,
    alive: Query<(), With<GroundThing>>,
    mut spawner: SquareSpawner,
) {
    let Some(snapshot) = &server.latest else { return };
    let meshes_ready = swaps.ready.len() + swaps.items.len();
    if seen.tick == Some(snapshot.tick) && seen.meshes == meshes_ready {
        return;
    }
    *seen = Seen { tick: Some(snapshot.tick), meshes: meshes_ready };

    // what the server says is there: the thing, its tile and the mesh it needs
    enum Mesh {
        Object { id: u32, kind: u8, orientation: u8 },
        Item(u32),
    }
    let mut wanted: BTreeMap<Thing, (osrs_core::tiles::TileCoord, Mesh)> = BTreeMap::new();
    for fire in &snapshot.spawned {
        let loc = fire.loc;
        wanted.insert(Thing::Fire(loc.key()), (loc.tile, Mesh::Object { id: loc.id, kind: loc.kind, orientation: loc.orientation }));
    }
    if let Some(icons) = icons.as_deref() {
        for item in &snapshot.ground_items {
            if let Some(def) = icons.item_named(&item.name) {
                wanted.insert(Thing::Item(item.uid), (item.tile, Mesh::Item(def.id)));
            }
        }
    }

    // gone from the server's list, or its square was dropped from under it
    for key in shown.keys().copied().collect::<Vec<_>>() {
        let entities = &shown[&key];
        let keep = wanted.contains_key(&key) && entities.iter().all(|e| alive.contains(*e));
        if !keep {
            for entity in shown.remove(&key).unwrap_or_default().into_iter().filter(|e| alive.contains(*e)) {
                spawner.commands.entity(entity).despawn();
            }
            spawner.picker().remove(key.owner());
        }
    }

    for (key, (tile, mesh)) in wanted {
        if shown.contains_key(&key) {
            continue;
        }
        // the square is not loaded: nothing to stand it on yet
        let Some(origin) = spawner.world.ground_at(tile.x, tile.z) else { continue };
        let (object_key, mesh) = match mesh {
            Mesh::Object { id, kind, orientation } => {
                let mesh_key = (id, kind, orientation);
                let Some(mesh) = swaps.ready.get(&mesh_key) else {
                    if swaps.requested_object(mesh_key) {
                        stream.request_object(id, kind, orientation);
                    }
                    continue;
                };
                ((id, kind, orientation, 255, 0), mesh.clone())
            }
            Mesh::Item(id) => {
                let Some(mesh) = swaps.items.get(&id) else {
                    if swaps.items_requested.insert(id) {
                        stream.request_item(id);
                    }
                    continue;
                };
                ((ITEM_KEY_BASE + id, 10, 0, 255, 0), mesh.clone())
            }
        };
        // an object without a model is not drawn
        let Some(mesh) = mesh else {
            shown.insert(key, Vec::new());
            continue;
        };
        // an object with an animation (flames) plays it, looping
        let clip = match key {
            Thing::Fire(loc) => swaps.clips.get(&(loc.id, 10, 0)).cloned(),
            Thing::Item(_) => None,
        };
        if let Some(clip) = clip {
            let entity = spawn_animated(
                &mut spawner.commands,
                &mut spawner.meshes,
                &mut spawner.materials,
                &mut spawner.images,
                &textures.0 .0,
                &mesh,
                vec![clip],
                Transform::from_translation(origin),
            );
            spawner.world.attach(square_of(tile.x, tile.z), entity);
            spawner.commands.entity(entity).insert(GroundThing);
            // the animated model is drawn on its own, but is clicked like any object: its box
            // comes from the same one-object scene the still ones are spawned from
            let scene = Arc::new(ObjectScene::single(object_key, mesh, origin, tile.plane as u8));
            spawner.picker().add(key.owner(), &scene, Vec3::ZERO);
            shown.insert(key, vec![entity]);
            continue;
        }
        let scene = Arc::new(ObjectScene::single(object_key, mesh, origin, tile.plane as u8));
        let entities = spawner.spawn_objects(square_of(tile.x, tile.z), key.owner(), &scene);
        for entity in &entities {
            spawner.commands.entity(*entity).insert(GroundThing);
        }
        shown.insert(key, entities);
    }
}

/// Marks the entities of a fire or ground item.
#[derive(Component)]
pub struct GroundThing;
