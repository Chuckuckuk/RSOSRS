//! Ready-made worlds. Currently just the dev arena (`DevOpenFloorCollisionMap`). The Jad and
//! Purutu encounter setups (`JadEncounterSetup`, `PurutuEncounterSetup`) are intentionally not
//! ported yet; the boss/phase/trigger/spawn machinery they sit on top of lives in `World`.

use std::rc::Rc;

use super::config::{CombatLevels, PlayerConfig};
use super::world::World;
use crate::collision::{CollisionMap, CollisionMapData};
use crate::data::{Registry, ScenarioDef};
use crate::tiles::TileCoord;

/// `DevOpenFloorCollisionMap`: an open, flat floor of `2 * half_extent` tiles per side centred on
/// the origin, with the encounter seeded from `encounter_seed`. Unity defaults are
/// `half_extent = 15`, `encounter_seed = 12345`.
pub fn dev_open_floor_world(half_extent: i32, encounter_seed: i32, player_start: TileCoord, player_config: PlayerConfig) -> World {
    dev_open_floor_world_at(TileCoord::new(0, 0), half_extent, encounter_seed, player_start, player_config)
}

/// [`dev_open_floor_world`] centred on `center` instead of the origin, for placing the mock
/// server's floor under a real map location.
pub fn dev_open_floor_world_at(
    center: TileCoord,
    half_extent: i32,
    encounter_seed: i32,
    player_start: TileCoord,
    player_config: PlayerConfig,
) -> World {
    let size = half_extent * 2;
    let cell_count = (size * size) as usize;
    let map = CollisionMap::new(CollisionMapData {
        origin_x: center.x - half_extent,
        origin_z: center.z - half_extent,
        width: size,
        height: size,
        flags: vec![0; cell_count],
        heights: vec![0; cell_count],
        max_step_height: 32,
    })
    .expect("the dev floor's arrays always match its size");

    World::new(map, encounter_seed, player_start, player_config)
}

/// Builds a complete fight from its data description: the floor, the player (levels, gear,
/// prayers, boosts, stance, spell, inventory), and the monsters. Anything the definition asks for
/// that the rules refuse (a prayer above the player's level, an unknown item) is an error, not a
/// silent skip, so a scenario file cannot quietly describe a fight other than the one it says.
///
/// The floor is flat and open, centred on the origin; `location` is ignored.
pub fn build_scenario(registry: Rc<Registry>, scenario: &ScenarioDef) -> Result<World, String> {
    let start = TileCoord::new(scenario.player.start[0], scenario.player.start[1]);
    let world = dev_open_floor_world(scenario.half_extent, scenario.seed, start, player_config(scenario));
    populate(world, registry, scenario, TileCoord::new(0, 0))
}

/// [`build_scenario`] on a real map: everything is placed relative to the scenario's `location`
/// (the origin when it has none), on `map`, which must already hold the ground the player and the
/// monsters start on.
pub fn build_scenario_on(registry: Rc<Registry>, scenario: &ScenarioDef, map: CollisionMap) -> Result<World, String> {
    let origin = scenario_origin(scenario);
    let start = TileCoord::new(origin.x + scenario.player.start[0], origin.z + scenario.player.start[1]);
    let world = World::new(map, scenario.seed, start, player_config(scenario));
    populate(world, registry, scenario, origin)
}

/// The world tile a scenario's (0, 0) stands on when it is played on the real map.
pub fn scenario_origin(scenario: &ScenarioDef) -> TileCoord {
    scenario.location.map_or(TileCoord::new(0, 0), |[x, z]| TileCoord::new(x, z))
}

/// Where the player starts when `scenario` is played on the real map.
pub fn scenario_start(scenario: &ScenarioDef) -> TileCoord {
    let origin = scenario_origin(scenario);
    TileCoord::new(origin.x + scenario.player.start[0], origin.z + scenario.player.start[1])
}

fn player_config(scenario: &ScenarioDef) -> PlayerConfig {
    let setup = &scenario.player;
    let levels = setup.levels;
    PlayerConfig {
        max_health: levels.hitpoints,
        max_prayer_points: levels.prayer,
        is_running: setup.running,
        agility_level: levels.agility,
        run_energy_enabled: true,
        levels: CombatLevels::new(levels.attack, levels.strength, levels.defence, levels.ranged, levels.magic),
        ..PlayerConfig::default()
    }
}

