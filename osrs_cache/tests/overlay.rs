//! A cache opened with an overlay reads the groups it holds from it, and everything else from its
//! files; a cache without one never sees it. Against the installed game cache (skipped when
//! absent).

use osrs_cache::container::{self, Compression};
use osrs_cache::overlay::Overlay;
use osrs_cache::*;
use std::sync::Arc;

#[test]
fn a_cache_with_an_overlay_reads_its_groups_from_it() {
    let Ok(plain) = Cache::open_live() else { return };
    let dir = plain.dir().to_path_buf();
    let overlay = Overlay::new();
    let with = Cache::open(&dir).unwrap().with_overlay(Arc::clone(&overlay));
    let also = Cache::open(&dir).unwrap().with_overlay(Arc::clone(&overlay));

    let replaced = container::encode(b"not the real model", Compression::None);
    let original = plain.read_raw(idx::MODELS, 1).unwrap();
    overlay.set([((idx::MODELS, 1), replaced.clone())]);

    assert_eq!(with.read_raw(idx::MODELS, 1).unwrap(), replaced);
    assert_eq!(also.read_raw(idx::MODELS, 1).unwrap(), replaced, "caches sharing an overlay see the same groups");
    assert_eq!(with.read_group(idx::MODELS, 1).unwrap(), b"not the real model");
    assert_eq!(plain.read_raw(idx::MODELS, 1).unwrap(), original, "a cache without the overlay never sees it");
    assert_eq!(with.read_raw(idx::MODELS, 2).unwrap(), plain.read_raw(idx::MODELS, 2).unwrap(), "other groups come from the files");

    overlay.set(std::iter::empty());
    assert_eq!(with.read_raw(idx::MODELS, 1).unwrap(), original);
}
