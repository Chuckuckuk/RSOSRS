use serde::{Deserialize, Serialize};

use super::OnHitDef;
use crate::combat::AttackStyle;
use crate::model::{AttributeSet, Condition, DamageType, EquipmentBonuses, Modifier};

fn one() -> i32 {
    1
}

/// A monster's skill levels. Monsters list Attack, Strength, Defence, Ranged and Magic; Hitpoints
/// is `NpcDef::hitpoints`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NpcLevels {
    pub attack: i32,
    pub strength: i32,
    pub defence: i32,
    pub ranged: i32,
    pub magic: i32,
}

impl Default for NpcLevels {
    fn default() -> Self {
        Self { attack: 1, strength: 1, defence: 1, ranged: 1, magic: 1 }
    }
}

/// One attack in a monster's repertoire (reference §4: each carries its own max hit and speed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NpcAttackDef {
    pub name: String,
    pub style: AttackStyle,
    pub damage_type: Option<DamageType>,
    pub max_hit: i32,
    /// Ticks between this attack's resolves (OSRS attack speed).
    pub speed: i32,
    /// Tiles; ignored for melee, which needs adjacency.
    pub range: i32,
    /// Ticks of visible wind-up before the roll (the Jad-style archetype); 0 for traditional.
    pub telegraph_ticks: i32,
    /// Ticks between the telegraph ending and the roll (the Doom-style archetype).
    pub impact_delay_ticks: i32,
    /// Ticks between the roll and the damage landing.
    pub projectile_travel_ticks: i32,
    /// 0 = a correct protection prayer blocks it; 1 = ignores prayer ("typeless" in the old
    /// wording); in between = pierces.
    pub prayer_penetration: f32,
    /// False for attacks no protection prayer is meant to answer to (the player cannot react).
    pub protectable: bool,
    /// Relative weight when the monster uses a weighted rotation.
    pub weight: f32,
    pub hit_count: i32,
    pub modifiers: Vec<Modifier>,
    pub on_hit: Option<OnHitDef>,
    /// The cache sequence the monster plays for this attack, when it differs from its usual
    /// `animations.attack` (Jad rears up for magic and stomps for ranged). An attack with a
    /// telegraph starts it as the telegraph begins, so the player can read it and react.
    pub animation: Option<u32>,
    /// A graphic (cache spot animation) that flies from the monster to the target, arriving as the
    /// hit lands (Jad's fireball).
    pub projectile: Option<u32>,
    /// A graphic played on the target as the attack is released (the rocks Jad's stomp drops).
    pub target_graphic: Option<u32>,
    /// A graphic played on the target as the hit lands (a spell's splash).
    pub impact_graphic: Option<u32>,
    /// The cache sound effect of this attack, when it differs from the monster's usual
    /// `sounds.attack` (Jad's breath before his magic). Heard as the attack starts.
    pub sound: Option<u32>,
    /// A cache sound effect heard as the attack resolves (the roll), not as it starts: Jad's
    /// ranged stomp. Only Jad's data uses it.
    pub resolve_sound: Option<u32>,
}

impl Default for NpcAttackDef {
    fn default() -> Self {
        Self {
            name: "Attack".into(),
            style: AttackStyle::Melee,
            damage_type: None,
            max_hit: 1,
            speed: 4,
            range: 1,
            telegraph_ticks: 0,
            impact_delay_ticks: 0,
            projectile_travel_ticks: 0,
            prayer_penetration: 0.0,
            protectable: true,
            weight: 1.0,
            hit_count: one(),
            modifiers: Vec::new(),
            on_hit: None,
            animation: None,
            projectile: None,
            target_graphic: None,
            impact_graphic: None,
            sound: None,
            resolve_sound: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationDef {
    /// Cycles the attack list in order.
    #[default]
    Fixed,
    /// Picks by `weight`.
    Weighted,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NpcMovementDef {
    #[default]
    Chase,
    Stationary,
    MaintainDistance { distance: i32 },
    /// Chases a melee-fighting target, kites anything else.
    StyleReactive { kite_distance: i32 },
}

/// The cache sequences an NPC plays in a fight.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CombatAnimations {
    pub attack: u32,
    pub defend: u32,
    pub death: u32,
}

/// The cache sound effects (archive 4) a monster makes in a fight; any may be missing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NpcSounds {
    /// As it attacks (an attack may name its own instead).
    pub attack: Option<u32>,
    /// As a hit lands on it.
    pub hit: Option<u32>,
    pub death: Option<u32>,
}

/// Helpers a monster calls in when it is hurt (Jad's Yt-HurKot): `count` of monster `npc` appear
/// around it once its hitpoints fall to `below_hitpoints` or less. With `again_once_healed_to_full`
/// the call is answered again, replacing the helpers that have died, each time it falls that low
/// after having been healed back to full health.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SummonDef {
    pub npc: String,
    pub count: i32,
    pub below_hitpoints: i32,
    #[serde(default)]
    pub again_once_healed_to_full: bool,
}

/// A summoned helper that heals the monster that called it: `amount` hitpoints every `every_ticks`
/// ticks while it is within `range` tiles of it, playing `animation` as it does. A helper the
/// player attacks fights the player instead, until the player is more than `forget_player_beyond`
/// tiles away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HealDef {
    pub amount: i32,
    pub every_ticks: i32,
    pub range: i32,
    pub forget_player_beyond: i32,
    #[serde(default)]
    pub animation: Option<u32>,
}

