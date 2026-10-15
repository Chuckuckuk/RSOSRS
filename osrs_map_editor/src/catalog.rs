//! Everything the palette offers, decoded once from the base cache and the game's data files.

use bevy::prelude::*;
use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::npc::NpcDef;
use osrs_cache::Cache;
use osrs_core::data::{cache_npc_id, Registry};
use std::collections::{BTreeMap, HashMap};

/// An object the palette can place.
pub struct ObjectEntry {
    pub id: u32,
    pub name: String,
    /// `name` in lower case, for searching.
    pub name_lower: String,
    /// The shape it is placed as unless told otherwise (see [`default_kind`]).
    pub kind: u8,
}

/// What the palette's search box asks for: a name containing it (any case), or an id equal to it.
pub struct Search {
    lower: String,
    id: Option<u32>,
}

impl Search {
    pub fn new(text: &str) -> Search {
        let text = text.trim();
        Search { lower: text.to_lowercase(), id: text.parse().ok() }
    }

    /// Whether `name_lower` (a name already in lower case) or `id` matches.
    pub fn matches(&self, name_lower: &str, id: Option<u32>) -> bool {
        self.lower.is_empty() || name_lower.contains(&self.lower) || (id.is_some() && id == self.id)
    }
}

/// An NPC the palette can place: every NPC the cache can draw, plus the curated ones in
/// `data/npcs.json` (only those have combat stats on the server).
pub struct NpcEntry {
    /// What a spawn stores: the curated id when `npcs.json` has the NPC, else `cache:<id>`.
    pub key: String,
    pub name: String,
    pub combat_level: i32,
    /// The cache NPC it is drawn as, if any.
    pub cache_id: Option<u32>,
    /// Whether `npcs.json` gives it combat stats.
    pub has_stats: bool,
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
    /// Every placeable NPC, one per model, by name, level and cache id.
    pub npcs: Vec<NpcEntry>,
    /// Index into `npcs` by spawn key, and by cache id.
    npc_index: HashMap<String, usize>,
    npc_by_cache: HashMap<u32, usize>,
    /// The palette's rows: `npcs` with the same name and level together.
    pub npc_groups: Vec<NpcGroup>,
    /// The cache's definitions of the placeable NPCs, for drawing their thumbnails.
    pub npc_defs: HashMap<u32, NpcDef>,
    pub areas: Vec<AreaEntry>,
}

/// NPCs sharing a name and combat level: often the same NPC drawn several ways (other outfits,
/// recolours, the versions in other places).
pub struct NpcGroup {
    pub name: String,
    /// `name` in lower case, for searching.
    pub name_lower: String,
    pub combat_level: i32,
    /// Indices into [`Catalog::npcs`].
    pub members: Vec<usize>,
    /// Whether any of them has combat stats.
    pub has_stats: bool,
}

impl NpcGroup {
    /// Its name, and its combat level when it has one: "Bouncer (level 137)".
    pub fn label(&self) -> String {
        if self.combat_level > 0 {
            format!("{} (level {})", self.name, self.combat_level)
        } else {
            self.name.clone()
        }
    }
}

/// The mark after an NPC with combat stats.
pub fn star(has_stats: bool) -> &'static str {
    if has_stats {
        "  \u{2605}"
    } else {
        ""
    }
}

/// Facing names by quarter turn.
pub const FACINGS: [&str; 4] = ["N", "E", "S", "W"];

/// The shape an object is placed as: the first model type its definition lists (walls list 0,
/// floor decorations 22...), else 10, free-standing scenery.
pub fn default_kind(def: &ObjectDef) -> u8 {
    match def.model_types.first() {
        Some(&t) if t <= 22 => t,
        _ => 10,
    }
}

