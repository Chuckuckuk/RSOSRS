use std::collections::BTreeMap;
use super::config::PlayerConfig;
use super::gathering::PlayerAction;
use super::loadout::Loadout;
use super::skills::Skills;
use crate::actors::{Actor, ActorKind, CombatTargetView, EntityId};
use crate::actors::Mover;
use crate::collision::CollisionMap;
use crate::combat::{
    DamageBuffer, PendingAttackQueue, PlayerReactionState, PrayerPool,
    ProtectionPrayer, StatusEffectTracker,
};
use crate::model::Skill;
use crate::rng::{DotNetRandom, Rng};
use crate::rules::{self, PrayerDrainCounter};
use crate::tiles::TileCoord;

/// A walk the player has queued but not yet turned into a path (the C# `hasPendingClick` /
/// `pendingClickWorld` / `pendingClickForceInverse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingClick {
    pub tile: TileCoord,
    pub force_inverse: bool,
}

/// The player's state: the logic of `PlayerMover` minus input polling, plus everything the full
/// OSRS ruleset hangs off a player - skills, worn gear, stance, prayers, spell, special attack and
/// the tick-driven resources. The tick behaviour that needs other entities (queueing a walk,
/// auto-attacking) lives on `World`, which owns them; the registry-driven behaviour (equipping,
/// prayers, attack building, regen) is in `player_runtime.rs`.
pub struct Player {
    pub mover: Mover,
    pub damage_buffer: DamageBuffer,
    pub pending_attacks: PendingAttackQueue,
    /// The reaction system reads this at resolve time.
    pub reaction_state: PlayerReactionState,
    /// The resource Corruption-style effects drain.
    pub prayer: PrayerPool,
    /// Active Corruption/Burn/DisableRunning/Stun/Poison/Venom/Freeze effects.
    pub status_effects: StatusEffectTracker,
    pub is_running: bool,
    pub skills: Skills,
    pub loadout: Loadout,
    /// Index into the current weapon's stances; `None` means no stance bonuses at all.
    pub stance_index: Option<usize>,
    /// Ids of the active prayers, in activation order.
    pub active_prayers: Vec<String>,
    pub selected_spell: Option<String>,
    pub special_enabled: bool,
    /// 0..=100.
    pub special_energy: i32,
    /// Run energy in hundredths of a percent (`rules::RUN_ENERGY_MAX` is full).
    pub run_energy: i32,
    pub on_slayer_task: bool,
    pub agility_level: i32,
    pub(crate) run_energy_enabled: bool,
    pub(crate) stamina_ticks: i32,
    pub(crate) hp_regen_counter: i32,
    pub(crate) spec_regen_counter: i32,
    pub(crate) drift_counters: [i32; Skill::COUNT],
    pub(crate) prayer_drain: PrayerDrainCounter,
    pub(crate) consumable_delays: BTreeMap<String, i32>,
    pub(crate) ran_this_tick: bool,
    pub(crate) attack_interval_ticks: i32,
    pub inventory: super::inventory::Inventory,
    pub(crate) attack_target: Option<EntityId>,
    /// An NPC the player is walking up to, or standing at, to interact with it (anything but
    /// attacking). The player keeps facing it.
    pub(crate) interact_target: Option<EntityId>,
    pub(crate) attack_cooldown: i32,
    /// Whether the cooldown had run out at the start of this tick (set by the world as it ticks
    /// the cooldown down, whatever the player is doing).
    pub(crate) attack_ready: bool,
    pub(crate) pending_click: Option<PendingClick>,
    pub(crate) current_path_is_running: bool,
    /// Whether, this tick, the player is still walking up to something it clicked to interact
    /// with (a "red click" not yet carried out). NPCs that target the player hold still meanwhile
    /// ("red-x"), so the two do not dance round each other while the player steps out from under
    /// a large NPC. A walk click (yellow) clears the target and so lifts it.
    pub(crate) approaching_target: bool,
    /// What the player is busy doing besides fighting and walking (gathering from a tree).
    pub(crate) action: Option<PlayerAction>,
    /// Ticks until the next gathering attempt may be made; counts down every tick, whatever the
    /// player is doing (see `sim::gathering`).
    pub(crate) skill_clock: i32,
    /// The skilling rolls' own generator, so gathering never shifts a fight's rolls.
    pub(crate) skilling_rng: DotNetRandom,
}

