use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use super::config::{BossConfig, HealerConfig, NpcConfig, PassiveConfig, PlayerConfig};
use super::entity::{Behavior, BossState, Entity};
use super::events::{Combatant, WorldEvent};
use super::inventory::ItemStack;
use super::player::Player;
use super::sounds;
use crate::actors::{ActorKind, EntityId, MovementPolicy, StationaryMovementPolicy};
use crate::collision::{CollisionFlag, CollisionMap};
use crate::data::{EquipSlot, Registry};
use crate::combat::{
    get_damage_multiplier, resolve_heal, roll_damage, roll_damage_with_floor, AttackDefinition, AttackStyle, CombatStats, HazardAnchor,
    CombatState, HomingPulseOrb, NpcCombatState,
};
use crate::encounters::{EncounterContext, Phase, TriggerAction, TriggerBinding};
use crate::model::ConditionContext;
use crate::pathfinding::{BfsPathfinder, TargetSpec};
use crate::tick::{TickPhase, TickRegistrar, TickScheduler};
use crate::tiles::TileCoord;

/// The player is always entity 0; spawned entities count up from 1.
pub const PLAYER_ID: EntityId = EntityId(0);

/// Pseudo-entity registered in the Regen phase that advances the encounter's elapsed-tick
/// counter (the C# `DevOpenFloorCollisionMap`'s `advanceEncounterHook`). Regen is the last phase
/// of a tick, so every other phase this same tick still sees the PREVIOUS tick's count.
pub const ENCOUNTER_CLOCK_ID: EntityId = EntityId(u32::MAX);

/// How many ticks (10 minutes) a player can stand near aggressive monsters before they stop
/// attacking them unprompted (reference section 4, "tolerance").
pub const TOLERANCE_TICKS: i32 = 1000;

/// Builds an entity at the moment it is activated. The C# healers/elementals are inactive scene
/// objects whose `Awake` (actor + mover construction, which marks occupancy) only runs when a
/// trigger activates them - a blueprint defers the same work the same way.
pub type SpawnBlueprint = Box<dyn FnOnce(EntityId, &mut CollisionMap) -> Entity>;

type Slot = Option<Box<Entity>>;

/// Everything in one fight: the collision map, pathfinder, encounter state, the player, every
/// NPC, and the scheduler that ticks them in OSRS phase order (the C# `WorldContext` plus the
/// logic of each Runtime component).
///
/// Entities are owned here and addressed by `EntityId`. While one entity's tick function runs it
/// is temporarily taken out of its slot, so the function can freely mutate the rest of the world
/// (the player, other entities, the map) without aliasing.
///
/// Within a phase, entities tick in registration order. The C# order depends on Unity's scene
/// script order; here it is the order of the `spawn_*` calls (the player, then the encounter
/// clock, are always first).
pub struct World {
    pub map: CollisionMap,
    pub pathfinder: BfsPathfinder,
    pub encounter: EncounterContext,
    pub player: Player,
    entities: Vec<Slot>,
    scheduler: TickScheduler<EntityId>,
    spawn_groups: HashMap<String, Vec<SpawnBlueprint>>,
    log: Vec<String>,
    messages: Vec<String>,
    events: Vec<WorldEvent>,
    /// Fighters whose health reached 0 during the last tick; their death is confirmed at the end
    /// of the next one (the game shows the killing hit for a tick before the death plays).
    dying: Vec<Combatant>,
    /// Fighters whose death has been confirmed (a `Died` event was sent).
    dead: HashSet<Combatant>,
    registry: Rc<Registry>,
    /// Consecutive ticks the player has spent within reach of an aggressive monster that is
    /// subject to tolerance; reset the moment they leave.
    aggro_region_ticks: i32,
}

fn slot_index(id: EntityId) -> Option<usize> {
    if id == PLAYER_ID || id == ENCOUNTER_CLOCK_ID {
        None
    } else {
        Some(id.0 as usize - 1)
    }
}

fn entity_in(entities: &[Slot], id: EntityId) -> Option<&Entity> {
    slot_index(id).and_then(|i| entities.get(i)).and_then(|slot| slot.as_deref())
}

fn entity_is_dead(entities: &[Slot], id: EntityId) -> bool {
    entity_in(entities, id).is_some_and(Entity::is_dead)
}

/// Up to `count` open tiles around a monster of footprint `size` whose south-west tile is `origin`
/// for its helpers to appear on, avoiding `taken`: the middles of its four sides first, then its
/// corners, then a ring further out.
fn summon_spots(map: &CollisionMap, origin: TileCoord, size: i32, taken: &[TileCoord], count: usize) -> Vec<TileCoord> {
    let mid = size / 2;
    let mut candidates = Vec::new();
    for ring in 1..=3 {
        let (lo, hi) = (-ring, size - 1 + ring);
        candidates.extend([(mid, hi), (hi, mid), (mid, lo), (lo, mid)]);
        candidates.extend([(lo, lo), (hi, lo), (lo, hi), (hi, hi)]);
    }
    let mut spots = Vec::new();
    for (dx, dz) in candidates {
        let tile = TileCoord::new(origin.x + dx, origin.z + dz);
        let open = map.flags_at(tile) & CollisionFlag::BLOCK_FULL == 0;
        if open && !taken.contains(&tile) && !spots.contains(&tile) {
            spots.push(tile);
            if spots.len() == count {
                break;
            }
        }
    }
    spots
}

/// The sound of using consumable `item_id`: its own, else eating or drinking by its group.
fn consumable_sound(registry: &Registry, item_id: &str) -> Option<u32> {
    let def = registry.consumable(item_id)?;
    def.sound.or(Some(if def.delay_group == "potion" { sounds::DRINK } else { sounds::EAT }))
}

fn monster_distance_to_player(entity: &Entity, player: &Player) -> i32 {
    let size = entity.mover.actor.footprint_size();
    TargetSpec::distance_to_rect(player.tile(), entity.tile(), size, size)
}

fn hook_phases(behavior: &Behavior) -> &'static [TickPhase] {
    match behavior {
        Behavior::Passive => &[TickPhase::NpcMovement],
        _ => &[TickPhase::NpcAi, TickPhase::NpcMovement],
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Npc,
    Boss,
    Healer,
    Passive,
}

impl World {
    /// Builds a world around `map` with the player standing on `player_start`. The player and
    /// the encounter clock are registered first.
    pub fn new(mut map: CollisionMap, encounter_seed: i32, player_start: TileCoord, player_config: PlayerConfig) -> Self {
        let player = Player::new(player_start, &player_config, &mut map);

        let mut scheduler = TickScheduler::new();
        scheduler.register(TickPhase::Regen, ENCOUNTER_CLOCK_ID);
        scheduler.register(TickPhase::PlayerQueue, PLAYER_ID);
        scheduler.register(TickPhase::PlayerMovement, PLAYER_ID);
        scheduler.register(TickPhase::Regen, PLAYER_ID);

        Self {
            map,
            pathfinder: BfsPathfinder::new(),
            encounter: EncounterContext::new(encounter_seed),
            player,
            entities: Vec::new(),
            scheduler,
            spawn_groups: HashMap::new(),
            log: Vec::new(),
            messages: Vec::new(),
            events: Vec::new(),
            dying: Vec::new(),
            dead: HashSet::new(),
            registry: Rc::new(Registry::new()),
            aggro_region_ticks: 0,
        }
    }

