---
name: project-scope-moment-to-moment-only
description: "Current scope is reproducing moment-to-moment combat/encounter gameplay only — nothing needs to persist between fights (no run/session state, no banked rewards, no cross-fight progression)."
metadata: 
  node_type: memory
  type: project
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-04T18:28:00.956Z
---

The combat/encounter system only needs to reproduce moment-to-moment gameplay within a single fight. Nothing needs to persist between fights — no run/session wrapper, no banked-or-forfeited reward state, no cross-fight progression or stat carry-over.

**Why:** explicit scope decision from the user while reviewing the OSRS boss mechanics taxonomy (see [[project-boss-mechanics-taxonomy]]). This directly drops taxonomy category A — "escalating multi-encounter run with bankable/forfeitable rewards" (Doom of Mokhaiotl's delve scaling, Sol Heredit/Fortis Colosseum's inter-wave modifier picks) — from scope entirely, not just deferred.

**How to apply:** design `Encounter`/`Phase`/`Actor` for a single self-contained fight; don't add hooks for a run-wrapper layer, loot banking, or inter-encounter modifier state on the assumption it'll be needed later. If a future request reintroduces cross-fight persistence (e.g. wanting to build Doom of Mokhaiotl or Fortis Colosseum specifically), treat that as a new scope decision to revisit, not something already designed for.
