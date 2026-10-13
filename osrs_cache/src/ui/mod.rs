//! A pure-Rust software renderer for the game interface (IF3 component trees from cache archive 3).
//!
//! * [`layout`] - size/position modes
//! * [`raster`] - RGBA canvas, clip stack, blits
//! * [`text`]   - the client's font drawing and tags
//! * [`tree`]   - compose a [`UiState`] into a [`Canvas`] and a hit list
//! * [`toplevel`] - the three top-level layouts and the standard side-tab emulation
//! * [`assets`] - cache-backed [`Assets`]

pub mod assets;
pub mod layout;
pub mod raster;
pub mod text;
pub mod toplevel;
pub mod tree;

pub use assets::CacheAssets;
pub use layout::{resolve, Rect};
pub use raster::Canvas;
pub use toplevel::{arrange, modern_arrangement, standard_state, Layout, ModernArrangement, TopLevel};
pub use tree::{draw, hit_at, Assets, Hit, Override, UiState};
