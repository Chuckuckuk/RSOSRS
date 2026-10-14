use crate::collision::{CollisionFlag, CollisionMap};
use crate::tiles::TileCoord;

/// The most tiles an `AnyOf` target can list (a two-leaf door's reach).
pub const MAX_TARGET_TILES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// Reach this exact tile. The only kind eligible for the unreachable fallback.
    Exact,
    /// Reach any tile of an N x M footprint, anchored at its south-west tile.
    Rect,
    /// Reach any tile orthogonally touching an N x M footprint - the normal "use this door/NPC" case.
    Adjacent,
    /// Stop once within Chebyshev distance `range` of a point (no line-of-sight check).
    WithinRange,
    /// Stop once within Chebyshev distance `range` of any tile of an N x M footprint, standing
    /// outside it - how a ranged or magic attacker reaches a large target.
    WithinRangeOfRect,
    /// Reach any of a short list of tiles (`TargetSpec::tiles`): where a wall object such as a
    /// door can be used from.
    AnyOf,
}

/// What a path search is trying to reach. One entry point for every click/interaction target
/// kind the movement system needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetSpec {
    pub kind: TargetKind,
    /// South-west tile of the rect for Rect/Adjacent, the exact tile for Exact, or the centre for WithinRange.
    pub origin: TileCoord,
    pub width: i32,
    pub height: i32,
    pub range: i32,
    /// The tiles of an `AnyOf` target (the first `tile_count`).
    pub tiles: [TileCoord; MAX_TARGET_TILES],
    pub tile_count: usize,
}

impl TargetSpec {
    fn new(kind: TargetKind, origin: TileCoord, width: i32, height: i32, range: i32) -> Self {
        Self { kind, origin, width, height, range, tiles: [TileCoord::default(); MAX_TARGET_TILES], tile_count: 0 }
    }

    /// Any of `tiles` (at most [`MAX_TARGET_TILES`]; the rest are ignored).
    pub fn any_of(tiles: &[TileCoord]) -> Self {
        let mut target = Self::new(TargetKind::AnyOf, tiles.first().copied().unwrap_or_default(), 1, 1, 0);
        for (slot, tile) in target.tiles.iter_mut().zip(tiles) {
            *slot = *tile;
        }
        target.tile_count = tiles.len().min(MAX_TARGET_TILES);
        target
    }

    /// Where a straight wall object (a door) on `tile`, turned `orientation`, can be used from,
    /// as the game decides it: its own tile, the tile across its edge, and the two tiles beside
    /// it along the wall unless a wall stands between them and its tile.
    pub fn wall_reach(map: &CollisionMap, tile: TileCoord, orientation: u8) -> Vec<TileCoord> {
        let at = |(dx, dz): (i32, i32)| TileCoord::with_plane(tile.x + dx, tile.z + dz, tile.plane);
        let mut reach = vec![tile, at(rotate((-1, 0), orientation))];
        for side in [rotate((0, 1), orientation), rotate((0, -1), orientation)] {
            // the edge of the side tile that faces the wall's own tile
            let facing = match (-side.0, -side.1) {
                (0, 1) => CollisionFlag::BLOCK_NORTH,
                (0, -1) => CollisionFlag::BLOCK_SOUTH,
                (1, 0) => CollisionFlag::BLOCK_EAST,
                _ => CollisionFlag::BLOCK_WEST,
            };
            if map.flags_at(at(side)) & facing == 0 {
                reach.push(at(side));
            }
        }
        reach
    }

    pub fn exact(tile: TileCoord) -> Self {
        Self::new(TargetKind::Exact, tile, 1, 1, 0)
    }

    pub fn rect(sw_origin: TileCoord, width: i32, height: i32) -> Self {
        Self::new(TargetKind::Rect, sw_origin, width, height, 0)
    }

    pub fn adjacent(sw_origin: TileCoord, width: i32, height: i32) -> Self {
        Self::new(TargetKind::Adjacent, sw_origin, width, height, 0)
    }

    pub fn within_range_of(center: TileCoord, range: i32) -> Self {
        Self::new(TargetKind::WithinRange, center, 1, 1, range)
    }

    pub fn within_range_of_rect(sw_origin: TileCoord, width: i32, height: i32, range: i32) -> Self {
        Self::new(TargetKind::WithinRangeOfRect, sw_origin, width, height, range)
    }

    /// Chebyshev distance from `tile` to the nearest tile of a `width` x `height` footprint
    /// anchored at `sw_origin` (0 when `tile` is inside it).
    pub fn distance_to_rect(tile: TileCoord, sw_origin: TileCoord, width: i32, height: i32) -> i32 {
        let dx = (sw_origin.x - tile.x).max(tile.x - (sw_origin.x + width - 1)).max(0);
        let dz = (sw_origin.z - tile.z).max(tile.z - (sw_origin.z + height - 1)).max(0);
        dx.max(dz)
    }

    pub fn is_satisfied_by(&self, tile: TileCoord) -> bool {
        if tile.plane != self.origin.plane {
            return false;
        }

        match self.kind {
            TargetKind::Exact => tile.x == self.origin.x && tile.z == self.origin.z,
            TargetKind::Rect => self.contains(tile),
            TargetKind::Adjacent => !self.contains(tile) && self.is_orthogonally_adjacent(tile),
            TargetKind::WithinRange => tile.chebyshev_distance_to(self.origin) <= self.range,
            TargetKind::WithinRangeOfRect => {
                !self.contains(tile) && Self::distance_to_rect(tile, self.origin, self.width, self.height) <= self.range
            }
            TargetKind::AnyOf => self.tiles[..self.tile_count].contains(&tile),
        }
    }

