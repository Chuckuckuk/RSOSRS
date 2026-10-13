//! Integration tests against the locally installed game cache (skipped when absent).

use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::model::Model;
use osrs_cache::*;

fn cache() -> Option<Cache> {
    Cache::open_default().ok()
}

#[test]
fn name_hash_matches_known_value() {
    assert_eq!(name_hash("l42_42"), -1153413389);
}

#[test]
fn terrain_of_lumbridge_decodes() {
    let Some(c) = cache() else { return };
    let maps = c.index(idx::MAPS).unwrap();
    let files = c.read_files(&maps, (50 << 8) | 50).unwrap();
    let t = terrain::Terrain::decode(&files[0].1, 3200, 3200).unwrap();
    assert!(t.tiles.iter().any(|t| t.overlay != 0));
    assert!(!locs::decode(&files[1].1).is_empty());
}

#[test]
fn all_models_decode() {
    let Some(c) = cache() else { return };
    let models = c.index(idx::MODELS).unwrap();
    let (mut ok, mut bad) = (0, 0);
    for &id in models.groups.keys() {
        let Ok(data) = c.read_group(idx::MODELS, id) else { continue };
        match Model::decode(id, &data) {
            Ok(_) => ok += 1,
            Err(_) => bad += 1,
        }
    }
    assert_eq!(bad, 0, "{bad} models failed to decode ({ok} ok)");
}

#[test]
fn object_definitions_parse_cleanly() {
    let Some(c) = cache() else { return };
    let cfg = Configs::new(&c).unwrap();
    let defs = cfg.objects().unwrap();
    let unknown = defs.values().filter(|d: &&ObjectDef| d.unknown_opcode.is_some()).count();
    assert!(unknown < 50, "{unknown} definitions hit unknown opcodes");
    // Tree (1276) and Ladder (11) have a single untyped model each.
    assert_eq!(defs[&1276].name, "Tree");
    assert_eq!(defs[&1276].models, vec![1570]);
}

#[test]
fn identity_kits_parse_cleanly() {
    let Some(c) = cache() else { return };
    let kits = Configs::new(&c).unwrap().identkits().unwrap();
    assert!(kits.len() > 200);
    let unknown: Vec<u32> = kits.values().filter(|k| k.unknown_opcode.is_some()).map(|k| k.id).collect();
    assert!(unknown.is_empty(), "kits with unknown opcodes: {unknown:?}");
    // every kit with a body part has at least one model, and every model decodes
    for k in kits.values().filter(|k| k.body_part.is_some()) {
        assert!(!k.models.is_empty(), "kit {} has no models", k.id);
        for m in &k.models {
            let data = c.read_group(idx::MODELS, *m).unwrap();
            Model::decode(*m, &data).unwrap();
        }
    }
    // the default character: one kit per body part, matching that part's number
    for (slot, kit) in osrs_cache::kit::Appearance::default_male().kits.iter().enumerate() {
        assert_eq!(kits[&kit.unwrap()].body_part, Some(slot as u8));
    }
}

#[test]
fn default_character_builds() {
    let Some(c) = cache() else { return };
    let tex = Configs::new(&c).unwrap().texture_average_hsl().unwrap();
    let mesh = osrs_cache::kit::build_player(&c, &osrs_cache::kit::Appearance::default_male(), &[], &tex)
        .unwrap()
        .expect("player mesh");
    assert!(mesh.opaque.indices.len() > 300);
}

