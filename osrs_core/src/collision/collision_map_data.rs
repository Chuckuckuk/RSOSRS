use std::fmt;

/// The raw baked arrays a `CollisionMap` is built from.
#[derive(Debug, Clone, Default)]
pub struct CollisionMapData {
    pub origin_x: i32,
    pub origin_z: i32,
    pub width: i32,
    pub height: i32,
    pub flags: Vec<u32>,
    pub heights: Vec<i16>,
    pub max_step_height: i32,
}

/// Mirrors the C# `ArgumentException`s thrown by the `CollisionMap` constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionMapDataError {
    NonPositiveSize,
    ArrayLengthMismatch,
}

impl fmt::Display for CollisionMapDataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveSize => write!(f, "CollisionMapData width/height must be positive."),
            Self::ArrayLengthMismatch => {
                write!(f, "CollisionMapData flags/heights length must equal width * height.")
            }
        }
    }
}

impl std::error::Error for CollisionMapDataError {}
