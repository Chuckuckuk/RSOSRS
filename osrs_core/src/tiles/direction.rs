/// The 8 OSRS movement directions. `None` is reserved so a real direction can never alias
/// "unvisited" when used as a breadcrumb (see `BfsPathfinder`'s `came_from` array).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Direction {
    #[default]
    None,
    West,
    East,
    South,
    North,
    SouthWest,
    SouthEast,
    NorthWest,
    NorthEast,
}

impl Direction {
    /// The direction of a one-tile step by (`dx`, `dz`), each -1, 0 or 1 (`None` for no move or a
    /// longer one).
    pub fn from_delta(dx: i32, dz: i32) -> Direction {
        match (dx, dz) {
            (-1, 0) => Direction::West,
            (1, 0) => Direction::East,
            (0, -1) => Direction::South,
            (0, 1) => Direction::North,
            (-1, -1) => Direction::SouthWest,
            (1, -1) => Direction::SouthEast,
            (-1, 1) => Direction::NorthWest,
            (1, 1) => Direction::NorthEast,
            _ => Direction::None,
        }
    }

    // Indexed by enum declaration order above - keep in sync with the match-free indices.
    const DELTA_X: [i32; 9] = [0, -1, 1, 0, 0, -1, 1, -1, 1];
    const DELTA_Z: [i32; 9] = [0, 0, 0, -1, 1, -1, -1, 1, 1];

    fn index(self) -> usize {
        match self {
            Direction::None => 0,
            Direction::West => 1,
            Direction::East => 2,
            Direction::South => 3,
            Direction::North => 4,
            Direction::SouthWest => 5,
            Direction::SouthEast => 6,
            Direction::NorthWest => 7,
            Direction::NorthEast => 8,
        }
    }

    pub fn delta_x(self) -> i32 {
        Self::DELTA_X[self.index()]
    }

    pub fn delta_z(self) -> i32 {
        Self::DELTA_Z[self.index()]
    }

    pub fn is_diagonal(self) -> bool {
        matches!(
            self,
            Direction::SouthWest | Direction::SouthEast | Direction::NorthWest | Direction::NorthEast
        )
    }

    pub fn is_cardinal(self) -> bool {
        matches!(
            self,
            Direction::West | Direction::East | Direction::South | Direction::North
        )
    }

    pub fn opposite(self) -> Direction {
        match self {
            Direction::West => Direction::East,
            Direction::East => Direction::West,
            Direction::South => Direction::North,
            Direction::North => Direction::South,
            Direction::SouthWest => Direction::NorthEast,
            Direction::SouthEast => Direction::NorthWest,
            Direction::NorthWest => Direction::SouthEast,
            Direction::NorthEast => Direction::SouthWest,
            Direction::None => Direction::None,
        }
    }

    /// Splits a diagonal into the two cardinal components whose legality gates it under the
    /// no-corner-cutting rule (e.g. `NorthEast` requires North AND East to both be legal steps).
    /// Panics if `self` is not a diagonal direction (mirrors the C# `ArgumentException`).
    pub fn cardinal_components(self) -> (Direction, Direction) {
        match self {
            Direction::SouthWest => (Direction::South, Direction::West),
            Direction::SouthEast => (Direction::South, Direction::East),
            Direction::NorthWest => (Direction::North, Direction::West),
            Direction::NorthEast => (Direction::North, Direction::East),
            _ => panic!("{self:?} is not a diagonal direction."),
        }
    }
}

/// Fixed 8-way expansion order used by `BfsPathfinder`. Because the pathfinder's `came_from`
/// breadcrumb is first-write-wins, this order determines the exact shape of equal-length
/// shortest paths - do not reorder without updating `BfsPathfinderTests`' L-shape assertion.
pub const EXPANSION_ORDER: [Direction; 8] = [
    Direction::West,
    Direction::East,
    Direction::South,
    Direction::North,
    Direction::SouthWest,
    Direction::SouthEast,
    Direction::NorthWest,
    Direction::NorthEast,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_matches_direction() {
        assert_eq!(Direction::North.delta_z(), 1);
        assert_eq!(Direction::South.delta_z(), -1);
        assert_eq!(Direction::East.delta_x(), 1);
        assert_eq!(Direction::West.delta_x(), -1);
        assert_eq!(Direction::NorthEast.delta_x(), 1);
        assert_eq!(Direction::NorthEast.delta_z(), 1);
    }

    #[test]
    fn opposite_round_trips() {
        for dir in EXPANSION_ORDER {
            assert_eq!(dir.opposite().opposite(), dir);
        }
    }

    #[test]
    fn cardinal_components_match_diagonal_name() {
        assert_eq!(
            Direction::NorthEast.cardinal_components(),
            (Direction::North, Direction::East)
        );
    }

    #[test]
    #[should_panic]
    fn cardinal_components_panics_on_non_diagonal() {
        Direction::North.cardinal_components();
    }
}
