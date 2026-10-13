//! The server-authoritative boundary. The server alone owns the `World` and the `TickDriver`: it
//! decides when a tick happens and what it does. A client only sends `ClientCommand`s (requests,
//! never state changes) and receives `Snapshot`s (read-only copies of the result), so it cannot
//! tick, skip, pause or rewrite the simulation on its own.
//!
//! `Server` is the pure, synchronous core (easy to test). `spawn_server` runs one on its own
//! thread behind channels, which is the seam a real network transport would later replace.

use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use super::entity::{Behavior, Entity};
use crate::data::CombatAnimations;
use super::animations::PlayerAnimations;
use super::events::{Combatant, WorldEvent};
use super::examine::ExamineTexts;
use super::tick_driver::TickDriver;
use super::world::World;
use crate::actors::EntityId;
use crate::collision::CollisionMap;
use crate::combat::ProtectionPrayer;
use crate::data::EquipSlot;
use crate::model::Skill;
use crate::tiles::TileCoord;

/// Identifies a logged-in player to the server and its clients. Assigned by the server when a
/// player enters the world, never chosen by the client. Not the same as the world's `EntityId`
/// for the player (always `PLAYER_ID`): that names an actor inside one `World`, this names the
/// account/session that controls it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub u32);

impl std::fmt::Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Everything a client may ask of the server. Gameplay commands only take effect on the next tick
/// the server chooses to run; the control commands drive the server's tick pacing (dev tooling
/// today - a real deployment would gate them behind an admin check).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCommand {
    /// Enter the world under `name`. The server assigns the `PlayerId` and reports it in the
    /// following snapshots. A connection that is already in the world is told so.
    Login { name: String },
    WalkTo { tile: TileCoord, force_inverse: bool },
    Attack { target: EntityId },
    /// Ask what an item is (by its id).
    ExamineItem { item: String },
    /// Walk up to an NPC and face it, for any of its options but Attack and Examine.
    InteractNpc { target: EntityId },
    /// Sets a protection prayer directly, with no prayer-book lookup and no drain.
    SetPrayer(ProtectionPrayer),
    SetRunning(bool),
    SetPaused(bool),
    RequestSingleStep,
    Equip { item: String },
    Unequip { slot: EquipSlot },
    /// Choose `option` ("Wield", "Eat", "Drop", ...) on the item in an inventory slot.
    InventoryOption { slot: usize, option: String },
    /// Drag an inventory item onto another slot (they swap).
    MoveInventoryItem { from: usize, to: usize },
    SetStance { index: usize },
    /// Switches a prayer on or off by id.
    TogglePrayer { prayer: String },
    SelectSpell { spell: Option<String> },
    SetSpecial { enabled: bool },
    UseConsumable { item: String },
    SetOnTask(bool),
    /// "Examine" on a scenery object: the server answers with a message. Only the id travels;
    /// what the object is and what it says are the server's to know.
    ExamineObject { id: u32 },
    /// "Examine" on an NPC.
    ExamineNpc { target: EntityId },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkillSnapshot {
    pub skill: Skill,
    pub base: i32,
    pub current: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerSnapshot {
    pub tile: TileCoord,
    /// The tiles the player stepped through during the last tick, in order (empty when it did
    /// not move; the last one is `tile`). A run covers two, and clients draw the movement through
    /// them so a turn is not cut across whatever lies between the start and end tile.
    pub step_tiles: Vec<TileCoord>,
    pub health: i32,
    pub max_health: i32,
    /// Its death has been confirmed by the server (a tick after health reached 0).
    pub is_dead: bool,
    pub prayer: i32,
    pub max_prayer: i32,
    pub attack_target: Option<EntityId>,
    /// What the player's body is turned toward: its attack target, else the NPC it is
    /// interacting with.
    pub face_target: Option<EntityId>,
    pub is_running: bool,
    /// Cache sequences the player's own attack and block play (by what is wielded and the stance).
    pub animations: PlayerAnimations,
    pub combat_level: i32,
    pub skills: Vec<SkillSnapshot>,
    pub equipment: Vec<(EquipSlot, String)>,
    /// The 28 inventory slots.
    pub inventory: Vec<Option<ItemView>>,
    /// What is worn, with names, by slot.
    pub equipped: Vec<(EquipSlot, ItemView)>,
    pub active_prayers: Vec<String>,
    pub stance: Option<String>,
    /// The wielded weapon's data category (`"unarmed"` without one).
    pub weapon_category: String,
    /// The names of the wielded weapon's combat styles, in the order `SetStance { index }` counts them.
    pub stances: Vec<String>,
    /// The selected style's index in `stances`.
    pub stance_index: Option<usize>,
    /// Percent of the special attack bar the wielded weapon's special uses; `None` without one.
    pub special_cost: Option<i32>,
    pub spell: Option<String>,
    pub special_energy: i32,
    pub special_enabled: bool,
    /// Percent of a full bar.
    pub run_energy: i32,
    pub poisoned: bool,
    pub venomed: bool,
    pub frozen: bool,
}

/// An item as a client draws it: what it is called and how many.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemView {
    pub id: String,
    pub name: String,
    pub quantity: u32,
    /// Whether the server would let the player wear it: a client offers "Wield"/"Wear" only
    /// then, whatever the item's cache definition lists.
    pub wearable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EntitySnapshot {
    pub id: EntityId,
    pub name: String,
    pub tile: TileCoord,
    pub health: i32,
    pub max_health: i32,
    /// Its death has been confirmed by the server (a tick after health reached 0).
    pub is_dead: bool,
    pub size: i32,
    /// The tiles it stepped through in the last tick, in order (empty when it stood still).
    pub step_tiles: Vec<TileCoord>,
    /// The cache NPC definition to draw it from, when it has one.
    pub appearance: Option<u32>,
    /// Cache sequences it plays when it attacks, is hit and dies, when it has them.
    pub combat_animations: Option<CombatAnimations>,
    /// Sequences it plays besides standing, walking and its `combat_animations`: those of attacks
    /// with an animation of their own (`WorldEvent::Attack::animation`) and its healing one
    /// (`WorldEvent::Heal::animation`), so a client can prepare them before the first is played.
    pub extra_animations: Vec<u32>,
    /// The monster definition this was built from, if it came from data.
    pub npc_id: Option<String>,
    /// Who it is turned toward: the one it is fighting, or the one it is healing. A client keeps
    /// it facing them all the time, walking or not (OSRS's "interacting" entity).
    pub facing: Option<Combatant>,
}

/// The observable state after a tick. Plain owned data (no `Rc`s), so it is `Send` and could be
/// serialized as-is.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub tick: i32,
    pub tick_seconds: f32,
    pub paused: bool,
    /// The id the server gave this client's player at login; `None` until it has logged in.
    pub player_id: Option<PlayerId>,
    pub player: PlayerSnapshot,
    pub entities: Vec<EntitySnapshot>,
    /// What happened since the last snapshot, in order: attacks thrown, hits landed, deaths.
    pub events: Vec<WorldEvent>,
    /// What the server wants the player to read since the last snapshot: rejected commands and the
    /// combat log lines of the ticks that ran.
    pub messages: Vec<String>,
}

