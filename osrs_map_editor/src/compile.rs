//! Turning a [`MapProject`] into cache data, with `osrs_cache`'s encoders and writer.
//!
//! A [`Builder`] works out every group the project changes, as finished containers:
//! * each edited map square (archive 5): its placements file re-encoded with the edits applied,
//!   its terrain file untouched;
//! * the object definitions group (config archive 2, group 6), when the project mints marker
//!   objects for minimap icons;
//! * the reference table of each archive touched (archive 255), with the new CRCs and revisions,
//!   and new file ids for minted objects.
//!
//! The builder keeps what it reads from the base cache and what it last built, so the editor can
//! [`sync`](Builder::sync) it after every edit and only the squares that changed are encoded again.
//! The editor shows the containers through a cache [`Overlay`](osrs_cache::overlay::Overlay);
//! [`compile`] writes them into a copy of the base cache, beside the project's NPC spawns and a
//! [`Manifest`]. The base cache is only read.

use crate::builds::{Manifest, MANIFEST};
use crate::project::{parse_square_key, MapProject, MarkerDef, SquareEdits};
use osrs_cache::cache::join_files;
use osrs_cache::config::ObjectDef;
use osrs_cache::container::{self, Compression};
use osrs_cache::locs::{self, Placement};
use osrs_cache::reftable::RefTable;
use osrs_cache::squares::map_group;
use osrs_cache::writer::CacheWriter;
use osrs_cache::{cfg, idx, Cache, Error, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Containers by `(archive, group)`.
pub type Containers = BTreeMap<(u8, u32), Vec<u8>>;

/// The reference-table archive.
const META: u8 = 255;

fn fail<T>(why: impl Into<String>) -> Result<T> {
    Err(Error::Format(why.into()))
}

/// A container and the length of the data packed in it (a reference table records both).
struct Built {
    packed: Vec<u8>,
    data_len: usize,
}

/// Builds a project's groups on one base cache (read as stored, without an overlay).
pub struct Builder {
    base: Cache,
    compression: Compression,
    /// The base's reference tables (the maps one with its index too), read once.
    maps: Option<(RefTable, osrs_cache::Index)>,
    config: Option<RefTable>,
    /// Each map square's files as the base has them, by group.
    square_files: HashMap<u32, Vec<(u32, Vec<u8>)>>,
    /// The base's object definitions group, read the first time a marker is minted.
    object_files: Option<BTreeMap<u32, Vec<u8>>>,
    /// What was last built: each square's edits and container, and the markers and their group.
    squares: HashMap<u32, (SquareEdits, Built)>,
    markers: Vec<MarkerDef>,
    objects: Option<Built>,
}

impl Builder {
    pub fn new(base: Cache, compression: Compression) -> Builder {
        Builder {
            base,
            compression,
            maps: None,
            config: None,
            square_files: HashMap::new(),
            object_files: None,
            squares: HashMap::new(),
            markers: Vec::new(),
            objects: None,
        }
    }

    /// The cache it builds on.
    pub fn base(&self) -> &Cache {
        &self.base
    }

    fn maps(&mut self) -> Result<&(RefTable, osrs_cache::Index)> {
        if self.maps.is_none() {
            let table = RefTable::decode(&self.base.read_group(META, idx::MAPS as u32)?)?;
            let index = table.to_index(idx::MAPS);
            self.maps = Some((table, index));
        }
        Ok(self.maps.as_ref().unwrap())
    }

    /// Whether the base has map square (`sx`, `sy`) (only those can be edited).
    pub fn square_exists(&mut self, sx: i32, sy: i32) -> bool {
        self.maps().is_ok_and(|(_, index)| index.groups.contains_key(&map_group(sx, sy)))
    }

    fn square_files(&mut self, sx: i32, sy: i32) -> Result<&[(u32, Vec<u8>)]> {
        let gid = map_group(sx, sy);
        if !self.square_files.contains_key(&gid) {
            if !self.square_exists(sx, sy) {
                return fail(format!("map square {sx}_{sy} is not in the cache; only existing squares can be edited"));
            }
            self.maps()?;
            let (_, index) = self.maps.as_ref().unwrap();
            let files = self.base.read_files(index, gid)?;
            if files.len() < 2 {
                return fail(format!("map square {sx}_{sy} has no placements file"));
            }
            self.square_files.insert(gid, files);
        }
        Ok(&self.square_files[&gid])
    }

    /// The placements of map square (`sx`, `sy`) as the base has them (none for a square it does
    /// not have).
    pub fn base_placements(&mut self, sx: i32, sy: i32) -> Vec<Placement> {
        self.square_files(sx, sy).map(|files| locs::decode(&files[1].1)).unwrap_or_default()
    }

    /// Brings what is built up to date with `project`, encoding only the squares whose edits
    /// changed since the last call (and the object definitions only when its markers did).
    /// Returns those squares.
    pub fn sync(&mut self, project: &MapProject) -> Result<Vec<(i32, i32)>> {
        let mut wanted: HashMap<u32, (i32, i32, &SquareEdits)> = HashMap::new();
        for (key, edits) in project.squares.iter().filter(|(_, e)| !e.is_empty()) {
            let Some((sx, sy)) = parse_square_key(key) else {
                return fail(format!("bad square key {key:?} in the project"));
            };
            wanted.insert(map_group(sx, sy), (sx, sy, edits));
        }
        let mut changed = Vec::new();
        // squares whose edits were undone completely go back to the base
        self.squares.retain(|gid, _| {
            let keep = wanted.contains_key(gid);
            if !keep {
                changed.push(((*gid >> 8) as i32, (*gid & 0xFF) as i32));
            }
            keep
        });
        for (gid, (sx, sy, edits)) in wanted {
            if self.squares.get(&gid).is_some_and(|(built, _)| built == edits) {
                continue;
            }
            let compression = self.compression;
            let mut files = self.square_files(sx, sy)?.to_vec();
            files[1].1 = locs::encode(&edits.apply(&locs::decode(&files[1].1)));
            let data = join_files(&files.iter().map(|(_, d)| d.as_slice()).collect::<Vec<_>>());
            let built = Built { packed: container::encode(&data, compression), data_len: data.len() };
            self.squares.insert(gid, (edits.clone(), built));
            changed.push((sx, sy));
        }
        if project.marker_defs != self.markers {
            self.objects = if project.marker_defs.is_empty() { None } else { Some(self.build_objects(&project.marker_defs)?) };
            self.markers = project.marker_defs.clone();
        }
        Ok(changed)
    }

    /// The object definitions group with `markers` minted into it.
    fn build_objects(&mut self, markers: &[MarkerDef]) -> Result<Built> {
        if self.config.is_none() {
            self.config = Some(RefTable::decode(&self.base.read_group(META, idx::CONFIG as u32)?)?);
        }
        if self.object_files.is_none() {
            let index = self.config.as_ref().unwrap().to_index(idx::CONFIG);
            self.object_files = Some(self.base.read_files(&index, cfg::OBJECT)?.into_iter().collect());
        }
        let mut files = self.object_files.clone().unwrap();
        for m in markers {
            files.insert(m.id, m.encode());
        }
        let data = join_files(&files.values().map(Vec::as_slice).collect::<Vec<_>>());
        Ok(Built { packed: container::encode(&data, self.compression), data_len: data.len() })
    }

    /// Every container built, with the reference tables of the archives they belong to.
    pub fn containers(&mut self) -> Result<Containers> {
        let mut out = Containers::new();
        if !self.squares.is_empty() {
            let mut maps = self.maps()?.0.clone();
            for (&gid, (_, built)) in &self.squares {
                maps.touch(gid, &built.packed, built.data_len);
                out.insert((idx::MAPS, gid), built.packed.clone());
            }
            maps.revision = maps.revision.wrapping_add(1);
            out.insert((META, idx::MAPS as u32), container::encode(&maps.encode(), self.compression));
        }
        if let Some(built) = &self.objects {
            let mut config = self.config.clone().expect("read when the objects were built");
            let Some(group) = config.group_mut(cfg::OBJECT) else {
                return fail("the config archive has no object group");
            };
            // keep the name hashes of the files already listed; minted ones have none
            let names: HashMap<u32, i32> = group.files.iter().copied().collect();
            let mut ids: Vec<u32> = names.keys().copied().chain(self.markers.iter().map(|m| m.id)).collect();
            ids.sort_unstable();
            ids.dedup();
            group.files = ids.into_iter().map(|id| (id, names.get(&id).copied().unwrap_or(0))).collect();
            config.touch(cfg::OBJECT, &built.packed, built.data_len);
            config.revision = config.revision.wrapping_add(1);
            out.insert((idx::CONFIG, cfg::OBJECT), built.packed.clone());
            out.insert((META, idx::CONFIG as u32), container::encode(&config.encode(), self.compression));
        }
        Ok(out)
    }
}

/// The containers `project` changes, built on `base` (read as stored, without an overlay), packed
/// as `compression` says: the editor's preview skips compressing, which is most of the work.
pub fn build_with(project: &MapProject, base: Cache, compression: Compression) -> Result<Containers> {
    let mut builder = Builder::new(base, compression);
    builder.sync(project)?;
    builder.containers()
}

/// An object in `defs` that already is a bare marker for `area`: no model, blocks nothing,
/// carries that map icon. The editor places one of these when it can rather than minting one.
pub fn existing_marker(defs: &HashMap<u32, ObjectDef>, area: u16) -> Option<u32> {
    defs.values()
        .filter(|d| d.map_icon == Some(area) && d.models.is_empty() && d.interact_type == 0 && d.actions.iter().all(|a| a.is_empty()))
        .map(|d| d.id)
        .min()
}

/// The first object id after every definition in `defs` and every marker `project` minted.
pub fn next_object_id(defs: &HashMap<u32, ObjectDef>, project: &MapProject) -> u32 {
    let defs_max = defs.keys().copied().max();
    let minted_max = project.marker_defs.iter().map(|m| m.id).max();
    defs_max.max(minted_max).map_or(0, |m| m + 1)
}

/// Progress of a [`compile`], for a progress bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Building,
    Copying,
    Writing,
    Done,
}