    // ---- data ----

    pub fn registry(&self) -> &Rc<Registry> {
        &self.registry
    }

    /// Supplies the item/prayer/spell/monster definitions the data-driven commands resolve against.
    pub fn set_registry(&mut self, registry: Rc<Registry>) {
        self.registry = registry;
        let registry = Rc::clone(&self.registry);
        self.player.refresh_stats(&registry);
    }

    /// Whether aggressive monsters currently consider the player tolerated (10+ minutes nearby).
    pub fn player_is_tolerated(&self) -> bool {
        self.aggro_region_ticks >= TOLERANCE_TICKS
    }

    /// Spawns a monster from its data definition. It starts passive: it turns hostile when it
    /// aggroes on the player, when the player attacks it, or via `set_hostile`.
    pub fn spawn_npc_def(&mut self, npc_id: &str, start: TileCoord) -> Result<EntityId, String> {
        let def = self.registry.npc(npc_id).ok_or_else(|| format!("unknown monster '{npc_id}'"))?;
        let def = Rc::new(def.clone());
        let entity = Entity::new_from_def(self.next_id(), &def.name, start, &def, &mut self.map);
        Ok(self.add_entity(entity))
    }

    /// Forces a data-defined monster to start (or stop) fighting the player.
    pub fn set_hostile(&mut self, id: EntityId, hostile: bool) {
        if let Some(Behavior::Boss(state)) = self.entity_mut(id).map(|entity| &mut entity.behavior) {
            state.targets_player = hostile;
        }
    }

    // ---- entities ----

    pub fn entity(&self, id: EntityId) -> Option<&Entity> {
        entity_in(&self.entities, id)
    }

    pub fn entity_mut(&mut self, id: EntityId) -> Option<&mut Entity> {
        let index = slot_index(id)?;
        self.entities.get_mut(index).and_then(|slot| slot.as_deref_mut())
    }

    /// Every entity ever created (including dormant-then-activated ones), in ID order.
    pub fn entities(&self) -> impl Iterator<Item = &Entity> {
        self.entities.iter().filter_map(|slot| slot.as_deref())
    }

    fn next_id(&self) -> EntityId {
        EntityId(self.entities.len() as u32 + 1)
    }

    fn take_entity(&mut self, id: EntityId) -> Option<Box<Entity>> {
        let index = slot_index(id)?;
        self.entities.get_mut(index)?.take()
    }

    fn put_entity(&mut self, entity: Box<Entity>) {
        let index = slot_index(entity.id).expect("entities never use the reserved IDs");
        self.entities[index] = Some(entity);
    }

    fn place_entity(&mut self, entity: Entity) -> (EntityId, &'static [TickPhase]) {
        let id = entity.id;
        let phases = hook_phases(&entity.behavior);
        debug_assert_eq!(id, self.next_id(), "entities must be placed in ID order");
        self.entities.push(Some(Box::new(entity)));
        (id, phases)
    }

    fn add_entity(&mut self, entity: Entity) -> EntityId {
        let (id, phases) = self.place_entity(entity);
        for &phase in phases {
            self.scheduler.register(phase, id);
        }
        id
    }

    pub fn spawn_npc(&mut self, name: &str, start: TileCoord, config: NpcConfig) -> EntityId {
        let entity = Entity::new_npc(self.next_id(), name, start, &config, &mut self.map);
        self.add_entity(entity)
    }

    pub fn spawn_boss(&mut self, name: &str, start: TileCoord, config: BossConfig) -> EntityId {
        let entity = Entity::new_boss(self.next_id(), name, start, &config, &mut self.map);
        self.add_entity(entity)
    }

    pub fn spawn_healer(&mut self, name: &str, start: TileCoord, heal_target: Option<EntityId>, config: HealerConfig) -> EntityId {
        let entity = Entity::new_healer(self.next_id(), name, start, heal_target, &config, &mut self.map);
        self.add_entity(entity)
    }

    pub fn spawn_passive(&mut self, name: &str, start: TileCoord, config: PassiveConfig) -> EntityId {
        let entity = Entity::new_passive(self.next_id(), name, start, &config, &mut self.map);
        self.add_entity(entity)
    }

    /// Registers entities that stay dormant until a `SpawnEntities` trigger names `spawn_id`.
    pub fn register_spawn_group(&mut self, spawn_id: &str, blueprints: Vec<SpawnBlueprint>) {
        self.spawn_groups.entry(spawn_id.to_owned()).or_default().extend(blueprints);
    }

    /// The C# `BossCombatant.Configure`: installs the boss's phases, starting phase and triggers.
    pub fn configure_boss(&mut self, boss: EntityId, phases: Vec<Phase>, initial_phase: Phase, triggers: Vec<TriggerBinding>) {
        let entity = self.entity_mut(boss).expect("configure_boss: unknown entity");
        let Behavior::Boss(state) = &mut entity.behavior else {
            panic!("configure_boss: entity {boss:?} is not a boss");
        };

        state.phases_by_name.clear();
        for phase in phases {
            state.phases_by_name.insert(phase.name.clone(), phase);
        }
        state.combat_state = Some(NpcCombatState::new(Rc::clone(&initial_phase.attack_pattern)));
        state.current_phase = Some(initial_phase);
        state.triggers = triggers;
    }

    // ---- player commands (the public API of PlayerMover) ----

    pub fn player_set_attack_target(&mut self, target: EntityId) {
        self.player.set_attack_target(target);
    }

    pub fn player_set_interact_target(&mut self, target: EntityId) {
        self.player.set_interact_target(target);
    }

    pub fn player_request_walk_to(&mut self, tile: TileCoord, force_inverse: bool) {
        self.player.request_walk_to(tile, force_inverse);
    }

    pub fn player_equip(&mut self, item_id: &str) -> Result<(), String> {
        let registry = Rc::clone(&self.registry);
        self.player.equip(&registry, item_id)
    }

    pub fn player_unequip(&mut self, slot: EquipSlot) {
        let registry = Rc::clone(&self.registry);
        self.player.unequip(&registry, slot);
    }

    // ---- inventory ----

    /// What `item_id` is called: its item or consumable name, else the id.
    pub fn item_name(&self, item_id: &str) -> String {
        if let Some(item) = self.registry.item(item_id) {
            return item.name.clone();
        }
        if let Some(consumable) = self.registry.consumable(item_id) {
            return consumable.name.clone();
        }
        item_id.to_owned()
    }

    /// Whether items of this kind stack in one slot (ammunition does).
    pub fn item_is_stackable(&self, item_id: &str) -> bool {
        self.registry.item(item_id).is_some_and(|item| item.ammo.is_some() || item.slot == Some(EquipSlot::Ammo))
    }

    /// Puts items in the player's inventory. Returns how many did not fit.
    /// Puts `quantity` of an item in the inventory, returning how many did not fit.
    pub fn give_item(&mut self, item_id: &str, quantity: u32) -> Result<u32, String> {
        if self.registry.item(item_id).is_none() && self.registry.consumable(item_id).is_none() {
            return Err(format!("unknown item '{item_id}'"));
        }
        let stackable = self.item_is_stackable(item_id);
        Ok(self.player.inventory.add(item_id, quantity, stackable))
    }

