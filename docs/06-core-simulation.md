# 6. The simulation: ticks, data, World, Player, Server

This chapter covers the top of `osrs_core`: the machinery that turns the pieces from chapters 3-5
into a running game. Folders: [`tick/`](../osrs_core/src/tick/), [`encounters/`](../osrs_core/src/encounters/),
[`interaction/`](../osrs_core/src/interaction/), [`data/`](../osrs_core/src/data/) and
[`sim/`](../osrs_core/src/sim/).

---

## 6.1 Ticks

### `TickPhase` and `TickScheduler`

Files: [`tick_phase.rs`](../osrs_core/src/tick/tick_phase.rs),
[`tick_scheduler.rs`](../osrs_core/src/tick/tick_scheduler.rs).

`TickPhase` lists the seven phases (chapter 2.4). `TickScheduler<Id>` is generic over an ID type
(the world uses `EntityId`). It keeps one list of IDs per phase and runs a tick like this:

```rust
pub fn run_tick(&mut self, mut dispatch: impl FnMut(TickPhase, Id, i32, &mut TickRegistrar<Id>)) {
    self.current_tick += 1;
    for phase in PHASE_ORDER {
        self.apply_pending_changes();                       // registrations from earlier
        let list = std::mem::take(&mut self.registered[phase.index()]);
        for &id in &list { dispatch(phase, id, tick, &mut self.registrar); }
        self.registered[phase.index()] = list;
    }
}
```

The scheduler does not own the entities; it only holds IDs and calls back with
`(phase, id, tick)`. The caller (the `World`) looks the entity up and runs it. In Python terms
this is a dispatcher that calls `world.dispatch(phase, id)` in the right order.

**Deferred registration.** Something that registers or unregisters an ID *during* a phase (a boss
trigger spawning healers, say) does not modify the list being iterated. The change goes into a
`TickRegistrar` and is applied at the next phase boundary. In the C# the tickables called back into
the scheduler directly; Rust's borrow rules forbid mutating the scheduler while iterating it, so
the registrar is passed into the callback instead. Registering the same ID twice in a phase is a
no-op. Order within a phase is insertion order.

`PHASE_ORDER` is a `const` chosen at compile time from `NPCS_MOVE_BEFORE_PLAYERS`.

### From wall-clock time to ticks: `TickAccumulator` and `TickDriver`

Files: [`tick_accumulator.rs`](../osrs_core/src/tick/tick_accumulator.rs),
[`sim/tick_driver.rs`](../osrs_core/src/sim/tick_driver.rs).

`TickAccumulator::advance(dt, tick_seconds, max_catch_up)` adds elapsed seconds and returns how
many whole ticks to run. If the backlog exceeds `max_catch_up` ticks (the process was suspended,
say), the excess is discarded and counted in `dropped_ticks`, so the game never fast-forwards
through a burst of dozens of ticks. It accumulates in `f64` and compares with a small epsilon,
because 0.6 has no exact binary representation and repeated subtraction would otherwise drift
just under a tick boundary and lose a tick.

`TickDriver` wraps an accumulator with:

* `base_tick_seconds` (0.6) and `tick_rate_multiplier` (1.0; raise it to run faster);
* pause and single-step: while paused, `update` runs nothing unless a single step was requested;
* `tick_progress01()`, how far through the current tick we are, for interpolation;
* a primed accumulator, so the very first `update` runs a tick immediately instead of waiting
  600 ms.

`update(dt, run_tick)` calls the `run_tick` closure once per whole tick and returns the count.

---

## 6.2 Encounters and interaction (ported scaffolding)

Folder: [`encounters/`](../osrs_core/src/encounters/).

* **`EncounterContext`** is shared fight state: the seeded `DotNetRandom` every roll draws from,
  the elapsed tick count, named integer counters and boolean flags (for scripted triggers), and the
  participant IDs. The `World` owns one.
* **`Phase`** is a named `(attack pattern, movement policy)` pair: one stage of a boss fight.
* **`PhaseTrigger`** is a condition: HP at or below a percentage, elapsed ticks reached, an
  encounter flag equal to a value, or "the tracked entity died". `is_satisfied` evaluates it.
* **`TriggerAction`** is what happens: swap to a named phase, spawn a named group of dormant
  entities, set a flag, or modify a stat (logged as not implemented).
* **`TriggerBinding`** pairs a trigger with an action and fires **once**.

Bosses are configured from Rust (`World::configure_boss`), not from JSON. The machinery is
exercised by tests; the Jad and Purutu encounter setups it was built for are deliberately not
ported yet.

Folder: [`interaction/`](../osrs_core/src/interaction/) holds `MenuOption` (a label and an action
closure), the `Interactable` trait and `MenuEntrySwapper`, which remembers per-NPC-name which menu
option the player promoted to the top and reorders menus to match. These were ported from C#
with their tests, but the Bevy client builds its own menus (chapter 7) and does not use them yet.

---

## 6.3 The data layer

Folder: [`data/`](../osrs_core/src/data/), with JSON in [`osrs_core/data/`](../osrs_core/data/).
The JSON format is documented field by field in [`osrs_core/SIMULATOR.md`](../osrs_core/SIMULATOR.md);
this section explains how the code loads it.

### Definition types

Each is a serde struct with `#[serde(default, deny_unknown_fields)]`: missing keys take sensible
defaults, and **unknown keys are errors**, so a typo like `"max_hitt"` fails loudly instead of
silently becoming the default.

