//! The server's view of the scenery: which object (loc) stands on which tile, and which of them
//! are, for now, something else - a tree felled to a stump until it grows back, later a mined-out
//! rock or an opened door.

use std::collections::{BTreeMap, HashMap};

use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// A scenery object placed on the map.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Loc {
    /// The cache object id.
    pub id: u32,
    /// Its south-west tile.
    pub tile: TileCoord,
    /// The placement's shape (10 and 11 are free-standing objects such as trees).
    pub kind: u8,
    /// Quarter turns clockwise, 0..=3.
    pub orientation: u8,
    /// Its footprint in tiles, east by north, as placed (already turned).
    pub size: (i32, i32),
}

impl Loc {
    pub fn covers(&self, tile: TileCoord) -> bool {
        TargetSpec::rect(self.tile, self.size.0, self.size.1).is_satisfied_by(tile)
    }

    /// The same placement showing object `id` instead (a tree's stump).
    pub fn with_id(self, id: u32) -> Loc {
        Loc { id, ..self }
    }

    /// What identifies this placement whatever it currently looks like.
    pub fn key(&self) -> LocKey {
        LocKey { x: self.tile.x, z: self.tile.z, plane: self.tile.plane, id: self.id }
    }
}

/// A placement by its south-west tile and its original object id. Ordered, so anything walking
/// the changes does so in a fixed order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocKey {
    pub x: i32,
    pub z: i32,
    pub plane: i32,
    pub id: u32,
}

/// Every scenery object the server has loaded, findable by any tile it covers.
#[derive(Debug, Default, Clone)]
pub struct SceneryIndex {
    by_tile: HashMap<TileCoord, Vec<Loc>>,
}

impl SceneryIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a placement under every tile of its footprint.
    pub fn add(&mut self, loc: Loc) {
        for dx in 0..loc.size.0.max(1) {
            for dz in 0..loc.size.1.max(1) {
                let tile = TileCoord::with_plane(loc.tile.x + dx, loc.tile.z + dz, loc.tile.plane);
                let here = self.by_tile.entry(tile).or_default();
                if !here.contains(&loc) {
                    here.push(loc);
                }
            }
        }
    }

    /// The placements covering `tile`.
    pub fn at(&self, tile: TileCoord) -> &[Loc] {
        self.by_tile.get(&tile).map_or(&[], Vec::as_slice)
    }
}

/// A placement shown as another object for `ticks_left` more ticks: on the same spot (a tree's
/// stump) or moved and turned (an opened door, which swings into the next tile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocChange {
    pub loc: Loc,
    /// What is there meanwhile, where and how it is turned.
    pub replacement: Loc,
    /// How many more ends of a tick it stays changed: it changes back at the end of the tick on
    /// which this reaches 0.
    pub ticks_left: i32,
}

/// A despawn timer (see `DepletionDef::Timer`): how many ticks of gathering it has left, and
/// whether anyone gathered from it this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DespawnTimer {
    remaining: i32,
    full: i32,
    worked_this_tick: bool,
}

/// What has happened to the scenery: the objects temporarily replaced, and the despawn timers of
/// the resources being gathered.
#[derive(Debug, Default, Clone)]
pub struct SceneryState {
    changes: BTreeMap<LocKey, LocChange>,
    timers: BTreeMap<LocKey, DespawnTimer>,
}

