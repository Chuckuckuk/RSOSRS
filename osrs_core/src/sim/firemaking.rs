//! Firemaking: lighting logs that lie on the ground with a tinderbox, rolling to light them on
//! the skill clock, and leaving a fire on the tile that burns out after the log's burn time (and
//! leaves ashes). Which logs burn, at what level, and for how long is data (`data::LogDef`).
//!
//! Ways in, all ending in a `PlayerAction::Light` on a ground stack:
//!
//! * the tinderbox used on a log in the inventory (either way round) drops the log on the
//!   player's tile and starts lighting it at once;
//! * the tinderbox used on a log on the ground, or the log's "Light" option, walk onto the
//!   log's tile first.
//!
//! The loop, run in the player's PlayerQueue phase like gathering:
//!
//! 1. Checks the level, the tinderbox and the tile, and says it is trying.
//! 2. The first attempt comes `FIRST_ATTEMPT_TICKS` after the starting tick and then one every
//!    `ATTEMPT_INTERVAL` ticks (the skill clock, as for gathering). The chance is 65/256 at
//!    level 1 rising in a straight line to 100% at level 43 (OSRS wiki, "Firemaking").
//! 3. A success takes the log off the ground, gives the experience and puts a fire on its tile;
//!    the player then steps off it: west if it can, else east, south or north.
//!
//! A player who walks away leaves the log lying there, as in the game. Fires block lighting on
//! their own tile only (not doorways, ferns and the like).
//!
//! # Bonfires (Forester's campfires)
//!
//! Logs used on a fire (one a player lit) make it a bonfire: object `BONFIRE_LOC`, whose burn
//! time is the log's `bonfire_ticks` on top of `BONFIRE_BASE_TICKS`. Starting one takes a
//! tinderbox and no other bonfire within `BONFIRE_SPACING` tiles. A bonfire's "Tend-to" (or more
//! logs used on it) adds a log every `TEND_INTERVAL` ticks, from the tile beside it or diagonal,
//! for the full Firemaking experience of that log, until the logs run out. Each log restarts the
//! burn time at `BONFIRE_BASE_TICKS` plus its `bonfire_ticks`. No tinderbox is needed to tend one. "Check" says roughly how long is left.
//! A bonfire that burns out leaves ashes like a fire.
//!
//! "Rest" (the bonfire's left-click) walks the player up to it and redirects its respawn to the
//! tile it stands on (`World::redirect_respawn`), for as long as that bonfire burns: a death
//! meanwhile brings the player back there instead of at the usual respawn point. Each log put on
//! a bonfire grants it `RESPAWNS_PER_LOG` respawns; each death that respawns there uses one, and
//! the bonfire burns out (leaving ashes) with the last, if its timer has not already done so.
//!
//! Random draws come from the player's own skilling generator: one `0..256` per attempt.

use std::rc::Rc;

use super::objects::{Approach, PlayerAction};
use super::scenery::{Loc, SpawnedLoc};
use super::world::World;
use crate::collision::OccupancyMask;
use crate::model::Skill;
use crate::pathfinding::TargetSpec;
use crate::rng::Rng;
use crate::rules;
use crate::tiles::{Direction, TileCoord};

/// The item used to light logs.
pub const TINDERBOX: &str = "tinderbox";
/// The cache object of a Forester's campfire ("Bonfire" here).
pub const BONFIRE_LOC: u32 = 49927;
/// The cache sequence of adding logs to a bonfire (kneeling, as for lighting, until the real one
/// is found).
pub const TENDING_ANIMATION: u32 = 733;
/// A new bonfire burns this many ticks plus its first log's `bonfire_ticks` (the wiki's measured
/// duration column is `burn_ticks`, which is this plus that).
const BONFIRE_BASE_TICKS: i32 = 99;
/// Bonfires are at least this many tiles (Chebyshev) from each other: one nearer is refused.
const BONFIRE_SPACING: i32 = 5;
/// How many respawns each log put on a bonfire grants it.
const RESPAWNS_PER_LOG: i32 = 1;
/// Ticks between logs added, and from starting to the first one.
const TEND_INTERVAL: i32 = 9;
const TEND_FIRST_TICKS: i32 = 1;
/// A bonfire's options, the left-click one first.
pub const BONFIRE_OPTIONS: [&str; 3] = ["Rest", "Tend-to", "Check"];
/// What "Check" says by how much is left: one message per 60 ticks, the last for 240 and over.
const CHECK_MESSAGES: [&str; 5] = [
    "The embers glow softly.",
    "The flames flicker gently.",
    "The fire burns steadily.",
    "The fire burns brightly.",
    "The roaring fire crackles invitingly.",
];
/// What a burnt-out fire leaves.
pub const ASHES: &str = "ashes";
/// The cache object a lit fire is drawn as.
pub const FIRE_LOC: u32 = 26185;
/// The cache sequence of kneeling to light a fire.
pub const LIGHTING_ANIMATION: u32 = 733;