    /// Choosing `option` ("Wield", "Eat", "Drop", ...) on the item in inventory `slot`.
    pub fn player_inventory_option(&mut self, slot: usize, option: &str) {
        let Some(stack) = self.player.inventory.get(slot).cloned() else { return };
        let name = self.item_name(&stack.item).to_lowercase();
        match option.to_ascii_lowercase().as_str() {
            "wield" | "wear" | "equip" | "hold" => {
                let registry = Rc::clone(&self.registry);
                let Some(item) = registry.item(&stack.item) else {
                    self.game_message(format!("You can't wear the {name}."));
                    return;
                };
                let Some(slot_worn) = item.slot else {
                    self.game_message(format!("You can't wear the {name}."));
                    return;
                };
                let replaced = self.player.loadout.item_id_in(slot_worn).map(str::to_owned);
                if let Err(message) = self.player.equip(&registry, &stack.item) {
                    self.game_message(format!("You can't wear the {name}: {message}."));
                    return;
                }
                self.player.inventory.take(slot);
                if let Some(old) = replaced {
                    // what was worn takes the place of what is put on
                    let _ = self.player.inventory.put(slot, ItemStack { item: old, quantity: 1 });
                }
            }
            "eat" | "drink" => {
                let registry = Rc::clone(&self.registry);
                if registry.consumable(&stack.item).is_none() {
                    self.game_message(format!("You can't {} the {name}.", option.to_lowercase()));
                    return;
                }
                match self.player.use_consumable(&registry, &stack.item) {
                    Ok(()) => {
                        self.play_sound(consumable_sound(&registry, &stack.item), Combatant::Player, 0);
                        self.player.inventory.take_one(slot);
                        // what it leaves takes its place: the next dose down, or the empty vial
                        let leaves = registry.consumable(&stack.item).and_then(|def| def.leaves.clone());
                        if let Some(rest) = &leaves {
                            let _ = self.player.inventory.put(slot, ItemStack { item: rest.clone(), quantity: 1 });
                        }
                        let message = if option.eq_ignore_ascii_case("drink") {
                            // "prayer potion(3)" is "prayer potion" in the game's own message
                            let potion = name.split('(').next().unwrap_or(&name);
                            let finished = leaves.as_deref().is_none_or(|rest| registry.consumable(rest).is_none());
                            if finished {
                                format!("You drink some of your {potion}. You have finished your potion.")
                            } else {
                                format!("You drink some of your {potion}.")
                            }
                        } else {
                            format!("You eat the {name}.")
                        };
                        self.game_message(message);
                    }
                    Err(message) => self.game_message(format!("You can't do that: {message}.")),
                }
            }
            "drop" | "destroy" => {
                self.player.inventory.take(slot);
                self.game_message(format!("You drop the {name}."));
            }
            "use" => self.game_message(format!("You use the {name}. Nothing interesting happens.")),
            _ => {}
        }
    }

    /// Takes the worn item in `slot` off and into the inventory (if there is room).
    pub fn player_unequip_to_inventory(&mut self, slot: EquipSlot) {
        let Some(item) = self.player.loadout.item_id_in(slot).map(str::to_owned) else { return };
        let stackable = self.item_is_stackable(&item);
        let room = self.player.inventory.free_slots() > 0 || (stackable && self.player.inventory.count(&item) > 0);
        if !room {
            self.game_message("You don't have enough inventory space to do that.".to_owned());
            return;
        }
        self.player_unequip(slot);
        self.player.inventory.add(&item, 1, stackable);
    }

    pub fn player_set_stance(&mut self, index: usize) -> Result<(), String> {
        let registry = Rc::clone(&self.registry);
        self.player.set_stance(&registry, index)
    }

    pub fn player_toggle_prayer(&mut self, prayer_id: &str) -> Result<bool, String> {
        let registry = Rc::clone(&self.registry);
        let on = self.player.toggle_prayer(&registry, prayer_id)?;
        let sound = if on { registry.prayer(prayer_id).and_then(|prayer| prayer.sound) } else { Some(sounds::PRAYER_OFF) };
        self.play_sound(sound, Combatant::Player, 0);
        Ok(on)
    }

    pub fn player_select_spell(&mut self, spell_id: Option<&str>) -> Result<(), String> {
        let registry = Rc::clone(&self.registry);
        self.player.select_spell(&registry, spell_id)
    }

    pub fn player_set_special(&mut self, enabled: bool) -> Result<(), String> {
        let registry = Rc::clone(&self.registry);
        self.player.set_special_enabled(&registry, enabled)
    }

    pub fn player_use_consumable(&mut self, consumable_id: &str) -> Result<(), String> {
        let registry = Rc::clone(&self.registry);
        self.player.use_consumable(&registry, consumable_id)?;
        self.play_sound(consumable_sound(&registry, consumable_id), Combatant::Player, 0);
        Ok(())
    }

    /// Records sound effect `sound`, if there is one, heard from `from` in `delay_ticks`.
    fn play_sound(&mut self, sound: Option<u32>, from: Combatant, delay_ticks: i32) {
        if let Some(sound) = sound {
            self.events.push(WorldEvent::Sound { sound, from, delay_ticks });
        }
    }

    pub fn player_set_on_task(&mut self, on_task: bool) {
        self.player.on_slayer_task = on_task;
    }

    // ---- diagnostics ----

    /// The messages the Unity components would `Debug.Log`, in order.
    pub fn log(&self) -> &[String] {
        &self.log
    }

    fn log_line(&mut self, message: String) {
        self.log.push(message);
    }

    /// A line the player reads in the chatbox (rejections, what an item did). Combat results are
    /// not among them: those go to the developer log (`log`) and, as events, to the client.
    fn game_message(&mut self, message: String) {
        self.log.push(message.clone());
        self.messages.push(message);
    }

    /// The player's messages since this was last called.
    pub fn take_messages(&mut self) -> Vec<String> {
        std::mem::take(&mut self.messages)
    }

    /// Whether `who`'s death has been confirmed: its health reached 0 a tick ago or more.
    pub fn death_confirmed(&self, who: Combatant) -> bool {
        self.dead.contains(&who)
    }

    /// Confirms the deaths noticed last tick (emitting `Died`) and notices this tick's.
    fn settle_deaths(&mut self) {
        for who in std::mem::take(&mut self.dying) {
            if self.dead.insert(who) {
                self.events.push(WorldEvent::Died { who });
                let sound = match who {
                    Combatant::Player => Some(sounds::PLAYER_DEATH),
                    Combatant::Npc(id) => self.entity(id).and_then(|e| e.def.as_ref()).and_then(|def| def.sounds.death),
                };
                self.play_sound(sound, who, 0);
            }
        }
        let mut fresh = Vec::new();
        if self.player.is_dead() {
            fresh.push(Combatant::Player);
        }
        fresh.extend(self.entities().filter(|e| e.is_dead()).map(|e| Combatant::Npc(e.id)));
        self.dying.extend(fresh.into_iter().filter(|who| !self.dead.contains(who)));
    }

    /// What has happened (attacks, hits, deaths) since this was last called.
    pub fn take_events(&mut self) -> Vec<WorldEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn current_tick(&self) -> i32 {
        self.scheduler.current_tick()
    }

