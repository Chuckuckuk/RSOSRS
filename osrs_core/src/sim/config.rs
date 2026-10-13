use crate::combat::CombatStats;

/// The five combat levels the Unity components expose as serialized fields. Equipment bonuses do
/// not exist (see `CombatStats`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatLevels {
    pub attack: i32,
    pub strength: i32,
    pub defence: i32,
    pub ranged: i32,
    pub magic: i32,
}

impl CombatLevels {
    pub const fn new(attack: i32, strength: i32, defence: i32, ranged: i32, magic: i32) -> Self {
        Self { attack, strength, defence, ranged, magic }
    }

    /// Panics if any level is below 1 (bad entity data is a programmer error).
    pub fn to_stats(self) -> CombatStats {
        CombatStats::new(self.attack, self.strength, self.defence, self.ranged, self.magic)
            .expect("combat levels must each be at least 1")
    }
}

/// `PlayerMover`'s serialized fields, with the same defaults.
#[derive(Debug, Clone, Copy)]
pub struct PlayerConfig {
    pub footprint_size: i32,
    pub max_health: i32,
    /// No run energy yet and no toggle control - running is simply the resting default.
    /// Ctrl-click moves at the inverse for that one path only.
    pub is_running: bool,
    pub attack_interval_ticks: i32,
    pub max_prayer_points: i32,
    pub levels: CombatLevels,
    /// Agility level, which sets how fast run energy drains and regenerates.
    pub agility_level: i32,
    /// Whether running spends run energy. Off by default so a bare `PlayerConfig` keeps its
    /// original unlimited-running behaviour; the data-driven setups turn it on.
    pub run_energy_enabled: bool,
}

impl PlayerConfig {
    /// A new account: level 1 in every combat skill and prayer, 10 hitpoints.
    pub fn new_account() -> Self {
        Self {
            max_health: 10,
            max_prayer_points: 1,
            levels: CombatLevels::new(1, 1, 1, 1, 1),
            ..Self::default()
        }
    }
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            footprint_size: 1,
            max_health: 10,
            is_running: true,
            attack_interval_ticks: 4,
            max_prayer_points: 99,
            levels: CombatLevels::new(40, 40, 30, 1, 1),
            agility_level: 1,
            run_energy_enabled: false,
        }
    }
}

/// `NpcCombatant`'s serialized fields, with the same defaults.
#[derive(Debug, Clone, Copy)]
pub struct NpcConfig {
    pub footprint_size: i32,
    pub max_health: i32,
    /// False mirrors an unassigned `targetPlayer` reference in the scene: the NPC then never
    /// attacks or chases.
    pub targets_player: bool,
    pub melee_telegraph_ticks: i32,
    pub levels: CombatLevels,
}

impl Default for NpcConfig {
    fn default() -> Self {
        Self {
            footprint_size: 1,
            max_health: 20,
            targets_player: true,
            melee_telegraph_ticks: 2,
            levels: CombatLevels::new(20, 20, 10, 1, 1),
        }
    }
}

/// `BossCombatant`'s serialized fields, with the same defaults.
#[derive(Debug, Clone, Copy)]
pub struct BossConfig {
    pub footprint_size: i32,
    pub max_health: i32,
    pub targets_player: bool,
    pub levels: CombatLevels,
}

impl Default for BossConfig {
    fn default() -> Self {
        Self { footprint_size: 1, max_health: 250, targets_player: true, levels: CombatLevels::new(180, 180, 120, 180, 180) }
    }
}

/// `HealerNpc`'s serialized fields, with the same defaults. Only Defence matters - it never attacks.
#[derive(Debug, Clone, Copy)]
pub struct HealerConfig {
    pub footprint_size: i32,
    pub max_health: i32,
    pub heal_amount_per_tick: i32,
    pub levels: CombatLevels,
}

impl Default for HealerConfig {
    fn default() -> Self {
        Self { footprint_size: 1, max_health: 10, heal_amount_per_tick: 5, levels: CombatLevels::new(1, 1, 10, 1, 1) }
    }
}

/// `PassiveNpc`'s serialized fields, with the same defaults. Only Defence matters - it never attacks.
#[derive(Debug, Clone, Copy)]
pub struct PassiveConfig {
    pub footprint_size: i32,
    pub max_health: i32,
    pub levels: CombatLevels,
}

impl Default for PassiveConfig {
    fn default() -> Self {
        Self { footprint_size: 1, max_health: 10, levels: CombatLevels::new(1, 1, 10, 1, 1) }
    }
}
