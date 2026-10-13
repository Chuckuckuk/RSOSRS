# OSRS Combat Reference — Wiki-Sourced Context

Reference material pulled from oldschool.runescape.wiki (fetched 2026-10-07) on the tick system,
player/NPC stats, and the real combat formulas. > **Update 2026-10-09:** this document is now also the *specification* for the Rust simulator
> (`osrs_core`). Every formula and rate below is implemented in `osrs_core/src/rules/`, driven by the
> data in `osrs_core/data/`; see the **Implementation map** at the end. Where the original text was
> ambiguous or wrong, it has been corrected in place and marked **(wiki-verified 2026-10-09)** or
> **(unverified)** when it comes from memory of the wiki rather than a fetched page.

This is **reference, not a plan** — it exists so
`OSRS_COMBAT_PLAN.md`'s Open Question #3 ("damage model fidelity") has the actual numbers on hand
if/when it's revisited, without re-deriving them from scratch. It does not repeat anything already
captured in the `project-osrs-damage-calc-research` memory (prayer-penetration vs. pierce/ignore,
the three NPC attack-timing archetypes, the damage-buffer/Phoenix-necklace mechanic) — read that
alongside this for the full picture.

---

## 1. Combat style terminology

Three real combat styles exist: **Melee, Ranged, Magic.** Everything else below (stance names,
"typeless", attack-type categories) is a modifier or sub-category of one of these three, never a
4th style of its own.

### Melee