    /// A one-line summary of the whole world's observable state, for tracing and golden tests.
    pub fn snapshot(&self) -> String {
        let player = &self.player;
        let health = player.mover.actor.health();
        let mut line = format!(
            "t={} P{} hp={} pr={}",
            self.current_tick(),
            player.tile(),
            health.current(),
            player.prayer.current()
        );
        for entity in self.entities() {
            line.push_str(&format!(" | {}{} hp={}", entity.name, entity.tile(), entity.health().current()));
        }
        line
    }

    // ---- ticking ----

    /// Runs one full game tick: every phase in order, every registered entity in registration order.
    pub fn run_tick(&mut self) {
        let mut scheduler = std::mem::take(&mut self.scheduler);
        scheduler.run_tick(|phase, id, tick, registrar| self.dispatch(phase, id, tick, registrar));
        self.scheduler = scheduler;
        self.settle_deaths();
    }

    fn dispatch(&mut self, phase: TickPhase, id: EntityId, _tick: i32, registrar: &mut TickRegistrar<EntityId>) {
        if id == ENCOUNTER_CLOCK_ID {
            if phase == TickPhase::Regen {
                self.encounter.advance_tick();
                self.update_aggro_region();
            }
            return;
        }

        if id == PLAYER_ID {
            match phase {
                TickPhase::PlayerQueue => self.player_run_queue(),
                TickPhase::PlayerMovement => self.player_run_movement(),
                TickPhase::Regen => {
                    let registry = Rc::clone(&self.registry);
                    let praying = !self.player.active_prayers.is_empty();
                    self.player.run_regen(&registry);
                    if praying && self.player.active_prayers.is_empty() {
                        self.play_sound(Some(sounds::PRAYER_DRAINED), Combatant::Player, 0);
                    }
                }
                _ => {}
            }
            return;
        }

        let Some(mut entity) = self.take_entity(id) else { return };
        let kind = match entity.behavior {
            Behavior::Npc(_) => Kind::Npc,
            Behavior::Boss(_) => Kind::Boss,
            Behavior::Healer(_) => Kind::Healer,
            Behavior::Passive => Kind::Passive,
        };

        match (kind, phase) {
            (Kind::Npc, TickPhase::NpcAi) => self.run_npc_ai(&mut entity),
            (Kind::Npc, TickPhase::NpcMovement) => self.run_npc_movement(&mut entity),
            (Kind::Boss, TickPhase::NpcAi) => self.run_boss_ai(&mut entity, registrar),
            (Kind::Boss, TickPhase::NpcMovement) => self.run_boss_movement(&mut entity),
            (Kind::Healer, TickPhase::NpcAi) => self.run_heal_ai(&mut entity),
            (Kind::Healer, TickPhase::NpcMovement) => self.run_healer_movement(&mut entity),
            (Kind::Passive, TickPhase::NpcMovement) => self.run_passive_movement(&mut entity),
            _ => {}
        }

        self.put_entity(entity);
    }

    // ---- player ----

    fn player_run_queue(&mut self) {
        // The attack timer runs every tick, walking or standing, attacking or not: an attack is
        // allowed on a tick that began with it at zero.
        self.player.attack_ready = self.player.attack_cooldown <= 0;
        if self.player.attack_cooldown > 0 {
            self.player.attack_cooldown -= 1;
        }
        // set again below if the player is still walking up to what it clicked
        self.player.approaching_target = false;

        // Stunned (Aguascia's ice ball): can't attack and can't start a new move-click, but
        // whatever was already queued stays queued, resuming the instant the stun ends.
        if self.player.is_dead() || self.player.status_effects.is_stunned() {
            return;
        }

        if let Some(target_id) = self.player.attack_target {
            if self.entity(target_id).is_none_or(Entity::is_dead) {
                self.player.attack_target = None;
            } else {
                self.player_run_attack_tick(target_id);
                return; // an active attack target supersedes any queued walk-click
            }
        }

        if let Some(target_id) = self.player.interact_target {
            match self.entity(target_id) {
                Some(target) if !target.is_dead() => {
                    let size = target.mover.actor.footprint_size();
                    let reach = TargetSpec::adjacent(target.tile(), size, size);
                    if !reach.is_satisfied_by(self.player.tile()) {
                        self.player.approaching_target = true;
                        self.player.current_path_is_running = self.player.is_running;
                        self.player.mover.request_move(&mut self.pathfinder, &mut self.map, reach);
                    }
                    return;
                }
                _ => self.player.interact_target = None,
            }
        }

        let Some(click) = self.player.pending_click.take() else { return };
        self.player.current_path_is_running = if click.force_inverse { !self.player.is_running } else { self.player.is_running };
        self.player.mover.request_move(&mut self.pathfinder, &mut self.map, TargetSpec::exact(click.tile));
    }