| Type | File | Holds |
|---|---|---|
| `ItemDef` | [items.rs](../osrs_core/src/data/items.rs) | id, name, `EquipSlot`, bonuses, weight, always-on modifiers, tags, optional `WeaponDef` and `AmmoDef` |
| `WeaponDef` | items.rs | category, attack speed, attack range, stances, optional special attack, `powered` (powered staff max-hit source), `can_cast`, `ammo_family`, `thrown`, `attack_sound` |
| `StanceDef` | items.rs | name, kind (accurate, aggressive...), damage type, invisible bonuses, speed and range deltas, optional `sound` for a stance that sounds different |
| `SpecialAttackDef` | items.rs | energy cost, extra modifiers, hit count, optional speed/damage type/max hit overrides, `always_hits` |
| `SetDef` | items.rs | pieces (each a list of acceptable item ids), bonuses and modifiers granted only when every piece is worn |
| `PrayerDef` | [prayers.rs](../osrs_core/src/data/prayers.rs) | level, `PrayerEffects`, drain effect, exclusive `groups`, `protects`, modifiers, tags, activation `sound` |
| `SpellDef` | [spells.rs](../osrs_core/src/data/spells.rs) | spellbook, level, base max hit, range, cast speed, modifiers, `on_hit`, `level_scaling`, `requires_target_attribute`, `cast_sound`, `hit_sound` |
| `ConsumableDef` | [consumables.rs](../osrs_core/src/data/consumables.rs) | delay group and ticks, attack delay, a list of `ConsumableEffect`s (heal, boost, drain, restore, cures...), `leaves` (the next dose, or the vial), optional `sound` |
| `NpcDef` | [npcs.rs](../osrs_core/src/data/npcs.rs) | combat level, hitpoints, size, levels, bonuses, attributes, attacks, rotation, movement, aggression, `wander_radius`, `blocks_movement`, `appearance` (cache NPC id), combat `animations`, `sounds` (`NpcSounds`: attack, hit, death), `summons`, `heals_summoner`, `examine`, `attackable_if`, tags |
| `NpcAttackDef` | npcs.rs | style, damage type, max hit, speed, range, the three timing numbers, prayer penetration, `protectable`, weight, hit count, modifiers, `on_hit`, and presentation: `animation`, `projectile`, `target_graphic`, `impact_graphic`, `sound`, `resolve_sound` |
| `ScenarioDef` | [scenario.rs](../osrs_core/src/data/scenario.rs) | a whole fight: seed, floor size, `PlayerSetup` (levels, gear, inventory, prayers, potions, stance, spell...), `NpcSpawn`s, and an optional real-map `location`; `load_scenario` finds one by bundled name or file path |
| `OnHitDef` | [mod.rs](../osrs_core/src/data/mod.rs) | JSON form of a status effect; `to_status()` converts it, returning `Err` instead of panicking on bad numbers |

`NpcDef::attackable_if` is a `Condition` (chapter 3.4) evaluated against the player's attack:
it is how a Flying monster refuses melee unless it is a halberd, and a Leafy one refuses
everything but leaf-bladed weapons.

### `DataFile` and `Registry`

A JSON document may hold any mix of `items`, `sets`, `prayers`, `spells`, `npcs` and
`consumables` (the `DataFile` struct). The `Registry` holds every definition in six `BTreeMap`s
keyed by id.

| Method | Effect |
|---|---|
| `merge_json(source_name, text)` | parse one document and add its definitions; duplicate ids (across all documents loaded so far) are an error |
| `load_dir(path)` | every `*.json` in a folder, sorted by name, then `validate` |
| `builtin()` | the bundled files, embedded at compile time with `include_str!`, validated; panics if the bundle is broken (a build-time bug) |
| `validate()` | cross-checks everything and reports **all** problems at once as `DataError::Invalid(Vec<String>)` |
| `item(id)`, `npc(id)`, ... | lookups returning `Option<&Def>` |

Validation checks things serde cannot: a weapon must be in the weapon slot and have a stance;
speeds and hit counts must be at least 1; prayer penetration must be within 0..=1; a monster
attack's damage type must match its style; set pieces must name existing items; weighted
rotations need a positive weight.

`items_wiki.json` is a bulk import from the OSRS Wiki's equipment tables; `items.json` and
`sets.json` are hand-written. Ids must be unique across all files.

---

## 6.4 The `World`

File: [`sim/world.rs`](../osrs_core/src/sim/world.rs).

The `World` is everything in one fight or one game area. It owns:

```rust
pub struct World {
    pub map: CollisionMap,
    pub pathfinder: BfsPathfinder,
    pub encounter: EncounterContext,        // seeded RNG + elapsed ticks + flags
    pub player: Player,
    entities: Vec<Option<Box<Entity>>>,     // NPCs; index = id - 1
    scheduler: TickScheduler<EntityId>,
    spawn_groups: HashMap<String, Vec<SpawnBlueprint>>,   // dormant entities
    log: Vec<String>,                       // developer combat log
    messages: Vec<String>,                  // chat lines for the player
    events: Vec<WorldEvent>,                // for the client
    dying: Vec<Combatant>, dead: HashSet<Combatant>,
    registry: Rc<Registry>,
    aggro_region_ticks: i32,                // for aggression tolerance
}
```

### IDs

* The player is always `PLAYER_ID = EntityId(0)`.
* `ENCOUNTER_CLOCK_ID = EntityId(u32::MAX)` is a pseudo-entity registered in the `Regen` phase.
  Its "turn" advances the encounter's tick counter and the aggression-tolerance timer.