Three skills: **Attack** (accuracy + weapon-level requirements), **Strength** (damage), **Defence**
(reduces being hit by *any* style, not just melee). Melee's own accuracy/defence further splits
into three **attack types** — **stab, slash, crush** — matched against the defender's matching
defence bonus. These are categories of melee swing, not separate combat styles: a dagger or spear
is stab, a sword or scimitar is slash, an axe/warhammer/mace is crush (see
[Weapons/Categories](https://oldschool.runescape.wiki/w/Weapons/Categories) for the full per-weapon
list). Weak against Magic, strong against Ranged.

### Ranged

One skill, **Ranged**, for accuracy; damage comes from the weapon+ammo pair, not the skill level
alone (see the Ranged Strength term in §5's max-hit formula). For bows/crossbows, the **weapon**
supplies the Ranged Attack bonus and the **ammunition** (arrows/bolts) supplies the Ranged Strength
bonus; for thrown weapons (darts, knives, thrownaxes) the single item supplies both, so ammo choice
doesn't matter for them. Ammo is further grouped **light / standard / heavy**, which matters for
monster-specific ranged-defence quirks (a monster can have unusually low/high defence against one
ammo weight class specifically). Weak against Melee, strong against Magic.

### Magic

One skill, **Magic**, for accuracy — and, unusually, Magic level does **not** generally scale a
spell's own damage the way Strength/Ranged Strength scale melee/ranged (see §5's "max hit — magic").
Higher-level combat spells also aren't inherently more accurate than lower-level ones; accuracy
moves only via equipment/prayer bonuses, same as the other two styles. Three combat-relevant
spellbooks: **Standard** (the four elemental strike/bolt/blast/wave/surge spells, available to
everyone), **Ancient Magicks** (unlocked by Desert Treasure I; the strongest standard-skill combat
spells), **Arceuus** (necromancy-flavoured combat spells, expanded after A Kingdom Divided).
**Powered staves** (Trident, Sanguinesti staff, Tumeken's Shadow, etc.) are the one place Magic
level *does* directly scale max hit — they ignore the spellbook entirely and run their own built-in
effect off the player's Magic level, closer in shape to how Strength scales melee. Weak against
Ranged, strong against Melee.

### Combat triangle

Melee → beats Ranged → beats Magic → beats Melee. Expressed through **armour's defensive bonuses**,
not a hard damage block — any style can still kill any other style; the triangle only says which
armour choice is favoured. Per the wiki: melee armour defends well against melee *and* ranged but
poorly against magic; ranged armour defends well against magic but only middling against melee/
ranged; mage gear gives magic attack bonuses but little to no melee/ranged defence. The asymmetry
("melee beats ranged" despite melee armour ALSO defending well against ranged) comes from range —
melee has no way to attack from a distance, so a ranged or magic user can simply safespot it.

### "Typeless" — not a 4th style

**Typeless is old (pre-2017 Chambers of Xeric) terminology for an attack with 100% prayer
penetration — it is a modifier, not a combat style.** A "typeless" hit still rolls a completely
normal accuracy check against the matching real style's defence bonus, and the wiki itself always
phrases it as "typeless `<style>`" (e.g. "typeless melee damage"), never bare "typeless" — it just
cannot be reduced by *any* protection prayer, correct or not. This predates the 2017 update that
added partial prayer penetration; before that, protection prayers only ever blocked 100% or 0% of a
hit, so "typeless" was the only way to describe prayer-immune damage. Real examples the wiki lists:
Abyssal Sire's tentacles, Death spawns, Goraks, Zulrah's Magma phase, Jal-Nib, and Verzik Vitur's
Phase 3. This is the same 100%-penetration end of the spectrum this project already calls "ignore"
in [[project-osrs-damage-calc-research]] (vs. partial "pierce") — "typeless" and "ignore" are two
names from two different eras for the same end of that scale, not two different mechanics.

## 2. The tick system

- **One game tick = 0.6 seconds** (100 ticks/minute) nominally. Real servers drift slightly under
  load (0.606–0.618s observed), which is why OSRS is informally "0.6s per tick" rather than a hard
  guarantee — this project's `TickAccumulator` fixed-600ms model matches the nominal rate, not the
  server-load jitter.
- **Everything client-visible is tick-quantized.** An action registered during a tick doesn't take
  effect until the *next* tick begins — i.e. there's an inherent 0–0.6s input latency baked into the
  game, not a bug to minimize. This is the same shape as this project's accumulator-driven
  `TickRunner` + `TickProgress01` interpolation.
- **Movement:** walking = 1 tile/tick, running = 2 tiles/tick.
- **Stat drain/restore (boosts/potions):** boosted or lowered stats drift back to base at **1 level
  per 100 ticks** (~1/minute). The Preserve prayer slows the decay of *boosted* stats to 1 level per
  150 ticks; lowered stats still recover at the normal rate. *(Wiki: "Preserve" page; the exact
  per-skill timer alignment is approximated here as one counter per skill that restarts whenever
  the stat is on its base.)*
- **Special attack energy:** regenerates **10% per 50 ticks** (30 seconds — **wiki-verified
  2026-10-09**), or **10% per 25 ticks** with a Lightbearer equipped.
- **Hitpoints regen:** see §6.
- **Run energy / prayer drain / poison:** also tick-driven — see §6 for the exact rates.
- **"Tick manipulation" / "tick eating"** are player-skill terms for deliberately timing actions
  relative to tick boundaries (e.g. switching prayer on the exact resolve tick — see the
  `project-osrs-damage-calc-research` memory for how this project already models that specifically
  for protection prayers). Not a separate mechanic to build, just the *name* for exploiting the
  tick-quantization above.

## 3. Player stats & attributes

Seven combat-relevant skills: **Attack, Strength, Defence, Ranged, Magic, Prayer, Hitpoints.**
Prayer is a drainable resource (points, not a combat roll input itself); the other six feed the
formulas below.

### Combat level

```
CombatLevel = floor( Base + max(Melee, Range, Mage) )

Base   = 0.25 × (Defence + Hitpoints + floor(Prayer / 2))
Melee  = 0.325 × (Attack + Strength)
Range  = 0.325 × floor(Ranged × 1.5)
Mage   = 0.325 × floor(Magic × 1.5)
```

Melee-based combat level caps at 126; a pure ranged/magic build caps at 109 — only one of the three
style terms (whichever is highest) counts, never a sum of all three.

### Effective level pipeline (applies to Attack/Strength/Defence/Ranged/Magic alike)

Every combat roll (accuracy *and* max hit) runs the relevant base level through the same pipeline
before the roll-specific formula uses it:

```
EffectiveLevel = floor( floor( (BaseLevel + PotionBoost) × PrayerMultiplier ) + StanceBonus + 8 )
                 × VoidMultiplier        (only if Void Knight equipment is worn; 1.0 otherwise)
```

- **PotionBoost** is additive (e.g. Super Strength's +5 levels), applied before the prayer
  multiplier, not after.
- **PrayerMultiplier** comes from whichever single relevant prayer is active — see the prayer table
  below for the exact percentages. Prayers don't stack with each other (only one Attack-boosting /
  one Strength-boosting / one Defence-boosting prayer can be active per style at a time); the "+8"
  constant is always added regardless of prayer or stance.
- **StanceBonus** is the invisible per-style number from the attack-style table below.
- **VoidMultiplier** is ×1.1 for a full Void set matching the attack style, applied to the
  *whole* sum (prayer, stance and the +8) and floored again. **Correction (wiki-verified
  2026-10-09, DPS page):** the Void multiplier is on the *effective level*, as written above — but
  Elite Void's ×1.125 is a *damage-only* bonus for Ranged (accuracy stays ×1.1), and Magic Void is
  ×1.45 *accuracy* with Elite adding +2.5% Magic *damage*. The simulator therefore models Void as
  separate `accuracy_level` / `damage_level` modifiers per set (`osrs_core/data/sets.json`) rather
  than one number. Elite Void's higher damage replaces the standard Void damage bonus; it does not
  stack with it.
- **Monsters attacking** use the same pipeline with a +1 stance bonus, so a monster's effective
  Attack/Ranged/Magic level is `level + 1 + 8 = level + 9` — **not** `level + 8` as §4 below
  originally said (wiki-verified 2026-10-09, DPS page: "+1 for NPCs"). This is the attacking-side
  twin of the defending-side `+9`.

### Attack style (stance) bonuses

| Stance | Melee weapons | Ranged weapons | Effect |
|---|---|---|---|
| Accurate | ✅ | ✅ | +3 effective Attack (melee) / +3 effective Ranged (**both** the ranged accuracy and the ranged strength level) |
| Aggressive | ✅ | — | +3 effective Strength |
| Defensive | ✅ | — | +3 effective Defence |
| Controlled | some weapons only (longsword, scimitar, mace, spear, halberd, hasta, claw, whip) | — | +1 to Attack, Strength, *and* Defence simultaneously |
| Rapid | — | ✅ | no accuracy bonus; attack speed reduced by 1 tick instead |
| Longrange | — | ✅ | +3 effective Defence, +2 tiles attack range (cap 10 tiles) |
| Accurate (powered staff) | — | — | +3 effective Magic |
| Longrange (powered staff) | — | — | +1 effective Magic, +3 effective Defence, +2 tiles range |
| Spell / Defensive spell (autocast or manual) | — | — | **no invisible bonus** (Defensive spell adds +3 Defence); autocast does not get the +3 Magic that powered staves' Accurate does *(wiki-verified 2026-10-09, Combat Options)* |

### Prayer tiers (combat-relevant)

> The table below is the original summary. The complete per-prayer list — levels, every stat
> multiplier and the **drain effect** each prayer feeds into the drain formula — is
> `osrs_core/data/prayers.json` (**wiki-verified 2026-10-09**, Prayer page). Differences from the
> summary: Chivalry's drain effect is **12** (it was blank), Thick Skin/Burst/Clarity/Sharp Eye/Mystic
> Will are effect **1**, Rock Skin/Superhuman/Improved Reflexes/Hawk Eye/Mystic Lore are **6**, Steel
> Skin/Ultimate/Incredible/Eagle Eye/Mystic Might and all three protection prayers are **12**,
> Piety/Rigour/Augury are **24**. Mystic Lore adds +1% and Mystic Might +2% Magic damage; Augury +4%.
> Protection prayers block 100% of an NPC's damage but only **40% of a player's** (PvP, not modelled).
> Only one prayer per *group* (attack, strength, defence, ranged, magic, overhead) can be active;
> turning one on switches the others in its groups off.

| Prayer | Level | Effect | Drain rate |
|---|---|---|---|
| Clarity of Thought / Burst of Strength / Sharp Eye / Mystic Will | 4–9 | +5% Attack / Strength / Ranged / Magic | 1 pt / 36s |
| Improved Reflexes / Superhuman Strength / Hawk Eye / Mystic Lore | 16–27 | +10% Attack / Strength / Ranged / Magic | 1 pt / 6s |
| Protect from Melee / Missiles / Magic | 37–43 | ~100% PvM damage negation from the matching style | 1 pt / 3s |
| Chivalry | 60 | +15% Attack, +18% Strength, +20% Defence | — |
| Piety | 70 | +20% Attack, +23% Strength, +25% Defence | 1 pt / 1.5s |
| Rigour | 74 | +20% Ranged, +23% Ranged Strength, +25% Defence | 1 pt / 1.5s |
| Augury | 77 | +25% Magic, +25% Defence, +4% Magic damage | 1 pt / 1.5s |

(Percentages above are the `PrayerMultiplier` fed into the effective-level pipeline, e.g. Piety's
Strength line is ×1.23.)

### Equipment bonus categories

- **Attack bonuses** (five, one per combat style): Stab, Slash, Crush, Magic, Ranged.
- **Defence bonuses** (same five categories): Stab, Slash, Crush, Magic, Ranged.
- **"Other" bonuses:** Melee Strength, Ranged Strength, Magic Damage %, Prayer bonus (reduces drain
  — see §6), plus specific weapon/set effects (Salve Amulet, Slayer helm, Dragon hunter gear, etc. —
  see the attribute table below).

## 4. NPC stats & attributes

A monster's definition carries its own fixed **combat level**, **Hitpoints**, one or more **attack
styles** each with their own **max hit** and **attack speed** (in ticks, same unit as player
weapons — see §7), plus the same five-category **attack/defence bonuses per style** a player has.
Monsters roll accuracy and max hit through the *same* formulas as players (§5) — a monster's
"effective Attack level" still runs through the stance-bonus/prayer-multiplier pipeline in theory,
but in practice most monsters have no stance to select and no prayers, so their effective level is
usually just their raw Attack/Ranged/Magic level + 8.

**One asymmetry worth keeping precise:** when a *monster* is the defender, its defence roll adds a
flat **+9** to its Defence level that a player defender's own defence roll does not get:

```
MonsterDefenceRoll = (MonsterDefenceLevel + 9) × (MonsterStyleDefenceBonus + 64)
PlayerDefenceRoll  = PlayerEffectiveDefenceLevel × (PlayerStyleDefenceBonus + 64)
```

**Magic defence is different for both (wiki-verified 2026-10-09, Defence + Magic DPS + Combat pages):**

```
MonsterMagicDefenceRoll = (MonsterMagicLevel + 9) × (MonsterMagicDefenceBonus + 64)     ← its MAGIC level, not Defence
PlayerMagicDefenceLevel = floor(0.7 × EffectiveMagic + 0.3 × EffectiveDefence)          ← then × (MagicDefenceBonus + 64)
```

The player's 70/30 split uses the full effective-level pipeline for both terms (prayer, stance and
the +8). *(The exact flooring of the 70/30 sum is not spelled out on the fetched pages; the simulator
floors once after combining — unverified.)*

A monster **attacking** gets the mirror of the +9: `level + 9` effective Attack/Ranged/Magic (§3).
Its **max hit is a listed number per attack**, not derived from a Strength level, so a monster
definition carries `max_hit` per attack (`osrs_core/data/npcs.json`).

### Aggression

- Aggressive monsters have an aggression range (their attack range plus a bit of "max range") within
  which they'll initiate combat unprompted.
- **10-minute tolerance:** after a player has stood near aggressive monsters for 10 continuous
  minutes (1000 ticks), those monsters stop attacking them unprompted. **Correction (wiki-verified
  2026-10-09, Aggressiveness page):** it is *leaving the area and coming back* that makes them
  aggressive again — the original text credited a random event. Logging out/in or world-hopping does
  **not** reset it. A few monsters (Desert bandits, Vyres, Warped terrorbirds, everything in the
  Nightmare Zone) never become tolerant. The simulator approximates "the area" as the aggression
  range of any tolerance-subject aggressive monster: the timer runs while the player is inside one
  and resets the moment they are inside none.
- **Combat-level restriction (wiki-verified 2026-10-09):** an aggressive monster only attacks
  players whose combat level is at most double its own; a monster of level 63+ attacks everyone
  (63 × 2 = 126, the maximum). Some monsters ignore the restriction.

### Monster attribute tags

Not a stat — a categorical tag that specific player gear is written to exploit. Directly relevant to
this project's encounter-authoring (per the existing `project-osrs-damage-calc-research` memory
flagging that an Undead-tagged boss should document its Salve Amulet weakness even with no player
equipment system yet):

