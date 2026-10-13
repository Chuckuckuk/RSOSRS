---
name: project-npc-chase-movement
description: "OSRS NPCs chase targets with a single greedy no-route-search step, not a full BFS path like players - corrected a Stage 1 bug where NpcCombatant used player-style BFS pathing, which can't reproduce real corner-trap safespots."
metadata:
  type: project
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-04T20:22:38.320Z
---

User correction (verbatim): "NPC pathing is wrong. Their brain is simpler: regardless of size, the
south-west tile is considered the 'canonical' tile, and it will attempt to path directly at you.
There are additional pathfinding quirks resulting in things like 'corner traps'." See
[[project-boss-mechanics-taxonomy]] for the broader mechanics catalogue and
[[project-targetspec-non-square-npcs]] for the related SW-anchor convention already used elsewhere.

**Key facts:**
- **Players path with real BFS obstacle avoidance; NPCs do not.** Real OSRS NPC chase movement is
  a single greedy step per game tick toward the target: prefer the diagonal that reduces distance
  on both axes, fall back to whichever cardinal component is unblocked if the diagonal isn't, and
  if every option is blocked, the NPC simply does not move that tick. There is no search for an
  alternate route around an obstacle - unlike the player's click-to-move, which does full BFS.
- **A footprint's "canonical" tile for this comparison is always its south-west corner**,
  regardless of the NPC's size (1x1 up to large bosses) - distance-to-target is computed SW-tile
  to SW-tile, not centre-to-centre or nearest-edge. This project's `TargetSpec`/`CollisionMap`
  already modelled size>1 footprints as SW-anchored for collision purposes before this correction
  (see [[project-targetspec-non-square-npcs]]); the bug was that `NpcCombatant` wasn't using that
  convention for its *movement* decision at all, since it was BFS-pathing to an `Adjacent` TargetSpec.
- **"Corner trap" / pillar safespots are a direct consequence of the above, not a separate bug
  class.** If the NPC's diagonal step toward the player is denied by the no-corner-cutting rule
  (hugging a wall corner) and both cardinal fallbacks are also individually blocked by the same
  walls, the NPC stalls in place indefinitely - this is exactly how many real OSRS safespots work
  (a player can hit an NPC that geometrically cannot reach them, because its movement algorithm has
  no fallback smarter than "try diagonal, then each cardinal, then give up for this tick").

**Why it mattered / how it was applied:** Stage 1's first-pass `NpcCombatant.RunNpcAi` called
`Mover.RequestMove(pathfinder, TargetSpec.Adjacent(playerTile, ...))` every `NpcAi` tick - the same
full-BFS machinery as the player's click-to-move - so the NPC always found an optimal route around
any obstacle and could never exhibit a corner trap, which is wrong for OSRS NPC behaviour. Fixed by
adding `Mover.StepTowards(BfsPathfinder, TileCoord)`, which wraps the already-existing but
previously-unused `BfsPathfinder.TryStepTowards` single-step heuristic (it already implemented the
diagonal→cardinal→give-up fallback chain correctly; it just wasn't wired to anything) and switched
`NpcCombatant.RunNpcMovement` to call it instead of `RequestMove`+`Step`. `Mover.RequestMove`/`Step`
remain unchanged and are still correct for the player.

**How to apply:** any future NPC movement work (Stage 6's `IMovementPolicy`, boss-specific chase
behaviour) must default to this greedy single-step algorithm, not BFS, unless a specific boss's
real wiki-documented behaviour calls for something smarter (rare - most OSRS monsters use exactly
this dumb chase). Do not "fix" an NPC getting stuck on a corner/pillar - that stall is the correct,
authentic behaviour and is what makes safespotting possible; only `IMovementPolicy` swapping the
whole algorithm per-NPC/phase is the legitimate way to special-case it. Keep comparing footprints
by their south-west tile for any future per-NPC movement/targeting logic, matching the
already-established collision convention.