/// A player that has entered the world.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    id: PlayerId,
    name: String,
}

pub struct Server {
    world: World,
    driver: TickDriver,
    outbox: Vec<String>,
    /// The next `PlayerId` to hand out. Ids start at 1 and are never reused.
    next_player_id: u32,
    /// The world has one player slot today, so at most one session.
    session: Option<Session>,
    /// Brings more of the map in around the player; see [`Server::set_map_streamer`].
    map_streamer: Option<MapStreamer>,
    examines: ExamineTexts,
    /// Names of scenery objects by id, for examine texts the data does not list.
    object_names: std::collections::HashMap<u32, String>,
}

/// Called before each tick with the world's map and the tile the player stands on.
type MapStreamer = Box<dyn FnMut(&mut CollisionMap, TileCoord)>;

impl Server {
    pub fn new(world: World, driver: TickDriver) -> Self {
        Self {
            world,
            driver,
            outbox: Vec::new(),
            next_player_id: 1,
            session: None,
            map_streamer: None,
            examines: ExamineTexts::builtin(),
            object_names: Default::default(),
        }
    }

    /// Tells the server what the scenery objects of its map are called (id -> name).
    pub fn set_object_names(&mut self, names: std::collections::HashMap<u32, String>) {
        self.object_names = names;
    }

    /// Lets the server load the map as the player moves through it: `streamer` runs before every
    /// tick with the collision map and the player's tile, and merges in whatever squares are
    /// needed around that tile (see `CollisionMap::merge`). A server that has the whole map loaded
    /// up front does not need one.
    pub fn set_map_streamer(&mut self, mut streamer: impl FnMut(&mut CollisionMap, TileCoord) + 'static) {
        streamer(&mut self.world.map, self.world.player.tile());
        self.map_streamer = Some(Box::new(streamer));
    }

    /// The id of the player currently in the world, if one has logged in.
    pub fn player_id(&self) -> Option<PlayerId> {
        self.session.as_ref().map(|session| session.id)
    }

    /// A new player enters the world: the server picks its id and announces it on the console.
    fn login(&mut self, name: String) -> Result<(), String> {
        if let Some(session) = &self.session {
            return Err(format!("already logged in as {} (player id {})", session.name, session.id));
        }
        let name = name.trim();
        let name = if name.is_empty() { "Player" } else { name }.to_owned();
        let id = PlayerId(self.next_player_id);
        self.next_player_id += 1;
        let tile = self.world.player.tile();
        println!("[server] player \"{name}\" logged in: assigned player id {id}, entering the world at ({}, {})", tile.x, tile.z);
        self.outbox.push(format!("Welcome, {name}."));
        self.session = Some(Session { id, name });
        Ok(())
    }

    pub fn world(&self) -> &World {
        &self.world
    }

    /// Validates and applies one command. Anything the server doesn't accept is silently dropped
    /// (a dead player can't act, an unknown or dead target can't be attacked).
    pub fn handle_command(&mut self, command: ClientCommand) {
        let result: Result<(), String> = match command {
            ClientCommand::Login { name } => self.login(name),
            ClientCommand::SetPaused(paused) => {
                self.driver.set_paused(paused);
                Ok(())
            }
            ClientCommand::RequestSingleStep => {
                self.driver.request_single_step();
                Ok(())
            }
            ClientCommand::ExamineNpc { target } => {
                let text = match self.world.entity(target) {
                    Some(entity) => entity
                        .def
                        .as_ref()
                        .and_then(|def| def.examine.clone())
                        .unwrap_or_else(|| ExamineTexts::plain(&entity.name)),
                    None => "You see nothing special.".to_owned(),
                };
                self.outbox.push(text);
                Ok(())
            }
            ClientCommand::ExamineItem { item } => {
                let name = self.world.item_name(&item);
                self.outbox.push(ExamineTexts::plain(&name));
                Ok(())
            }
            ClientCommand::ExamineObject { id } => {
                let text = self.examines.object(id, self.object_names.get(&id).map(String::as_str));
                self.outbox.push(text);
                Ok(())
            }
            _ if self.world.player.is_dead() => Ok(()),
            ClientCommand::WalkTo { tile, force_inverse } => {
                self.world.player_request_walk_to(tile, force_inverse);
                Ok(())
            }
            ClientCommand::Attack { target } => {
                if self.world.entity(target).is_some_and(|entity| !entity.is_dead()) {
                    self.world.player_set_attack_target(target);
                }
                Ok(())
            }
            ClientCommand::InteractNpc { target } => {
                if self.world.entity(target).is_some_and(|entity| !entity.is_dead()) {
                    self.world.player_set_interact_target(target);
                }
                Ok(())
            }
            ClientCommand::SetPrayer(prayer) => {
                self.world.player.set_active_prayer(prayer);
                Ok(())
            }
            ClientCommand::SetRunning(running) => {
                self.world.player.is_running = running;
                Ok(())
            }
            ClientCommand::Equip { item } => self.world.player_equip(&item),
            ClientCommand::Unequip { slot } => {
                self.world.player_unequip_to_inventory(slot);
                Ok(())
            }
            ClientCommand::InventoryOption { slot, option } => {
                self.world.player_inventory_option(slot, &option);
                Ok(())
            }
            ClientCommand::MoveInventoryItem { from, to } => {
                self.world.player.inventory.swap(from, to);
                Ok(())
            }
            ClientCommand::SetStance { index } => self.world.player_set_stance(index),
            ClientCommand::TogglePrayer { prayer } => self.world.player_toggle_prayer(&prayer).map(|_| ()),
            ClientCommand::SelectSpell { spell } => self.world.player_select_spell(spell.as_deref()),
            ClientCommand::SetSpecial { enabled } => self.world.player_set_special(enabled),
            ClientCommand::UseConsumable { item } => self.world.player_use_consumable(&item),
            ClientCommand::SetOnTask(on_task) => {
                self.world.player_set_on_task(on_task);
                Ok(())
            }
        };
        // A rejected command is the player's to read, not something to drop.
        if let Err(message) = result {
            self.outbox.push(message);
        }
    }