| Attribute | Example monsters | What exploits it |
|---|---|---|
| Undead | Zombies, skeletons, revenants | Salve Amulet (+16.67–20% Attack/Strength, or also Ranged/Magic for the imbued/enchanted-imbued versions), Crumble Undead spell |
| Demon | Hellhounds, K'ril Tsutsaroth | Demonbane weapons (Arclight, Darklight), Demonbane spells (+20% accuracy) |
| Draconic | Dragons, wyverns, hydras | Dragon hunter crossbow/lance/wand (accuracy + damage) |
| Kalphite | Kalphite soldiers/workers, scarabs | Keris (+33% damage, 1/51 triple-damage chance) |
| Leafy | Kurasks, Turoths | **Immune to everything except** leaf-bladed weapons (+17.5% damage) |
| Golem | Gargoyles, runite golems | Granite hammer (+30% accuracy and damage) |
| Shade | Shades of Mort'ton line | Gadderhammer (+25% damage, 1/20 double-damage chance) |
| Spectral | Barrows brothers, banshees | Ectoplasmator (bonus Prayer XP, not damage) |
| Vampyre (tier 1/2/3) | Vyrewatch and related | Tier 2 needs silver weapons; tier 3 needs one of 6 specific weapons (Ivandis/Blisterwood/Hallowed flail, Sunspear) |
| Fiery | Dragons, fire giants, pyrefiends | Pearl bolts (e) bonus effect |
| Flying | Aviansies, Vespula | **Cannot be meleed at all except with a halberd** |
| Xerician | Chambers of Xeric monsters | Twisted bow's damage scaling caps at 350 Magic instead of 250 |

