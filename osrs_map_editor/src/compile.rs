//! Turning a [`MapProject`] into cache data, with `osrs_cache`'s encoders and writer.
//!
//! [`build`] works out every group the project changes, as finished containers:
//! * each edited map square (archive 5): its placements file re-encoded with the edits applied,
//!   its terrain file untouched;
//! * the object definitions group (config archive 2, group 6), when the project mints marker
//!   objects for minimap icons;
//! * the reference table of each archive touched (archive 255), with the new CRCs and revisions,
//!   and new file ids for minted objects.
//!
//! The editor shows those containers through a cache [`Overlay`](osrs_cache::overlay::Overlay);
//! [`compile`] writes them into a copy of the base cache, beside the project's NPC spawns and a
//! [`Manifest`]. The base cache is only read.

use crate::builds::{Manifest, MANIFEST};
use crate::project::{parse_square_key, MapProject};
use osrs_cache::cache::join_files;
use osrs_cache::config::ObjectDef;
use osrs_cache::container::{self, Compression};
use osrs_cache::reftable::RefTable;
use osrs_cache::writer::CacheWriter;
use osrs_cache::{cfg, idx, locs, Cache, Error, Result};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// Containers by `(archive, group)`.
pub type Containers = BTreeMap<(u8, u32), Vec<u8>>;

/// The reference-table archive.
const META: u8 = 255;

fn fail<T>(why: impl Into<String>) -> Result<T> {
    Err(Error::Format(why.into()))
}

/// The containers `project` changes, built on `base`. `base` should read its own files only (no
/// overlay), so edits are applied to the cache as stored, not on top of themselves.
pub fn build(project: &MapProject, base: &Cache) -> Result<Containers> {
    build_with(project, base, Compression::Gzip)
}

/// [`build`] with the containers packed as `compression` says: the editor's preview skips
/// compressing, which is most of the work.
pub fn build_with(project: &MapProject, base: &Cache, compression: Compression) -> Result<Containers> {
    let mut out = Containers::new();
    let mut tables: BTreeMap<u8, RefTable> = BTreeMap::new();

    let squares: Vec<_> = project.squares.iter().filter(|(_, e)| !e.is_empty()).collect();
    if !squares.is_empty() {
        let mut maps = RefTable::decode(&base.read_group(META, idx::MAPS as u32)?)?;
        let index = maps.to_index(idx::MAPS);
        for (key, edits) in squares {
            let Some((sx, sy)) = parse_square_key(key) else {
                return fail(format!("bad square key {key:?} in the project"));
            };
            let gid = ((sx << 8) | sy) as u32;
            if !index.groups.contains_key(&gid) {
                return fail(format!("map square {sx}_{sy} is not in the cache; only existing squares can be edited"));
            }
            let mut files = base.read_files(&index, gid)?;
            if files.len() < 2 {
                return fail(format!("map square {sx}_{sy} has no placements file"));
            }
            let placed = edits.apply(&locs::decode(&files[1].1));
            files[1].1 = locs::encode(&placed);
            let data = join_files(&files.iter().map(|(_, d)| d.as_slice()).collect::<Vec<_>>());
            let packed = container::encode(&data, compression);
            maps.touch(gid, &packed, data.len());
            out.insert((idx::MAPS, gid), packed);
        }
        tables.insert(idx::MAPS, maps);
    }

    if !project.marker_defs.is_empty() {
        let mut config = RefTable::decode(&base.read_group(META, idx::CONFIG as u32)?)?;
        let index = config.to_index(idx::CONFIG);
        let mut files: BTreeMap<u32, Vec<u8>> = base.read_files(&index, cfg::OBJECT)?.into_iter().collect();
        for m in &project.marker_defs {
            files.insert(m.id, m.encode());
        }
        let data = join_files(&files.values().map(Vec::as_slice).collect::<Vec<_>>());
        let packed = container::encode(&data, compression);
        let Some(group) = config.group_mut(cfg::OBJECT) else {
            return fail("the config archive has no object group");
        };
        // keep the name hashes of the files already listed; minted ones have none
        let names: HashMap<u32, i32> = group.files.iter().copied().collect();
        group.files = files.keys().map(|&id| (id, names.get(&id).copied().unwrap_or(0))).collect();
        config.touch(cfg::OBJECT, &packed, data.len());
        out.insert((idx::CONFIG, cfg::OBJECT), packed);
        tables.insert(idx::CONFIG, config);
    }

    for (archive, mut table) in tables {
        table.revision = table.revision.wrapping_add(1);
        out.insert((META, archive as u32), container::encode(&table.encode(), compression));
    }
    Ok(out)
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
    let containers = build(project, &Cache::open(base_dir)?)?;

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