    /// Re-pathed with full BFS every tick while out of range, same as any other player
    /// click-to-move - unlike NPC chase, the PLAYER keeps real obstacle-avoiding pathing even
    /// while auto-attacking. What counts as "in range" comes from the swing the current setup
    /// would make (adjacent for most melee, a few tiles for halberds, bows and spells).
    fn player_run_attack_tick(&mut self, target_id: EntityId) {
        let Some(mut target) = self.take_entity(target_id) else { return };
        let view = target.view();
        let registry = Rc::clone(&self.registry);

        let plan = match self.player.plan_attack(&registry, &view.stats) {
            Ok(plan) => plan,
            Err(message) => {
                self.game_message(format!("Player cannot attack: {message}."));
                self.player.attack_target = None;
                self.put_entity(target);
                return;
            }
        };

        if let Some(def) = &target.def {
            let context = ConditionContext {
                style: plan.style,
                damage_type: plan.definition.damage_type,
                target_attributes: view.stats.attributes(),
                flags: &plan.definition.flags,
            };
            if !def.attackable_if.holds(&context) {
                self.game_message(format!("Player cannot hurt {} with that.", target.name));
                self.player.attack_target = None;
                self.put_entity(target);
                return;
            }
        }

        let melee_adjacent = plan.style == AttackStyle::Melee && plan.range <= 1;
        let reach = if melee_adjacent {
            TargetSpec::adjacent(view.tile, view.footprint_size, view.footprint_size)
        } else {
            TargetSpec::within_range_of_rect(view.tile, view.footprint_size, view.footprint_size, plan.range)
        };
        let mut player_tile = self.player.tile();

        if !reach.is_satisfied_by(player_tile) {
            self.player.approaching_target = true;
            self.player.current_path_is_running = self.player.is_running;
            self.player.mover.request_move(&mut self.pathfinder, &mut self.map, reach);
            // Moving and acting share a tick: a step that ends in range is followed by the attack
            // on that same tick (from the tile the player arrives on), not on the next one.
            let arrival = self.player.tile_after_this_ticks_move();
            if !reach.is_satisfied_by(arrival) || !self.player.attack_ready {
                self.put_entity(target);
                return;
            }
            player_tile = arrival;
        }

        if !self.player.attack_ready {
            self.put_entity(target);
            return;
        }

        let stats = self.player.mover.actor.stats();
        let distance = TargetSpec::distance_to_rect(player_tile, view.tile, view.footprint_size, view.footprint_size);
        let special_cost = plan.special_cost;
        let speed = plan.speed;
        let ranged = plan.style == AttackStyle::Ranged;
        let attack = plan.finish(distance);
        self.events.push(WorldEvent::Attack { attacker: Combatant::Player, target: Combatant::Npc(target_id), animation: None });
        self.play_sound(attack.sound, Combatant::Player, 0);
        if ranged {
            // the ammunition flies (a thrown weapon would be its own graphic; none is drawn yet)
            if let Some(ammo) = self.player.loadout.item_id_in(EquipSlot::Ammo) {
                self.events.push(WorldEvent::Projectile {
                    from: Combatant::Player,
                    to: Combatant::Npc(target_id),
                    graphic: super::animations::projectile_graphic(ammo),
                    draw: super::animations::draw_graphic(ammo),
                    ticks: attack.projectile_travel_ticks.max(1),
                    released: false,
                });
            }
        }

        for _ in 0..attack.hit_count {
            // No player weapon uses impact_delay_ticks yet, but branching here anyway keeps this
            // call site identical in shape to the NPC ones, rather than a special case that would
            // silently need fixing the day a ranged/magic weapon arrives.
            if attack.impact_delay_ticks > 0 {
                // a player's successful hit never rolls 0
                target.pending_attacks.enqueue_with_floor(
                    Rc::clone(&attack),
                    player_tile,
                    Some(PLAYER_ID),
                    stats,
                    attack.impact_delay_ticks,
                    1,
                );
            } else {
                // Rolled (not applied) so it goes through the target's own DamageBuffer, same as
                // every other attacker - melee has 0 travel ticks, so it still lands this same
                // tick; ranged and magic land after their projectile's flight.
                // a player's successful hit never rolls 0: only a miss is 0
                let damage = roll_damage_with_floor(&attack, &stats, &view.stats, view.kind == ActorKind::Npc, self.encounter.rng(), 1.0, 1);
                target.damage_buffer.enqueue(damage, attack.projectile_travel_ticks, attack.on_hit_status_effect);
                // a spell is heard landing, or splashing when it misses
                if attack.style == AttackStyle::Magic {
                    let landing = if damage == 0 { Some(sounds::SPELL_SPLASH) } else { attack.hit_sound };
                    self.play_sound(landing, Combatant::Npc(target_id), attack.projectile_travel_ticks);
                }
                self.log_line(format!("Player hits for {damage} (target HP will reflect it once the hit lands)."));
            }
        }

        // Being hit makes a data-defined monster fight back.
        if target.def.is_some() {
            if let Behavior::Boss(state) = &mut target.behavior {
                state.targets_player = true;
            }
        }

        if let Some(cost) = special_cost {
            self.player.special_energy = (self.player.special_energy - cost).max(0);
            self.player.special_enabled = false;
        }
        // The swing resolves this tick; the next may start `speed` ticks after it.
        self.player.attack_cooldown = speed - 1;
        self.put_entity(target);
    }

    fn player_run_movement(&mut self) {
        let entities = &self.entities;
        let attacker_is_dead = |id: EntityId| entity_is_dead(entities, id);
        let landed = self.player.run_movement(&mut self.map, self.encounter.rng(), &attacker_is_dead);
        for damage in landed {
            self.events.push(WorldEvent::Hit { target: Combatant::Player, damage });
            let sound = if damage == 0 { sounds::PLAYER_BLOCK } else { sounds::PLAYER_HIT };
            self.play_sound(Some(sound), Combatant::Player, 0);
        }
        let registry = Rc::clone(&self.registry);
        self.player.after_damage(&registry);
    }

    // ---- shared NPC plumbing ----

    /// The incoming-damage pipeline shared by every NPC kind, run at the start of its
    /// NpcMovement phase: it runs after NpcAi/PlayerQueue (where hits against this entity are
    /// enqueued, e.g. by the player's attack tick) within the same tick, so a
    /// 0-delay/0-travel-tick hit still lands the tick it was rolled. NPCs have no reaction
    /// state of their own (nothing here has a prayer book), so a Doom-style attack against them
    /// always rolls for full damage once its delay elapses.
    fn advance_incoming(&mut self, entity: &mut Entity) {
        let view = entity.view();
        let player = &self.player;
        let entities = &self.entities;
        let attacker_is_dead =
            |id: EntityId| if id == PLAYER_ID { player.is_dead() } else { entity_is_dead(entities, id) };

        entity.pending_attacks.advance(&view, &mut entity.damage_buffer, None, self.encounter.rng(), &attacker_is_dead);
        let landed = entity.damage_buffer.advance(entity.mover.actor.health_mut(), None);
        let target = Combatant::Npc(entity.id);
        let hit_sound = entity.def.as_ref().and_then(|def| def.sounds.hit);
        for damage in landed {
            self.events.push(WorldEvent::Hit { target, damage });
            self.play_sound(hit_sound, target, 0);
        }
    }

    /// Resolves an attack that has just finished telegraphing against the player: a Doom-style
    /// attack (impact delay > 0) is handed to the player's pending queue to roll later;
    /// anything else rolls now and its damage goes to the player's damage buffer.
    fn fire_at_player(
        &mut self,
        attacker: EntityId,
        attacker_name: &str,
        attacker_stats: CombatStats,
        attack: &Rc<AttackDefinition>,
        hazard_origin: TileCoord,
    ) {
        // a telegraphed attack was reported when its telegraph began (`report_telegraph_start`)
        if attack.telegraph_ticks == 0 {
            self.events.push(WorldEvent::Attack {
                attacker: Combatant::Npc(attacker),
                target: Combatant::Player,
                animation: attack.animation,
            });
            self.play_sound(attack.sound, Combatant::Npc(attacker), 0);
        }
        // an attack with a sound of its own at the roll (Jad's stomp) is heard now
        self.play_sound(attack.hit_sound, Combatant::Npc(attacker), 0);
        // what the client draws of it: the hit lands after the delay and the flight
        let lands_in = attack.impact_delay_ticks + attack.projectile_travel_ticks;
        if let Some(graphic) = attack.projectile {
            self.events.push(WorldEvent::Projectile {
                from: Combatant::Npc(attacker),
                to: Combatant::Player,
                graphic,
                draw: None,
                ticks: lands_in.max(1),
                released: attack.telegraph_ticks > 0,
            });
        }
        if let Some(graphic) = attack.target_graphic {
            self.events.push(WorldEvent::Graphic { on: Combatant::Player, graphic, delay_ticks: 0 });
        }
        if let Some(graphic) = attack.impact_graphic {
            self.events.push(WorldEvent::Graphic { on: Combatant::Player, graphic, delay_ticks: lands_in });
        }

        // The attacker's own turn ends the instant it launches the attack; waiting out the delay
        // moves to the TARGET's queue. The attacker's ID is captured too, so if it dies before
        // the delay elapses, the shot still dies with it.
        if attack.impact_delay_ticks > 0 {
            for _ in 0..attack.hit_count {
                self.player.pending_attacks.enqueue(
                    Rc::clone(attack),
                    hazard_origin,
                    Some(attacker),
                    attacker_stats,
                    attack.impact_delay_ticks,
                );
            }
            self.log_line(format!(
                "{attacker_name} fires {} at the player (impact in {} ticks).",
                attack.name, attack.impact_delay_ticks
            ));
            return;
        }

        for _ in 0..attack.hit_count {
            let player_view = self.player.view();
            let multiplier =
                get_damage_multiplier(attack, Some(&self.player.reaction_state), hazard_origin, player_view.tile);
            let damage = roll_damage(
                attack,
                &attacker_stats,
                &player_view.stats,
                player_view.kind == ActorKind::Npc,
                self.encounter.rng(),
                multiplier,
            );
            self.player.damage_buffer.enqueue(damage, attack.projectile_travel_ticks, attack.on_hit_status_effect);

            let landing = if attack.projectile_travel_ticks > 0 {
                format!(" (landing in {} ticks).", attack.projectile_travel_ticks)
            } else {
                ".".to_owned()
            };
            self.log_line(format!("{attacker_name} uses {} on the player for {damage}{landing}", attack.name));
        }
    }