## 5. Combat formulas

### Accuracy (hit chance)

```
AttackRoll  = EffectiveAttackLevel × (EquipmentAttackBonus + 64)
DefenceRoll = EffectiveDefenceLevel × (StyleDefenceBonus + 64)      [player defending]
            = (DefenceLevel + 9) × (StyleDefenceBonus + 64)         [monster defending]

if AttackRoll > DefenceRoll:
    HitChance = 1 − (DefenceRoll + 2) / (2 × (AttackRoll + 1))
else:
    HitChance = AttackRoll / (2 × (DefenceRoll + 1))
```

Both `EffectiveAttackLevel`/`EffectiveDefenceLevel` are the §3 effective-level pipeline output for
Attack/Defence respectively, using whichever `StyleDefenceBonus` category matches the *incoming*
attack's style.

### Max hit — melee

```
EffectiveStrength = EffectiveLevel pipeline, run on the Strength skill (§3)
MaxHit = floor( 0.5 + EffectiveStrength × (StrengthBonus + 64) / 640 )
```
(equivalent to `floor((EffectiveStrength × (StrengthBonus + 64) + 320) / 640)` — the form already
recorded in `OSRS_COMBAT_PLAN.md`'s Open Question #3 and in the `project-osrs-damage-calc-research`
memory; both are the same formula written two ways.)