#[test]
fn animation_data_parses_cleanly() {
    use osrs_cache::anim::{Animations, Sequence};
    let Some(c) = cache() else { return };
    let index = c.index(idx::CONFIG).unwrap();
    let seqs: Vec<(u32, Vec<u8>)> = c.read_files(&index, cfg::SEQUENCE).unwrap();
    // every frame of every sequence decodes and consumes exactly its bytes
    let mut anims = Animations::new(&c).unwrap();
    let (mut ok, mut bad) = (0, Vec::new());
    for (id, d) in &seqs {
        let seq = Sequence::decode(*id, d);
        for (g, f) in &seq.frames {
            match anims.frame(*g, *f) {
                Ok(_) => ok += 1,
                Err(e) => bad.push((*id, *g, *f, e.to_string())),
            }
        }
    }
    // only frames of the float-based skeletal bases (see `frame_bases_parse_cleanly`) may fail
    assert!(
        bad.iter().all(|(_, _, _, e)| e.contains("missing frame base")),
        "frames failed for another reason, first {:?}",
        bad.iter().find(|(_, _, _, e)| !e.contains("missing frame base"))
    );
    assert!(ok > 200_000 && bad.len() < 500, "{} frames failed ({ok} ok)", bad.len());
}

#[test]
fn frame_bases_parse_cleanly() {
    use osrs_cache::anim::FrameBase;
    let Some(c) = cache() else { return };
    let ix = c.index(idx::SKELETONS).unwrap();
    let mut bad = Vec::new();
    let mut tails = std::collections::HashSet::new();
    for &g in ix.groups.keys() {
        let d = c.read_file(&ix, g, 0).unwrap();
        if FrameBase::decode(g, &d).is_err() {
            bad.push(g);
        } else {
            tails.insert(d[d.len() - 2..].to_vec());
        }
    }
    // ~127 of 2675 bases are the newer float skeletons (op 14 sequences), which are not supported
    assert!(bad.len() < 200, "{} frame bases failed, first {:?}", bad.len(), &bad[..bad.len().min(5)]);
    assert_eq!(tails.len(), 1, "trailing bytes vary: {tails:?}");
}

#[test]
fn player_idle_animation_poses_the_character() {
    use osrs_cache::anim::{Animations, Clip};
    use osrs_cache::kit::{build_player, Appearance};
    let Some(c) = cache() else { return };
    let tex = Configs::new(&c).unwrap().texture_average_hsl().unwrap();
    let mesh = build_player(&c, &Appearance::default_male(), &[], &tex).unwrap().unwrap();
    let rig = mesh.rig.as_ref().unwrap();
    assert_eq!(rig.positions.len(), rig.skin.len());
    assert!(rig.skin.iter().any(|s| *s != 0), "character vertices have no animation labels");

    // walk (819) and run (824) are plain frame-based cycles too
    let mut anims = Animations::new(&c).unwrap();
    for (sequence, frames) in [(819, 8), (824, 8)] {
        let clip = Clip::bake(&mut anims, sequence, rig).unwrap();
        assert_eq!(clip.frames.len(), frames, "sequence {sequence}");
        assert!(clip.frames.iter().any(|f| f.iter().zip(&rig.positions).any(|(a, b)| a != b)));
    }

    let clip = Clip::bake(&mut anims, 808, rig).unwrap();
    assert_eq!(clip.frames.len(), 12);
    let extent = |p: &Vec<[f32; 3]>, axis: usize| {
        let (lo, hi) = p.iter().fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(v[axis]), h.max(v[axis])));
        hi - lo
    };
    let moved = clip.frames.iter().any(|f| f.iter().zip(&rig.positions).any(|(a, b)| a != b));
    assert!(moved, "idle frames equal the bind pose");
    for f in &clip.frames {
        // a sane pose keeps the body about as tall and wide as the bind pose; depth is thin
        // (arms at the sides) and the idle stance swings hands and a foot forward
        for (axis, max) in [(0, 1.3), (1, 1.3), (2, 3.0)] {
            let (a, b) = (extent(f, axis), extent(&rig.positions, axis));
            assert!(a > b * 0.7 && a < b * max, "axis {axis}: extent {a} vs bind {b}");
        }
    }
}

