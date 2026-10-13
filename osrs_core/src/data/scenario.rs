use serde::{Deserialize, Serialize};

fn one() -> i32 {
    1
}

/// A player's skill levels in a scenario file. Everything defaults to 1 (hitpoints to 10, the
/// minimum in the game), so a file lists only what matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SkillLevelsDef {
    pub attack: i32,
    pub strength: i32,
    pub defence: i32,
    pub ranged: i32,
    pub magic: i32,
    pub prayer: i32,
    pub hitpoints: i32,
    pub agility: i32,
}

impl Default for SkillLevelsDef {
    fn default() -> Self {
        Self { attack: one(), strength: one(), defence: one(), ranged: one(), magic: one(), prayer: one(), hitpoints: 10, agility: one() }
    }
}

impl SkillLevelsDef {
    pub fn maxed() -> Self {
        Self { attack: 99, strength: 99, defence: 99, ranged: 99, magic: 99, prayer: 99, hitpoints: 99, agility: 99 }
    }
}

/// A stack the player starts with in the inventory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryStack {
    pub item: String,
    #[serde(default = "one_u32")]
    pub quantity: u32,
}

fn one_u32() -> u32 {
    1
}

/// Everything about the player at the start of a scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PlayerSetup {
    pub start: [i32; 2],
    pub levels: SkillLevelsDef,
    /// Item ids to wear, in order (later items replace earlier ones in the same slot).
    pub equipment: Vec<String>,
    /// Prayer ids to switch on, in order.
    pub prayers: Vec<String>,
    /// Consumable ids to use before the scenario starts (potions to pre-boost).
    pub consumables: Vec<String>,
    pub stance: Option<usize>,
    pub spell: Option<String>,
    pub special: bool,
    pub on_slayer_task: bool,
    pub running: bool,
    /// Index into the scenario's `npcs` of the monster the player attacks from tick one.
    pub attack: Option<usize>,
    /// What the player carries, in inventory order (stackables fill one slot, others one each).
    pub inventory: Vec<InventoryStack>,
}

impl Default for PlayerSetup {
    fn default() -> Self {
        Self {
            start: [0, 0],
            levels: SkillLevelsDef::default(),
            equipment: Vec::new(),
            prayers: Vec::new(),
            consumables: Vec::new(),
            stance: None,
            spell: None,
            special: false,
            on_slayer_task: false,
            running: true,
            attack: None,
            inventory: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NpcSpawn {
    pub npc: String,
    pub at: [i32; 2],
    /// Starts out fighting the player instead of waiting to be provoked.
    #[serde(default)]
    pub hostile: bool,
    /// Hitpoints it starts on, when not its full health (to begin partway through a fight).
    #[serde(default)]
    pub hitpoints: Option<i32>,
}

/// A complete, self-contained fight set-up: the floor, the RNG seed, the player and the monsters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioDef {
    pub name: String,
    pub seed: i32,
    /// The floor is `2 * half_extent` tiles square, centred on the origin.
    pub half_extent: i32,
    pub player: PlayerSetup,
    pub npcs: Vec<NpcSpawn>,
    /// The world tile the scenario's (0, 0) stands on when it is played on the real map (the game
    /// client's `--scenario`); every `start` and `at` is relative to it. Headless runs put the
    /// scenario on a flat floor at the origin and ignore this.
    pub location: Option<[i32; 2]>,
    pub notes: String,
}

impl Default for ScenarioDef {
    fn default() -> Self {
        Self {
            name: String::new(),
            seed: 12345,
            half_extent: 15,
            player: PlayerSetup::default(),
            npcs: Vec::new(),
            location: None,
            notes: String::new(),
        }
    }
}

impl ScenarioDef {
    pub fn from_json(source: &str, json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|e| format!("{source}: {e}"))
    }
}

/// Scenarios bundled with the crate, by name.
pub fn builtin_scenarios() -> Vec<(&'static str, &'static str)> {
    vec![
        ("whip_vs_zombie", include_str!("../../data/scenarios/whip_vs_zombie.json")),
        ("range_vs_demon", include_str!("../../data/scenarios/range_vs_demon.json")),
        ("mage_vs_jad", include_str!("../../data/scenarios/mage_vs_jad.json")),
        ("jad", include_str!("../../data/scenarios/jad.json")),
        ("jad_healers", include_str!("../../data/scenarios/jad_healers.json")),
    ]
}

/// A bundled scenario by name, or else a scenario file at that path.
pub fn load_scenario(name_or_path: &str) -> Result<ScenarioDef, String> {
    if let Some((name, json)) = builtin_scenarios().into_iter().find(|(name, _)| *name == name_or_path) {
        return ScenarioDef::from_json(name, json);
    }
    let json = std::fs::read_to_string(name_or_path).map_err(|e| {
        let names: Vec<&str> = builtin_scenarios().into_iter().map(|(name, _)| name).collect();
        format!("'{name_or_path}' is neither a bundled scenario ({}) nor a readable file: {e}", names.join(", "))
    })?;
    ScenarioDef::from_json(name_or_path, &json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_bundled_scenario_parses() {
        for (name, json) in builtin_scenarios() {
            ScenarioDef::from_json(name, json).unwrap_or_else(|e| panic!("{e}"));
        }
    }

    #[test]
    fn unknown_keys_are_rejected() {
        assert!(ScenarioDef::from_json("t", r#"{"playr":{}}"#).is_err());
    }

    #[test]
    fn a_minimal_scenario_gets_sensible_defaults() {
        let scenario = ScenarioDef::from_json("t", r#"{"npcs":[{"npc":"goblin","at":[3,0]}]}"#).unwrap();
        assert_eq!(scenario.seed, 12345);
        assert_eq!(scenario.player.levels.hitpoints, 10);
        assert!(!scenario.npcs[0].hostile);
    }
}
