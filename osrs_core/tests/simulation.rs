//! End-to-end checks of the data-driven rules: a real `World`, the bundled dataset, and the
//! commands a client would send. Each test names the piece of OSRS_COMBAT_REFERENCE.md it pins.

use std::rc::Rc;

use osrs_core::actors::EntityId;
use osrs_core::combat::{analyze_attack, AttackStyle, ProtectionPrayer};
use osrs_core::data::{EquipSlot, Registry};
use osrs_core::sim::scenarios::dev_open_floor_world;
use osrs_core::sim::{CombatLevels, PlayerConfig, World, PLAYER_ID};
use osrs_core::tiles::TileCoord;

fn config(levels: CombatLevels) -> PlayerConfig {
    PlayerConfig { max_health: 99, levels, ..PlayerConfig::default() }
}

fn maxed() -> CombatLevels {
    CombatLevels::new(99, 99, 99, 99, 99)
}

fn world_with(config: PlayerConfig) -> World {
    let mut world = dev_open_floor_world(20, 4242, TileCoord::new(0, 0), config);
    world.set_registry(Rc::new(Registry::builtin()));
    world
}

fn run(world: &mut World, ticks: i32) {
    for _ in 0..ticks {
        world.run_tick();
    }
}

fn hp(world: &World, id: EntityId) -> i32 {
    world.entity(id).unwrap().health().current()
}

fn player_hits(world: &World) -> usize {
    world.log().iter().filter(|line| line.starts_with("Player hits for")).count()
}

fn dummy_next_to_player(world: &mut World) -> EntityId {
    world.spawn_npc_def("combat_dummy", TileCoord::new(1, 0)).unwrap()
}

// ---- reference section 7: attack speed ----

#[test]
fn a_four_tick_weapon_swings_every_four_ticks() {
    let mut world = world_with(config(maxed()));
    world.player_equip("dragon_scimitar").unwrap();
    let dummy = dummy_next_to_player(&mut world);
    world.player_set_attack_target(dummy);

    run(&mut world, 17); // swings on ticks 1, 5, 9, 13, 17

    assert_eq!(player_hits(&world), 5);
}

#[test]
fn a_slower_weapon_and_the_rapid_stance_change_the_cadence() {
    let mut world = world_with(config(maxed()));
    world.player_equip("armadyl_godsword").unwrap(); // speed 6
    let dummy = dummy_next_to_player(&mut world);
    world.player_set_attack_target(dummy);
    run(&mut world, 13); // ticks 1, 7, 13
    assert_eq!(player_hits(&world), 3);

    let mut world = world_with(config(maxed()));
    world.player.set_special_enabled(&Registry::builtin(), false).unwrap();
    world.player_equip("rune_crossbow").unwrap();
    world.player_equip("rune_bolts").unwrap();
    world.player_set_stance(1).unwrap(); // Rapid: 6 - 1 = 5 ticks
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(3, 0)).unwrap();
    world.player_set_attack_target(dummy);
    run(&mut world, 11); // ticks 1, 6, 11
    assert_eq!(player_hits(&world), 3);
}

// ---- reference section 5: accuracy and max hit through the whole pipeline ----

#[test]
fn damage_dealt_never_exceeds_the_analysed_max_hit_and_the_dummy_loses_exactly_that_hp() {
    let mut world = world_with(config(maxed()));
    world.player_equip("abyssal_whip").unwrap();
    world.player_toggle_prayer("piety").unwrap();
    let dummy = dummy_next_to_player(&mut world);
    world.player_set_attack_target(dummy);

    let plan = world.player.plan_attack(world.registry(), &world.entity(dummy).unwrap().view().stats).unwrap();
    let analysis = analyze_attack(
        plan.definition.expect_targeted(),
        &world.player.mover.actor.stats(),
        &world.entity(dummy).unwrap().view().stats,
        true,
    );
    // 99 Strength, Piety x1.23 = 121, +0 stance (Flick is Accurate), +8 = 129; (129 * 146 + 320) / 640 = 29
    assert_eq!(analysis.max_hit, 29);

    let before = hp(&world, dummy);
    run(&mut world, 80);
    let dealt = before - hp(&world, dummy);
    assert!(dealt > 0, "20 swings at ~70% should land something");
    assert!(dealt <= 20 * analysis.max_hit);
}