* NPCs get IDs 1, 2, 3, ... in spawn order, and NPC `n` lives at `entities[n - 1]`. Slots are never
  reused; a dead NPC stays in its slot.

The world has exactly one player slot. Multiplayer would need a different design here.

### Spawning

| Method | Creates |
|---|---|
| `spawn_npc_def(id, tile)` | an NPC from a JSON `NpcDef` (the normal path) |
| `spawn_npc`, `spawn_boss`, `spawn_healer`, `spawn_passive` | the older Rust-configured kinds from the C# port, built from `NpcConfig`, `BossConfig`, ... ([`sim/config.rs`](../osrs_core/src/sim/config.rs)) |
| `register_spawn_group(name, blueprints)` | dormant entities that a `SpawnEntities` trigger activates later. A `SpawnBlueprint` is a `Box<dyn FnOnce(EntityId, &mut CollisionMap) -> Entity>`, i.e. "a function that will build the entity when called", so its occupancy is not marked until it appears. |

Each spawn registers the entity's ID in the phases its kind uses: `NpcAi` and `NpcMovement`, or
only `NpcMovement` for passive NPCs.

### Entities and behaviours

File: [`sim/entity.rs`](../osrs_core/src/sim/entity.rs).

```rust
pub struct Entity {
    pub id, pub name, pub mover: Mover,
    pub damage_buffer: DamageBuffer, pub pending_attacks: PendingAttackQueue,
    pub behavior: Behavior,
    pub def: Option<Rc<NpcDef>>,      // set when built from JSON
    pub wander: Option<Wander>,
}
pub enum Behavior { Npc(NpcState), Boss(BossState), Healer(HealerState), Passive }
```

`Behavior` is an enum with data (primer 1.5); each variant carries the state only that kind
needs. A **data-defined monster with attacks becomes `Behavior::Boss`** with a single phase named
`"default"`, because the boss machinery (pattern + movement policy + triggers) is the general
case. A data-defined monster with no attacks becomes `Passive`.

[`sim/npc_factory.rs`](../osrs_core/src/sim/npc_factory.rs) does the conversion:

* `attack_from_def` turns an `NpcAttackDef` into an `AttackDefinition` with a fixed max hit, the
  three timing numbers, `with_attack_speed(speed)`, a prayer-match reaction if `protectable`, and
  the on-hit status effect;
* `pattern_from_def` builds a `RangeAwarePattern` over all the monster's attacks;
* `movement_policy_from_def` maps the JSON `movement` to a policy;
* `stats_from_def` builds `CombatStats` with the monster's bonuses, attributes and a +1 stance
  bonus (so it attacks at `level + 9`).

Data-defined monsters **start passive**: `targets_player` is false until aggression triggers,
the player hits them, or a scenario marks them hostile.

### Running a tick

```rust
pub fn run_tick(&mut self) {
    let mut scheduler = std::mem::take(&mut self.scheduler);   // step 1: take it out
    scheduler.run_tick(|phase, id, tick, registrar| self.dispatch(phase, id, tick, registrar));
    self.scheduler = scheduler;                                // step 2: put it back
    self.settle_deaths();
}
```

The scheduler is taken out of `self` so that the closure can borrow `self` mutably while the
scheduler runs (primer 1.7).

`dispatch` routes each `(phase, id)`:

* the encounter clock in `Regen`: advance the encounter tick and the tolerance timer;
* the player: `player_run_queue`, `player_run_movement` or `player.run_regen`;
* an NPC: **take the entity out of its slot**, match on `(behaviour, phase)` to the right
  `run_*` function, then put it back. While it is out, the `run_*` function holds `&mut self`
  (the rest of the world) and `&mut entity` separately.

### The player's queue phase: `player_run_queue`

1. The attack timer: `attack_ready = attack_cooldown <= 0`, then decrement if positive. It runs
   every tick whether walking, standing, attacking or not, like the game's.
2. Clear `approaching_target` (set again below if still walking up to something).
3. Dead or stunned: stop here. A queued walk survives a stun.
4. **Attack target set** (and still alive): run `player_run_attack_tick` and stop. An attack
   target overrides any queued walk.
5. **Interact target set** ("Talk-to" and other non-attack NPC options): if not adjacent, path to
   adjacency. Nothing else happens on arrival yet.
6. **Pending walk click**: decide whether this path runs (`force_inverse` is the ctrl-click
   "opposite of your run toggle" option), then BFS to the tile.

### The player's attack: `player_run_attack_tick`

1. Take the target out of its slot.
2. `player.plan_attack(registry, target_stats)` builds the swing the current setup would make
   (6.5). An `Err` ("this weapon needs bolt ammunition equipped") clears the target and becomes a
   chat message.
3. If the monster's `attackable_if` condition rejects this attack, the same happens.
4. Work out the reach: `Adjacent` for melee with range 1, else `WithinRangeOfRect` with the
   attack's range.
5. If out of reach: path toward it (BFS, every tick, so the player avoids obstacles, unlike NPCs).
   Then ask where this tick's movement will leave the player (`tile_after_this_ticks_move`). If
   that tile is in reach and the attack timer is ready, attack **this tick from that tile**.
   Moving and attacking share a tick in OSRS. Otherwise put the target back and stop.
6. If the timer is not ready, stop.
7. Fix the projectile travel time from the distance (`plan.finish(distance)`), push an `Attack`
   event, and for ranged attacks a `Projectile` event (graphic chosen from the ammunition).
