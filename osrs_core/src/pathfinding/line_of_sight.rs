use crate::tiles::TileCoord;

/// Whether a straight shot from one tile to another gets through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineOfSight {
    Clear,
    /// The first blocking tile on the way, counting from the shooter.
    Blocked { at: TileCoord },
}

impl LineOfSight {
    pub fn is_clear(self) -> bool {
        self == LineOfSight::Clear
    }
}

/// The tiles a straight line from `from` to `to` passes through, in order, excluding `from`
/// itself and including `to`. It steps the longer axis one tile at a time and rounds the other,
/// so it is symmetric enough for a tile game and never skips a tile along the dominant axis.
pub fn tiles_between(from: TileCoord, to: TileCoord) -> Vec<TileCoord> {
    let (dx, dz) = (to.x - from.x, to.z - from.z);
    let steps = dx.abs().max(dz.abs());
    (1..=steps)
        .map(|step| {
            let x = from.x + round_div(dx * step, steps);
            let z = from.z + round_div(dz * step, steps);
            TileCoord::with_plane(x, z, from.plane)
        })
        .collect()
}

/// `numerator / denominator` rounded to the nearest integer, halves away from zero.
fn round_div(numerator: i32, denominator: i32) -> i32 {
    let doubled = 2 * numerator.abs() + denominator;
    let rounded = doubled / (2 * denominator);
    rounded * numerator.signum()
}

/// Whether `from` can see `to` past the tiles in `blockers`. Neither endpoint blocks: a shooter
/// is not blocked by the tile it stands on, and a target is hit on its own tile. The tiles that
/// can block are only the ones named, so what blocks (a boulder, a pillar) is the caller's call.
pub fn line_of_sight(from: TileCoord, to: TileCoord, blockers: &[TileCoord]) -> LineOfSight {
    let path = tiles_between(from, to);
    for tile in path.iter().take(path.len().saturating_sub(1)) {
        if blockers.contains(tile) {
            return LineOfSight::Blocked { at: *tile };
        }
    }
    LineOfSight::Clear
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    #[test]
    fn a_straight_line_passes_through_every_tile_between() {
        assert_eq!(tiles_between(tc(0, 0), tc(3, 0)), vec![tc(1, 0), tc(2, 0), tc(3, 0)]);
        assert_eq!(tiles_between(tc(0, 0), tc(0, -2)), vec![tc(0, -1), tc(0, -2)]);
        assert_eq!(tiles_between(tc(0, 0), tc(2, 2)), vec![tc(1, 1), tc(2, 2)]);
    }

    #[test]
    fn a_shot_to_the_same_tile_is_always_clear() {
        assert!(line_of_sight(tc(4, 4), tc(4, 4), &[tc(4, 4)]).is_clear());
    }

    #[test]
    fn a_blocker_in_the_way_blocks_and_is_named() {
        let blockers = [tc(2, 0)];

        assert_eq!(line_of_sight(tc(0, 0), tc(5, 0), &blockers), LineOfSight::Blocked { at: tc(2, 0) });
    }

    #[test]
    fn the_first_blocker_from_the_shooter_is_the_one_named() {
        let blockers = [tc(4, 0), tc(2, 0)];

        assert_eq!(line_of_sight(tc(0, 0), tc(6, 0), &blockers), LineOfSight::Blocked { at: tc(2, 0) });
    }

    #[test]
    fn a_blocker_off_the_line_does_not_block() {
        assert!(line_of_sight(tc(0, 0), tc(5, 0), &[tc(2, 1), tc(3, -1)]).is_clear());
    }

    #[test]
    fn neither_endpoint_blocks() {
        assert!(line_of_sight(tc(0, 0), tc(3, 0), &[tc(0, 0), tc(3, 0)]).is_clear());
    }

    #[test]
    fn a_shallow_diagonal_follows_the_rounded_line() {
        // 5 across, 2 up: z rounds to 0,1,1,2,2 for steps 1..5 (halves away from zero)
        let path = tiles_between(tc(0, 0), tc(5, 2));

        assert_eq!(path, vec![tc(1, 0), tc(2, 1), tc(3, 1), tc(4, 2), tc(5, 2)]);
    }

    #[test]
    fn the_line_is_the_same_from_either_end() {
        let forward = tiles_between(tc(0, 0), tc(7, 3));
        let mut backward = tiles_between(tc(7, 3), tc(0, 0));
        backward.reverse();
        let mut forward_with_start = vec![tc(0, 0)];
        forward_with_start.extend(forward);
        let mut backward_with_start = backward;
        backward_with_start.push(tc(7, 3));

        // the same tiles are crossed, give or take the one half-way rounding, which stays within one
        for (a, b) in forward_with_start.iter().zip(&backward_with_start) {
            assert!(a.chebyshev_distance_to(*b) <= 1, "{a:?} vs {b:?}");
        }
    }
}