#[test]
fn a_different_stance_changes_the_effective_level_it_adds_to() {
    let mut world = world_with(config(maxed()));
    world.player_equip("dragon_scimitar").unwrap();

    world.player_set_stance(0).unwrap(); // Chop: +3 Attack
    let chop = world.player.mover.actor.stats();
    world.player_set_stance(1).unwrap(); // Slash: +3 Strength
    let slash = world.player.mover.actor.stats();
    world.player_set_stance(2).unwrap(); // Lunge: controlled, +1 to all three
    let lunge = world.player.mover.actor.stats();

    assert_eq!(chop.effective_accuracy_level(AttackStyle::Melee), 99 + 3 + 8);
    assert_eq!(slash.effective_accuracy_level(AttackStyle::Melee), 99 + 8);
    assert_eq!(slash.effective_damage_level(AttackStyle::Melee), 99 + 3 + 8);
    assert_eq!(chop.effective_damage_level(AttackStyle::Melee), 99 + 8);
    assert_eq!(lunge.effective_accuracy_level(AttackStyle::Melee), 99 + 1 + 8);
    assert_eq!(lunge.effective_damage_level(AttackStyle::Melee), 99 + 1 + 8);
    assert_eq!(lunge.effective_defence_level(), 99 + 1 + 8);
}

// ---- reference section 3: ranged and magic ----

#[test]
fn a_crossbow_needs_matching_ammo_then_hits_from_range_after_a_projectile_delay() {
    let mut world = world_with(config(maxed()));
    world.player_equip("rune_crossbow").unwrap();
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(5, 0)).unwrap();
    world.player_set_attack_target(dummy);

    run(&mut world, 2);
    assert!(world.log().iter().any(|line| line.contains("needs bolt ammunition")), "{:?}", world.log());
    assert_eq!(world.player.attack_target(), None);

    world.player_equip("dragon_arrow").unwrap(); // wrong family
    world.player_set_attack_target(dummy);
    run(&mut world, 1);
    assert_eq!(world.player.attack_target(), None);

    world.player_equip("rune_bolts").unwrap();
    world.player_set_attack_target(dummy);
    run(&mut world, 1);
    assert_eq!(player_hits(&world), 1, "in range 7, it fires immediately without walking");
    assert_eq!(world.player.tile(), TileCoord::new(0, 0));
}

#[test]
fn a_ranged_attacker_walks_into_range_first() {
    let mut world = world_with(config(maxed()));
    world.player_equip("rune_crossbow").unwrap();
    world.player_equip("rune_bolts").unwrap();
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(14, 0)).unwrap();
    world.player_set_attack_target(dummy);

    run(&mut world, 8);

    assert!(world.player.tile().x >= 7, "walked toward the target: {:?}", world.player.tile());
    assert!(player_hits(&world) >= 1);
}

#[test]
fn spell_max_hit_and_requirements() {
    let mut world = world_with(config(maxed()));
    world.player_equip("kodai_wand").unwrap();
    world.player_equip("occult_necklace").unwrap();
    world.player_select_spell(Some("fire_surge")).unwrap();
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(6, 0)).unwrap();
    let defender = world.entity(dummy).unwrap().view().stats;

    let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
    let analysis = analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true);

    // Fire Surge 24 * (1 + 0.15 + 0.05) = 28.8, floored (the occult necklace is +5% since its nerf).
    assert_eq!(analysis.max_hit, 28);
    assert_eq!(plan.speed, 5);
    assert_eq!(plan.style, AttackStyle::Magic);

    // Crumble Undead only works on the Undead.
    world.player_select_spell(Some("crumble_undead")).unwrap();
    assert!(world.player.plan_attack(world.registry(), &defender).is_err());
    let zombie = world.spawn_npc_def("zombie", TileCoord::new(7, 0)).unwrap();
    let zombie_stats = world.entity(zombie).unwrap().view().stats;
    assert!(world.player.plan_attack(world.registry(), &zombie_stats).is_ok());
}