#[test]
fn lumbridge_collision_matches_the_scenery() {
    use osrs_cache::collision::{flag, CollisionGrid};
    use osrs_cache::terrain_mesh::{PlacedObjects, TerrainGrid};
    let Some(c) = cache() else { return };
    let cfg = Configs::new(&c).unwrap();
    let grid = TerrainGrid::load(&c, 49, 49, 3, 3, cfg.underlays().unwrap(), cfg.overlays().unwrap()).unwrap();
    let placed = PlacedObjects::load(&c, &grid).unwrap();
    let defs = cfg.objects().unwrap();
    let col = CollisionGrid::build(&grid, &placed, &defs);
    assert_eq!((col.origin_x, col.origin_z, col.width, col.depth), (49 * 64, 49 * 64, 192, 192));

    // the square the player spawns on is open ground
    assert_eq!(col.at(3222, 3218) & (flag::OBJECT | flag::FLOOR | flag::FLOOR_DECORATION), 0);

    // the fountain, a solid object with a name and actions, blocks the tile it stands on
    let fountain = placed
        .placements
        .iter()
        .find(|(_, _, p)| p.plane == 0 && p.kind == 10 && defs.get(&p.id).is_some_and(|d| d.name == "Fountain"))
        .expect("Lumbridge has a fountain");
    let (fx, fz) = (fountain.0 + col.origin_x, fountain.1 + col.origin_z);
    assert_ne!(col.at(fx, fz) & flag::OBJECT, 0, "fountain tile at {fx},{fz}");

    // terrain flagged unwalkable (the river, cliffs) exists, as do walls
    let floor = col.flags.iter().filter(|f| **f & flag::FLOOR != 0).count();
    let walls = col.flags.iter().filter(|f| **f & 0xFF != 0).count();
    assert!(floor > 500, "{floor} floor-blocked tiles");
    assert!(walls > 500, "{walls} tiles with wall edges");
    // not everything is blocked
    let open = col.flags.iter().filter(|f| **f == 0).count();
    assert!(open > col.flags.len() / 3, "{open} fully open tiles of {}", col.flags.len());

    // walls block from both sides: a straight wall on the west edge also blocks the tile beside it
    let (x, z, _) = placed
        .placements
        .iter()
        .find(|(_, _, p)| {
            p.plane == 0 && p.kind == 0 && p.orientation == 0 && defs.get(&p.id).is_some_and(|d| d.interact_type != 0)
        })
        .expect("a west-facing wall");
    assert_ne!(col.at(x + col.origin_x, z + col.origin_z) & flag::WEST, 0);
    assert_ne!(col.at(x + col.origin_x - 1, z + col.origin_z) & flag::EAST, 0);
}

#[test]
fn a_square_loaded_alone_matches_the_same_tiles_of_a_larger_block() {
    use osrs_cache::collision::CollisionGrid;
    use osrs_cache::squares::{MapSource, SQUARE};
    use osrs_cache::terrain_mesh::{PlacedObjects, TerrainGrid};

    let Ok(source) = MapSource::open() else { return };
    // a 5x5 block around Lumbridge: its inner 3x3 squares have every neighbour, so they are right
    let (cfg_cache, first) = (source.cache(), (48, 48));
    let cfg = Configs::new(cfg_cache).unwrap();
    let grid = TerrainGrid::load(cfg_cache, first.0, first.1, 5, 5, cfg.underlays().unwrap(), cfg.overlays().unwrap()).unwrap();
    let placed = PlacedObjects::load(cfg_cache, &grid).unwrap();
    let block = CollisionGrid::build(&grid, &placed, source.object_defs());

    let mut checked = 0;
    for sy in 49..=51 {
        for sx in 49..=51 {
            let alone = source.collision(sx, sy).unwrap();
            assert_eq!(alone.len(), (SQUARE * SQUARE) as usize);
            for z in 0..SQUARE {
                for x in 0..SQUARE {
                    let world = (sx * SQUARE + x, sy * SQUARE + z);
                    assert_eq!(
                        alone[(z * SQUARE + x) as usize],
                        block.at(world.0, world.1),
                        "square ({sx}, {sy}) tile {world:?}"
                    );
                    checked += 1;
                }
            }
        }
    }
    assert_eq!(checked, 9 * 64 * 64);
}

