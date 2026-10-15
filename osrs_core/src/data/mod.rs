//! The data-driven half of the simulator: serde definitions for items, sets, prayers, spells,
//! monsters, consumables and gathering (tools and resource nodes), a `Registry` that loads and cross-checks them, and the bundled OSRS
//! dataset (`Registry::builtin`). Nothing here knows how a tick runs; `sim` turns definitions
//! into live entities.

mod consumables;
mod firemaking;
mod doors;
mod gathering;
mod items;
mod npcs;
mod spawns;
mod prayers;
mod scenario;
mod spells;

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub use consumables::{ConsumableDef, ConsumableEffect};
pub use firemaking::LogDef;
pub use doors::{DoorDef, DoorLeafDef, DoorSwing, LeafMove};
pub use gathering::{DepletionDef, GatheringNodeDef, NodeLocDef, PetDropDef, Requirement, ToolDef};
pub use items::{AmmoDef, AmmoWeight, EquipSlot, ItemDef, SetDef, SpecialAttackDef, StanceDef, StanceKind, WeaponDef};
pub use npcs::{
    cache_npc_def, cache_npc_id, cache_npc_key, cache_npc_variant, AggressionDef, CombatAnimations, HealDef, NpcAttackDef, NpcDef,
    NpcLevels, NpcMovementDef, NpcSounds, RotationDef, SummonDef,
};
pub use spawns::{
    default_npc_spawns, read_json_file, read_npc_spawns, resolve_spawns, write_npc_spawns, CacheNpc, NpcSpawnPoint, ResolvedSpawns,
};
pub use prayers::PrayerDef;
pub use scenario::{builtin_scenarios, load_scenario, InventoryStack, NpcSpawn, PlayerSetup, ScenarioDef, SkillLevelsDef};
pub use spells::{LevelScaling, SpellDef};

use crate::combat::StatusEffectDefinition;

/// A status effect an attack or spell applies on hit, in data form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnHitDef {
    Poison { severity: i32 },
    Venom,
    Freeze { ticks: i32 },
    Stun { ticks: i32 },
    DisableRunning { ticks: i32 },
    Burn { damage_per_tick: i32, ticks: i32 },
    Corruption { prayer_drain_per_tick: i32, ticks: i32 },
}

impl OnHitDef {
    /// `Err` instead of the panic the status constructors would raise on non-positive numbers.
    pub fn to_status(self) -> Result<StatusEffectDefinition, String> {
        let positive = |name: &str, value: i32| if value > 0 { Ok(()) } else { Err(format!("{name} must be positive, was {value}")) };
        Ok(match self {
            OnHitDef::Poison { severity } => {
                positive("poison severity", severity)?;
                StatusEffectDefinition::poison(severity)
            }
            OnHitDef::Venom => StatusEffectDefinition::venom(),
            OnHitDef::Freeze { ticks } => {
                positive("freeze ticks", ticks)?;
                StatusEffectDefinition::freeze(ticks)
            }
            OnHitDef::Stun { ticks } => {
                positive("stun ticks", ticks)?;
                StatusEffectDefinition::stun(ticks)
            }
            OnHitDef::DisableRunning { ticks } => {
                positive("disable-running ticks", ticks)?;
                StatusEffectDefinition::disable_running(ticks)
            }
            OnHitDef::Burn { damage_per_tick, ticks } => {
                positive("burn damage", damage_per_tick)?;
                positive("burn ticks", ticks)?;
                StatusEffectDefinition::burn(damage_per_tick, ticks)
            }
            OnHitDef::Corruption { prayer_drain_per_tick, ticks } => {
                positive("corruption drain", prayer_drain_per_tick)?;
                positive("corruption ticks", ticks)?;
                StatusEffectDefinition::corruption(prayer_drain_per_tick, ticks)
            }
        })
    }
}

/// Serde default for a count of one.
pub(crate) fn one_u32() -> u32 {
    1
}

/// The on-disk shape: one JSON document may hold any mix of definition kinds.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DataFile {
    pub items: Vec<ItemDef>,
    pub sets: Vec<SetDef>,
    pub prayers: Vec<PrayerDef>,
    pub spells: Vec<SpellDef>,
    pub npcs: Vec<NpcDef>,
    pub consumables: Vec<ConsumableDef>,
    pub tools: Vec<ToolDef>,
    pub gathering: Vec<GatheringNodeDef>,
    pub logs: Vec<LogDef>,
    pub doors: Vec<DoorDef>,
}