    /// Feeds `dt` real seconds to the driver, running however many whole ticks that adds up to.
    /// Returns a snapshot if any ran.
    pub fn advance(&mut self, dt: f32) -> Option<Snapshot> {
        let Self { world, driver, map_streamer, .. } = self;
        let ran = driver.update(dt, || {
            if let Some(streamer) = map_streamer {
                streamer(&mut world.map, world.player.tile());
            }
            world.run_tick()
        });
        (ran > 0).then(|| self.snapshot())
    }

    /// The current state, plus (and consuming) the messages that have built up since the last one.
    pub fn snapshot(&mut self) -> Snapshot {
        let mut messages = std::mem::take(&mut self.outbox);
        messages.extend(self.world.take_messages());

        let player = &self.world.player;
        let health = player.mover.actor.health();
        let registry = self.world.registry();
        let combat_options = player.combat_options(registry);
        Snapshot {
            tick: self.world.current_tick(),
            tick_seconds: self.driver.effective_tick_seconds(),
            paused: self.driver.is_paused(),
            player_id: self.player_id(),
            player: PlayerSnapshot {
                tile: player.tile(),
                step_tiles: player.mover.last_step_tiles().to_vec(),
                health: health.current(),
                max_health: health.max(),
                is_dead: self.world.death_confirmed(Combatant::Player),
                prayer: player.prayer.current(),
                max_prayer: player.prayer.max(),
                attack_target: player.attack_target(),
                face_target: player.face_target(),
                is_running: player.is_running,
                animations: {
                    let weapon = player.loadout.item_id_in(EquipSlot::Weapon).and_then(|id| registry.item(id));
                    // the weapon's category, or its name when the data has no category for it
                    let kind = weapon.map(|item| match &item.weapon {
                        Some(w) if !w.category.is_empty() => w.category.to_lowercase(),
                        _ => item.name.to_lowercase(),
                    });
                    let damage_type = player.current_stance(registry).and_then(|s| s.damage_type);
                    let shield = player.loadout.item_id_in(EquipSlot::Shield).is_some();
                    super::animations::player_animations(kind.as_deref(), damage_type, shield)
                },
                combat_level: player.combat_level(),
                skills: Skill::ALL
                    .into_iter()
                    .map(|skill| SkillSnapshot { skill, base: player.skills.base(skill), current: player.skills.current(skill) })
                    .collect(),
                equipment: player.loadout.slots().map(|(slot, id)| (slot, id.to_owned())).collect(),
                inventory: player
                    .inventory
                    .slots()
                    .iter()
                    .map(|slot| {
                        slot.as_ref().map(|stack| ItemView {
                            id: stack.item.clone(),
                            name: self.world.item_name(&stack.item),
                            quantity: stack.quantity,
                            wearable: registry.item(&stack.item).is_some_and(|item| item.slot.is_some()),
                        })
                    })
                    .collect(),
                equipped: player
                    .loadout
                    .slots()
                    .map(|(slot, id)| (slot, ItemView { id: id.to_owned(), name: self.world.item_name(id), quantity: 1, wearable: true }))
                    .collect(),
                active_prayers: player.active_prayers.clone(),
                stance: player.current_stance(registry).map(|stance| stance.name),
                weapon_category: combat_options.weapon_category,
                stances: combat_options.stances,
                stance_index: combat_options.stance_index,
                special_cost: combat_options.special_cost,
                spell: player.selected_spell.clone(),
                special_energy: player.special_energy,
                special_enabled: player.special_enabled,
                run_energy: player.run_energy / 100,
                poisoned: player.status_effects.is_poisoned(),
                venomed: player.status_effects.is_venomed(),
                frozen: player.status_effects.is_frozen(),
            },
            entities: self
                .world
                .entities()
                .map(|entity| EntitySnapshot {
                    is_dead: self.world.death_confirmed(Combatant::Npc(entity.id)),
                    ..entity_snapshot(entity)
                })
                .collect(),
            events: self.world.take_events(),
            messages,
        }
    }
}

fn entity_snapshot(entity: &Entity) -> EntitySnapshot {
    EntitySnapshot {
        id: entity.id,
        name: entity.name.clone(),
        tile: entity.tile(),
        health: entity.health().current(),
        max_health: entity.health().max(),
        is_dead: entity.is_dead(),
        size: entity.mover.actor.footprint_size(),
        step_tiles: entity.mover.last_step_tiles().to_vec(),
        appearance: entity.def.as_ref().and_then(|def| def.appearance),
        combat_animations: entity.def.as_ref().and_then(|def| def.animations),
        extra_animations: entity.def.as_ref().map_or_else(Vec::new, |def| {
            let mut sequences: Vec<u32> = def.attacks.iter().filter_map(|attack| attack.animation).collect();
            sequences.extend(def.heals_summoner.and_then(|heal| heal.animation));
            sequences.sort_unstable();
            sequences.dedup();
            sequences
        }),
        npc_id: entity.def.as_ref().map(|def| def.id.clone()),
        facing: facing_of(entity),
    }
}

/// Who an NPC faces: the player while it fights them, else the monster it heals; nobody once it
/// is dead.
fn facing_of(entity: &Entity) -> Option<Combatant> {
    if entity.is_dead() {
        return None;
    }
    match &entity.behavior {
        Behavior::Boss(state) if state.targets_player => Some(Combatant::Player),
        Behavior::Npc(state) if state.targets_player => Some(Combatant::Player),
        Behavior::Healer(state) => state.heal_target.map(Combatant::Npc),
        _ => {
            let heals = entity.def.as_ref().is_some_and(|def| def.heals_summoner.is_some());
            entity.leader.filter(|_| heals).map(Combatant::Npc)
        }
    }
}

/// The client's end of the link to a running server thread. Dropping it stops the server.
pub struct ServerHandle {
    commands: Sender<ClientCommand>,
    snapshots: Receiver<Snapshot>,
}

