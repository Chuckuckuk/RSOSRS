//! Choosing an option on scenery, and doors.
//!
//! [`World::player_interact_object`] is the one place an object's option is dispatched: a door's
//! "Open"/"Close", a gathering node's option (`sim::gathering`), else "Nothing interesting
//! happens." Whatever is chosen becomes the player's [`PlayerAction`] and runs in its PlayerQueue
//! phase.
//!
//! Doors (OSRS wiki, "Door"):
//! - The player walks to where the door can be used from (`TargetSpec::wall_reach`) and it pops
//!   open or shut, with no animation. Every leaf of a double door or gate goes together.
//! - An opened door swings into the next tile and turns (`DoorSwing::moves`). Its wall moves with
//!   it, so the doorway can be walked through.
//! - A door that a player opened or closed goes back by itself after [`DOOR_RESET_TICKS`].
//! - Closing the same door [`STUCK_CLOSE_LIMIT`] times as fast as the game allows (each close
//!   needs an open in between, a tick each) gets "The door seems to be stuck." instead.

use std::rc::Rc;

use super::gathering::GatherAction;
use super::scenery::{Loc, LocKey};
use super::world::World;
use crate::data::{DoorDef, LeafMove};
use crate::pathfinding::{rotate, TargetSpec};
use crate::tiles::TileCoord;

/// How long a door a player opened or closed stays that way: 300 seconds.
pub const DOOR_RESET_TICKS: i32 = 500;

/// How many closes in a row, as fast as possible, make a door stick.
pub const STUCK_CLOSE_LIMIT: usize = 5;

/// The shortest time `STUCK_CLOSE_LIMIT` closes of one door can take: each close but the first
/// needs an open before it, and every door action takes a tick.
pub const STUCK_WINDOW_TICKS: i32 = (STUCK_CLOSE_LIMIT as i32 - 1) * 2;

/// Something the player is busy doing that is not fighting or walking: at most one at a time.
/// Walking, attacking or talking to an NPC cancels it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayerAction {
    Gather(GatherAction),
    Door(DoorAction),
}

/// Opening or closing a door.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoorAction {
    /// The `DoorDef` id.
    pub door: String,
    /// Its leaves as the map places them, in order along the wall.
    pub leaves: Vec<Loc>,
    pub closing: bool,
}

/// Where a leaf placed as `from` stands once it has moved by `step` (swung open), showing `id`.
fn swung(from: Loc, step: LeafMove, id: u32) -> Loc {
    let (dx, dz) = rotate(step.offset, from.orientation);
    Loc { id, tile: from.tile.offset(dx, dz), orientation: (from.orientation as i32 + step.turn).rem_euclid(4) as u8, ..from }
}

/// The other way: where a leaf standing open as `open` stands shut, showing `id`.
fn unswung(open: Loc, step: LeafMove, id: u32) -> Loc {
    let orientation = (open.orientation as i32 - step.turn).rem_euclid(4) as u8;
    let (dx, dz) = rotate(step.offset, orientation);
    Loc { id, tile: open.tile.offset(-dx, -dz), orientation, ..open }
}

/// The answer to an option that does nothing here.
fn nothing() -> Result<(), String> {
    Err("Nothing interesting happens.".to_owned())
}

/// How walking up to something went this tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Approach {
    /// The player can use it from where it stands.
    There,
    /// Still walking.
    OnTheWay,
    /// No path leads there.
    NoWay,
}

impl World {
    /// Choosing `action` on the scenery object `id` over `tile` (as the client sees it now: an
    /// opened door is where it swung to).
    pub fn player_interact_object(&mut self, id: u32, tile: TileCoord, action: &str) -> Result<(), String> {
        let Some(original) = self.scenery_state.find(&self.scenery, id, tile) else { return nothing() };
        let registry = Rc::clone(self.registry());
        if let Some(door) = registry.door_for(id) {
            let open_now = door.state_of(id) == Some(true);
            let option = if open_now { "Close" } else { "Open" };
            if action.eq_ignore_ascii_case(option) {
                return self.player_start_door(door, original, open_now);
            }
        }
        match registry.gathering_node_for(id, action) {
            Some(node) => self.player_start_gathering(node, original),
            None => nothing(),
        }
    }

