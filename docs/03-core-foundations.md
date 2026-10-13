# 3. Core foundations: tiles, randomness, rules, model

These four modules sit at the bottom of `osrs_core`. They have no knowledge of entities or ticks.
Everything above them builds on what they define.

---

## 3.1 `tiles`: positions and directions

Files: [`tiles/tile_coord.rs`](../osrs_core/src/tiles/tile_coord.rs),
[`tiles/direction.rs`](../osrs_core/src/tiles/direction.rs),
[`tiles/world_tile.rs`](../osrs_core/src/tiles/world_tile.rs).

### `TileCoord`

```rust
pub struct TileCoord { pub x: i32, pub z: i32, pub plane: i32 }
```

It derives `Copy`, `Eq` and `Hash`, so it behaves like a Python tuple: cheap to pass, usable as a
dictionary key. Two tiles on different planes are never equal.

| Method | Meaning |
|---|---|
| `TileCoord::new(x, z)` | plane 0 |
| `with_plane(x, z, p)` | explicit plane |
| `chebyshev_distance_to(other)` | `max(|dx|, |dz|)`: the number of steps between two tiles, since diagonals cost one step |
| `squared_distance_to(other)` | `dx² + dz²`, used only to break ties in the pathfinder's fallback |
| `tile + Direction::North` | the neighbouring tile (the `Add` trait implements `+`) |

### `Direction`

Nine variants: the eight compass directions plus `None`. `None` exists so the pathfinder can use
`Direction` as a breadcrumb ("I arrived here from the west") where `None` means "unvisited" or
"this is the start".

* `delta_x()` / `delta_z()` give the step, from two lookup tables indexed by variant.
* `cardinal_components()` splits a diagonal into its two straight parts (`NorthEast` becomes
  `(North, East)`). Collision uses this to forbid corner-cutting.
* [`EXPANSION_ORDER`](../osrs_core/src/tiles/direction.rs) is the fixed order in which the
  pathfinder tries neighbours: W, E, S, N, SW, SE, NW, NE. Because the pathfinder keeps the
  *first* way it reaches a tile, this order decides which of several equally short paths is
  chosen. It reproduces the game's "straight leg, then diagonal leg" path shape. Changing it
  changes how every path looks.

### World/tile conversion

`world_to_tile(x, z, plane)` floors (never truncates) so that `-0.1` maps to tile `-1`.
`tile_to_world_center` returns `(x + 0.5, z + 0.5)`. These come from the C# Unity version and
are not used by the Bevy client, which has its own render-space conventions (chapter 2.6).

---

## 3.2 `rng`: reproducible randomness

File: [`rng.rs`](../osrs_core/src/rng.rs).

```rust
pub trait Rng {
    fn next_double(&mut self) -> f64;                 // [0, 1)
    fn next_range(&mut self, min: i32, max: i32) -> i32;   // [min, max)
}
```

These are the only two random operations the game logic needs. Code that rolls dice takes
`&mut dyn Rng`, so it works with either implementation:

* **`DotNetRandom`** is a faithful port of .NET's `System.Random(seed)` (a Knuth subtractive
  generator with a 56-entry state array). The port uses `wrapping_sub`/`wrapping_add` to reproduce
  32-bit overflow exactly. Its test checks published .NET outputs: seed 0 must give
  1,559,595,546 and seed 42 must give 1,434,747,710. Matching .NET means the Rust simulation and
  the original Unity C# draw identical numbers from the same seed.
* **`ScriptedRng`** returns a fixed list of doubles and ints in order, and panics if the code asks
  for one more than expected. Tests use it to force a hit or a miss: `ScriptedRng::new(vec![0.0],
  vec![7])` means "the accuracy roll passes, the damage roll is 7".

Python comparison: `random.Random(seed)` is reproducible too, but only within CPython's
Mersenne Twister. Here the requirement is reproducibility across two languages, so the algorithm
itself had to be copied.

---

## 3.3 `rules`: the OSRS formulas as pure functions