/// Aggression (reference §4): who the monster starts a fight with unprompted.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AggressionDef {
    pub aggressive: bool,
    /// Tiles from the monster within which it notices a player.
    pub range: i32,
    /// Never becomes tolerant of a player who stays near it (Nightmare Zone monsters, Vyres).
    pub ignores_tolerance: bool,
    /// Attacks players of any combat level, instead of only those at most double its own.
    pub ignores_combat_level: bool,
}

impl Default for AggressionDef {
    fn default() -> Self {
        Self { aggressive: false, range: 4, ignores_tolerance: false, ignores_combat_level: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NpcDef {
    pub id: String,
    pub name: String,
    pub combat_level: i32,
    pub hitpoints: i32,
    /// Footprint in tiles (square).
    pub size: i32,
    pub levels: NpcLevels,
    /// The monster's own attack/defence bonuses (its `attack.*` are rolled with, its `defence.*`
    /// defend with); strength bonuses are unused because each attack lists its max hit.
    pub bonuses: EquipmentBonuses,
    pub attributes: AttributeSet,
    pub attacks: Vec<NpcAttackDef>,
    pub rotation: RotationDef,
    pub movement: NpcMovementDef,
    pub aggression: AggressionDef,
    /// Tiles around where it spawned that it strolls within while idle (0 = stays put).
    pub wander_radius: i32,
    /// Whether it stands in the player's way too. Every NPC blocks other NPCs; the player walks
    /// through most of them (and under Jad), so set this only for the ones that should stop him.
    pub blocks_movement: bool,
    /// The game cache's NPC definition a client draws it from (models, animations), if it has one.
    pub appearance: Option<u32>,
    /// The cache sequences it plays when it attacks, blocks and dies.
    pub animations: Option<CombatAnimations>,
    /// What "Examine" says about it (a plain description from its name when absent).
    pub examine: Option<String>,
    /// Which player attacks can hurt it at all, checked against the attack's own flags
    /// (`weapon_category:halberd`, `spell:...`) and style - how Flying monsters refuse non-halberd
    /// melee and Leafy monsters refuse everything but leaf-bladed weapons.
    pub attackable_if: Condition,
    /// `immune_poison`, `immune_venom`, ...
    pub tags: Vec<String>,
    /// Helpers it calls in when hurt.
    pub summons: Vec<SummonDef>,
    /// Set for a helper that heals the monster that summoned it.
    pub heals_summoner: Option<HealDef>,
    pub sounds: NpcSounds,
    /// Free-form provenance (wiki page, whether the numbers are verified).
    pub notes: String,
}

/// How a spawn names an NPC straight from the game cache, by its cache id: `cache:415`. Every NPC
/// the cache has can be placed this way, curated in `npcs.json` or not (see [`cache_npc_def`]).
pub const CACHE_NPC_PREFIX: &str = "cache:";

/// The spawn id of cache NPC `id`.
pub fn cache_npc_key(id: u32) -> String {
    format!("{CACHE_NPC_PREFIX}{id}")
}

/// The cache id a spawn id like `cache:415` names, if it is one.
pub fn cache_npc_id(key: &str) -> Option<u32> {
    key.strip_prefix(CACHE_NPC_PREFIX)?.parse().ok()
}

/// A definition for cache NPC `id` that nobody has written stats for: it looks and is named as
/// the cache says, stands where it is put, and cannot be fought (its combat stats live on the
/// wiki, and are added to `npcs.json` when an NPC needs them; a curated entry whose `appearance`
/// is `id` then takes its place).
pub fn cache_npc_def(id: u32, name: &str, combat_level: Option<u16>, size: u8) -> NpcDef {
    NpcDef {
        id: cache_npc_key(id),
        name: name.to_owned(),
        combat_level: combat_level.map_or(0, i32::from),
        size: i32::from(size.max(1)),
        appearance: Some(id),
        attackable_if: Condition::Not(Box::new(Condition::Always)),
        notes: "From the game cache only: no combat stats yet.".into(),
        ..NpcDef::default()
    }
}

impl Default for NpcDef {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            combat_level: 1,
            hitpoints: 10,
            size: 1,
            levels: NpcLevels::default(),
            bonuses: EquipmentBonuses::default(),
            attributes: AttributeSet::default(),
            attacks: Vec::new(),
            rotation: RotationDef::Fixed,
            movement: NpcMovementDef::Chase,
            aggression: AggressionDef::default(),
            wander_radius: 0,
            blocks_movement: false,
            appearance: None,
            examine: None,
            animations: None,
            attackable_if: Condition::Always,
            tags: Vec::new(),
            summons: Vec::new(),
            heals_summoner: None,
            sounds: NpcSounds::default(),
            notes: String::new(),
        }
    }
}

impl NpcDef {
    pub fn has_tag(&self, tag: &str) -> bool {
        self.tags.iter().any(|t| t == tag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_npc_key_round_trips_and_its_def_cannot_be_fought() {
        assert_eq!(cache_npc_key(3105), "cache:3105");
        assert_eq!(cache_npc_id("cache:3105"), Some(3105));
        assert_eq!(cache_npc_id("man"), None);
        assert_eq!(cache_npc_id("cache:x"), None);

        let def = cache_npc_def(3105, "Hans", None, 0);
        assert_eq!((def.id.as_str(), def.name.as_str(), def.appearance), ("cache:3105", "Hans", Some(3105)));
        assert_eq!((def.combat_level, def.size), (0, 1));
        assert!(def.attacks.is_empty() && !def.aggression.aggressive);
        let ctx = crate::model::ConditionContext {
            style: AttackStyle::Melee,
            damage_type: DamageType::Slash,
            target_attributes: AttributeSet::default(),
            flags: &[],
        };
        assert!(!def.attackable_if.holds(&ctx));
    }

    #[test]
    fn a_curated_monster_is_found_by_the_cache_npc_it_is_drawn_as() {
        let registry = crate::data::Registry::builtin();
        assert_eq!(registry.npc_with_appearance(415).map(|d| d.id.as_str()), Some("abyssal_demon"));
        assert!(registry.npc_with_appearance(3105).is_none());
    }

    #[test]
    fn a_monster_parses_with_sparse_json() {
        let npc: NpcDef = serde_json::from_str(
            r#"{
                "id": "rune_dragon", "name": "Rune dragon", "combat_level": 380, "hitpoints": 330, "size": 4,
                "levels": {"attack": 300, "strength": 300, "defence": 300, "ranged": 1, "magic": 300},
                "attributes": ["draconic", "fiery"],
                "attacks": [
                    {"name": "Bite", "max_hit": 26, "speed": 4},
                    {"name": "Dragonfire", "style": "magic", "max_hit": 50, "speed": 4, "range": 8,
                     "on_hit": {"burn": {"damage_per_tick": 2, "ticks": 5}}}
                ],
                "movement": {"style_reactive": {"kite_distance": 6}},
                "aggression": {"aggressive": true}
            }"#,
        )
        .unwrap();

        assert_eq!(npc.size, 4);
        assert_eq!(npc.attacks.len(), 2);
        assert_eq!(npc.attacks[0].style, AttackStyle::Melee);
        assert_eq!(npc.attacks[1].style, AttackStyle::Magic);
        assert!(npc.attributes.contains(crate::model::Attribute::Draconic));
        assert_eq!(npc.movement, NpcMovementDef::StyleReactive { kite_distance: 6 });
        assert!(npc.aggression.aggressive);
        assert_eq!(npc.aggression.range, 4);
    }
}