    /// Reports an attack whose telegraph has just begun (`advance` moved `combat_state` from idle
    /// to telegraphing): the attacker starts its attack animation now, which is the warning the
    /// player reacts to (Jad rearing up). Attacks without a telegraph are reported as they resolve,
    /// in `fire_at_player`.
    fn report_telegraph_start(&mut self, attacker: EntityId, was_idle: bool, combat_state: &NpcCombatState) {
        if !was_idle || combat_state.state() != CombatState::Telegraphing {
            return;
        }
        let Some(attack) = combat_state.current_attack() else { return };
        if attack.telegraph_ticks > 0 {
            self.events.push(WorldEvent::Attack {
                attacker: Combatant::Npc(attacker),
                target: Combatant::Player,
                animation: attack.animation,
            });
            self.play_sound(attack.sound, Combatant::Npc(attacker), 0);
        }
    }

    // ---- plain NPC ----

    fn run_npc_ai(&mut self, entity: &mut Entity) {
        let id = entity.id;
        let Behavior::Npc(state) = &mut entity.behavior else { return };
        let actor = &entity.mover.actor;
        if actor.health().is_dead() || !state.targets_player || self.player.is_dead() {
            return;
        }

        let player_tile = self.player.tile();
        let in_range = state.attack.build_range_check(actor.current_tile(), actor.footprint_size()).is_satisfied_by(player_tile);
        let health_percent = actor.health().current() as f32 / actor.health().max() as f32;
        let combat_style = self.player.reaction_state.combat_style;

        let was_idle = state.combat_state.state() == CombatState::Idle;
        let resolved = state.combat_state.advance(
            in_range,
            health_percent,
            self.encounter.elapsed_ticks(),
            actor.current_tile(),
            player_tile,
            combat_style,
            self.encounter.rng(),
        );
        if !resolved {
            self.report_telegraph_start(id, was_idle, &state.combat_state);
            return;
        }

        let attack = Rc::clone(state.combat_state.current_attack().expect("an attack resolved, so one was selected"));
        // Reaction.Anchor picks which tile CAPTURED AT TELEGRAPH START this attack's hazard is
        // centered on - never a live tile, or moving away during the telegraph would never
        // actually dodge a target-anchored hazard.
        let hazard_origin = if attack.reaction.anchor == HazardAnchor::Target {
            state.combat_state.target_tile_at_selection()
        } else {
            state.combat_state.self_tile_at_selection()
        };
        let stats = actor.stats();

        self.fire_at_player(id, &entity.name, stats, &attack, hazard_origin);
    }

    fn run_npc_movement(&mut self, entity: &mut Entity) {
        self.advance_incoming(entity);
        if entity.is_dead() {
            // a body blocks nobody
            entity.mover.leave_map(&mut self.map);
            entity.mover.step(&mut self.map, 0);
            return;
        }

        let Behavior::Npc(state) = &entity.behavior else { return };
        let policy = Rc::clone(&state.movement_policy);
        let targets_player = state.targets_player;
        if !targets_player && self.wander_step(entity) {
            return;
        }
        self.step_towards_player_policy(entity, policy.as_ref(), targets_player);
    }

    /// A summoned healer that is not fighting the player goes to stand next to the monster it
    /// heals. Returns false (doing nothing) for anything else, or once its leader is dead.
    fn walk_to_leader(&mut self, entity: &mut Entity) -> bool {
        let healer = entity.def.as_ref().is_some_and(|def| def.heals_summoner.is_some());
        let Some(leader) = entity.leader.filter(|_| healer).and_then(|id| self.entity(id)) else { return false };
        if leader.is_dead() {
            return false;
        }
        let size = leader.mover.actor.footprint_size();
        let beside = TargetSpec::adjacent(leader.tile(), size, size);
        if beside.is_satisfied_by(entity.tile()) {
            entity.mover.step(&mut self.map, 0);
        } else {
            if entity.mover.destination().is_none() {
                entity.mover.request_move(&mut self.pathfinder, &mut self.map, beside);
            }
            entity.mover.step(&mut self.map, 1);
        }
        true
    }

    /// An idle wanderer's tick: carry on along the stroll it set off on, or perhaps set off on a
    /// new one. Returns false (doing nothing) for an entity that does not wander.
    fn wander_step(&mut self, entity: &mut Entity) -> bool {
        let Some(wander) = entity.wander.as_mut() else { return false };
        if entity.mover.destination().is_none() {
            if let Some(target) = wander.choose(entity.mover.actor.current_tile()) {
                entity.mover.request_move(&mut self.pathfinder, &mut self.map, TargetSpec::exact(target));
            }
        }
        entity.mover.step(&mut self.map, 1);
        true
    }

    /// Movement against the player: hands the policy a view of the player if it is a valid
    /// (assigned and alive) target, plus the style it is currently fighting in.
    fn step_towards_player_policy(&mut self, entity: &mut Entity, policy: &dyn MovementPolicy, targets_player: bool) {
        // Red-x: while the player is still walking up to something it clicked to interact with,
        // an NPC after the player holds still. Only a walk click (which clears that target) or
        // reaching it lifts this; clicking something else to interact keeps it.
        if targets_player && self.player.approaching_target {
            entity.mover.step(&mut self.map, 0);
            return;
        }
        let target = (targets_player && !self.player.is_dead()).then(|| self.player.view());
        let style = if target.is_some() { self.player.reaction_state.combat_style } else { AttackStyle::Melee };
        policy.step(&mut entity.mover, &self.pathfinder, &mut self.map, target.as_ref(), style);
    }

    // ---- boss ----

