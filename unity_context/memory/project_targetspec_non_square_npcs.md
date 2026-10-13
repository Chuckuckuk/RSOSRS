---
name: project-targetspec-non-square-npcs
description: "TargetSpec.Width/Height is deliberately kept independent (not constrained to square) even for NPC targets, diverging from true OSRS's single integer NPC size attribute."
metadata: 
  node_type: memory
  type: project
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-04T17:28:17.934Z
---

[TargetSpec.cs](Assets/Osrs/Core/Pathfinding/TargetSpec.cs) uses separate `Width`/`Height` for `Rect`/`Adjacent` targets rather than a single OSRS-style integer `size`. This was flagged as a divergence from true OSRS (where NPC footprints are always square, driven by one integer `size` attribute) — a `TargetSpec.AdjacentSquare(origin, size)`/`RectSquare` convenience was proposed to prevent accidentally desyncing width/height for NPCs specifically.

**Why:** the user explicitly chose to keep the general Width/Height form rather than add a square-constrained convenience, in order to give NPCs more shape freedom than true OSRS allows. This is a deliberate design choice, not an oversight — do not "fix" `TargetSpec` toward OSRS-exact square-only NPC footprints later. See [[project-bidirectional-interactions]] for the related future interaction-request work this feeds into.

**How to apply:** when building NPC definitions/spawning code later, it's fine for an NPC to have independent width/height. Object footprints already needed this freedom (non-square objects, rotation swaps); NPCs now share the same affordance by choice.
