use std::collections::HashMap;
use std::rc::Rc;

use super::config::{BossConfig, HealerConfig, NpcConfig, PassiveConfig};
use super::npc_factory;
use super::wander::Wander;
use crate::actors::{Actor, ActorKind, CombatTargetView, EntityId, Health, MovementPolicy, Mover};
use crate::collision::CollisionMap;
use crate::data::NpcDef;
use crate::combat::{
    AttackDefinition, AttackStyle, DamageBuffer, FixedRotationPattern, NpcCombatState,
    PendingAttackQueue, ProtectionPrayer, ReactionRequirement,
};
use crate::encounters::{Phase, TriggerBinding};
use crate::tiles::TileCoord;

/// State specific to a plain `NpcCombatant`.
pub struct NpcState {
    pub targets_player: bool,
    pub attack: Rc<AttackDefinition>,
    pub combat_state: NpcCombatState,
    /// This NPC has no Phase of its own (it never swaps movesets), so its movement policy is a
    /// direct field, defaulting to chase-then-hold-at-adjacency.
    pub movement_policy: Rc<dyn MovementPolicy>,
}

/// State specific to a `BossCombatant`: its phases and triggers.
pub struct BossState {
    pub targets_player: bool,
    pub phases_by_name: HashMap<String, Phase>,
    pub triggers: Vec<TriggerBinding>,
    pub current_phase: Option<Phase>,
    /// `None` until `World::configure_boss` is called.
    pub combat_state: Option<NpcCombatState>,
}

impl BossState {
    pub fn current_phase_name(&self) -> Option<&str> {
        self.current_phase.as_ref().map(|phase| phase.name.as_str())
    }
}

/// State specific to a `HealerNpc`.
pub struct HealerState {
    pub heal_target: Option<EntityId>,
    pub heal_amount_per_tick: i32,
    pub movement_policy: Rc<dyn MovementPolicy>,
}

/// What kind of NPC an `Entity` is, and the state only that kind has.
pub enum Behavior {
    Npc(NpcState),
    Boss(BossState),
    Healer(HealerState),
    Passive,
}

/// Where one of a monster's `summons` stands: whether the call will be answered when its hitpoints
/// next fall low enough, and the helpers it has called so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummonState {
    pub armed: bool,
    pub spawned: Vec<EntityId>,
}

/// One non-player thing in the world. Shared by every NPC kind: the actor/mover, the two
/// buffers incoming attacks flow through, and a name (the menu swap key).
pub struct Entity {
    pub id: EntityId,
    pub name: String,
    pub mover: Mover,
    pub damage_buffer: DamageBuffer,
    pub pending_attacks: PendingAttackQueue,
    pub behavior: Behavior,
    /// The data definition this entity was built from, if any (aggression, attackability, tags).
    pub def: Option<Rc<NpcDef>>,
    /// Set for NPCs that stroll about while they are not fighting.
    pub wander: Option<Wander>,
    /// The monster that summoned this one, for a summoned helper.
    pub leader: Option<EntityId>,
    /// One per entry of the definition's `summons`.
    pub summons: Vec<SummonState>,
    /// Ticks until a healer may heal its leader again.
    pub heal_cooldown: i32,
}

impl Entity {
    fn new(id: EntityId, name: &str, mover: Mover, behavior: Behavior) -> Self {
        Self {
            id,
            name: name.to_owned(),
            mover,
            damage_buffer: DamageBuffer::new(),
            pending_attacks: PendingAttackQueue::new(),
            behavior,
            def: None,
            wander: None,
            leader: None,
            summons: Vec::new(),
            heal_cooldown: 0,
        }
    }

    /// A monster built from a data definition. One with attacks becomes a configurable fighter
    /// (the boss machinery with a single phase); one without is inert. Either way it starts
    /// NOT hostile: aggression or being attacked is what turns it on.
    pub fn new_from_def(id: EntityId, name: &str, start: TileCoord, def: &Rc<NpcDef>, map: &mut CollisionMap) -> Self {
        let actor = Actor::new(ActorKind::Npc, start, def.size, def.hitpoints, npc_factory::stats_from_def(def));
        let mut mover = Mover::new(actor, map);
        // every NPC blocks other NPCs; it stands in players' way only if its definition says so
        mover.set_blocks_players(map, def.blocks_movement);

        let behavior = if def.attacks.is_empty() {
            Behavior::Passive
        } else {
            let pattern = Rc::new(npc_factory::pattern_from_def(def));
            let phase = Phase::new("default", pattern, Some(npc_factory::movement_policy_from_def(def.movement)));
            let mut phases_by_name = HashMap::new();
            phases_by_name.insert(phase.name.clone(), phase.clone());
            Behavior::Boss(BossState {
                targets_player: false,
                phases_by_name,
                triggers: Vec::new(),
                combat_state: Some(NpcCombatState::new(Rc::clone(&phase.attack_pattern))),
                current_phase: Some(phase),
            })
        };

        let mut entity = Self::new(id, name, mover, behavior);
        entity.def = Some(Rc::clone(def));
        entity.summons = def.summons.iter().map(|_| SummonState { armed: true, spawned: Vec::new() }).collect();
        if def.wander_radius > 0 {
            let seed = (id.0 as i32).wrapping_mul(7919) ^ start.x.wrapping_mul(31) ^ start.z;
            entity.wander = Some(Wander::new(start, def.wander_radius, seed));
        }
        entity
    }