/// Ticks from the tick that starts the action to the first attempt, and between attempts after it.
const FIRST_ATTEMPT_TICKS: i32 = 3;
const ATTEMPT_INTERVAL: i32 = 4;
/// The `low`/`high` of `rules::success_numerator` for lighting a fire: 65/256 at level 1 and
/// 100% from level 43 (the wiki's crowd-sourced line, 513/256 at 99 before the cap).
const CHANCE: [i32; 2] = [64, 512];

/// Where the player steps after lighting: the game tries west, east, south, then north.
const STEP_ORDER: [Direction; 4] = [Direction::West, Direction::East, Direction::South, Direction::North];

/// Lighting the log lying on the ground as stack `uid`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightAction {
    /// The ground stack.
    pub uid: u32,
    /// Whether the attempt has begun (said so, is kneeling).
    pub started: bool,
}

/// Adding logs to the fire standing on `tile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TendAction {
    /// The tile of the fire.
    pub tile: TileCoord,
    /// The log being added, or `None` for whichever the player has first ("Tend-to").
    pub log: Option<String>,
    /// Whether the player has arrived and begun (is kneeling).
    pub started: bool,
}

/// Resting by the bonfire standing on `tile`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestAction {
    /// The tile of the bonfire.
    pub tile: TileCoord,
}

impl World {
    /// Using the inventory item in slot `used` on the one in slot `target`. Only a tinderbox on a
    /// log does anything yet; the answer to any other pair is the game's own. The log is dropped
    /// on the player's tile and lit from there.
    pub fn player_use_item_on_item(&mut self, used: usize, target: usize) -> Result<(), String> {
        let (Some(a), Some(b)) = (self.player.inventory.get(used), self.player.inventory.get(target)) else {
            return Ok(()); // an empty slot: nothing happens
        };
        if used == target {
            return Ok(());
        }
        let (log_slot, log) = match (a.item == TINDERBOX, b.item == TINDERBOX) {
            (true, false) => (target, b.item.clone()),
            (false, true) => (used, a.item.clone()),
            _ => return Err("Nothing interesting happens.".to_owned()),
        };
        if self.registry().log(&log).is_none() {
            return Err("Nothing interesting happens.".to_owned());
        }
        self.check_lightable(&log, self.player.tile())?;
        self.player.inventory.take_one(log_slot);
        let tile = self.player.tile();
        let uid = self.drop_ground_item(&log, 1, tile);
        self.player.set_action(PlayerAction::Light(LightAction { uid, started: false }));
        Ok(())
    }

    /// Using the inventory item in `slot` on the ground stack `uid`: a tinderbox on a log lights it.
    pub fn player_use_item_on_ground_item(&mut self, slot: usize, uid: u32) -> Result<(), String> {
        let Some(used) = self.player.inventory.get(slot).map(|s| s.item.clone()) else { return Ok(()) };
        let Some(ground) = self.ground_items.get(uid).cloned() else { return Ok(()) };
        if used != TINDERBOX || self.registry().log(&ground.item).is_none() {
            return Err("Nothing interesting happens.".to_owned());
        }
        self.player_light_ground_item(uid)
    }

    /// The "Light" option of the log lying as stack `uid`: walk onto it and light it.
    pub fn player_light_ground_item(&mut self, uid: u32) -> Result<(), String> {
        let Some(ground) = self.ground_items.get(uid).cloned() else { return Ok(()) };
        if self.registry().log(&ground.item).is_none() {
            return Err("Nothing interesting happens.".to_owned());
        }
        self.check_lightable(&ground.item, ground.tile)?;
        self.player.set_action(PlayerAction::Light(LightAction { uid, started: false }));
        Ok(())
    }