8. For each of `hit_count` hits: roll now with `successful_floor = 1` and enqueue the damage into
   the **target's** `DamageBuffer` (or into its `PendingAttackQueue` if the attack has an impact
   delay).
9. A data-defined monster that is hit starts fighting back (`targets_player = true`).
10. Pay special-attack energy and disarm the special.
11. `attack_cooldown = speed - 1`, which with step 1 makes swings exactly `speed` ticks apart.

### NPC turns

* **`run_boss_ai`** (all data-defined fighters):
  1. `update_aggression`: an aggressive monster within its aggression range of the player turns
     hostile, unless the player has become tolerated, or the player's combat level is more than
     double the monster's and the monster is below level 63.
  2. `answer_summons`: a monster with `summons` whose hitpoints have fallen low enough calls its
     helpers (Jad's four Yt-HurKot) onto open tiles around it (`summon_spots`: the middles of its
     sides, then its corners, then further out). They are placed and registered with the tick's
     registrar, so they act from the next phase boundary. A call that may come again is re-armed
     once the monster is back at full health, and then only replaces the helpers that died.
  3. `heal_leader`: a helper with `heals_summoner` heals the monster that called it every
     `every_ticks` ticks while within `range`, emitting `WorldEvent::Heal`. A helper the player
     has attacked (`targets_player`) does not heal; it gives the player up once they are more
     than `forget_player_beyond` tiles away.
  4. Advance any homing orbs (chapter 4.10).
  5. Evaluate phase triggers (before the death check, so "on death" triggers fire on the killing
     tick). Apply their actions.
  6. Ask the current phase's pattern whether any attack can reach the player (`engages`).
  7. Advance the `NpcCombatState`. If a telegraph has just begun, `report_telegraph_start` pushes
     the `Attack` event now, carrying the attack's own `animation`: the wind-up is the warning the
     player reacts to (Jad). If an attack resolved: either conjure its orb, or call
     `fire_at_player`.
* **`fire_at_player`** pushes an `Attack` event (only for attacks without a telegraph, which were
  not announced yet) and the attack's graphics (`Projectile`, marked `released` for a telegraphed
  attack; `Graphic` events for its target and impact graphics), then for each hit either queues a
  delayed roll in
  the player's `PendingAttackQueue` (impact delay > 0), or computes the reaction multiplier from
  the player's current prayer, rolls (floor 0), and enqueues into the player's `DamageBuffer`.
* **`run_*_movement`** for every NPC kind starts with `advance_incoming`: advance the NPC's own
  pending queue and damage buffer (landing hits and emitting `Hit` events). Then, if alive and not
  hostile: a summoned healer walks to stand beside the monster it heals (`walk_to_leader`), and
  anything with a `Wander` takes a wander step. Otherwise a data-defined monster that can
  already reach the player with an attack holds still (the base rule, chapter 5.7), and anything
  else applies its movement policy.
* **"Red-x".** While the player is still walking up to something it clicked (`approaching_target`),
  an NPC that targets the player holds still. This stops the two from circling each other while
  the player steps out from under a large NPC.
* **Healers** walk to their heal target and heal it every tick while adjacent. **Passive** NPCs
  only land incoming hits and wander.

### Wandering

File: [`sim/wander.rs`](../osrs_core/src/sim/wander.rs). An idle NPC with `wander_radius > 0`
has its own `DotNetRandom`, seeded from its ID and spawn tile. On a tick when it has no path, with
probability 1/12 it picks a random tile within the radius of its spawn and paths there (one tile a
tick). Separate seeds mean wandering never perturbs fight rolls.

### Aggression tolerance

`update_aggro_region` (in the encounter clock's turn) counts consecutive ticks the player spends
within range of an aggressive, tolerance-subject monster. After `TOLERANCE_TICKS` (1000 ticks,
10 minutes) such monsters stop starting fights. Leaving resets the count.

### Deaths

A death is confirmed one tick after health reaches 0. `settle_deaths` runs after each tick:
everything in `dying` (noticed last tick) is moved into `dead` with a `Died` event, then anything
at 0 HP that is not yet dead is added to `dying`. The game shows the killing blow's splat for a
tick before the death animation, and this reproduces that.

### Outputs

| Buffer | Read with | Contains | Goes to |
|---|---|---|---|
| `log` | `log()` | every developer log line, never cleared | the `simulate` binary, tests |
| `messages` | `take_messages()` | lines the player should read (rejections, "You eat the shark.") | `Snapshot::messages` |
| `events` | `take_events()` | `Attack`, `Projectile`, `Graphic`, `Heal`, `Hit`, `Died`, `Sound` in order | `Snapshot::events` |

`snapshot()` (the method on `World`) returns a one-line text summary for traces.

### Player commands on `World`

`World` exposes the player's API as `player_*` methods (`player_request_walk_to`,
`player_set_attack_target`, `player_equip`, `player_toggle_prayer`, `player_inventory_option`,
...). Most clone the `Rc<Registry>` first, then call the matching `Player` method. Cloning the
`Rc` is needed because `self.player.equip(&self.registry, ...)` would borrow `self` mutably and
immutably at once.

`player_inventory_option(slot, option)` implements the inventory menu: "Wield"/"Wear" equips and
puts the replaced item back in that slot; "Eat"/"Drink" uses the consumable and removes one;
"Drop" deletes the stack (there are no ground items yet); "Use" just says nothing happens.

---