Weapon/passive-specific multipliers layer on top of this base, e.g.: Dharok's (scales with missing
HP, can exceed the base significantly), Slayer helm on-task (×1.1667), Dragon mace special (×1.5).
**Global cap: 200 damage**, regardless of how many multipliers stack.

**Order and flooring (wiki-verified 2026-10-09, DPS page):** the multipliers are applied *one at a
time in a fixed order, flooring after each* — never combined first. Slayer helm (×7/6 melee, ×1.15
ranged/magic) and the Salve amulet (×7/6 or ×1.2) **do not stack**: the Salve wins where both apply.
The same target-specific multiplier is also applied to the **attack roll**, not only the max hit.
`osrs_core` expresses this with `Modifier`s: ordered per target (`attack_roll`, `max_hit`, ...), with
a `group` + `priority` for the do-not-stack rule (`osrs_core/src/model/modifier.rs`).

**Dharok's set:** `MaxHit × (1 + (MaxHP − CurrentHP)/100 × MaxHP/100)`.

**Twisted bow (wiki-verified 2026-10-09, Twisted bow page):** accuracy and damage multipliers scale
with the target's Magic (the higher of its Magic level and Magic attack bonus), capped at 250 — or
350 against Xerician (Chambers of Xeric) monsters:

```
Accuracy % = min(140, 140 + floor((3m − 10)/100) − floor((3m/10 − 100)² / 100))
Damage   % = min(250, 250 + floor((3m − 14)/100) − floor((3m/10 − 140)² / 100))
```