    /// Sends the player to open or close `door`, whose leaf `original` (as the map places it) was
    /// chosen.
    fn player_start_door(&mut self, door: &DoorDef, original: Loc, closing: bool) -> Result<(), String> {
        if closing && !door.can_close {
            return nothing();
        }
        let leaves = self.door_leaves(door, original);
        if leaves.len() != door.swing.leaf_count() {
            return nothing(); // the other leaf is not where it should be
        }
        // a pair the map places open has no closed form to swing back to
        let placed_open = |leaf: &Loc| door.state_of(leaf.id) == Some(true) && !self.scenery_state.is_replaced(leaf);
        if leaves.len() > 1 && leaves.iter().any(placed_open) {
            return nothing();
        }
        self.player.set_action(PlayerAction::Door(DoorAction { door: door.id.clone(), leaves, closing }));
        Ok(())
    }

    /// The leaves of `door` that `leaf` belongs to, as the map places them, in order along the
    /// wall: the leaf itself for a single door; for a pair, it and its partner beside it.
    fn door_leaves(&self, door: &DoorDef, leaf: Loc) -> Vec<Loc> {
        if door.swing.leaf_count() == 1 {
            return vec![leaf];
        }
        let along = rotate((0, 1), leaf.orientation);
        for sign in [1, -1] {
            let beside = leaf.tile.offset(along.0 * sign, along.1 * sign);
            let partner = self.scenery.at(beside).iter().copied().find(|other| {
                other.kind == leaf.kind
                    && other.orientation == leaf.orientation
                    && door.state_of(other.id) == door.state_of(leaf.id)
                    && other.key() != leaf.key()
            });
            if let Some(partner) = partner {
                return if sign == 1 { vec![leaf, partner] } else { vec![partner, leaf] };
            }
        }
        vec![leaf]
    }

    /// One tick of opening or closing a door. Returns whether it used the tick.
    pub(super) fn run_door(&mut self, action: DoorAction) -> bool {
        let registry = Rc::clone(self.registry());
        let Some(door) = registry.door(&action.door) else {
            self.player.action = None;
            return false;
        };
        // someone else opened or closed it meanwhile: nothing left to do
        let current: Vec<Loc> = action.leaves.iter().map(|leaf| self.scenery_state.current(leaf)).collect();
        if current.iter().any(|leaf| door.state_of(leaf.id) != Some(action.closing)) {
            self.player.action = None;
            return true;
        }

        let tiles: Vec<TileCoord> =
            current.iter().flat_map(|leaf| TargetSpec::wall_reach(&self.map, leaf.tile, leaf.orientation)).collect();
        match self.approach(TargetSpec::any_of(&tiles)) {
            Approach::There => {}
            Approach::OnTheWay => return true,
            Approach::NoWay => {
                self.player.action = None;
                self.game_message("I can't reach that!".to_owned());
                return true;
            }
        }

        self.player.action = None;
        if action.closing && self.door_sticks(action.leaves[0].key()) {
            self.game_message("The door seems to be stuck.".to_owned());
            return true;
        }
        for (leaf, step) in action.leaves.iter().zip(door.swing.moves()) {
            if self.scenery_state.is_replaced(leaf) {
                // back to how the map places it
                self.restore_loc(&leaf.key());
            } else if let Some(forms) = door.leaf(leaf.id) {
                let moved = if action.closing { unswung(*leaf, *step, forms.closed) } else { swung(*leaf, *step, forms.open) };
                self.change_loc(*leaf, moved, DOOR_RESET_TICKS);
            }
        }
        true
    }

