---
name: reference-osrs-combat-reference-doc
description: "OSRS_COMBAT_REFERENCE.md (project root) holds wiki-sourced combat-style terminology (melee/ranged/magic, stab/slash/crush, typeless), tick-system, player/NPC stat, and combat-formula reference material (accuracy roll, max hit per style, prayer/stance bonus tables, monster attribute weaknesses, HP/run-energy/poison tick rates) assembled 2026-10-07 for OSRS_COMBAT_PLAN.md's Open Question #3 (damage model fidelity)."
metadata:
  node_type: memory
  type: reference
  originSessionId: e1f77dfa-c2fc-49b7-bed9-e87fa858f104
  modified: 2026-10-08T04:02:13.345Z
---

`OSRS_COMBAT_REFERENCE.md` at the project root is a standalone wiki-citation reference doc (not a
plan, not a narrative of decisions) — assembled by fetching oldschool.runescape.wiki pages directly
rather than recalling formulas from memory. Covers: the tick system (0.6s/tick, movement/regen/drain
rates all tick-quantized), player combat-level formula, the effective-level pipeline (potion boost →
prayer multiplier → stance bonus → +8 → void multiplier) shared by every accuracy/max-hit roll,
stance-bonus and prayer-tier tables, the accuracy formula (attack roll vs. defence roll, noting
monsters get a +9 Defence-level constant players don't), max-hit formulas for all three styles
(melee/ranged strength-based, magic spell-fixed-base-with-%-modifiers, powered-staff exception), the
200-damage global cap, monster attribute tags (Undead/Demon/Draconic/Kalphite/Leafy/Golem/Shade/
Vampyre-tiers/Flying/Xerician) and what exploits each, and aggression/tolerance timer mechanics.

**Why it exists:** `OSRS_COMBAT_PLAN.md` Open Question #3 says "real formulas now on hand if/when
this gets revisited" but only sketches the melee max-hit/accuracy shape inline — this file is that
promise made concrete and complete (all three styles, not just melee), so revisiting damage-model
fidelity later doesn't require re-deriving or re-fetching anything.

**How to apply:** read this file first before implementing any accuracy-roll/max-hit fidelity work,
authoring a new boss's attack bonuses, or giving an NPC a monster-attribute tag (Undead, Draconic,
etc.) that implies a specific weakness. Does NOT duplicate [[project-osrs-damage-calc-research]]
(prayer-penetration vs. pierce/ignore, the three NPC attack-timing archetypes, the damage-buffer/
Phoenix-necklace mechanic) — read both together for the full picture, this one is the raw
stats/formulas, that one is the archetype/mechanic reasoning already applied to this project's code.