#[test]
fn a_square_missing_from_the_cache_is_solid() {
    use osrs_cache::collision::flag;
    use osrs_cache::squares::MapSource;

    let Ok(source) = MapSource::open() else { return };
    // far off the edge of the world
    let flags = source.collision(0, 0).unwrap_or_default();
    if !flags.is_empty() {
        assert!(flags.iter().all(|f| f & flag::FLOOR != 0));
    }
}

#[test]
fn a_terrain_window_is_the_same_mesh_as_those_tiles_of_the_whole_block() {
    use osrs_cache::squares::MapSource;
    use osrs_cache::terrain_mesh::TerrainMesh;

    let Ok(source) = MapSource::open() else { return };
    let around = source.neighbourhood(50, 50).unwrap();
    let whole = around.grid.build_mesh(0);
    let window = around.grid.build_mesh_window(0, 64, 64, 64, 64);

    // one key per triangle: corner positions (in thousandths of a tile) and colours
    let triangles = |m: &TerrainMesh, shift: f32, keep: &dyn Fn(&[f32; 3]) -> bool| {
        let mut keys = Vec::new();
        for tri in m.indices.chunks_exact(3) {
            let corners: Vec<[f32; 3]> = tri.iter().map(|i| m.positions[*i as usize]).collect();
            if !corners.iter().all(|p| keep(p)) {
                continue;
            }
            let mut key = Vec::new();
            for (i, p) in tri.iter().zip(&corners) {
                key.push(((p[0] - shift) * 1000.0).round() as i32);
                key.push((p[1] * 1000.0).round() as i32);
                key.push(((p[2] + shift) * 1000.0).round() as i32);
                key.extend(m.colors[*i as usize].iter().map(|c| (c * 1000.0).round() as i32));
            }
            keys.push(key);
        }
        keys.sort();
        keys
    };
    // grid z is minus north: tiles 64..=128 are z in -128..=-64
    let inside = |p: &[f32; 3]| (64.0..=128.0).contains(&p[0]) && (-128.0..=-64.0).contains(&p[2]);
    let expected = triangles(&whole, 64.0, &inside);
    let got = triangles(&window, 0.0, &|_| true);
    assert!(expected.len() > 1000, "{} triangles in the square", expected.len());
    assert_eq!(got.len(), expected.len());
    assert!(got == expected, "the window differs from the same tiles of the whole block");
}

#[test]
fn a_worn_helmet_and_weapon_change_the_character() {
    use osrs_cache::config::Configs;
    use osrs_cache::item::ids_by_name;
    use osrs_cache::kit::{build_player, Appearance, WornPart};
    let Ok(c) = Cache::open_default() else { return };
    let configs = Configs::new(&c).unwrap();
    let items = configs.items().unwrap();
    let names = ids_by_name(&items);
    let tex = configs.texture_average_hsl().unwrap();
    let part = |name: &str| WornPart::of(&items[&names[name]], false).unwrap_or_else(|| panic!("{name} has no worn model"));

    let bare = build_player(&c, &Appearance::default_male(), &[], &tex).unwrap().unwrap();
    let helm = part("bronze full helm");
    assert_eq!(helm.slot, 0, "a helmet is worn on the head");
    assert!(helm.covers.contains(&8), "a full helm covers the hair: {:?}", helm.covers);
    let whip = part("abyssal whip");
    assert_eq!(whip.slot, 3);
    let dressed = build_player(&c, &Appearance::default_male(), &[helm, whip], &tex).unwrap().unwrap();

    let count = |m: &osrs_cache::scene::LocMesh| m.opaque.indices.len() + m.translucent.indices.len();
    assert!(count(&dressed) != count(&bare), "dressed {} vs bare {}", count(&dressed), count(&bare));
    assert!(dressed.rig.is_some(), "a dressed character still has a rig to animate");
}
