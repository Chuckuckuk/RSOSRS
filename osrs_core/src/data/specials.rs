use serde::{Deserialize, Serialize};

fn one() -> i32 {
    1
}

/// A hazard an attack of this monster can leave in the world (a boulder, lava), named by `id` in
/// the attacks that leave it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HazardData {
    pub id: String,
    pub name: String,
    /// Stands in the way of line of sight for everything but the monster that left it.
    pub blocks_line_of_sight: bool,
    /// Stands in players' way (monsters walk through it).
    pub blocks_players: bool,
    /// Damage to a player standing on it, each tick.
    pub damage_per_tick: i32,
    /// Ticks until it goes away; absent is permanent.
    pub lifetime_ticks: Option<i32>,
    /// The monster that left it destroys it by walking over it.
    pub cleared_by_owner_walking: bool,
    /// The game cache's object a client draws it as (a boulder).
    pub object: Option<u32>,
    /// The game cache's graphic (spot animation) a client plays on its tile instead (a lava pool);
    /// it wins over `object`.
    pub graphic: Option<u32>,
}

impl Default for HazardData {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Hazard".into(),
            blocks_line_of_sight: false,
            blocks_players: false,
            damage_per_tick: 0,
            lifetime_ticks: None,
            cleared_by_owner_walking: false,
            object: None,
            graphic: None,
        }
    }
}

/// Which tile an area is centred on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnchorData {
    /// The attacker's own tile when the attack was selected.
    #[serde(rename = "self")]
    SelfTile,
    /// The player's tile when the attack was selected.
    #[default]
    Target,
}

/// A random scatter of tiles around the anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScatterData {
    /// How far from the anchor a tile may fall.
    pub radius: i32,
    pub min: usize,
    pub max: usize,
}

/// The tiles an area covers: a `square` of that radius around the anchor, or these `offsets` from
/// it, or a `scatter` of random tiles around it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TilesData {
    pub anchor: AnchorData,
    pub square: Option<i32>,
    pub offsets: Option<Vec<[i32; 2]>>,
    pub scatter: Option<ScatterData>,
    /// The anchor tile is always covered (the first rock lands on the player).
    pub always_include_anchor: bool,
}

/// What an area deals to a player standing on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DamageData {
    Fixed(i32),
    Uniform { max: i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrayerData {
    Melee,
    Ranged,
    Magic,
}

/// The one protection prayer that cuts the damage of an area without negating it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrayerReductionData {
    pub prayer: PrayerData,
    pub penetration: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementData {
    /// One hazard, on the tile the area is centred on.
    #[default]
    Anchor,
    /// One on every tile the area covers.
    EveryCoveredTile,
}

/// A hazard an area leaves where it lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeavesData {
    pub hazard: String,
    #[serde(default)]
    pub placement: PlacementData,
}

/// A special decided by where the player stands: it covers `tiles` and hurts whoever is on them
/// when it lands. It never rolls accuracy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PositionData {
    pub tiles: TilesData,
    pub damage: DamageData,
    #[serde(default)]
    pub prayer_reduction: Option<PrayerReductionData>,
    #[serde(default)]
    pub leaves: Option<LeavesData>,
    /// The cache graphic shown falling on each tile while it waits to land (a rider with a delay).
    #[serde(default)]
    pub falling_graphic: Option<u32>,
}

/// Hazards a shot leaves around the player when it lands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandingHazardsData {
    pub hazard: String,
    pub radius: i32,
    #[serde(default = "one")]
    pub min: i32,
    #[serde(default = "one")]
    pub max: i32,
}

/// A shot that needs a clear line to the player. The hit it makes is the attack's own (its style,
/// max hit, prayer penetration and so on); this is what else it does.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ObstructedShotData {
    /// Holds the player in place until the shot lands.
    pub binds_until_landing: bool,
    pub landing_hazards: Option<LandingHazardsData>,
    /// What goes off on the blocker's tile when the shot is stopped, centred on that tile.
    pub on_blocked: Option<PositionData>,
    /// The cache sound heard where the shot strikes what blocks it.
    pub blocked_sound: Option<u32>,
}

/// A special attack, named by the kind of resolver it uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpecialData {
    Position(PositionData),
    ObstructedShot(ObstructedShotData),
}

/// A special that goes off with an attack, `delay_ticks` after it resolves (none: at once),
/// aimed at where the player stood when it did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiderData {
    #[serde(default)]
    pub delay_ticks: i32,
    pub special: PositionData,
}

/// A special a phase fires on its own clock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeriodicData {
    pub name: String,
    pub every_ticks: i32,
    pub special: PositionData,
}

/// One use of an attack in a phase's cycle: the attack by name, and optionally how long after it
/// resolves the next one may start, for this use only (a 3-2-2 volley).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhaseAttackData {
    pub attack: String,
    #[serde(default)]
    pub speed: Option<i32>,
    #[serde(default)]
    pub weight: Option<f32>,
}

/// One stage of a monster's fight. The first has no `when_hp_below`; each later one begins when the
/// monster's health falls to that fraction of its maximum (0.6 is 60%).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PhaseData {
    pub name: String,
    pub when_hp_below: Option<f32>,
    pub attacks: Vec<PhaseAttackData>,
    pub rotation: Option<super::npcs::RotationDef>,
    /// Replaces the monster's own movement for this phase.
    pub movement: Option<super::npcs::NpcMovementDef>,
    pub periodic: Vec<PeriodicData>,
}
