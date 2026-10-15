//! Where the editor keeps things, under `~/.rsosrs` (`osrs_cache::active::rsosrs_dir`): projects
//! in `projects/`, the caches it builds in `caches/<name>/`, each with a manifest saying what it
//! was built from.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file a built cache carries to say where it came from.
pub const MANIFEST: &str = "rsosrs_cache.json";

pub fn caches_dir() -> PathBuf {
    osrs_cache::active::rsosrs_dir().join("caches")
}

pub fn projects_dir() -> PathBuf {
    osrs_cache::active::rsosrs_dir().join("projects")
}

/// The directory a built cache called `name` lives in.
pub fn cache_dir_of(name: &str) -> PathBuf {
    caches_dir().join(name)
}

/// What a built cache records about itself (`rsosrs_cache.json`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub name: String,
    pub project: String,
    /// The cache it was built on.
    pub base: PathBuf,
    /// Seconds since the Unix epoch.
    pub compiled_at: u64,
    pub squares: usize,
    pub npc_spawns: usize,
    pub marker_defs: usize,
    /// [`crate::project::MapProject::fingerprint`] of the project as built.
    #[serde(default)]
    pub fingerprint: Option<u64>,
}

impl Manifest {
    /// The manifest of the built cache in `dir`, if it has one.
    pub fn read(dir: &Path) -> Option<Manifest> {
        serde_json::from_str(&std::fs::read_to_string(dir.join(MANIFEST)).ok()?).ok()
    }

    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(MANIFEST), text)
    }
}

/// One built cache under [`caches_dir`].
#[derive(Debug, Clone)]
pub struct CacheInfo {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Option<Manifest>,
}

/// Every built cache, by name.
pub fn list_caches() -> Vec<CacheInfo> {
    let Ok(entries) = std::fs::read_dir(caches_dir()) else { return Vec::new() };
    let mut out: Vec<CacheInfo> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| osrs_cache::active::is_cache_dir(p))
        .map(|dir| CacheInfo { name: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(), manifest: Manifest::read(&dir), dir })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}
