//! Gathering skills (woodcutting now; mining and fishing are meant to fit the same loop): walking
//! up to a resource, checking the requirements, rolling for the product on the skill clock, and
//! depleting the resource. Everything a particular resource does differently is data
//! (`data::GatheringNodeDef`).
//!
//! The loop, run in the player's PlayerQueue phase:
//!
//! 1. Walk until next to the object (orthogonally, as for an NPC).
//! 2. On arrival, check the requirements again, say the start message and start the tool's
//!    animation.
//! 3. Every tick the skill clock is at 0, roll: the clock is set to the node's interval (4 ticks
//!    for trees) and counts down one per tick whatever the player does, so re-clicking does not
//!    reset it. A player whose clock is idle rolls on the tick they start.
//! 4. A success gives the product and the experience, rolls for the pet, and depletes the
//!    resource if its depletion rule says so, which ends the action. A full inventory ends it too.
//!
//! Random draws come from the player's own skilling generator, never the encounter's, so
//! gathering cannot change a fight's rolls. Per attempt they are drawn in this order: the attempt
//! (`0..256`), then on a success the pet (`0..N`), then if it depletes the respawn time.

use std::rc::Rc;

use super::objects::{Approach, PlayerAction};
use super::player::Player;
use super::scenery::Loc;
use super::world::World;
use crate::data::{DepletionDef, GatheringNodeDef, Registry, Requirement, ToolDef};
use crate::pathfinding::TargetSpec;
use crate::rng::{one_in, Rng};
use crate::rules;

/// Gathering from one placed resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatherAction {
    /// The `GatheringNodeDef` id.
    pub node: String,
    pub loc: Loc,
    /// Whether the player has arrived and begun (said the start message, is animating).
    pub started: bool,
}

/// Whether the player has `tool` and the level to use it (boosts count).
fn can_use(player: &Player, tool: &ToolDef) -> bool {
    player.skills.current(tool.skill) >= tool.level && player.has_item(&tool.item)
}

/// The tool the player would gather from `node` with: of the ones it takes that the player can
/// use, the one with the best chance at the player's level (the higher-level tool on a tie).
/// Returns it with its success numerator.
fn best_tool<'a>(player: &Player, registry: &'a Registry, node: &GatheringNodeDef) -> Option<(&'a ToolDef, i32)> {
    let level = player.skills.current(node.skill);
    node.chances
        .iter()
        .filter_map(|(item, [low, high])| {
            let tool = registry.tool(item).filter(|tool| can_use(player, tool))?;
            Some((tool, rules::success_numerator(*low, *high, level)))
        })
        .max_by_key(|(tool, numerator)| (*numerator, tool.level))
}

/// Whether the player meets `requirement`.
fn meets(player: &Player, registry: &Registry, requirement: &Requirement) -> bool {
    match requirement {
        Requirement::Level { skill, level, .. } => player.skills.current(*skill) >= *level,
        Requirement::Tool { category, .. } => registry.tools().any(|tool| &tool.category == category && can_use(player, tool)),
        Requirement::FreeSlots { slots, .. } => player.inventory.free_slots() >= *slots,
    }
}

/// The refusal of the first requirement the player does not meet, if any.
fn first_unmet(player: &Player, registry: &Registry, requirements: &[Requirement]) -> Option<String> {
    requirements.iter().find(|r| !meets(player, registry, r)).map(Requirement::refusal)
}

/// Where a pet that was rolled goes. Followers are not modelled, so it is always an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PetOutcome {
    /// Into the backpack.
    Backpack,
    /// The player already has it, or has no room: no pet, only the message.
    Missed,
}

impl PetOutcome {
    fn for_player(player: &Player, pet_item: &str) -> PetOutcome {
        if player.has_item(pet_item) || player.inventory.free_slots() == 0 {
            PetOutcome::Missed
        } else {
            PetOutcome::Backpack
        }
    }