impl SceneryState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Shows `replacement` instead of `loc` for `ticks` ticks. Its despawn timer, if any, is done
    /// with.
    pub fn replace(&mut self, loc: Loc, replacement: Loc, ticks: i32) {
        self.timers.remove(&loc.key());
        self.changes.insert(loc.key(), LocChange { loc, replacement, ticks_left: ticks });
    }

    /// Puts `key`'s placement back before its time (a door closed again), returning the change.
    pub fn restore(&mut self, key: &LocKey) -> Option<LocChange> {
        self.changes.remove(key)
    }

    /// What stands in for `loc` now: its replacement while it has one, else itself.
    pub fn current(&self, loc: &Loc) -> Loc {
        self.changes.get(&loc.key()).map_or(*loc, |change| change.replacement)
    }

    /// The placement (as the map places it) showing object `id` over `tile` now: one whose
    /// replacement stands there (an opened door that swung in), or else an unchanged original.
    pub fn find(&self, index: &SceneryIndex, id: u32, tile: TileCoord) -> Option<Loc> {
        let moved = self.changes.values().find(|change| change.replacement.id == id && change.replacement.covers(tile));
        moved.map(|change| change.loc).or_else(|| index.at(tile).iter().copied().find(|loc| loc.id == id && !self.is_replaced(loc)))
    }

    pub fn is_replaced(&self, loc: &Loc) -> bool {
        self.changes.contains_key(&loc.key())
    }

    /// Every placement currently shown as something else, in a fixed order.
    pub fn changes(&self) -> impl Iterator<Item = &LocChange> {
        self.changes.values()
    }

    /// Notes that `loc` is being gathered from this tick, starting a despawn timer of `ticks` if
    /// it has none.
    pub fn work_timer(&mut self, loc: &Loc, ticks: i32) {
        let timer = self.timers.entry(loc.key()).or_insert(DespawnTimer { remaining: ticks, full: ticks, worked_this_tick: false });
        timer.worked_this_tick = true;
    }

    /// Whether `loc`'s despawn timer has run out (it has none: not yet).
    pub fn timer_expired(&self, loc: &Loc) -> bool {
        self.timers.get(&loc.key()).is_some_and(|timer| timer.remaining <= 0)
    }

    /// The end of a tick: timers worked this tick run down a tick and the others back up (one
    /// that is full again is forgotten), and the changes whose time is up are undone and returned
    /// (so whatever they changed, such as collision, can be put back too).
    pub fn advance(&mut self) -> Vec<LocChange> {
        self.timers.retain(|_, timer| {
            if std::mem::take(&mut timer.worked_this_tick) {
                timer.remaining = (timer.remaining - 1).max(0);
                true
            } else {
                timer.remaining += 1;
                timer.remaining < timer.full
            }
        });
        let mut expired = Vec::new();
        self.changes.retain(|_, change| {
            change.ticks_left -= 1;
            let done = change.ticks_left <= 0;
            if done {
                expired.push(*change);
            }
            !done
        });
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(x: i32, z: i32, size: i32) -> Loc {
        Loc { id: 1276, tile: TileCoord::new(x, z), kind: 10, orientation: 0, size: (size, size) }
    }

    #[test]
    fn a_placement_is_found_from_any_tile_it_covers() {
        let mut index = SceneryIndex::new();
        index.add(tree(10, 10, 2));

        for (x, z) in [(10, 10), (11, 10), (10, 11), (11, 11)] {
            assert_eq!(index.at(TileCoord::new(x, z)), &[tree(10, 10, 2)]);
        }
        assert!(index.at(TileCoord::new(12, 10)).is_empty());
        assert!(tree(10, 10, 2).covers(TileCoord::new(11, 11)));
        assert!(!tree(10, 10, 2).covers(TileCoord::new(9, 11)));
    }

    #[test]
    fn a_replacement_lasts_until_its_tick() {
        let mut state = SceneryState::new();
        let loc = tree(0, 0, 1);
        state.replace(loc, loc.with_id(1342), 2);

        assert_eq!(state.current(&loc).id, 1342);
        assert!(state.advance().is_empty());
        assert_eq!(state.current(&loc).id, 1342);
        let expired = state.advance();
        assert_eq!(expired.len(), 1);
        assert_eq!(state.current(&loc).id, 1276);
    }

    #[test]
    fn a_moved_replacement_is_found_where_it_now_stands_and_can_be_put_back() {
        let mut index = SceneryIndex::new();
        let door = Loc { id: 1535, tile: TileCoord::new(5, 5), kind: 0, orientation: 0, size: (1, 1) };
        index.add(door);
        let mut state = SceneryState::new();
        assert_eq!(state.find(&index, 1535, door.tile), Some(door));

        let open = Loc { id: 1536, tile: TileCoord::new(4, 5), orientation: 1, ..door };
        state.replace(door, open, 500);

        assert_eq!(state.find(&index, 1536, open.tile), Some(door), "the open door, where it swung to");
        assert_eq!(state.find(&index, 1535, door.tile), None, "the closed door is not there any more");
        assert_eq!(state.restore(&door.key()).map(|c| c.replacement), Some(open));
        assert_eq!(state.find(&index, 1535, door.tile), Some(door));
    }

    #[test]
    fn a_despawn_timer_runs_down_while_worked_and_back_up_while_not() {
        let mut state = SceneryState::new();
        let loc = tree(0, 0, 1);

        for _ in 0..3 {
            state.work_timer(&loc, 3);
            assert!(!state.timer_expired(&loc));
            state.advance();
        }
        assert!(state.timer_expired(&loc), "three ticks of work use up a three-tick timer");

        state.advance();
        assert!(!state.timer_expired(&loc), "a tick off lets it grow back one");
        state.advance();
        state.advance();
        assert!(state.timers.is_empty(), "a timer that is full again is forgotten");
    }
}
