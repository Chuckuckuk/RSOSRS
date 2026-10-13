/// Per-tile collision bits (the C# `[Flags] enum CollisionFlag : uint`). Plain `u32` constants
/// rather than a wrapper type, because `CollisionMap` stores and combines them as raw `u32`s.
pub struct CollisionFlag;

impl CollisionFlag {
    pub const NONE: u32 = 0;
    pub const BLOCK_NORTH_WEST: u32 = 0x1;
    pub const BLOCK_NORTH: u32 = 0x2;
    pub const BLOCK_NORTH_EAST: u32 = 0x4;
    pub const BLOCK_EAST: u32 = 0x8;
    pub const BLOCK_SOUTH_EAST: u32 = 0x10;
    pub const BLOCK_SOUTH: u32 = 0x20;
    pub const BLOCK_SOUTH_WEST: u32 = 0x40;
    pub const BLOCK_WEST: u32 = 0x80;
    pub const BLOCK_OBJECT: u32 = 0x100;
    pub const BLOCK_NPC: u32 = 0x200;
    pub const BLOCK_FLOOR_DECORATION: u32 = 0x40000;
    pub const BLOCK_FLOOR: u32 = 0x200000;
    pub const BLOCK_FULL: u32 = Self::BLOCK_OBJECT | Self::BLOCK_FLOOR_DECORATION | Self::BLOCK_FLOOR;
}
