//! NPC spawns: who stands where when the open world starts.
//!
//! A cache built by the map editor carries them beside its game files, as `npc_spawns.json`; the
//! server places them at start (the installed game's cache has none, and the open world then has
//! its default man).

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

/// The spawns kept in the cache directory `dir`, if it has a spawns file.
pub fn read_npc_spawns(dir: &Path) -> Option<Vec<NpcSpawnPoint>> {
    let path = dir.join(NPC_SPAWNS_FILE);
    let text = std::fs::read_to_string(&path).ok()?;
    match serde_json::from_str(&text) {
        Ok(spawns) => Some(spawns),
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
        assert_eq!(read_npc_spawns(&dir), Some(spawns));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
