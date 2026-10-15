//! Groups replaced in memory, for the whole process: the map editor's uncompiled edits.
//!
//! [`Cache::read_raw`](crate::Cache::read_raw) answers from here first, so every reader (terrain,
//! placements, collision, the minimap, object definitions, reference tables) sees an edited map
//! square exactly as a compiled cache would show it, without anything being written to disk. The
//! map editor fills it from [`crate::compile::build`], the same containers a compile writes.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

type Groups = HashMap<(u8, u32), Arc<Vec<u8>>>;

static OVERLAY: OnceLock<RwLock<Groups>> = OnceLock::new();

fn groups() -> &'static RwLock<Groups> {
    OVERLAY.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Replaces the whole overlay with `containers` (raw containers by `(archive, group)`).
pub fn set(containers: impl IntoIterator<Item = ((u8, u32), Vec<u8>)>) {
    let map = containers.into_iter().map(|(k, v)| (k, Arc::new(v))).collect();
    *groups().write().unwrap() = map;
}

/// Empties the overlay: every read goes to the cache files again.
pub fn clear() {
    if let Some(lock) = OVERLAY.get() {
        lock.write().unwrap().clear();
    }
}

/// The replacement container of `archive/group`, if there is one.
pub fn get(archive: u8, group: u32) -> Option<Arc<Vec<u8>>> {
    OVERLAY.get()?.read().unwrap().get(&(archive, group)).cloned()
}

/// Every `(archive, group)` replaced.
pub fn keys() -> Vec<(u8, u32)> {
    OVERLAY.get().map(|l| l.read().unwrap().keys().copied().collect()).unwrap_or_default()
}