    fn contains(&self, tile: TileCoord) -> bool {
        tile.x >= self.origin.x
            && tile.x < self.origin.x + self.width
            && tile.z >= self.origin.z
            && tile.z < self.origin.z + self.height
    }

    fn is_orthogonally_adjacent(&self, tile: TileCoord) -> bool {
        let within_x_band = tile.x >= self.origin.x && tile.x < self.origin.x + self.width;
        let within_z_band = tile.z >= self.origin.z && tile.z < self.origin.z + self.height;

        let touches_west = tile.x == self.origin.x - 1 && within_z_band;
        let touches_east = tile.x == self.origin.x + self.width && within_z_band;
        let touches_south = tile.z == self.origin.z - 1 && within_x_band;
        let touches_north = tile.z == self.origin.z + self.height && within_x_band;

        touches_west || touches_east || touches_south || touches_north
    }
}

/// `(dx, dz)` turned `quarter_turns` clockwise (north to east): how an offset in the frame of a
/// west-edge wall applies to one placed turned.
pub fn rotate((dx, dz): (i32, i32), quarter_turns: u8) -> (i32, i32) {
    (0..quarter_turns & 3).fold((dx, dz), |(x, z), _| (z, -x))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    #[test]
    fn exact_matches_only_its_own_tile_and_plane() {
        let target = TargetSpec::exact(tc(3, 4));

        assert!(target.is_satisfied_by(tc(3, 4)));
        assert!(!target.is_satisfied_by(tc(3, 5)));
        assert!(!target.is_satisfied_by(TileCoord::with_plane(3, 4, 1)));
    }

    #[test]
    fn rect_matches_every_tile_of_the_footprint() {
        let target = TargetSpec::rect(tc(5, 5), 2, 3);

        assert!(target.is_satisfied_by(tc(5, 5)));
        assert!(target.is_satisfied_by(tc(6, 7)));
        assert!(!target.is_satisfied_by(tc(7, 5)));
        assert!(!target.is_satisfied_by(tc(5, 8)));
    }

    #[test]
    fn adjacent_matches_the_orthogonal_ring_but_not_the_footprint_or_corners() {
        let target = TargetSpec::adjacent(tc(5, 5), 2, 2);

        assert!(target.is_satisfied_by(tc(4, 5))); // west
        assert!(target.is_satisfied_by(tc(7, 6))); // east
        assert!(target.is_satisfied_by(tc(6, 4))); // south
        assert!(target.is_satisfied_by(tc(5, 7))); // north
        assert!(!target.is_satisfied_by(tc(5, 5))); // inside
        assert!(!target.is_satisfied_by(tc(4, 4))); // diagonal corner
        assert!(!target.is_satisfied_by(tc(8, 5))); // too far
    }

    #[test]
    fn within_range_of_rect_measures_to_the_nearest_footprint_tile_and_excludes_the_inside() {
        let target = TargetSpec::within_range_of_rect(tc(10, 10), 3, 3, 4);

        assert!(target.is_satisfied_by(tc(16, 10))); // 4 tiles east of the footprint's east edge (x = 12)
        assert!(!target.is_satisfied_by(tc(17, 10)));
        assert!(target.is_satisfied_by(tc(8, 8)));
        assert!(!target.is_satisfied_by(tc(11, 11))); // inside the footprint
        assert_eq!(TargetSpec::distance_to_rect(tc(11, 11), tc(10, 10), 3, 3), 0);
    }

    #[test]
    fn within_range_uses_chebyshev_distance() {
        let target = TargetSpec::within_range_of(tc(10, 10), 3);

        assert!(target.is_satisfied_by(tc(7, 7)));
        assert!(target.is_satisfied_by(tc(13, 10)));
        assert!(!target.is_satisfied_by(tc(6, 10)));
    }

    #[test]
    fn any_of_matches_only_its_tiles() {
        let target = TargetSpec::any_of(&[tc(1, 1), tc(3, 2)]);

        assert!(target.is_satisfied_by(tc(1, 1)));
        assert!(target.is_satisfied_by(tc(3, 2)));
        assert!(!target.is_satisfied_by(tc(2, 1)));
        assert!(!target.is_satisfied_by(TileCoord::default()), "an unused slot is not a target");
    }

    #[test]
    fn rotation_turns_clockwise() {
        assert_eq!(rotate((-1, 0), 0), (-1, 0)); // west
        assert_eq!(rotate((-1, 0), 1), (0, 1)); // north
        assert_eq!(rotate((-1, 0), 2), (1, 0)); // east
        assert_eq!(rotate((-1, 0), 3), (0, -1)); // south
    }

    #[test]
    fn a_door_is_reached_from_its_tile_across_its_edge_and_beside_it_unless_walled_off() {
        let mut map = crate::collision::test_grid::parse("....\n....\n....\n....", 0);
        // a door on the west edge of (2, 1); a wall between (2, 2) and the door's tile
        map.add_flags(tc(2, 2), CollisionFlag::BLOCK_SOUTH);

        let reach = TargetSpec::wall_reach(&map, tc(2, 1), 0);

        assert!(reach.contains(&tc(2, 1)), "its own tile");
        assert!(reach.contains(&tc(1, 1)), "across the edge");
        assert!(reach.contains(&tc(2, 0)), "beside it, open");
        assert!(!reach.contains(&tc(2, 2)), "beside it, but walled off");
    }
}
