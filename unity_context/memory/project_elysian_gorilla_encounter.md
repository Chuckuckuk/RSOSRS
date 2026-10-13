---
name: project-elysian-gorilla-encounter
description: "In-progress OSRS-authored recreation of New World: Aeternum's Elysian Wilds gorilla trio (Purutu/Igniscia/Aguascia/Aeriscia) - sourced the real names/elements, decided each gorilla's style pairing, and drove Stage 6 (movement policies) to build as a prerequisite."
metadata: 
  node_type: memory
  type: project
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-05T00:28:25.727Z
---

Building a new OSRS-authored boss encounter (via [[project-boss-mechanics-taxonomy]]'s staged plan, `OSRS_COMBAT_PLAN.md`) based on New World: Aeternum's Elysian Wilds gorilla elites, at the user's request (2026-10-04).

**Source material (confirmed via web research, not guessed):**
- **Pugnacious Purutu** - the base/physical gorilla (`fl_gorilla_melee_elite_named_09fl12`), drops the Lifetaker artifact.
- **Irritable Igniscia** (fire, `fl_gorilla_fire_elite_named_09fl12`), **Annoyed Aguascia** (ice, `fl_gorilla_ice_elite_named_09fl12`, drops Boltcaster), **Agitated Aeriscia** (lightning, `fl_gorilla_lightning_elite_named_09fl12`) - all share Purutu's barebones gorilla skeleton (arm swings, ground pounds, leaps, charges) in the source game; a separate ranged variant "hangs back and throws rocks" instead of melee.

**Design decisions made so far (user's calls, not mine):**
- Flat elemental reskinning ("same moveset, different damage type") was rejected as giving the player no real reaction variety - all three would be countered by the same single prayer. Instead each got a DISTINCT style pairing: **Igniscia = melee/magic, Aeriscia = melee/ranged, Aguascia = pure ranged AoE that never approaches at all** (absorbing the source game's separate "ranged variant" into Aguascia specifically, rather than being a 4th separate NPC).
- Asked to sequence engine work before authoring: Leap/Charge/Aguascia's "never approaches" behavior exposed real gaps (Stage 5 Line/Cone shapes, Stage 6 movement policies, an unplanned "instant reposition" primitive for Leap/Charge). User chose "build Stage 5/6 properly first" over approximating with gorilla-specific hacks - see [[feedback_attack_lifecycle_ownership]] for the general pattern this matches (this user consistently prefers correct primitives over accidentally-correct one-offs).
- After designing the style pairings, re-scoped: Igniscia/Aeriscia's mixed-style attacks need NO new engine work (the existing `WeightedRandomPattern` already mixes styles freely, exactly like Jad). Only Aguascia's "never approaches" genuinely needed Stage 6. Stage 5 (Line/Cone shapes for Charge, Hazard) turned out to be unneeded by this specific trio as scoped and was deliberately deferred rather than built speculatively - see the Stage 5 section of `OSRS_COMBAT_PLAN.md` for the full reasoning (this project is solo-scope, so radius AoE already works via existing `ReactionRequirement.MoveToTile`, and only a line/cone-shaped attack would actually need new shape code).

**Built so far:** Stage 6 (`IMovementPolicy` + Stationary/ChaseToMeleeRange/MaintainDistance/StyleReactive, `Mover.StepAway`/`BfsPathfinder.TryStepAway`, `Phase.MovementPolicy`, `PlayerMover` now implements `ICombatTarget`). See `OSRS_COMBAT_PLAN.md`'s Stage 6 section for the full file list.

**Not yet done:** the actual gorilla `AttackDefinition`s/`Phase`s/encounter-setup file (the Jad-shaped authoring step) - Leap and Charge specifically are NOT yet buildable (still need the deferred Stage 5 work + a reposition primitive) and may need to be dropped from the final moveset or revisited if the user wants them back.

**How to apply:** don't re-research the New World source material (names/elements/skeleton) - it's all above. Don't re-litigate the Stage 5 deferral without checking whether the moveset changed again first. When resuming, the next step is authoring the actual encounter data, following `JadEncounterSetup.cs`'s pattern.
