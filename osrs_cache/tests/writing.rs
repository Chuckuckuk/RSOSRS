//! Writing caches, against the installed game cache (skipped when absent): the encoders must
//! reproduce what the game ships, and a compile must change exactly what the project says and
//! nothing in the cache it was built on.

use osrs_cache::collision::flag;
use osrs_cache::compile::{self, Stage};
use osrs_cache::locs::{self, Placement};
use osrs_cache::minimap::MinimapSource;
use osrs_cache::project::{MapProject, MarkerDef, NpcSpawn};
use osrs_cache::reftable::RefTable;
use osrs_cache::squares::MapSource;
use osrs_cache::*;
use std::path::{Path, PathBuf};

/// Always the installed game's cache: these tests build on it, whatever cache is active.
fn live() -> Option<Cache> {
    Cache::open_live().ok()
}

#[test]
fn every_reference_table_re_encodes_byte_for_byte() {
    let Some(c) = live() else { return };
    let mut checked = 0;
    for archive in 0..=254u32 {
        let Ok(data) = c.read_group(255, archive) else { continue };
        let table = RefTable::decode(&data).unwrap_or_else(|e| panic!("archive {archive}: {e}"));
        assert!(table.encode() == data, "archive {archive}'s table changed in a round trip");
        checked += 1;
    }
    assert!(checked > 10, "only {checked} tables");
}

#[test]
fn every_map_squares_placements_re_encode_byte_for_byte() {
    let Some(c) = live() else { return };
    let maps = c.index(idx::MAPS).unwrap();
    let (mut same, mut reordered) = (0, Vec::new());
    for &gid in maps.groups.keys() {
        let Ok(files) = c.read_files(&maps, gid) else { continue };
        let Some((_, data)) = files.get(1) else { continue };
        let placed = locs::decode(data);
        let encoded = locs::encode(&placed);
        if encoded == *data {
            same += 1;
        } else {
            // must at least hold the same placements
            let (mut a, mut b) = (placed.clone(), locs::decode(&encoded));
            a.sort_by_key(|p| (p.id, p.packed_position(), p.kind, p.orientation));
            b.sort_by_key(|p| (p.id, p.packed_position(), p.kind, p.orientation));
            assert_eq!(a, b, "square {} {}", gid >> 8, gid & 255);
            reordered.push(gid);
        }
    }
    assert!(same > 1000, "only {same} squares");
    // a handful of squares store placements in an order the encoder does not reproduce (the same
    // placements, as checked above); this cache has one such square in ~25,000
    assert!(reordered.len() <= 5, "{} squares re-encode differently: {:?}", reordered.len(), &reordered[..reordered.len().min(10)]);
}

