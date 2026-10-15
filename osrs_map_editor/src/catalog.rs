//! Everything the palette offers, decoded once from the base cache and the game's data files.

use bevy::prelude::*;
use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::Cache;
use osrs_core::data::Registry;
use std::collections::{BTreeMap, HashMap};

/// An object the palette can place.
pub struct ObjectEntry {
    pub id: u32,
    pub name: String,
    /// The shape it is placed as unless told otherwise (see [`default_kind`]).
    pub kind: u8,
}

/// An NPC from `data/npcs.json` (only those have stats and behaviour on the server).
pub struct NpcEntry {
    pub id: String,
    pub name: String,
    pub combat_level: i32,
}

/// A map function (minimap icon).
pub struct AreaEntry {
    pub id: u16,
    pub name: String,
    pub sprite: u32,
}

#[derive(Resource)]
pub struct Catalog {
    /// Every object definition in the base cache.
    pub defs: HashMap<u32, ObjectDef>,
    /// Objects with a model, by id.
    pub objects: Vec<ObjectEntry>,
    /// Gathering resources: object id -> the node it belongs to (`"tree"`, `"oak_tree"`...).
    pub resources: BTreeMap<u32, String>,
    pub npcs: Vec<NpcEntry>,
    pub areas: Vec<AreaEntry>,
}

/// The shape an object is placed as: the first model type its definition lists (walls list 0,
/// floor decorations 22...), else 10, free-standing scenery.
pub fn default_kind(def: &ObjectDef) -> u8 {
    match def.model_types.first() {
        Some(&t) if t <= 22 => t,
        _ => 10,
    }
}

/// Footprint in tiles of `def` turned `orientation` quarter turns: odd turns swap its sides.
pub fn footprint(def: Option<&ObjectDef>, orientation: u8) -> (i32, i32) {
    let (w, h) = def.map_or((1, 1), |d| (d.size_x.max(1) as i32, d.size_y.max(1) as i32));
    if orientation & 1 == 1 {
        (h, w)
    } else {
        (w, h)
    }
}

/// What the shape numbers mean, for the shape picker.
pub const KINDS: [(u8, &str); 13] = [
    (0, "wall (straight)"),
    (1, "wall (diagonal corner)"),
    (2, "wall (corner)"),
    (3, "wall (square corner)"),
    (4, "wall decoration"),
    (9, "diagonal wall"),
    (10, "scenery"),
    (11, "scenery (diagonal)"),
    (12, "roof (straight)"),
    (13, "roof (diagonal)"),
    (19, "roof (flat)"),
    (21, "roof edge"),
    (22, "floor decoration"),
];

pub fn kind_name(kind: u8) -> &'static str {
    KINDS.iter().find(|(k, _)| *k == kind).map_or("other", |(_, n)| n)
}

impl Catalog {
    pub fn load(base: &Cache) -> osrs_cache::Result<Catalog> {
        let defs = Configs::new(base)?.objects()?;
        let mut objects: Vec<ObjectEntry> = defs
            .values()
            .filter(|d| !d.models.is_empty())
            .map(|d| ObjectEntry { id: d.id, name: display_name(d), kind: default_kind(d) })
            .collect();
        objects.sort_by_key(|o| o.id);

        let registry = Registry::builtin();
        let mut resources = BTreeMap::new();
        for node in registry.gathering_nodes() {
            for loc in &node.locs {
                resources.insert(loc.loc, node.id.clone());
            }
        }
        let mut npcs: Vec<NpcEntry> =
            registry.npcs().map(|n| NpcEntry { id: n.id.clone(), name: n.name.clone(), combat_level: n.combat_level }).collect();
        npcs.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));

        let mut areas: Vec<AreaEntry> = osrs_cache::minimap::map_functions(base)?
            .into_iter()
            .filter_map(|(id, (sprite, name))| Some(AreaEntry { id: u16::try_from(id).ok()?, name, sprite }))
            .collect();
        areas.sort_by_key(|a| a.id);
        Ok(Catalog { defs, objects, resources, npcs, areas })
    }

    pub fn object_name(&self, id: u32) -> String {
        self.defs.get(&id).map_or_else(|| format!("object {id}"), display_name)
    }

    pub fn area_name(&self, id: u16) -> String {
        self.areas.iter().find(|a| a.id == id).map_or_else(|| format!("icon {id}"), |a| area_label(a))
    }
}

/// An object's name, or its id when it has none (the cache calls many "null").
pub fn display_name(d: &ObjectDef) -> String {
    if d.name.is_empty() || d.name == "null" {
        format!("({})", d.id)
    } else {
        d.name.clone()
    }
}

pub fn area_label(a: &AreaEntry) -> String {
    if a.name.is_empty() {
        format!("icon {}", a.id)
    } else {
        a.name.clone()
    }
}