#[test]
fn a_low_level_player_cannot_select_a_high_level_spell() {
    let mut world = world_with(config(CombatLevels::new(1, 1, 1, 1, 20)));
    world.player_equip("kodai_wand").unwrap();

    assert!(world.player_select_spell(Some("fire_surge")).is_err());
    assert!(world.player_select_spell(Some("wind_strike")).is_ok());
}

#[test]
fn powered_staves_ignore_the_spellbook_and_scale_with_magic_level() {
    let mut world = world_with(config(maxed()));
    world.player_equip("trident_of_the_swamp").unwrap();
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(6, 0)).unwrap();
    let defender = world.entity(dummy).unwrap().view().stats;

    let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
    let analysis = analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true);

    assert_eq!(analysis.max_hit, 99 / 3 - 2); // 31
    assert_eq!(plan.speed, 4);
}

// ---- reference section 4: attributes and gear that exploits them ----

#[test]
fn a_salve_amulet_boosts_against_the_undead_only_and_beats_a_slayer_helm() {
    let mut world = world_with(config(maxed()));
    world.player_equip("abyssal_whip").unwrap();
    let zombie = world.spawn_npc_def("zombie", TileCoord::new(1, 0)).unwrap();
    let goblin = world.spawn_npc_def("goblin", TileCoord::new(2, 0)).unwrap();
    let max_hit = |world: &World, target: EntityId| {
        let defender = world.entity(target).unwrap().view().stats;
        let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
        analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true).max_hit
    };

    let plain = max_hit(&world, zombie);
    world.player_equip("salve_amulet_ei").unwrap();
    let salved = max_hit(&world, zombie);
    let salved_vs_living = max_hit(&world, goblin);

    assert!(salved > plain, "{salved} should beat {plain}");
    assert!(salved_vs_living < salved, "the multiplier only applies to the undead");

    // Slayer helm on task alone gives 7/6; with the salve both apply but only the salve counts.
    world.player_set_on_task(true);
    world.player_equip("slayer_helmet_i").unwrap();
    let both = max_hit(&world, zombie);
    assert_eq!(both, salved);
}

#[test]
fn demonbane_and_kalphite_weapons_key_off_the_attribute_tags() {
    let mut world = world_with(config(maxed()));
    let demon = world.spawn_npc_def("abyssal_demon", TileCoord::new(1, 0)).unwrap();
    let goblin = world.spawn_npc_def("goblin", TileCoord::new(2, 0)).unwrap();
    world.player_equip("arclight").unwrap();
    let roll = |world: &World, target: EntityId| {
        let defender = world.entity(target).unwrap().view().stats;
        let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
        analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true)
    };

    let vs_demon = roll(&world, demon);
    let vs_goblin = roll(&world, goblin);

    // x1.7 max hit against the Demon, nothing against the Goblin
    assert!(vs_demon.max_hit as f64 >= vs_goblin.max_hit as f64 * 1.6);
}

#[test]
fn flying_monsters_can_only_be_meleed_with_a_halberd() {
    let mut world = world_with(config(maxed()));
    world.player_equip("abyssal_whip").unwrap();
    let bird = world.spawn_npc_def("aviansie", TileCoord::new(1, 0)).unwrap();
    world.player_set_attack_target(bird);

    run(&mut world, 3);
    assert!(world.log().iter().any(|l| l.contains("cannot hurt")), "{:?}", world.log());
    assert_eq!(world.player.attack_target(), None);

    world.player_equip("rune_halberd").unwrap();
    world.player_set_attack_target(bird);
    run(&mut world, 3);
    assert!(player_hits(&world) >= 1, "{:?}", world.log());
}

#[test]
fn leafy_monsters_refuse_ordinary_weapons_but_not_leaf_bladed_ones() {
    let mut world = world_with(config(maxed()));
    let kurask = world.spawn_npc_def("kurask", TileCoord::new(1, 0)).unwrap();
    world.player_equip("abyssal_whip").unwrap();
    world.player_set_attack_target(kurask);
    run(&mut world, 2);
    assert_eq!(player_hits(&world), 0);

    world.player_equip("leaf_bladed_battleaxe").unwrap();
    world.player_set_attack_target(kurask);
    run(&mut world, 3);
    assert!(player_hits(&world) >= 1);
}