    /// Notes a close of `door` and says whether it sticks: whether, with this one, it has been
    /// closed `STUCK_CLOSE_LIMIT` times within the shortest time that takes. A close that sticks
    /// is not counted.
    fn door_sticks(&mut self, door: LocKey) -> bool {
        let now = self.encounter.elapsed_ticks();
        let (last, closes) = self.player.door_closes.get_or_insert((door, Vec::new()));
        if *last != door {
            *last = door;
            closes.clear();
        }
        closes.retain(|at| now - at <= STUCK_WINDOW_TICKS);
        if closes.len() + 1 >= STUCK_CLOSE_LIMIT {
            return true;
        }
        closes.push(now);
        false
    }

    /// Walks the player toward `reach`, unless it is there already.
    pub(super) fn approach(&mut self, reach: TargetSpec) -> Approach {
        if reach.is_satisfied_by(self.player.tile()) {
            return Approach::There;
        }
        self.player.approaching_target = true;
        self.player.current_path_is_running = self.player.is_running;
        if self.player.mover.request_move(&mut self.pathfinder, &mut self.map, reach) {
            Approach::OnTheWay
        } else {
            Approach::NoWay
        }
    }

    /// Shows `replacement` instead of `loc` for `ticks` ticks, moving its wall (if it is one).
    pub(super) fn change_loc(&mut self, loc: Loc, replacement: Loc, ticks: i32) {
        let shown = self.scenery_state.current(&loc);
        self.move_wall(&shown, &replacement);
        self.scenery_state.replace(loc, replacement, ticks);
    }

    /// Puts `key`'s placement back as the map has it, moving its wall back.
    pub(super) fn restore_loc(&mut self, key: &LocKey) {
        if let Some(change) = self.scenery_state.restore(key) {
            self.move_wall(&change.replacement, &change.loc);
        }
    }

    /// Takes `from`'s wall off the map and puts `to`'s on. Only wall shapes have wall bits (see
    /// `collision::wall_edges`); other objects keep their collision as it is (a stump blocks like
    /// its tree).
    fn move_wall(&mut self, from: &Loc, to: &Loc) {
        self.map.remove_wall(from.tile, from.kind, from.orientation);
        self.map.add_wall(to.tile, to.kind, to.orientation);
    }

    /// The scenery's end of a tick: changes whose time is up go back, walls and all.
    pub(super) fn advance_scenery(&mut self) {
        for change in self.scenery_state.advance() {
            self.move_wall(&change.replacement, &change.loc);
        }
    }

    /// The player's current action for this tick. Returns whether it used the tick (so a queued
    /// walk does not also run).
    pub(super) fn player_run_action(&mut self) -> bool {
        match self.player.action.clone() {
            Some(PlayerAction::Gather(gather)) => self.run_gather(gather),
            Some(PlayerAction::Door(door)) => self.run_door(door),
            None => false,
        }
    }