    /// Using the inventory item in `slot` on the scenery object `id` at `tile`: a log on a fire
    /// feeds it. Nothing else does anything.
    pub fn player_use_item_on_object(&mut self, slot: usize, id: u32, tile: TileCoord) -> Result<(), String> {
        let Some(used) = self.player.inventory.get(slot).map(|s| s.item.clone()) else { return Ok(()) };
        let fire = self.scenery_state.spawned_at(tile).filter(|fire| fire.loc.id == id);
        let (Some(fire), Some(_)) = (fire, self.registry().log(&used)) else {
            return Err("Nothing interesting happens.".to_owned());
        };
        self.check_tendable(&fire.loc, &used)?;
        self.player.set_action(PlayerAction::Tend(TendAction { tile: fire.loc.tile, log: Some(used), started: false }));
        Ok(())
    }

    /// Choosing `action` on a fire a player made. Only a bonfire has options.
    pub(super) fn player_interact_fire(&mut self, fire: Loc, action: &str) -> Result<(), String> {
        if fire.id != BONFIRE_LOC {
            return Err("Nothing interesting happens.".to_owned());
        }
        match action.to_ascii_lowercase().as_str() {
            "tend-to" => {
                let log = self.first_bonfire_log().ok_or("You have no logs to add to the fire.")?;
                self.check_tendable(&fire, &log)?;
                self.player.set_action(PlayerAction::Tend(TendAction { tile: fire.tile, log: None, started: false }));
                Ok(())
            }
            "rest" => {
                self.player.set_action(PlayerAction::Rest(RestAction { tile: fire.tile }));
                Ok(())
            }
            "check" => {
                let left = self.scenery_state.spawned_at(fire.tile).map_or(0, |fire| fire.ticks_left);
                self.game_message(CHECK_MESSAGES[((left + 1) / 60).clamp(0, 4) as usize].to_owned());
                Ok(())
            }
            _ => Err("Nothing interesting happens.".to_owned()),
        }
    }

    /// The first log in the inventory a bonfire takes.
    fn first_bonfire_log(&self) -> Option<String> {
        let registry = self.registry();
        self.player.inventory.slots().iter().flatten().map(|stack| &stack.item).find(|item| registry.log(item).is_some()).cloned()
    }

    /// What stops the player adding `log` to `fire`, as the message it is told.
    fn check_tendable(&self, fire: &Loc, log: &str) -> Result<(), String> {
        let Some(def) = self.registry().log(log) else { return Err("Nothing interesting happens.".to_owned()) };
        if self.player.skills.current(Skill::Firemaking) < def.level {
            let name = self.item_name(log).to_lowercase();
            return Err(format!("You need a Firemaking level of {} to burn {name}.", def.level));
        }
        if fire.id == BONFIRE_LOC {
            return Ok(());
        }
        // starting one
        if self.player.inventory.count(TINDERBOX) == 0 {
            return Err("You need a tinderbox to light a fire.".to_owned());
        }
        let near = self.scenery_state.spawned().any(|other| {
            other.loc.id == BONFIRE_LOC && TargetSpec::distance_to_rect(other.loc.tile, fire.tile, 1, 1) <= BONFIRE_SPACING
        });
        if near {
            return Err("There's a Forester's Campfire nearby, help tend to that one or move further away.".to_owned());
        }
        Ok(())
    }