impl Player {
    pub fn new(start: TileCoord, config: &PlayerConfig, map: &mut CollisionMap) -> Self {
        let actor = Actor::new(ActorKind::Player, start, config.footprint_size, config.max_health, config.levels.to_stats());

        let levels = config.levels;

        Self {
            mover: Mover::new(actor, map),
            damage_buffer: DamageBuffer::new(),
            pending_attacks: PendingAttackQueue::new(),
            reaction_state: PlayerReactionState::default(),
            prayer: PrayerPool::new(config.max_prayer_points),
            status_effects: StatusEffectTracker::new(),
            is_running: config.is_running,
            skills: Skills::new(
                levels.attack,
                levels.strength,
                levels.defence,
                levels.ranged,
                levels.magic,
                config.max_prayer_points,
                config.max_health,
            )
            .with(Skill::Agility, config.agility_level),
            loadout: Loadout::new(),
            stance_index: None,
            active_prayers: Vec::new(),
            selected_spell: None,
            special_enabled: false,
            special_energy: rules::SPEC_ENERGY_MAX,
            run_energy: rules::RUN_ENERGY_MAX,
            on_slayer_task: false,
            agility_level: config.agility_level,
            run_energy_enabled: config.run_energy_enabled,
            stamina_ticks: 0,
            hp_regen_counter: 0,
            spec_regen_counter: 0,
            drift_counters: [0; Skill::COUNT],
            prayer_drain: PrayerDrainCounter::default(),
            consumable_delays: BTreeMap::new(),
            ran_this_tick: false,
            attack_interval_ticks: config.attack_interval_ticks,
            inventory: super::inventory::Inventory::new(),
            attack_target: None,
            interact_target: None,
            attack_cooldown: 0,
            attack_ready: true,
            pending_click: None,
            current_path_is_running: false,
            approaching_target: false,
            action: None,
            skill_clock: 0,
            skilling_rng: DotNetRandom::new(0),
        }
    }

    pub fn tile(&self) -> TileCoord {
        self.mover.actor.current_tile()
    }

    pub fn view(&self) -> CombatTargetView {
        self.mover.actor.view()
    }

    pub fn is_dead(&self) -> bool {
        self.mover.actor.health().is_dead()
    }

    /// Where the active path ends, if any steps remain.
    pub fn destination(&self) -> Option<TileCoord> {
        self.mover.destination()
    }

    pub fn attack_target(&self) -> Option<EntityId> {
        self.attack_target
    }

    pub fn combat_level(&self) -> i32 {
        self.skills.combat_level()
    }

    /// Placeholder prayer-book input: 1/2/3 activate Protect from Melee/Ranged/Magic, 0 clears.
    /// (The registry-driven `toggle_prayer` is the real prayer book; this sets protection directly,
    /// with no drain and no prayer to look up.)
    pub fn set_active_prayer(&mut self, prayer: ProtectionPrayer) {
        self.reaction_state.active_prayer = prayer;
    }

    /// Makes `target` this player's attack target - chasing (if needed) and attacking happen in
    /// the PlayerQueue phase from here on, superseding any queued walk-click.
    pub fn set_attack_target(&mut self, target: EntityId) {
        self.attack_target = Some(target);
        self.interact_target = None;
        self.pending_click = None;
        self.action = None;
    }

    /// Walk up to `target` and stay facing it. Replaces an attack or a queued walk.
    pub fn set_interact_target(&mut self, target: EntityId) {
        self.interact_target = Some(target);
        self.attack_target = None;
        self.pending_click = None;
        self.action = None;
    }

