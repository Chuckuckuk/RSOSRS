//! Reader for the Old School RuneScape game cache (`main_file_cache.dat2` / `.idxN`).
//!
//! Layers: [`Cache`] (sector/container IO) -> [`Index`] (reference table) ->
//! typed decoders ([`model`], [`terrain`], [`config`], [`locs`]).
//!
//! Writing: every layer has an inverse (`container::encode`, `cache::join_files`, `locs::encode`,
//! [`reftable`]), and [`writer`] writes groups into a copy of a cache, never into the installed
//! one. [`active`] picks which cache the game reads; an [`overlay`] replaces groups in memory.

pub mod active;
pub mod anim;
pub mod bundle;
pub mod buf;
pub mod cache;
pub mod clientscript;
pub mod collision;
pub mod config;
pub mod dbtable;
pub mod font;
pub mod healthbar;
pub mod hitsplat;
pub mod item;
pub mod interface;
pub mod item_icon;
pub mod kit;
pub mod container;
pub mod locs;
pub mod minimap;
pub mod model;
pub mod npc;
pub mod overlay;
pub mod reftable;
pub mod terrain;
pub mod scene;
pub mod sound;
pub mod spotanim;
pub mod squares;
pub mod terrain_mesh;
pub mod textures;
#[doc(hidden)]
pub mod testing;
pub mod ui;
pub mod writer;
pub mod xtea;

pub use cache::{Cache, Index};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Format(String),
    Missing(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::Format(s) => write!(f, "format: {s}"),
            Error::Missing(s) => write!(f, "missing: {s}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub(crate) fn fmt_err<T>(s: impl Into<String>) -> Result<T> {
    Err(Error::Format(s.into()))
}

/// Jagex string hash used for named groups (`m50_50`, `l50_50`, ...).
pub fn name_hash(s: &str) -> i32 {
    let mut h: i32 = 0;
    for b in s.bytes() {
        h = (b as i32).wrapping_add(h.wrapping_shl(5).wrapping_sub(h));
    }
    h
}

/// Well-known archive (index) ids.
pub mod idx {
    pub const ANIM_FRAMES: u8 = 0;
    pub const SKELETONS: u8 = 1;
    pub const CONFIG: u8 = 2;
    pub const INTERFACES: u8 = 3;
    pub const MAPS: u8 = 5;
    pub const MODELS: u8 = 7;
    pub const SPRITES: u8 = 8;
    pub const TEXTURES: u8 = 9;
}

/// Group ids inside the CONFIG archive.
pub mod cfg {
    pub const UNDERLAY: u32 = 1;
    pub const IDENTKIT: u32 = 3;
    pub const OVERLAY: u32 = 4;
    pub const INV: u32 = 5;
    pub const OBJECT: u32 = 6;
    pub const ENUM: u32 = 8;
    pub const NPC: u32 = 9;
    pub const ITEM: u32 = 10;
    pub const SEQUENCE: u32 = 12;
    pub const SPOTANIM: u32 = 13;
}
