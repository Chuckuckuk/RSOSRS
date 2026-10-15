//! The map editor's model, apart from its window: projects of edits ([`project`]), turning them
//! into a new cache ([`compile`]), and the caches it has built ([`builds`]). The editor binary
//! (`main.rs` and its modules) is the window on top.
//!
//! The cache format itself (encoding groups, reference tables, writing sectors) is `osrs_cache`'s;
//! the NPC spawns a build carries are `osrs_core::data`'s, since the game reads them.

pub mod builds;
pub mod compile;
pub mod project;
