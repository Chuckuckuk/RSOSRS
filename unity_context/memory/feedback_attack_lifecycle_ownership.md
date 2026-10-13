---
name: feedback-attack-lifecycle-ownership
description: "Design-ownership rule for any \"launched, awaiting later resolution\" state in the combat/encounter engine - it belongs to the target or the encounter, never the attacker's own component."
metadata: 
  node_type: memory
  type: feedback
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-04T23:49:57.121Z
---

Once an attack (or any hazard) has launched and committed to a future resolution, that pending state must not live on the attacker's own component - it belongs on the target (or the encounter, if no single target applies).

**Why:** caught by the user (2026-10-04) in [[project-osrs-damage-calc-research]]: the first implementation of Doom-style attacks (roll deferred until impact) modeled the delay as `NpcCombatState.Traveling`, a state owned by the attacking NPC's own state machine. Since several attacker components early-return once `actor.Health.IsDead` (stop calling `Advance` entirely), killing the attacker mid-travel would silently delete an already-launched, already-telegraphed shot instead of letting it land - exactly backwards, since a real projectile doesn't care whether its caster is still alive. The fix moved this to a new `PendingAttackQueue` owned by the defender, mirroring how `DamageBuffer` (an earlier, already-correct instance of this same pattern) already worked.

**How to apply:** when adding any mechanic shaped like "X happens now, but its effect/roll/resolution happens N ticks later" (projectiles, delayed AoEs, telegraphed traps, timed triggers), ask first whether that later resolution should survive the thing that started it going away. If yes - true for nearly anything projectile- or hazard-shaped - put the pending state on whatever it will eventually affect (the target, via `ICombatTarget`, or `EncounterContext` if it's not target-specific), not on the originator's own per-entity state. Treat any new "~~ing" sub-state tacked onto an attacker's state machine (an NPC, a boss phase controller) as a smell worth double-checking against this rule before building it.

**Refinement (same day, follow-up):** "should the pending state live independently of the launcher's component" and "should its eventual outcome still depend on whether the launcher is ALIVE" are two separate questions - don't conflate them. The user confirmed the first (yes, always) but then asked to explicitly keep the second as a real rule for this project: a killed attacker's already-launched-but-unresolved shot should still fail to land. The right shape is to capture a live reference to the launcher's `Health` (or equivalent) when the pending state is created, then check `IsDead` explicitly at resolve time - never infer this from whether the launcher's own update loop happened to still be running, since that couples the outcome to incidental component lifecycle (disabled, pooled, future refactors) rather than the specific condition (death) that's actually meant to matter. See [[project-osrs-damage-calc-research]] for the concrete `PendingAttackQueue.AttackerHealth` implementation.
