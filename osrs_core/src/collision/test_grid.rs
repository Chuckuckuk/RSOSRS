//! Parses a small ASCII map into a `CollisionMap` for tests, so collision/corner/pathfinding
//! rules can be expressed as readable grids instead of hand-built flag arrays (the C#
//! `CollisionTestGrid`).
//!
//! The top row of the string is the NORTH-most row (highest Z); column 0 is the WEST-most
//! column (X = 0). Characters:
//!   `.`  open floor, height 0
//!   `#`  full block (BlockObject)
//!   `~`  no floor (BlockFloor)
//!   `|`  wall on the EAST edge of this tile (and the WEST edge of the tile to its right)
//!   `_`  wall on the NORTH edge of this tile (and the SOUTH edge of the tile above it)
//!   `0`-`9`  open floor at that height (1/8-unit), for step-height tests

use super::{CollisionFlag, CollisionMap, CollisionMapData};

pub(crate) fn parse(ascii: &str, max_step_height: i32) -> CollisionMap {
    let normalized = ascii.replace("\r\n", "\n");
    let mut rows: Vec<&str> = normalized.split('\n').collect();
    while rows.last().is_some_and(|row| row.is_empty()) {
        rows.pop();
    }

    let row_count = rows.len();
    let width = rows.iter().map(|row| row.chars().count()).max().unwrap_or(0);
    let height = row_count;

    let mut flags = vec![0u32; width * height];
    let mut heights = vec![0i16; width * height];

    for (row, line) in rows.iter().enumerate() {
        let z = row_count - 1 - row; // flip: ascii row 0 (top) = max Z (north)
        for (x, c) in line.chars().enumerate() {
            let idx = z * width + x;
            match c {
                '.' => {}
                '#' => flags[idx] |= CollisionFlag::BLOCK_OBJECT,
                '~' => flags[idx] |= CollisionFlag::BLOCK_FLOOR,
                '|' => {
                    flags[idx] |= CollisionFlag::BLOCK_EAST;
                    if x + 1 < width {
                        flags[z * width + (x + 1)] |= CollisionFlag::BLOCK_WEST;
                    }
                }
                '_' => {
                    flags[idx] |= CollisionFlag::BLOCK_NORTH;
                    if z + 1 < height {
                        flags[(z + 1) * width + x] |= CollisionFlag::BLOCK_SOUTH;
                    }
                }
                '0'..='9' => heights[idx] = c as i16 - '0' as i16,
                _ => {}
            }
        }
    }

    CollisionMap::new(CollisionMapData {
        origin_x: 0,
        origin_z: 0,
        width: width as i32,
        height: height as i32,
        flags,
        heights,
        max_step_height,
    })
    .expect("test grid always produces consistent data")
}

/// A fully open `width` x `height` grid with no walls, no height variation.
pub(crate) fn open(width: i32, height: i32, max_step_height: i32) -> CollisionMap {
    let len = (width * height) as usize;
    CollisionMap::new(CollisionMapData {
        origin_x: 0,
        origin_z: 0,
        width,
        height,
        flags: vec![0; len],
        heights: vec![0; len],
        max_step_height,
    })
    .expect("test grid always produces consistent data")
}
