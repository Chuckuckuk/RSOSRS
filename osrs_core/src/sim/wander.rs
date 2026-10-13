//! How an idle NPC wanders: now and then it picks a tile within a few tiles of where it spawned
//! and strolls there, one tile a tick, then stands about again. It only does this while it is not
//! fighting, and it never leaves its patch.

use crate::rng::{DotNetRandom, Rng};
use crate::tiles::TileCoord;

/// On a given idle tick an NPC sets off with a 1 in this chance (about once every 7 seconds).
const IDLE_ODDS: i32 = 12;

#[derive(Debug, Clone)]
pub struct Wander {
    home: TileCoord,
    radius: i32,
    /// Each wanderer has its own stream, so adding one never changes what another (or a fight's
    /// rolls) does.
    rng: DotNetRandom,
}

impl Wander {
    pub fn new(home: TileCoord, radius: i32, seed: i32) -> Self {
        Self { home, radius: radius.max(0), rng: DotNetRandom::new(seed) }
    }

    pub fn home(&self) -> TileCoord {
        self.home
    }

    pub fn radius(&self) -> i32 {
        self.radius
    }

    /// Called on a tick the NPC has nowhere to go: usually nothing, sometimes a tile within the
    /// radius of home to walk to.
    pub fn choose(&mut self, current: TileCoord) -> Option<TileCoord> {
        if self.radius == 0 || self.rng.next_range(0, IDLE_ODDS) != 0 {
            return None;
        }
        let target = TileCoord::with_plane(
            self.home.x + self.rng.next_range(-self.radius, self.radius + 1),
            self.home.z + self.rng.next_range(-self.radius, self.radius + 1),
            self.home.plane,
        );
        (target != current).then_some(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_mostly_stands_still_and_only_ever_picks_tiles_near_home() {
        let home = TileCoord::new(100, 200);
        let mut wander = Wander::new(home, 5, 7);
        let (mut chosen, mut ticks) = (0, 0);
        for _ in 0..5000 {
            ticks += 1;
            if let Some(t) = wander.choose(home) {
                chosen += 1;
                assert!((t.x - home.x).abs() <= 5 && (t.z - home.z).abs() <= 5, "{t:?}");
            }
        }
        // about one tick in twelve
        assert!(chosen > ticks / 20 && chosen < ticks / 5, "{chosen} of {ticks}");
    }

    #[test]
    fn a_radius_of_zero_never_wanders() {
        let mut wander = Wander::new(TileCoord::new(0, 0), 0, 1);
        assert!((0..1000).all(|_| wander.choose(TileCoord::new(0, 0)).is_none()));
    }
}