Files: [`rules/formulas.rs`](../osrs_core/src/rules/formulas.rs),
[`rules/ratio.rs`](../osrs_core/src/rules/ratio.rs),
[`rules/resources.rs`](../osrs_core/src/rules/resources.rs).

This module is the project's "physics". Every function takes numbers and returns numbers. There
are no entities, no world and no randomness, so each function is trivially testable, and each
test quotes a known value from the OSRS Wiki. The specification these follow is
[`unity_context/OSRS_COMBAT_REFERENCE.md`](../unity_context/OSRS_COMBAT_REFERENCE.md).

Integer arithmetic is used wherever the real game floors, so results match the game exactly
rather than approximately. Watch for `div_euclid` (floor division; primer 1.3).

### `Ratio`: exact multipliers

```rust
pub struct Ratio { pub num: i64, pub den: i64 }
impl Ratio { pub fn apply(self, value: i64) -> i64 { (value * self.num).div_euclid(self.den) } }
```

OSRS multiplies by gear bonuses like 7/6 (Slayer helmet) and 11/10 (Void) and floors after each
one. Floats could round the wrong way at a boundary, so multipliers are stored as fractions.
`Ratio::ONE` is the identity. Ratios appear in JSON as `{"num": 7, "den": 6}`.

### Levels and rolls

The accuracy and damage pipeline (reference §3 and §5):

```
effective_level = floor( floor(level * prayer_percent / 100) + stance_bonus + 8 ) then × void ratio, floored
roll            = effective_level * max(0, equipment_bonus + 64)
```

| Function | Formula / meaning |
|---|---|
| `effective_level(level, prayer_percent, stance_bonus, void)` | as above. `level` already includes potion boosts. `prayer_percent` is 100 with no prayer, 123 for Piety's strength part. |
| `roll(effective, bonus)` | attack or defence roll; a bonus below -64 is clamped so the roll is never negative |
| `monster_defence_roll(level, bonus)` | monsters defend with `level + 9`, not the player pipeline |
| `player_magic_defence_level(eff_magic, eff_defence)` | players defend against magic with `floor((7 * magic + 3 * defence) / 10)` |
| `hit_chance(att, def)` | if `att > def`: `1 - (def + 2) / (2 * (att + 1))`, else `att / (2 * (def + 1))` |
| `base_max_hit(eff_strength, strength_bonus)` | `floor((eff * (bonus + 64) + 320) / 640)`; used for melee and ranged |
| `apply_multipliers(value, &[Ratio])` | applies each ratio with a floor in between, then clamps to 0..=200 |
| `MAX_HIT_CAP` | 200, the game's ceiling on one hit |
| `magic_max_hit(base, permille)` | `floor(base * (1000 + permille) / 1000)`. All "Magic Damage %" sources are **summed** first, then floored once. |
| `powered_staff_base_max_hit(magic, divisor, offset)` | `floor(magic / divisor) + offset`, e.g. Trident of the seas is `(3, -5)` |
| `twisted_bow_accuracy_percent` / `twisted_bow_damage_percent` | the twisted bow's scaling curves against the target's Magic, capped at 140 % and 250 % |
| `combat_level(att, str, def, hp, pray, rng, mag)` | `floor(0.25*(def + hp + floor(pray/2)) + 0.325*max(att+str, 1.5*rng, 1.5*mag))`, computed exactly in integers scaled by 1000 |

A worked example from the tests: 99 Strength + 19 from a Super strength potion = 118. Piety:
`floor(118 * 123 / 100) = 145`. Aggressive stance +3, plus 8, gives effective strength 156. With
+86 strength bonus, `floor((156 * 150 + 320) / 640) = 37`.

### Resources over time

[`resources.rs`](../osrs_core/src/rules/resources.rs) holds the per-tick rates:

| Resource | Rule |
|---|---|
| HP regen | +1 HP every 100 ticks; Rapid Heal *or* a Hitpoints cape halves it; a Regeneration bracelet halves it again |
| Stat drift | a boosted or drained stat moves one level toward base every 100 ticks; Preserve slows only the decay of boosts (to 150) |
| Potion boost | `flat + floor(base * percent / 100)` (Super strength is 5 + 15 %) |
| Prayer drain | `PrayerDrainCounter`: each tick add the sum of the active prayers' "drain effect"; whenever the counter **exceeds** `60 + 2 * prayer_bonus`, lose one point and subtract that amount |
| Smite / Redemption | a quarter of damage dealt / a quarter of Prayer level |
| Run energy | stored in hundredths of a percent (10,000 = full). Drain per running tick `floor(60 + 67 * min(weight, 64) / 64) * (1 - agility/300)`, ×0.3 with a stamina potion, ×0.85 with a ring of endurance. Regen per non-running tick `floor(agility / 10) + 15`, ×1.3 in full Graceful. |
| Special energy | +10 % every 50 ticks (25 with a Lightbearer ring), max 100 |
| Poison | every 30 ticks, deal `ceil(severity / 5)`, then severity drops by 1; ends at 0 |
| Venom | every 30 ticks, damage goes 6, 8, 10 ... capped at 20; never wears off by itself |
| Projectile travel | melee 0 ticks; ranged `1 + floor((3 + distance) / 6)`; magic `1 + floor((1 + distance) / 3)` |

`projectile_travel_ticks` is at the bottom of `resources.rs` and takes an `AttackStyle` from the
`combat` module, which is the one place `rules` reaches upward.

---

## 3.4 `model`: the shared vocabulary

Files: [`model/`](../osrs_core/src/model/). These are small, mostly `Copy` types with serde support,
used by both the data layer (to parse JSON) and the combat layer (to compute).

### Damage types and bonuses

* [`DamageType`](../osrs_core/src/model/bonuses.rs): `Stab`, `Slash`, `Crush`, `Ranged`, `Magic`.
  Each maps to an `AttackStyle` (`Melee`, `Ranged`, `Magic`). An attack rolls against the
  defender's defence bonus of the **same** damage type.
* `StyleBonuses`: one number per damage type.
* `EquipmentBonuses`: an attack table, a defence table, melee strength, ranged strength, magic
  damage (in permille: +10 % is 100) and prayer. Wearing gear is summing these
  (`EquipmentBonuses::sum`). Its JSON uses `#[serde(default)]`, so `{"attack": {"slash": 82}}` is
  enough; everything else is 0.
* [`Skill`](../osrs_core/src/model/skill.rs): the seven combat skills, with `index()` for array
  storage.

### Monster attributes

[`Attribute`](../osrs_core/src/model/attributes.rs) lists monster tags such as `Undead`, `Demon`,
`Draconic`, `Kalphite`, `Leafy`, `Flying`, `Xerician`. Gear exploits them (a Salve amulet works
only on undead). `AttributeSet` stores a set of them as bits of a `u32` so it is `Copy` and can
sit inside `CombatStats`; in JSON it is a plain list (`#[serde(from/into = "Vec<Attribute>")]`).

### Prayer and stance effects

[`PrayerEffects`](../osrs_core/src/model/effects.rs) holds percentages (100 = no change) for
attack, strength, defence, ranged attack, ranged strength and magic, plus a magic-damage permille.
`combine` merges two prayers: for each stat the larger percentage wins (same-stat prayers are
mutually exclusive in the game, so they never compound), and the damage permilles add.

`StanceBonuses` are the invisible +1/+3 bonuses that the selected combat stance adds to effective
levels. `ranged` applies to both ranged accuracy and ranged strength.

### `Modifier`: one primitive for every conditional effect

File: [`model/modifier.rs`](../osrs_core/src/model/modifier.rs).

The game is full of gear and effects that change a roll under some condition: Void (+10 % if the
full set is worn), Slayer helmet (+1/6 on task), Salve amulet (vs undead), Dragon hunter lance
(vs dragons), Keris (×3 damage vs kalphites one time in 51), Dharok's (more damage the lower your
HP), special attacks, and so on. Rather than writing code for each item, the project expresses
all of them with one data type:

```rust
pub struct Modifier {
    pub target: ModifierTarget,         // which step of the roll it changes
    pub ratio: Ratio,                   // the multiplier (default 1/1)
    pub add: i32,                       // for additive targets
    pub condition: Condition,           // when it applies (default: always)
    pub group: Option<String>,          // non-stacking group
    pub priority: i32,                  // winner within a group
    pub chance_one_in: i32,             // DamageRoll only: applies 1 time in N
    pub scaling: Option<ModifierScaling>,   // ratio computed at attack time
}
```

**Targets**, in the order the damage resolver applies them:

| `ModifierTarget` | Effect | Examples |
|---|---|---|
| `AccuracyLevel` | multiplies the effective attack level | Void |
| `DamageLevel` | multiplies the effective strength level | Void |
| `AttackRoll` | multiplies the attack roll | Slayer helmet, Salve, Dragon hunter, special-attack accuracy |
| `TargetDefenceRoll` | multiplies the defender's roll; ratios below 1 reduce it | defence-reducing specials |
| `MagicDamagePermille` | adds `add` to the summed Magic Damage % | Augury, Salve on magic |
| `MaxHit` | multiplies the max hit, flooring | Slayer helmet, Salve, special damage, Dharok's |
| `MaxHitAdd` | adds `add` to the max hit after multipliers | |
| `DamageRoll` | multiplies each rolled hit after the roll, optionally only 1 time in `chance_one_in` | Keris, Gadderhammer |

**Conditions** form a small expression language evaluated against the attack being made:

```rust
pub enum Condition {
    Always,
    TargetHas(Attribute),      // {"target_has": "undead"}
    Style(AttackStyle),        // {"style": "melee"}
    DamageType(DamageType),    // {"damage_type": "stab"}
    Flag(String),              // {"flag": "on_task"}
    Not(Box<Condition>),       // {"not": ...}
    All(Vec<Condition>),       // {"all": [...]}
    Any(Vec<Condition>),       // {"any": [...]}
}
```

`Condition::holds(&ConditionContext)` evaluates it recursively. The context carries the attack's
style, damage type, the target's attributes and the attack's **flags**. Flags are strings the
player's attack carries: `on_task`, `special`, `weapon:<id>`, `weapon_category:<c>`,
`weapon_tag:<t>`, `stance:<name>`, `ammo:<family>`, `ammo_item:<id>`, `ammo_weight:<w>`,
`spell:<id>`, `spellbook:<book>` (built in `Player::plan_attack`; chapter 6).

`Box<Condition>` in `Not` is needed because a type cannot contain itself directly (its size would
be infinite); the box puts the inner condition on the heap.

**Groups** stop effects from stacking. The Salve amulet and Slayer helmet both carry
`"group": "slayer_or_salve"`. `Modifier::applicable(list, target, ctx)` filters to modifiers with
the right target whose condition holds, then within each group keeps only the highest priority
one (the first in list order wins a tie). Modifiers without a group always apply.

**Scaling** lets a modifier's ratio depend on live state. `resolved(&ScalingContext)` replaces the
ratio when the attack is built:

| `ModifierScaling` | Ratio |
|---|---|
| `MissingHitpoints` (Dharok's) | `(10000 + missing_hp * max_hp) / 10000`, i.e. `1 + missing/100 × max/100` |
| `TwistedBowAccuracy` | `twisted_bow_accuracy_percent(target_magic, cap) / 100` |
| `TwistedBowDamage` | `twisted_bow_damage_percent(target_magic, cap) / 100` |

The cap is 250, or 350 against `Xerician` targets.

Builder helpers (`Modifier::multiplier`, `additive`, `.when`, `.in_group`, `.with_chance_one_in`,
`.scaled`) exist for tests and Rust-defined content; data files write the struct as JSON:

```json
{ "target": "max_hit", "ratio": {"num": 7, "den": 6},
  "condition": {"all": [{"flag": "on_task"}, {"style": "melee"}]},
  "group": "slayer_or_salve", "priority": 1 }
```