/// Dresses the player and places the monsters, `origin` tiles from where the scenario's numbers say.
fn populate(mut world: World, registry: Rc<Registry>, scenario: &ScenarioDef, origin: TileCoord) -> Result<World, String> {
    let setup = &scenario.player;
    world.set_registry(registry);

    for item in &setup.equipment {
        world.player_equip(item)?;
    }
    for stack in &setup.inventory {
        let left_over = world.give_item(&stack.item, stack.quantity)?;
        if left_over > 0 {
            return Err(format!("the inventory is full: {left_over} {} do not fit", stack.item));
        }
    }
    if let Some(stance) = setup.stance {
        world.player_set_stance(stance)?;
    }
    if let Some(spell) = &setup.spell {
        world.player_select_spell(Some(spell))?;
    }
    for prayer in &setup.prayers {
        world.player_toggle_prayer(prayer)?;
    }
    for consumable in &setup.consumables {
        world.player_use_consumable(consumable)?;
        world.player.clear_consumable_delays();
    }
    world.player_set_on_task(setup.on_slayer_task);
    if setup.special {
        world.player_set_special(true)?;
    }

    let mut spawned = Vec::new();
    for spawn in &scenario.npcs {
        let at = TileCoord::new(origin.x + spawn.at[0], origin.z + spawn.at[1]);
        let id = world.spawn_npc_def(&spawn.npc, at)?;
        if let Some(hitpoints) = spawn.hitpoints {
            let health = world.entity_mut(id).expect("just spawned").mover.actor.health_mut();
            let wound = health.max() - hitpoints.clamp(1, health.max());
            health.damage(wound);
        }
        if spawn.hostile {
            world.set_hostile(id, true);
        }
        spawned.push(id);
    }
    if let Some(index) = setup.attack {
        let target = spawned.get(index).ok_or_else(|| format!("player.attack index {index} has no matching npc"))?;
        world.player_set_attack_target(*target);
    }
    Ok(world)
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::actors::EntityId;
    use crate::combat::{
        AttackDefinition, AttackPattern, AttackStyle, FixedRotationPattern, GroundDamage, GroundTarget,
        HazardAnchor, ProtectionPrayer,
    };
    use crate::encounters::{Phase, PhaseTrigger, TriggerAction, TriggerBinding};
    use crate::sim::config::{BossConfig, HealerConfig, NpcConfig, PassiveConfig};
    use crate::sim::entity::Entity;
    use crate::pathfinding::TargetSpec;
    use crate::sim::world::{SpawnBlueprint, PLAYER_ID};
    use crate::sim::{sounds, Combatant, WorldEvent};

    fn tc(x: i32, z: i32) -> TileCoord {
        TileCoord::new(x, z)
    }

    fn arena(player_start: TileCoord) -> World {
        dev_open_floor_world(15, 12345, player_start, PlayerConfig::default())
    }

    /// An arena whose player can soak a long fight (the default 10 HP dies to the first big hit,
    /// after which a boss - correctly - stops acting, as it only acts on a living target).
    fn tough_arena(player_start: TileCoord) -> World {
        dev_open_floor_world(15, 12345, player_start, PlayerConfig { max_health: 10_000, ..Default::default() })
    }

    fn run_ticks(world: &mut World, ticks: i32) {
        for _ in 0..ticks {
            world.run_tick();
        }
    }

    // ---- world plumbing ----

    #[test]
    fn the_encounter_clock_advances_once_per_tick_in_the_regen_phase() {
        let mut world = arena(tc(0, 0));

        run_ticks(&mut world, 3);

        assert_eq!(world.current_tick(), 3);
        assert_eq!(world.encounter.elapsed_ticks(), 3);
    }

    #[test]
    fn a_walk_request_is_pathed_in_the_player_queue_phase_and_run_two_tiles_per_tick() {
        let mut world = arena(tc(0, 0));
        world.player_request_walk_to(tc(6, 0), false);

        world.run_tick(); // queue phase paths it, movement phase takes the first run step
        assert_eq!(world.player.tile(), tc(2, 0));
        assert_eq!(world.player.mover.actor.previous_tile(), tc(0, 0));

        run_ticks(&mut world, 2);
        assert_eq!(world.player.tile(), tc(6, 0));
    }

    #[test]
    fn ctrl_click_forces_the_inverse_of_the_running_toggle_for_that_path_only() {
        let mut world = arena(tc(0, 0));
        world.player_request_walk_to(tc(6, 0), true); // running by default, so inverse = walk

        world.run_tick();

        assert_eq!(world.player.tile(), tc(1, 0));
    }

    #[test]
    fn walking_cancels_the_attack_target_and_attacking_cancels_a_queued_walk() {
        let mut world = arena(tc(0, 0));
        let dummy = world.spawn_passive("Dummy", tc(5, 5), PassiveConfig::default());

        world.player_set_attack_target(dummy);
        assert_eq!(world.player.attack_target(), Some(dummy));
        world.player_request_walk_to(tc(3, 3), false);
        assert_eq!(world.player.attack_target(), None);

        world.player_set_attack_target(dummy);
        assert!(world.player.pending_click.is_none());
    }

    #[test]
    fn entities_are_given_sequential_ids_after_the_player() {
        let mut world = arena(tc(0, 0));

        let a = world.spawn_passive("A", tc(5, 5), PassiveConfig::default());
        let b = world.spawn_passive("B", tc(6, 6), PassiveConfig::default());

        assert_eq!(a, EntityId(1));
        assert_eq!(b, EntityId(2));
        assert_ne!(a, PLAYER_ID);
    }

    // ---- player attacking ----

    #[test]
    fn the_player_chases_attacks_on_cooldown_and_eventually_kills_a_passive_npc() {
        let mut world = arena(tc(0, 0));
        let dummy = world.spawn_passive("Dummy", tc(4, 0), PassiveConfig { max_health: 3, ..Default::default() });
        world.player_set_attack_target(dummy);

        run_ticks(&mut world, 200);

        assert!(world.entity(dummy).unwrap().is_dead(), "player should have killed a 3 HP dummy in 200 ticks");
        assert!(world.log().iter().any(|l| l.starts_with("Player hits for")));
        // Once the target is dead the player stops attacking.
        run_ticks(&mut world, 1);
        assert_eq!(world.player.attack_target(), None);
    }

    // ---- plain NPC ----

    #[test]
    fn a_melee_npc_chases_the_player_telegraphs_and_damages_them() {
        let mut world = arena(tc(0, 0));
        let npc = world.spawn_npc("Goblin", tc(6, 0), NpcConfig::default());

        run_ticks(&mut world, 40);

        let goblin = world.entity(npc).unwrap();
        assert!(goblin.tile().chebyshev_distance_to(world.player.tile()) <= 1, "goblin should have closed to melee range");
        assert!(world.log().iter().any(|l| l.contains("Goblin uses Melee on the player")));
    }

    #[test]
    fn a_correct_protect_from_melee_blocks_a_melee_npcs_hits() {
        let mut world = arena(tc(0, 0));
        world.spawn_npc("Goblin", tc(1, 0), NpcConfig::default());
        world.player.set_active_prayer(ProtectionPrayer::Melee);
        let starting_hp = world.player.mover.actor.health().current();

        run_ticks(&mut world, 60);

        assert_eq!(world.player.mover.actor.health().current(), starting_hp);
        assert!(world.log().iter().any(|l| l.contains("Goblin uses Melee on the player for 0")));
    }

    #[test]
    fn an_npc_with_no_player_target_never_attacks_or_moves() {
        let mut world = arena(tc(0, 0));
        let npc = world.spawn_npc("Idle", tc(5, 5), NpcConfig { targets_player: false, ..Default::default() });

        run_ticks(&mut world, 20);

        assert_eq!(world.entity(npc).unwrap().tile(), tc(5, 5));
        assert!(world.log().is_empty());
    }

    // ---- generic boss / trigger / spawn machinery (a stand-in boss, not Jad or Purutu) ----

    /// A stationary boss that fires a ranged attack and whose HP-threshold trigger wakes one healer.
    fn boss_world() -> (World, EntityId) {
        let mut world = tough_arena(tc(0, 0));
        let boss = world.spawn_boss("Boss", tc(0, 8), BossConfig::default());

        let slam = AttackDefinition::targeted("Slam", Some(15), 2, AttackStyle::Ranged)
            .shared();
        let pattern: Rc<dyn AttackPattern> = Rc::new(FixedRotationPattern::new(vec![slam]));
        let phase = Phase::new("Main", pattern, None);
        let trigger = TriggerBinding::new(PhaseTrigger::hp_threshold(0.5), TriggerAction::spawn_entities("healers"));
        world.configure_boss(boss, vec![phase.clone()], phase, vec![trigger]);

        let healer: SpawnBlueprint = Box::new(move |id, map| {
            Entity::new_healer(id, "Healer", TileCoord::new(0, 6), Some(boss), &HealerConfig::default(), map)
        });
        world.register_spawn_group("healers", vec![healer]);
        (world, boss)
    }

    #[test]
    fn a_boss_telegraphs_then_attacks_the_player_from_range() {
        let (mut world, _) = boss_world();

        run_ticks(&mut world, 12);

        assert!(world.log().iter().any(|l| l.starts_with("Boss uses Slam")), "{:?}", world.log());
    }

    #[test]
    fn spawn_groups_stay_dormant_until_the_hp_threshold_trigger_fires() {
        let (mut world, boss) = boss_world();
        run_ticks(&mut world, 5);
        assert_eq!(world.entities().count(), 1, "only the boss exists before the trigger fires");

        // Drop the boss to exactly half health (the threshold is "<=").
        world.entity_mut(boss).unwrap().mover.actor.health_mut().damage(125);
        run_ticks(&mut world, 1);

        assert_eq!(world.entities().count(), 2);
        assert!(world.entities().any(|e| e.name == "Healer"));
    }

    #[test]
    fn a_spawned_healer_walks_to_the_boss_and_heals_it() {
        let (mut world, boss) = boss_world();
        world.entity_mut(boss).unwrap().mover.actor.health_mut().damage(150);
        let hp_before_healing = world.entity(boss).unwrap().health().current();

        run_ticks(&mut world, 30);

        assert!(world.entity(boss).unwrap().health().current() > hp_before_healing);
        assert!(world.log().iter().any(|l| l.contains("heals Boss")));
    }

    #[test]
    fn a_when_entity_died_trigger_fires_on_the_tick_of_the_killing_blow() {
        let mut world = tough_arena(tc(0, 0));
        let boss = world.spawn_boss("Boss", tc(0, 8), BossConfig { max_health: 10, ..Default::default() });
        let pattern: Rc<dyn AttackPattern> = Rc::new(FixedRotationPattern::new(vec![AttackDefinition::targeted(
            "Slam",
            Some(15),
            2,
            AttackStyle::Ranged,
        )
        .shared()]));
        let phase = Phase::new("Main", pattern, None);
        let trigger = TriggerBinding::new(PhaseTrigger::when_entity_died(), TriggerAction::set_flag("bossDead", true));
        world.configure_boss(boss, vec![phase.clone()], phase, vec![trigger]);

        world.entity_mut(boss).unwrap().mover.actor.health_mut().damage(10);
        run_ticks(&mut world, 1);

        assert!(world.encounter.flag("bossDead"));
    }

    /// A boss at (0, 8), fighting a player at (0, 0), whose only attack is a ground-targeted rock
    /// fall that lands when its 3-tick telegraph ends. The boss has 10 hitpoints so a test can kill it.
    fn rock_fall_world(target: GroundTarget) -> (World, EntityId) {
        rock_world_with(AttackDefinition::ground("Rock", Some(15), 3, target, GroundDamage::Fixed(30)))
    }

    fn rock_world_with(rock: AttackDefinition) -> (World, EntityId) {
        let mut world = tough_arena(tc(0, 0));
        let boss = world.spawn_boss("Boss", tc(0, 8), BossConfig { max_health: 10, ..Default::default() });
        let pattern: Rc<dyn AttackPattern> = Rc::new(FixedRotationPattern::new(vec![rock.shared()]));
        let phase = Phase::new("Main", pattern, None);
        world.configure_boss(boss, vec![phase.clone()], phase, vec![]);
        (world, boss)
    }

    fn rock_hits(world: &World) -> usize {
        world.log().iter().filter(|l| l.starts_with("Boss uses Rock on the player")).count()
    }

    #[test]
    fn a_ground_attack_hits_whoever_stands_on_its_tiles_when_its_telegraph_ends() {
        let (mut world, _) = rock_fall_world(GroundTarget::square(HazardAnchor::Target, 0));

        run_ticks(&mut world, 3);
        assert_eq!(rock_hits(&world), 0, "it lands when the telegraph ends, not before");
        run_ticks(&mut world, 1);

        assert_eq!(rock_hits(&world), 1, "{:?}", world.log());
    }

    #[test]
    fn a_ground_attack_is_gone_once_it_has_landed_and_does_not_linger() {
        let (mut world, _) = rock_fall_world(GroundTarget::square(HazardAnchor::Target, 0));

        run_ticks(&mut world, 4);
        let hits = rock_hits(&world);
        // the boss's next attack starts a fresh telegraph; nothing from the first one remains
        run_ticks(&mut world, 2);

        assert_eq!(hits, 1);
        assert_eq!(rock_hits(&world), 1, "no second landing before the next telegraph ends");
    }

    #[test]
    fn a_ground_attack_ends_with_its_attacker_if_it_dies_during_the_telegraph() {
        let (mut world, boss) = rock_fall_world(GroundTarget::square(HazardAnchor::Target, 0));

        run_ticks(&mut world, 2);
        world.entity_mut(boss).unwrap().mover.actor.health_mut().damage(10);
        run_ticks(&mut world, 4);

        assert_eq!(rock_hits(&world), 0, "{:?}", world.log());
    }

    #[test]
    fn a_ground_attack_aimed_elsewhere_misses_a_player_who_is_not_on_its_tiles() {
        // aimed at the boss's own tile, far from the player
        let (mut world, _) = rock_fall_world(GroundTarget::square(HazardAnchor::SelfTile, 0));

        run_ticks(&mut world, 6);

        assert!(world.log().iter().any(|l| l.starts_with("Boss lands Rock over 1 tile")), "{:?}", world.log());
        assert_eq!(rock_hits(&world), 0, "{:?}", world.log());
    }

    #[test]
    fn a_ground_attack_never_misses_and_deals_its_plain_damage() {
        let (mut world, _) = rock_fall_world(GroundTarget::square(HazardAnchor::Target, 0));
        let before = world.player.mover.actor.health().current();

        run_ticks(&mut world, 4);

        assert_eq!(rock_hits(&world), 1);
        assert_eq!(before - world.player.mover.actor.health().current(), 30);
    }

    #[test]
    fn the_one_reducing_prayer_cuts_a_ground_attack_without_negating_it() {
        let target = || GroundTarget::square(HazardAnchor::Target, 0);
        let reducible = || {
            AttackDefinition::ground("Rock", Some(15), 3, target(), GroundDamage::Fixed(30))
                .with_prayer_reduction(ProtectionPrayer::Melee, 0.5)
        };
        let lost = |prayer: Option<ProtectionPrayer>| {
            let (mut world, _) = rock_world_with(reducible());
            if let Some(prayer) = prayer {
                world.player.set_active_prayer(prayer);
            }
            let before = world.player.mover.actor.health().current();
            run_ticks(&mut world, 4);
            before - world.player.mover.actor.health().current()
        };

        assert_eq!(lost(None), 30);
        assert_eq!(lost(Some(ProtectionPrayer::Melee)), 15, "the right prayer halves it");
        assert_eq!(lost(Some(ProtectionPrayer::Magic)), 30, "the wrong one does nothing");
    }

    #[test]
    fn a_ground_attack_covers_every_tile_of_its_area() {
        // a single tile one east of the player misses, the 3x3 around the player's tile hits
        let (mut beside, _) = rock_fall_world(GroundTarget::offsets(HazardAnchor::Target, vec![(1, 0)]));
        let (mut around, _) = rock_fall_world(GroundTarget::square(HazardAnchor::Target, 1));

        run_ticks(&mut beside, 4);
        run_ticks(&mut around, 4);

        assert_eq!(rock_hits(&beside), 0);
        assert_eq!(rock_hits(&around), 1);
    }

    // ---- data-defined monsters ----

    /// A sturdy player and the bundled monster definitions.
    fn data_arena() -> World {
        let mut world = tough_arena(tc(0, 0));
        world.set_registry(Rc::new(crate::data::Registry::builtin()));
        world
    }

    #[test]
    fn a_monster_that_can_already_hit_the_player_holds_still_and_one_that_cannot_walks_up() {
        let mut world = data_arena();
        // Jad's ranged and magic reach 15 tiles: 8 away he has no reason to move
        let jad = world.spawn_npc_def("tztok_jad", tc(-2, 8)).unwrap();
        world.set_hostile(jad, true);
        // a man only punches, so he has to come to the player
        let man = world.spawn_npc_def("man", tc(6, 0)).unwrap();
        world.set_hostile(man, true);

        run_ticks(&mut world, 8);

        assert_eq!(world.entity(jad).unwrap().tile(), tc(-2, 8), "Jad walked although he could hit the player");
        assert!(world.entity(man).unwrap().tile().chebyshev_distance_to(tc(0, 0)) <= 1, "the man should have come to melee range");
    }

    #[test]
    fn a_telegraphed_attack_is_announced_with_its_animation_as_its_telegraph_begins() {
        let mut world = data_arena();
        let jad = world.spawn_npc_def("tztok_jad", tc(-2, 8)).unwrap();
        world.set_hostile(jad, true);

        let mut announced = None;
        let mut resolved = None;
        for tick in 1..=12 {
            world.run_tick();
            for event in world.take_events() {
                if let WorldEvent::Attack { attacker: Combatant::Npc(id), animation, .. } = event {
                    if id == jad && announced.is_none() {
                        announced = Some((tick, animation));
                    }
                }
            }
            if resolved.is_none() && world.log().iter().any(|line| line.starts_with("TzTok-Jad uses")) {
                resolved = Some(tick);
            }
        }

        let (announced_on, animation) = announced.expect("Jad never attacked");
        assert!(matches!(animation, Some(2652) | Some(2656)), "a ranged or magic attack plays its own animation, got {animation:?}");
        assert_eq!(resolved, Some(announced_on + 3), "the roll comes 3 ticks after the wind-up starts");
    }

    /// Jad, hostile, with the player far enough away that his healers are left alone.
    fn jad_world() -> (World, EntityId) {
        let mut world = data_arena();
        let jad = world.spawn_npc_def("tztok_jad", tc(-2, 8)).unwrap();
        world.set_hostile(jad, true);
        (world, jad)
    }

    fn healers_of(world: &World) -> Vec<EntityId> {
        world.entities().filter(|e| e.name == "Yt-HurKot").map(|e| e.id).collect()
    }

    fn hurt(world: &mut World, id: EntityId, amount: i32) {
        world.entity_mut(id).unwrap().mover.actor.health_mut().damage(amount);
    }

    #[test]
    fn jad_calls_four_healers_below_150_who_walk_up_to_him_and_heal_him() {
        let (mut world, jad) = jad_world();
        run_ticks(&mut world, 2);
        assert!(healers_of(&world).is_empty(), "no healers while Jad is above 150");

        hurt(&mut world, jad, 110); // 140 left
        run_ticks(&mut world, 1);
        let healers = healers_of(&world);
        assert_eq!(healers.len(), 4);
        assert!(healers.iter().all(|h| world.entity(*h).unwrap().leader == Some(jad)));

        run_ticks(&mut world, 20);
        let beside = TargetSpec::adjacent(tc(-2, 8), 5, 5);
        let tiles: Vec<TileCoord> = healers.iter().map(|h| world.entity(*h).unwrap().tile()).collect();
        assert!(tiles.iter().all(|t| beside.is_satisfied_by(*t)), "healers stand next to Jad, not in him: {tiles:?}");
        for (i, a) in tiles.iter().enumerate() {
            assert!(!tiles[i + 1..].contains(a), "two healers on one tile: {tiles:?}");
        }
        assert!(world.entity(jad).unwrap().health().current() > 140, "and have healed him");
        assert!(world.log().iter().any(|l| l.contains("Yt-HurKot heals TzTok-Jad for 5")));
    }

    #[test]
    fn a_healer_heals_five_every_four_ticks() {
        let (mut world, jad) = jad_world();
        hurt(&mut world, jad, 200); // 50 left: far from full for the whole test
        run_ticks(&mut world, 1);
        let healer = healers_of(&world)[0];
        // keep the other three out of it
        for other in healers_of(&world).into_iter().skip(1) {
            hurt(&mut world, other, 60);
        }
        run_ticks(&mut world, 12); // time to walk up
        world.take_events();

        let before = world.entity(jad).unwrap().health().current();
        let mut heals = 0;
        for _ in 0..8 {
            world.run_tick();
            heals += world
                .take_events()
                .iter()
                .filter(|e| matches!(e, WorldEvent::Heal { healer: Combatant::Npc(h), .. } if *h == healer))
                .count();
        }
        assert_eq!(heals, 2, "two heals in eight ticks");
        assert_eq!(world.entity(jad).unwrap().health().current(), before + 10);
    }

    #[test]
    fn a_healer_the_player_attacks_fights_back_instead_of_healing() {
        // Jad right by the player, so a healer is within its leash of them
        let mut world = data_arena();
        let jad = world.spawn_npc_def("tztok_jad", tc(-2, 2)).unwrap();
        world.set_hostile(jad, true);
        hurt(&mut world, jad, 200);
        run_ticks(&mut world, 1);
        // the healer on the player's side of Jad: one behind him has to find a way round first
        let healers = healers_of(&world);
        let healer = *healers
            .iter()
            .min_by_key(|h| world.entity(**h).unwrap().tile().chebyshev_distance_to(tc(0, 0)))
            .unwrap();
        for other in healers.iter().filter(|h| **h != healer) {
            hurt(&mut world, *other, 60);
        }
        world.set_hostile(healer, true); // what the player's first hit does

        let before = world.entity(jad).unwrap().health().current();
        run_ticks(&mut world, 16);

        assert_eq!(world.entity(jad).unwrap().health().current(), before, "a distracted healer does not heal");
        assert!(world.log().iter().any(|l| l.starts_with("Yt-HurKot uses Claw on the player")), "{:?}", world.log());
    }

    #[test]
    fn dead_healers_come_again_only_once_jad_has_been_healed_to_full() {
        let (mut world, jad) = jad_world();
        hurt(&mut world, jad, 110);
        run_ticks(&mut world, 1);
        let first = healers_of(&world);
        for healer in &first {
            hurt(&mut world, *healer, 60);
        }
        run_ticks(&mut world, 3);
        assert_eq!(world.entities().filter(|e| e.name == "Yt-HurKot" && !e.is_dead()).count(), 0);

        // still below 150, but never healed to full: no new healers
        hurt(&mut world, jad, 10);
        run_ticks(&mut world, 3);
        assert_eq!(healers_of(&world).len(), 4);

        // healed to full, then below 150 again: the four dead ones are replaced
        let missing = world.entity(jad).unwrap().health().max() - world.entity(jad).unwrap().health().current();
        world.entity_mut(jad).unwrap().mover.actor.health_mut().heal(missing);
        run_ticks(&mut world, 1);
        hurt(&mut world, jad, 110);
        run_ticks(&mut world, 1);
        assert_eq!(world.entities().filter(|e| e.name == "Yt-HurKot" && !e.is_dead()).count(), 4);
    }

    #[test]
    fn jads_ranged_and_magic_name_their_graphics() {
        let (mut world, _) = jad_world();
        let mut graphics = Vec::new();
        let mut projectiles = Vec::new();
        for _ in 0..60 {
            world.run_tick();
            for event in world.take_events() {
                match event {
                    WorldEvent::Graphic { graphic, delay_ticks, .. } => graphics.push((graphic, delay_ticks)),
                    WorldEvent::Projectile { graphic, released, ticks, .. } => projectiles.push((graphic, released, ticks)),
                    _ => {}
                }
            }
        }
        // over 60 ticks Jad uses both; the rocks start at once, the splash as the fireball lands
        // after its flight, which is the same number of ticks as the fireball is drawn in the air
        assert!(graphics.contains(&(451, 0)), "{graphics:?}");
        let (_, _, flight) = *projectiles.iter().find(|(graphic, ..)| *graphic == 448).expect("a fireball flew");
        assert!(projectiles.contains(&(448, true, flight)), "{projectiles:?}");
        assert!(graphics.contains(&(157, flight)), "{graphics:?}");
    }

    #[test]
    fn a_scenario_on_the_real_map_is_placed_at_its_location_with_its_inventory() {
        let scenario = ScenarioDef::from_json(
            "t",
            r#"{
                "location": [100, 200],
                "player": { "start": [1, 2], "inventory": [ { "item": "shark", "quantity": 3 }, { "item": "prayer_potion_4" } ] },
                "npcs": [ { "npc": "man", "at": [3, 0] } ]
            }"#,
        )
        .unwrap();
        let map = dev_open_floor_world_at(tc(100, 200), 15, 1, tc(100, 200), PlayerConfig::default()).map;

        let world = build_scenario_on(Rc::new(crate::data::Registry::builtin()), &scenario, map).unwrap();

        assert_eq!(world.player.tile(), tc(101, 202));
        assert_eq!(scenario_start(&scenario), tc(101, 202));
        assert_eq!(world.entities().next().unwrap().tile(), tc(103, 200));
        assert_eq!(world.player.inventory.count("shark"), 3);
        assert_eq!(world.player.inventory.count("prayer_potion_4"), 1);
    }

    #[test]
    fn the_healer_scenario_starts_jad_wounded_so_his_healers_come_at_once() {
        let scenario = crate::data::load_scenario("jad_healers").unwrap();
        let mut world = build_scenario(Rc::new(crate::data::Registry::builtin()), &scenario).unwrap();
        assert_eq!(world.entities().next().unwrap().health().current(), 140);

        world.run_tick();

        assert_eq!(healers_of(&world).len(), 4);
    }

    #[test]
    fn each_drink_of_a_potion_leaves_the_next_dose_down_and_the_last_an_empty_vial() {
        let mut world = data_arena();
        world.give_item("prayer_potion_4", 1).unwrap();

        let mut seen = vec![world.player.inventory.get(0).unwrap().item.clone()];
        for _ in 0..4 {
            world.player_inventory_option(0, "Drink");
            world.player.clear_consumable_delays();
            seen.push(world.player.inventory.get(0).unwrap().item.clone());
        }

        assert_eq!(seen, ["prayer_potion_4", "prayer_potion_3", "prayer_potion_2", "prayer_potion_1", "vial"]);
        assert_eq!(world.item_name("prayer_potion_3"), "Prayer potion(3)", "the game's own name, which picks its picture");
        assert!(world.take_messages().iter().any(|m| m == "You drink some of your prayer potion. You have finished your potion."));

        // the vial is carried, not worn
        world.player_inventory_option(0, "Wield");
        assert_eq!(world.player.inventory.get(0).unwrap().item, "vial");
    }

    // ---- sounds ----

    /// The sound effects heard this tick, with who they come from.
    fn sounds_of(events: &[WorldEvent]) -> Vec<(u32, Combatant)> {
        events
            .iter()
            .filter_map(|event| match *event {
                WorldEvent::Sound { sound, from, .. } => Some((sound, from)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn prayers_and_potions_are_heard() {
        let mut world = data_arena();
        world.player_toggle_prayer("protect_from_melee").unwrap();
        world.player_toggle_prayer("protect_from_melee").unwrap();
        world.give_item("prayer_potion_4", 1).unwrap();
        world.player_inventory_option(0, "Drink");

        let heard: Vec<u32> = sounds_of(&world.take_events()).into_iter().map(|(sound, _)| sound).collect();
        assert_eq!(heard, [2676, sounds::PRAYER_OFF, sounds::DRINK], "protect_from_melee on, off, then a drink");
    }

    #[test]
    fn running_out_of_prayer_points_is_heard() {
        let mut world = data_arena();
        world.player_toggle_prayer("piety").unwrap();
        world.take_events();
        let mut heard = Vec::new();
        for _ in 0..2000 {
            world.run_tick();
            heard.extend(sounds_of(&world.take_events()));
            if world.player.active_prayers.is_empty() {
                break;
            }
        }
        assert!(world.player.active_prayers.is_empty(), "piety drains the points in time");
        assert!(heard.contains(&(sounds::PRAYER_DRAINED, Combatant::Player)));
    }

    #[test]
    fn jads_stomp_is_heard_on_the_roll_tick() {
        let (mut world, jad) = jad_world();
        let (mut wound_up, mut heard) = (None, None);
        for tick in 1..=60 {
            world.run_tick();
            for event in world.take_events() {
                match event {
                    WorldEvent::Attack { animation: Some(2652), .. } if wound_up.is_none() => wound_up = Some(tick),
                    WorldEvent::Sound { sound: 163, from, .. } if heard.is_none() => {
                        assert_eq!(from, Combatant::Npc(jad));
                        heard = Some(tick);
                    }
                    _ => {}
                }
            }
            if heard.is_some() {
                break;
            }
        }
        assert_eq!(heard.zip(wound_up).map(|(h, w)| h - w), Some(3), "heard {heard:?}, wind-up {wound_up:?}");
    }

    #[test]
    fn jad_breathes_in_as_his_magic_wind_up_begins() {
        let (mut world, jad) = jad_world();
        for _ in 0..40 {
            world.run_tick();
            let events = world.take_events();
            let magic = events.iter().any(|e| matches!(e, WorldEvent::Attack { attacker: Combatant::Npc(id), animation: Some(2656), .. } if *id == jad));
            if magic {
                assert!(sounds_of(&events).contains(&(159, Combatant::Npc(jad))), "firebreath with the rear-up");
                return;
            }
        }
        panic!("Jad never used magic in 40 ticks");
    }

    #[test]
    fn a_whip_is_heard_lashing_and_the_zombie_being_hit() {
        let mut world = data_arena();
        world.player_equip("abyssal_whip").unwrap();
        let zombie = world.spawn_npc_def("zombie", tc(1, 0)).unwrap();
        world.player_set_attack_target(zombie);
        let mut heard = Vec::new();
        for _ in 0..3 {
            world.run_tick();
            heard.extend(sounds_of(&world.take_events()));
        }
        assert!(heard.contains(&(2720, Combatant::Player)), "{heard:?}");
        assert!(heard.contains(&(923, Combatant::Npc(zombie))), "zombie_hit as the lash lands: {heard:?}");
    }

    #[test]
    fn an_attack_with_a_resolve_sound_is_heard_on_the_roll_tick_not_the_wind_up() {
        // a 3-tick telegraphed ranged attack (Jad's stomp shape) whose sound is the resolve one
        let mut registry = crate::data::Registry::builtin();
        let mut jad = registry.npc("tztok_jad").unwrap().clone();
        jad.sounds.attack = None;
        for attack in &mut jad.attacks {
            attack.sound = None;
            attack.resolve_sound = None;
        }
        jad.attacks[1].resolve_sound = Some(777);
        registry.insert_npc_for_test(jad);
        let mut world = tough_arena(tc(0, 0));
        world.set_registry(Rc::new(registry));
        let jad = world.spawn_npc_def("tztok_jad", tc(-2, 8)).unwrap();
        world.set_hostile(jad, true);

        let (mut wound_up, mut heard) = (None, None);
        for tick in 1..=60 {
            world.run_tick();
            for event in world.take_events() {
                match event {
                    WorldEvent::Attack { animation: Some(2652), .. } if wound_up.is_none() => wound_up = Some(tick),
                    WorldEvent::Sound { sound: 777, .. } if heard.is_none() => heard = Some(tick),
                    _ => {}
                }
            }
            if heard.is_some() {
                break;
            }
        }
        assert_eq!(heard.zip(wound_up).map(|(h, w)| h - w), Some(3), "heard {heard:?}, wind-up {wound_up:?}");
    }

    #[test]
    fn every_sound_the_data_names_is_a_synth_sound_in_the_cache() {
        let Ok(cache) = osrs_cache::Cache::open_default() else { return };
        let index = cache.index(osrs_cache::sound::ARCHIVE).unwrap();
        let registry = crate::data::Registry::builtin();
        let named = registry.named_sounds();
        assert!(named.len() > 80, "{} distinct sounds", named.len());
        for id in named {
            let effect = osrs_cache::sound::load_from(&cache, &index, id).unwrap_or_else(|e| panic!("sound {id}: {e}"));
            assert!(effect.duration_ms() > 0, "sound {id} is silent");
        }
    }

    #[test]
    fn the_bundled_jad_scenario_builds() {
        let scenario = crate::data::load_scenario("jad").unwrap();
        let world = build_scenario(Rc::new(crate::data::Registry::builtin()), &scenario).unwrap();
        assert_eq!(world.entities().count(), 1);
        assert!(world.player.active_prayers.iter().any(|p| p == "protect_from_magic"));
    }

    // ---- OSRS's "dumb" NPC pathing and the traps it allows ----

    /// A sturdy player at `start` on a hand-drawn map (see `collision::test_grid`), with the
    /// bundled monsters.
    fn drawn_world(ascii: &str, start: TileCoord) -> World {
        let map = crate::collision::test_grid::parse(ascii, 100);
        let mut world = World::new(map, 12345, start, PlayerConfig { max_health: 10_000, ..Default::default() });
        world.set_registry(Rc::new(crate::data::Registry::builtin()));
        world
    }

    fn hostile(world: &mut World, npc: &str, at: TileCoord) -> EntityId {
        let id = world.spawn_npc_def(npc, at).unwrap();
        world.set_hostile(id, true);
        id
    }

    /// Who threw an attack at the player over `ticks` ticks.
    fn attackers_over(world: &mut World, ticks: i32) -> std::collections::BTreeSet<EntityId> {
        let mut attackers = std::collections::BTreeSet::new();
        for _ in 0..ticks {
            world.run_tick();
            for event in world.take_events() {
                if let WorldEvent::Attack { attacker: Combatant::Npc(id), target: Combatant::Player, .. } = event {
                    attackers.insert(id);
                }
            }
        }
        attackers
    }

    #[test]
    fn a_melee_monster_diagonal_to_the_player_with_its_west_step_blocked_is_stuck_for_good() {
        // The corner safespot. The zombie walks in diagonally and ends up north-east of the
        // player, touching them only at a corner. Diagonally adjacent, it tries its east/west
        // step and nothing else: west is a block, and although the step south (which would
        // put it beside the player) is open, it never tries it. Melee needs a side, not a
        // corner, so it never attacks.
        let mut world = drawn_world(
            "\
......
......
......
......
#.....
......",
            tc(0, 0),
        );
        let zombie = hostile(&mut world, "zombie", tc(4, 4));

        let attackers = attackers_over(&mut world, 30);

        assert_eq!(world.entity(zombie).unwrap().tile(), tc(1, 1), "stuck on the corner, diagonal to the player");
        assert!(attackers.is_empty(), "it never gets a swing in");
    }

    #[test]
    fn in_a_corner_only_two_melee_monsters_reach_the_player_and_the_rest_queue_behind() {
        // The corner trap. In a corner the player has only two sides open, north and east. The
        // first two monsters take them; the third walks up diagonally and is stuck behind them
        // (NPCs block one another and never path around), so it never swings.
        let mut world = drawn_world(
            "\
........
........
........
........
........
........
........
........",
            tc(0, 0),
        );
        let goblins = [hostile(&mut world, "goblin", tc(5, 4)), hostile(&mut world, "goblin", tc(4, 5)), hostile(&mut world, "goblin", tc(5, 5))];

        let attackers = attackers_over(&mut world, 40);

        let sides = [tc(1, 0), tc(0, 1)];
        let at_a_side = goblins.iter().filter(|g| sides.contains(&world.entity(**g).unwrap().tile())).count();
        assert_eq!(at_a_side, 2);
        assert_eq!(attackers.len(), 2, "only the two at the sides attack: {attackers:?}");
        let trapped = goblins.iter().find(|g| !attackers.contains(*g)).unwrap();
        assert_eq!(world.entity(*trapped).unwrap().tile(), tc(1, 1), "the third waits on the corner tile");
    }

    // ---- determinism ----

    #[test]
    fn the_same_seed_replays_an_identical_fight_and_a_different_seed_diverges() {
        let trace = |seed: i32| {
            let mut world = dev_open_floor_world(15, seed, tc(0, 0), PlayerConfig { max_health: 10_000, ..Default::default() });
            world.spawn_npc("Goblin", tc(6, 0), NpcConfig::default());
            (0..150)
                .map(|_| {
                    world.run_tick();
                    world.snapshot()
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(trace(12345), trace(12345));
        assert_ne!(trace(12345), trace(54321));
    }
}