    fn run_boss_ai(&mut self, entity: &mut Entity, registrar: &mut TickRegistrar<EntityId>) {
        self.update_aggression(entity);
        self.answer_summons(entity, registrar);
        self.heal_leader(entity);
        let id = entity.id;
        let Behavior::Boss(state) = &mut entity.behavior else { return };
        let actor = &entity.mover.actor;

        let target_alive = state.targets_player && !self.player.is_dead();

        // Orbs are deliberately independent of this boss's own attack-selection loop (and of
        // whether THIS boss is still alive) - they only need a living target to home toward, so
        // they advance unconditionally, before any of the early-returns below that gate the
        // boss's OWN turn to act.
        if target_alive && !state.active_orbs.is_empty() {
            self.advance_orbs(&mut state.active_orbs);
        }

        if !target_alive || state.current_phase.is_none() {
            return;
        }

        let health_percent = actor.health().current() as f32 / actor.health().max() as f32;

        // Evaluated every tick BEFORE the death early-return below, with the real is_dead value,
        // so a "when I die, spawn X" trigger fires on the same tick as the killing blow.
        for i in 0..state.triggers.len() {
            let fired = state.triggers[i].try_fire(&self.encounter, health_percent, actor.health().is_dead());
            if let Some(action) = fired {
                self.apply_boss_action(&entity.name, state, action, registrar);
            }
        }

        if actor.health().is_dead() {
            return; // triggers above still fire on death; nothing else does
        }

        // Multi-attack NPCs don't gate telegraph-start on a single attack's range - per-attack
        // reachability (melee needs adjacency, magic doesn't) is the pattern's own job via
        // self_tile/target_tile in the selection context.
        let player_tile = self.player.tile();
        let combat_style = self.player.reaction_state.combat_style;
        // A pattern that knows about reach (data-defined monsters) only lets the fight begin when
        // one of its attacks can actually get to the player; every other pattern always engages.
        let engaged = state
            .current_phase
            .as_ref()
            .is_none_or(|phase| phase.attack_pattern.engages(actor.current_tile(), player_tile));
        let combat_state = state.combat_state.as_mut().expect("a boss with a phase has a combat state");
        let was_idle = combat_state.state() == CombatState::Idle;
        let resolved = combat_state.advance(
            engaged,
            health_percent,
            self.encounter.elapsed_ticks(),
            actor.current_tile(),
            player_tile,
            combat_style,
            self.encounter.rng(),
        );
        if !resolved {
            self.report_telegraph_start(id, was_idle, combat_state);
            return;
        }

        let attack = Rc::clone(combat_state.current_attack().expect("an attack resolved, so one was selected"));

        if let Some(orb_definition) = &attack.spawns_orb {
            // Slicer: this boss's own part is done the instant it's conjured - the orb lives and
            // pulses on its own from here, independent of this NPC's attack-selection loop.
            state.active_orbs.push(HomingPulseOrb::new(Rc::clone(orb_definition), actor.current_tile(), actor.stats()));
            let message = format!("{} conjures {}.", entity.name, orb_definition.name);
            self.log_line(message);
            return;
        }

        let hazard_origin = if attack.reaction.anchor == HazardAnchor::Target {
            combat_state.target_tile_at_selection()
        } else {
            combat_state.self_tile_at_selection()
        };
        let stats = actor.stats();

        self.fire_at_player(id, &entity.name, stats, &attack, hazard_origin);
    }

    fn advance_orbs(&mut self, orbs: &mut Vec<HomingPulseOrb>) {
        let player_view = self.player.view();

        for i in (0..orbs.len()).rev() {
            let pulsed = orbs[i].advance(player_view.tile);
            if pulsed && orbs[i].is_within_pulse_radius(player_view.tile) {
                let pulse_attack = Rc::clone(&orbs[i].definition().pulse_attack);
                let multiplier = get_damage_multiplier(
                    &pulse_attack,
                    Some(&self.player.reaction_state),
                    orbs[i].current_tile(),
                    player_view.tile,
                );
                let damage = roll_damage(
                    &pulse_attack,
                    &orbs[i].attacker_stats(),
                    &player_view.stats,
                    player_view.kind == ActorKind::Npc,
                    self.encounter.rng(),
                    multiplier,
                );
                if damage > 0 {
                    self.player.damage_buffer.enqueue(damage, pulse_attack.projectile_travel_ticks, pulse_attack.on_hit_status_effect);
                    orbs[i].extend_on_hit();
                }
            }

            if orbs[i].is_expired() {
                orbs.remove(i);
            }
        }
    }

    fn apply_boss_action(
        &mut self,
        boss_name: &str,
        state: &mut BossState,
        action: TriggerAction,
        registrar: &mut TickRegistrar<EntityId>,
    ) {
        match action {
            TriggerAction::SwapPhase { phase_name } => match state.phases_by_name.get(&phase_name).cloned() {
                Some(next) => {
                    if let Some(combat_state) = state.combat_state.as_mut() {
                        combat_state.set_pattern(Rc::clone(&next.attack_pattern));
                    }
                    state.current_phase = Some(next);
                }
                None => {
                    self.log_line(format!("{boss_name}: SwapPhase trigger named an unknown phase '{phase_name}'."));
                }
            },
            TriggerAction::SpawnEntities { spawn_id } => self.activate_spawn_group(&spawn_id, registrar),
            TriggerAction::SetFlag { flag_name, value } => self.encounter.set_flag(&flag_name, value),
            TriggerAction::ModifyStats { stat_name, delta } => {
                // Recognised-but-unimplemented, same stance as ReactionKind::Interrupt - nothing
                // in this project's encounters needs it yet.
                self.log_line(format!(
                    "{boss_name}: ModifyStats trigger fired (stat={stat_name}, delta={delta}) - not implemented yet, no effect."
                ));
            }
        }
    }

    /// Activates the dormant entities registered under `spawn_id`. Their hooks are registered
    /// through the tick's registrar, so - like Unity's `SetActive(true)` -  they join the schedule
    /// at the next phase boundary rather than mid-phase.
    fn activate_spawn_group(&mut self, spawn_id: &str, registrar: &mut TickRegistrar<EntityId>) {
        let Some(blueprints) = self.spawn_groups.remove(spawn_id) else { return };
        self.log_line(format!("Spawning {} entit(ies) for '{spawn_id}'.", blueprints.len()));

        for blueprint in blueprints {
            let entity = blueprint(self.next_id(), &mut self.map);
            let (id, phases) = self.place_entity(entity);
            for &phase in phases {
                registrar.register(phase, id);
            }
        }
    }

    /// A monster with `summons` calls its helpers once its hitpoints fall low enough (Jad below 150
    /// calls four Yt-HurKot). A call that may be answered again is re-armed when the monster has
    /// been healed back to full; the next time it falls low, only the helpers that died are
    /// replaced. Helpers join the schedule at the next phase boundary, like a spawn group.
    fn answer_summons(&mut self, entity: &mut Entity, registrar: &mut TickRegistrar<EntityId>) {
        let Some(def) = entity.def.clone() else { return };
        if def.summons.is_empty() || entity.is_dead() {
            return;
        }
        let (current, max) = (entity.health().current(), entity.health().max());
        for (index, summon) in def.summons.iter().enumerate() {
            if !entity.summons[index].armed {
                if summon.again_once_healed_to_full && current >= max {
                    entity.summons[index].armed = true;
                }
                continue;
            }
            if current > summon.below_hitpoints {
                continue;
            }
            entity.summons[index].armed = false;
            let entities = &self.entities;
            entity.summons[index].spawned.retain(|id| !entity_is_dead(entities, *id));
            let missing = (summon.count as usize).saturating_sub(entity.summons[index].spawned.len());
            let Some(helper) = self.registry.npc(&summon.npc).map(|d| Rc::new(d.clone())) else { continue };
            let size = entity.mover.actor.footprint_size();
            let taken: Vec<TileCoord> = entity.summons[index]
                .spawned
                .iter()
                .filter_map(|id| self.entity(*id).map(Entity::tile))
                .collect();
            let spots = summon_spots(&self.map, entity.tile(), size, &taken, missing);
            for tile in spots {
                let mut new = Entity::new_from_def(self.next_id(), &helper.name, tile, &helper, &mut self.map);
                new.leader = Some(entity.id);
                let (id, phases) = self.place_entity(new);
                for &phase in phases {
                    registrar.register(phase, id);
                }
                entity.summons[index].spawned.push(id);
            }
            self.log_line(format!("{} calls {} {} to its side.", entity.name, missing, helper.name));
        }
    }

