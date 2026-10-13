//! Rust port of `Osrs.Core` (see the Unity project's `Assets/Osrs/Core`) - the engine-agnostic
//! OSRS combat/tile logic, ported headless first and wired into a Bevy app later.
//!
//! Porting order mirrors the C# namespace split: `tiles` (no dependencies) before `combat`
//! (depends only on `tiles` conceptually, though `CombatStats`/`CombatMath` don't even need
//! that yet). Modules land here in the same order Actor/Mover/collision/pathfinding/tick would
//! need them, so later additions slot in without reshuffling what's already here.
//!
//! See `PARITY_PLAN.md` for the full port order and the design decisions (ID-based world instead
//! of shared references, deferred scheduler changes, a .NET-compatible seeded RNG).

pub mod actors;
pub mod collision;
pub mod combat;
pub mod core_log;
pub mod data;
pub mod encounters;
pub mod interaction;
pub mod model;
pub mod pathfinding;
pub mod rules;
pub mod rng;
pub mod sim;
pub mod tick;
pub mod tiles;