## 6.5 The `Player`

Files: [`sim/player.rs`](../osrs_core/src/sim/player.rs) (state and movement),
[`sim/player_runtime.rs`](../osrs_core/src/sim/player_runtime.rs) (registry-driven behaviour),
[`skills.rs`](../osrs_core/src/sim/skills.rs), [`loadout.rs`](../osrs_core/src/sim/loadout.rs),
[`inventory.rs`](../osrs_core/src/sim/inventory.rs).

`Player` is one big struct. Grouped:

| Group | Fields |
|---|---|
| body | `mover` (actor + path), `damage_buffer`, `pending_attacks`, `status_effects` |
| defence | `reaction_state` (active protection prayer, fighting style), `prayer: PrayerPool` |
| skills | `skills: Skills` (base and current level per combat skill), `agility_level` |
| gear | `loadout: Loadout` (item id per slot), `inventory: Inventory` (28 slots), `stance_index` |
| choices | `active_prayers`, `selected_spell`, `special_enabled`, `on_slayer_task`, `is_running` |
| resources | `special_energy` (0-100), `run_energy` (0-10000), regen and drift counters, `prayer_drain`, `consumable_delays`, `stamina_ticks` |
| intent | `attack_target`, `interact_target`, `pending_click`, `attack_cooldown`, `attack_ready`, `approaching_target` |

`PlayerConfig::new_account()` is what the live client logs in as: level 1 everywhere, 10 HP,
1 prayer point.

### Movement: `run_movement`

Called in the `PlayerMovement` phase.

1. If run energy is tracked and empty, switch running off (and leave it off).
2. `step_budget`: 0 tiles if dead or frozen; 2 if this path is a run and running is not disabled;
   else 1.
3. Step the mover; remember whether the player actually ran (for run-energy drain).
4. Advance the pending-attack queue (Doom-style rolls that are due), then the damage buffer
   (landing hits), then status effects. Status effects go last so a debuff that landed this tick
   also pulses this tick.
5. Return the damage amounts that landed (the world turns them into `Hit` events), then
   `after_damage` runs the Phoenix necklace and Redemption checks.

### Gear, stance, prayers, spells, specials

| Method | Effect |
|---|---|
| `equip(registry, item)` | via `Loadout::equip`; a new weapon resets the stance to 0 and disarms the special |
| `unequip(registry, slot)` | |
| `set_stance(registry, index)` | must exist on the wielded weapon (or bare fists: punch, kick, block) |
| `toggle_prayer(registry, id)` | off if on; else checks base Prayer level and points, switches off prayers sharing a group, switches on |
| `select_spell(registry, id)` | checks Magic level |
| `set_special_enabled(registry, on)` | needs a weapon with a special and enough energy |
| `use_consumable(registry, id)` | checks the delay group's cooldown, applies each effect, starts the cooldown, adds attack delay |

After every change that affects rolls, **`refresh_stats`** rebuilds the `CombatStats` from current
skill levels (at least 1), the loadout's summed bonuses (items plus complete sets), the combined
prayer effects and the stance bonuses, and stores them on the actor. Only protection prayers set
`reaction_state.active_prayer`.

[`Loadout`](../osrs_core/src/sim/loadout.rs) stores only item *ids* per slot and looks everything
else up in the registry when asked, so it can never hold stale copies. `active_sets` finds sets
whose every piece (any alternative) is worn; `totals` and `modifiers` include them.

### Building a swing: `plan_attack`

`plan_attack(registry, target_stats) -> Result<AttackPlan, String>` decides what attack the current
setup would make against this target. It is pure (it does not change the player) and is used both
for real swings and for `expected_dps`.

1. Start the modifier list with the loadout's modifiers (items and sets) plus active prayers'
   modifiers.
2. Build the flags: `on_task`, `weapon_category:<c>`, `weapon:<id>`, `weapon_tag:<t>` per tag,
   `stance:<snake_case_name>`.
3. Choose the attack kind:
   * **A spell** if one is selected and the weapon can cast: magic style, `Spell` or (Magic Dart
     style) `PoweredStaff` max hit, the spell's speed and range (plus the stance's range delta,
     capped at 10), the spell's modifiers and on-hit effect, flags `spell:<id>` and
     `spellbook:<book>`. A spell that requires a target attribute (Crumble Undead) is refused
     otherwise.
   * **A powered staff's** own attack if the weapon is powered.
   * **Otherwise the weapon**: damage type from the stance, else ranged if the weapon uses ammo or
     is thrown, else slash. If the weapon uses ammo, the ammo slot must hold the matching family
     (adds `ammo:`, `ammo_item:` and `ammo_weight:` flags). If the special is armed and affordable,
     fold it in: its modifiers, hit count, `always_hits`, damage type, max hit and speed
     overrides, and the `special` flag. Ranged range is clamped to 1..=10.
