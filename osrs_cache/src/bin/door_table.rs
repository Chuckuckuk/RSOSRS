//! Generates the door table (`osrs_core/data/doors.json`) from the whole map, with a coverage
//! report on stderr: `door_table > osrs_core/data/doors.json`.
//!
//! - A *closed door* is a straight-wall placement (shape 0) whose only option is "Open".
//! - Its *open form* is an object whose only option is "Close" with the same name: one with the
//!   same models at the nearest id (within `MAX_ID_GAP`), else one with other models right next
//!   to it (within `ADJACENT_GAP`, as many open doors have a model of their own). Several closed
//!   doors may share one open form (4636, 4637, 4638 and 4640 all open into 4639). An open form
//!   that does not block (clip type 0) is not used: the server gives an open door its wall.
//! - Two closed doors side by side on one wall, same name and turn, are the leaves of a pair: a
//!   double door, or a gate when they are called "Gate".
//!
//! A door on a diagonal wall (shape 9) is the same object as a straight one and swings by
//! `LeafMove::DIAGONAL`, so it needs no entry of its own: the report counts those whose object
//! has a door type as covered.
//!
//! Doors with other options (locked, "Pick-lock", "Knock-at", quest doors), on other wall shapes,
//! or with no clear open form are left out and counted in the report.

use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::*;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// How far apart a closed door's id and its open form's may be, when they share models.
const MAX_ID_GAP: u32 = 10;

/// How far apart they may be when the open form has models of its own.
const ADJACENT_GAP: u32 = 2;

fn options(def: &ObjectDef) -> Vec<&str> {
    def.actions.iter().map(|a| a.trim()).filter(|a| !a.is_empty()).collect()
}

fn only(def: &ObjectDef, option: &str) -> bool {
    matches!(options(def)[..], [o] if o.eq_ignore_ascii_case(option))
}

/// The tile one step along a wall turned `orientation`, clockwise from north for a west-edge wall.
fn along(orientation: u8) -> (i32, i32) {
    match orientation & 3 {
        0 => (0, 1),
        1 => (1, 0),
        2 => (0, -1),
        _ => (-1, 0),
    }
}

#[derive(Clone, Copy)]
struct Placed {
    id: u32,
    tile: (i32, i32, u8),
    kind: u8,
    orientation: u8,
}

