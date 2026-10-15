//! The game logic that lives inside the Unity project's `Osrs.Runtime` MonoBehaviours
//! (`PlayerMover`, `NpcCombatant`, `BossCombatant`, `HealerNpc`, `PassiveNpc`, the encounter
//! setups, `TickRunner`, `WorldContext`), minus rendering and input. A `World` owns every entity
//! and drives them from a `TickScheduler<EntityId>`; see `PARITY_PLAN.md` for why entities are
//! referred to by ID rather than by reference.

mod animations;
mod config;
mod entity;
mod events;
mod inventory;
mod examine;
mod firemaking;
mod gathering;
mod ground_items;
mod loadout;
mod npc_factory;
mod objects;
mod player;
mod player_runtime;
pub mod scenarios;
mod scenery;
mod server;
mod skills;
pub mod sounds;
mod tick_driver;
mod wander;
mod world;

pub use animations::{draw_graphic, player_animations, projectile_graphic, PlayerAnimations};
pub use config::{BossConfig, CombatLevels, HealerConfig, NpcConfig, PassiveConfig, PlayerConfig};
pub use events::{Combatant, WorldEvent};
pub use examine::ExamineTexts;
pub use firemaking::{LightAction, RestAction, BONFIRE_LOC, BONFIRE_OPTIONS};
pub use gathering::GatherAction;
pub use ground_items::{GroundItem, GroundItems, DESPAWN_TICKS};
pub use objects::{DoorAction, PlayerAction};
pub use scenery::{Loc, LocChange, LocKey, SceneryIndex, SpawnedLoc};
pub use inventory::{Inventory, ItemStack, INVENTORY_SIZE};
pub use entity::{Behavior, BossState, Entity, HealerState, NpcState};
pub use loadout::Loadout;
pub use npc_factory::{attack_from_def, pattern_from_def};
pub use player::Player;
pub use player_runtime::{AttackPlan, Dps, UNARMED_SPEED_FALLBACK};
pub use skills::Skills;
pub use server::{
    spawn_server, ClientCommand, EntitySnapshot, GroundItemView, ItemView, PlayerId, PlayerSnapshot, Server, ServerHandle, SkillSnapshot, Snapshot,
};
pub use tick_driver::TickDriver;
pub use world::{SpawnBlueprint, World, ENCOUNTER_CLOCK_ID, PLAYER_ID};