    pub fn new_npc(id: EntityId, name: &str, start: TileCoord, config: &NpcConfig, map: &mut CollisionMap) -> Self {
        let actor = Actor::new(ActorKind::Npc, start, config.footprint_size, config.max_health, config.levels.to_stats());
        let mover = Mover::new(actor, map);
        let attack = AttackDefinition::new("Melee", 0, config.melee_telegraph_ticks, AttackStyle::Melee)
            .with_reaction(ReactionRequirement::prayer_match(ProtectionPrayer::Melee))
            .shared();
        // Only one attack exists, so a one-entry FixedRotationPattern is just "always this attack".
        let combat_state = NpcCombatState::new(Rc::new(FixedRotationPattern::new(vec![Rc::clone(&attack)])));
        let state = NpcState {
            targets_player: config.targets_player,
            attack,
            combat_state,
            movement_policy: Rc::new(crate::actors::ChaseToMeleeRangeMovementPolicy),
        };
        Self::new(id, name, mover, Behavior::Npc(state))
    }

    pub fn new_boss(id: EntityId, name: &str, start: TileCoord, config: &BossConfig, map: &mut CollisionMap) -> Self {
        let actor = Actor::new(ActorKind::Npc, start, config.footprint_size, config.max_health, config.levels.to_stats());
        let mover = Mover::new(actor, map);
        let state = BossState {
            targets_player: config.targets_player,
            phases_by_name: HashMap::new(),
            triggers: Vec::new(),
            current_phase: None,
            combat_state: None,
        };
        Self::new(id, name, mover, Behavior::Boss(state))
    }

    pub fn new_healer(
        id: EntityId,
        name: &str,
        start: TileCoord,
        heal_target: Option<EntityId>,
        config: &HealerConfig,
        map: &mut CollisionMap,
    ) -> Self {
        let actor = Actor::new(ActorKind::Npc, start, config.footprint_size, config.max_health, config.levels.to_stats());
        let mover = Mover::new(actor, map);
        let state = HealerState {
            heal_target,
            heal_amount_per_tick: config.heal_amount_per_tick,
            movement_policy: Rc::new(crate::actors::ChaseToMeleeRangeMovementPolicy),
        };
        Self::new(id, name, mover, Behavior::Healer(state))
    }

    pub fn new_passive(id: EntityId, name: &str, start: TileCoord, config: &PassiveConfig, map: &mut CollisionMap) -> Self {
        let actor = Actor::new(ActorKind::Npc, start, config.footprint_size, config.max_health, config.levels.to_stats());
        let mover = Mover::new(actor, map);
        Self::new(id, name, mover, Behavior::Passive)
    }

    pub fn view(&self) -> CombatTargetView {
        self.mover.actor.view()
    }

    pub fn health(&self) -> &Health {
        self.mover.actor.health()
    }

    pub fn is_dead(&self) -> bool {
        self.health().is_dead()
    }

    pub fn tile(&self) -> TileCoord {
        self.mover.actor.current_tile()
    }

    /// Menu swap key (the C# `SwapKey`): swaps are grouped by name, matching how OSRS's own Menu
    /// Entry Swapper keys by NPC name, not by spawn.
    pub fn swap_key(&self) -> &str {
        &self.name
    }

    /// The labels of the canonical right-click menu, default first. Choosing "Attack" is
    /// `World::player_set_attack_target(entity.id)`.
    pub fn menu_labels(&self) -> [&'static str; 2] {
        ["Attack", "Examine"]
    }

    /// The text the "Examine" option logs.
    pub fn examine_text(&self) -> String {
        let flavour = match self.behavior {
            Behavior::Npc(_) => "It's an aggressive creature.",
            Behavior::Boss(_) => "It looks extremely dangerous.",
            Behavior::Healer(_) => "It's healing its master.",
            Behavior::Passive => "It doesn't seem interested in fighting.",
        };
        format!("{}: {}", self.name, flavour)
    }
}