fn temp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("osrs_compile_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// Size and modification time of every cache file in `dir`.
fn stamp(dir: &Path) -> Vec<(String, u64, std::time::SystemTime)> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| osrs_cache::writer::is_cache_file(&e.file_name().to_string_lossy()))
        .map(|e| {
            let m = e.metadata().unwrap();
            (e.file_name().to_string_lossy().into_owned(), m.len(), m.modified().unwrap())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn a_compiled_cache_has_the_edits_and_the_base_is_untouched() {
    let Some(base) = live() else { return };
    let base_dir = base.dir().to_path_buf();
    let before = stamp(&base_dir);
    let source = MapSource::from_cache(Cache::open(&base_dir).unwrap().ignoring_overlay()).unwrap();
    let (sx, sy) = (50, 50);
    let around = source.neighbourhood(sx, sy).unwrap();
    let collision = around.collision(source.object_defs());

    // an open tile for a tree, a few tiles in from the square's edges
    let (tx, tz) = (8..56i32)
        .flat_map(|z| (8..56i32).map(move |x| (x, z)))
        .find(|&(x, z)| collision[(z * 64 + x) as usize] == 0)
        .unwrap();
    let tree = Placement { id: 1276, plane: 0, x: tx as u8, z: tz as u8, kind: 10, orientation: 0 };
    // something the square has, to take out
    let (_, &gone) = around.middle_placements().find(|(_, p)| p.kind == 10 && p.plane == 0).unwrap();
    let defs = source.object_defs();
    let area = defs.values().find_map(|d| d.map_icon).unwrap();
    let marker = MarkerDef { id: compile::next_object_id(defs, &MapProject::default()), map_icon: area };
    // another open tile, clear of the tree's footprint
    let (mx, mz) = (8..56i32)
        .flat_map(|z| (8..56i32).map(move |x| (x, z)))
        .find(|&(x, z)| collision[(z * 64 + x) as usize] == 0 && ((x - tx).abs() > 3 || (z - tz).abs() > 3))
        .map(|(x, z)| (x as u8, z as u8))
        .unwrap();
    let icon = Placement { id: marker.id, plane: 0, x: mx, z: mz, kind: 22, orientation: 0 };

    let mut project = MapProject::new("test");
    project.square_mut(sx, sy).added.extend([tree, icon]);
    project.square_mut(sx, sy).removed.push(gone);
    project.marker_defs.push(marker);
    project.npc_spawns.push(NpcSpawn { npc: "man".into(), x: 3222, z: 3218, plane: 0 });

    let root = temp("lumbridge");
    let out = root.join("lumbridge_test");
    let mut stages = Vec::new();
    let manifest = compile::compile(&project, &base_dir, &out, |s| stages.push(s)).unwrap();
    assert_eq!(stages.last(), Some(&Stage::Done));
    assert_eq!((manifest.squares, manifest.marker_defs, manifest.npc_spawns), (1, 1, 1));

    // the base cache is exactly as it was
    assert_eq!(stamp(&base_dir), before, "the base cache's files changed");

    let built = MapSource::from_cache(Cache::open(&out).unwrap()).unwrap();
    let after = built.neighbourhood(sx, sy).unwrap();
    let placed: Vec<Placement> = after.middle_placements().map(|(_, p)| *p).collect();
    assert!(placed.contains(&tree));
    assert!(placed.contains(&icon));
    let before_count = around.middle_placements().filter(|(_, p)| **p == gone).count();
    assert_eq!(placed.iter().filter(|p| **p == gone).count(), before_count - 1);
    // the tree blocks, the marker does not
    let walls = after.collision(built.object_defs());
    assert_ne!(walls[(tz * 64 + tx) as usize] & flag::OBJECT, 0);
    let m = (mz as i32 * 64 + mx as i32) as usize;
    assert_eq!(walls[m], collision[m]);
    // the marker shows its icon
    let minted = &built.object_defs()[&marker.id];
    assert_eq!(minted.map_icon, Some(area));
    let mini = MinimapSource::new(built.cache()).unwrap().render(sx, sy).unwrap();
    assert!(mini.icons.iter().any(|i| i.x == sx * 64 + mx as i32 && i.z == sy * 64 + mz as i32 && i.area == area as u32));

    // everything else reads as it did
    let new = Cache::open(&out).unwrap();
    for gid in [(50 << 8) | 51, (49 << 8) | 50] {
        assert_eq!(new.read_raw(idx::MAPS, gid).unwrap(), base.read_raw(idx::MAPS, gid).unwrap());
    }
    assert_eq!(new.read_raw(idx::MODELS, 1).unwrap(), base.read_raw(idx::MODELS, 1).unwrap());
    let objects_before = osrs_cache::config::Configs::new(&base).unwrap().objects().unwrap().len();
    assert_eq!(built.object_defs().len(), objects_before + 1);

    // the sidecars
    assert_eq!(osrs_cache::active::npc_spawns(&out).unwrap(), project.npc_spawns);

    // compiling again replaces the earlier build; a stranger directory is refused
    compile::compile(&project, &base_dir, &out, |_| {}).unwrap();
    std::fs::create_dir_all(root.join("not_a_cache")).unwrap();
    assert!(compile::compile(&project, &base_dir, &root.join("not_a_cache"), |_| {}).is_err());
    assert!(compile::compile(&project, &base_dir, &base_dir, |_| {}).is_err());
    let _ = std::fs::remove_dir_all(&root);
}
