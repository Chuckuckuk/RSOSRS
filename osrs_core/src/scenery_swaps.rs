//! Scenery the server says is, for now, another object (a felled tree is its stump until it grows
//! back, an opened door stands swung into the next tile): the client hides the original's
//! entities and click box and puts the replacement where the server says, turned as it says, and
//! undoes it when the server stops listing the change.
//!
//! The server sends the whole set of changes in every snapshot, so this is a diff against what is
//! on screen. A square that is dropped takes its replacements with it; when it comes back, its
//! originals are hidden again and the replacements spawned anew. Replacement meshes are built on
//! the square loading thread (`SquareStream::request_object`), once per object, shape and turn.

use super::map_stream::SquareSpawner;
use super::ServerLink;
use bevy::prelude::*;
use osrs_cache::scene::LocMesh;
use osrs_core::sim::{LocChange, LocKey};
use osrs_core::tiles::TileCoord;
use osrs_render::streaming::{square_of, SquareStream};
use osrs_render::world::{ObjectScene, PlacedLoc};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

/// An object mesh as placed: object id, shape, quarter turns.
pub type MeshKey = (u32, u8, u8);

/// Replacement meshes asked of the loading thread, and the ones that came back (`None`: the
/// object has no model, so the original is only hidden).
#[derive(Resource, Default)]
pub struct SwapMeshes {
    pub ready: HashMap<MeshKey, Option<LocMesh>>,
    requested: HashSet<MeshKey>,
    /// Ground item meshes by cache item id, and the ones asked for (`ground_scene`).
    pub items: HashMap<u32, Option<LocMesh>>,
    /// The baked animation of each object mesh that has one (a fire's flames).
    pub clips: HashMap<MeshKey, osrs_cache::anim::Clip>,
    pub items_requested: HashSet<u32>,
}

impl SwapMeshes {
    /// Notes that the mesh `key` is being asked for; true the first time only.
    pub fn requested_object(&mut self, key: MeshKey) -> bool {
        self.requested.insert(key)
    }
}

/// Marks the entities of a replacement, so a dropped square's are known to be gone.
#[derive(Component)]
pub struct SwapPart;

/// A change on screen, and the replacement's entities (none for one with no model).
pub struct Shown {
    change: LocChange,
    entities: Vec<Entity>,
}

/// What the last pass saw, so a frame where nothing changed costs nothing.
#[derive(Default)]
pub struct Seen {
    tick: Option<i32>,
    meshes: usize,
}

/// The click-box owner of the replacement for `key`: its own, so it can be dropped alone. Map
/// squares own theirs by square coordinates, which never reach this far below zero.
pub fn owner(key: &LocKey) -> (i32, i32) {
    (i32::MIN / 2 - key.x, key.z)
}

/// The tile under the middle of `change`'s object, which is where the picker knows it by.
fn middle_tile(change: &LocChange) -> (i32, i32) {
    let loc = change.loc;
    (loc.tile.x + loc.size.0 / 2, loc.tile.z + loc.size.1 / 2)
}

/// The change among `changes` (by original object id) whose original `placed` is part of.
fn change_of<'a>(changes: &'a HashMap<u32, Vec<LocChange>>, placed: &PlacedLoc) -> Option<&'a LocChange> {
    let tile = TileCoord::new(placed.tile.0, placed.tile.1);
    changes.get(&placed.id)?.iter().find(|change| change.loc.covers(tile))
}

/// Brings the scene in line with the server's scenery changes. It does the work only when there
/// is something new: a tick's snapshot, a square's objects spawned, or a replacement mesh back.
#[allow(clippy::too_many_arguments)]
pub fn show_scenery_changes(
    server: NonSend<ServerLink>,
    stream: NonSend<SquareStream>,
    mut swaps: ResMut<SwapMeshes>,
    mut shown: Local<BTreeMap<LocKey, Shown>>,
    mut seen: Local<Seen>,
    mut placed: Query<(&PlacedLoc, &mut Visibility), Without<SwapPart>>,
    spawned: Query<(), Added<PlacedLoc>>,
    alive: Query<(), With<SwapPart>>,
    mut spawner: SquareSpawner,
) {
    let Some(snapshot) = &server.latest else { return };
    let fresh = seen.tick != Some(snapshot.tick) || seen.meshes != swaps.ready.len() || !spawned.is_empty();
    if !fresh {
        return;
    }
    *seen = Seen { tick: Some(snapshot.tick), meshes: swaps.ready.len() };
    if snapshot.scenery.is_empty() && shown.is_empty() {
        return;
    }
    let wanted: BTreeMap<LocKey, LocChange> = snapshot.scenery.iter().map(|change| (change.loc.key(), *change)).collect();

    // changes that are over: the replacement goes and the original comes back
    let mut over: HashMap<u32, Vec<LocChange>> = HashMap::new();
    for key in shown.keys().filter(|key| !wanted.contains_key(key)).copied().collect::<Vec<_>>() {
        let Some(gone) = shown.remove(&key) else { continue };
        for entity in gone.entities.into_iter().filter(|e| alive.contains(*e)) {
            spawner.commands.entity(entity).despawn();
        }
        spawner.picker().remove(owner(&key));
        spawner.picker().show(middle_tile(&gone.change), key.id);
        over.entry(key.id).or_default().push(gone.change);
    }

    // one pass over the placed objects: show the originals that are back, hide the replaced
    // ones (again, if their square has come back) and note which originals are on screen
    let mut replaced: HashMap<u32, Vec<LocChange>> = HashMap::new();
    for change in wanted.values() {
        replaced.entry(change.loc.id).or_default().push(*change);
    }
    let mut originals: HashSet<LocKey> = HashSet::new();
    for (place, mut visibility) in &mut placed {
        if change_of(&over, place).is_some() {
            *visibility = Visibility::Inherited;
        } else if let Some(change) = change_of(&replaced, place) {
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
            originals.insert(change.loc.key());
        }
    }

    for (key, change) in &wanted {
        spawner.picker().hide(middle_tile(change), change.loc.id);
        // the replacement: there already, unless its square was dropped meanwhile
        if let Some(current) = shown.get(key) {
            if current.entities.iter().all(|e| alive.contains(*e)) {
                continue;
            }
            shown.remove(key);
            spawner.picker().remove(owner(key));
        }
        // the original's square is not loaded: nothing to put in its place yet
        if !originals.contains(key) {
            continue;
        }
        let shown_as = change.replacement;
        // where it stands now (the original's spot for a stump, the next tile for an open door)
        let Some(origin) = spawner.world.object_origin(shown_as.tile.x, shown_as.tile.z, shown_as.size.0, shown_as.size.1) else {
            continue;
        };
        let mesh_key = (shown_as.id, shown_as.kind, shown_as.orientation);
        let Some(mesh) = swaps.ready.get(&mesh_key) else {
            if swaps.requested.insert(mesh_key) {
                stream.request_object(mesh_key.0, mesh_key.1, mesh_key.2);
            }
            continue;
        };
        let entities = match mesh {
            Some(mesh) => {
                let object_key = (mesh_key.0, mesh_key.1, mesh_key.2, 255, 0);
                let scene = Arc::new(ObjectScene::single(object_key, mesh.clone(), origin, shown_as.tile.plane as u8));
                // it leaves with the original's square
                let square = square_of(change.loc.tile.x, change.loc.tile.z);
                let entities = spawner.spawn_objects(square, owner(key), &scene);
                for entity in &entities {
                    spawner.commands.entity(*entity).insert(SwapPart);
                }
                entities
            }
            None => Vec::new(),
        };
        shown.insert(*key, Shown { change: *change, entities });
    }
}