    /// One tick of adding logs. Returns that it used the tick.
    pub(super) fn run_tend_action(&mut self, mut tend: TendAction) -> bool {
        // burnt out meanwhile: stop without a word
        let Some(fire) = self.scenery_state.spawned_at(tend.tile).filter(|f| f.loc.id == FIRE_LOC || f.loc.id == BONFIRE_LOC) else {
            self.player.action = None;
            return true;
        };
        let reach = TargetSpec::within_range_of_rect(fire.loc.tile, 1, 1, 1);
        if !reach.is_satisfied_by(self.player.tile()) {
            self.player.approaching_target = true;
            self.player.current_path_is_running = self.player.is_running;
            self.player.mover.request_move(&mut self.pathfinder, &mut self.map, reach);
            return true;
        }

        // the log being added, if the player still has it; running out of logs ends it
        let has = |world: &World, log: &String| world.player.inventory.count(log) > 0;
        let log = match &tend.log {
            Some(log) => Some(log.clone()).filter(|log| has(self, log)),
            None => self.first_bonfire_log(),
        };
        let Some(log) = log else {
            if !tend.started {
                self.game_message("You have no logs to add to the fire.".to_owned());
            }
            self.player.action = None;
            return true;
        };
        if let Err(message) = self.check_tendable(&fire.loc, &log) {
            self.game_message(message);
            self.player.action = None;
            return true;
        }
        if !tend.started {
            tend.started = true;
            self.player.skill_clock = self.player.skill_clock.max(TEND_FIRST_TICKS);
        }
        if self.player.skill_clock > 0 {
            self.player.action = Some(PlayerAction::Tend(tend));
            return true;
        }
        self.player.skill_clock = TEND_INTERVAL;

        let registry = Rc::clone(self.registry());
        let def = registry.log(&log).expect("checked above");
        let slot = self.player.inventory.slots().iter().position(|s| s.as_ref().is_some_and(|s| s.item == log));
        if let Some(slot) = slot {
            self.player.inventory.take_one(slot);
        }
        self.grant_xp(Skill::Firemaking, def.xp_tenths);
        if fire.loc.id == BONFIRE_LOC {
            // the timer starts over, as on a new bonfire lit with this log (never shortened)
            let ticks = (BONFIRE_BASE_TICKS + def.bonfire_ticks).max(fire.ticks_left);
            self.scenery_state.spawn(fire.loc, ticks);
            self.scenery_state.add_respawns(&fire.loc, RESPAWNS_PER_LOG);
        } else {
            // a new bonfire stands where the fire was, and lasts by this log alone
            self.scenery_state.despawn(&fire.loc);
            let bonfire = Loc { id: BONFIRE_LOC, ..fire.loc };
            self.scenery_state.spawn(bonfire, BONFIRE_BASE_TICKS + def.bonfire_ticks);
            self.scenery_state.add_respawns(&bonfire, RESPAWNS_PER_LOG);
        }
        self.player.action = Some(PlayerAction::Tend(tend));
        true
    }

    /// One tick of walking up to a bonfire to rest by it. Returns that it used the tick.
    pub(super) fn run_rest_action(&mut self, rest: RestAction) -> bool {
        // burnt out meanwhile: stop without a word
        let Some(fire) = self.scenery_state.spawned_at(rest.tile).filter(|f| f.loc.id == BONFIRE_LOC) else {
            self.player.action = None;
            return true;
        };
        match self.approach(TargetSpec::within_range_of_rect(fire.loc.tile, 1, 1, 1)) {
            Approach::There => {}
            Approach::OnTheWay => return true,
            Approach::NoWay => {
                self.player.action = None;
                self.game_message("I can't reach that!".to_owned());
                return true;
            }
        }
        self.player.action = None;
        let tile = self.player.tile();
        self.redirect_respawn(tile, fire.loc);
        self.game_message("You rest by the fire. If you die while it burns, you will wake up here.".to_owned());
        true
    }

    /// What stops the player lighting `log` on `tile`, as the message it is told.
    fn check_lightable(&self, log: &str, tile: TileCoord) -> Result<(), String> {
        let Some(def) = self.registry().log(log) else { return Err("Nothing interesting happens.".to_owned()) };
        if self.player.skills.current(Skill::Firemaking) < def.level {
            let name = self.item_name(log).to_lowercase();
            return Err(format!("You need a Firemaking level of {} to burn {name}.", def.level));
        }
        if self.player.inventory.count(TINDERBOX) == 0 {
            return Err("You need a tinderbox to light a fire.".to_owned());
        }
        if self.scenery_state.has_spawned_at(tile) {
            return Err("You can't light a fire here.".to_owned());
        }
        Ok(())
    }