// ---- special attacks ----

#[test]
fn a_special_attack_costs_energy_hits_twice_and_then_disarms() {
    let mut world = world_with(config(maxed()));
    world.player_equip("dragon_dagger").unwrap();
    world.player_set_special(true).unwrap();
    let dummy = dummy_next_to_player(&mut world);
    world.player_set_attack_target(dummy);

    run(&mut world, 1);

    assert_eq!(player_hits(&world), 2, "the dragon dagger special is two hits");
    assert_eq!(world.player.special_energy, 75);
    assert!(!world.player.special_enabled);

    run(&mut world, 4); // next swing is a normal one
    assert_eq!(player_hits(&world), 3);
}

#[test]
fn a_special_cannot_be_armed_without_the_energy_or_without_one_to_arm() {
    let mut world = world_with(config(maxed()));
    assert!(world.player_set_special(true).is_err());

    world.player_equip("armadyl_godsword").unwrap();
    world.player.special_energy = 40;
    assert!(world.player_set_special(true).is_err());
    world.player.special_energy = 50;
    assert!(world.player_set_special(true).is_ok());
}

#[test]
fn special_energy_regenerates_ten_percent_every_fifty_ticks() {
    let mut world = world_with(config(maxed()));
    world.player.special_energy = 0;

    run(&mut world, 100);

    assert_eq!(world.player.special_energy, 20);
}

// ---- reference section 6: tick-driven resources ----

#[test]
fn hitpoints_regenerate_one_per_hundred_ticks_and_faster_with_a_bracelet() {
    let mut world = world_with(config(maxed()));
    world.player.mover.actor.health_mut().damage(50);
    run(&mut world, 200);
    assert_eq!(world.player.mover.actor.health().current(), 49 + 2);

    let mut world = world_with(config(maxed()));
    world.player_equip("regen_bracelet").unwrap();
    world.player_toggle_prayer("rapid_heal").unwrap();
    world.player.mover.actor.health_mut().damage(50);
    run(&mut world, 100);
    assert_eq!(world.player.mover.actor.health().current(), 49 + 4);
}

#[test]
fn prayers_drain_by_their_drain_effect_and_stop_when_the_pool_is_empty() {
    let mut world = world_with(config(maxed()));
    world.player_toggle_prayer("piety").unwrap();
    run(&mut world, 60);
    // effect 24 against resistance 60: 23 points by tick 60 (the counter must exceed it)
    assert_eq!(world.player.prayer.current(), 99 - 23);

    world.player.prayer.drain(1000);
    run(&mut world, 1);
    assert!(world.player.active_prayers.is_empty(), "no points left, so the prayer switches off");
}

#[test]
fn prayer_bonus_gear_slows_the_drain() {
    let drained = |gear: bool| {
        let mut world = world_with(config(maxed()));
        if gear {
            world.player_equip("bandos_chestplate").unwrap();
            world.player_equip("fire_cape").unwrap();
        }
        world.player_toggle_prayer("protect_from_melee").unwrap();
        run(&mut world, 300);
        99 - world.player.prayer.current()
    };

    assert!(drained(true) < drained(false));
}

#[test]
fn only_one_prayer_per_group_is_active_and_levels_gate_activation() {
    let mut world = world_with(config(maxed()));
    world.player_toggle_prayer("piety").unwrap();
    world.player_toggle_prayer("ultimate_strength").unwrap(); // shares the strength group
    assert_eq!(world.player.active_prayers, vec!["ultimate_strength".to_owned()]);

    world.player_toggle_prayer("protect_from_melee").unwrap();
    assert_eq!(world.player.reaction_state.active_prayer, ProtectionPrayer::Melee);
    world.player_toggle_prayer("protect_from_magic").unwrap(); // replaces the other overhead
    assert_eq!(world.player.reaction_state.active_prayer, ProtectionPrayer::Magic);

    let mut low = world_with(PlayerConfig { max_prayer_points: 20, ..config(maxed()) });
    assert!(low.player_toggle_prayer("piety").is_err());
}

