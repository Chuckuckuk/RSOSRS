//! Pets that follow the player (the beaver from Woodcutting). A pet is an item (`ItemDef::follower`
//! names the monster it is while out); at most one follows the player at a time.
//!
//! Per the OSRS wiki (Pet, Beaver):
//! - a pet rolled with no follower out appears beside the player; with one out, it goes into the
//!   backpack (see `sim::gathering`)
//! - dropping a pet item lets it out to follow; its "Pick-up" option puts it back in the backpack
//! - it keeps to walking pace and appears beside the player when it gets stuck or left behind
//! - it walks through, and is walked through by, everything

use std::rc::Rc;

use super::entity::{Behavior, Entity};
use super::objects::{Approach, PlayerAction};
use super::world::World;
use crate::actors::EntityId;
use crate::pathfinding::TargetSpec;
use crate::tiles::TileCoord;

/// How far (in tiles, either way) a follower may be from the player before it appears beside them
/// instead of walking: off the edge of the view. The wiki only says it teleports when "stuck
/// and/or too far away"; this is the stand-in distance.
pub const CATCH_UP_DISTANCE: i32 = 15;

/// The pet following the player.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Follower {
    pub entity: EntityId,
    /// The item it turns back into when picked up.
    pub item: String,
}

impl World {
    /// The entity following the player, if one is.
    pub fn follower(&self) -> Option<EntityId> {
        self.follower.as_ref().map(|follower| follower.entity)
    }

    /// The item the follower is, if one is out.
    pub fn follower_item(&self) -> Option<&str> {
        self.follower.as_ref().map(|follower| follower.item.as_str())
    }

    /// Whether item `item` is a pet (it can follow the player).
    pub(super) fn is_pet(&self, item: &str) -> bool {
        self.registry().item(item).is_some_and(|def| def.follower.is_some())
    }

    /// Lets pet `item` out beside the player as its follower. Returns false (doing nothing) when
    /// one is already out or `item` is not a pet.
    pub(super) fn call_follower(&mut self, item: &str) -> bool {
        if self.follower.is_some() {
            return false;
        }
        let registry = Rc::clone(self.registry());
        let Some(def) = registry.item(item).and_then(|def| def.follower.as_ref()).and_then(|npc| registry.npc(npc)) else {
            return false;
        };
        let def = Rc::new(def.clone());
        let at = self.beside_player().unwrap_or_else(|| self.player.tile());
        let entity = self.add_follower_entity(at, &def);
        self.follower = Some(Follower { entity, item: item.to_owned() });
        true
    }

    /// A free tile next to the player, if there is one.
    fn beside_player(&self) -> Option<TileCoord> {
        super::world::summon_spots(&self.map, self.player.tile(), 1, &[], 1).into_iter().next()
    }

    /// The follower's tick: keep beside the player at walking pace (OSRS wiki, Pet: "Pets are
    /// limited to a walking pace"), so a running player draws ahead of it; or appear beside them
    /// when left behind (another plane, more than `CATCH_UP_DISTANCE` tiles, or no way through).
    pub(super) fn run_follower_movement(&mut self, entity: &mut Entity) {
        let player = self.player.tile();
        let behind = player.plane != entity.tile().plane || player.chebyshev_distance_to(entity.tile()) > CATCH_UP_DISTANCE;
        if behind || !self.walk_beside(entity, player, 1) {
            if let Some(tile) = self.beside_player() {
                entity.mover.teleport_to(&mut self.map, tile);
            }
        }
    }

    /// "Pick-up" on entity `target`: walk up to the player's own follower and take it back into
    /// the backpack. Anyone else's (there are none yet) or anything else is left alone.
    pub fn player_pick_up_follower(&mut self, target: EntityId) {
        if self.follower() == Some(target) {
            self.player.set_action(PlayerAction::PickUp { entity: target });
        }
    }

    /// One tick of picking up the follower. Returns that it used the tick.
    pub(super) fn run_pick_up_action(&mut self, target: EntityId) -> bool {
        let Some(tile) = self.entity(target).filter(|_| self.follower() == Some(target)).map(Entity::tile) else {
            self.player.action = None;
            return true;
        };
        match self.approach(TargetSpec::adjacent(tile, 1, 1)) {
            Approach::OnTheWay => return true,
            Approach::NoWay => {
                self.player.action = None;
                return true;
            }
            Approach::There => {}
        }
        self.player.action = None;
        if self.player.inventory.free_slots() == 0 {
            self.game_message("You don't have enough inventory space.".to_owned());
            return true;
        }
        if let Some(follower) = self.follower.take() {
            self.remove_entity(follower.entity);
            let _ = self.give_item(&follower.item, 1);
        }
        true
    }
}

