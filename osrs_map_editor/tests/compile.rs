//! Building projects into caches, against the installed game cache (skipped when absent): a
//! compile must change exactly what the project says and nothing in the cache it was built on, and
//! the editor's preview (the same groups held in an overlay) must show what a compile will hold.

use osrs_cache::collision::flag;
use osrs_cache::container::Compression;
use osrs_cache::locs::Placement;
use osrs_cache::minimap::MinimapSource;
use osrs_cache::overlay::Overlay;
use osrs_cache::squares::MapSource;
use osrs_cache::*;
use osrs_core::data::{read_npc_spawns, NpcSpawnPoint};
use osrs_map_editor::compile::{self, Stage};
use osrs_map_editor::project::{MapProject, MarkerDef};
use osrs_render::streaming::SquareLoader;
use osrs_cache::testing::{stamp, temp_dir as temp};
use std::sync::Arc;

#[test]
fn a_compiled_cache_has_the_edits_and_the_base_is_untouched() {
    let Ok(base) = Cache::open_live() else { return };
    let base_dir = base.dir().to_path_buf();
    let before = stamp(&base_dir);
    let source = MapSource::from_cache(Cache::open(&base_dir).unwrap()).unwrap();
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
    project.npc_spawns.push(NpcSpawnPoint { npc: "man".into(), x: 3222, z: 3218, plane: 0 });

    let root = temp("lumbridge");
    let out = root.join("lumbridge_test");
    let mut stages = Vec::new();
    let manifest = compile::compile(&project, &base_dir, &out, |s| stages.push(s)).unwrap();
    assert_eq!(stages.last(), Some(&Stage::Done));
    assert_eq!((manifest.squares, manifest.marker_defs, manifest.npc_spawns), (1, 1, 1));
    assert!(project.is_compiled_into(&out));

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

    // the spawns travel with the cache, for the server
    assert_eq!(read_npc_spawns(&out).unwrap(), project.npc_spawns);

    // compiling again replaces the earlier build; a stranger directory is refused
    compile::compile(&project, &base_dir, &out, |_| {}).unwrap();
    std::fs::create_dir_all(root.join("not_a_cache")).unwrap();
    assert!(compile::compile(&project, &base_dir, &root.join("not_a_cache"), |_| {}).is_err());
    assert!(compile::compile(&project, &base_dir, &base_dir, |_| {}).is_err());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_preview_draws_a_square_with_its_edits_before_any_compile() {
    let Ok(base) = Cache::open_live() else { return };
    let mut project = MapProject::new("t");
    let tree = Placement { id: 1276, plane: 0, x: 19, z: 22, kind: 10, orientation: 0 };
    project.square_mut(50, 50).added.push(tree);
    let trees = |loader: &mut SquareLoader| loader.load(50, 50).unwrap().objects.instances.iter().filter(|(k, _, _)| k.0 == 1276).count();

    // a loader on the base cache with the editor's overlay over it, as the editor draws the world
    let overlay = Overlay::new();
    let mut loader = SquareLoader::from_cache(Cache::open(base.dir()).unwrap().with_overlay(Arc::clone(&overlay))).unwrap();
    let before = trees(&mut loader);
    overlay.set(compile::build_with(&project, Cache::open(base.dir()).unwrap(), Compression::None).unwrap());
    assert_eq!(trees(&mut loader), before + 1);
    // nothing was written: the base cache itself still has none
    let mut plain = SquareLoader::from_cache(Cache::open(base.dir()).unwrap()).unwrap();
    assert_eq!(trees(&mut plain), before);
}