#[test]
fn potion_boosts_apply_before_the_prayer_multiplier_and_decay_one_level_per_hundred_ticks() {
    let mut world = world_with(config(maxed()));
    world.player_use_consumable("super_strength_4").unwrap();
    assert_eq!(world.player.skills.current(osrs_core::model::Skill::Strength), 99 + 19);
    assert_eq!(world.player.mover.actor.stats().strength(), 118);

    run(&mut world, 100);
    assert_eq!(world.player.skills.current(osrs_core::model::Skill::Strength), 117);
    assert_eq!(world.player.mover.actor.stats().strength(), 117);
}

#[test]
fn consumables_respect_their_delay_groups() {
    let mut world = world_with(config(maxed()));
    world.player.mover.actor.health_mut().damage(60);

    world.player_use_consumable("shark").unwrap();
    assert!(world.player_use_consumable("shark").is_err(), "same group, still on cooldown");
    assert!(world.player_use_consumable("super_strength_4").is_ok(), "a potion shares no group with food");
    assert!(world.player_use_consumable("cooked_karambwan").is_ok(), "combo food has its own group");
    assert_eq!(world.player.mover.actor.health().current(), 39 + 20 + 18);

    run(&mut world, 3);
    assert!(world.player_use_consumable("shark").is_ok());
}

#[test]
fn eating_delays_the_next_swing() {
    let mut world = world_with(config(maxed()));
    world.player_equip("dragon_scimitar").unwrap();
    let dummy = dummy_next_to_player(&mut world);
    world.player_set_attack_target(dummy);
    run(&mut world, 1); // swing 1 on tick 1
    world.player_use_consumable("shark").unwrap(); // +3 ticks of attack delay

    run(&mut world, 5); // would normally swing again on tick 5
    assert_eq!(player_hits(&world), 1);
    run(&mut world, 3);
    assert_eq!(player_hits(&world), 2);
}

#[test]
fn running_spends_run_energy_and_an_empty_bar_forces_walking() {
    let config = PlayerConfig { run_energy_enabled: true, agility_level: 1, ..config(maxed()) };
    let mut world = world_with(config);
    world.player_request_walk_to(TileCoord::new(18, 0), false);

    run(&mut world, 5);
    let after_five = world.player.run_energy;
    assert!(after_five < 10_000);
    assert_eq!(after_five, 10_000 - 5 * 59 + 0, "agility 1, no weight: 59 units per running tick");

    world.player.run_energy = 0;
    let before = world.player.tile();
    run(&mut world, 2);
    assert_eq!(before.chebyshev_distance_to(world.player.tile()), 2, "one tile per tick once exhausted");
}

#[test]
fn standing_still_regenerates_run_energy_by_agility() {
    let config = PlayerConfig { run_energy_enabled: true, agility_level: 99, ..config(maxed()) };
    let mut world = world_with(config);
    world.player.run_energy = 1000;

    run(&mut world, 10);

    assert_eq!(world.player.run_energy, 1000 + 10 * 24);
}

// ---- poison, status, and the damage queue ----

#[test]
fn a_poisoning_monster_poisons_on_a_landed_hit_and_antipoison_cures_it() {
    let mut registry = Registry::builtin();
    registry
        .merge_json(
            "test",
            r#"{"npcs":[{"id":"spider","name":"Spider","hitpoints":20,
                "levels":{"attack":200,"strength":200},
                "attacks":[{"name":"Bite","max_hit":6,"speed":4,"on_hit":{"poison":{"severity":30}}}]}]}"#,
        )
        .unwrap();
    let mut world = dev_open_floor_world(20, 99, TileCoord::new(0, 0), config(CombatLevels::new(1, 1, 1, 1, 1)));
    world.set_registry(Rc::new(registry));
    let spider = world.spawn_npc_def("spider", TileCoord::new(1, 0)).unwrap();
    world.set_hostile(spider, true);

    run(&mut world, 30);
    assert!(world.player.status_effects.is_poisoned());

    world.player_use_consumable("antidote_plus_plus_4").unwrap();
    assert!(!world.player.status_effects.is_poisoned());
    run(&mut world, 20);
    assert!(!world.player.status_effects.is_poisoned(), "immune after the antidote");
}