#[derive(Debug)]
pub enum DataError {
    Parse { source: String, error: serde_json::Error },
    Io { path: String, error: std::io::Error },
    Duplicate { kind: &'static str, id: String },
    Invalid(Vec<String>),
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DataError::Parse { source, error } => write!(f, "{source}: {error}"),
            DataError::Io { path, error } => write!(f, "{path}: {error}"),
            DataError::Duplicate { kind, id } => write!(f, "duplicate {kind} id '{id}'"),
            DataError::Invalid(problems) => write!(f, "invalid data:\n  {}", problems.join("\n  ")),
        }
    }
}

impl std::error::Error for DataError {}

/// Every definition, by id. Iteration order is by id, so anything derived from it is deterministic.
#[derive(Debug, Default, Clone)]
pub struct Registry {
    items: BTreeMap<String, ItemDef>,
    sets: BTreeMap<String, SetDef>,
    prayers: BTreeMap<String, PrayerDef>,
    spells: BTreeMap<String, SpellDef>,
    npcs: BTreeMap<String, NpcDef>,
    consumables: BTreeMap<String, ConsumableDef>,
    /// By item id.
    tools: BTreeMap<String, ToolDef>,
    gathering: BTreeMap<String, GatheringNodeDef>,
    /// By item id.
    logs: BTreeMap<String, LogDef>,
    doors: BTreeMap<String, DoorDef>,
}

