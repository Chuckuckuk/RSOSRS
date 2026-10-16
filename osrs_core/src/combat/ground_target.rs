use crate::rng::Rng;
use crate::tiles::TileCoord;

/// Which tile an area attack is centred on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HazardAnchor {
    /// The attacker's own tile at the moment the attack was selected.
    #[default]
    SelfTile,
    /// The target's tile at the moment the attack was selected.
    Target,
}

/// How a ground-targeted attack picks the tiles it covers, relative to its anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileSelection {
    /// Every one of these `(dx, dz)` offsets from the anchor: a square, a cross, a line, a claw
    /// pattern. Not necessarily contiguous.
    Offsets(Vec<(i32, i32)>),
    /// Between `min` and `max` distinct tiles drawn at random from the square `radius` tiles out
    /// from the anchor (falling rocks around the player). The count is drawn first when it varies.
    Scatter { radius: i32, min: usize, max: usize },
}

/// An attack that is aimed at the ground rather than at a player: it covers a set of tiles chosen
/// when it resolves, and after the attack's own `impact_delay_ticks` it hits whoever is standing on
/// them then (not whoever it was aimed at). What the attack does to each is its ordinary roll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundTarget {
    pub anchor: HazardAnchor,
    pub selection: TileSelection,
    /// The anchor tile is always one of the covered tiles, whatever the selection draws (the first
    /// rock always lands on the player). It counts toward a scatter's count.
    pub always_includes_anchor: bool,
}

impl GroundTarget {
    pub fn offsets(anchor: HazardAnchor, offsets: Vec<(i32, i32)>) -> Self {
        assert!(!offsets.is_empty(), "a ground attack must cover at least one tile");
        Self { anchor, selection: TileSelection::Offsets(offsets), always_includes_anchor: false }
    }

    /// The square `radius` tiles out from the anchor: 0 is the one tile, 1 is the 3x3 around it.
    pub fn square(anchor: HazardAnchor, radius: i32) -> Self {
        assert!(radius >= 0, "radius must not be negative");
        let offsets = (-radius..=radius).flat_map(|dx| (-radius..=radius).map(move |dz| (dx, dz))).collect();
        Self { anchor, selection: TileSelection::Offsets(offsets), always_includes_anchor: false }
    }

    pub fn scatter(anchor: HazardAnchor, radius: i32, count: usize) -> Self {
        Self::scatter_between(anchor, radius, count, count)
    }

    /// A scatter of `min` to `max` tiles, uniform, drawn once per use.
    pub fn scatter_between(anchor: HazardAnchor, radius: i32, min: usize, max: usize) -> Self {
        assert!(radius >= 0, "radius must not be negative");
        assert!(min > 0, "a ground attack must cover at least one tile");
        assert!(max >= min, "max must not be below min");
        Self { anchor, selection: TileSelection::Scatter { radius, min, max }, always_includes_anchor: false }
    }

    /// Makes the anchor tile always one of the covered tiles.
    pub fn including_anchor(mut self) -> Self {
        self.always_includes_anchor = true;
        self
    }

    /// The tile this area is centred on.
    pub fn anchor_tile(&self, self_tile: TileCoord, target_tile: TileCoord) -> TileCoord {
        match self.anchor {
            HazardAnchor::SelfTile => self_tile,
            HazardAnchor::Target => target_tile,
        }
    }

