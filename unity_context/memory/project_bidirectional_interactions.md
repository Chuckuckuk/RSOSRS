---
name: project-bidirectional-interactions
description: "Player-attacks-NPC is now built (ICombatTarget + PlayerMover.SetAttackTarget), mirroring the pre-existing NPC-attacks-player path (NpcAi) - not a shared request-queue abstraction yet, just two symmetric call paths."
metadata: 
  node_type: memory
  type: project
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-04T20:56:47.011Z
---

Originally a forward-looking requirement; **now minimally implemented** (Stage 4 session): the player can click an NPC (`HealerNpc`/`NpcCombatant`/`BossCombatant` all implement `ILeftClickOption`) to make it an attack target, chasing it with the player's own real BFS `Mover.RequestMove`/`Step` (never `Mover.StepTowards` - that greedy algorithm is NPC-only, see [[project-npc-chase-movement]]) and auto-attacking on a fixed cooldown once adjacent, via `PlayerMover.RunAttackTick` (`Assets/Osrs/Runtime/Entities/PlayerMover.cs`). This is the missing other half of invariant #5 - NPC-attacks-player already existed (`NpcCombatant.RunNpcAi`/`BossCombatant.RunBossAi`).

**Key facts:**
- `ICombatTarget` (`Assets/Osrs/Core/Actors/ICombatTarget.cs`) is the actor-agnostic surface (`Health`, `CurrentTile`, `FootprintSize`) any attackable Runtime component implements - this is what stayed "generic" rather than the originally-imagined shared TargetSpec-based request queue; a full enqueue/queue-draining abstraction was NOT built, since one direct method call (`PlayerMover.SetAttackTarget`) covered the actual need.
- `PlayerMover.Instance` is a new solo-scope singleton (mirrors `CursorTileTracker.Instance`'s existing pattern) so NPCs can hand themselves over as a target without a wired scene reference back to the player.
- Edge cases from the original open question are resolved pragmatically, not elegantly: target dying mid-chase just clears `attackTarget` (checked every `RunPlayerQueue` tick); target becoming unreachable (BFS `RequestMove` returns false) leaves the player standing still, retried forever, since nothing currently un-sets `attackTarget` in that case - acceptable for now (no such obstacle exists in the dev scene), revisit if it ever causes a visible soft-lock.
- The player's own outgoing attack reuses `AttackDefinition`/`DamageResolver` for the same min/max-roll shape, but deliberately skips `NpcCombatState`'s telegraph machinery and `ReactionRequirement` entirely - OSRS doesn't telegraph the player's own hits to the player, and NPCs don't run protection prayers against incoming player damage.

**How to apply:** if a REAL shared request-queue abstraction turns out to be needed later (e.g. for Stage 7's linked multi-NPC state, or multiple simultaneous attackers), build it then - don't retrofit speculatively now. For any new attackable Runtime type, implement `ICombatTarget` + `ILeftClickOption` (`OnLeftClick() => PlayerMover.Instance?.SetAttackTarget(this);`) exactly like `NpcCombatant`/`HealerNpc`/`BossCombatant` do, rather than inventing a new click-handling path.