/// Compiles `project` on the cache in `base_dir` into a new cache in `out_dir`, named after the
/// directory. The base is only read. `out_dir` may be an earlier compile (it has a manifest),
/// which is replaced once the new one is complete; any other existing directory is refused, as
/// are the base itself and the live cache.
pub fn compile(project: &MapProject, base_dir: &Path, out_dir: &Path, mut progress: impl FnMut(Stage)) -> Result<Manifest> {
    let same = |a: &Path, b: &Path| match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    if same(base_dir, out_dir) {
        return fail("a cache cannot be compiled over its own base");
    }
    if osrs_cache::active::live_dir().is_some_and(|live| same(&live, out_dir)) {
        return fail("the live game cache is never written to");
    }
    if out_dir.exists() && !out_dir.join(MANIFEST).exists() {
        return fail(format!("{} exists and is not a compiled cache; choose another name", out_dir.display()));
    }
    let Some(name) = out_dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
        return fail("the output needs a directory name");
    };

    progress(Stage::Building);
    let containers = build_with(project, Cache::open(base_dir)?, Compression::Gzip)?;

    // build beside the destination, then swap it in, so a failed compile leaves the old one whole
    let staging = out_dir.with_file_name(format!("{name}.building"));
    if staging.exists() {
        std::fs::remove_dir_all(&staging)?;
    }
    progress(Stage::Copying);
    let mut writer = CacheWriter::create(base_dir, &staging)?;
    progress(Stage::Writing);
    for (&(archive, group), packed) in &containers {
        writer.put(archive, group, packed)?;
    }
    writer.finish()?;

    let manifest = Manifest {
        name: name.clone(),
        project: project.name.clone(),
        base: base_dir.to_path_buf(),
        compiled_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        squares: project.edited_squares().len(),
        npc_spawns: project.npc_spawns.len(),
        marker_defs: project.marker_defs.len(),
        fingerprint: Some(project.fingerprint()),
    };
    osrs_core::data::write_npc_spawns(&staging, &project.npc_spawns)?;
    manifest.write(&staging)?;

    if out_dir.exists() {
        std::fs::remove_dir_all(out_dir)
            .map_err(|e| Error::Format(format!("could not replace {} (is the game running on it?): {e}", out_dir.display())))?;
    }
    std::fs::rename(&staging, out_dir)?;
    progress(Stage::Done);
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syncing_encodes_only_the_squares_whose_edits_changed() {
        let Ok(base) = Cache::open_live() else { return };
        let mut builder = Builder::new(base, Compression::None);
        let tree = |x| Placement { id: 1276, plane: 0, x, z: 1, kind: 10, orientation: 0 };
        let mut project = MapProject::new("t");
        project.square_mut(50, 50).added.push(tree(1));
        project.square_mut(50, 51).added.push(tree(2));
        let mut first = builder.sync(&project).unwrap();
        first.sort();
        assert_eq!(first, [(50, 50), (50, 51)]);
        assert!(builder.sync(&project).unwrap().is_empty(), "nothing changed");

        project.square_mut(50, 51).added.push(tree(3));
        assert_eq!(builder.sync(&project).unwrap(), [(50, 51)]);
        // undoing a square's last edit takes its container out
        project.square_mut(50, 50).added.clear();
        assert_eq!(builder.sync(&project).unwrap(), [(50, 50)]);
        let containers = builder.containers().unwrap();
        assert!(!containers.contains_key(&(idx::MAPS, map_group(50, 50))));
        assert!(containers.contains_key(&(idx::MAPS, map_group(50, 51))) && containers.contains_key(&(META, idx::MAPS as u32)));
    }
}