*(The fetched page garbled the exact expression; this form is reconstructed from memory and
reproduces the page's "accuracy cap reached by ~240 Magic" and "~250% damage only inside the
Chambers" statements. Unverified to the last digit.)*

### Max hit — ranged

Mirrors melee exactly, but keyed on Ranged/Ranged Strength instead of Strength:

```
EffectiveRangedStrength = EffectiveLevel pipeline, run on the Ranged skill (§3)
MaxHit = floor( 0.5 + EffectiveRangedStrength × (AmmoRangedStrengthBonus + 64) / 640 ) × GearBonus
```
`GearBonus` covers things like Slayer helm on-task (×1.15) or Salve Amulet on an Undead target
(×1.2) — multiplicative, applied after the base floor.

### Max hit — magic

Fundamentally different shape: a spell's max hit is a **fixed base value per spell** (e.g. Fire
Blast = 16, Ice Barrage = 30), *not* derived from the Magic level — Magic level normally affects
accuracy, not damage. The base is then scaled by:

```
MaxHit = floor( SpellBaseMaxHit × (1 + sum of all Magic Damage% bonuses) )
```

**The bonuses are summed *before* the single floor (wiki-verified 2026-10-09, Maximum magic hit):**
gear Magic Damage %, prayers (Mystic Lore +1%, Mystic Might +2%, Augury +4%), Elite Void (+2.5%),
Salve, Virtus robes, etc. are *added together*, not multiplied. Gear is capped at +100% visible
bonus. Slayer helm / Salve then apply as a separate ×multiplier on the result (§5 order above).

