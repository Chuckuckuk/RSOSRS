use super::direction::Direction;
use std::ops::Add;

/// A discrete tile coordinate. World unit = 1 tile, with the tile CENTRE at
/// `(x + 0.5, y, z + 0.5)` so tile boundaries land on integers (keeps wall rendering and
/// gizmos aligned). World-to-tile conversion must floor, never truncate.
///
/// `plane` is carried for forward compatibility but v1 asserts `plane == 0` everywhere;
/// stairs/ladders are modelled as teleports, not traversable tiles, when planes are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TileCoord {
    pub x: i32,
    pub z: i32,
    pub plane: i32,
}

impl TileCoord {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z, plane: 0 }
    }

    pub fn with_plane(x: i32, z: i32, plane: i32) -> Self {
        Self { x, z, plane }
    }

    /// Chebyshev (king-move) distance - the correct metric for OSRS tile movement cost.
    pub fn chebyshev_distance_to(self, other: TileCoord) -> i32 {
        (self.x - other.x).abs().max((self.z - other.z).abs())
    }

    /// Squared Euclidean distance - used only by the pathfinder's unreachable fallback tie-break.
    pub fn squared_distance_to(self, other: TileCoord) -> i32 {
        let dx = self.x - other.x;
        let dz = self.z - other.z;
        dx * dx + dz * dz
    }
}

impl Add<Direction> for TileCoord {
    type Output = TileCoord;

    fn add(self, dir: Direction) -> TileCoord {
        TileCoord::with_plane(self.x + dir.delta_x(), self.z + dir.delta_z(), self.plane)
    }
}

impl std::fmt::Display for TileCoord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, plane {})", self.x, self.z, self.plane)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_defaults_plane_to_zero() {
        let tile = TileCoord::new(3, 4);
        assert_eq!(tile.plane, 0);
    }

    #[test]
    fn add_direction_steps_one_tile() {
        let tile = TileCoord::new(5, 5);
        assert_eq!(tile + Direction::North, TileCoord::new(5, 6));
        assert_eq!(tile + Direction::NorthEast, TileCoord::new(6, 6));
    }

    #[test]
    fn chebyshev_distance_uses_the_larger_axis_delta() {
        let a = TileCoord::new(0, 0);
        let b = TileCoord::new(3, 1);
        assert_eq!(a.chebyshev_distance_to(b), 3);
    }

    #[test]
    fn squared_distance_matches_euclidean_squared() {
        let a = TileCoord::new(0, 0);
        let b = TileCoord::new(3, 4);
        assert_eq!(a.squared_distance_to(b), 25);
    }

    #[test]
    fn equality_accounts_for_plane() {
        let a = TileCoord::with_plane(1, 1, 0);
        let b = TileCoord::with_plane(1, 1, 1);
        assert_ne!(a, b);
    }
}