    /// Starts `action` (gathering...). Replaces an attack, an NPC interaction or a queued walk.
    pub(crate) fn set_action(&mut self, action: PlayerAction) {
        self.action = Some(action);
        self.attack_target = None;
        self.interact_target = None;
        self.pending_click = None;
    }

    pub fn action(&self) -> Option<&PlayerAction> {
        self.action.as_ref()
    }

    /// Whether the player carries or wears `item`.
    pub fn has_item(&self, item: &str) -> bool {
        self.inventory.count(item) > 0 || self.loadout.slots().any(|(_, worn)| worn == item)
    }

    pub fn interact_target(&self) -> Option<EntityId> {
        self.interact_target
    }

    /// The entity the player's body is turned toward, if any: what it is attacking or
    /// interacting with.
    pub fn face_target(&self) -> Option<EntityId> {
        self.attack_target.or(self.interact_target)
    }

    /// Queues a walk to `tile` - the single path both a plain ground click and a "Walk here"
    /// menu option funnel through. Walking anywhere cancels whatever we were attacking.
    pub fn request_walk_to(&mut self, tile: TileCoord, force_inverse: bool) {
        self.attack_target = None;
        self.interact_target = None;
        self.action = None;
        self.pending_click = Some(PendingClick { tile, force_inverse });
    }

    /// How many tiles the player's movement covers this tick, and whether that is a run.
    fn step_budget(&self) -> (i32, bool) {
        let out_of_energy = self.run_energy_enabled && self.run_energy <= 0;
        let running = self.current_path_is_running && !self.status_effects.is_running_disabled() && !out_of_energy;
        let steps = if self.is_dead() || self.status_effects.is_frozen() {
            0
        } else if running {
            2
        } else {
            1
        };
        (steps, running)
    }

    /// Where this tick's movement will leave the player, along the path it is on. The game moves
    /// and acts on the same tick, so a player stepping into range can attack on that very tick.
    pub(crate) fn tile_after_this_ticks_move(&self) -> TileCoord {
        let (steps, _) = self.step_budget();
        self.mover.tile_after_steps(steps)
    }

    /// The PlayerMovement phase: step along the path, then run the incoming-damage pipeline.
    ///
    /// Runs after PlayerQueue/NpcAi (where hits/attacks against this player are enqueued)
    /// within the same tick, so a 0-delay/0-travel-tick hit still lands the tick it was rolled.
    /// `pending_attacks` first, since a Doom-style roll that comes due THIS tick needs to feed
    /// the damage buffer before it advances. `status_effects` advances LAST so a debuff that
    /// just landed this same tick also pulses this same tick, rather than starting a tick late.
    pub(crate) fn run_movement(
        &mut self,
        map: &mut CollisionMap,
        rng: &mut dyn Rng,
        attacker_is_dead: &dyn Fn(EntityId) -> bool,
    ) -> Vec<i32> {
        // Aeriscia's run-disable forces walk speed regardless of the player's own toggle; so does
        // an empty run-energy bar when run energy is being tracked.
        let out_of_energy = self.run_energy_enabled && self.run_energy <= 0;
        if out_of_energy {
            // The game switches running off when the bar empties and leaves it off.
            self.is_running = false;
            self.current_path_is_running = false;
        }
        let (steps, actually_running) = self.step_budget();
        let before = self.tile();
        self.mover.step(map, steps);
        self.ran_this_tick = actually_running && self.tile() != before;

        let view = self.view();
        self.pending_attacks.advance(&view, &mut self.damage_buffer, Some(&self.reaction_state), rng, attacker_is_dead);
        let landed = self.damage_buffer.advance(self.mover.actor.health_mut(), Some(&mut self.status_effects));
        self.status_effects.advance(self.mover.actor.health_mut(), Some(&mut self.prayer));
        landed
    }
}