fn insert_all<T>(
    map: &mut BTreeMap<String, T>,
    kind: &'static str,
    list: Vec<T>,
    id_of: impl Fn(&T) -> &str,
) -> Result<(), DataError> {
    for definition in list {
        let id = id_of(&definition).to_owned();
        if map.insert(id.clone(), definition).is_some() {
            return Err(DataError::Duplicate { kind, id });
        }
    }
    Ok(())
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parses one JSON document and adds its definitions. Ids must be unique across everything
    /// loaded so far; call `validate` once all documents are in.
    pub fn merge_json(&mut self, source_name: &str, json: &str) -> Result<(), DataError> {
        let file: DataFile =
            serde_json::from_str(json).map_err(|error| DataError::Parse { source: source_name.to_owned(), error })?;
        self.merge(file)
    }

    pub fn merge(&mut self, file: DataFile) -> Result<(), DataError> {
        insert_all(&mut self.items, "item", file.items, |d| &d.id)?;
        insert_all(&mut self.sets, "set", file.sets, |d| &d.id)?;
        insert_all(&mut self.prayers, "prayer", file.prayers, |d| &d.id)?;
        insert_all(&mut self.spells, "spell", file.spells, |d| &d.id)?;
        insert_all(&mut self.npcs, "npc", file.npcs, |d| &d.id)?;
        insert_all(&mut self.consumables, "consumable", file.consumables, |d| &d.id)?;
        insert_all(&mut self.tools, "tool", file.tools, |d| &d.item)?;
        insert_all(&mut self.gathering, "gathering node", file.gathering, |d| &d.id)?;
        insert_all(&mut self.logs, "log", file.logs, |d| &d.item)?;
        insert_all(&mut self.doors, "door", file.doors, |d| &d.id)?;
        Ok(())
    }

    /// Loads and merges every `*.json` file directly inside `dir` (sorted by name), then validates.
    pub fn load_dir(dir: &Path) -> Result<Self, DataError> {
        let io_error = |path: &Path, error| DataError::Io { path: path.display().to_string(), error };
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| io_error(dir, e))?
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        paths.sort();

        let mut registry = Self::new();
        for path in paths {
            let text = std::fs::read_to_string(&path).map_err(|e| io_error(&path, e))?;
            registry.merge_json(&path.display().to_string(), &text)?;
        }
        registry.validate()?;
        Ok(registry)
    }

    /// The bundled OSRS dataset (`osrs_core/data/*.json`), validated.
    pub fn builtin() -> Self {
        let mut registry = Self::new();
        for (name, json) in [
            ("items.json", include_str!("../../data/items.json")),
            ("items_wiki.json", include_str!("../../data/items_wiki.json")),
            ("sets.json", include_str!("../../data/sets.json")),
            ("prayers.json", include_str!("../../data/prayers.json")),
            ("spells.json", include_str!("../../data/spells.json")),
            ("consumables.json", include_str!("../../data/consumables.json")),
            ("npcs.json", include_str!("../../data/npcs.json")),
            ("tools.json", include_str!("../../data/tools.json")),
            ("gathering.json", include_str!("../../data/gathering.json")),
            ("firemaking.json", include_str!("../../data/firemaking.json")),
            ("doors.json", include_str!("../../data/doors.json")),
        ] {
            registry.merge_json(name, json).unwrap_or_else(|e| panic!("bundled data is malformed: {e}"));
        }
        registry.validate().unwrap_or_else(|e| panic!("bundled data is inconsistent: {e}"));
        registry
    }

    pub fn item(&self, id: &str) -> Option<&ItemDef> {
        self.items.get(id)
    }

    pub fn set(&self, id: &str) -> Option<&SetDef> {
        self.sets.get(id)
    }

    pub fn prayer(&self, id: &str) -> Option<&PrayerDef> {
        self.prayers.get(id)
    }

    pub fn spell(&self, id: &str) -> Option<&SpellDef> {
        self.spells.get(id)
    }

    pub fn npc(&self, id: &str) -> Option<&NpcDef> {
        self.npcs.get(id)
    }

    /// The curated monster that cache NPC `id` is (drawn as it, or listing it in `cache_ids`), if
    /// `npcs.json` has one: a spawn of `cache:<id>` fights with its stats.
    pub fn npc_for_cache_id(&self, id: u32) -> Option<&NpcDef> {
        self.npcs.values().find(|d| npcs::covers_cache_npc(d, id))
    }

    /// Replaces (or adds) a monster definition: tests tweak a bundled one this way.
    #[cfg(test)]
    pub(crate) fn insert_npc_for_test(&mut self, def: NpcDef) {
        self.npcs.insert(def.id.clone(), def);
    }

    pub fn consumable(&self, id: &str) -> Option<&ConsumableDef> {
        self.consumables.get(id)
    }

    pub fn items(&self) -> impl Iterator<Item = &ItemDef> {
        self.items.values()
    }

    pub fn sets(&self) -> impl Iterator<Item = &SetDef> {
        self.sets.values()
    }

    pub fn prayers(&self) -> impl Iterator<Item = &PrayerDef> {
        self.prayers.values()
    }

    pub fn spells(&self) -> impl Iterator<Item = &SpellDef> {
        self.spells.values()
    }

    pub fn npcs(&self) -> impl Iterator<Item = &NpcDef> {
        self.npcs.values()
    }

    /// Every cache sound effect id the data names (weapons, stances, spells, prayers, monsters,
    /// their attacks, consumables, doors, gathering nodes) and the game's own (`sim::sounds`), sorted and without repeats.
    pub fn named_sounds(&self) -> Vec<u32> {
        let mut sounds: std::collections::BTreeSet<u32> = crate::sim::sounds::ALL.into_iter().collect();
        for item in self.items() {
            if let Some(weapon) = &item.weapon {
                sounds.extend(weapon.attack_sound);
                sounds.extend(weapon.stances.iter().filter_map(|stance| stance.sound));
            }
        }
        for spell in self.spells() {
            sounds.extend([spell.cast_sound, spell.hit_sound].into_iter().flatten());
        }
        sounds.extend(self.prayers().filter_map(|prayer| prayer.sound));
        sounds.extend(self.consumables().filter_map(|consumable| consumable.sound));
        sounds.extend(self.doors().flat_map(|door| [door.open_sound, door.close_sound]).flatten());
        sounds.extend(self.gathering_nodes().flat_map(|node| [node.attempt_sound, node.depleted_sound]).flatten());
        for npc in self.npcs() {
            sounds.extend([npc.sounds.attack, npc.sounds.hit, npc.sounds.death].into_iter().flatten());
            for attack in &npc.attacks {
                sounds.extend([attack.sound, attack.resolve_sound].into_iter().flatten());
            }
        }
        sounds.into_iter().collect()
    }

    pub fn consumables(&self) -> impl Iterator<Item = &ConsumableDef> {
        self.consumables.values()
    }

    /// The tool that item `item` is, if it is one.
    pub fn tool(&self, item: &str) -> Option<&ToolDef> {
        self.tools.get(item)
    }

    pub fn tools(&self) -> impl Iterator<Item = &ToolDef> {
        self.tools.values()
    }

    pub fn gathering_node(&self, id: &str) -> Option<&GatheringNodeDef> {
        self.gathering.get(id)
    }

    pub fn gathering_nodes(&self) -> impl Iterator<Item = &GatheringNodeDef> {
        self.gathering.values()
    }

    /// The node that choosing `action` on scenery object `loc` gathers from, if any.
    pub fn gathering_node_for(&self, loc: u32, action: &str) -> Option<&GatheringNodeDef> {
        self.gathering.values().find(|node| node.action.eq_ignore_ascii_case(action) && node.depleted_form(loc).is_some())
    }

    /// The firemaking definition of the log item `item`, if it can be lit.
    pub fn log(&self, item: &str) -> Option<&LogDef> {
        self.logs.get(item)
    }

    pub fn doors(&self) -> impl Iterator<Item = &DoorDef> {
        self.doors.values()
    }

    pub fn door(&self, id: &str) -> Option<&DoorDef> {
        self.doors.get(id)
    }

    /// The door that scenery object `object` is a leaf of (in either state). A closed form
    /// belongs to one door; an open form shared by several gives the first.
    pub fn door_for(&self, object: u32) -> Option<&DoorDef> {
        self.doors.values().find(|door| door.state_of(object).is_some())
    }

    /// Adds (or replaces) a door: tests build doors the bundle does not have this way.
    #[cfg(test)]
    pub(crate) fn insert_door_for_test(&mut self, def: DoorDef) {
        self.doors.insert(def.id.clone(), def);
    }

    /// Adds (or replaces) a gathering node: tests build nodes the bundle does not have this way.
    #[cfg(test)]
    pub(crate) fn insert_gathering_node_for_test(&mut self, def: GatheringNodeDef) {
        self.gathering.insert(def.id.clone(), def);
    }

    /// Cross-checks the whole registry and reports every problem at once (not just the first), so
    /// a data author fixes a file in one pass.
    pub fn validate(&self) -> Result<(), DataError> {
        let mut problems = Vec::new();

        // a cache NPC is one monster at most, or its spawns' stats would depend on lookup order
        let mut claimed: BTreeMap<u32, &str> = BTreeMap::new();
        for npc in self.npcs.values() {
            for id in npc.appearance.iter().chain(&npc.cache_ids) {
                if let Some(other) = claimed.insert(*id, &npc.id) {
                    if other != npc.id {
                        problems.push(format!("npc '{}': cache NPC {id} is also '{other}'", npc.id));
                    }
                }
            }
        }

        for item in self.items.values() {
            let who = format!("item '{}'", item.id);
            if let Some(weapon) = &item.weapon {
                if item.slot != Some(EquipSlot::Weapon) {
                    problems.push(format!("{who}: has weapon data but is not in the weapon slot"));
                }
                if weapon.attack_speed < 1 {
                    problems.push(format!("{who}: attack_speed must be at least 1"));
                }
                if weapon.attack_range < 1 {
                    problems.push(format!("{who}: attack_range must be at least 1"));
                }
                if weapon.stances.is_empty() && weapon.powered.is_none() && !weapon.can_cast {
                    problems.push(format!("{who}: a weapon needs at least one stance"));
                }
                for stance in &weapon.stances {
                    if let Some(damage_type) = stance.damage_type {
                        if weapon.powered.is_none() && !weapon.can_cast && weapon.attack_range == 1 && damage_type.style() != crate::combat::AttackStyle::Melee {
                            problems.push(format!("{who}: melee stance '{}' uses non-melee damage type", stance.name));
                        }
                    }
                }
                if let Some(special) = &weapon.special {
                    if !(1..=100).contains(&special.energy_cost) {
                        problems.push(format!("{who}: special energy_cost must be within 1..=100"));
                    }
                    if special.hit_count < 1 {
                        problems.push(format!("{who}: special hit_count must be at least 1"));
                    }
                }
            }
            if item.ammo.is_some() && item.slot != Some(EquipSlot::Ammo) && item.slot != Some(EquipSlot::Weapon) {
                problems.push(format!("{who}: ammo data on a non-ammo item"));
            }
        }

        for set in self.sets.values() {
            for (index, alternatives) in set.pieces.iter().enumerate() {
                if alternatives.is_empty() {
                    problems.push(format!("set '{}': piece {index} lists no items", set.id));
                }
                for item_id in alternatives {
                    if !self.items.contains_key(item_id) {
                        problems.push(format!("set '{}': unknown item '{item_id}'", set.id));
                    }
                }
            }
        }

        for prayer in self.prayers.values() {
            if prayer.drain_effect < 0 {
                problems.push(format!("prayer '{}': drain_effect must not be negative", prayer.id));
            }
        }

        for spell in self.spells.values() {
            if spell.cast_speed < 1 {
                problems.push(format!("spell '{}': cast_speed must be at least 1", spell.id));
            }
            if let Some(on_hit) = spell.on_hit {
                if let Err(message) = on_hit.to_status() {
                    problems.push(format!("spell '{}': {message}", spell.id));
                }
            }
        }

        for npc in self.npcs.values() {
            let who = format!("npc '{}'", npc.id);
            if npc.hitpoints < 1 {
                problems.push(format!("{who}: hitpoints must be at least 1"));
            }
            if npc.size < 1 {
                problems.push(format!("{who}: size must be at least 1"));
            }
            let levels = npc.levels;
            if [levels.attack, levels.strength, levels.defence, levels.ranged, levels.magic].iter().any(|l| *l < 1) {
                problems.push(format!("{who}: every level must be at least 1"));
            }
            for attack in &npc.attacks {
                if attack.speed < 1 {
                    problems.push(format!("{who}: attack '{}' speed must be at least 1", attack.name));
                }
                if attack.hit_count < 1 {
                    problems.push(format!("{who}: attack '{}' hit_count must be at least 1", attack.name));
                }
                if !(0.0..=1.0).contains(&attack.prayer_penetration) {
                    problems.push(format!("{who}: attack '{}' prayer_penetration must be within 0..=1", attack.name));
                }
                if let Some(damage_type) = attack.damage_type {
                    if damage_type.style() != attack.style {
                        problems.push(format!("{who}: attack '{}' damage type does not match its style", attack.name));
                    }
                }
                if let Some(on_hit) = attack.on_hit {
                    if let Err(message) = on_hit.to_status() {
                        problems.push(format!("{who}: attack '{}': {message}", attack.name));
                    }
                }
            }
            if npc.rotation == RotationDef::Weighted && npc.attacks.iter().all(|a| a.weight <= 0.0) && !npc.attacks.is_empty() {
                problems.push(format!("{who}: a weighted rotation needs at least one positive weight"));
            }
            for summon in &npc.summons {
                if !self.npcs.contains_key(&summon.npc) {
                    problems.push(format!("{who}: summons unknown monster '{}'", summon.npc));
                }
                if summon.count < 1 || summon.below_hitpoints < 1 {
                    problems.push(format!("{who}: a summon needs a count and a hitpoint threshold of at least 1"));
                }
            }
            if let Some(heal) = npc.heals_summoner {
                if heal.amount < 1 || heal.every_ticks < 1 || heal.range < 0 || heal.forget_player_beyond < 1 {
                    problems.push(format!("{who}: heals_summoner needs a positive amount, interval and leash, and a range of at least 0"));
                }
            }
        }

        for consumable in self.consumables.values() {
            if consumable.delay_ticks < 0 || consumable.attack_delay_ticks < 0 {
                problems.push(format!("consumable '{}': delays must not be negative", consumable.id));
            }
            if let Some(left) = &consumable.leaves {
                if !self.items.contains_key(left) && !self.consumables.contains_key(left) {
                    problems.push(format!("consumable '{}': leaves unknown item '{left}'", consumable.id));
                }
            }
        }

        for tool in self.tools.values() {
            if !self.items.contains_key(&tool.item) {
                problems.push(format!("tool '{}': unknown item", tool.item));
            }
            if !(1..=99).contains(&tool.level) {
                problems.push(format!("tool '{}': level must be within 1..=99", tool.item));
            }
        }

        for log in self.logs.values() {
            let who = format!("log '{}'", log.item);
            if !self.items.contains_key(&log.item) {
                problems.push(format!("{who}: unknown item"));
            }
            if !(1..=99).contains(&log.level) {
                problems.push(format!("{who}: level must be within 1..=99"));
            }
            if log.xp_tenths < 0 || log.burn_ticks < 1 {
                problems.push(format!("{who}: needs xp_tenths >= 0 and burn_ticks >= 1"));
            }
        }

        let mut node_locs = BTreeMap::new();
        for node in self.gathering.values() {
            let who = format!("gathering node '{}'", node.id);
            if node.locs.is_empty() {
                problems.push(format!("{who}: lists no objects"));
            }
            for loc in &node.locs {
                if let Some(other) = node_locs.insert((loc.loc, node.action.to_ascii_lowercase()), &node.id) {
                    problems.push(format!("{who}: object {} '{}' is already gathered by '{other}'", loc.loc, node.action));
                }
            }
            if node.roll_interval_ticks < 1 {
                problems.push(format!("{who}: roll_interval_ticks must be at least 1"));
            }
            if node.xp_tenths < 0 {
                problems.push(format!("{who}: xp_tenths must not be negative"));
            }
            if !self.items.contains_key(&node.product) && !self.consumables.contains_key(&node.product) {
                problems.push(format!("{who}: unknown product '{}'", node.product));
            }
            if node.chances.is_empty() {
                problems.push(format!("{who}: lists no tools in chances"));
            }
            for (tool, [low, high]) in &node.chances {
                if !self.tools.contains_key(tool) {
                    problems.push(format!("{who}: chances name '{tool}', which is not a tool"));
                }
                if *low < 0 || high < low {
                    problems.push(format!("{who}: chances for '{tool}' need 0 <= low <= high"));
                }
            }
            match node.depletion {
                DepletionDef::Timer { ticks } if ticks < 1 => {
                    problems.push(format!("{who}: a despawn timer needs at least 1 tick"));
                }
                DepletionDef::Chance { one_in } if one_in < 1 => {
                    problems.push(format!("{who}: a depletion chance needs one_in >= 1"));
                }
                _ => {}
            }
            let [min, max] = node.respawn_ticks;
            if min < 1 || max < min {
                problems.push(format!("{who}: respawn_ticks need 1 <= min <= max"));
            }
            if let Some(pet) = &node.pet {
                if !self.items.contains_key(&pet.item) {
                    problems.push(format!("{who}: unknown pet item '{}'", pet.item));
                }
                if pet.base_chance < 1 {
                    problems.push(format!("{who}: pet base_chance must be at least 1"));
                }
            }
            for requirement in &node.requirements {
                if let Requirement::Tool { category, .. } = requirement {
                    if !self.tools.values().any(|t| &t.category == category) {
                        problems.push(format!("{who}: requires a {category}, but no tool is one"));
                    }
                }
            }
        }

        // a closed form belongs to one door (both leaves of a pair may be the same object); an
        // open form may be shared (several doors open into the same object), but is never also
        // a closed form
        let mut closed_in = BTreeMap::new();
        let mut open_forms = std::collections::BTreeSet::new();
        for door in self.doors.values() {
            let who = format!("door '{}'", door.id);
            if door.leaves.len() != door.swing.leaf_count() {
                problems.push(format!("{who}: a {:?} door has {} leaves, not {}", door.swing, door.leaves.len(), door.swing.leaf_count()));
            }
            for leaf in &door.leaves {
                if leaf.closed == leaf.open {
                    problems.push(format!("{who}: object {} is both closed and open", leaf.closed));
                }
                if let Some(other) = closed_in.insert(leaf.closed, &door.id) {
                    if other != &door.id {
                        problems.push(format!("{who}: object {} already closes '{other}'", leaf.closed));
                    }
                }
                open_forms.insert(leaf.open);
            }
        }
        for object in open_forms.iter().filter(|object| closed_in.contains_key(*object)) {
            problems.push(format!("door object {object} is both a closed and an open form"));
        }

        if problems.is_empty() {
            Ok(())
        } else {
            Err(DataError::Invalid(problems))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duplicate_ids_are_rejected_across_documents() {
        let mut registry = Registry::new();
        registry.merge_json("a", r#"{"items":[{"id":"x","name":"X","slot":"head"}]}"#).unwrap();

        let error = registry.merge_json("b", r#"{"items":[{"id":"x","name":"X2","slot":"head"}]}"#).unwrap_err();

        assert!(matches!(error, DataError::Duplicate { kind: "item", .. }));
    }

    #[test]
    fn validation_reports_every_problem_at_once() {
        let mut registry = Registry::new();
        registry
            .merge_json(
                "bad",
                r#"{
                    "sets":[{"id":"s","pieces":[["ghost"]]}],
                    "npcs":[{"id":"n","hitpoints":0,"attacks":[{"name":"A","speed":0}]}]
                }"#,
            )
            .unwrap();

        let DataError::Invalid(problems) = registry.validate().unwrap_err() else { panic!("expected Invalid") };

        assert!(problems.iter().any(|p| p.contains("unknown item 'ghost'")));
        assert!(problems.iter().any(|p| p.contains("hitpoints")));
        assert!(problems.iter().any(|p| p.contains("speed")));
    }

    #[test]
    fn parse_errors_name_their_source() {
        let error = Registry::new().merge_json("items.json", "{ nope").unwrap_err();

        assert!(error.to_string().starts_with("items.json:"));
    }

    #[test]
    fn on_hit_data_converts_to_status_effects_without_panicking_on_bad_numbers() {
        assert!(OnHitDef::Poison { severity: 20 }.to_status().is_ok());
        assert!(OnHitDef::Burn { damage_per_tick: 0, ticks: 3 }.to_status().is_err());
        assert!(OnHitDef::Freeze { ticks: -1 }.to_status().is_err());
    }

    #[test]
    fn an_empty_registry_validates() {
        assert!(Registry::new().validate().is_ok());
    }

    #[test]
    fn the_bundled_dataset_loads_and_validates() {
        let registry = Registry::builtin();

        assert!(registry.items().count() > 30);
        assert!(registry.prayers().count() >= 25);
        assert!(registry.spells().count() >= 25);
        assert!(registry.npcs().count() >= 8);
        assert!(registry.consumables().count() >= 15);
        assert!(registry.tools().count() >= 9);
        assert!(registry.gathering_nodes().count() >= 1);
        assert!(registry.doors().count() >= 1);
    }

    #[test]
    fn doors_pair_their_closed_and_open_objects() {
        let registry = Registry::builtin();

        let door = registry.door_for(1535).expect("1535 is a door");
        assert_eq!((door.state_of(1535), door.state_of(1536)), (Some(false), Some(true)));
        assert_eq!(registry.door_for(1536).map(|d| &d.id), Some(&door.id));
        assert_eq!(registry.door_for(1524).map(|d| d.swing), Some(DoorSwing::Double));
        assert!(registry.door_for(1276).is_none(), "a tree is not a door");
    }

    #[test]
    fn doors_may_share_an_open_form_but_not_a_closed_one() {
        let mut registry = Registry::new();
        registry
            .merge_json(
                "doors",
                r#"{"doors":[
                    {"id":"a","swing":"single","leaves":[{"closed":1,"open":9}]},
                    {"id":"b","swing":"single","leaves":[{"closed":2,"open":9}]},
                    {"id":"pair","swing":"gate","leaves":[{"closed":3,"open":8},{"closed":3,"open":8}]}
                ]}"#,
            )
            .unwrap();
        assert!(registry.validate().is_ok(), "a shared open form and a pair of identical leaves are fine");

        registry.merge_json("more", r#"{"doors":[{"id":"c","swing":"single","leaves":[{"closed":1,"open":7}]}]}"#).unwrap();
        let DataError::Invalid(problems) = registry.validate().unwrap_err() else { panic!("expected Invalid") };
        assert!(problems.iter().any(|p| p.contains("object 1 already closes")), "{problems:?}");
    }

    #[test]
    fn a_door_with_the_wrong_number_of_leaves_or_a_shared_object_is_reported() {
        let mut registry = Registry::new();
        registry
            .merge_json(
                "bad",
                r#"{"doors":[
                    {"id":"a","swing":"double","leaves":[{"closed":1,"open":2}]},
                    {"id":"b","swing":"single","leaves":[{"closed":2,"open":3}]}
                ]}"#,
            )
            .unwrap();

        let DataError::Invalid(problems) = registry.validate().unwrap_err() else { panic!("expected Invalid") };

        assert!(problems.iter().any(|p| p.contains("has 1 leaves, not 2")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("object 2 is both a closed and an open form")), "{problems:?}");
    }

    #[test]
    fn the_regular_tree_is_chopped_with_axes_into_logs() {
        let registry = Registry::builtin();

        let tree = registry.gathering_node_for(1276, "Chop down").expect("object 1276 is a tree");
        assert_eq!(tree.skill, crate::model::Skill::Woodcutting);
        assert_eq!(tree.product, "logs");
        assert_eq!(tree.xp_tenths, 250);
        assert_eq!(tree.depletion, DepletionDef::EverySuccess);
        assert_eq!(tree.chances["bronze_axe"], [64, 200]);
        assert_eq!(tree.chances["crystal_axe"], [250, 800]);
        assert_eq!(tree.pet.as_ref().map(|p| (p.item.as_str(), p.base_chance)), Some(("beaver", 317_647)));
        assert!(registry.gathering_node_for(1276, "Examine").is_none());
        assert_eq!(registry.gathering_node_for(1282, "Chop down").map(|n| n.id.as_str()), Some("regular_tree"), "a dead tree");
    }

    #[test]
    fn the_other_trees_follow_the_wiki() {
        let registry = Registry::builtin();
        let node = |loc, action| registry.gathering_node_for(loc, action).unwrap_or_else(|| panic!("object {loc} is a tree"));

        let oak = node(10820, "Chop down");
        assert_eq!((oak.product.as_str(), oak.xp_tenths, oak.depletion), ("oak_logs", 375, DepletionDef::Timer { ticks: 45 }));
        assert_eq!(oak.chances["crystal_axe"], [125, 400]);
        assert_eq!(oak.requirements[0].refusal(), "You need a Woodcutting level of 15 to chop down this tree.");

        let juniper = node(27499, "Chop down");
        assert_eq!(juniper.depletion, DepletionDef::Chance { one_in: 16 });
        assert_eq!(node(29668, "Cut").product, "redwood_logs");
        assert_eq!(node(3037, "Cut down").product, "arctic_pine_logs");
        assert_eq!(node(2023, "Chop").product, "achey_tree_logs");
        assert_eq!(node(10822, "Chop down").pet.as_ref().map(|p| p.base_chance), Some(145_013));
        assert!(registry.gathering_nodes().all(|n| n.attempt_sound == Some(2735) && n.depleted_sound == Some(2734)));
        assert_eq!(registry.tool("rune_axe").map(|t| t.level), Some(41));
        assert!(registry.item("logs").is_some_and(|i| i.slot.is_none()));
    }

    #[test]
    fn a_node_naming_an_unknown_tool_or_product_is_reported() {
        let mut registry = Registry::new();
        registry
            .merge_json(
                "bad",
                r#"{
                    "items":[{"id":"stick","name":"Stick","slot":"weapon"}],
                    "tools":[{"item":"stick","category":"wand","skill":"woodcutting","level":1}],
                    "gathering":[{"id":"t","skill":"woodcutting","action":"Chop down","locs":[{"loc":1,"depleted":2}],
                        "requirements":[{"tool":{"category":"axe"}}],"xp_tenths":10,"product":"ghost_log","chances":{"stick":[1,2],"ghost_axe":[1,2]},
                        "depletion":"every_success","respawn_ticks":[5,1]}]
                }"#,
            )
            .unwrap();

        let DataError::Invalid(problems) = registry.validate().unwrap_err() else { panic!("expected Invalid") };

        assert!(problems.iter().any(|p| p.contains("unknown product 'ghost_log'")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("'ghost_axe', which is not a tool")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("requires a axe, but no tool is one")), "{problems:?}");
        assert!(problems.iter().any(|p| p.contains("respawn_ticks")), "{problems:?}");
    }

    #[test]
    fn bundled_values_match_the_reference_tables() {
        let registry = Registry::builtin();

        let piety = registry.prayer("piety").unwrap();
        assert_eq!((piety.effects.attack_percent, piety.effects.strength_percent, piety.effects.defence_percent), (120, 123, 125));
        assert_eq!(piety.drain_effect, 24);
        assert_eq!(registry.prayer("rigour").unwrap().effects.ranged_strength_percent, 123);
        assert_eq!(registry.prayer("augury").unwrap().effects.magic_damage_permille, 40);
        assert_eq!(registry.prayer("protect_from_melee").unwrap().protects, Some(crate::combat::AttackStyle::Melee));

        let whip = registry.item("abyssal_whip").unwrap();
        assert_eq!(whip.weapon.as_ref().unwrap().attack_speed, 4);
        assert_eq!(whip.bonuses.melee_strength, 82);

        assert_eq!(registry.spell("ice_barrage").unwrap().base_max_hit, 30);
        assert_eq!(registry.spell("fire_blast").unwrap().base_max_hit, 16);
        assert_eq!(registry.npc("tztok_jad").unwrap().attacks[0].max_hit, 97);
        assert_eq!(registry.consumable("prayer_potion_1").unwrap().leaves.as_deref(), Some("vial"));
        assert!(registry.item("vial").unwrap().slot.is_none());
    }

    #[test]
    fn every_set_piece_in_the_bundle_exists() {
        let registry = Registry::builtin();
        assert!(registry.sets().count() >= 7);
    }

    #[test]
    fn a_typo_in_a_data_file_is_an_error_not_a_silent_default() {
        let error = Registry::new().merge_json("typo", r#"{"items":[{"id":"x","name":"X","slot":"head","bonus":{}}]}"#).unwrap_err();

        assert!(error.to_string().contains("unknown field"), "{error}");
    }
}
