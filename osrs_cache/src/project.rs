//! A map editor project: the edits made to the world, kept apart from any cache.
//!
//! A project is the source of truth; caches are built from it ([`crate::compile`]). Keeping edits
//! as edits (rather than as an edited copy of the cache) means a project can be compiled again on
//! top of a newer live cache after the game updates.
//!
//! Saved as JSON under `~/.rsosrs/projects/<name>.json`.

use crate::locs::Placement;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MapProject {
    pub name: String,
    /// Edits by map square, keyed `"sx_sy"` (JSON keys must be strings).
    #[serde(default)]
    pub squares: BTreeMap<String, SquareEdits>,
    #[serde(default)]
    pub npc_spawns: Vec<NpcSpawn>,
    /// Invisible objects minted to carry a minimap icon where the cache has none for that icon.
    #[serde(default)]
    pub marker_defs: Vec<MarkerDef>,
}

/// One map square's changes to its placements. Positions are square-local, as the map file
/// stores them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SquareEdits {
    /// Placements the cache has that are taken out.
    #[serde(default)]
    pub removed: Vec<Placement>,
    /// Placements put in.
    #[serde(default)]
    pub added: Vec<Placement>,
}

impl SquareEdits {
    pub fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty()
    }

    /// `base` (a square's placements as the cache has them) with these edits applied: each removed
    /// placement takes out one equal placement, then the added ones go in.
    pub fn apply(&self, base: &[Placement]) -> Vec<Placement> {
        let mut out = base.to_vec();
        for r in &self.removed {
            if let Some(i) = out.iter().position(|p| p == r) {
                out.remove(i);
            }
        }
        out.extend_from_slice(&self.added);
        out
    }
}

/// An NPC standing in the world when the server starts. `npc` is an id in `data/npcs.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NpcSpawn {
    pub npc: String,
    /// World tile.
    pub x: i32,
    pub z: i32,
    #[serde(default)]
    pub plane: u8,
}

/// An object definition the compile adds: no model, does not block, carries map icon `map_icon`
/// (an area id, config group 35).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarkerDef {
    pub id: u32,
    pub map_icon: u16,
}

impl MarkerDef {
    /// The definition's bytes: name (opcode 2), no clipping (17), not impenetrable (18), map icon
    /// (82), end (0).
    pub fn encode(&self) -> Vec<u8> {
        let mut out = vec![2];
        out.extend_from_slice(b"Map icon\0");
        out.extend_from_slice(&[17, 18, 82]);
        out.extend_from_slice(&self.map_icon.to_be_bytes());
        out.push(0);
        out
    }
}

/// `"sx_sy"`, the key of map square (`sx`, `sy`) in [`MapProject::squares`].
pub fn square_key(sx: i32, sy: i32) -> String {
    format!("{sx}_{sy}")
}

/// The map square a [`square_key`] names.
pub fn parse_square_key(key: &str) -> Option<(i32, i32)> {
    let (a, b) = key.split_once('_')?;
    Some((a.parse().ok()?, b.parse().ok()?))
}

impl MapProject {
    pub fn new(name: &str) -> MapProject {
        MapProject { name: name.to_string(), ..Default::default() }
    }

    /// `~/.rsosrs/projects/<name>.json`.
    pub fn path_of(name: &str) -> PathBuf {
        crate::active::projects_dir().join(format!("{name}.json"))
    }

    pub fn load(path: &Path) -> std::io::Result<MapProject> {
        let text = std::fs::read_to_string(path)?;
        serde_json::from_str(&text).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        // write beside, then rename, so a crash never leaves half a project
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, path)
    }

    /// The edits of square (`sx`, `sy`), if any.
    pub fn square(&self, sx: i32, sy: i32) -> Option<&SquareEdits> {
        self.squares.get(&square_key(sx, sy))
    }

    pub fn square_mut(&mut self, sx: i32, sy: i32) -> &mut SquareEdits {
        self.squares.entry(square_key(sx, sy)).or_default()
    }

    /// Every edited square, as coordinates.
    pub fn edited_squares(&self) -> Vec<(i32, i32)> {
        self.squares.iter().filter(|(_, e)| !e.is_empty()).filter_map(|(k, _)| parse_square_key(k)).collect()
    }

    /// Whether the project changes nothing.
    pub fn is_empty(&self) -> bool {
        self.squares.values().all(SquareEdits::is_empty) && self.npc_spawns.is_empty() && self.marker_defs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ObjectDef;

    #[test]
    fn a_marker_def_is_an_invisible_non_blocking_icon() {
        let d = MarkerDef { id: 60_000, map_icon: 13 }.encode();
        let (def, clean) = ObjectDef::decode_with(60_000, &d, &[]);
        assert!(clean, "consumed exactly");
        assert_eq!(def.map_icon, Some(13));
        assert!(def.models.is_empty());
        assert_eq!(def.interact_type, 0);
        assert!(!def.solid);
    }

    #[test]
    fn removing_takes_out_one_equal_placement_and_adding_appends() {
        let p = |id, x| Placement { id, plane: 0, x, z: 1, kind: 10, orientation: 0 };
        let edits = SquareEdits { removed: vec![p(5, 1), p(9, 9)], added: vec![p(7, 3)] };
        assert_eq!(edits.apply(&[p(5, 1), p(5, 1), p(6, 2)]), vec![p(5, 1), p(6, 2), p(7, 3)]);
    }

    #[test]
    fn square_keys_round_trip() {
        assert_eq!(parse_square_key(&square_key(50, 50)), Some((50, 50)));
        assert_eq!(parse_square_key("nope"), None);
    }
}
