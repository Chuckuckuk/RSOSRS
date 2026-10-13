# Rust ↔ C# parity plan

Goal: bring `osrs_core` to behavioural parity with the Unity project's `Osrs.Core` (and the game logic
inside `Osrs.Runtime`), minus rendering/input. C# sources:
`C:\Users\kucku\Documents\Unity Projects\Crafting Inventory System Demo\Assets\Osrs`.
Reference copies of the Unity markdown docs live in `../unity_context/`.

## Design decisions

1. **No shared references.** A `World` owns every entity; things refer to each other by ID. Core types
   take what they need as arguments (`Mover::step(&mut self, map: &mut CollisionMap, ..)`) instead of
   storing references. A queued attack stores the attacker's ID and `advance` is given a way to ask
   whether that attacker is dead. No `Rc<RefCell<..>>` in core logic.
2. **`TickScheduler` is generic over an ID type**; `run_tick` calls a dispatch function per (phase, id).
   Keeps C# behaviour: register/unregister deferred until the next phase starts, insertion-stable order
   within a phase, NPCs move before players.
3. **RNG.** `Rng` trait with (a) a port of .NET's seeded `System.Random` (so the same seed gives the same
   rolls in both engines — required for the golden-trace check) and (b) `ScriptedRng` for tests
   (mirrors C# `ScriptedRandom`).
4. **Callbacks / shared definitions.** `Func<>` conditions/weights → `Box<dyn Fn(..)>`.
   `AttackDefinition` / `HomingPulseOrbDefinition` shared via `Rc`. C# events → boxed closures.
5. No external dependencies; `OccupancyMask` / `CollisionFlag` are hand-written bitflag types.
6. Every ported file brings its C# EditMode tests with it (inline `#[cfg(test)]`).

## Port order

| # | Module | C# sources | Status |
|---|---|---|---|
| 0 | tiles, combat math/stats/style | `TileCoord`, `Direction`, `CombatMath`, `CombatStats`, `AttackStyle` | done |
| 1 | Foundation | `ICoreLog`, `ActorKind`, `Health`, `PrayerPool`, small enums, `Rng` | done |
| 2 | `tick` | `TickPhase`, `TickAccumulator`, `TickScheduler` | done |
| 3 | `collision` | `CollisionMapData`, `CollisionMap`, `CollisionFlag`, `OccupancyMask` | done |
| 4 | `pathfinding` | `TargetSpec`, `BfsPathfinder`, `PathFollower` | done |
| 5 | `actors` | `Actor`, `Mover`, `ICombatTarget` (trait) | done |
| 6 | Combat data | `StatusEffectDefinition`, `ReactionRequirement`, `AttackDefinition`, `HomingPulseOrbDefinition`, `AttackSelectionContext` | done |
| 7 | Combat resolution | `ReactionResolver`, `DamageResolver`, `HealResolver`, `DamageBuffer`, `StatusEffectTracker`, `PendingAttackQueue`, `PlayerReactionState` | done |
| 8 | Patterns / NPC AI | `IAttackPattern` + 3 patterns, `NpcCombatState`, `HomingPulseOrb` | done |
| 9 | Movement policies | Stationary, ChaseToMeleeRange, MaintainDistance, StyleReactive | done |
| 10 | `encounters` | `EncounterContext`, `Phase`, trigger types/evaluator/binding | done |
| 11 | `interaction` | `MenuOption`, `MenuEntrySwapper`, `IInteractable` | done |
| 12 | `sim::World` | logic from `BossCombatant`, `NpcCombatant`, `HealerNpc`, `PassiveNpc`, `PlayerMover` | done |
| 13 | Encounter builders | `DevOpenFloorCollisionMap` done; `JadEncounterSetup`, `PurutuEncounterSetup` **deferred by request** | partial |
| 14 | `TickDriver` | `TickRunner` pause/step/catch-up logic | done |
| 15 | Golden traces | Unity EditMode test dumps per-tick JSON for seeded runs; Rust integration test diffs | deferred (needs Jad/Purutu or another shared scenario) |

Not ported (rendering/input, belongs to the later Bevy step): camera, `ActorView`, `TileGridView`,
`TileHighlightView`, `CursorTileTracker`, `InteractionMenuController` drawing, menu-preference persistence.
`WorldTile`'s Vector3 conversion is ported as plain-float `world_to_tile` / `tile_to_world_center`.

## Deliberate deviations from the C#

- `ICombatTarget` becomes `CombatTargetView` (read-only snapshot) plus the entity's own `DamageBuffer`
  and `PendingAttackQueue`, passed separately as `&mut`.
- `PhaseTriggerKind` / `TriggerActionKind` + payload fields collapse into enums with payloads.
- Entities tick in `spawn_*` call order (the C# order follows Unity scene script order, which is not
  recoverable from code). The player and the encounter clock always register first.
- Dormant spawn groups (the C# inactive healers/elementals) are `SpawnBlueprint`s built at activation,
  because the C# builds their actor/mover (and marks occupancy) in `Awake`, which only runs on activation.

## Known C# quirks (no longer kept)

- (Fixed in Rust 2026-10-12, at the user's request that NPCs block one another like walls.) A
  multi-tile mover was blocked by its own occupancy marker when stepping south, west or south-west,
  because `Mover` marked only the SW tile while `CollisionMap.CanTraverse` checks the whole destination
  footprint. Rust now marks the whole footprint, lifts the mover's own mark while it plans a move,
  counts occupants per tile, and lets players through NPCs unless `blocks_movement` is set. The C#
  still has the old behaviour.

## After parity: the data-driven simulator (2026-10-09)

The port reached parity and then moved past it: the full OSRS ruleset in `OSRS_COMBAT_REFERENCE.md`
is now implemented from data. See `SIMULATOR.md` for the architecture, the data formats and the list of
places this intentionally diverges from the C# (attack timing, magic defence, monster accuracy).
The golden-trace check against Unity (item 15 above) would now need the C# side to adopt those same
corrections first.
