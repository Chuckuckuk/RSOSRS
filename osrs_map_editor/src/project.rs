//! A map editor project: the edits made to the world, kept apart from any cache.
//!
//! A project is the source of truth; caches are built from it ([`crate::compile`]). Keeping edits
//! as edits (rather than as an edited copy of the cache) means a project can be compiled again on
//! top of a newer live cache after the game updates.
//!
//! Saved as JSON under `~/.rsosrs/projects/<name>.json`.

use crate::builds::{self, Manifest};
use osrs_cache::locs::Placement;
use osrs_core::data::NpcSpawnPoint;
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
    pub npc_spawns: Vec<NpcSpawnPoint>,
    /// Invisible objects minted to carry a minimap icon where the cache has none for that icon.
    #[serde(default)]
    pub marker_defs: Vec<MarkerDef>,
    /// The compiled cache this project plays on (a name under `~/.rsosrs/caches`), once one has
    /// been made for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<String>,
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

/// An object definition the compile adds: no model, does not block, carries map icon `map_icon`
/// (an area id, config group 35).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarkerDef {
    pub id: u32,
    pub map_icon: u16,
}

impl MarkerDef {
    /// The definition's bytes (see `osrs_cache::config::encode_icon_marker`).
    pub fn encode(&self) -> Vec<u8> {
        osrs_cache::config::encode_icon_marker("Map icon", self.map_icon)
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
        builds::projects_dir().join(format!("{name}.json"))
    }

    pub fn load(path: &Path) -> std::io::Result<MapProject> {
        osrs_core::data::read_json_file(path)
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

    /// The directory of the compiled cache this project plays on, if it has one that still
    /// exists: the one it names, else (for a project from before it named one) a compiled cache
    /// whose manifest says it was built from a project of this name.
    pub fn compiled_cache(&self) -> Option<PathBuf> {
        if let Some(name) = &self.cache {
            let dir = builds::cache_dir_of(name);
            if osrs_cache::active::is_cache_dir(&dir) {
                return Some(dir);
            }
        }
        builds::list_caches()
            .into_iter()
            .find(|c| c.manifest.as_ref().is_some_and(|m| m.project == self.name))
            .map(|c| c.dir)
    }

    /// A fingerprint of everything a compile writes from this project (its square edits, NPC
    /// spawns and minted markers; not its name or cache): a compiled cache records the one it was
    /// built from, so it can tell when the project has changed since. FNV-1a over the JSON, so it
    /// is the same from run to run.
    pub fn fingerprint(&self) -> u64 {
        let json = serde_json::to_vec(&(&self.squares, &self.npc_spawns, &self.marker_defs)).expect("plain data serialises");
        json.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
    }

    /// Whether the compiled cache in `dir` was built from the project as it is now. A cache whose
    /// manifest has no fingerprint (built before they were kept) counts as out of date.
    pub fn is_compiled_into(&self, dir: &Path) -> bool {
        Manifest::read(dir).and_then(|m| m.fingerprint) == Some(self.fingerprint())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_takes_out_one_equal_placement_and_adding_appends() {
        let p = |id, x| Placement { id, plane: 0, x, z: 1, kind: 10, orientation: 0 };
        let edits = SquareEdits { removed: vec![p(5, 1), p(9, 9)], added: vec![p(7, 3)] };
        assert_eq!(edits.apply(&[p(5, 1), p(5, 1), p(6, 2)]), vec![p(5, 1), p(6, 2), p(7, 3)]);
    }

    #[test]
    fn the_fingerprint_follows_the_edits_and_not_the_name_or_cache() {
        let mut a = MapProject::new("a");
        let blank = a.fingerprint();
        a.cache = Some("elsewhere".into());
        a.name = "renamed".into();
        assert_eq!(a.fingerprint(), blank);
        a.npc_spawns.push(NpcSpawnPoint { npc: "man".into(), x: 1, z: 2, plane: 0 });
        assert_ne!(a.fingerprint(), blank);
        assert_eq!(a.fingerprint(), a.clone().fingerprint());
    }

    #[test]
    fn square_keys_round_trip() {
        assert_eq!(parse_square_key(&square_key(50, 50)), Some((50, 50)));
        assert_eq!(parse_square_key("nope"), None);
    }
}