#[test]
fn the_phoenix_necklace_heals_wipes_the_queue_and_is_used_up() {
    let mut world = world_with(config(maxed()));
    world.player_equip("phoenix_necklace").unwrap();
    world.player.mover.actor.health_mut().damage(80); // 19 left of 99: not yet at 20%
    world.player.damage_buffer.enqueue(5, 0, None);
    world.player.damage_buffer.enqueue(30, 3, None);

    run(&mut world, 1); // the 5 lands: 14 left, <= 19.8 -> necklace fires

    let hp = world.player.mover.actor.health().current();
    assert_eq!(hp, 14 + 29, "heals 30% of max HP (29)");
    assert_eq!(world.player.damage_buffer.pending_count(), 0, "the later hit is wiped too");
    assert_eq!(world.player.loadout.item_id_in(EquipSlot::Neck), None, "and the necklace is gone");
}

// ---- aggression ----

#[test]
fn aggressive_monsters_start_fights_within_range_and_passive_ones_wait_to_be_hit() {
    let mut world = world_with(config(CombatLevels::new(10, 10, 10, 1, 1)));
    world.spawn_npc_def("abyssal_demon", TileCoord::new(3, 0)).unwrap(); // aggressive, level 124
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(0, 3)).unwrap();

    run(&mut world, 12);

    assert!(world.log().iter().any(|l| l.starts_with("Abyssal demon uses")), "{:?}", world.log());
    assert_eq!(hp(&world, dummy), 10_000);
}

#[test]
fn monsters_ignore_players_far_above_their_level_unless_they_are_level_63_or_more() {
    // Goblin is combat level 2: a level-126 player is out of its league.
    let mut world = world_with(config(maxed()));
    world.spawn_npc_def("goblin", TileCoord::new(3, 0)).unwrap();
    run(&mut world, 12);
    assert!(!world.log().iter().any(|l| l.starts_with("Goblin uses")));

    // Kalphite soldier is combat level 85: it attacks anyone. (Aggressive in the bundle.)
    let mut world = world_with(config(maxed()));
    world.spawn_npc_def("kalphite_soldier", TileCoord::new(3, 0)).unwrap();
    run(&mut world, 12);
    assert!(world.log().iter().any(|l| l.starts_with("Kalphite soldier uses")), "{:?}", world.log());
}

#[test]
fn the_player_id_is_still_entity_zero() {
    assert_eq!(PLAYER_ID, EntityId(0));
    let _ = EquipSlot::Weapon;
}

// ---- scaling modifiers ----

#[test]
fn the_twisted_bow_scales_with_the_targets_magic_level() {
    let mut registry = Registry::builtin();
    registry
        .merge_json(
            "test",
            r#"{"npcs":[
                {"id":"low_magic","name":"Low magic","hitpoints":100,"levels":{"magic":1}},
                {"id":"high_magic","name":"High magic","hitpoints":100,"levels":{"magic":250}},
                {"id":"olm_like","name":"Olm-like","hitpoints":100,"levels":{"magic":350},"attributes":["xerician"]}
            ]}"#,
        )
        .unwrap();
    let mut world = dev_open_floor_world(20, 5, TileCoord::new(0, 0), config(maxed()));
    world.set_registry(Rc::new(registry));
    world.player_equip("twisted_bow").unwrap();
    world.player_equip("dragon_arrow").unwrap();

    let max_hit = |world: &mut World, id: &str, x: i32| {
        let target = world.spawn_npc_def(id, TileCoord::new(x, 0)).unwrap();
        let defender = world.entity(target).unwrap().view().stats;
        let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
        analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true).max_hit
    };

    let low = max_hit(&mut world, "low_magic", 4);
    let high = max_hit(&mut world, "high_magic", 6);
    let xeric = max_hit(&mut world, "olm_like", 8);

    assert!(high > low * 2, "{high} should be more than double {low}");
    assert!(xeric > high, "the Xerician cap of 350 lets it scale further: {xeric} vs {high}");
}