impl Entity {
    /// Whether this is a pet following the player.
    pub fn is_follower(&self) -> bool {
        matches!(self.behavior, Behavior::Follower)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::Registry;
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::sim::{ClientCommand, PlayerConfig, Server, TickDriver};

    /// A flat world with the bundled data and a beaver in the player's backpack (slot 0).
    fn world_with_beaver() -> World {
        let mut world = dev_open_floor_world(30, 1, TileCoord::new(0, 0), PlayerConfig::new_account());
        world.set_registry(Rc::new(Registry::builtin()));
        world.give_item("beaver", 1).unwrap();
        world
    }

    fn ticks(world: &mut World, n: i32) {
        for _ in 0..n {
            world.run_tick();
        }
    }

    fn beaver_tile(world: &World) -> TileCoord {
        world.entity(world.follower().expect("a follower")).unwrap().tile()
    }

    #[test]
    fn dropping_a_pet_lets_it_out_beside_the_player_and_only_one_at_a_time() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");

        assert_eq!(world.player.inventory.count("beaver"), 0);
        assert_eq!(world.ground_items().count(), 0, "it follows rather than lying there");
        assert_eq!(world.player.tile().chebyshev_distance_to(beaver_tile(&world)), 1);

        world.give_item("beaver", 1).unwrap();
        world.player_inventory_option(0, "Drop");
        assert_eq!(world.player.inventory.count("beaver"), 1, "kept: one follower at a time");
        assert!(world.take_messages().contains(&"You already have a follower.".to_owned()));
    }

    #[test]
    fn the_follower_keeps_beside_the_walking_player() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");
        world.player.is_running = false;

        world.player_request_walk_to(TileCoord::new(6, 0), false);
        ticks(&mut world, 9);

        assert_eq!(world.player.tile(), TileCoord::new(6, 0));
        assert_eq!(world.player.tile().chebyshev_distance_to(beaver_tile(&world)), 1, "caught up and beside");
        assert_ne!(beaver_tile(&world), world.player.tile(), "never on the player");
    }

    #[test]
    fn a_running_player_draws_ahead_of_the_walking_follower_which_then_catches_up() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");
        world.player.is_running = true;
        let beaver = world.follower().unwrap();

        world.player_request_walk_to(TileCoord::new(12, 0), false);
        for tick in 0..14 {
            let before = beaver_tile(&world);
            world.run_tick();
            let entity = world.entity(beaver).unwrap();
            assert!(before.chebyshev_distance_to(entity.tile()) <= 1, "tick {tick}: one tile a tick, never a jump");
        }
        assert_eq!(world.player.tile(), TileCoord::new(12, 0));
        assert_eq!(world.player.tile().chebyshev_distance_to(beaver_tile(&world)), 1, "caught up once the player stopped");
    }

    #[test]
    fn a_follower_left_far_behind_appears_beside_the_player() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");

        let far = TileCoord::new(CATCH_UP_DISTANCE + 10, 0);
        world.player.mover.teleport_to(&mut world.map, far);
        ticks(&mut world, 1);

        assert_eq!(far.chebyshev_distance_to(beaver_tile(&world)), 1);
    }

    #[test]
    fn picking_the_follower_up_puts_it_back_in_the_backpack() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");
        let beaver = world.follower().unwrap();

        world.player_pick_up_follower(beaver);
        ticks(&mut world, 3);

        assert_eq!(world.follower(), None);
        assert!(world.entity(beaver).is_none(), "gone from the world");
        assert_eq!(world.player.inventory.count("beaver"), 1);
        ticks(&mut world, 2); // it no longer ticks
    }

    #[test]
    fn a_full_backpack_cannot_take_the_follower_back() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");
        world.give_item("shark", 28).unwrap();
        let beaver = world.follower().unwrap();

        world.player_pick_up_follower(beaver);
        ticks(&mut world, 3);

        assert_eq!(world.follower(), Some(beaver));
        assert!(world.take_messages().contains(&"You don't have enough inventory space.".to_owned()));
    }

    #[test]
    fn the_follower_cannot_be_attacked() {
        let mut world = world_with_beaver();
        world.player_inventory_option(0, "Drop");
        let beaver = world.follower().unwrap();

        world.player_set_attack_target(beaver);

        assert_eq!(world.player.attack_target(), None);
    }

    #[test]
    fn the_client_sees_the_follower_and_its_pick_up_option_takes_it_back() {
        let mut server = Server::new(world_with_beaver(), TickDriver::default());
        server.handle_command(ClientCommand::InventoryOption { slot: 0, option: "Drop".into() });
        let snapshot = server.snapshot();
        let beaver = snapshot.entities.iter().find(|e| e.name == "Beaver").expect("drawn");
        assert_eq!(beaver.appearance, Some(12169));
        assert_eq!(beaver.facing, Some(crate::sim::Combatant::Player));
        let id = beaver.id;

        server.handle_command(ClientCommand::InteractNpc { target: id, option: "Pick-up".into() });
        for _ in 0..5 {
            server.advance(0.6);
        }

        assert!(server.snapshot().entities.iter().all(|e| e.id != id));
        assert_eq!(server.world().player.inventory.count("beaver"), 1);
    }
}
