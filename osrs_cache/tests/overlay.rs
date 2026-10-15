//! The in-memory overlay, against the installed game cache (skipped when absent). A test binary
//! of its own: the overlay is process-wide and would change what other tests read.

use osrs_cache::compile;
use osrs_cache::locs::{self, Placement};
use osrs_cache::project::MapProject;
use osrs_cache::*;

#[test]
fn the_overlay_shows_built_edits_without_writing_anything() {
    let Ok(base) = Cache::open_live() else { return };
    let base_dir = base.dir().to_path_buf();
    let (sx, sy) = (50, 50);
    let gid = ((sx << 8) | sy) as u32;
    let mut project = MapProject::new("overlay");
    let tree = Placement { id: 1276, plane: 0, x: 1, z: 1, kind: 10, orientation: 0 };
    project.square_mut(sx, sy).added.push(tree);
    let built = compile::build(&project, &Cache::open(&base_dir).unwrap().ignoring_overlay()).unwrap();
    assert!(built.contains_key(&(idx::MAPS, gid)) && built.contains_key(&(255, idx::MAPS as u32)));

    let has_tree = |c: &Cache| {
        let maps = c.index(idx::MAPS).unwrap();
        locs::decode(&c.read_files(&maps, gid).unwrap()[1].1).contains(&tree)
    };
    osrs_cache::overlay::set(built);
    assert!(has_tree(&Cache::open(&base_dir).unwrap()));
    assert!(!has_tree(&Cache::open(&base_dir).unwrap().ignoring_overlay()));
    osrs_cache::overlay::clear();
    assert!(!has_tree(&Cache::open(&base_dir).unwrap()));
}