/// Footprint in tiles of `def` turned `orientation` quarter turns (`osrs_cache::scene::footprint`),
/// one tile when the definition is unknown.
pub fn footprint(def: Option<&ObjectDef>, orientation: u8) -> (i32, i32) {
    def.map_or((1, 1), |d| {
        let (w, h) = osrs_cache::scene::footprint(d, orientation);
        (w as i32, h as i32)
    })
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
        let configs = Configs::new(base)?;
        let defs = configs.objects()?;
        let mut objects: Vec<ObjectEntry> = defs
            .values()
            .filter(|d| !d.models.is_empty())
            .map(|d| {
                let name = display_name(d);
                ObjectEntry { id: d.id, name_lower: name.to_lowercase(), name, kind: default_kind(d) }
            })
            .collect();
        objects.sort_by_key(|o| o.id);

        let registry = Registry::builtin();
        let mut resources = BTreeMap::new();
        for node in registry.gathering_nodes() {
            for loc in &node.locs {
                resources.insert(loc.loc, node.id.clone());
            }
        }
        let mut npcs = Vec::new();
        // every NPC the cache can draw: it has models of its own (not one that turns into others
        // by game state) and a standing animation
        let npc_defs: HashMap<u32, NpcDef> = configs
            .npcs()?
            .into_iter()
            .filter(|(_, d)| !d.models.is_empty() && d.stand.is_some() && !d.name.is_empty() && d.name != "null")
            .collect();
        for def in npc_defs.values() {
            let curated = registry.npc_for_cache_id(def.id);
            npcs.push(NpcEntry {
                key: registry.spawn_key_for_cache_npc(def.id),
                name: strip_tags(&def.name),
                combat_level: curated.map_or(def.combat_level.map_or(0, i32::from), |c| c.combat_level),
                cache_id: Some(def.id),
                has_stats: curated.is_some(),
            });
        }
        // curated ones the cache cannot draw yet (no appearance): placeable, but invisible in game
        for n in registry.npcs().filter(|n| n.appearance.is_none()) {
            npcs.push(NpcEntry { key: n.id.clone(), name: n.name.clone(), combat_level: n.combat_level, cache_id: None, has_stats: true });
        }
        npcs.sort_by(|a, b| a.name.cmp(&b.name).then(a.combat_level.cmp(&b.combat_level)).then(a.cache_id.cmp(&b.cache_id)));
        let mut npc_index = HashMap::new();
        let mut npc_by_cache = HashMap::new();
        for (i, n) in npcs.iter().enumerate() {
            npc_index.entry(n.key.clone()).or_insert(i);
            if let Some(id) = n.cache_id {
                npc_by_cache.insert(id, i);
            }
        }
        // one palette row per name and level; the models under it are picked from a list
        let mut npc_groups: Vec<NpcGroup> = Vec::new();
        for (i, n) in npcs.iter().enumerate() {
            match npc_groups.last_mut() {
                Some(g) if g.name == n.name && g.combat_level == n.combat_level => {
                    g.members.push(i);
                    g.has_stats |= n.has_stats;
                }
                _ => npc_groups.push(NpcGroup {
                    name: n.name.clone(),
                    name_lower: n.name.to_lowercase(),
                    combat_level: n.combat_level,
                    members: vec![i],
                    has_stats: n.has_stats,
                }),
            }
        }

        let mut areas: Vec<AreaEntry> = osrs_cache::minimap::map_functions(base)?
            .into_iter()
            .filter_map(|(id, (sprite, name))| Some(AreaEntry { id: u16::try_from(id).ok()?, name, sprite }))
            .collect();
        areas.sort_by_key(|a| a.id);
        Ok(Catalog { defs, objects, resources, npcs, npc_index, npc_by_cache, npc_groups, npc_defs, areas })
    }

    /// The palette entry a spawn's `npc` key names.
    pub fn npc(&self, key: &str) -> Option<&NpcEntry> {
        // a cache NPC whose model has since become a curated monster's own is listed under the
        // curated id, so it is also found by its cache id
        let i = self.npc_index.get(key).or_else(|| self.npc_by_cache.get(&cache_npc_id(key)?))?;
        Some(&self.npcs[*i])
    }

    /// The NPCs of palette row `group`.
    pub fn group_members(&self, group: usize) -> impl Iterator<Item = &NpcEntry> {
        self.npc_groups.get(group).into_iter().flat_map(|g| g.members.iter().map(|&i| &self.npcs[i]))
    }

    /// A spawn's NPC's name, or its key when the palette does not know it.
    pub fn npc_name(&self, key: &str) -> String {
        self.npc(key).map_or_else(|| key.to_owned(), |n| n.name.clone())
    }

    pub fn object_name(&self, id: u32) -> String {
        self.defs.get(&id).map_or_else(|| format!("object {id}"), display_name)
    }

    pub fn area_name(&self, id: u16) -> String {
        self.areas.iter().find(|a| a.id == id).map_or_else(|| format!("icon {id}"), |a| area_label(a))
    }
}

/// `name` without the game's text markup (`<col=00efef>Portal</col>` is "Portal").
pub fn strip_tags(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut in_tag = false;
    for c in name.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_owned()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_is_taken_out_of_names() {
        assert_eq!(strip_tags("<col=00efef>Portal</col>"), "Portal");
        assert_eq!(strip_tags("Hans"), "Hans");
        assert_eq!(strip_tags("a < b"), "a");
    }
}