impl ServerHandle {
    pub fn send(&self, command: ClientCommand) {
        // A send error means the server thread has exited; there is nothing left to command.
        let _ = self.commands.send(command);
    }

    /// Every snapshot that arrived since the last call, oldest first.
    pub fn drain_snapshots(&self) -> Vec<Snapshot> {
        self.snapshots.try_iter().collect()
    }
}

/// Runs a `Server` on its own thread. `build` runs on that thread because `World` holds `Rc`s and
/// can't be sent across threads. The server paces itself on its own clock and publishes a
/// snapshot after every tick (and an initial one before the first).
pub fn spawn_server(build: impl FnOnce() -> Server + Send + 'static) -> ServerHandle {
    const POLL_INTERVAL: Duration = Duration::from_millis(5);

    let (command_tx, command_rx) = mpsc::channel();
    let (snapshot_tx, snapshot_rx) = mpsc::channel();

    thread::spawn(move || {
        let mut server = build();
        if snapshot_tx.send(server.snapshot()).is_err() {
            return;
        }

        let mut last = Instant::now();
        loop {
            loop {
                match command_rx.try_recv() {
                    Ok(command) => server.handle_command(command),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return,
                }
            }

            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32();
            last = now;

            if let Some(snapshot) = server.advance(dt) {
                if snapshot_tx.send(snapshot).is_err() {
                    return;
                }
            }
            thread::sleep(POLL_INTERVAL);
        }
    });

    ServerHandle { commands: command_tx, snapshots: snapshot_rx }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::data::Registry;
    use crate::sim::PlayerConfig;
    use std::rc::Rc;

    fn server() -> Server {
        let world = dev_open_floor_world(15, 1, TileCoord::new(0, 0), PlayerConfig::default());
        Server::new(world, TickDriver::default())
    }

    #[test]
    fn a_monster_faces_whom_it_fights_and_a_healer_whom_it_heals() {
        let mut server = data_server();
        let world = &mut server.world;
        let jad = world.spawn_npc_def("tztok_jad", TileCoord::new(-2, 8)).unwrap();
        let man = world.spawn_npc_def("man", TileCoord::new(5, 0)).unwrap();
        world.set_hostile(jad, true);
        world.entity_mut(jad).unwrap().mover.actor.health_mut().damage(110);
        server.advance(0.0);
        server.advance(0.6);

        let snapshot = server.snapshot();
        let facing = |id| snapshot.entities.iter().find(|e| e.id == id).unwrap().facing;
        assert_eq!(facing(jad), Some(Combatant::Player));
        assert_eq!(facing(man), None, "a passive man faces nobody in particular");
        let healer = snapshot.entities.iter().find(|e| e.name == "Yt-HurKot").expect("Jad called his healers");
        assert_eq!(healer.facing, Some(Combatant::Npc(jad)));
    }

    #[test]
    fn the_inventory_says_which_items_can_be_worn() {
        let mut server = data_server();
        server.world.give_item("abyssal_whip", 1).unwrap();
        server.world.give_item("prayer_potion_4", 1).unwrap();
        server.world.give_item("vial", 1).unwrap();

        let snapshot = server.snapshot();
        let wearable: Vec<(String, bool)> =
            snapshot.player.inventory.iter().flatten().map(|item| (item.id.clone(), item.wearable)).collect();

        assert_eq!(
            wearable,
            [("abyssal_whip".to_owned(), true), ("prayer_potion_4".to_owned(), false), ("vial".to_owned(), false)]
        );
    }

    fn data_server() -> Server {
        let mut world = dev_open_floor_world(15, 1, TileCoord::new(0, 0), PlayerConfig { max_prayer_points: 70, ..PlayerConfig::default() });
        world.set_registry(Rc::new(Registry::builtin()));
        Server::new(world, TickDriver::default())
    }

    #[test]
    fn snapshots_list_the_tiles_stepped_through_each_tick() {
        let mut server = server();
        server.advance(0.0);
        server.handle_command(ClientCommand::SetRunning(true));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(3, 2), force_inverse: false });

        let mut moves = Vec::new();
        for _ in 0..4 {
            let snapshot = server.advance(0.6).expect("a tick");
            if !snapshot.player.step_tiles.is_empty() {
                assert_eq!(snapshot.player.step_tiles.last(), Some(&snapshot.player.tile));
                moves.push(snapshot.player.step_tiles);
            }
        }
        // a run takes two tiles a tick, in path order, ending at the destination
        assert!(moves.iter().all(|m| m.len() <= 2));
        assert!(moves.iter().any(|m| m.len() == 2));
        let all: Vec<TileCoord> = moves.into_iter().flatten().collect();
        assert_eq!(all.last(), Some(&TileCoord::new(3, 2)));
        for pair in all.windows(2) {
            assert!((pair[1].x - pair[0].x).abs() <= 1 && (pair[1].z - pair[0].z).abs() <= 1, "{pair:?}");
        }

        // standing still: no steps
        let idle = server.advance(0.6).unwrap();
        assert!(idle.player.step_tiles.is_empty());
    }

    #[test]
    fn logging_in_assigns_a_server_side_player_id_that_snapshots_report() {
        let mut server = server();
        assert_eq!(server.snapshot().player_id, None, "nobody has entered the world yet");

        server.handle_command(ClientCommand::Login { name: "Zezima".into() });
        let snapshot = server.snapshot();

        assert_eq!(snapshot.player_id, Some(PlayerId(1)));
        assert_eq!(server.player_id(), Some(PlayerId(1)));
        assert!(snapshot.messages.iter().any(|m| m.contains("Zezima")), "{:?}", snapshot.messages);
    }

    #[test]
    fn a_second_login_on_the_same_connection_is_rejected_and_keeps_the_first_id() {
        let mut server = server();
        server.handle_command(ClientCommand::Login { name: "First".into() });
        server.handle_command(ClientCommand::Login { name: "Second".into() });
        let snapshot = server.snapshot();

        assert_eq!(snapshot.player_id, Some(PlayerId(1)));
        assert!(snapshot.messages.iter().any(|m| m.contains("already logged in as First")), "{:?}", snapshot.messages);
    }

    #[test]
    fn data_commands_change_the_servers_player_and_show_up_in_the_snapshot() {
        let mut server = data_server();

        server.handle_command(ClientCommand::Equip { item: "abyssal_whip".into() });
        server.handle_command(ClientCommand::SetStance { index: 1 });
        server.handle_command(ClientCommand::TogglePrayer { prayer: "protect_from_melee".into() });
        let snapshot = server.snapshot();

        assert_eq!(snapshot.player.equipment, vec![(EquipSlot::Weapon, "abyssal_whip".to_owned())]);
        assert_eq!(snapshot.player.stance.as_deref(), Some("Lash"));
        assert_eq!(snapshot.player.active_prayers, vec!["protect_from_melee".to_owned()]);
        assert_eq!(snapshot.player.max_prayer, 70);
        assert!(snapshot.messages.is_empty());
    }

    #[test]
    fn the_snapshot_lists_the_wielded_weapons_styles_and_special() {
        let mut server = data_server();

        // bare fists: three styles, no special
        let fists = server.snapshot().player;
        assert_eq!(fists.weapon_category, "unarmed");
        assert_eq!(fists.stances, vec!["Punch", "Kick", "Block"]);
        assert_eq!(fists.special_cost, None);

        server.handle_command(ClientCommand::Equip { item: "abyssal_whip".into() });
        server.handle_command(ClientCommand::SetStance { index: 1 });
        let whip = server.snapshot().player;
        assert_eq!(whip.weapon_category, "whip");
        assert_eq!(whip.stances, vec!["Flick", "Lash", "Deflect"]);
        assert_eq!(whip.stance_index, Some(1));
        assert_eq!(whip.stance.as_deref(), Some("Lash"));
        assert_eq!(whip.special_cost, Some(50));

        server.handle_command(ClientCommand::Equip { item: "dragon_dagger".into() });
        assert_eq!(server.snapshot().player.special_cost, Some(25));
    }

    #[test]
    fn a_rejected_command_comes_back_as_a_message_not_a_state_change() {
        let mut server = data_server();

        server.handle_command(ClientCommand::TogglePrayer { prayer: "piety".into() }); // level 70: allowed
        server.handle_command(ClientCommand::Equip { item: "no_such_item".into() });
        server.handle_command(ClientCommand::SetSpecial { enabled: true });
        let snapshot = server.snapshot();

        assert!(snapshot.messages.iter().any(|m| m.contains("no_such_item")), "{:?}", snapshot.messages);
        assert!(snapshot.messages.iter().any(|m| m.contains("special")), "{:?}", snapshot.messages);
        assert!(snapshot.player.equipment.is_empty());
        assert!(!snapshot.player.special_enabled);
        assert!(server.snapshot().messages.is_empty(), "messages are delivered once");
    }

    #[test]
    fn combat_reaches_the_client_as_events_not_as_chat_lines() {
        let mut server = data_server();
        server.handle_command(ClientCommand::Equip { item: "abyssal_whip".into() });
        server.world.spawn_npc_def("combat_dummy", TileCoord::new(1, 0)).unwrap();
        server.handle_command(ClientCommand::Attack { target: EntityId(1) });

        let snapshot = server.advance(0.0).expect("the first tick runs immediately");

        assert!(snapshot.messages.iter().all(|m| !m.contains("hits for") && !m.contains("uses ")), "{:?}", snapshot.messages);
        assert!(snapshot.events.iter().any(|e| matches!(e, WorldEvent::Attack { .. })), "{:?}", snapshot.events);
        assert_eq!(snapshot.entities[0].npc_id.as_deref(), Some("combat_dummy"));
    }

    #[test]
    fn only_the_servers_own_clock_advances_ticks() {
        let mut server = server();
        server.advance(0.0); // the primed first tick

        assert!(server.advance(0.3).is_none());
        let snapshot = server.advance(0.3).expect("0.6s of server time is one tick");
        assert_eq!(snapshot.tick, 2);
    }

    #[test]
    fn a_walk_command_moves_the_player_only_when_the_server_ticks() {
        let mut server = server();
        server.advance(0.0);
        server.handle_command(ClientCommand::SetRunning(false));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(3, 0), force_inverse: false });

        assert_eq!(server.snapshot().player.tile, TileCoord::new(0, 0));

        server.advance(0.6); // queues the click and plans the path
        server.advance(0.6);
        assert_ne!(server.snapshot().player.tile, TileCoord::new(0, 0));
    }

    #[test]
    fn a_map_streamer_runs_each_tick_with_the_players_tile_and_can_grow_the_map() {
        use crate::collision::CollisionMapData;
        use std::cell::RefCell;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut server = server();
        let floor_width = server.world().map.width();
        let log = Rc::clone(&seen);
        server.set_map_streamer(move |map, tile| {
            log.borrow_mut().push(tile);
            // the first time it is asked, open a block just east of the floor
            if map.width() == floor_width {
                let cells = 16;
                map.merge(CollisionMapData {
                    origin_x: map.origin_x() + map.width(),
                    origin_z: 0,
                    width: 4,
                    height: 4,
                    flags: vec![0; cells],
                    heights: vec![0; cells],
                    max_step_height: 100,
                })
                .unwrap();
            }
        });
        assert_eq!(seen.borrow().len(), 1, "it runs once on being installed");
        assert_eq!(server.world().map.width(), floor_width + 4);

        server.advance(0.0);
        assert!(seen.borrow().len() >= 2, "and before each tick");
        assert!(seen.borrow().iter().all(|t| *t == TileCoord::new(0, 0)));
    }

    #[test]
    fn examining_an_object_comes_back_as_a_message_from_the_server() {
        let mut server = server();
        server.set_object_names([(9999, "Wobbly stool".to_owned())].into());

        server.handle_command(ClientCommand::ExamineObject { id: 9999 });
        server.handle_command(ClientCommand::ExamineObject { id: 1 });
        let snapshot = server.snapshot();

        assert_eq!(snapshot.messages, vec!["It's a wobbly stool.".to_owned(), "You see nothing special.".to_owned()]);
    }

    #[test]
    fn a_man_strolls_about_his_spawn_tile_and_reports_the_tiles_he_stepped_through() {
        let mut server = data_server();
        let home = TileCoord::new(3, 3);
        server.world.spawn_npc_def("man", home).unwrap();
        server.advance(0.0);

        let mut visited = std::collections::HashSet::new();
        let mut last = home;
        let mut moves = 0;
        for _ in 0..600 {
            let Some(snapshot) = server.advance(0.6) else { continue };
            let man = &snapshot.entities[0];
            assert!(man.step_tiles.len() <= 1, "walks at one tile a tick: {:?}", man.step_tiles);
            if let Some(step) = man.step_tiles.last() {
                assert_eq!(*step, man.tile);
                assert!((step.x - last.x).abs() <= 1 && (step.z - last.z).abs() <= 1);
                moves += 1;
            }
            last = man.tile;
            visited.insert(man.tile);
            assert!((man.tile.x - home.x).abs() <= 5 && (man.tile.z - home.z).abs() <= 5, "left his patch: {:?}", man.tile);
        }
        assert!(moves > 20, "he should have strolled, took {moves} steps");
        assert!(visited.len() > 3, "and gone to more than a few places: {}", visited.len());
    }

    #[test]
    fn examining_an_npc_says_what_its_definition_says_or_describes_it_by_name() {
        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(2, 2)).unwrap();
        let dummy = server.world.spawn_npc_def("combat_dummy", TileCoord::new(3, 3)).unwrap();

        server.handle_command(ClientCommand::ExamineNpc { target: man });
        server.handle_command(ClientCommand::ExamineNpc { target: dummy });
        server.handle_command(ClientCommand::ExamineNpc { target: EntityId(99) });
        let messages = server.snapshot().messages;

        assert_eq!(messages[0], "One of Gielinor's many citizens.");
        assert_eq!(messages[1], "It's a combat dummy.");
        assert_eq!(messages[2], "You see nothing special.");
    }

    #[test]
    fn a_fight_is_reported_as_attacks_hits_and_a_death_in_order() {
        use super::super::events::{Combatant, WorldEvent};

        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(1, 0)).unwrap();
        server.advance(0.0);
        server.handle_command(ClientCommand::Attack { target: man });

        let mut events = Vec::new();
        for _ in 0..60 {
            if let Some(snapshot) = server.advance(0.6) {
                events.extend(snapshot.events);
            }
        }

        let npc = Combatant::Npc(man);
        assert!(events.contains(&WorldEvent::Attack { attacker: Combatant::Player, target: npc, animation: None }));
        assert!(events.iter().any(|e| matches!(e, WorldEvent::Attack { attacker, target: Combatant::Player, .. } if *attacker == npc)));
        let hits_on_man: Vec<i32> = events
            .iter()
            .filter_map(|e| match e {
                WorldEvent::Hit { target, damage } if *target == npc => Some(*damage),
                _ => None,
            })
            .collect();
        assert!(hits_on_man.iter().any(|d| *d > 0), "{hits_on_man:?}");
        assert!(hits_on_man.iter().sum::<i32>() >= 7, "the man has 7 hitpoints: {hits_on_man:?}");
        assert_eq!(events.iter().filter(|e| **e == WorldEvent::Died { who: npc }).count(), 1);
        // the death comes after the hit that did it
        let died = events.iter().position(|e| *e == WorldEvent::Died { who: npc }).unwrap();
        let last_hit = events.iter().rposition(|e| matches!(e, WorldEvent::Hit { target, .. } if *target == npc)).unwrap();
        assert!(last_hit < died);
    }

    /// A fresh account against a man, many fights: both have a max hit of 1, and a hit sometimes misses.
    #[test]
    fn a_new_account_fights_a_man_with_level_one_stats() {
        use super::super::events::{Combatant, WorldEvent};

        let (mut player_hits, mut man_hits) = (Vec::new(), Vec::new());
        let mut man_deaths = 0;
        for seed in 1..=12 {
            let mut world = dev_open_floor_world(15, seed, TileCoord::new(0, 0), PlayerConfig::new_account());
            world.set_registry(Rc::new(Registry::builtin()));
            let mut server = Server::new(world, TickDriver::default());
            let man = server.world.spawn_npc_def("man", TileCoord::new(1, 0)).unwrap();
            server.advance(0.0);
            let start = server.snapshot();
            assert_eq!((start.player.health, start.player.max_health), (10, 10));
            assert!(start.player.skills.iter().all(|s| s.base == 1 || s.base == 10), "{:?}", start.player.skills);
            server.handle_command(ClientCommand::Attack { target: man });
            for _ in 0..200 {
                let Some(snapshot) = server.advance(0.6) else { continue };
                for event in snapshot.events {
                    match event {
                        WorldEvent::Hit { target: Combatant::Npc(_), damage } => man_hits.push(damage),
                        WorldEvent::Hit { target: Combatant::Player, damage } => player_hits.push(damage),
                        WorldEvent::Died { who: Combatant::Npc(_) } => man_deaths += 1,
                        _ => {}
                    }
                }
            }
        }
        assert!(man_hits.iter().all(|d| (0..=1).contains(d)), "a level 1 hits for at most 1: {man_hits:?}");
        assert!(player_hits.iter().all(|d| (0..=1).contains(d)), "so does a man: {player_hits:?}");
        assert!(man_hits.contains(&0) && man_hits.contains(&1), "some swings miss and some land");
        assert!(man_deaths > 0, "a man with 7 hitpoints does die to a level 1");
    }

    #[test]
    fn a_death_is_confirmed_the_tick_after_the_hit_that_did_it() {
        use super::super::events::{Combatant, WorldEvent};

        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(1, 0)).unwrap();
        server.advance(0.0);
        server.handle_command(ClientCommand::Attack { target: man });

        let npc = Combatant::Npc(man);
        let (mut killing_hit_tick, mut died_tick) = (None, None);
        for _ in 0..80 {
            let Some(snapshot) = server.advance(0.6) else { continue };
            let entity = snapshot.entities.iter().find(|e| e.id == man).unwrap();
            if entity.health <= 0 && killing_hit_tick.is_none() {
                killing_hit_tick = Some(snapshot.tick);
                assert!(!entity.is_dead, "health is 0 but the death is not confirmed yet");
                assert!(!snapshot.events.contains(&WorldEvent::Died { who: npc }));
            }
            if snapshot.events.contains(&WorldEvent::Died { who: npc }) {
                died_tick = Some(snapshot.tick);
                assert!(entity.is_dead, "the snapshot that reports the death shows it");
                break;
            }
        }
        assert_eq!(died_tick, killing_hit_tick.map(|t| t + 1));
    }

    #[test]
    fn interacting_with_an_npc_walks_up_to_it_and_keeps_facing_it() {
        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(6, 0)).unwrap();
        server.advance(0.0);

        server.handle_command(ClientCommand::InteractNpc { target: man });
        assert_eq!(server.snapshot().player.face_target, Some(man));
        for _ in 0..12 {
            server.advance(0.6);
        }
        let snapshot = server.snapshot();
        let man_tile = snapshot.entities.iter().find(|e| e.id == man).unwrap().tile;
        assert!(snapshot.player.tile.chebyshev_distance_to(man_tile) <= 2, "walked up to the man");
        assert_eq!(snapshot.player.face_target, Some(man), "and still faces him");
        assert_eq!(snapshot.player.attack_target, None, "without attacking");

        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(0, 3), force_inverse: false });
        assert_eq!(server.snapshot().player.face_target, None, "walking away drops it");
    }

    /// A chaser after the player (at the origin, room to spare) and a man to click on.
    fn red_x_server() -> (Server, EntityId, EntityId) {
        use crate::sim::NpcConfig;

        let mut server = data_server();
        let chaser = server.world.spawn_npc("Chaser", TileCoord::new(0, 9), NpcConfig::default());
        let man = server.world.spawn_npc_def("man", TileCoord::new(8, 0)).unwrap();
        server.advance(0.0);
        (server, chaser, man)
    }

    fn tile_of(server: &mut Server, id: EntityId) -> TileCoord {
        server.snapshot().entities.iter().find(|e| e.id == id).unwrap().tile
    }

    #[test]
    fn an_npc_after_the_player_holds_still_while_the_player_walks_up_to_something_it_clicked() {
        let (mut server, chaser, man) = red_x_server();
        let start = tile_of(&mut server, chaser);

        server.handle_command(ClientCommand::InteractNpc { target: man });
        for _ in 0..3 {
            server.advance(0.6);
        }
        assert_eq!(tile_of(&mut server, chaser), start, "frozen while the player is on its way");

        // clicking something else to interact with keeps it frozen
        server.handle_command(ClientCommand::Attack { target: man });
        server.advance(0.6);
        assert_eq!(tile_of(&mut server, chaser), start);

        // once the player has got there the NPC is free to come
        for _ in 0..12 {
            server.advance(0.6);
        }
        assert_ne!(tile_of(&mut server, chaser), start, "the freeze ends on arrival");
    }

    #[test]
    fn the_player_attacks_on_the_tick_its_step_brings_it_into_range() {
        use super::super::events::{Combatant, WorldEvent};
        use crate::sim::NpcConfig;

        let mut server = data_server();
        let dummy = server.world.spawn_npc("Dummy", TileCoord::new(4, 0), NpcConfig { targets_player: false, ..Default::default() });
        server.advance(0.0);
        server.handle_command(ClientCommand::Attack { target: dummy });

        let mut previous = server.snapshot().player.tile;
        for _ in 0..6 {
            let Some(snapshot) = server.advance(0.6) else { continue };
            let swung = snapshot.events.contains(&WorldEvent::Attack { attacker: Combatant::Player, target: Combatant::Npc(dummy), animation: None });
            if swung {
                assert_eq!(previous, TileCoord::new(2, 0), "it was still a step short at the start of the tick");
                assert_eq!(snapshot.player.tile, TileCoord::new(3, 0), "and the step took it into range");
                return;
            }
            previous = snapshot.player.tile;
        }
        panic!("never attacked");
    }

    #[test]
    fn a_walk_click_lifts_the_freeze() {
        let (mut server, chaser, man) = red_x_server();
        let start = tile_of(&mut server, chaser);

        server.handle_command(ClientCommand::InteractNpc { target: man });
        server.advance(0.6);
        server.advance(0.6);
        assert_eq!(tile_of(&mut server, chaser), start);

        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(0, 2), force_inverse: false });
        server.advance(0.6);
        assert_ne!(tile_of(&mut server, chaser), start, "a yellow click lets it come");
    }

    fn inventory_server() -> Server {
        let mut server = data_server();
        server.world.give_item("bronze_sword", 1).unwrap();
        server.world.give_item("abyssal_whip", 1).unwrap();
        server.world.give_item("shark", 2).unwrap();
        server.world.give_item("bronze_arrow", 30).unwrap();
        server
    }

    #[test]
    fn the_inventory_holds_items_and_stacks_ammunition() {
        let mut server = inventory_server();
        let inventory = server.snapshot().player.inventory;
        assert_eq!(inventory.len(), 28);
        let names: Vec<&str> = inventory.iter().flatten().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["Bronze sword", "Abyssal whip", "Shark", "Shark", "Bronze arrow"]);
        let arrows = inventory.iter().flatten().find(|i| i.id == "bronze_arrow").unwrap();
        assert_eq!(arrows.quantity, 30, "ammunition stacks");
    }

    #[test]
    fn wielding_swaps_the_worn_item_back_into_the_slot() {
        let mut server = inventory_server();
        server.handle_command(ClientCommand::InventoryOption { slot: 0, option: "Wield".into() });
        let snapshot = server.snapshot();
        assert_eq!(snapshot.player.equipped.len(), 1);
        assert_eq!(snapshot.player.equipped[0].1.name, "Bronze sword");
        assert!(snapshot.player.inventory[0].is_none(), "it left the inventory");

        // wielding the whip puts the sword back where the whip was
        server.handle_command(ClientCommand::InventoryOption { slot: 1, option: "Wield".into() });
        let snapshot = server.snapshot();
        assert_eq!(snapshot.player.equipped[0].1.name, "Abyssal whip");
        assert_eq!(snapshot.player.inventory[1].as_ref().map(|i| i.name.as_str()), Some("Bronze sword"));
    }

    #[test]
    fn eating_heals_and_uses_up_one() {
        let mut server = inventory_server();
        server.world.player.mover.actor.health_mut().damage(8);
        let before = server.snapshot().player.health;
        server.handle_command(ClientCommand::InventoryOption { slot: 2, option: "Eat".into() });
        let snapshot = server.snapshot();
        assert!(snapshot.player.health > before);
        assert_eq!(snapshot.player.inventory.iter().flatten().filter(|i| i.id == "shark").count(), 1);
    }

    #[test]
    fn a_bow_fires_the_wiki_arrows_once_they_are_wielded() {
        use super::super::events::{Combatant, WorldEvent};

        let mut server = data_server();
        server.world.give_item("magic_shortbow", 1).unwrap();
        server.world.give_item("bronze_arrow", 50).unwrap();
        server.handle_command(ClientCommand::InventoryOption { slot: 0, option: "Wield".into() });
        server.handle_command(ClientCommand::InventoryOption { slot: 1, option: "Wield".into() });
        let man = server.world.spawn_npc_def("man", TileCoord::new(4, 0)).unwrap();
        server.advance(0.0);
        server.handle_command(ClientCommand::Attack { target: man });

        let mut attacked = false;
        let mut messages = Vec::new();
        for _ in 0..10 {
            if let Some(snapshot) = server.advance(0.6) {
                messages.extend(snapshot.messages);
                attacked |= snapshot
                    .events
                    .iter()
                    .any(|e| matches!(e, WorldEvent::Attack { attacker: Combatant::Player, .. }));
            }
        }
        assert!(attacked, "the player shot: {messages:?}");
        assert!(messages.iter().all(|m| !m.contains("needs arrow ammunition")), "{messages:?}");
    }

    #[test]
    fn an_npc_that_chases_reports_the_tiles_it_stepped_through() {
        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(9, 0)).unwrap();
        server.world.set_hostile(man, true);
        server.advance(0.0);

        let mut last = TileCoord::new(9, 0);
        let mut moves = 0;
        for _ in 0..6 {
            let Some(snapshot) = server.advance(0.6) else { continue };
            let entity = snapshot.entities.iter().find(|e| e.id == man).unwrap();
            if entity.tile != last {
                moves += 1;
                assert_eq!(entity.step_tiles.last(), Some(&entity.tile), "a move comes with its step tiles");
                last = entity.tile;
            }
        }
        assert!(moves >= 3, "the man walked toward the player: {moves}");
    }

    #[test]
    fn npcs_do_not_block_the_players_path_unless_they_are_designated_to() {
        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(3, 0)).unwrap();
        server.world.entity_mut(man).unwrap().wander = None;
        server.advance(0.0);
        server.handle_command(ClientCommand::SetRunning(false));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(6, 0), force_inverse: false });
        let mut stepped = Vec::new();
        for _ in 0..8 {
            if let Some(snapshot) = server.advance(0.6) {
                stepped.extend(snapshot.player.step_tiles);
            }
        }
        assert!(stepped.contains(&TileCoord::new(3, 0)), "the player walked straight through the man: {stepped:?}");
        assert_eq!(stepped.last(), Some(&TileCoord::new(6, 0)));
    }

    #[test]
    fn the_attack_timer_runs_down_while_the_player_walks() {
        let mut server = data_server();
        server.world.give_item("abyssal_whip", 1).unwrap();
        server.handle_command(ClientCommand::InventoryOption { slot: 0, option: "Wield".into() });
        let man = server.world.spawn_npc_def("man", TileCoord::new(1, 0)).unwrap();
        server.world.entity_mut(man).unwrap().wander = None;
        server.advance(0.0);
        server.handle_command(ClientCommand::Attack { target: man });
        server.advance(0.6); // swings (whip: 4 ticks), timer starts
        let after_swing = server.world.player.attack_cooldown;
        assert!(after_swing > 0);

        // walk away: the timer keeps counting
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(0, 6), force_inverse: false });
        server.advance(0.6);
        server.advance(0.6);
        assert!(server.world.player.attack_cooldown < after_swing, "ticked down while walking");
        for _ in 0..3 {
            server.advance(0.6);
        }
        assert_eq!(server.world.player.attack_cooldown, 0);
    }

    #[test]
    fn a_chasing_npc_follows_a_move_on_the_tick_after_it_not_during_it() {
        let mut server = data_server();
        let man = server.world.spawn_npc_def("man", TileCoord::new(1, 0)).unwrap();
        server.world.entity_mut(man).unwrap().wander = None;
        server.world.set_hostile(man, true);
        server.advance(0.0);
        server.advance(0.6); // he is next to the player and settles there
        let man_tile = |server: &mut Server| {
            server.snapshot().entities.iter().find(|e| e.id == man).unwrap().tile
        };
        let before = man_tile(&mut server);

        // the player walks away: on that very tick the man has not moved yet
        server.handle_command(ClientCommand::SetRunning(false));
        server.handle_command(ClientCommand::WalkTo { tile: TileCoord::new(-6, 0), force_inverse: false });
        server.advance(0.6);
        assert_eq!(man_tile(&mut server), before, "he does not react on the tick of the move");
        // the next tick he starts after the position the player reached
        server.advance(0.6);
        assert_ne!(man_tile(&mut server), before);
    }

    #[test]
    fn taking_off_and_dropping() {
        let mut server = inventory_server();
        server.handle_command(ClientCommand::InventoryOption { slot: 0, option: "Wield".into() });
        server.handle_command(ClientCommand::Unequip { slot: EquipSlot::Weapon });
        let snapshot = server.snapshot();
        assert!(snapshot.player.equipped.is_empty());
        assert!(snapshot.player.inventory.iter().flatten().any(|i| i.id == "bronze_sword"));

        server.handle_command(ClientCommand::InventoryOption { slot: 1, option: "Drop".into() });
        assert!(server.snapshot().player.inventory[1].is_none());
    }

    #[test]
    fn commands_for_unknown_targets_are_dropped() {
        let mut server = server();

        server.handle_command(ClientCommand::Attack { target: EntityId(99) });

        assert_eq!(server.snapshot().player.attack_target, None);
    }

    #[test]
    fn pause_is_a_server_side_state_a_client_can_only_request() {
        let mut server = server();
        server.handle_command(ClientCommand::SetPaused(true));

        assert!(server.advance(10.0).is_none());
        assert!(server.snapshot().paused);

        server.handle_command(ClientCommand::RequestSingleStep);
        assert_eq!(server.advance(0.0).map(|s| s.tick), Some(1));
    }

    #[test]
    fn a_spawned_server_ticks_on_its_own_and_publishes_snapshots() {
        let handle = spawn_server(|| {
            // Fast ticks so the test is quick; pacing is still entirely the server's.
            let world = dev_open_floor_world(15, 1, TileCoord::new(0, 0), PlayerConfig::default());
            Server::new(world, TickDriver::new(0.01, 1.0, 5))
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut latest = 0;
        while latest < 3 && Instant::now() < deadline {
            latest = handle.drain_snapshots().last().map_or(latest, |s| s.tick);
            thread::sleep(Duration::from_millis(10));
        }

        assert!(latest >= 3, "the server thread should have ticked by itself, got tick {latest}");
    }
}