4. Resolve scaling modifiers (Dharok's, twisted bow) against the player's HP and the target's
   Magic.
5. Return an `AttackPlan { definition, speed, range, style, special_cost }`.

`AttackPlan::finish(distance)` sets the projectile travel ticks and wraps the definition in `Rc`.

`expected_dps` runs `analyze_attack` on the plan and returns hit chance, max hit, swing interval
and damage per second. The `simulate` binary prints it.

### The `Regen` phase: `run_regen`

Once per tick for a living player:

1. Count down consumable delay groups.
2. HP regen (interval from Rapid Heal or a Hitpoints cape, and a Regeneration bracelet).
3. For each of the five combat stats off its base, count toward the drift interval (Preserve
   slows boost decay) and move one level toward base when due.
4. Prayer drain: sum the active prayers' drain effects, feed the `PrayerDrainCounter` with the
   gear's prayer bonus; at 0 points switch every prayer off.
5. Run energy (only when `run_energy_enabled`): drain on a tick spent running, regenerate
   otherwise; count stamina down.
6. Special energy +10 every 50 (or 25) ticks.
7. If any stat changed, `refresh_stats`.

### `after_damage`

* **Phoenix necklace**: at or below 20 % HP, heal 30 % of max, clear the whole damage buffer and
  remove the necklace.
* **Redemption**: below 10 % HP with Redemption on, heal a quarter of the Prayer level and drain
  all prayer points (switching prayers off).

### Skills and inventory

* [`Skill`](../osrs_core/src/model/skill.rs) lists every skill in the game's own stat order, so
  `index()` is the stat number the client's scripts use (Woodcutting is 8).
* [`Skills`](../osrs_core/src/sim/skills.rs) keeps base and current levels and experience for
  every skill (`[i32; Skill::COUNT]`). Experience is in **tenths** of a point, as the game keeps
  it (an oak log's 37.5 XP is 375). Setting a level sets the experience to that level's threshold
  (`rules::xp_tenths_for_level`); `add_xp` raises the level when a threshold is crossed. Boosts
  never lower a stat; drains floor at 1; restores stop at base; `combat_level()` uses base levels.
* **`World::grant_xp(skill, tenths)` is the one way experience is earned.** It emits
  `WorldEvent::Experience` (for XP drops) and, on a level up, `WorldEvent::LevelUp` plus the
  game's "Congratulations, you've just advanced a ... level." message.
* [`Inventory`](../osrs_core/src/sim/inventory.rs) is 28 `Option<ItemStack>` slots. Stackable
  items (ammunition) join an existing stack; everything else takes one slot per item. `add`
  returns how many did not fit.

### Gathering skills: [`sim/gathering.rs`](../osrs_core/src/sim/gathering.rs)

Woodcutting today; mining and fishing are meant to be more data plus at most a new depletion
rule. Everything a resource does differently is data:

| File | Holds |
|---|---|
| [`data/tools.json`](../osrs_core/data/tools.json) | `ToolDef`: item, category (`"axe"`), skill, level, the animation the player loops |
| [`data/gathering.json`](../osrs_core/data/gathering.json) | `GatheringNodeDef`: the option that starts it (`"Chop down"`), the scenery ids and what each becomes while depleted, requirements, tool category, roll interval, XP, product, the per-tool `[low, high]` success chances, depletion rule, respawn range, pet, messages |

`Requirement` (level, tool of a category, free slots) is checked when the player clicks, on
arrival and before every attempt; its refusal is the game's message, overridable per entry.

The loop runs in the player's PlayerQueue phase, from the **one action slot**
`Player::action` (`PlayerAction::Gather`); walking, attacking or talking to an NPC clears it:

1. walk until orthogonally next to the object;
2. on arrival say the start message and start the best usable tool's animation (`best_tool`:
   the highest success chance at the player's level);
3. roll whenever the **skill clock** is 0: it is set to the node's interval (4 ticks for trees)
   and counts down every tick, so re-clicking never resets it;
4. a success gives the product and XP, rolls the pet (1 in `base - 25 x level`; pets are items
   only for now), and depletes the object if the rule says so. `EverySuccess` fells a regular
   tree with its one log; `Timer { ticks }` is the oak-and-up despawn timer (runs down while
   chopped, back up while not, fells on the first success after it runs out).

The success chance is `rules::success_numerator(low, high, level)` over 256 (the wiki's skilling
success formula). Draws come from the player's own `skilling_rng`, never the encounter's, so
gathering cannot change a fight's rolls.

### Ground items: [`sim/ground_items.rs`](../osrs_core/src/sim/ground_items.rs)

`GroundItems` holds stacks (`uid`, item, quantity, tile) that despawn after `DESPAWN_TICKS` (300:
the game's 1 minute private + 2 public; there is one player, so no owner). "Drop" puts the stack
on the player's tile, a burnt-out fire leaves ashes, a stackable stack merges with the one on its
tile. `GroundItemOption { uid, option: "Take" }` is `PlayerAction::Take`: walk onto the tile,
then pick up (a full inventory says "You don't have enough inventory space to hold that item.").
The snapshot lists every stack as `ground_items`.

### Firemaking: [`sim/firemaking.rs`](../osrs_core/src/sim/firemaking.rs)

`UseItemOnItem { used, target }` with a tinderbox and a log (either way round) checks the level
(`data/firemaking.json`: `LogDef { item, level, xp_tenths, burn_ticks, bonfire_ticks }`), **drops the log on the
player's tile** and starts a `PlayerAction::Light` on that stack. `UseItemOnGroundItem` and the log's
"Light" ground option (`GroundItemOption`) do the same for a log already lying somewhere, walking
onto it first. The first attempt comes 3 ticks after the starting tick and then one every 4 on
the skill clock; the chance is `success_numerator(64, 512, level)` (65/256 at level 1, 100% from
43). A success removes the stack, grants the XP, spawns a fire (`SceneryState::spawn`, object
26185, lasting the log's `burn_ticks`) on the player's tile and steps the player west, else east,
south, north; the fire leaves ashes when it goes out. Walking away leaves the log lying there.
Spawned objects are in the snapshot's `spawned` list; a fire stops lighting on its own tile. Not
modelled yet: bow firemaking, coloured logs, other tiles that refuse a fire, permanent fires
becoming bonfires.

**Bonfires (Forester's campfires, object 49927).** `UseItemOnObject { slot, id, tile }` with a log
on a spawned fire (or bonfire) starts a `PlayerAction::Tend`; a bonfire's "Tend-to" does the same
with whichever log the inventory has first (`player_interact_object` looks at spawned objects
before the map's). The player walks to within 1 tile (diagonals count), adds a log after 1 tick
and then every 9, for the log's full XP. The fire it was is replaced by a bonfire lasting 99 +
`bonfire_ticks` (= the wiki's duration column); each further log adds `bonfire_ticks`, and a log
that would take it past 300 is not added. Starting one needs a tinderbox and no bonfire within
5 tiles (Chebyshev); tending needs none. "Check" says one of five messages by `(left + 1) / 60`.
A burnt-out bonfire leaves ashes. "Rest" is not modelled (it answers "Nothing interesting
happens."), the tend animation reuses the lighting one (733), and the "out of logs" message is
invented. Client: a picked inventory item clicked on a fire sends `UseItemOnObject`
(`ground_click::use_target`).

### Scenery: [`sim/scenery.rs`](../osrs_core/src/sim/scenery.rs)

* `SceneryIndex`: which object (`Loc { id, tile, kind, orientation, size }`) covers which tile.
  The server's map loader fills it with each square's ground-level free-standing objects (shapes
  10 and 11) as it loads the square's collision (`square_map` in `main.rs`).
* `SceneryState`: placements shown as another object for a while (`LocChange`, a stump for a
  felled tree), counted down at the end of every tick by the world clock, plus the despawn
  timers. The whole set goes to the client in every `Snapshot::scenery`.

### Animation and graphic ids

[`sim/animations.rs`](../osrs_core/src/sim/animations.rs) maps the wielded weapon's category (or
name) and the stance's damage type to cache animation ids for the player's attack and block
(punch 422, whip 1658, bow 426, ...). A shield replaces an unarmed block with 1156. It also maps
arrow tiers to the projectile graphic and the "drawing the bow" graphic. These ids go into the
snapshot so the client can play the right animation.

### Sounds

`WorldEvent::Sound { sound, from, delay_ticks }` names a cache sound effect (archive 4, see
[chapter 8](08-cache-decoding.md)) heard from a fighter. `World::play_sound` records one next to
the event it belongs to:

| When | Sound | Where the id comes from |
|---|---|---|
| the player attacks | the swing, or a spell's cast | `StanceDef::sound`, else `WeaponDef::attack_sound` (bare fists: `unarmed_punch`); `SpellDef::cast_sound` |
| a player spell lands | its impact, or `spellfail` when it splashes | `SpellDef::hit_sound`; delayed by the flight |
| a monster attacks | as the attack is thrown; a telegraphed one as its wind-up starts | `NpcAttackDef::sound`, else `NpcDef::sounds.attack` (both end up in `AttackDefinition::sound`) |
| a monster's attack resolves | its roll-tick sound, if it has one: Jad's ranged stomp (`firewave_hit`, 163) is heard on the roll, 3 ticks after the stomp animation starts, not at the wind-up | `NpcAttackDef::resolve_sound` (`AttackDefinition::hit_sound`, played in `fire_at_player`) |
| a hit lands on a monster / the player | its hit sound / `human_hit` (513, the male set; 510 is female), or `human_block_1` (511) for a 0 | `NpcDef::sounds.hit`; `sim::sounds` |
| something dies | its death sound / `human_death` | `NpcDef::sounds.death`; `sim::sounds` |
| a prayer goes on / off / runs dry | its own / `cancel_prayer` / `prayer_drain` | `PrayerDef::sound`; `sim::sounds` |
| food or a potion is used | its own, else `eat` (2393) / `liquid` (2401) by `delay_group` | `ConsumableDef::sound`; `sim::sounds` |

[`sim/sounds.rs`](../osrs_core/src/sim/sounds.rs) holds the game's own ids, named as Jagex names
them. The ids in the data come from the OSRS Wiki's "List of sound IDs" (Jagex's config names);
each data file's notes say which are guesses. A test checks that every id the data names is a synth
sound in the installed cache.

---

## 6.6 The `Server`

File: [`sim/server.rs`](../osrs_core/src/sim/server.rs).

### `Server` (the synchronous core)

```rust
pub struct Server {
    world: World,
    driver: TickDriver,
    outbox: Vec<String>,                 // messages from command handling
    next_player_id: u32,
    session: Option<Session>,            // one logged-in player at most
    map_streamer: Option<MapStreamer>,   // Box<dyn FnMut(&mut CollisionMap, TileCoord)>
    examines: ExamineTexts,
    object_names: HashMap<u32, String>,
}
```

| Method | Effect |
|---|---|
| `new(world, driver)` | |
| `handle_command(cmd)` | validate and apply one `ClientCommand` |
| `advance(dt) -> Option<Snapshot>` | feed `dt` seconds to the driver; for each tick it runs, call the map streamer with the player's tile then `world.run_tick()`; return a snapshot if any tick ran |
| `snapshot()` | build a `Snapshot`, draining messages and events |
| `set_map_streamer(f)` | install a hook that loads collision around the player before every tick (it is also called once immediately) |
| `set_object_names(map)` | names of scenery objects, for examine texts |

The rest of the codebase cannot mutate the world through a `Server`: `world()` only returns `&World`.

### Commands

`ClientCommand` is an enum (primer 1.5). Grouped:

| Group | Variants |
|---|---|
| session | `Login { name }` |
| movement | `WalkTo { tile, force_inverse }`, `SetRunning(bool)` |
| NPCs | `Attack { target }`, `InteractNpc { target }`, `ExamineNpc { target }` |
| scenery | `ExamineObject { id }`, `InteractObject { id, tile, action }` (the server finds the placement covering `tile` and dispatches the option: gathering nodes today, "Nothing interesting happens." otherwise) |
| items | `Equip`, `Unequip { slot }`, `InventoryOption { slot, option }`, `MoveInventoryItem { from, to }`, `UseConsumable`, `ExamineItem` |
| combat setup | `SetStance`, `TogglePrayer`, `SetPrayer(ProtectionPrayer)` (direct, no drain), `SelectSpell`, `SetSpecial`, `SetOnTask` |
| pacing (dev only) | `SetPaused`, `RequestSingleStep` |

`handle_command` is one `match`. Examine and pacing commands work even when the player is dead;
the guard arm `_ if self.world.player.is_dead() => Ok(())` silently drops everything else. Attacks
on unknown or dead targets are dropped too. Any `Err(String)` from the world is pushed to the
outbox, so the player reads why a command was refused.

**Login.** The server assigns a `PlayerId` from a counter starting at 1 (never reused), prints
`[server] player "name" logged in: ...`, and pushes "Welcome, name.". A second login on the same
server is refused. `PlayerId` names the account/session and is distinct from the world's
`EntityId(0)` for the player actor. Commands are not yet gated on being logged in.

### `Snapshot`

Plain owned data:

| Field | Contents |
|---|---|
| `tick`, `tick_seconds`, `paused` | timing |
| `player_id` | `None` until logged in |
| `player: PlayerSnapshot` | tile, **step_tiles** (the tiles stepped through last tick, in order), health, `is_dead` (confirmed deaths only), prayer, attack and face targets, `facing_loc` (the object it works at), `action_animation` (the sequence it loops there), running, the player's attack/block animation ids, combat level, every skill with its XP, equipment, the 28 inventory slots and the worn items as `ItemView { id, name, quantity }`, active prayers, stance, spell, special energy, run energy %, poisoned/venomed/frozen |
| `entities: Vec<EntitySnapshot>` | per NPC: id, name, tile, step_tiles, health, `is_dead`, size, `appearance` (cache NPC id), combat animations, `extra_animations` (attack and heal sequences), data id, and `facing` (the player it fights or the monster it heals) |
| `events` | `WorldEvent`s since the last snapshot |
| `messages` | outbox lines followed by the world's messages |
| `scenery` | every `LocChange` in force (felled trees as stumps), the whole set each time |

### `spawn_server` and `ServerHandle`

`spawn_server(build)` creates two channels, starts a thread, and on that thread:

1. calls `build()` to construct the `Server` (it must be built there because `World` holds `Rc`s
   and cannot cross threads);
2. sends an initial snapshot;
3. loops forever: drain every waiting command into `handle_command`; measure real time since the
   last loop; `advance(dt)`; send a snapshot if a tick ran; sleep 5 ms.

It returns a `ServerHandle` with `send(command)` and `drain_snapshots()` (everything that arrived,
oldest first). Dropping the handle closes the command channel; the server notices the
disconnection and the thread ends.

### Examine texts

[`sim/examine.rs`](../osrs_core/src/sim/examine.rs): scenery examine texts come from
`data/object_examines.json` (by object id, then by lower-case name); failing that,
`ExamineTexts::plain(name)` produces "It's a/an <name>." NPCs use their definition's `examine`
or the same plain form. The client only ever sends an id, and the text is the server's to decide.

---

## 6.7 Scenarios and the `simulate` binary

[`sim/scenarios.rs`](../osrs_core/src/sim/scenarios.rs):

* `dev_open_floor_world(half_extent, seed, start, config)` builds a world on a flat open floor
  `2 * half_extent` tiles square centred on the origin (`dev_open_floor_world_at` centres it
  elsewhere). Tests use it constantly.
* `build_scenario(registry, &ScenarioDef)` builds a full fight from JSON **by issuing the same
  calls a client would**: equip each item, set the stance, select the spell, toggle each prayer,
  drink each potion (clearing delays between them), set the slayer task, arm the special, spawn
  each NPC (optionally hostile), and set the attack target. Anything the rules refuse is an error,
  so a scenario file can never quietly describe a different fight from the one it claims.
* `build_scenario_on(registry, &ScenarioDef, map)` does the same on a real map: everything is
  offset by the scenario's `location`. The game client's `--scenario` uses it (chapter 7.1);
  `scenario_start` says where the player enters.

[`src/bin/simulate.rs`](../osrs_core/src/bin/simulate.rs):

```
cargo run -p osrs_core --bin simulate -- --list
cargo run -p osrs_core --bin simulate -- jad --ticks 40
cargo run -p osrs_core --bin simulate -- whip_vs_zombie --ticks 60
cargo run -p osrs_core --bin simulate -- my_fight.json --data ./my_data --quiet
```

It loads the registry (bundled or `--data` folder), builds the scenario, prints the player's
combat level and expected DPS against each monster, runs ticks printing new log lines and the
world's one-line snapshot each tick, and stops early if the player dies.
