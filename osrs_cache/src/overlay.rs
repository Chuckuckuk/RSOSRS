//! Groups replaced in memory: a cache opened [`with_overlay`](crate::Cache::with_overlay) answers
//! reads of these groups from here instead of from its files.
//!
//! The map editor keeps its uncompiled edits in one, so every reader (terrain, placements,
//! collision, the minimap, object definitions, reference tables) sees an edited map square as a
//! compiled cache would show it, without anything being written to disk. Several caches may share
//! one overlay (it is behind an `Arc`, and changes are seen by all of them at once).

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

type Groups = HashMap<(u8, u32), Arc<Vec<u8>>>;

/// Raw containers by `(archive, group)`.
#[derive(Default)]
pub struct Overlay {
    groups: RwLock<Groups>,
}

impl Overlay {
    pub fn new() -> Arc<Overlay> {
        Arc::new(Overlay::default())
    }

    /// Replaces everything held with `containers`.
    pub fn set(&self, containers: impl IntoIterator<Item = ((u8, u32), Vec<u8>)>) {
        let map = containers.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
        *self.groups.write().unwrap() = map;
    }

    pub fn clear(&self) {
        self.groups.write().unwrap().clear();
    }

    /// The replacement container of `archive/group`, if there is one.
    pub fn get(&self, archive: u8, group: u32) -> Option<Arc<Vec<u8>>> {
        self.groups.read().unwrap().get(&(archive, group)).cloned()
    }

    /// Every `(archive, group)` replaced.
    pub fn keys(&self) -> Vec<(u8, u32)> {
        self.groups.read().unwrap().keys().copied().collect()
    }
}