    /// One tick of lighting. Returns that it used the tick.
    pub(super) fn run_light_action(&mut self, mut light: LightAction) -> bool {
        // someone has taken it, or it has despawned: stop without a word
        let Some(ground) = self.ground_items.get(light.uid).cloned() else {
            self.player.action = None;
            return true;
        };
        if self.walk_to_ground_item(light.uid) != Some(true) {
            return true; // still on its way
        }
        if let Err(message) = self.check_lightable(&ground.item, ground.tile) {
            self.game_message(message);
            self.player.action = None;
            return true;
        }
        if !light.started {
            light.started = true;
            self.game_message("You attempt to light the logs.".to_owned());
            self.player.skill_clock = self.player.skill_clock.max(FIRST_ATTEMPT_TICKS);
        }
        if self.player.skill_clock > 0 {
            self.player.action = Some(PlayerAction::Light(light));
            return true;
        }
        self.player.skill_clock = ATTEMPT_INTERVAL;
        let level = self.player.skills.current(Skill::Firemaking);
        let numerator = rules::success_numerator(CHANCE[0], CHANCE[1], level);
        let draw = self.player.skilling_rng.next_range(0, 256);
        if !rules::attempt_succeeds(numerator, draw) {
            self.player.action = Some(PlayerAction::Light(light));
            return true;
        }

        let registry = Rc::clone(self.registry());
        let def = registry.log(&ground.item).expect("checked by check_lightable");
        self.ground_items.remove(light.uid);
        self.game_message("The fire catches and the logs begin to burn.".to_owned());
        self.grant_xp(Skill::Firemaking, def.xp_tenths);

        let tile = self.player.tile();
        let fire = Loc { id: FIRE_LOC, tile, kind: 10, orientation: 0, size: (1, 1) };
        self.scenery_state.spawn(fire, def.burn_ticks);
        self.player.action = None;
        if let Some(step) = self.step_off_fire(tile) {
            self.player.mover.request_move(&mut self.pathfinder, &mut self.map, TargetSpec::exact(step));
        }
        true
    }

    /// The tile the player steps to from the fire it just lit on `tile`.
    fn step_off_fire(&self, tile: TileCoord) -> Option<TileCoord> {
        STEP_ORDER.into_iter().find_map(|direction| {
            let next = TileCoord::with_plane(tile.x + direction.delta_x(), tile.z + direction.delta_z(), tile.plane);
            let open = self.map.can_traverse(tile, direction, 1, OccupancyMask::NONE);
            (open && !self.scenery_state.has_spawned_at(next)).then_some(next)
        })
    }

