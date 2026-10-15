//! The map editor's preview: a square loaded while the cache overlay holds an edit shows the edit.
//! A test binary of its own, as the overlay is process-wide.

use osrs_cache::compile;
use osrs_cache::container::Compression;
use osrs_cache::locs::Placement;
use osrs_cache::project::MapProject;
use osrs_cache::Cache;
use osrs_render::streaming::SquareLoader;

#[test]
fn a_square_loaded_through_the_overlay_has_the_placed_tree() {
    let Ok(base) = Cache::open_live() else { return };
    let base = base.ignoring_overlay();
    let mut project = MapProject::new("t");
    let tree = Placement { id: 1276, plane: 0, x: 19, z: 22, kind: 10, orientation: 0 };
    project.square_mut(50, 50).added.push(tree);
    let count = |loader: &mut SquareLoader| loader.load(50, 50).unwrap().objects.instances.iter().filter(|(k, _, _)| k.0 == 1276).count();

    let mut loader = SquareLoader::open().unwrap();
    let before = count(&mut loader);
    osrs_cache::overlay::set(compile::build_with(&project, &base, Compression::None).unwrap());
    let after = count(&mut loader);
    osrs_cache::overlay::clear();
    assert_eq!(after, before + 1, "trees: {before} before, {after} after");
}
