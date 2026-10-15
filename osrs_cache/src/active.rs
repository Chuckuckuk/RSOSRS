//! Which cache the game reads: the installed game's ("live"), or one the map editor compiled.
//!
//! The editor never writes to the installed cache. It builds a full copy with the edits applied
//! under `~/.rsosrs/caches/<name>/` ([`crate::compile`]), and the game reads whichever cache is
//! *active*:
//!
//! 1. `OSRS_CACHE_DIR`, if set: a cache directory, or `live`;
//! 2. else `~/.rsosrs/active_cache`, a text file holding a directory or `live` (the editor's
//!    Caches menu writes it);
//! 3. else the live cache.
//!
//! `RSOSRS_HOME` moves `~/.rsosrs` elsewhere (tests use it).

use std::path::{Path, PathBuf};

/// Environment variable naming the cache directory to read (or `live`).
pub const ENV_CACHE_DIR: &str = "OSRS_CACHE_DIR";
/// Environment variable replacing `~/.rsosrs`.
pub const ENV_HOME: &str = "RSOSRS_HOME";
/// What the selection file or variable holds to mean the installed game's cache.
pub const LIVE: &str = "live";
/// The file a compiled cache carries to say where it came from ([`crate::compile::Manifest`]).
pub const MANIFEST: &str = "rsosrs_cache.json";
/// The NPC spawns a compiled cache carries ([`crate::project::NpcSpawn`]).
pub const NPC_SPAWNS: &str = "npc_spawns.json";

/// A choice of cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Live,
    Dir(PathBuf),
}

impl Selection {
    fn parse(s: &str) -> Option<Selection> {
        let s = s.trim();
        if s.is_empty() {
            None
        } else if s.eq_ignore_ascii_case(LIVE) {
            Some(Selection::Live)
        } else {
            Some(Selection::Dir(PathBuf::from(s)))
        }
    }
}

fn user_home() -> PathBuf {
    PathBuf::from(std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).unwrap_or_default())
}

/// `~/.rsosrs`, where compiled caches, editor projects and the active selection live.
pub fn rsosrs_dir() -> PathBuf {
    match std::env::var(ENV_HOME) {
        Ok(d) if !d.is_empty() => PathBuf::from(d),
        _ => user_home().join(".rsosrs"),
    }
}

pub fn caches_dir() -> PathBuf {
    rsosrs_dir().join("caches")
}

pub fn projects_dir() -> PathBuf {
    rsosrs_dir().join("projects")
}

fn selection_file() -> PathBuf {
    rsosrs_dir().join("active_cache")
}

/// The directory a compiled cache called `name` lives in.
pub fn cache_dir_of(name: &str) -> PathBuf {
    caches_dir().join(name)
}

/// Whether `dir` holds a cache (its data file).
pub fn is_cache_dir(dir: &Path) -> bool {
    dir.join("main_file_cache.dat2").exists()
}

/// The installed game's cache: `~/.runelite/jagexcache/oldschool/LIVE`, or `~/jagexcache/...`.
pub fn live_dir() -> Option<PathBuf> {
    let home = user_home();
    [".runelite/jagexcache/oldschool/LIVE", "jagexcache/oldschool/LIVE"]
        .into_iter()
        .map(|rel| home.join(rel))
        .find(|p| is_cache_dir(p))
}

/// The selection in force, before checking that its directory exists.
pub fn selection() -> Selection {
    resolve(std::env::var(ENV_CACHE_DIR).ok().as_deref(), std::fs::read_to_string(selection_file()).ok().as_deref())
}

/// The variable wins over the file; neither (or an empty one) means live.
fn resolve(env: Option<&str>, file: Option<&str>) -> Selection {
    env.and_then(Selection::parse).or_else(|| file.and_then(Selection::parse)).unwrap_or(Selection::Live)
}

/// The directory of the active cache. A selected directory that holds no cache falls back to the
/// live one, with a warning, so a deleted build never leaves the game without a world.
pub fn active_dir() -> Option<PathBuf> {
    match selection() {
        Selection::Live => live_dir(),
        Selection::Dir(dir) if is_cache_dir(&dir) => Some(dir),
        Selection::Dir(dir) => {
            eprintln!("[cache] the selected cache {} has no data file; reading the live cache", dir.display());
            live_dir()
        }
    }
}

/// Makes `selection` the cache the game reads from now on (`OSRS_CACHE_DIR` still overrides it).
pub fn set_active(selection: &Selection) -> std::io::Result<()> {
    std::fs::create_dir_all(rsosrs_dir())?;
    let text = match selection {
        Selection::Live => LIVE.to_string(),
        Selection::Dir(dir) => dir.display().to_string(),
    };
    std::fs::write(selection_file(), text)
}

/// One compiled cache under [`caches_dir`].
#[derive(Debug, Clone)]
pub struct CacheInfo {
    pub name: String,
    pub dir: PathBuf,
    pub manifest: Option<crate::compile::Manifest>,
}

/// Every compiled cache, by name.
pub fn list_caches() -> Vec<CacheInfo> {
    let Ok(entries) = std::fs::read_dir(caches_dir()) else { return Vec::new() };
    let mut out: Vec<CacheInfo> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_cache_dir(p))
        .map(|dir| CacheInfo {
            name: dir.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            manifest: std::fs::read_to_string(dir.join(MANIFEST)).ok().and_then(|s| serde_json::from_str(&s).ok()),
            dir,
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The NPC spawns carried by the cache in `dir`, if it has any (a compiled cache does; the live
/// one does not).
pub fn npc_spawns(dir: &Path) -> Option<Vec<crate::project::NpcSpawn>> {
    let text = std::fs::read_to_string(dir.join(NPC_SPAWNS)).ok()?;
    match serde_json::from_str(&text) {
        Ok(spawns) => Some(spawns),
        Err(e) => {
            eprintln!("[cache] {} is unreadable: {e}", dir.join(NPC_SPAWNS).display());
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_variable_wins_over_the_file_and_nothing_means_live() {
        let dir = |s: &str| Selection::Dir(PathBuf::from(s));
        assert_eq!(resolve(None, None), Selection::Live);
        assert_eq!(resolve(None, Some("C:/caches/a\n")), dir("C:/caches/a"));
        assert_eq!(resolve(Some("C:/caches/b"), Some("C:/caches/a")), dir("C:/caches/b"));
        assert_eq!(resolve(Some("LIVE"), Some("C:/caches/a")), Selection::Live);
        assert_eq!(resolve(Some(""), Some("live")), Selection::Live);
        assert_eq!(resolve(Some("  "), Some("C:/caches/a")), dir("C:/caches/a"));
    }
}
