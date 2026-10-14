//! Items lying on the ground: dropped from the inventory, left by a burnt-out fire. The player
//! takes them by walking onto their tile; they despawn after `DESPAWN_TICKS`.
//!
//! Only the one player exists, so there is no owner and no private period: an item is simply
//! there for the 3 minutes (the game's 1 minute private + 2 public) and then gone.

use super::gathering::PlayerAction;
use super::world::World;
use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// How long an item lies on the ground: 3 minutes of 0.6 s ticks.
pub const DESPAWN_TICKS: i32 = 300;

/// One stack on a tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroundItem {
    /// Names this stack for as long as it lies there; never reused.
    pub uid: u32,
    pub item: String,
    pub quantity: u32,
    pub tile: TileCoord,
    pub ticks_left: i32,
}

/// Every item on the ground, in the order they landed.
#[derive(Debug, Clone, Default)]
pub struct GroundItems {
    items: Vec<GroundItem>,
    next_uid: u32,
}

impl GroundItems {
    /// Puts `quantity` of `item` on `tile`, onto the stack of it already there when `stackable`.
    /// Returns the uid of the stack.
    pub fn add(&mut self, item: &str, quantity: u32, tile: TileCoord, stackable: bool) -> u32 {
        if stackable {
            if let Some(stack) = self.items.iter_mut().find(|g| g.tile == tile && g.item == item) {
                stack.quantity = stack.quantity.saturating_add(quantity);
                stack.ticks_left = DESPAWN_TICKS;
                return stack.uid;
            }
        }
        self.next_uid += 1;
        let uid = self.next_uid;
        self.items.push(GroundItem { uid, item: item.to_owned(), quantity, tile, ticks_left: DESPAWN_TICKS });
        uid
    }

    pub fn get(&self, uid: u32) -> Option<&GroundItem> {
        self.items.iter().find(|g| g.uid == uid)
    }

    pub fn remove(&mut self, uid: u32) -> Option<GroundItem> {
        let index = self.items.iter().position(|g| g.uid == uid)?;
        Some(self.items.remove(index))
    }

    pub fn iter(&self) -> impl Iterator<Item = &GroundItem> {
        self.items.iter()
    }

    /// The end of a tick: stacks run down a tick, and the ones that have run out go.
    pub fn advance(&mut self) {
        self.items.retain_mut(|g| {
            g.ticks_left -= 1;
            g.ticks_left > 0
        });
    }
}

impl World {
    /// Puts items on the ground; returns the stack's uid.
    pub fn drop_ground_item(&mut self, item: &str, quantity: u32, tile: TileCoord) -> u32 {
        let stackable = self.item_is_stackable(item);
        self.ground_items.add(item, quantity, tile, stackable)
    }

    /// The items on the ground now.
    pub fn ground_items(&self) -> impl Iterator<Item = &GroundItem> {
        self.ground_items.iter()
    }

    /// "Take" on the ground stack `uid`: walk onto its tile, then pick it up.
    pub fn player_take_ground_item(&mut self, uid: u32) -> Result<(), String> {
        if self.ground_items.get(uid).is_none() {
            return Ok(()); // gone meanwhile: nothing happens, as in the game
        }
        self.player.set_action(PlayerAction::Take { uid });
        Ok(())
    }

    /// Walks the player towards the tile of `uid` when it is not on it. Returns whether it is
    /// there (`None`: the stack is gone).
    pub(super) fn walk_to_ground_item(&mut self, uid: u32) -> Option<bool> {
        let tile = self.ground_items.get(uid)?.tile;
        if self.player.tile() == tile {
            return Some(true);
        }
        self.player.approaching_target = true;
        self.player.current_path_is_running = self.player.is_running;
        self.player.mover.request_move(&mut self.pathfinder, &mut self.map, TargetSpec::exact(tile));
        Some(false)
    }

    /// One tick of taking. Returns that it used the tick.
    pub(super) fn run_take_action(&mut self, uid: u32) -> bool {
        match self.walk_to_ground_item(uid) {
            None => self.player.action = None,
            Some(false) => {}
            Some(true) => {
                self.player.action = None;
                let Some(stack) = self.ground_items.get(uid).cloned() else { return true };
                let left_over = self.give_item(&stack.item, stack.quantity).unwrap_or(stack.quantity);
                if left_over == stack.quantity {
                    self.game_message("You don't have enough inventory space to hold that item.".to_owned());
                } else if left_over == 0 {
                    self.ground_items.remove(uid);
                } else if let Some(g) = self.ground_items.items.iter_mut().find(|g| g.uid == uid) {
                    g.quantity = left_over;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Registry;
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::sim::PlayerConfig;
    use std::rc::Rc;

    fn world() -> World {
        let mut world = dev_open_floor_world(10, 1, TileCoord::new(0, 0), PlayerConfig::new_account());
        world.set_registry(Rc::new(Registry::builtin()));
        world
    }

    #[test]
    fn a_dropped_item_lands_on_the_players_tile_and_can_be_taken_back() {
        let mut world = world();
        world.give_item("logs", 1).unwrap();
        world.player_inventory_option(0, "Drop");

        let dropped: Vec<_> = world.ground_items().cloned().collect();
        assert_eq!(dropped.len(), 1);
        assert_eq!((dropped[0].item.as_str(), dropped[0].tile), ("logs", TileCoord::new(0, 0)));
        assert_eq!(world.player.inventory.count("logs"), 0);

        world.player_take_ground_item(dropped[0].uid).unwrap();
        world.run_tick();
        assert_eq!(world.player.inventory.count("logs"), 1);
        assert_eq!(world.ground_items().count(), 0);
    }

    #[test]
    fn the_player_walks_to_an_item_to_take_it() {
        let mut world = world();
        let uid = world.drop_ground_item("logs", 1, TileCoord::new(3, 0));
        world.player_take_ground_item(uid).unwrap();

        for _ in 0..10 {
            world.run_tick();
        }

        assert_eq!(world.player.tile(), TileCoord::new(3, 0));
        assert_eq!(world.player.inventory.count("logs"), 1);
    }

    #[test]
    fn a_full_inventory_leaves_the_item_and_says_so() {
        let mut world = world();
        world.give_item("shark", 28).unwrap();
        let uid = world.drop_ground_item("logs", 1, TileCoord::new(0, 0));
        world.player_take_ground_item(uid).unwrap();
        world.run_tick();

        assert_eq!(world.ground_items().count(), 1);
        assert!(world.take_messages().contains(&"You don't have enough inventory space to hold that item.".to_owned()));
    }

    #[test]
    fn items_despawn_after_three_minutes() {
        let mut world = world();
        world.drop_ground_item("logs", 1, TileCoord::new(5, 5));
        for _ in 0..DESPAWN_TICKS - 1 {
            world.run_tick();
        }
        assert_eq!(world.ground_items().count(), 1);
        world.run_tick();
        assert_eq!(world.ground_items().count(), 0);
    }
}