    /// A summoned healer restores its leader's hitpoints every so often while it is close enough,
    /// unless the player has distracted it by attacking it. A distracted healer fights the player,
    /// and gives up on them (going back to healing) once they are far enough away.
    fn heal_leader(&mut self, entity: &mut Entity) {
        let Some(heal) = entity.def.as_ref().and_then(|def| def.heals_summoner) else { return };
        let Some(leader_id) = entity.leader else { return };
        if entity.is_dead() {
            return;
        }
        let player_distance = monster_distance_to_player(entity, &self.player);
        let Behavior::Boss(state) = &mut entity.behavior else { return };
        if state.targets_player && (self.player.is_dead() || player_distance > heal.forget_player_beyond) {
            state.targets_player = false;
        }
        if state.targets_player {
            return;
        }
        if entity.heal_cooldown > 0 {
            entity.heal_cooldown -= 1;
            return;
        }
        let healer_tile = entity.tile();
        let Some(leader) = self.entity_mut(leader_id) else { return };
        let size = leader.mover.actor.footprint_size();
        let close = TargetSpec::distance_to_rect(healer_tile, leader.tile(), size, size) <= heal.range;
        let hurt = leader.health().current() < leader.health().max();
        if leader.is_dead() || !close || !hurt {
            return;
        }
        leader.mover.actor.health_mut().heal(heal.amount);
        let leader_name = leader.name.clone();
        entity.heal_cooldown = heal.every_ticks - 1;
        self.events.push(WorldEvent::Heal {
            healer: Combatant::Npc(entity.id),
            target: Combatant::Npc(leader_id),
            amount: heal.amount,
            animation: heal.animation,
        });
        self.log_line(format!("{} heals {leader_name} for {}.", entity.name, heal.amount));
    }

    /// Reference section 4: an aggressive monster starts a fight with a player within its aggression
    /// range, unless the player has become tolerated, or is too high a combat level for it.
    fn update_aggression(&mut self, entity: &mut Entity) {
        let Some(def) = entity.def.clone() else { return };
        let Behavior::Boss(state) = &mut entity.behavior else { return };
        if state.targets_player || !def.aggression.aggressive || self.player.is_dead() || entity.mover.actor.health().is_dead() {
            return;
        }
        if !def.aggression.ignores_tolerance && self.player_is_tolerated() {
            return;
        }
        // At most double the monster's level, but anything level 63+ attacks everyone.
        let too_high_level = !def.aggression.ignores_combat_level && def.combat_level < 63 && self.player.combat_level() > 2 * def.combat_level;
        if too_high_level {
            return;
        }
        if monster_distance_to_player(entity, &self.player) <= def.aggression.range {
            if let Behavior::Boss(state) = &mut entity.behavior {
                state.targets_player = true;
            }
        }
    }

    /// Counts how long the player has been standing near tolerance-subject aggressive monsters.
    fn update_aggro_region(&mut self) {
        let player = &self.player;
        let near = !player.is_dead()
            && self.entities().any(|entity| {
                entity.def.as_ref().is_some_and(|def| {
                    def.aggression.aggressive
                        && !def.aggression.ignores_tolerance
                        && !entity.is_dead()
                        && monster_distance_to_player(entity, player) <= def.aggression.range
                })
            });
        self.aggro_region_ticks = if near { self.aggro_region_ticks + 1 } else { 0 };
    }

    fn run_boss_movement(&mut self, entity: &mut Entity) {
        self.advance_incoming(entity);
        if entity.is_dead() {
            // a body blocks nobody
            entity.mover.leave_map(&mut self.map);
            entity.mover.step(&mut self.map, 0);
            return;
        }

        // Movement is whatever the CURRENT phase's policy says (stationary if there is none).
        let Behavior::Boss(state) = &entity.behavior else { return };
        let policy: Rc<dyn MovementPolicy> = match &state.current_phase {
            Some(phase) => Rc::clone(&phase.movement_policy),
            None => Rc::new(StationaryMovementPolicy),
        };
        let pattern = state.current_phase.as_ref().map(|phase| Rc::clone(&phase.attack_pattern));
        let targets_player = state.targets_player;
        if !targets_player && self.walk_to_leader(entity) {
            return;
        }
        if !targets_player && self.wander_step(entity) {
            return;
        }
        // The base rule: a monster that can already hit its target stands still and attacks
        // rather than walking up to it (Jad stays put and fires from range).
        let in_reach = targets_player
            && !self.player.is_dead()
            && pattern.is_some_and(|pattern| pattern.reaches(entity.tile(), self.player.tile()) == Some(true));
        if in_reach && policy.holds_when_in_reach() {
            entity.mover.step(&mut self.map, 0);
            return;
        }
        self.step_towards_player_policy(entity, policy.as_ref(), targets_player);
    }

    // ---- healer ----

    fn run_heal_ai(&mut self, entity: &mut Entity) {
        let Behavior::Healer(state) = &entity.behavior else { return };
        if entity.is_dead() {
            return;
        }
        let Some(target_id) = state.heal_target else { return };
        let heal_amount = state.heal_amount_per_tick;
        let healer_tile = entity.tile();
        let name = entity.name.clone();

        let Some(target) = self.entity_mut(target_id) else { return };
        if target.is_dead() {
            return;
        }

        let target_view = target.view();
        let adjacent = TargetSpec::adjacent(target_view.tile, target_view.footprint_size, target_view.footprint_size)
            .is_satisfied_by(healer_tile);
        if !adjacent {
            return;
        }

        let healed = resolve_heal(heal_amount, target.mover.actor.health_mut());
        if healed > 0 {
            let message = format!(
                "{name} heals {} for {healed} (boss HP {}/{}).",
                target.name,
                target.health().current(),
                target.health().max()
            );
            self.log_line(message);
        }
    }

    fn run_healer_movement(&mut self, entity: &mut Entity) {
        // Nothing targets a healer with a Doom-style attack today, but it's an attack target like
        // everything else, so it still needs somewhere for one to wait.
        self.advance_incoming(entity);
        if entity.is_dead() {
            // a body blocks nobody
            entity.mover.leave_map(&mut self.map);
            entity.mover.step(&mut self.map, 0);
            return;
        }

        let Behavior::Healer(state) = &entity.behavior else { return };
        let policy = Rc::clone(&state.movement_policy);
        let target = state
            .heal_target
            .and_then(|id| self.entity(id))
            .filter(|target| !target.is_dead())
            .map(Entity::view);

        policy.step(&mut entity.mover, &self.pathfinder, &mut self.map, target.as_ref(), AttackStyle::Melee);
    }

    // ---- passive ----

    fn run_passive_movement(&mut self, entity: &mut Entity) {
        self.advance_incoming(entity);
        // No movement policy at all (not even Stationary): there is no target to hold adjacency
        // with, chase, or react to. Alive or dead, it just doesn't move (unless it wanders).
        if entity.is_dead() {
            entity.mover.leave_map(&mut self.map);
        } else if self.wander_step(entity) {
            return;
        }
        entity.mover.step(&mut self.map, 0);
    }
}