**Exception — powered staves** (Trident of the Seas/Swamp, Sanguinesti staff, Tumeken's Shadow,
their specials): these have no "spell" at all and instead scale max hit directly from the player's
*visible* (boosted) Magic level via their own per-weapon formula, closer in shape to how melee/ranged
scale from Strength/Ranged Strength. The simulator's built-ins use `floor(Magic / 3) + offset` with
offsets −5 (Trident of the seas), −2 (swamp), −1 (Sanguinesti), +1 (Tumeken's shadow) — the
offsets are **unverified** (recalled, not fetched), and the data file marks them so. Tumeken's shadow
additionally multiplies the *visible gear bonus* ×3 outside the Tombs of Amascut (×4 inside) before the
+100% cap (wiki-verified, Maximum magic hit page). Magic Dart is a standard spell whose base is
`floor(Magic / 10) + 10` (without a Slayer's staff).

## 6. Other tick-synchronized resources

| Resource | Rate | Notes |
|---|---|---|
| Hitpoints regen | **1 HP per 100 ticks** (~once/minute) | Doubled (1 HP per 50 ticks) with Rapid Heal prayer **or** a Hitpoints cape (these two do not stack); a Regeneration bracelet doubles it again and **does** stack with either → 1 HP per 25 ticks (4/min). *(wiki-verified 2026-10-09)* |
| Prayer drain | `seconds = 0.6 × (DrainResistance / DrainEffect)`, `DrainResistance = 60 + 2×PrayerBonus` | Each active prayer has its own `DrainEffect`; multiple active prayers' effects sum, draining the shared pool faster. +1 Prayer bonus ≈ +3.33% longer drain duration |
| Run energy drain (while running) | `UnitsLost = floor(60 + 67 × clamp[0,64](Weight) / 64) × (1 − Agility/300)` per tick | Weight is capped at 64kg for this formula; negative weight (special gear) behaves as 0kg. Energy is in **units out of 10 000**. A Stamina potion cuts the drain to ×0.3 (floored); a charged Ring of endurance to ×0.85. The wiki page's worked example ("~169 units/tick, 59 s") is inconsistent with its own formula (which gives 40/tick at 99 Agility, 0 kg), so the formula — not the example — is what is implemented. *(wiki-verified formula 2026-10-09)* |
| Run energy regen (while walking/standing) | `floor(Agility / 10) + 15` per tick | ×1.3 (floored) while wearing the full Graceful outfit. Running spends energy; any tick not running regains it. An empty bar switches running off. |
| Poison damage | Once per **30 ticks (18s)** | **Damage = floor((severity + 4) / 5)** (severity 30 → 6); severity drops by 1 per hit, so the visible damage steps down every 5 hits, and the poison ends when severity reaches 0. **Venom**: starts at 6 and rises by 2 per hit to 20, and never wears off by itself. Poison and venom are mutually exclusive; since April 2024 a poison no longer downgrades an existing venom. Antipoison-style immunity (timed) blocks both. *(wiki-verified 2026-10-09; the venom 6→20 progression is from memory — unverified)* |
| Attack timing | The tick the swing resolves, then `AttackSpeed − 1` ticks of cooldown | A 4-tick weapon swings on ticks 1, 5, 9... Eating adds the food's delay (3 ticks, karambwan 2) to the cooldown. Rapid is −1 tick. Spell casts are 5 ticks for standard spells (powered staves use their weapon speed). |
| Projectile flight | Ranged `1 + floor((3 + d)/6)`, Magic `1 + floor((1 + d)/3)` ticks | Melee lands on the swing tick. The roll — and the prayer check — happens on the swing tick; only the HP loss waits (the damage queue). *(unverified — recalled from the wiki's Hit delay page, not fetched)* |

## 7. Weapon / attack-speed reference

Attack speed is measured in the same game ticks as everything else — multiply by 0.6 for seconds.
Lower = faster.

| Ticks | Seconds | Example weapons |
|---|---|---|
| 2 | 1.2s | Blowpipe/darts under Rapid |
| 3 | 1.8s | Swift blade, goblin paint cannon |
| 4 | 2.4s | Scimitars, daggers (most common "fast" melee speed) |
| 5 | 3.0s | Longswords, most bows |
| 6 | 3.6s | Most crossbows |
| 7 | 4.2s | Halberds, most two-handed swords |
| 9 | 5.4s | Dark bow (base speed) |

---

## Sources

- [Typeless](https://oldschool.runescape.wiki/w/Typeless)
- [Combat triangle](https://oldschool.runescape.wiki/w/Combat_triangle)
- [Melee](https://oldschool.runescape.wiki/w/Melee)
- [Ranged](https://oldschool.runescape.wiki/w/Ranged)
- [Magic](https://oldschool.runescape.wiki/w/Magic)
- [Attack style](https://oldschool.runescape.wiki/w/Attack_style)
- [Game tick](https://oldschool.runescape.wiki/w/Game_tick)
- [Combat level](https://oldschool.runescape.wiki/w/Combat_level)
- [Maximum melee hit](https://oldschool.runescape.wiki/w/Maximum_melee_hit)
- [Maximum ranged hit](https://oldschool.runescape.wiki/w/Maximum_ranged_hit)
- [Maximum magic hit](https://oldschool.runescape.wiki/w/Maximum_magic_hit)
- [Damage per second/Melee](https://oldschool.runescape.wiki/w/Damage_per_second/Melee) (accuracy roll derivation)
- [Combat Options](https://oldschool.runescape.wiki/w/Combat_Options) (attack style bonuses)
- [Prayer](https://oldschool.runescape.wiki/w/Prayer)
- [Hitpoints](https://oldschool.runescape.wiki/w/Hitpoints)
- [Run energy](https://oldschool.runescape.wiki/w/Run_energy)
- [Poison](https://oldschool.runescape.wiki/w/Poison)
- [Attribute](https://oldschool.runescape.wiki/w/Attribute)
- [Aggressiveness](https://oldschool.runescape.wiki/w/Aggressiveness)
- [Weapon speed / Attack speed](https://oldschool.runescape.wiki/w/Weapon_speed)

---

## Implementation map (2026-10-09)

Where each mechanic above lives in `osrs_core`. Formulas are pure functions in `src/rules/`; the
numbers (items, prayers, spells, monsters) are JSON in `data/`; `src/sim/` is the runtime that
ticks them. `cargo run --bin simulate -- <scenario>` runs any `data/scenarios/*.json`.

| Reference | Code | Data |
|---|---|---|
| §1 styles, typeless | `combat::AttackStyle`, `model::DamageType`, `AttackDefinition::prayer_penetration` | `damage_type` on stances/attacks |
| §2 tick order | `tick::TickScheduler`, `sim::World::run_tick` | — |
| §2/§6 HP regen, stat drift, prayer drain, run energy, spec energy | `rules/resources.rs`, `sim/player_runtime.rs::run_regen` | prayer `drain_effect`, item `tags` (`regen_bracelet`, `lightbearer`, `hitpoints_cape`, `ring_of_endurance`), set `graceful_outfit` |
| §3 combat level | `rules::combat_level`, `sim::Skills::combat_level` | — |
| §3 effective level pipeline | `rules::effective_level`, `combat::CombatStats::effective_*_level` | prayer `effects`, stance `bonuses`, set `accuracy_level`/`damage_level` modifiers |
| §3 stances | `data::StanceDef`, `Player::plan_attack` | weapon `stances` |
| §3 prayers | `data::PrayerDef`, `Player::toggle_prayer` | `prayers.json` |
| §3 equipment bonuses | `model::EquipmentBonuses`, `sim::Loadout::totals` | item `bonuses` |
| §4 NPC stats, attributes | `data::NpcDef`, `sim::npc_factory`, `model::Attribute` | `npcs.json` |
| §4 aggression, tolerance, level rule | `World::update_aggression`, `World::update_aggro_region` | npc `aggression` |
| §4 attribute-keyed gear (Salve, Demonbane, Keris, Dragon hunter, Leafy, Flying) | `model::Modifier` + `Condition::TargetHas`, `NpcDef::attackable_if` | item `modifiers`, npc `attackable_if` |
| §5 accuracy, max hit (melee/ranged/magic/powered) | `rules/formulas.rs`, `combat::damage_resolver::{analyze, roll}`, `combat::MaxHitSource` | weapon `powered`, spell `base_max_hit` |
| §5 multiplier order, non-stacking, 200 cap | `rules::apply_multipliers`, `Modifier::applicable` | modifier `group`/`priority` |
| §6 poison/venom | `combat::StatusEffectTracker` | npc/spell `on_hit` |
| §7 attack speed | `AttackDefinition::with_attack_speed`, `AttackPlan::speed` | weapon `attack_speed`, npc attack `speed` |
| Special attacks | `data::SpecialAttackDef`, `Player::set_special_enabled` | weapon `special` |
| Food and potions | `data::ConsumableDef`, `Player::use_consumable` | `consumables.json` |
| Phoenix necklace, Redemption | `Player::after_damage` | item/prayer `tags` |

**Not modelled (yet):** Smite and Retribution (PvP / death effects); PvP's 40% protection; ammo
consumption and ammo-saving effects; inventory limits (anything in the registry can be worn or used
freely); multi-hit specials with per-hit rolls that differ (Dragon claws' cascade, Dark bow's
minimum hit); Blood barrage healing and Shadow barrage defence drain; stat-reducing special
attacks (Bandos godsword); line of sight; the prayer-level/Magic-level *quest* unlocks (Ancient
Magicks needs Desert Treasure — the simulator only checks the Magic level). Item bonuses marked
`"notes": "... from memory"` in `data/items.json` are unverified recollections of the wiki's
equipment tables and should be checked before being relied on for exact numbers.
