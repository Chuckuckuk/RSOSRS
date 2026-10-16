//! Writing caches, against the installed game cache (skipped when absent): the encoders must
//! reproduce what the game ships, and a group written into a copy must read back there without
//! touching the cache it was copied from. (Building a whole map editor project is tested in
//! `osrs_map_editor/tests/compile.rs`.)

use osrs_cache::locs::{self, Placement};
use osrs_cache::reftable::RefTable;
use osrs_cache::*;
use osrs_cache::testing::{stamp, temp_dir as temp};

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

#[test]
fn a_group_written_into_a_copy_reads_back_and_the_base_is_untouched() {
    let Some(base) = live() else { return };
    let base_dir = base.dir().to_path_buf();
    let before = stamp(&base_dir);
    let root = temp("writer");
    let out = root.join("copy");

    // Lumbridge's square with one more tree, written into a copy
    let maps = base.index(idx::MAPS).unwrap();
    let gid = (50 << 8) | 50;
    let mut files = base.read_files(&maps, gid).unwrap();
    let mut placed = locs::decode(&files[1].1);
    let tree = Placement { id: 1276, plane: 0, x: 1, z: 1, kind: 10, orientation: 0 };
    placed.push(tree);
    files[1].1 = locs::encode(&placed);
    let data = osrs_cache::cache::join_files(&files.iter().map(|(_, d)| d.as_slice()).collect::<Vec<_>>());
    let packed = osrs_cache::container::encode(&data, osrs_cache::container::Compression::Gzip);
    let mut table = RefTable::decode(&base.read_group(255, idx::MAPS as u32).unwrap()).unwrap();
    table.touch(gid, &packed, data.len());
    let mut writer = osrs_cache::writer::CacheWriter::create(&base_dir, &out).unwrap();
    writer.put(idx::MAPS, gid, &packed).unwrap();
    writer.put(255, idx::MAPS as u32, &osrs_cache::container::encode(&table.encode(), osrs_cache::container::Compression::Gzip)).unwrap();
    writer.finish().unwrap();

    assert_eq!(stamp(&base_dir), before, "the base cache's files changed");
    let copy = Cache::open(&out).unwrap();
    let copy_maps = copy.index(idx::MAPS).unwrap();
    assert!(locs::decode(&copy.read_files(&copy_maps, gid).unwrap()[1].1).contains(&tree));
    assert_eq!(copy_maps.groups[&gid].crc, osrs_cache::reftable::crc32(&packed));
    assert_eq!(copy.read_raw(idx::MAPS, (50 << 8) | 51).unwrap(), base.read_raw(idx::MAPS, (50 << 8) | 51).unwrap());
    let _ = std::fs::remove_dir_all(&root);
}