    /// The fires burning now, in a fixed order.
    pub fn spawned_scenery(&self) -> impl Iterator<Item = &SpawnedLoc> {
        self.scenery_state.spawned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::sim::{PlayerConfig, WorldEvent};

    fn world_with(level: i32, items: &[&str]) -> World {
        world_seeded(level, items, 4242)
    }

    fn world_seeded(level: i32, items: &[&str], seed: i32) -> World {
        let mut world = dev_open_floor_world(10, seed, TileCoord::new(5, 5), PlayerConfig::new_account());
        world.set_registry(Rc::new(crate::data::Registry::builtin()));
        world.player.skills.set_base(Skill::Firemaking, level);
        for item in items {
            world.give_item(item, 1).unwrap();
        }
        world
    }

    fn run_until(world: &mut World, limit: i32, done: impl Fn(&World) -> bool) -> i32 {
        for ran in 0..limit {
            if done(world) {
                return ran;
            }
            world.run_tick();
        }
        limit
    }

    fn fires(world: &World) -> usize {
        world.spawned_scenery().count()
    }

    #[test]
    fn a_tinderbox_on_logs_lights_a_fire_and_the_player_steps_west() {
        let mut world = world_with(43, &["tinderbox", "logs"]);
        let before = world.player.skills.xp_tenths(Skill::Firemaking);
        world.player_use_item_on_item(0, 1).unwrap();

        run_until(&mut world, 40, |w| fires(w) == 1);
        run_until(&mut world, 3, |_| false);

        let fire = world.spawned_scenery().next().unwrap();
        assert_eq!((fire.loc.id, fire.loc.tile), (FIRE_LOC, TileCoord::new(5, 5)));
        assert_eq!(world.player.tile(), TileCoord::new(4, 5), "one tile west of the fire");
        assert_eq!(world.player.inventory.count("logs"), 0, "the log is burnt");
        assert_eq!(world.ground_items().count(), 0, "and is not on the ground either");
        assert_eq!(world.player.skills.xp_tenths(Skill::Firemaking) - before, 400);
        assert!(world.take_events().contains(&WorldEvent::Experience { skill: Skill::Firemaking, gained_tenths: 400 }));
        let messages = world.take_messages();
        assert!(messages.contains(&"You attempt to light the logs.".to_owned()), "{messages:?}");
        assert!(messages.contains(&"The fire catches and the logs begin to burn.".to_owned()), "{messages:?}");
    }

    #[test]
    fn the_item_used_first_does_not_matter() {
        let mut world = world_with(43, &["logs", "tinderbox"]);
        assert_eq!(world.player_use_item_on_item(0, 1), Ok(()));
        assert!(world.player.action().is_some());

        let mut world = world_with(43, &["logs", "tinderbox"]);
        assert_eq!(world.player_use_item_on_item(1, 0), Ok(()));
        assert!(world.player.action().is_some());
    }

    #[test]
    fn logs_above_the_players_level_are_refused() {
        let mut world = world_with(1, &["tinderbox", "oak_logs"]);

        assert_eq!(world.player_use_item_on_item(0, 1), Err("You need a Firemaking level of 15 to burn oak logs.".to_owned()));
        assert!(world.player.action().is_none());
    }

    #[test]
    fn other_item_pairs_do_nothing_interesting() {
        let mut world = world_with(1, &["logs", "shark"]);
        assert_eq!(world.player_use_item_on_item(0, 1), Err("Nothing interesting happens.".to_owned()));

        let mut world = world_with(1, &["tinderbox", "shark"]);
        assert_eq!(world.player_use_item_on_item(0, 1), Err("Nothing interesting happens.".to_owned()));
    }

    #[test]
    fn a_fire_burns_out_after_the_logs_burn_time() {
        let mut world = world_with(43, &["tinderbox", "logs"]);
        world.player_use_item_on_item(0, 1).unwrap();
        run_until(&mut world, 40, |w| fires(w) == 1);

        let ran = run_until(&mut world, 200, |w| fires(w) == 0);

        assert!((100..=102).contains(&ran), "a log burns for 102 ticks, but {ran} passed");
    }

    #[test]
    fn the_player_steps_east_when_west_has_a_fire_and_cannot_light_on_a_fire() {
        let mut world = world_with(43, &["tinderbox", "logs", "logs"]);
        let west = Loc { id: FIRE_LOC, tile: TileCoord::new(4, 5), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(west, 1000);
        world.player_use_item_on_item(0, 1).unwrap();
        run_until(&mut world, 40, |w| fires(w) == 2);
        run_until(&mut world, 3, |_| false);

        assert_eq!(world.player.tile(), TileCoord::new(6, 5), "east, as west has a fire");

        world.player_request_walk_to(TileCoord::new(5, 5), false);
        run_until(&mut world, 5, |_| false);
        assert_eq!(world.player.tile(), TileCoord::new(5, 5));
        assert_eq!(world.player_use_item_on_item(0, 2), Err("You can't light a fire here.".to_owned()));
    }

    /// A world with a fire lit at (5, 5) and the player on (4, 5), holding `items` (a tinderbox
    /// is among them unless `items` says otherwise).
    fn world_beside_fire(level: i32, items: &[&str]) -> World {
        let mut world = world_with(level, items);
        let fire = Loc { id: FIRE_LOC, tile: TileCoord::new(5, 5), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 1000);
        world.player_request_walk_to(TileCoord::new(4, 5), false);
        run_until(&mut world, 5, |_| false);
        assert_eq!(world.player.tile(), TileCoord::new(4, 5));
        world
    }

    fn bonfire(world: &World) -> Option<SpawnedLoc> {
        world.spawned_scenery().find(|f| f.loc.id == BONFIRE_LOC).copied()
    }

    #[test]
    fn logs_used_on_a_fire_make_a_bonfire_that_lasts_by_that_log() {
        let mut world = world_beside_fire(43, &["tinderbox", "oak_logs"]);
        let before = world.player.skills.xp_tenths(Skill::Firemaking);

        world.player_use_item_on_object(1, FIRE_LOC, TileCoord::new(5, 5)).unwrap();
        run_until(&mut world, 10, |w| bonfire(w).is_some());

        let made = bonfire(&world).expect("a bonfire stands there");
        assert_eq!(made.loc.tile, TileCoord::new(5, 5));
        assert_eq!(world.spawned_scenery().count(), 1, "the fire it was is gone");
        assert!((108..=109).contains(&made.ticks_left), "99 + 10 for oak, {}", made.ticks_left);
        assert_eq!(world.player.inventory.count("oak_logs"), 0);
        assert_eq!(world.player.skills.xp_tenths(Skill::Firemaking) - before, 600, "the full experience");
    }

    #[test]
    fn starting_a_bonfire_takes_a_tinderbox_and_room_but_tending_one_does_not() {
        let mut world = world_beside_fire(43, &["logs"]);
        assert_eq!(
            world.player_use_item_on_object(0, FIRE_LOC, TileCoord::new(5, 5)),
            Err("You need a tinderbox to light a fire.".to_owned())
        );

        let mut world = world_beside_fire(43, &["tinderbox", "logs"]);
        let near = Loc { id: BONFIRE_LOC, tile: TileCoord::new(10, 5), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(near, 1000);
        assert!(world.player_use_item_on_object(1, FIRE_LOC, TileCoord::new(5, 5)).unwrap_err().contains("Forester's Campfire nearby"));
        let far = Loc { tile: TileCoord::new(11, 5), ..near };
        world.scenery_state.despawn(&near);
        world.scenery_state.spawn(far, 1000);
        assert_eq!(world.player_use_item_on_object(1, FIRE_LOC, TileCoord::new(5, 5)), Ok(()));

        // a bonfire takes logs with no tinderbox on the player
        let mut world = world_with(43, &["logs"]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(5, 6), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 50);
        assert_eq!(world.player_interact_object(BONFIRE_LOC, TileCoord::new(5, 6), "Tend-to"), Ok(()));
        run_until(&mut world, 5, |w| w.player.inventory.count("logs") == 0);
        assert_eq!(bonfire(&world).map(|b| b.ticks_left > 50), Some(true));
    }

    #[test]
    fn each_log_restarts_the_burn_time() {
        let mut world = world_with(90, &["redwood_logs", "redwood_logs"]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(5, 6), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 250);
        world.player_interact_object(BONFIRE_LOC, TileCoord::new(5, 6), "Tend-to").unwrap();

        run_until(&mut world, 20, |w| w.player.inventory.count("redwood_logs") < 2);
        let left = bonfire(&world).unwrap().ticks_left;
        assert!((248..=250).contains(&left), "kept, not cut to 144: {left}");

        let mut world = world_with(90, &["redwood_logs"]);
        world.scenery_state.spawn(fire, 20);
        world.player_interact_object(BONFIRE_LOC, TileCoord::new(5, 6), "Tend-to").unwrap();
        run_until(&mut world, 20, |w| w.player.inventory.count("redwood_logs") == 0);
        let left = bonfire(&world).unwrap().ticks_left;
        assert!((BONFIRE_BASE_TICKS + 45 - 2..=BONFIRE_BASE_TICKS + 45).contains(&left), "restarted: {left}");
    }

    #[test]
    fn logs_follow_one_another_nine_ticks_apart() {
        let mut world = world_with(43, &["logs", "logs", "logs"]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(5, 6), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 100);
        world.player_interact_object(BONFIRE_LOC, TileCoord::new(5, 6), "Tend-to").unwrap();

        let first = run_until(&mut world, 20, |w| w.player.inventory.count("logs") == 2);
        let second = run_until(&mut world, 20, |w| w.player.inventory.count("logs") == 1);

        assert_eq!(second, TEND_INTERVAL, "first at {first}");
    }

    #[test]
    fn check_names_how_much_is_left() {
        let mut world = world_with(1, &[]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(5, 6), kind: 10, orientation: 0, size: (1, 1) };
        for (left, message) in [(1, "The embers glow softly."), (58, "The embers glow softly."), (59, "The flames flicker gently."),
            (179, "The fire burns brightly."), (238, "The fire burns brightly."), (239, "The roaring fire crackles invitingly."), (300, "The roaring fire crackles invitingly.")]
        {
            world.scenery_state.spawn(fire, left);
            world.player_interact_object(BONFIRE_LOC, fire.tile, "Check").unwrap();
            assert_eq!(world.take_messages(), vec![message.to_owned()], "{left} ticks");
        }
    }

    #[test]
    fn a_burnt_out_bonfire_leaves_ashes() {
        let mut world = world_with(1, &[]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(5, 6), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 2);
        run_until(&mut world, 5, |w| fires(w) == 0);
        assert!(world.ground_items().any(|g| g.item == ASHES && g.tile == fire.tile));
    }

    #[test]
    fn low_levels_take_longer_to_light() {
        let ticks = |level: i32, seed: i32| {
            let mut world = world_seeded(level, &["tinderbox", "logs"], seed);
            world.player_use_item_on_item(0, 1).unwrap();
            run_until(&mut world, 3000, |w| fires(w) == 1)
        };
        let low: i32 = (0..20).map(|seed| ticks(1, seed)).sum();
        let high: i32 = (0..20).map(|seed| ticks(43, seed)).sum();

        assert!(low > high, "{low} vs {high}");
        assert!((0..20).all(|seed| ticks(43, seed) == FIRST_ATTEMPT_TICKS + 1), "a sure thing lights on the first attempt");
    }

    /// A bonfire two tiles east of the player, which stands at (5, 5).
    fn world_with_bonfire(ticks: i32) -> (World, Loc) {
        let mut world = world_with(1, &[]);
        let fire = Loc { id: BONFIRE_LOC, tile: TileCoord::new(7, 5), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, ticks);
        world.scenery_state.add_respawns(&fire, 2);
        (world, fire)
    }

    #[test]
    fn resting_walks_to_the_bonfire_and_redirects_the_respawn_there() {
        let (mut world, fire) = world_with_bonfire(1000);
        let home = world.respawn_point();
        assert_eq!(world.player_interact_object(BONFIRE_LOC, fire.tile, "Rest"), Ok(()));

        run_until(&mut world, 10, |w| w.respawn_point() != home);

        assert_eq!(world.player.tile(), TileCoord::new(6, 5), "stopped beside the fire");
        assert_eq!(world.respawn_point(), TileCoord::new(6, 5));
        assert!(world.take_messages().iter().any(|m| m.contains("You rest by the fire")));
    }

    #[test]
    fn the_redirect_ends_with_the_fire() {
        let (mut world, fire) = world_with_bonfire(30);
        let home = world.respawn_point();
        world.player_interact_object(BONFIRE_LOC, fire.tile, "Rest").unwrap();
        run_until(&mut world, 10, |w| w.respawn_point() != home);
        assert_ne!(world.respawn_point(), home);

        run_until(&mut world, 60, |w| fires(w) == 0);

        assert_eq!(world.respawn_point(), home, "back to the usual respawn point");
    }

    #[test]
    fn dying_beside_a_burning_bonfire_brings_the_player_back_at_it() {
        let (mut world, fire) = world_with_bonfire(1000);
        world.player_interact_object(BONFIRE_LOC, fire.tile, "Rest").unwrap();
        run_until(&mut world, 10, |w| w.player.action().is_none());
        world.player_request_walk_to(TileCoord::new(0, 0), false);
        run_until(&mut world, 10, |w| w.player.tile() == TileCoord::new(0, 0));

        let hp = world.player.mover.actor.health().current();
        world.player.mover.actor.health_mut().damage(hp);
        let mut back = false;
        for _ in 0..20 {
            world.run_tick();
            back |= world.take_events().contains(&WorldEvent::Respawned);
        }
        assert!(back);

        assert_eq!(world.player.tile(), TileCoord::new(6, 5));
    }

    #[test]
    fn each_log_grants_a_respawn_and_the_last_one_used_burns_the_bonfire_out() {
        let mut world = world_with(43, &["tinderbox", "logs", "logs"]);
        let fire = Loc { id: FIRE_LOC, tile: TileCoord::new(9, 5), kind: 10, orientation: 0, size: (1, 1) };
        world.scenery_state.spawn(fire, 1000);
        world.player_use_item_on_object(1, FIRE_LOC, fire.tile).unwrap();
        run_until(&mut world, 30, |w| w.player.inventory.count("logs") == 0);
        assert_eq!(bonfire(&world).map(|b| b.respawns), Some(2), "one per log");

        world.player_interact_object(BONFIRE_LOC, fire.tile, "Rest").unwrap();
        run_until(&mut world, 10, |w| w.player.action().is_none());
        let rest = world.player.tile();
        let die = |world: &mut World| {
            let hp = world.player.mover.actor.health().current();
            world.player.mover.actor.health_mut().damage(hp);
            for _ in 0..12 {
                world.run_tick();
            }
        };

        die(&mut world);
        assert_eq!(world.player.tile(), rest);
        assert_eq!(bonfire(&world).map(|b| b.respawns), Some(1));

        die(&mut world);
        assert_eq!(world.player.tile(), rest, "the last respawn still lands there");
        assert!(bonfire(&world).is_none(), "and uses the bonfire up");
        assert!(world.ground_items().any(|g| g.item == ASHES && g.tile == fire.tile));
        assert_ne!(world.respawn_point(), rest, "back to the usual respawn point");
    }
}