    /// The scenery object the player is turned toward while it works at it.
    pub fn player_facing_loc(&self) -> Option<Loc> {
        match &self.player.action {
            Some(PlayerAction::Gather(gather)) => Some(gather.loc),
            Some(PlayerAction::Door(door)) => door.leaves.first().map(|leaf| self.scenery_state.current(leaf)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::CollisionFlag;
    use crate::data::{DoorSwing, Registry};
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::sim::PlayerConfig;

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn wall(id: u32, x: i32, z: i32, orientation: u8) -> Loc {
        Loc { id, tile: tc(x, z), kind: 0, orientation, size: (1, 1) }
    }

    /// A flat world split by a wall on the west edge of x = 5 (its whole length), with `doors` set into
    /// it (their walls placed and indexed), the player standing at (0, 0) on the west side.
    fn walled(doors: &[Loc], registry: Registry) -> World {
        let mut world = dev_open_floor_world(10, 4242, tc(0, 0), PlayerConfig::default());
        world.set_registry(Rc::new(registry));
        for z in -10..=10 {
            if !doors.iter().any(|door| door.covers(tc(5, z)) || door.covers(tc(4, z))) {
                world.map.add_wall(tc(5, z), 0, 0);
            }
        }
        for door in doors {
            world.map.add_wall(door.tile, door.kind, door.orientation);
            world.add_scenery(*door);
        }
        world
    }

    fn run(world: &mut World, ticks: i32) {
        for _ in 0..ticks {
            world.run_tick();
        }
    }

    /// Walks the player to (8, 0) for long enough and says whether it got there.
    fn walks_through(world: &mut World) -> bool {
        world.player_request_walk_to(tc(8, 0), false);
        run(world, 40);
        world.player.tile() == tc(8, 0)
    }

    fn single_door() -> Loc {
        wall(1535, 5, 0, 0)
    }

    #[test]
    fn a_door_swings_into_the_next_tile_and_takes_its_wall_with_it() {
        let door = single_door();
        let mut world = walled(&[door], Registry::builtin());
        assert!(!walks_through(&mut world), "the closed door blocks the doorway");

        world.player_interact_object(1535, door.tile, "Open").unwrap();
        run(&mut world, 20);

        let changes: Vec<_> = world.scenery_changes().copied().collect();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].replacement, wall(1536, 4, 0, 1), "open: west of its frame, on that tile's north edge");
        assert_eq!(world.map.flags_at(door.tile) & CollisionFlag::BLOCK_WEST, 0, "the doorway is clear");
        assert_ne!(world.map.flags_at(tc(4, 0)) & CollisionFlag::BLOCK_NORTH, 0, "the open door is a wall where it stands");
        assert!(walks_through(&mut world));
    }

    #[test]
    fn an_opened_door_closes_itself_after_five_minutes() {
        let door = single_door();
        let mut world = walled(&[door], Registry::builtin());
        world.player_interact_object(1535, door.tile, "Open").unwrap();
        run(&mut world, 20);
        assert!(world.scenery_changes().next().is_some());

        run(&mut world, DOOR_RESET_TICKS);

        assert!(world.scenery_changes().next().is_none());
        assert_ne!(world.map.flags_at(door.tile) & CollisionFlag::BLOCK_WEST, 0, "shut again");
        assert_eq!(world.map.flags_at(tc(4, 0)) & CollisionFlag::BLOCK_NORTH, 0);
    }

    #[test]
    fn closing_an_opened_door_by_where_it_stands_puts_it_back() {
        let door = single_door();
        let mut world = walled(&[door], Registry::builtin());
        world.player_interact_object(1535, door.tile, "Open").unwrap();
        run(&mut world, 20);

        // the client sees it where it swung to, with its open id
        world.player_interact_object(1536, tc(4, 0), "Close").unwrap();
        run(&mut world, 5);

        assert!(world.scenery_changes().next().is_none());
        assert_ne!(world.map.flags_at(door.tile) & CollisionFlag::BLOCK_WEST, 0);
        assert!(world.player_interact_object(1536, tc(4, 0), "Close").is_err(), "nothing there now");
    }

    #[test]
    fn a_door_the_map_places_open_can_be_shut_until_it_resets() {
        // 1536 placed open: where 1535 would swing to from (5, 0)
        let open = wall(1536, 4, 0, 1);
        let mut world = walled(&[open], Registry::builtin());
        // its frame's wall is not there while it stands open
        world.map.remove_wall(tc(5, 0), 0, 0);
        assert!(walks_through(&mut world));
        world.player_request_walk_to(tc(0, 0), false);
        run(&mut world, 40);

        world.player_interact_object(1536, open.tile, "Close").unwrap();
        run(&mut world, 20);

        let changes: Vec<_> = world.scenery_changes().copied().collect();
        assert_eq!(changes[0].replacement, single_door(), "shut in its frame");
        assert!(!walks_through(&mut world));
        run(&mut world, DOOR_RESET_TICKS);
        assert!(world.scenery_changes().next().is_none(), "open again");
    }

    #[test]
    fn a_door_that_cannot_be_closed_is_left_open() {
        let mut registry = Registry::builtin();
        let mut bank = registry.door_for(1535).unwrap().clone();
        bank.can_close = false;
        registry.insert_door_for_test(bank);
        let mut world = walled(&[wall(1536, 4, 0, 1)], registry);

        assert_eq!(world.player_interact_object(1536, tc(4, 0), "Close"), Err("Nothing interesting happens.".to_owned()));
    }

    #[test]
    fn both_leaves_of_a_double_door_swing_out_together_and_back() {
        let (south, north) = (wall(1521, 5, 0, 0), wall(1524, 5, 1, 0));
        let mut world = walled(&[south, north], Registry::builtin());
        assert_eq!(world.registry().door_for(1524).map(|d| d.swing), Some(DoorSwing::Double));

        world.player_interact_object(1524, north.tile, "Open").unwrap();
        run(&mut world, 20);

        let mut moved: Vec<Loc> = world.scenery_changes().map(|c| c.replacement).collect();
        moved.sort_by_key(|leaf| leaf.tile.z);
        assert_eq!(moved, vec![wall(1522, 4, 0, 3), wall(1525, 4, 1, 1)], "each leaf swings out on its outer hinge");
        assert!(walks_through(&mut world));

        world.player_request_walk_to(tc(0, 0), false);
        run(&mut world, 40);
        world.player_interact_object(1522, tc(4, 0), "Close").unwrap();
        run(&mut world, 20);
        assert!(world.scenery_changes().next().is_none(), "both shut");
        assert!(!walks_through(&mut world));
    }

    #[test]
    fn a_gate_opens_both_leaves() {
        let (south, north) = (wall(1558, 5, 0, 0), wall(1560, 5, 1, 0));
        let mut world = walled(&[south, north], Registry::builtin());

        world.player_interact_object(1558, south.tile, "Open").unwrap();
        run(&mut world, 20);

        let ids: Vec<u32> = world.scenery_changes().map(|c| c.replacement.id).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&1559) && ids.contains(&1567), "{ids:?}");
        assert!(walks_through(&mut world));
    }

    #[test]
    fn options_that_do_not_fit_the_door_do_nothing() {
        let door = single_door();
        let mut world = walled(&[door], Registry::builtin());

        assert!(world.player_interact_object(1535, door.tile, "Close").is_err(), "it is shut already");
        assert!(world.player_interact_object(1536, tc(4, 0), "Close").is_err(), "no open door there");
    }

    /// Stands the player at the door, then opens and closes it `STUCK_CLOSE_LIMIT` times, one
    /// action a tick, except that the last open waits `last_gap` ticks before the last close.
    /// Returns the world and the messages after the last close.
    fn spam_closes(last_gap: i32) -> (World, Vec<String>) {
        let door = single_door();
        let mut world = walled(&[door], Registry::builtin());
        world.player_request_walk_to(tc(4, 0), false);
        run(&mut world, 10);
        for close in 0..STUCK_CLOSE_LIMIT {
            world.player_interact_object(1535, door.tile, "Open").unwrap();
            run(&mut world, if close == STUCK_CLOSE_LIMIT - 1 { last_gap } else { 1 });
            world.take_messages();
            world.player_interact_object(1536, tc(4, 0), "Close").unwrap();
            run(&mut world, 1);
        }
        let messages = world.take_messages();
        (world, messages)
    }

    #[test]
    fn closing_a_door_five_times_as_fast_as_possible_gets_it_stuck() {
        let (world, messages) = spam_closes(1);

        assert_eq!(messages, vec!["The door seems to be stuck.".to_owned()]);
        assert_eq!(world.scenery_changes().count(), 1, "it stays open");
    }

    #[test]
    fn closing_a_door_five_times_a_tick_slower_does_not() {
        let (world, messages) = spam_closes(2);

        assert!(messages.is_empty(), "{messages:?}");
        assert_eq!(world.scenery_changes().count(), 0, "shut");
    }

    #[test]
    fn swinging_open_and_back_is_the_same_door() {
        let step = DoorSwing::Single.moves()[0];
        for orientation in 0..4 {
            let shut = wall(1535, 7, 7, orientation);
            assert_eq!(unswung(swung(shut, step, 1536), step, 1535), shut, "orientation {orientation}");
        }
    }
}