    fn message(self) -> &'static str {
        match self {
            PetOutcome::Backpack => "You feel something weird sneaking into your backpack.",
            PetOutcome::Missed => "You have a funny feeling like you would have been followed...",
        }
    }
}

impl World {
    /// Checks what can be checked from afar, then sends the player to gather from `loc`.
    pub(super) fn player_start_gathering(&mut self, node: &GatheringNodeDef, loc: Loc) -> Result<(), String> {
        if let Some(refusal) = first_unmet(&self.player, self.registry(), &node.requirements) {
            return Err(refusal);
        }
        if let Some(PlayerAction::Gather(current)) = &self.player.action {
            if current.loc == loc && current.started {
                return Ok(()); // already at it; clicking again changes nothing
            }
        }
        self.player.set_action(PlayerAction::Gather(GatherAction { node: node.id.clone(), loc, started: false }));
        Ok(())
    }

    /// One tick of gathering. Returns whether it used the tick.
    pub(super) fn run_gather(&mut self, mut gather: GatherAction) -> bool {
        let registry = Rc::clone(self.registry());
        let Some(node) = registry.gathering_node(&gather.node) else {
            self.player.action = None;
            return false;
        };

        // someone (anyone) has depleted it: stop without a word
        if self.scenery_state.is_replaced(&gather.loc) {
            self.player.action = None;
            return true;
        }

        // (with no path yet, it keeps trying)
        if self.approach(TargetSpec::adjacent(gather.loc.tile, gather.loc.size.0, gather.loc.size.1)) != Approach::There {
            return true;
        }

        // checked on arrival and before every attempt: an axe dropped or an inventory filled
        // meanwhile ends it
        if let Some(refusal) = first_unmet(&self.player, &registry, &node.requirements) {
            self.game_message(refusal);
            self.player.action = None;
            return true;
        }
        let Some((_, numerator)) = best_tool(&self.player, &registry, node) else {
            self.player.action = None;
            return true;
        };
        if !gather.started {
            gather.started = true;
            if let Some(message) = &node.start_message {
                self.game_message(message.clone());
            }
        }
        if let DepletionDef::Timer { ticks } = node.depletion {
            self.scenery_state.work_timer(&gather.loc, ticks);
        }

        if self.player.skill_clock > 0 {
            self.player.action = Some(PlayerAction::Gather(gather));
            return true;
        }
        self.player.skill_clock = node.roll_interval_ticks;
        let draw = self.player.skilling_rng.next_range(0, 256);
        if !rules::attempt_succeeds(numerator, draw) {
            self.player.action = Some(PlayerAction::Gather(gather));
            return true;
        }

        let continues = self.gathered(node, &gather.loc);
        self.player.action = continues.then_some(PlayerAction::Gather(gather));
        true
    }

    /// A successful attempt: the product, the experience, the pet roll and depletion. Returns
    /// whether the player carries on.
    fn gathered(&mut self, node: &GatheringNodeDef, loc: &Loc) -> bool {
        // The free-slot requirement is checked before every attempt, so this only happens for a
        // node whose data leaves it out. As in the game, there is no experience without the
        // product: the player is told and stops. (Overflow onto the ground needs ground items.)
        let left_over = self.give_item(&node.product, node.product_quantity).unwrap_or(node.product_quantity);
        if left_over == node.product_quantity {
            let room = node.requirements.iter().find(|r| matches!(r, Requirement::FreeSlots { .. }));
            let refusal = room.map_or_else(|| "You don't have enough inventory space.".to_owned(), Requirement::refusal);
            self.game_message(refusal);
            return false;
        }
        if let Some(message) = &node.success_message {
            self.game_message(message.clone());
        }
        self.grant_xp(node.skill, node.xp_tenths);

        if let Some(pet) = &node.pet {
            let denominator = rules::pet_chance_denominator(pet.base_chance, self.player.skills.base(node.skill));
            if one_in(&mut self.player.skilling_rng, denominator) {
                let outcome = PetOutcome::for_player(&self.player, &pet.item);
                if outcome == PetOutcome::Backpack {
                    let _ = self.give_item(&pet.item, 1);
                }
                self.game_message(outcome.message().to_owned());
            }
        }

        let depletes = match node.depletion {
            DepletionDef::EverySuccess => true,
            DepletionDef::Timer { .. } => self.scenery_state.timer_expired(loc),
        };
        if depletes {
            if let Some(replacement) = node.depleted_form(loc.id) {
                let [min, max] = node.respawn_ticks;
                let respawn = self.player.skilling_rng.next_range(min, max + 1);
                self.change_loc(*loc, loc.with_id(replacement), respawn);
            }
            return false;
        }

        // the inventory filled up: say so now rather than on the next attempt
        let room = node.requirements.iter().find(|r| matches!(r, Requirement::FreeSlots { .. }));
        if let Some(requirement) = room.filter(|r| !meets(&self.player, self.registry(), r)) {
            self.game_message(requirement.refusal());
            return false;
        }
        true
    }

