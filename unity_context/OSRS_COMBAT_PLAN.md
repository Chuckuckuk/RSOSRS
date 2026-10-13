# OSRS Combat & Encounter System — Implementation Plan

## North star

A boss fight is **authored as data, not code**. Anyone who can read an OSRS wiki page and translate
attack patterns into behaviour definitions should be able to build an encounter without writing C#.
The validation test (Stage 8) is literal: author three structurally different bosses as pure data.
Any C# required to do so is an architecture gap to close, not a boss to special-case.

## Scope

**In:** moment-to-moment combat within a single fight — attacks, telegraphs, reactions, phases,
movement behaviour, hazards, adds, encounter-local state.

**Out:**
- Anything persisting between fights (no run wrappers, banked/forfeited rewards, progression).
  Each fight spins up, resolves, tears down.
- Object/environment-interaction minigames (Wintertodt, Zalcano, Gauntlet prep room).
- Group-scaling and contribution-based loot (reward layer, not attack behaviour).

## Current state (updated 2026-10-04 — Stages 1-4 built, Jad authored as data, awaiting Play-mode verification)

**Jad is authored.** `Assets/Osrs/Runtime/Entities/JadEncounterSetup.cs` is the entire amount of
Jad-specific code Stage 4 allows: it constructs three `AttackDefinition`s (melee/ranged/magic, each
3-tick telegraphed, each requiring its matching protection prayer), wraps them in one
`WeightedRandomPattern` (melee weighted to 0 unless adjacent, ranged/magic always 1), bundles that
into a single `Phase`, and registers one `TriggerBinding` (`PhaseTrigger.HpThreshold(0.5)` →
`TriggerAction.SpawnEntities("jad_healers")`) before calling `BossCombatant.Configure(...)`.
`BossCombatant` (generic, Stage 3/4's boss driver) and `HealerNpc` have never heard of Jad - nothing
boss-shaped is hardcoded outside this one setup file. **Not yet done:** Play-mode verification (see
below) and a Stage 8-style narrative write-up; this is "Jad complete" in the architecture sense the
plan means, not yet confirmed running.

**Stage 1 (vertical slice)** — `Actor`/`Health`/`Mover`/`PathFollower`, `AttackStyle`/
`AttackRangeKind`/`AttackDefinition`/`NpcCombatState`/`DamageResolver`, `EncounterContext`.
`PlayerMover` is a thin Runtime adapter over `Actor`+`Mover`; `ActorView` binds any Actor-backed
Transform via `TickProgress01`; `NpcCombatant` is the original single-melee-attack chaser, kept as a
regression fixture (not the architecture Jad uses). NPC chase movement was corrected mid-session
from full-BFS to `Mover.StepTowards` (real OSRS "dumb" greedy chase, including corner-trap stalls) -
see [[project-npc-chase-movement]].

**Stage 2 (reaction requirements)** — `ReactionKind`/`ReactionRequirement`/`ProtectionPrayer`/
`PlayerReactionState`/`ReactionResolver`; `AttackDefinition.Reaction` (default `None`).
`ReactionResolver.GetDamageMultiplier` returns `attack.PrayerPenetration` when the correct prayer is
active (0 = normal full block, 1 = "ignore"), `1f` otherwise - prayer only ever helps, never hurts.
**Corrected the plan's own stale note here:** the multiplier is `PrayerPenetration` itself when
prayer is correct, not `1 - PrayerPenetration` as an earlier draft of this doc said - the
`AttackDefinition.PrayerPenetration` doc comment and [[project-osrs-damage-calc-research]] were
already right; this paragraph just hadn't been updated to match. `DamageResolver` gained a
`damageMultiplier` overload (3-arg original still defaults to `1f`, so no existing call site broke).
Dev-only prayer-switching input (1/2/3/0 keys) lives on `PlayerMover` until a real prayer-book UI
exists.

**Verified against the wiki, then refined into three named timing archetypes (user-requested,
2026-10-04):** real OSRS only checks the active protection prayer at the exact tick an attack's
accuracy/damage roll happens - never sampled across the telegraph ("prayer flicking": wrong prayer
for the whole telegraph still blocks the hit if switched correct exactly on the resolve tick;
correct prayer held the whole telegraph still eats full damage if dropped one tick early). The
production code already only ever called `ReactionResolver.GetDamageMultiplier` from the
resolve-tick branch - not a bug, just unverified. But the user's follow-up categorized real NPC
attacks into three distinct timing shapes the original single-telegraph state machine couldn't all
express:
1. **Traditional** (most NPCs) — roll happens the instant the attack is registered, no reliable
   telegraph; prayer must already be up when the animation begins.
2. **Jad-style** (TzTok-Jad) — multi-tick telegraph, then the roll happens at the END of it, same
   as Traditional from that point on. This was `NpcCombatState`'s only shape before this refinement.
3. **Doom-style** (Doom of Mokhaiotl) — little/no telegraph, but a visibly style-distinguishable
   projectile/effect whose roll is DEFERRED until it actually lands, giving a full late-reaction
   window during the travel time itself, not just a telegraph.

All three are now expressible with two `AttackDefinition` fields: `TelegraphTicks` (unchanged) and
new `ImpactDelayTicks` (default 0 - Traditional/Jad-style; >0 means Doom-style's roll is deferred
until impact). `PrayerCheckTimingTests.cs` now covers all three archetypes end-to-end. Jad's own
three attacks (`JadEncounterSetup.cs`) are unchanged - they're genuinely Jad-style, `ImpactDelayTicks`
defaults to 0. See [[project-osrs-damage-calc-research]] for the wiki citations and the archetype
taxonomy recorded for future bosses (e.g. a real Doom of Mokhaiotl encounter, Stage 8+, would use
`ImpactDelayTicks` for its ground slams).

**Moved the Doom-style travel delay off the attacker entirely (user-caught design flaw,
2026-10-04):** this was first built as `NpcCombatState.Traveling`, a state owned by the
*attacker's* own state machine - wrong, because an already-launched attack's fate then depended on
the attacker continuing to exist and continuing to be ticked every frame. Concretely: both
`BossCombatant.RunBossAi` and `NpcCombatant.RunNpcAi` early-return the instant `actor.Health.IsDead`
is true, which means if the attacker died mid-travel, `NpcCombatState.Advance` would simply never
be called again and the already-launched, already-telegraphed shot would silently vanish instead of
landing. `NpcCombatState` is back to a 2-state machine (`Idle`/`Telegraphing` only, no `Traveling`)
and `Advance` now always resolves the instant a telegraph ends, regardless of `ImpactDelayTicks` -
the caller reads `CurrentAttack.ImpactDelayTicks` and, if >0, hands the attack to a new
`Combat/PendingAttackQueue.cs`, owned by the **target**, not the attacker (same ownership pattern as
`DamageBuffer`). The attacker is immediately free to select its next attack; the in-flight shot's
roll (reading the *defender's* live `PlayerReactionState`, never the attacker's) happens entirely on
the target's own tick once the delay elapses, then feeds the result into that same target's
`DamageBuffer` - composing correctly even for an attack with both a roll-delay and its own
post-roll `ProjectileTravelTicks`. All three attacker call sites (`PlayerMover`, `NpcCombatant`,
`BossCombatant`) and the Doom-style tests (`PrayerCheckTimingTests.cs`,
`PendingAttackQueueTests.cs` (new)) were updated to match; a `FakeCombatTarget.cs` test double was
added since exercising `PendingAttackQueue` needs a real `ICombatTarget`, not just primitives.

**Then explicitly preserved the one outcome of the old bug that was actually wanted
(user follow-up, same day):** the old, fragile mechanism happened to produce a specific result the
user likes - an attacker killed before its Doom-style shot lands takes that shot down with it,
rather than it hitting from beyond the grave. `PendingAttackQueue.Enqueue` now also captures the
attacker's live `Health` reference (not a snapshot) alongside the attack and its launch tile;
`Advance` checks it at the exact resolve tick and drops the entry - no roll, nothing reaches
`DamageBuffer` - if the attacker has since died. This makes the behavior an explicit, intentional
check instead of an accident of the attacker's own AI loop having stopped calling in, so it no
longer depends on *why* the attacker stopped ticking (which was the actual bug - a disabled or
pooled attacker could orphan a shot too, not just a dead one). All three attacker call sites now
pass `actor.Health`; `PendingAttackQueueTests.cs` gained two tests covering death-mid-flight and
already-dead-at-launch.

**Started the Elysian Wilds gorilla trio (user-sourced from New World's Purutu/Igniscia/
Aguascia/Aeriscia, 2026-10-04):** three OSRS-authored gorilla bosses reskinning New World's barebones
gorilla elites, each with a distinct style pairing (Igniscia melee/magic, Aeriscia melee/ranged,
Aguascia pure ranged-AoE-and-never-approaches) rather than one shared moveset with cosmetic-only
elemental flavor. This is what prompted Stage 6 (see below) and the Stage 5 deferral decision.

**Purutu (Phase 1) is authored as data** (`Runtime/Entities/PurutuEncounterSetup.cs`), following the
full design doc the user supplied (`ENJO.md`, not yet committed anywhere else - treat it as the
source of truth for this encounter's numbers/behavior going forward). Four attacks in one
`WeightedRandomPattern`: Melee (adjacent-only) and Ranged (non-adjacent-only, high prayer
penetration - "encourages the player to stay near him"), plus two specials that both apply
Corruption on hit - Ground Smash (a `MoveToTile` reaction anchored to the TARGET's tile at
telegraph-start, not the attacker's - see the new `HazardAnchor`/`NpcCombatState.
SelfTileAtSelection`/`TargetTileAtSelection` below) and Slicer (spawns a `HomingPulseOrb`, see Stage
7). Movement is `ChaseToMeleeRangeMovementPolicy` ("attempts to approach at all times despite being
capable of ranged attacks"). A `PhaseTrigger.WhenEntityDied()` → `TriggerAction.SpawnEntities
("elemental_trio")` binding hands off to Phase 2 once Purutu dies - fixed a real latent bug to make
this work: `BossCombatant.RunBossAi` was hardcoding `trackedEntityDied: false` always, so
`WhenEntityDied` triggers could never fire for ANY boss before now (unexercised, since Jad never
used one). The three elemental GameObjects this spawns are wired as inert placeholders
(`elementalsToSpawn`, same `SetActive(true)` stand-in Jad's healers use) - their own
`BossCombatant`/`EncounterSetup` content is Phase 2 work, not built this pass.

**New primitive: hazard-anchor tile capture.** `NpcCombatState` now snapshots `SelfTileAtSelection`/
`TargetTileAtSelection` once, at the exact Advance call that STARTS a telegraph, never updated again
until the next selection. `ReactionRequirement.MoveToTile` gained a `HazardAnchor` parameter (`Self`
- unchanged default, or `Target`); the caller (`NpcCombatant`/`BossCombatant`) picks whichever
snapshot the attack's anchor calls for instead of a live tile. Needed the instant Ground Smash
("smashes the area around his target") specified a hazard whose position is fixed at cast time, not
wherever the target currently stands - without this, moving away during the telegraph would never
actually dodge it.

**New primitive: status effects + a prayer-points resource**, reused across the whole encounter per
`ENJO.md` (Corruption, Igniscia's burn, Aeriscia's run-disable, Aguascia's stun), not built
Purutu-only. `Combat/PrayerPool.cs` mirrors `Health`'s shape (clamped Drain/Restore). `Combat/
StatusEffectKind.cs` + `StatusEffectDefinition.cs` + `StatusEffectTracker.cs`: pluggable-by-Kind data
(matching `ReactionRequirement`/`TriggerAction`'s pattern), refresh-not-stack semantics per Kind,
`IsRunningDisabled`/`IsStunned` flags. `AttackDefinition` gained `OnHitStatusEffect` (nullable),
applied via a new `DamageBuffer.Enqueue`/`Advance` parameter at the exact tick a hit actually LANDS
(not when rolled) - correct even for a travelling/delayed hit. `PlayerMover` now owns `Prayer` +
`StatusEffects`, advanced last each tick (after `DamageBuffer`, so a debuff landing THIS tick also
pulses this tick); `RunPlayerQueue` no-ops while stunned, `RunPlayerMovement` forces walk speed while
running is disabled.

**Added a real damage buffer/queue, researched via the Phoenix necklace (user-requested,
2026-10-04):** the prayer-timing refinement above conflated "when is the roll checked" with "when
does HP actually change" - real OSRS separates these for ranged/magic: Traditional and Jad-style
attacks check prayer at the roll tick (telegraph-end), but the already-fixed damage amount then
sits in the defender's **attack buffer** while the projectile physically travels, landing (and only
then reducing HP) some ticks later. Confirmed this is a real, named, inspectable mechanic via the
Phoenix necklace's wiki page: its auto-heal doesn't just cap the ONE hit that procs it, it **clears
the defender's entire buffer**, which is why an early sub-hit of a multi-hit special (dragon claws,
dark bow) triggering it cancels the later sub-hits outright even though they were already rolled.
Built `Combat/DamageBuffer.cs` (Core, tested) - a per-defender queue of `(amount, ticksRemaining)`
entries; `Enqueue` fixes the amount at roll time, `Advance` (called once per tick from each
combat-capable Runtime component's movement phase) applies whatever has finished travelling, and
`Clear` wipes the whole queue unconditionally (the Phoenix mechanic - nothing consumes it yet, no
player equipment exists in this project at all, but the buffer is shaped to support it without a
redesign later). Added `AttackDefinition.ProjectileTravelTicks` (default 0 - melee/Doom-style,
whose deferred roll already happens at landing) alongside the existing `ImpactDelayTicks` (which
defers the ROLL, not the apply). `DamageResolver` gained `Roll` (rolls the amount, doesn't touch
Health) alongside the unchanged `Resolve` overloads; every attacker in the project (`PlayerMover`,
`NpcCombatant`, `BossCombatant`) now rolls then enqueues via the target's `ICombatTarget.DamageBuffer`
instead of applying directly, even for 0-travel-tick (melee) hits, which still land the same tick
thanks to the tick-phase ordering (decision phases 0/1 always precede movement phases 3/4 within
one `TickScheduler.RunTick()` call). Jad's ranged/magic attacks (`JadEncounterSetup.cs`) now set
`projectileTravelTicks: 1` to demonstrate this live - melee stays 0. `DamageBufferTests.cs` covers
enqueue/advance/multi-hit/clear in isolation. See [[project-osrs-damage-calc-research]] for the
Phoenix necklace citation.

**Stage 3 (attack selection)** — `AttackSelectionContext` (HP%, elapsed ticks, per-NPC counter,
`SelfTile`/`TargetTile`/`TargetCombatStyle`), `IAttackPattern`, `FixedRotationPattern` (Vorkath-style
cycle), `WeightedRandomPattern` (live-state-evaluated weights, Vardorvis-style), `ConditionalPattern`
(counter/time-branching, Zulrah-style). `NpcCombatState` now holds a pattern + its own selection
counter instead of one fixed `AttackDefinition`, with `SetPattern` for Stage 4's phase swaps.
`EncounterContext.ElapsedTicks` is now actually wired (`DevOpenFloorCollisionMap` advances it on
`TickPhase.Regen`, the last phase) - it existed since Stage 1 but nothing called `AdvanceTick()`.

**Stage 4 (phases & triggers)** — `Phase` (name + `IAttackPattern`; deliberately minimal - no
hazards/movement-policy fields yet, since Stages 5/6 don't exist), `PhaseTrigger`/`PhaseTriggerKind`
(`HpThreshold`/`ElapsedTicks`/`EncounterFlag`/`EntityDied`), `TriggerAction`/`TriggerActionKind`
(`SwapPhase`/`SpawnEntities`/`ModifyStats`/`SetFlag` - `ModifyStats` is recognised-but-unimplemented,
same stance as `ReactionKind.Interrupt`), `PhaseTriggerEvaluator` (pure predicate), `TriggerBinding`
(fire-once bookkeeping). `BossCombatant` (new, generic) is the Stage 3/4 driver: stationary (no
`IMovementPolicy` yet - Jad barely chases anyway), evaluates its `TriggerBinding`s every `NpcAi` tick
before advancing `NpcCombatState`, and exposes a `SpawnRequested` C# event since Core has no notion
of a prefab to instantiate - Runtime-side setup (`JadEncounterSetup`) subscribes and does the actual
`GameObject.SetActive`. `HealerNpc` (new) chases a `BossCombatant` via `Mover.StepTowards` and heals
it via the new `HealResolver` every tick it's adjacent and both are alive - no telegraph, matching
real Yt-HurKot. Healers are pre-placed in the scene disabled (`m_IsActive: 0`), not
`Instantiate()`'d, since there is no prefab pipeline yet; `SpawnId` is the seam a real one would
plug into later, unchanged.

**Player-attacks-NPC was also built this session** (the other half of invariant #5's bidirectional
interactions, previously only a forward-looking memory note - see [[project-bidirectional-interactions]]):
`ICombatTarget` (Core) + `PlayerMover.Instance` singleton + `PlayerMover.SetAttackTarget`/
`RunAttackTick`. Clicking an NPC (`NpcCombatant`/`HealerNpc`/`BossCombatant` all implement
`ILeftClickOption`+`ICombatTarget` now) chases it with the player's own real BFS pathing (NOT
`StepTowards` - that stays NPC-only) and auto-attacks on a fixed cooldown once adjacent. This is
required for Jad's "killing the healers stops the healing" criterion - without it there was no way
for the player to deal damage to anything.

**Scene wiring (`SampleScene.unity`):** `Capsule` has `ActorView`; `Npc` (Stage 1/2 regression
fixture) at tile (4,0); `Jad` (`BossCombatant`, footprint 5, 250 HP) at tile (10,10); `JadHealer1`/
`JadHealer2` (`HealerNpc`, disabled) at tiles (14,10)/(10,14); `JadEncounterSetup` (no visual) wires
them all together in its `Start()`. **Not yet done:** Play-mode verification of ANY of this - a human
needs to press Play and confirm: the `Npc` fixture still works (regression check); clicking `Jad`
or a disabled-until-spawned healer correctly drives the player into melee range and damages it; Jad
cycles all three attack styles with the matching prayer negating/piercing correctly; at 50% HP the
two healers activate, walk to Jad, and visibly heal it; killing both healers stops the healing. No
compile errors found via static review, but this entire batch (including the hand-authored `.meta`
GUIDs for every new script - Unity's own import pass has not run since before this batch) has not
been compiled by Unity itself yet.

## Pre-Stage-1 state (Stage 0 complete + unplanned polish)

**`Osrs.Core`** (asmdef: `noEngineReferences: true`, zero references — plain C#, enforced):
- `Tiles/` — `TileCoord` (tile centre at X+0.5/Z+0.5), `Direction` + fixed BFS expansion order
- `Collision/` — `CollisionFlag`, `OccupancyMask`, `CollisionMapData`, `CollisionMap`
  (directional two-sided walls, no corner-cutting, per-edge step height, split NPC/player occupancy)
- `Tick/` — `TickPhase`, `ITickable`, `TickScheduler` (explicit phase order, buffered registration),
  `TickAccumulator`
- `Pathfinding/` — `TargetSpec` (Exact/Rect/Adjacent/WithinRange), `BfsPathfinder`
  (128×128 re-centred window, zero-alloc, unreachable fallback gated to Exact, `TryStepTowards`
  for trailing follow)
- `ICoreLog` — diagnostic seam so Core can report without UnityEngine
- **Unchanged since the plan was written.** Every bit of this session's work lived in Runtime/scene —
  which is itself a warning sign, see "PlayerMover is overdue for its Stage 1 split" below.

**`Osrs.Runtime`** (references `Osrs.Core`, `Unity.InputSystem`), compiled and wired into
`Assets/Scenes/SampleScene.unity`, all verified running in Play mode:
- `TickRunner`, `WorldContext` (pre-existing) + `WorldTile` (world↔tile conversion) +
  `DevOpenFloorCollisionMap` (stand-in open 30×30 collision map + `BfsPathfinder`, still bakes
  nothing from geometry)
- `Entities/PlayerMover` — click-to-move. Grew well past Stage 0's original scope: run is now the
  default (no toggle key), Ctrl-click inverts run/walk for that one path only (`isRunning` itself
  never changes), exposes `CurrentTile`/`TryGetDestinationTile` for the highlight view
- `Input/CursorTileTracker` — singleton, one raycast per frame against the `Ground` layer only (layer
  3, already named in `TagManager.asset` — the floor was already on it) for the walk-here target,
  plus a second unrestricted raycast purely to detect `ILeftClickOption`
- `Input/ILeftClickOption` — one-method interface for a future NPC/object's click action to supersede
  walk-here. **Nothing implements it yet** — this is Stage 1's interaction hook, pre-wired
- `Debug/TileGridView` — single-mesh line grid over the floor, sized from the live `CollisionMap`
  bounds (not a duplicated constant)
- `Debug/TileHighlightView` — green/red/white tile-quad highlights for canonical / destination /
  hovered tiles, via `Graphics.DrawMesh` (no extra Renderers)
- `Camera/FollowCamera` — scroll zoom, arrow-key/middle-drag orbit (OSRS-style). Smooths a lagged
  *focus point*, not the final position directly, so rotation/zoom are instant and only movement
  causes drift — an earlier version baked rotation into the smoothed value and drifted on rotate

**`Osrs.Tests`** (EditMode; references **only `Osrs.Core`**): `CollisionMapTests`,
`CornerCuttingTests`, `OccupancyTests`, `BfsPathfinderTests`, `TickSchedulerTests`. **No new tests
since the plan was written** — everything built this session is Runtime-only and untestable as-is.

**Empty, intended as homes for upcoming work:** `Assets/Osrs/Data/` (ScriptableObject definitions),
`Assets/Osrs/Prefabs/`, `Assets/Osrs/Scenes/` (Stage 0 punted on moving off the crafting demo's
scene — still there), `Assets/Osrs/Editor/{Baking,Gizmos,Painting}/`.

**Scene:** `Assets/Scenes/SampleScene.unity` — floor `Cube` + `Capsule` (click-to-move working) +
`WorldContext` GameObject (`TickRunner`+`WorldContext`+`DevOpenFloorCollisionMap`+`CursorTileTracker`)
+ `Main Camera` (`FollowCamera`) + `TileGrid` + `TileHighlights`. Backup of the pre-Stage-0 state at
`SampleScene.unity.bak`.

**PlayerMover is overdue for its Stage 1 split.** It now owns click input, run/walk resolution,
ctrl-click inversion, path consumption, and tile-delta interpolation — all in Runtime, all untestable,
all exactly the logic Stage 1 describes moving into a Core-side mover + thin Runtime adapter. Every
turn this grows in Runtime instead is debt Stage 1 has to unwind.

**Not yet exercised:** the BFS unreachable-fallback and diagonal/obstacle-avoidance paths.
`DevOpenFloorCollisionMap` has zero blocked tiles, so there's nothing to deflect off of yet — the
open-floor happy path works, but the fallback behaviour described in Stage 0's "done when" is
unverified until an obstacle exists.

## Architectural invariants

1. **Logic in Core, adapters in Runtime.** `Osrs.Tests` cannot see `Osrs.Runtime`, so any logic in
   Runtime is permanently untestable. Runtime holds only: input polling, transform/animation
   binding, ScriptableObject→Core-data conversion, scene wiring.
2. **Core never references UnityEngine.** Enforced by `noEngineReferences`. Use `ICoreLog`, and
   plain C# types instead of `Vector3`/`Mathf`.
3. **Nothing reads `Time.deltaTime`** except `TickRunner`. Logic advances on `OnTick`; views
   interpolate from `TickProgress01`.
4. **Determinism.** Same inputs + same seed ⇒ same fight. Per-encounter seeded RNG, no
   `UnityEngine.Random`, no dictionary-iteration-order dependencies in resolution.
5. **Interaction requests are actor-agnostic.** NPC-attacks-player and player-interacts-NPC/object
   share one request abstraction (TargetSpec + action-on-satisfy), produced by `NpcAi` or
   `PlayerQueue` alike. `InteractionCheckPre/Post` already check arrival actor-agnostically.
6. **`TargetSpec` keeps independent Width/Height.** Deliberate divergence from OSRS's square-only
   integer NPC `size`, for NPC shape freedom. Do not "fix" toward square-only.
7. **Data-driven or it doesn't count.** If a mechanic needs a new `if` in the combat engine rather
   than a new data asset, the abstraction is wrong.

---

# Stages

Each stage leaves something runnable. Jad is complete at the end of Stage 4.

## Stage 0 — Unblock the in-flight click-to-move ✅ done (+ unplanned extras)

1. ✅ Unity imported `WorldTile.cs`, `DevOpenFloorCollisionMap.cs`, `PlayerMover.cs` — clean compile.
2. ✅ Camera added — ended up as the full `FollowCamera` (zoom + OSRS-style orbit), well past the
   "fixed angled camera" originally scoped here.
3. ❌ **Not done, deliberately deferred.** Still in the crafting demo's `SampleScene`, not
   `Assets/Osrs/Scenes/`. Revisit before this scene accumulates much more.
4. ✅ Scene wired: `WorldContext` GameObject holds `TickRunner`+`WorldContext`+
   `DevOpenFloorCollisionMap`+`CursorTileTracker`; `PlayerMover` is on the Capsule.
5. ✅ Orphaned `PlayerController` + the non-kinematic `Rigidbody` both removed from the Capsule (the
   Rigidbody would have fought `PlayerMover`'s direct transform writes under gravity).

**Unplanned, built in response to follow-up requests but worth tracking here since they're now load-
bearing:** tile grid + 3-colour tile highlighting (`TileGridView`/`TileHighlightView`); run-by-default
with Ctrl-click inversion; shared cursor raycasting split into a Ground-layer-only walk target and an
unrestricted `ILeftClickOption` check (this is Stage 1's "player interacts with NPC/object" hook,
arrived at from a bug fix rather than top-down design, but it satisfies invariant #5 early).

**Done when:** clicking the floor moves the Capsule one tile per 600ms along a BFS path (✅ confirmed
working); diagonals work (not explicitly exercised — nothing currently forces a diagonal-only path);
clicking unreachable/far tiles produces the partial-walk fallback (**unverified** — see below).

**Known gotchas:** the Capsule sits at exactly `(0,1,0)` but tile centres are at `X+0.5/Z+0.5`, so
there's a one-time 0.5-unit snap on first run (not reported as an issue, likely unnoticed/acceptable).
`DevOpenFloorCollisionMap` bakes nothing — it's a fully open 30×30 grid with zero blocked tiles, which
also means the unreachable-fallback path has nothing to trigger it yet. The real baker
(`Assets/Osrs/Editor/Baking/`) still needs to land before that's testable, and before Stage 5's
hazards need real arena geometry.

## Stage 1 — Vertical slice: one NPC, one attack, end to end ✅ built (Play-mode unverified)

Smallest complete loop: an NPC walks to the player, hits them, the player can die.

**Build (Core):**
- `Actors/Actor` — tile position, footprint size, occupancy mask, current/previous tile for
  interpolation, `Health`. Both player and NPC are `Actor`s. Deliberately general enough to cover
  non-combat entities later (barrier/shield objects), not just "things that fight".
- `Actors/Health` — current/max, damage/heal, death flag.
- `Encounters/EncounterContext` — the shared blackboard: participant registry, elapsed ticks, named
  flags/counters, seeded RNG. Minimal now; load-bearing from Stage 4 onward.
- `Combat/AttackDefinition` — target shape, telegraph duration in ticks, damage range, attack style
  (Melee/Ranged/Magic only - **not** a 4th "Typeless" value; see the OSRS damage-calc research
  note below), and `PrayerPenetration` (a float 0–1, pulled forward from Stage 2 once research
  showed "typeless" is 100% prayer penetration on a real style, not a style of its own - and that
  "pierce" (partial, e.g. Phantom Muspah's ranged attacks) and "ignore" (100%, e.g. Doom of
  Mokhaiotl's ground slams) are different points on that spectrum, not the same thing a bool
  could express).
- `Combat/NpcCombatState` — the telegraph→resolve state machine (idle → telegraphing N ticks →
  resolve), advanced in `NpcAi`.
- `Combat/DamageResolver` — applies a resolved attack to targets.

**Build (Runtime):**
- Split `PlayerMover`: Core-side mover/path-consumption logic + a thin Runtime adapter for click
  input and transform interpolation. (Current version puts untestable logic in Runtime.)
- `ActorView` — binds an `Actor`'s logical tile to a `Transform`, interpolating via `TickProgress01`.

**Phase wiring:** NPC → `NpcAi` (decide/advance attack state) + `NpcMovement` (step);
player → `PlayerQueue` + `PlayerMovement`.

**Done when:** an NPC paths to the player, telegraphs, resolves, player HP drops, death ends the fight.

**Tests:** telegraph resolves on the expected tick; damage application and `Health` clamping;
NPC-before-player movement ordering (`NpcsMoveBeforePlayers`) holds.

## Stage 2 — Reaction requirements ✅ built (Play-mode unverified)

Generalises "did the player respond correctly" past prayer specifically.

**Build:** `PlayerReactionState` (active protection prayer, current combat style) readable at the
resolve tick; `ReactionRequirement` as a pluggable type — `PrayerMatch` / `MoveToTile` / `Interrupt` /
`None`. (`AttackStyle` and `PrayerPenetration` already exist as of Stage 1 - see its build list. A
`PrayerMatch` check must apply `PrayerPenetration` itself as a damage multiplier when the correct
prayer is active (0 = normal full block, 1 = "ignore") - not `1 - PrayerPenetration`, and not just a
binary blocked/not-blocked - that's what makes pierce vs. ignore actually work.)

**Done when:** correct prayer negates, wrong prayer takes full damage, a piercing attack lands
regardless, and a move-away requirement resolves on distance rather than prayer.

**Unlocks:** prayer-piercing specials; forced-movement reactions (partial).

## Stage 3 — Attack selection strategies ✅ built (Play-mode unverified)

**Build:** `IAttackPattern.SelectNext(AttackSelectionContext)` where context carries NPC HP%, elapsed
ticks, a per-NPC counter, and player positions/styles. Implementations:
- `FixedRotationPattern` — hard-coded cycle (Vorkath)
- `WeightedRandomPattern` — weights evaluated from **live state**, so an HP threshold can shift
  frequency without swapping the moveset (Vardorvis)
- `ConditionalPattern` — branches on elapsed time or counter (Zulrah)

All draw from the encounter's seeded RNG, never `UnityEngine.Random`.

**Unlocks:** fixed deterministic rotations; time/counter-gated rotations; HP-reactive frequency.

## Stage 4 — Phases & triggers → **Jad complete** ✅ built and authored (Play-mode unverified)

**Build:**
- `Phase` — bundles available attacks, attack pattern, **movement policy**, active hazards, and
  vulnerability/immunity rules. Phase transitions swap the whole bundle, not just add spawns.
- `PhaseTrigger` — `HpThreshold` / `ElapsedTicks` / `EncounterFlag` / `EntityDied`.
- `TriggerAction` — `SwapPhase` / `SpawnEntities` / `ModifyStats` / `SetFlag`.

**Done when TzTok-Jad is fully authored as data:** three attack styles with telegraphs, weighted-random
selection, prayer-based reaction, healers spawned at an HP threshold, healers healing him while alive,
killing them stopping it. **No Jad-specific C#.**

**Unlocks:** wholesale phase swaps; healing-via-adds; spawn triggers.

## Stage 5 — Attack shapes & ambient hazards — deliberately deferred

Not built. Re-scoped after designing the Elysian gorilla encounter: this project is solo-scope (one
player, confirmed by [[project-scope-moment-to-moment-only]]), so an "area attack" only ever needs to
answer one question - "is the single player's tile covered" - which a **radius** shape already
answers today via the existing `ReactionRequirement.MoveToTile(moveAwayDistance)` +
`ReactionResolver`'s Chebyshev-distance check, with zero new code (this is exactly what Aguascia's
AoE ranged attack and a Ground-Pound-style move both need). `Line`/`Cone` shapes (for a Charge-style
attack) are NOT covered by that and remain a real gap - they need a direction fixed at telegraph
START (captured once, alongside `CurrentAttack`, inside `NpcCombatState`) checked against the
defender's LIVE tile at resolve time, which `ReactionRequirement`/`ReactionResolver`'s current
signature doesn't support. Build this, plus `Hazard` (ambient, independent-ticking effects), only if
a specific encounter actually needs a line/cone attack or a lingering room-wide effect - don't build
speculative shape plumbing the gorilla trio (as scoped) never exercises. **Open design question**
(still unresolved, carried forward from the original version of this section): if/when a Line/Cone
shape IS needed, should it extend `TargetKind`, or be a separate `ShapeSpec`? `TargetSpec` currently
means "where may I stand to satisfy this"; a hit-shape means "what does this cover" - same predicate
structure, different intent, so sharing the type is tempting but may muddy both.

**Unlocks (once built):** room/radius AoE; instance-wide ambient hazards; line/cone attacks.

## Stage 6 — Movement policies ✅ built (Play-mode unverified)

**Built:** `IMovementPolicy` (`Core/Actors/`) with four implementations - `StationaryMovementPolicy`,
`ChaseToMeleeRangeMovementPolicy` (the old hardcoded chase-then-hold-at-adjacency every NPC used to
duplicate inline), `MaintainDistanceMovementPolicy(preferredDistance)` (kites: steps away while
closer than preferred, towards while farther, holds at exactly that distance), and
`StyleReactiveMovementPolicy(kiteDistance)` (closes on a melee target, kites a ranged/magic one -
Yama's category-C mechanic, composing the two simpler policies rather than re-implementing either).
`MaintainDistance`/`StyleReactive` needed a genuinely new primitive - `BfsPathfinder.TryStepAway` +
`Mover.StepAway`, exact mirrors of `TryStepTowards`/`StepTowards` with the direction negated - since
nothing before this ever needed an NPC to retreat. `Phase` now carries `IMovementPolicy
MovementPolicy` (defaulting to `StationaryMovementPolicy.Instance`, so Jad's own Phase is unaffected
and needs no change). `BossCombatant` reads its policy from `currentPhase.MovementPolicy` each tick;
`NpcCombatant`/`HealerNpc` (which have no `Phase` concept at all) hold a direct `IMovementPolicy`
field instead, each still defaulting to `ChaseToMeleeRangeMovementPolicy` to preserve their exact
prior behavior. `PlayerMover` now implements `ICombatTarget` (it was missing a public
`FootprintSize` and the interface declaration) so a policy can target the player the same
actor-agnostic way an attack already does. **Deliberately deferred:** the "instant reposition"
primitive a Leap/Charge gap-closer would need (teleport-adjacent-then-hit) was part of the original
move-list research but isn't needed by the gorilla trio as currently scoped (Igniscia/Aeriscia keep
normal chase movement with a mixed-style moveset; Aguascia uses `MaintainDistance` and never closes
at all) - revisit if a future encounter wants an attack that relocates its attacker.

**Unlocks:** player-style-reactive positioning AI; kiting/ranged-variant NPCs.

## Stage 7 — Timers, sub-entities, linked state — partially built (Purutu's Slicer)

**Built:** the "sub-entities with their own resource pool, independent movement" bullet below,
specifically - `Combat/HomingPulseOrb.cs` + `HomingPulseOrbDefinition.cs` (Purutu's Slicer: a
levitating orb that homes toward its target, pulses an AoE hit every `PulseIntervalTicks`, and
extends its own `TicksRemaining` on a landed hit - "gains duration when it lands a hit"). Reuses
`AttackDefinition` for the pulse's own damage/style/reaction instead of duplicating those fields -
a pulse is just an ordinary attack whose "attacker position" happens to be the orb's current tile.
Deliberately NOT built on `Actor`/`Mover`/`CollisionMap` - it levitates, so a plain uncollided
greedy step toward the target each tick is correct, not a missing feature. Owned generically by
`BossCombatant` (any boss's `AttackDefinition.SpawnsOrb` can use one, not just Purutu's), advanced
every tick independent of whether the boss that spawned it is still alive or mid-telegraph on its
own next attack (same "launched effects outlive their launcher" principle as `PendingAttackQueue`).

**Not built:**
- `EncounterTimer` — a deadline + condition + consequence primitive, separate from telegraph-resolve:
  expresses DPS checks, enrage timers, and kill-window-or-reset (CoX Vanguards).
- A genuinely HP-budgeted (not just timed) sub-entity - Slicer uses a tick-countdown, not a
  drainable resource pool like TzKal-Zuk's Ancestral Glyph; revisit if a future encounter needs that
  shape specifically.
- **Linked multi-NPC state** through `EncounterContext`: one entity's status changing another's
  vulnerability or behaviour (Dusk/Dawn, Xarpus exhumeds) - likely needed for Phase 2's "Aeriscia
  waits to attack until after [Igniscia]" per ENJO.md, not yet built.

**Unlocks:** DPS-check/burn windows; linked multi-NPC state; finite-capacity mitigation constructs;
healing-denial in full.

## Stage 8 — Validation by authoring

Author three structurally different encounters **as data, with zero new C#**:

| Boss | Exercises |
|---|---|
| TzTok-Jad | telegraph + prayer reaction + HP-threshold healer spawns |
| Vorkath | fixed deterministic rotation + specials + AoE shapes |
| Grotesque Guardians *or* Verzik | wholesale phase swaps + linked multi-NPC state |
| "Composite" boss (original, kitchen-sink) | every mechanic below, deliberately stacked |

The fourth case is a single invented boss, authored last, that combines every primitive the stages
built: multiple phases each swapping moveset/movement-policy/hazards; at least one fixed-rotation
segment and one HP-reactive-weight segment; a prayer-piercing special; a forced-movement (not just
prayer) reaction requirement; a cone or line AoE shape; an ambient hazard running independently of
the boss's own attack loop; an HP-threshold add-spawn where the add heals the boss; a finite-capacity
shared mitigation construct (Stage 7); an `EncounterTimer` DPS-check/enrage window; and linked state
between the boss and at least one sub-entity. If authoring this exposes two primitives that can't
compose — e.g. a movement policy that can't coexist with an active hazard, or a timer that can't
observe a sub-entity's resource pool — that composition gap is the real finding of Stage 8, more
so than any single boss passing in isolation.

Any C# required for any of the four is a gap in the abstraction — close the gap rather than
special-casing the boss. This stage *is* the test of the north star.

---

## Taxonomy coverage map

Derived from a wiki survey of ~20 bosses (early game → raids → 2024/25 additions).

| # | Mechanic category | Stage |
|---|---|---|
| 1 | Room/radius AoE hitting all occupants | 5 |
| 2 | Prayer-piercing specials | 2 |
| 3 | Forced-movement / tile-avoidance reactions | 2, 5 |
| 4 | Shared/split damage among party | — (see open questions) |
| 5 | Fixed deterministic rotation | 3 |
| 6 | Time/counter-gated rotation | 3 |
| 7 | HP-threshold phase swaps (whole moveset/arena) | 4 |
| 8 | Linked multi-NPC state | 7 |
| 9 | Hard DPS-check / burn-timer windows | 7 |
| 10 | Healing / resource-denial | 4 (adds), 7 (full) |
| 11 | Mandatory non-attack object interaction | out of scope |
| 12 | Interrupt-via-correct-targeting | 2 (hook), 7 |
| 13 | Positioning relative to other players | — (see open questions) |
| 14 | Instance-wide ambient hazard | 5 |
| 15 | Multi-combat / PvP-risk layer | out of scope |
| 16 | Group-scaling / contribution loot | out of scope |
| A | Escalating multi-encounter run, bankable rewards | out of scope (no cross-fight persistence) |
| B | HP-reactive special frequency | 3 |
| C | Player-style-reactive positioning AI | 6 |
| D | Finite-capacity shared mitigation construct | 7 |

## Open questions

1. **Single-player or party?** Categories 4 (shared/split damage) and 13 (stack/spread positioning)
   are meaningless solo. Several raid mechanics assume multiple players. Current scope says
   "moment-to-moment" but not how many actors are on the player side — this decides whether those
   two categories are in scope at all.
2. **Shapes:** extend `TargetKind` or add a parallel `ShapeSpec`? (Stage 5.)
3. **Damage model fidelity:** full OSRS accuracy/defence rolls, or simplified damage ranges until the
   behaviour layer is proven? Real formulas now on hand if/when this gets revisited: melee max hit =
   `floor((effective Strength × (Strength bonus + 64) + 320) / 640)`; accuracy compares an attack roll
   (`effective Attack × (Attack bonus + 64)`) against a defence roll (`(Defence level + 9) × (style
   Defence bonus + 64)` for monsters), with hit-chance branching on which roll is larger. This needs
   player/NPC equipment-bonus modelling that doesn't exist at all yet - still deliberately out of scope
   for `DamageResolver`'s current flat `random.Next(min,max)`.
4. **Telegraph presentation:** how do animations/visual tells bind to telegraph state without putting
   logic in Runtime — does `ActorView` observe `NpcCombatState`, or does Core emit view events?
5. **Collision baking:** `DevOpenFloorCollisionMap` is a stand-in. When does the real baker
   (`Assets/Osrs/Editor/Baking/`) land — before Stage 5 hazards need real arena geometry?