    /// The tiles this attack covers, from the attacker's and target's tiles as they were when the
    /// attack was selected. The target's tile is all this ever takes from the target.
    pub fn tiles(&self, self_tile: TileCoord, target_tile: TileCoord, rng: &mut dyn Rng) -> Vec<TileCoord> {
        let at = match self.anchor {
            HazardAnchor::SelfTile => self_tile,
            HazardAnchor::Target => target_tile,
        };
        let tile = |dx: i32, dz: i32| TileCoord::with_plane(at.x + dx, at.z + dz, at.plane);
        let mut covered: Vec<TileCoord> = match &self.selection {
            TileSelection::Offsets(offsets) => offsets.iter().map(|&(dx, dz)| tile(dx, dz)).collect(),
            TileSelection::Scatter { radius, min, max } => {
                let count = if min == max { *min } else { rng.next_range(*min as i32, *max as i32 + 1) as usize };
                // the anchor is placed up front, so the draw is of the rest
                let reserved = self.always_includes_anchor as usize;
                let mut candidates: Vec<(i32, i32)> = (-radius..=*radius)
                    .flat_map(|dx| (-radius..=*radius).map(move |dz| (dx, dz)))
                    .filter(|&offset| !(self.always_includes_anchor && offset == (0, 0)))
                    .collect();
                // partial Fisher-Yates: the first `drawn` slots end up a uniform draw without repeats
                let drawn = count.saturating_sub(reserved).min(candidates.len());
                for i in 0..drawn {
                    let j = rng.next_range(i as i32, candidates.len() as i32) as usize;
                    candidates.swap(i, j);
                }
                candidates[..drawn].iter().map(|&(dx, dz)| tile(dx, dz)).collect()
            }
        };
        if self.always_includes_anchor && !covered.contains(&at) {
            covered.insert(0, at);
        }
        covered
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rng::DotNetRandom;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    #[test]
    fn a_square_covers_the_tiles_around_the_anchor() {
        let target = GroundTarget::square(HazardAnchor::Target, 1);

        let tiles = target.tiles(tc(0, 0), tc(10, 10), &mut DotNetRandom::new(1));

        assert_eq!(tiles.len(), 9);
        assert!(tiles.contains(&tc(9, 9)) && tiles.contains(&tc(10, 10)) && tiles.contains(&tc(11, 11)));
        assert!(!tiles.contains(&tc(12, 10)));
    }

    #[test]
    fn the_anchor_picks_whose_tile_the_area_is_centred_on() {
        let on_self = GroundTarget::square(HazardAnchor::SelfTile, 0);
        let on_target = GroundTarget::square(HazardAnchor::Target, 0);

        assert_eq!(on_self.tiles(tc(1, 2), tc(8, 9), &mut DotNetRandom::new(1)), vec![tc(1, 2)]);
        assert_eq!(on_target.tiles(tc(1, 2), tc(8, 9), &mut DotNetRandom::new(1)), vec![tc(8, 9)]);
    }

    #[test]
    fn offsets_need_not_be_contiguous() {
        let target = GroundTarget::offsets(HazardAnchor::Target, vec![(0, 0), (3, 0), (-2, 4)]);

        let tiles = target.tiles(tc(0, 0), tc(5, 5), &mut DotNetRandom::new(1));

        assert_eq!(tiles, vec![tc(5, 5), tc(8, 5), tc(3, 9)]);
    }

    #[test]
    fn a_scatter_draws_distinct_tiles_inside_the_radius_and_is_reproducible() {
        let target = GroundTarget::scatter(HazardAnchor::Target, 2, 8);

        let first = target.tiles(tc(0, 0), tc(10, 10), &mut DotNetRandom::new(7));
        let again = target.tiles(tc(0, 0), tc(10, 10), &mut DotNetRandom::new(7));

        assert_eq!(first, again, "the same seed scatters the same way");
        assert_eq!(first.len(), 8);
        assert!(first.iter().all(|t| t.chebyshev_distance_to(tc(10, 10)) <= 2));
        let mut sorted = first.clone();
        sorted.sort_by_key(|t| (t.x, t.z));
        sorted.dedup();
        assert_eq!(sorted.len(), 8, "no tile twice");
    }

    #[test]
    fn a_scatter_count_range_draws_a_count_inside_it() {
        let target = GroundTarget::scatter_between(HazardAnchor::Target, 1, 3, 6);
        let mut seen = std::collections::BTreeSet::new();

        for seed in 0..60 {
            let tiles = target.tiles(tc(0, 0), tc(10, 10), &mut DotNetRandom::new(seed));
            assert!((3..=6).contains(&tiles.len()), "{} tiles", tiles.len());
            seen.insert(tiles.len());
        }

        assert_eq!(seen.len(), 4, "every count from 3 to 6 comes up: {seen:?}");
    }

    #[test]
    fn including_the_anchor_always_covers_it_and_counts_toward_the_count() {
        let target = GroundTarget::scatter_between(HazardAnchor::Target, 1, 3, 6).including_anchor();

        for seed in 0..60 {
            let tiles = target.tiles(tc(0, 0), tc(10, 10), &mut DotNetRandom::new(seed));
            assert!(tiles.contains(&tc(10, 10)), "seed {seed}");
            assert!((3..=6).contains(&tiles.len()));
            let mut unique = tiles.clone();
            unique.sort_by_key(|t| (t.x, t.z));
            unique.dedup();
            assert_eq!(unique.len(), tiles.len(), "no tile twice");
        }
    }

    #[test]
    fn a_scatter_asking_for_more_tiles_than_exist_covers_them_all() {
        let target = GroundTarget::scatter(HazardAnchor::SelfTile, 1, 50);

        assert_eq!(target.tiles(tc(0, 0), tc(0, 0), &mut DotNetRandom::new(1)).len(), 9);
    }
}