    /// The sequence the player loops for its current action, while it is at it: the animation of
    /// the tool it gathers with.
    pub fn player_action_animation(&self) -> Option<u32> {
        let gather = match &self.player.action {
            Some(PlayerAction::Gather(gather)) => gather,
            Some(PlayerAction::Light(light)) => return light.started.then_some(super::firemaking::LIGHTING_ANIMATION),
            Some(PlayerAction::Tend(tend)) => return tend.started.then_some(super::firemaking::TENDING_ANIMATION),
            Some(PlayerAction::Rest(rest)) => return rest.resting.then_some(super::firemaking::RESTING_ANIMATION),
            Some(PlayerAction::Door(_)) | Some(PlayerAction::Take { .. }) | None => return None,
        };
        let node = self.registry().gathering_node(&gather.node).filter(|_| gather.started)?;
        best_tool(&self.player, self.registry(), node)?.0.animation
    }
}

/// Success chances for tests: every attempt succeeds, or none does.
#[cfg(test)]
pub(crate) const SURE: [i32; 2] = [255, 255];
#[cfg(test)]
pub(crate) const HOPELESS: [i32; 2] = [-1, -1];

/// The bundled regular tree with every axe's chance set to `chance` (tests use [`SURE`] and
/// [`HOPELESS`] so nothing depends on how a seed rolls).
#[cfg(test)]
pub(crate) fn regular_tree_with_chance(chance: [i32; 2]) -> GatheringNodeDef {
    let mut node = Registry::builtin().gathering_node("regular_tree").expect("the bundle has the regular tree").clone();
    for value in node.chances.values_mut() {
        *value = chance;
    }
    node
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::CollisionFlag;
    use crate::model::Skill;
    use crate::rng::ScriptedRng;
    use crate::sim::scenarios::dev_open_floor_world;
    use crate::sim::{PlayerConfig, WorldEvent};
    use crate::tiles::TileCoord;

    const TREE: u32 = 1276;
    const STUMP: u32 = 1342;

    fn tree_tile() -> TileCoord {
        TileCoord::new(5, 0)
    }

    fn tree() -> Loc {
        Loc { id: TREE, tile: tree_tile(), kind: 10, orientation: 0, size: (1, 1) }
    }

    /// A flat world with one regular tree five tiles east of the player, a level-`level`
    /// woodcutter carrying `items`, and the bundled data plus `extra` nodes.
    fn woods_with(level: i32, items: &[&str], extra: &[GatheringNodeDef]) -> World {
        let mut world = dev_open_floor_world(10, 4242, TileCoord::new(0, 0), PlayerConfig::new_account());
        let mut registry = Registry::builtin();
        for node in extra {
            registry.insert_gathering_node_for_test(node.clone());
        }
        world.set_registry(Rc::new(registry));
        world.player.skills.set_base(Skill::Woodcutting, level);
        for item in items {
            world.give_item(item, 1).unwrap();
        }
        world.map.add_flags(tree_tile(), CollisionFlag::BLOCK_OBJECT);
        world.add_scenery(tree());
        world
    }

    fn woods(level: i32, items: &[&str]) -> World {
        woods_with(level, items, &[])
    }

    /// The regular tree, changed: every attempt succeeds, and `change` does the rest.
    fn sure_tree(change: impl FnOnce(&mut GatheringNodeDef)) -> GatheringNodeDef {
        let mut node = regular_tree_with_chance(SURE);
        change(&mut node);
        node
    }

    fn chop(world: &mut World) -> Result<(), String> {
        world.player_interact_object(TREE, tree_tile(), "Chop down")
    }

    /// Runs ticks until `done` holds (or `limit` ticks pass), returning how many ran.
    fn run_until(world: &mut World, limit: i32, done: impl Fn(&World) -> bool) -> i32 {
        for ran in 0..limit {
            if done(world) {
                return ran;
            }
            world.run_tick();
        }
        limit
    }

    fn logs(world: &World) -> u32 {
        world.player.inventory.count("logs")
    }

    /// The regular tree, where no attempt succeeds (so the player keeps chopping).
    fn hopeless_tree() -> GatheringNodeDef {
        regular_tree_with_chance(HOPELESS)
    }

    #[test]
    fn the_player_walks_up_to_the_tree_and_swings() {
        let mut world = woods_with(1, &["bronze_axe"], &[hopeless_tree()]);
        chop(&mut world).unwrap();

        run_until(&mut world, 20, |w| w.player_action_animation().is_some());

        assert_eq!(world.player.tile(), TileCoord::new(4, 0), "next to the tree, not on it");
        assert!(world.take_messages().contains(&"You swing your axe at the tree.".to_owned()));
        assert_eq!(world.player_action_animation(), Some(879), "the bronze axe's chop");
        assert_eq!(world.player_facing_loc(), Some(tree()));
    }

    #[test]
    fn a_log_gives_experience_and_fells_the_tree_to_a_stump() {
        let mut world = woods(1, &["bronze_axe"]);
        chop(&mut world).unwrap();

        run_until(&mut world, 400, |w| logs(w) == 1);

        let messages = world.take_messages();
        assert!(messages.contains(&"You get some logs.".to_owned()), "{messages:?}");
        assert_eq!(world.player.skills.xp_tenths(Skill::Woodcutting), 250);
        assert!(world.take_events().contains(&WorldEvent::Experience { skill: Skill::Woodcutting, gained_tenths: 250 }));
        let changes: Vec<_> = world.scenery_changes().copied().collect();
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].loc, changes[0].replacement), (tree(), tree().with_id(STUMP)));
        assert!(world.player.action().is_none(), "a regular tree gives one log, then the player stops");
        assert_eq!(world.player_action_animation(), None);
    }

    #[test]
    fn the_stump_grows_back_within_the_wikis_respawn_time() {
        let mut world = woods_with(1, &["bronze_axe"], &[sure_tree(|_| {})]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| logs(w) == 1);

        let ran = run_until(&mut world, 200, |w| w.scenery_changes().next().is_none());

        // respawn_ticks is 59..=98, counting the tick it was felled on
        assert!((58..=97).contains(&ran), "grew back after {ran} more ticks");
    }

    #[test]
    fn attempts_come_every_four_ticks() {
        // a tree that never runs out, where every attempt succeeds
        let node = sure_tree(|n| n.depletion = DepletionDef::Timer { ticks: 10_000 });
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| logs(w) == 1);

        let mut gaps = Vec::new();
        for _ in 0..3 {
            let before = logs(&world);
            gaps.push(run_until(&mut world, 20, |w| logs(w) > before));
        }

        assert_eq!(gaps, vec![4, 4, 4]);
    }

    #[test]
    fn a_despawn_timer_fells_the_tree_on_the_first_success_after_it_runs_out() {
        // 8 ticks of chopping: attempts on the first tick, the 5th and the 9th (when it has run out)
        let node = sure_tree(|n| n.depletion = DepletionDef::Timer { ticks: 8 });
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        chop(&mut world).unwrap();

        run_until(&mut world, 60, |w| w.scenery_changes().next().is_some());

        assert_eq!(logs(&world), 3);
        assert!(world.player.action().is_none());
    }

    #[test]
    fn chopping_needs_an_axe_the_player_can_use() {
        let mut world = woods(1, &[]);
        assert_eq!(chop(&mut world), Err("You do not have an axe which you have the woodcutting level to use.".to_owned()));

        let mut world = woods(1, &["steel_axe"]);
        assert!(chop(&mut world).is_err(), "a steel axe needs level 6");

        let mut world = woods(6, &["steel_axe"]);
        assert_eq!(chop(&mut world), Ok(()));
    }

    #[test]
    fn a_tree_above_the_players_level_is_refused_with_its_level() {
        let node = sure_tree(|n| {
            n.requirements[0] = Requirement::Level {
                skill: Skill::Woodcutting,
                level: 15,
                message: Some("You need a {skill} level of {level} to chop down this tree.".into()),
            }
        });
        let mut world = woods_with(1, &["bronze_axe"], &[node]);

        assert_eq!(chop(&mut world), Err("You need a Woodcutting level of 15 to chop down this tree.".to_owned()));
    }

    #[test]
    fn a_full_inventory_is_refused_and_filling_it_stops_the_player() {
        let mut world = woods(1, &["bronze_axe"]);
        world.give_item("shark", 27).unwrap();
        assert_eq!(chop(&mut world), Err("Your inventory is too full to hold any more logs.".to_owned()));

        let node = sure_tree(|n| n.depletion = DepletionDef::Timer { ticks: 10_000 });
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        world.give_item("shark", 25).unwrap();
        chop(&mut world).unwrap();

        run_until(&mut world, 40, |w| w.player.action().is_none());

        assert_eq!(logs(&world), 2);
        assert!(world.take_messages().contains(&"Your inventory is too full to hold any more logs.".to_owned()));
    }

    #[test]
    fn a_product_that_does_not_fit_gives_no_experience() {
        // a node whose data forgets the free-slot requirement
        let node = sure_tree(|n| n.requirements.retain(|r| !matches!(r, Requirement::FreeSlots { .. })));
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        world.give_item("shark", 27).unwrap();
        chop(&mut world).unwrap();

        run_until(&mut world, 20, |w| w.player.action().is_none());

        assert_eq!(logs(&world), 0);
        assert_eq!(world.player.skills.xp_tenths(Skill::Woodcutting), 0);
        assert!(world.scenery_changes().next().is_none(), "the tree still stands");
        assert!(world.take_messages().contains(&"You don't have enough inventory space.".to_owned()));
    }

    #[test]
    fn the_best_axe_the_player_can_use_is_chosen() {
        let mut world = woods_with(1, &["bronze_axe", "rune_axe", "iron_axe"], &[hopeless_tree()]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| w.player_action_animation().is_some());
        assert_eq!(world.player_action_animation(), Some(877), "iron: rune needs level 41");

        let mut world = woods_with(41, &["bronze_axe", "rune_axe"], &[hopeless_tree()]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| w.player_action_animation().is_some());
        assert_eq!(world.player_action_animation(), Some(867));
    }

    #[test]
    fn a_wielded_axe_counts() {
        let mut world = woods(1, &[]);
        world.player_equip("bronze_axe").unwrap();

        assert_eq!(chop(&mut world), Ok(()));
    }

    #[test]
    fn walking_away_stops_chopping() {
        let node = sure_tree(|n| n.depletion = DepletionDef::Timer { ticks: 10_000 });
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| logs(w) == 1);

        world.player_request_walk_to(TileCoord::new(0, 0), false);
        world.run_tick();

        assert!(world.player.action().is_none());
        assert_eq!(world.player_action_animation(), None);
    }

    #[test]
    fn a_stump_cannot_be_chopped() {
        let mut world = woods_with(1, &["bronze_axe"], &[sure_tree(|_| {})]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| logs(w) == 1);

        assert!(chop(&mut world).is_err(), "the tree is not there any more");
        assert_eq!(world.player_interact_object(STUMP, tree_tile(), "Chop down"), Err("Nothing interesting happens.".to_owned()));
    }

    #[test]
    fn options_nothing_handles_say_nothing_interesting_happens() {
        let mut world = woods(1, &["bronze_axe"]);

        assert_eq!(world.player_interact_object(TREE, tree_tile(), "Climb"), Err("Nothing interesting happens.".to_owned()));
        assert!(world.player_interact_object(TREE, TileCoord::new(7, 7), "Chop down").is_err(), "no tree there");
    }

    #[test]
    fn a_pet_goes_into_the_backpack_unless_the_player_has_one_or_no_room() {
        // base 26 at level 1 is a 1 in 1 chance
        let node = sure_tree(|n| n.pet.as_mut().unwrap().base_chance = 26);
        let mut world = woods_with(1, &["bronze_axe"], &[node]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| logs(w) == 1);

        assert_eq!(world.player.inventory.count("beaver"), 1);
        assert!(world.take_messages().contains(&"You feel something weird sneaking into your backpack.".to_owned()));

        assert_eq!(PetOutcome::for_player(&world.player, "beaver"), PetOutcome::Missed, "one is enough");
        let mut full = woods(1, &[]);
        assert_eq!(PetOutcome::for_player(&full.player, "beaver"), PetOutcome::Backpack);
        full.give_item("shark", 28).unwrap();
        assert_eq!(PetOutcome::for_player(&full.player, "beaver"), PetOutcome::Missed, "no room");
    }

    #[test]
    fn a_tool_the_node_does_not_take_is_never_chosen() {
        let node = sure_tree(|n| {
            n.chances.remove("iron_axe");
        });
        let mut world = woods_with(1, &["iron_axe", "bronze_axe"], &[node]);
        chop(&mut world).unwrap();
        run_until(&mut world, 20, |w| w.player_action_animation().is_some() || logs(w) > 0);

        assert_eq!(logs(&world), 1, "chopped with the bronze axe");
    }

    #[test]
    fn one_in_n_is_a_draw_of_zero() {
        let mut rng = ScriptedRng::from_ints(vec![0, 1]);
        assert!(one_in(&mut rng, 317_622));
        assert!(!one_in(&mut rng, 317_622));
    }

    #[test]
    fn the_same_seed_chops_the_same_way() {
        let run = || {
            let mut world = woods(1, &["bronze_axe"]);
            chop(&mut world).unwrap();
            run_until(&mut world, 400, |w| logs(w) == 1);
            (world.current_tick(), world.scenery_changes().copied().collect::<Vec<_>>())
        };

        assert_eq!(run(), run());
    }

    #[test]
    fn gathering_does_not_touch_the_encounter_generator() {
        let mut world = woods(1, &["bronze_axe"]);
        let before = world.encounter.rng().clone().next_range(0, 1_000_000);
        chop(&mut world).unwrap();
        run_until(&mut world, 400, |w| logs(w) == 1);

        assert_eq!(world.encounter.rng().clone().next_range(0, 1_000_000), before);
    }

    #[test]
    fn a_level_up_is_congratulated() {
        let mut world = woods(1, &[]);
        world.grant_xp(Skill::Woodcutting, 830);

        assert_eq!(world.player.skills.base(Skill::Woodcutting), 2);
        assert!(world.take_events().contains(&WorldEvent::LevelUp { skill: Skill::Woodcutting, level: 2 }));
        assert!(world
            .take_messages()
            .contains(&"Congratulations, you've just advanced a Woodcutting level. You are now level 2.".to_owned()));
    }
}
