//! NPC spawns: who stands where when the open world starts.
//!
//! A cache built by the map editor carries them beside its game files, as `npc_spawns.json`; the
//! server places them at start (the installed game's cache has none, and the open world then has
//! its [`default_npc_spawns`]).

use super::{cache_npc_def, cache_npc_id, cache_npc_key, cache_npc_variant, DataFile, NpcDef, Registry};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The spawns file in a cache directory.
pub const NPC_SPAWNS_FILE: &str = "npc_spawns.json";

/// An NPC standing in the world when the server starts. `npc` is an id in `data/npcs.json`, or a
/// cache NPC as `cache:<id>` (see [`super::cache_npc_key`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcSpawnPoint {
    pub npc: String,
    /// World tile.
    pub x: i32,
    pub z: i32,
    #[serde(default)]
    pub plane: u8,
}

/// The open world's NPCs when nothing says otherwise: a man (level 2) a few tiles from where the
/// player enters, `entry`.
pub fn default_npc_spawns(entry: (i32, i32)) -> Vec<NpcSpawnPoint> {
    vec![NpcSpawnPoint { npc: "man".into(), x: entry.0 + 4, z: entry.1 - 3, plane: 0 }]
}

/// A JSON file, read and parsed; a byte-order mark at its start (as Windows editors may save) is
/// allowed.
pub fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> std::io::Result<T> {
    let text = std::fs::read_to_string(path)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// The spawns kept in the cache directory `dir`, if it has a spawns file.
pub fn read_npc_spawns(dir: &Path) -> Option<Vec<NpcSpawnPoint>> {
    let path = dir.join(NPC_SPAWNS_FILE);
    match read_json_file(&path) {
        Ok(spawns) => Some(spawns),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            eprintln!("[data] {} is unreadable: {e}", path.display());
            None
        }
    }
}

/// Writes `spawns` as the spawns file of the cache directory `dir`.
pub fn write_npc_spawns(dir: &Path, spawns: &[NpcSpawnPoint]) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(spawns).map_err(std::io::Error::other)?;
    std::fs::write(dir.join(NPC_SPAWNS_FILE), text)
}

/// What the game cache says of an NPC, for one nobody has written stats for.
pub struct CacheNpc {
    pub name: String,
    pub combat_level: Option<u16>,
    pub size: u8,
}

impl Registry {
    /// The id a spawn of cache NPC `id` is stored under: the curated monster's own id when `id` is
    /// the model it is drawn as, else `cache:<id>` (another model of a curated monster, or an NPC
    /// nobody has curated; [`resolve_spawns`] settles those when the server starts).
    pub fn spawn_key_for_cache_npc(&self, id: u32) -> String {
        match self.npc_for_cache_id(id) {
            Some(curated) if curated.appearance == Some(id) => curated.id.clone(),
            _ => cache_npc_key(id),
        }
    }
}

/// How [`resolve_spawns`] settled the cache NPCs.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResolvedSpawns {
    /// Kinds drawn as their own model with a curated monster's stats.
    pub variants: usize,
    /// Kinds made from the cache alone, which cannot be fought.
    pub plain: usize,
}

/// Makes every spawn of a cache NPC (`cache:<id>`) name a definition the registry has. When
/// `npcs.json` covers it (its model, or one of its `cache_ids`), it fights with those stats, drawn
/// as its own model, so stats added there reach spawns already placed. Otherwise it is made from
/// what `cache_npc` says of it, and cannot be fought; one the cache does not have is left for
/// spawning to report.
pub fn resolve_spawns(registry: &mut Registry, spawns: &mut [NpcSpawnPoint], mut cache_npc: impl FnMut(u32) -> Option<CacheNpc>) -> ResolvedSpawns {
    let mut made: Vec<NpcDef> = Vec::new();
    let mut resolved = ResolvedSpawns::default();
    for s in spawns.iter_mut() {
        let Some(id) = cache_npc_id(&s.npc) else { continue };
        s.npc = registry.spawn_key_for_cache_npc(id);
        if registry.npc(&s.npc).is_some() || made.iter().any(|d| d.id == s.npc) {
            continue;
        }
        if let Some(curated) = registry.npc_for_cache_id(id) {
            made.push(cache_npc_variant(curated, id));
            resolved.variants += 1;
        } else if let Some(npc) = cache_npc(id) {
            made.push(cache_npc_def(id, &npc.name, npc.combat_level, npc.size));
            resolved.plain += 1;
        }
    }
    if !made.is_empty() {
        if let Err(e) = registry.merge(DataFile { npcs: made, ..Default::default() }) {
            eprintln!("[data] cache NPCs: {e}");
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_round_trip_through_a_directory_and_a_missing_file_is_none() {
        let dir = std::env::temp_dir().join(format!("osrs_spawns_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(dir.join(NPC_SPAWNS_FILE));
        assert_eq!(read_npc_spawns(&dir), None);
        let spawns = vec![NpcSpawnPoint { npc: "cache:3105".into(), x: 3221, z: 3221, plane: 0 }];
        write_npc_spawns(&dir, &spawns).unwrap();
        assert_eq!(read_npc_spawns(&dir), Some(spawns.clone()));
        // a byte-order mark in front is allowed
        let text = std::fs::read_to_string(dir.join(NPC_SPAWNS_FILE)).unwrap();
        std::fs::write(dir.join(NPC_SPAWNS_FILE), format!("\u{feff}{text}")).unwrap();
        assert_eq!(read_npc_spawns(&dir), Some(spawns));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_npcs_resolve_to_the_curated_monster_its_variants_or_the_cache_alone() {
        let mut registry = Registry::builtin();
        let at = |npc: &str| NpcSpawnPoint { npc: npc.into(), x: 0, z: 0, plane: 0 };
        let mut spawns = vec![at("cache:415"), at("cache:7241"), at("cache:3105"), at("cache:3105"), at("man"), at("cache:999999")];
        let hans = |id: u32| (id == 3105).then(|| CacheNpc { name: "Hans".into(), combat_level: None, size: 1 });
        let resolved = resolve_spawns(&mut registry, &mut spawns, hans);
        assert_eq!(resolved, ResolvedSpawns { variants: 1, plain: 1 });
        let ids: Vec<&str> = spawns.iter().map(|s| s.npc.as_str()).collect();
        assert_eq!(ids, ["abyssal_demon", "cache:7241", "cache:3105", "cache:3105", "man", "cache:999999"]);
        let variant = registry.npc("cache:7241").unwrap();
        assert_eq!((variant.appearance, variant.hitpoints), (Some(7241), registry.npc("abyssal_demon").unwrap().hitpoints));
        assert_eq!(registry.npc("cache:3105").unwrap().name, "Hans");
        assert!(registry.npc("cache:999999").is_none(), "unknown to the cache: left for spawning to report");
    }
}
