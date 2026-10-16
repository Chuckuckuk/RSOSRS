/// What a hazard is: a thing a special leaves in the world that lives on its own. It is data, so a
/// boss's attacks can name the hazards they leave (a boulder, lava) and bosses share the pieces.
///
/// What it does is a handful of switches rather than code: it stands in the way of line of sight,
/// it stands in the way of players, it hurts a player standing on it, it ends after a while, and
/// its owner can clear it by walking over it. A hazard never stands in its owner's way or sight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HazardDef {
    pub name: String,
    /// Blocks line of sight for everyone but its owner.
    pub blocks_line_of_sight: bool,
    /// Stands in players' way (NPCs walk through it).
    pub blocks_players: bool,
    /// Damage to a player standing on it, each tick; 0 for none.
    pub damage_per_tick: i32,
    /// Ticks until it goes away; `None` is permanent.
    pub lifetime_ticks: Option<i32>,
    /// Whether its owner destroys it by walking over it.
    pub cleared_by_owner_walking: bool,
}

impl HazardDef {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            blocks_line_of_sight: false,
            blocks_players: false,
            damage_per_tick: 0,
            lifetime_ticks: None,
            cleared_by_owner_walking: false,
        }
    }

    pub fn blocking_line_of_sight(mut self) -> Self {
        self.blocks_line_of_sight = true;
        self
    }

    pub fn blocking_players(mut self) -> Self {
        self.blocks_players = true;
        self
    }

    pub fn hurting(mut self, damage_per_tick: i32) -> Self {
        assert!(damage_per_tick >= 0, "damage_per_tick must not be negative");
        self.damage_per_tick = damage_per_tick;
        self
    }

    pub fn lasting(mut self, ticks: i32) -> Self {
        assert!(ticks >= 1, "a hazard must last at least a tick");
        self.lifetime_ticks = Some(ticks);
        self
    }

    pub fn cleared_by_owner_walking(mut self) -> Self {
        self.cleared_by_owner_walking = true;
        self
    }
}