fn main() -> Result<()> {
    let cache = Cache::open_default()?;
    let defs = Configs::new(&cache)?.objects()?;
    let maps = cache.index(idx::MAPS)?;

    // every wall-shaped placement (straight, corner or diagonal) with an "Open" option, anywhere
    let mut openable: Vec<Placed> = Vec::new();
    // map squares whose objects could not be read (or that have none), to report
    let (mut read, mut unreadable) = (0, Vec::new());
    for &group in maps.groups.keys() {
        let files = match cache.read_files(&maps, group) {
            Ok(files) if files.len() >= 2 => files,
            _ => {
                unreadable.push(((group >> 8) as i32, (group & 0xff) as i32));
                continue;
            }
        };
        read += 1;
        let (rx, ry) = ((group >> 8) as i32, (group & 0xff) as i32);
        for p in locs::decode(&files[1].1) {
            let Some(def) = defs.get(&p.id) else { continue };
            if matches!(p.kind, 0..=3 | 9) && options(def).iter().any(|o| o.eq_ignore_ascii_case("Open")) {
                let tile = (rx * 64 + p.x as i32, ry * 64 + p.z as i32, p.plane);
                openable.push(Placed { id: p.id, tile, kind: p.kind, orientation: p.orientation });
            }
        }
    }

    let plain = |placed: &Placed| placed.kind == 0 && only(&defs[&placed.id], "Open");
    let at: HashMap<(i32, i32, u8), Vec<Placed>> = openable.iter().filter(|p| plain(p)).fold(HashMap::new(), |mut map, p| {
        map.entry(p.tile).or_default().push(*p);
        map
    });

    // pairs: a plain closed door with another of the same name and turn one step along the wall
    let mut pair_counts: BTreeMap<(u32, u32), usize> = BTreeMap::new();
    let mut in_pair: HashMap<u32, usize> = HashMap::new();
    let mut single: HashMap<u32, usize> = HashMap::new();
    for p in openable.iter().filter(|p| plain(p)) {
        let step = |sign: i32| {
            let (dx, dz) = along(p.orientation);
            (p.tile.0 + dx * sign, p.tile.1 + dz * sign, p.tile.2)
        };
        let partner = |sign: i32| {
            at.get(&step(sign)).and_then(|here| {
                here.iter()
                    .find(|o| o.orientation == p.orientation && defs[&o.id].name.eq_ignore_ascii_case(&defs[&p.id].name))
                    .copied()
            })
        };
        if let Some(next) = partner(1) {
            *pair_counts.entry((p.id, next.id)).or_default() += 1;
        }
        if partner(1).is_some() || partner(-1).is_some() {
            *in_pair.entry(p.id).or_default() += 1;
        } else {
            *single.entry(p.id).or_default() += 1;
        }
    }

    // each id goes in one door: its commonest pair, or single if it stands alone more often
    let mut pairs: Vec<(u32, u32)> = Vec::new();
    let mut paired: BTreeSet<u32> = BTreeSet::new();
    let mut by_count: Vec<(&(u32, u32), &usize)> = pair_counts.iter().collect();
    by_count.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (&(a, b), _) in by_count {
        let alone = |id: u32| single.get(&id).copied().unwrap_or(0) > in_pair.get(&id).copied().unwrap_or(0);
        if paired.contains(&a) || paired.contains(&b) || alone(a) || alone(b) {
            continue;
        }
        paired.insert(a);
        paired.insert(b);
        pairs.push((a, b));
    }
    let singles: BTreeSet<u32> =
        openable.iter().filter(|p| plain(p)).map(|p| p.id).filter(|id| !paired.contains(id)).collect();

    // open forms: same name, "Close" only; same models nearby, else other models right next to it
    let mut open_forms: HashMap<String, Vec<u32>> = HashMap::new();
    for def in defs.values().filter(|d| only(d, "Close")) {
        open_forms.entry(def.name.to_lowercase()).or_default().push(def.id);
    }
    let candidates = |closed: u32| -> Vec<u32> {
        let def = &defs[&closed];
        let mut found: Vec<(bool, u32, u32)> = open_forms
            .get(&def.name.to_lowercase())
            .into_iter()
            .flatten()
            .filter_map(|&id| {
                let (gap, same_models) = (id.abs_diff(closed), defs[&id].models == def.models);
                let near = if same_models { gap <= MAX_ID_GAP } else { gap <= ADJACENT_GAP };
                near.then_some((!same_models, gap, id))
            })
            .collect();
        found.sort();
        found.into_iter().map(|(_, _, id)| id).collect()
    };
    let blocks = |id: &u32| defs[id].interact_type != 0;
    let mut doors: Vec<(String, &str, Vec<(u32, u32)>)> = Vec::new();
    let mut no_open_form: BTreeSet<u32> = BTreeSet::new();

    for &(a, b) in &pairs {
        // the cheapest pair of distinct open forms
        let best = candidates(a)
            .into_iter()
            .flat_map(|oa| candidates(b).into_iter().map(move |ob| (oa, ob)))
            .filter(|(oa, ob)| (oa != ob || a == b) && blocks(oa) && blocks(ob))
            // fewest open forms with other models first, then the nearest ids
            .min_by_key(|(oa, ob)| {
                let other_models = |closed: u32, open: u32| (defs[&closed].models != defs[&open].models) as u32;
                (other_models(a, *oa) + other_models(b, *ob), oa.abs_diff(a) + ob.abs_diff(b), *oa, *ob)
            });
        match best {
            Some((oa, ob)) => {
                let swing = if defs[&a].name.to_lowercase().contains("gate") { "gate" } else { "double" };
                doors.push((format!("door_{a}_{b}"), swing, vec![(a, oa), (b, ob)]));
            }
            None => {
                no_open_form.insert(a);
                no_open_form.insert(b);
            }
        }
    }
    for &id in &singles {
        match candidates(id).into_iter().find(blocks) {
            Some(open) => {
                doors.push((format!("door_{id}"), "single", vec![(id, open)]));
            }
            None => {
                no_open_form.insert(id);
            }
        }
    }
    doors.sort_by_key(|(_, _, leaves)| leaves[0].0);

    // the table
    println!("{{");
    println!("  \"doors\": [");
    for (i, (id, swing, leaves)) in doors.iter().enumerate() {
        let leaves: Vec<String> = leaves.iter().map(|(c, o)| format!("{{ \"closed\": {c}, \"open\": {o} }}")).collect();
        let comma = if i + 1 < doors.len() { "," } else { "" };
        println!("    {{ \"id\": \"{id}\", \"swing\": \"{swing}\", \"leaves\": [{}] }}{comma}", leaves.join(", "));
    }
    println!("  ]");
    println!("}}");

    // the coverage report
    let covered: BTreeSet<u32> = doors.iter().flat_map(|(_, _, leaves)| leaves.iter().map(|(c, _)| *c)).collect();
    let mut tally: BTreeMap<&str, usize> = BTreeMap::new();
    let mut left_out: BTreeMap<String, usize> = BTreeMap::new();
    for p in &openable {
        let def = &defs[&p.id];
        let reason = if p.kind == 9 && covered.contains(&p.id) && only(def, "Open") {
            "covered (diagonal)"
        } else if p.kind == 9 {
            "diagonal, no door type"
        } else if p.kind != 0 {
            "other wall shape"
        } else if !only(def, "Open") {
            "more than Open"
        } else if covered.contains(&p.id) {
            "covered"
        } else if candidates(p.id).is_empty() {
            "no open form (walk-through doors, barriers)"
        } else if !candidates(p.id).iter().any(blocks) {
            "open form does not block"
        } else {
            "partner leaf has no open form"
        };
        *tally.entry(reason).or_default() += 1;
        if !reason.starts_with("covered") {
            *left_out.entry(format!("{reason}: {} '{}' {:?}", p.id, def.name, options(def))).or_default() += 1;
        }
    }
    let count = |swing: &str| doors.iter().filter(|(_, s, _)| *s == swing).count();
    eprintln!("map squares read: {read}; unreadable or without objects: {}", unreadable.len());
    if !unreadable.is_empty() {
        eprintln!("  e.g. {:?}", &unreadable[..unreadable.len().min(12)]);
    }
    eprintln!(
        "{} door types ({} single, {} double, {} gate) from {} openable wall placements (straight, corner, diagonal)",
        doors.len(),
        count("single"),
        count("double"),
        count("gate"),
        openable.len()
    );
    for (reason, count) in &tally {
        eprintln!("  {reason}: {count}");
    }
    let mut biggest: Vec<(&String, &usize)> = left_out.iter().collect();
    biggest.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    eprintln!("most common left out:");
    for (what, count) in biggest.iter().take(25) {
        eprintln!("  {count:5}  {what}");
    }
    Ok(())
}