#[test]
fn dragon_hunter_gear_only_applies_to_draconic_targets() {
    let mut world = world_with(config(maxed()));
    let mut registry = Registry::builtin();
    registry.merge_json("t", r#"{"npcs":[{"id":"drake","name":"Drake","hitpoints":50,"attributes":["draconic"]}]}"#).unwrap();
    world.set_registry(Rc::new(registry));
    world.player_equip("dragon_hunter_crossbow").unwrap();
    world.player_equip("rune_bolts").unwrap();
    let drake = world.spawn_npc_def("drake", TileCoord::new(4, 0)).unwrap();
    let dummy = world.spawn_npc_def("combat_dummy", TileCoord::new(6, 0)).unwrap();
    let analyse = |world: &World, id| {
        let defender = world.entity(id).unwrap().view().stats;
        let plan = world.player.plan_attack(world.registry(), &defender).unwrap();
        analyze_attack(plan.definition.expect_targeted(), &world.player.mover.actor.stats(), &defender, true)
    };

    let vs_drake = analyse(&world, drake);
    let vs_dummy = analyse(&world, dummy);

    assert!(vs_drake.max_hit as f64 >= vs_dummy.max_hit as f64 * 1.2);
    assert!(vs_drake.attack_roll as f64 >= vs_dummy.attack_roll as f64 * 1.29);
}

// ---- tolerance ----

#[test]
fn ten_minutes_near_aggressive_monsters_makes_them_tolerant_and_leaving_resets_it() {
    // A goblin is aggressive but ignores a level-46 player (too high a level), so it never starts
    // a fight - which makes it a clean source of "monsters nearby" for the tolerance timer.
    let mut world = world_with(config(CombatLevels::new(10, 10, 10, 1, 1)));
    world.spawn_npc_def("goblin", TileCoord::new(3, 0)).unwrap();

    run(&mut world, 999);
    assert!(!world.player_is_tolerated());
    run(&mut world, 1);
    assert!(world.player_is_tolerated(), "1000 ticks = 10 minutes");

    // Now a monster that WOULD attack arrives, but the player is tolerated.
    world.spawn_npc_def("abyssal_demon", TileCoord::new(0, 3)).unwrap();
    run(&mut world, 15);
    assert!(!world.log().iter().any(|l| l.starts_with("Abyssal demon uses")), "{:?}", world.log());

    // Walking away from every aggressive monster resets the timer.
    world.player_request_walk_to(TileCoord::new(-19, -19), false);
    run(&mut world, 30);
    assert!(!world.player_is_tolerated());
}

// ---- bundled scenarios ----

#[test]
fn every_bundled_scenario_builds_runs_and_replays_identically() {
    use osrs_core::data::{builtin_scenarios, ScenarioDef};
    use osrs_core::sim::scenarios::build_scenario;

    let registry = Rc::new(Registry::builtin());
    for (name, json) in builtin_scenarios() {
        let scenario = ScenarioDef::from_json(name, json).unwrap();
        let play = || {
            let mut world = build_scenario(Rc::clone(&registry), &scenario).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut trace = Vec::new();
            for _ in 0..60 {
                world.run_tick();
                trace.push(world.snapshot());
            }
            (trace, world.log().to_vec())
        };

        let (first, first_log) = play();
        let (second, second_log) = play();

        assert_eq!(first, second, "{name}: same seed, same fight");
        assert_eq!(first_log, second_log, "{name}: same log");
    }
}

#[test]
fn a_scenario_that_asks_for_the_impossible_fails_loudly() {
    use osrs_core::data::ScenarioDef;
    use osrs_core::sim::scenarios::build_scenario;

    let registry = Rc::new(Registry::builtin());
    let too_low = ScenarioDef::from_json(
        "t",
        r#"{"player":{"levels":{"prayer":10},"prayers":["piety"]},"npcs":[]}"#,
    )
    .unwrap();
    let unknown_item = ScenarioDef::from_json("t", r#"{"player":{"equipment":["bogus_sword"]}}"#).unwrap();
    let bad_target = ScenarioDef::from_json("t", r#"{"player":{"attack":3},"npcs":[]}"#).unwrap();

    assert!(build_scenario(Rc::clone(&registry), &too_low).is_err());
    assert!(build_scenario(Rc::clone(&registry), &unknown_item).is_err());
    assert!(build_scenario(registry, &bad_target).is_err());
}
