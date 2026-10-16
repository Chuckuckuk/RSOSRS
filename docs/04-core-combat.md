# 4. Combat

Folder: [`osrs_core/src/combat/`](../osrs_core/src/combat/).

This chapter follows one attack through its whole life: how it is described, how an NPC chooses
it, how long it takes, how its damage is rolled, and how that damage reaches a health bar. The
`World` that drives all this is in chapter 6; this chapter covers the parts it calls.

---

## 4.1 The life of an attack

```
  describe          choose              wait                roll                 fly                land
┌──────────┐   ┌──────────────┐   ┌──────────────┐   ┌──────────────┐   ┌──────────────┐   ┌──────────────┐
│ Attack-  │──►│ AttackPattern│──►│ NpcCombat-   │──►│ damage_      │──►│ DamageBuffer │──►│ Health       │
│ Definition│  │ .select_next │   │ State        │   │ resolver::   │   │ (travel      │   │ .damage()    │
│ (data)   │   │              │   │ (telegraph)  │   │ roll         │   │  ticks)      │   │ + status fx  │
└──────────┘   └──────────────┘   └──────┬───────┘   └──────▲───────┘   └──────────────┘   └──────────────┘
                                         │ impact delay > 0 │
                                         └─► PendingAttack- ┘
                                             Queue (on the defender)
```

The player's own attacks skip the "choose" and "wait" boxes: the player builds an attack from
gear when it swings (chapter 6), then joins the same roll and buffer path.

---

## 4.2 Describing an attack: `AttackDefinition`

Files: [`attack_definition.rs`](../osrs_core/src/combat/attack_definition.rs),
[`targeted_attack.rs`](../osrs_core/src/combat/targeted_attack.rs),
[`ground_attack.rs`](../osrs_core/src/combat/ground_attack.rs),
[`projectile_speed.rs`](../osrs_core/src/combat/projectile_speed.rs).

An attack is the fields every attack has, plus a `kind` that says *when* it resolves and holds only
what that kind uses, so a boulder does not carry (and silently ignore) an accuracy modifier, and a
melee swing does not carry a telegraph length it never uses.

**Common to every attack** (`AttackDefinition`):

| Field | Meaning |
|---|---|
| `name` | for logs |
| `range` | optional override of the attacker's own attack range for this attack (below); `None` uses the attacker's |
| `attack_speed` | optional override of the attacker's own attack speed for this attack (below) |
| `on_hit_status_effect` | poison, freeze, burn... applied when it lands |
| `animation`, `projectile`, `target_graphic`, `impact_graphic`, `sound`, `hit_sound` | presentation only |
| `kind` | when and how it resolves, below |

**`AttackKind`**: an attack is either *standard* or a *special*. A standard attack is one of three
timing archetypes, each carrying only its own numbers, and is answered by nothing but the overhead
protection prayer. Anything that does not fit those, or that must check anything else, is a special.

| Kind | Resolves | Own fields |
|---|---|---|
| `Instant` | the tick it is chosen (the "traditional" archetype: most melee) | `projectile_speed` (override) |
| `Telegraphed` | when a visible wind-up ends (Jad) | `telegraph_ticks`, `projectile_speed` (override) |
| `OnContact` | the roll waits until it lands, `impact_delay_ticks` after release (Doom-style) | `impact_delay_ticks` |
| `Special` | resolves itself, by the `SpecialAttack` it carries (below) | its own |

A `SpecialAttack` is the extension point for everything that is not a plain roll against one
defender. The only one so far is `Ground`: on the tiles it covers, when its telegraph ends (4.10),
with `telegraph_ticks`, tiles and damage.

The three standard kinds wrap a **`TargetedAttack`**, which is what is rolled against a defender:

| Field | Meaning |
|---|---|
| `style` | `Melee`, `Ranged` or `Magic` ([`AttackStyle`](../osrs_core/src/combat/attack_style.rs)) |
| `damage_type` | which defence bonus it rolls against; defaults from the style (melee defaults to slash) |
| `max_hit` | a [`MaxHitSource`](../osrs_core/src/combat/max_hit_source.rs) (below) |
| `modifiers`, `flags` | conditional effects and the flags they test (chapter 3.4) |
| `hit_count` | separate rolls per use (dragon claws: 4) |
| `always_hits` | skip the accuracy roll entirely |
| `prayer_penetration` | the fraction of damage that gets through the *correct* protection prayer: 0 = fully blocked, 1 = ignores prayer |
| `mitigation` | what lets the defender cut it down ([`Mitigation`](../osrs_core/src/combat/mitigation.rs)): the protection prayer for its style by default |

The damage resolver, the reaction resolver, `PendingAttackQueue` and the player's swings take a
`TargetedAttack`, so rolling a ground attack against a defender's accuracy cannot be written.

**Projectile flight.** An `Instant` or `Telegraphed` attack rolls when it is released and its damage
lands after the flight. How long the flight is comes from a `ProjectileSpeed`: chebyshev tiles per
tick, `ceil(distance / speed)` and never less than one tick, with an engine-wide default
(`ProjectileSpeed::DEFAULT`, 3 tiles a tick) that an attack may override. Melee has no flight. An
`OnContact` attack has none either: its impact delay *is* the time until it lands. The player's
ranged and magic weapons override the default with the game's own flight formulas
(`ProjectileSpeed::OsrsRanged` and `OsrsMagic`), so their timing is unchanged.
`AttackDefinition::flight_ticks(distance)` is the one place that answers it.

Constructors: `AttackDefinition::instant(name, range, style)`, `telegraphed(name, range,
telegraph_ticks, style)`, `on_contact(name, range, impact_delay_ticks, style)` and `ground(name,
range, telegraph_ticks, target, damage)`, where `range` is an `Option` (`None` inherits the
attacker's). Data and config, which only have a wind-up number, use
`npc_factory::timed_attack(name, range, telegraph_ticks, style)`, which picks `Instant` or
`Telegraphed` from it. The `with_*` methods set the rest (primer 1.4).
They validate their input and panic on nonsense, including a builder for a field the attack's kind
does not have (`with_always_hits` on a ground attack, `with_projectile_speed` on an `OnContact`
one), because a bad attack is a bug in data or code, not something a player can cause.

### `MaxHitSource`

| Variant | Base max hit |
|---|---|
| `Derived` | melee/ranged formula from the attacker's effective strength and strength bonus |
| `Fixed(n)` | a monster's listed max hit |
| `Spell { base }` | a spell's fixed base, scaled only by Magic Damage % |
| `PoweredStaff { divisor, offset, gear_bonus_scale }` | `floor(magic / divisor) + offset`, then Magic Damage % (the gear part multiplied by `gear_bonus_scale`, capped at +100 %) |

### Attack range: a stat of the attacker

How far an attacker can strike from is a stat of the attacker too (`NpcDef::attack_range`, the
weapon's range for the player), and an attack may name its own `range` to replace it. The reach an
attack is measured by is `attack.range` if it has one, else the attacker's:

* a **melee-style** attack whose reach is 1 or less must be orthogonally adjacent to the
  attacker's footprint;
* anything else (a ranged or magic attack, a melee attack with a longer reach such as a halberd's
  2, and every ground attack) is within that many tiles.

So an archer with `attack_range: 7` has attacks that inherit 7, while Jad (`attack_range: 15`)
gives his melee swing `range: 1`, which keeps it needing adjacency.
`AttackDefinition::build_range_check(tile, footprint, attacker_range)` answers the outer reach.

An attack may also name a `min_range`: the fewest tiles away it can be used from. With `range`
that is a distance band, so one monster can have different attacks available at different
distances (a lob that only works from 4+ tiles, a swipe that only works up close).
`AttackDefinition::is_in_reach(tile, footprint, attacker_range, target)` applies both bounds, and
`RangeAwarePattern` and the plain NPC AI use it to decide what is usable and whether to engage.
`min_range` is measured to the nearest tile of the attacker's footprint, and validation rejects
one beyond the attack's own reach.

### Attack speed: a stat of the attacker

OSRS gives an attacker a rate: the number of ticks between consecutive attacks resolving. That is a
stat of the attacker, not of each attack: `NpcCombatState::with_attack_speed(speed)` for a monster
(`NpcDef::attack_speed`; the player's comes from the weapon). An attack may carry its own
`attack_speed` (`with_attack_speed`) that replaces the attacker's for as long as that attack is the
one resolving (a slow special among fast swings).

An attack without a telegraph resolves on the tick it is chosen; one with a telegraph spends
`telegraph_ticks` winding up first (4.4). So after an attack resolves the state idles for:

```
speed    = attack.attack_speed or the attacker's attack speed
busy     = telegraph_ticks
cooldown = max(0, speed - 1 - busy)
```

The tests `attack_speed_spaces_consecutive_resolves_exactly_that_many_ticks_apart` and
`the_attackers_own_attack_speed_spaces_its_attacks_and_an_attack_may_override_it` prove that
resolves then land exactly `speed` ticks apart.

---

## 4.3 Who is fighting: `CombatStats`

File: [`combat_stats.rs`](../osrs_core/src/combat/combat_stats.rs).

`CombatStats` is a `Copy` bundle of everything a roll reads about one combatant:

* five **current** levels (attack, strength, defence, ranged, magic), already including potion
  boosts or drains; hitpoints live in `Health` instead;
* the summed `EquipmentBonuses`;
* the combined `PrayerEffects`;
* the stance's `StanceBonuses`;
* the monster `AttributeSet`.

`CombatStats::new` checks each level is at least 1 and returns a `Result`. Extras default to
neutral, so a bare `new(...)` behaves like "no gear, no prayer". Players rebuild theirs whenever
gear, prayers, stance or boosts change (`Player::refresh_stats`); monsters build theirs once from
data (`npc_factory::stats_from_def`, which also adds a +1 stance bonus so monsters attack at
`level + 9`).

Its methods turn levels into the numbers the formulas need:

| Method | Returns |
|---|---|
| `effective_accuracy_level(style)` | `rules::effective_level` on Attack, Ranged or Magic with the matching prayer and stance |
| `effective_damage_level(style)` | the same on Strength, Ranged or Magic |
| `effective_defence_level()` | the same on Defence |
| `accuracy_level_base(style)` | the visible level (powered staves scale from it) |
| `strength_bonus_for(style)` | melee or ranged strength bonus (0 for magic) |
| `defence_roll_against(damage_type, defender_is_monster)` | a monster: `(level + 9) * (bonus + 64)` using Defence, or Magic against magic. A player: the effective pipeline, against magic using `70 % magic + 30 % defence`. |

[`combat_math.rs`](../osrs_core/src/combat/combat_math.rs) is an older, simplified version of
these formulas (no gear, no prayers) kept from the first stage of the C# port. It is re-exported
but the live code paths use `rules` and `damage_resolver` instead.

---

## 4.4 NPC attack timing: `NpcCombatState`

File: [`npc_combat_state.rs`](../osrs_core/src/combat/npc_combat_state.rs).

Each fighting NPC owns one of these. It is a two-state machine, advanced once per tick in the
`NpcAi` phase:

```
              cooldown_remaining > 0: count down, return false
              target not engaged:     return false
      ┌──────┐   engaged & cooldown 0: select attack, snapshot tiles    ┌──────────────┐
      │ Idle │ ───────────────────────────────────────────────────────►│ Telegraphing │
      └──────┘                                                         └──────┬───────┘
          ▲      ticks_remaining reaches 0: return TRUE ("resolved"),         │ ticks_remaining -= 1
          └──────────── cooldown_remaining = cooldown after ◄───────────┘ (> 0: return false)
```

`advance(target_engaged, health_percent, elapsed_ticks, self_tile, target_tile, target_style,
rng) -> bool` returns `true` on exactly the tick an attack resolves. The caller then reads
`current_attack()` and decides what to do (roll now, or queue a delayed roll).

Details that matter:

* **Commitment.** Once telegraphing has started, the attack resolves no matter what: the target
  walking out of range mid-wind-up does not cancel it. `target_engaged` only gates *starting*.
* **Snapshots.** `self_tile_at_selection()` and `target_tile_at_selection()` record where both
  sides stood when the attack was chosen. A "move away from where it was cast" hazard uses these,
  not live positions, otherwise stepping away during the telegraph could never dodge it.
* **Selection counter.** The machine counts how many attacks it has chosen and passes the count
  to the pattern. Patterns stay stateless (4.5) and the per-NPC memory lives here.
* **Phase swaps.** `set_pattern` replaces the pattern for future selections without disturbing an
  attack already in progress.
* **Timing arithmetic.** With no telegraph (an `Instant` or `OnContact` attack, most monster
  melee), an attack chosen on tick T resolves on T itself: the swing and its damage roll happen
  together, as in the game. With `telegraph_ticks() = 3`, it resolves on T+3. That is why "busy"
  in 4.2 is the telegraph length.
* **Announcing.** For a telegraphed attack the `World` reports the attack (and its animation)
  on the tick the telegraph starts, so the wind-up is the warning; an attack without one is
  reported as it resolves (chapter 6.4).

---

## 4.5 Choosing an attack: `AttackPattern`

File: [`attack_pattern.rs`](../osrs_core/src/combat/attack_pattern.rs).

```rust
pub trait AttackPattern {
    fn select_next(&self, context: &AttackSelectionContext, rng: &mut dyn Rng) -> Rc<AttackDefinition>;
    /// Some(can an attack reach?), or None when the pattern does not know about reach.
    fn reaches(&self, self_tile: TileCoord, target_tile: TileCoord) -> Option<bool> { None }
    /// May a new attack start? `reaches`, or always when that is unknown.
    fn engages(&self, self_tile: TileCoord, target_tile: TileCoord) -> bool {
        self.reaches(self_tile, target_tile).unwrap_or(true)
    }
}
```

`reaches` returns an `Option<bool>` so "no" and "don't know" stay apart (the Python equivalent
would be returning `True`, `False` or `None`). Movement uses that difference: a monster holds still
only when its pattern answers a definite yes (chapter 6.4).

Patterns must be pure functions of the context and the RNG, with no hidden state, so one pattern
can be shared by many NPCs and selections stay reproducible.
[`AttackSelectionContext`](../osrs_core/src/combat/attack_selection_context.rs) carries the NPC's
health percent, the encounter's elapsed ticks, the selection counter, both tiles, and the style
the player is fighting with.

| Pattern | File | How it picks | RNG draws |
|---|---|---|---|
| `FixedRotationPattern` | [link](../osrs_core/src/combat/fixed_rotation_pattern.rs) | `rotation[counter % len]` (Vorkath-style fixed cycle) | none |
| `WeightedRandomPattern` | [link](../osrs_core/src/combat/weighted_random_pattern.rs) | weights are *functions* of the context, so HP thresholds can shift odds (Vardorvis); all-zero weights fall back to the first entry | one `next_double` |
| `ConditionalPattern` | [link](../osrs_core/src/combat/conditional_pattern.rs) | first branch whose condition closure is true, else a fallback (Zulrah-style scripts) | none |
| `RangeAwarePattern` | [link](../osrs_core/src/combat/range_aware_pattern.rs) | only attacks that can currently reach the target; then fixed cycle or weights | one `next_double` in weighted mode |

`RangeAwarePattern` is what every **data-defined** monster uses (built by
[`sim/npc_factory.rs`](../osrs_core/src/sim/npc_factory.rs)). It overrides `reaches`, so a
monster whose only attack is melee will not start an attack until it is adjacent.

Weights are summed in `f32` exactly as the C# did, so a roll lands in the same bucket in both
languages.

---

## 4.6 Rolling damage: `damage_resolver`

File: [`damage_resolver.rs`](../osrs_core/src/combat/damage_resolver.rs). Re-exported from
`combat` as `analyze_attack`, `roll_damage`, `roll_damage_with_floor` and `resolve_damage`.

The resolver is split in two:

* **`analyze(attack, attacker, defender, defender_is_monster) -> RollAnalysis`** is the
  deterministic half. It returns the attack roll, the defence roll and the max hit. There is no
  hit chance anywhere: a swing is decided by throwing the two dice.
* **`simulate_swings(...)`** is the Monte Carlo: it runs `roll_with_floor` (floor 1, so a hit is
  any nonzero result) `n` times from a seeded RNG and returns the hit rate and the mean damage.
  Every modifier and special case the dice cover is therefore covered, with no formula to keep in
  step. `Player::expected_dps` is built on it (20,000 swings from a fixed seed).
* **`roll_with_floor(...)`** is `analyze` plus dice.

### The pipeline inside `analyze`

```
ctx = { style, damage_type, defender attributes, attack flags }

attack side:
  eff_acc   = attacker.effective_accuracy_level(style)
  eff_acc   = apply AccuracyLevel modifiers (each: multiply, floor)
  att_roll  = eff_acc * (attack_bonus[damage_type] + 64)
  att_roll  = apply AttackRoll modifiers

defence side:
  def_roll  = defender.defence_roll_against(damage_type, defender_is_monster)
  def_roll  = apply TargetDefenceRoll modifiers

max hit:
  base = Fixed(n)               -> n
       | Derived                -> base_max_hit(DamageLevel-modified eff strength, strength bonus)
       | Spell{base}            -> magic_max_hit(base, gear + prayer + MagicDamagePermille mods)
       | PoweredStaff{d,o,s}    -> magic_max_hit(floor(magic/d)+o, min(gear*s, 1000) + prayer + mods)
  max  = apply MaxHit multipliers (floor after each), cap 200
  max += MaxHitAdd modifiers, clamp 0..=200
```

"Apply X modifiers" always means `Modifier::applicable(&attack.modifiers, X, &ctx)`, which
filters by condition and resolves non-stacking groups (chapter 3.4).

### The dice inside `roll_with_floor`

```rust
let analysis = analyze(...);
if !attack.always_hits {
    // each roll is the top face of a die: throw both, the attack must beat the defence
    if rng.next_range(0, att_roll + 1) <= rng.next_range(0, def_roll + 1) { return 0; }   // miss
}
let mut rolled = rng.next_range(0, analysis.max_hit + 1).max(successful_floor);    // 0..=max
for m in DamageRoll modifiers {
    if m.chance_one_in > 1 && rng.next_range(0, m.chance_one_in) != 0 { continue; } // no proc
    rolled = m.ratio.apply(rolled);
}
rolled = min(rolled, 200);
return round_half_away_from_zero(rolled as f32 * damage_multiplier);
```

* **Draw order is part of the contract.** Two `next_range` for accuracy, the attack die then the
  defence die (none if `always_hits`); only on a hit, one `next_range` for damage; then one more per
  chance-based `DamageRoll` modifier. Changing this order changes every seeded fight.
* **A successful hit can roll 0.** That is real OSRS behaviour for monsters. For the player's own
  attacks the world passes `successful_floor = 1`, so the player's hits that pass accuracy deal at
  least 1 and only misses show 0. The extra parameter changes no RNG draws.
* **`damage_multiplier`** comes from the reaction resolver (4.7): 0 for a fully blocked hit,
  `prayer_penetration` for a piercing one, 1 otherwise. The multiply is done in `f32` and then
  rounded half away from zero, to match C#'s `Math.Round` on a float.

`roll` is `roll_with_floor` with a floor of 0, and `resolve` is `roll` followed by
`health.damage(...)`.

---

## 4.7 Avoiding damage: reactions and prayers

Files: [`mitigation.rs`](../osrs_core/src/combat/mitigation.rs),
[`reaction_resolver.rs`](../osrs_core/src/combat/reaction_resolver.rs),
[`player_reaction_state.rs`](../osrs_core/src/combat/player_reaction_state.rs),
[`protection_prayer.rs`](../osrs_core/src/combat/protection_prayer.rs).

Two things avoid damage, one per kind of attack:

* **A targeted attack checks the protection prayer for its style.** Melee, ranged and magic attacks
  are all answered by the matching prayer without saying so (`ProtectionPrayer::against(style)`);
  nothing has to be declared.
* **An area attack is avoided by where the defender stands.** A `GroundAttack` has no mitigation: it
  resolves against whoever is on its tiles when it lands (4.10).
* **Anything else is a special.** A standard attack checks only the overhead protection prayer
  (`Mitigation::Unmitigable` for a typeless one that nothing changes). An attack that must check
  another state of the defender is a `SpecialAttack`, which carries its own check.

When the mitigation is met, `prayer_penetration` of the damage still gets through (0 blocks it, 1
means the prayer does nothing); otherwise the damage is whole.

[`get_damage_multiplier`](../osrs_core/src/combat/reaction_resolver.rs) reads the player's
`PlayerReactionState` (active protection prayer and current fighting style) **at the moment of the
roll**. This is the mechanic behind "prayer flicking": you can hold the wrong prayer for the whole
wind-up and still block the hit by switching on the resolve tick, and you can lose a correct
prayer's protection by dropping it one tick early.
[`prayer_check_timing_tests.rs`](../osrs_core/src/combat/prayer_check_timing_tests.rs) pins this
for all three timing archetypes (4.8).

Data-defined monster attacks are checked against the matching protection prayer by default;
`"protectable": false` makes one `Mitigation::Unmitigable`.

---

## 4.8 Delayed damage: three timing archetypes

OSRS attacks fall into three timing shapes, and the code supports all of them with two buffers.

| Archetype | Kind | When the roll happens | When damage lands |
|---|---|---|---|
| Traditional | `Instant` | the tick the attack is chosen | after the projectile's flight (melee: that same tick) |
| Jad-style | `Telegraphed` | the resolve tick, after a long wind-up | after the projectile's flight |
| Doom-style | `OnContact` | `impact_delay_ticks` after the attack is released | the moment it rolls |

### `PendingAttackQueue`: attacks waiting to roll

File: [`pending_attack_queue.rs`](../osrs_core/src/combat/pending_attack_queue.rs).

When a Doom-style attack resolves, it is not rolled yet. It goes into the **defender's**
`PendingAttackQueue` together with the attacker's ID, a copy of the attacker's stats and the
hazard origin tile. The defender owns it because once an attack is launched, it belongs to
whatever it is flying at.

Each tick `advance` counts every entry down. When one is due it:

1. drops it without rolling if the attacker has died since (asked through the
   `attacker_is_dead` callback; the queue only stores an `EntityId`);
2. computes the reaction multiplier from the defender's **current** state;
3. rolls (`roll_with_floor`) and enqueues the result into the defender's `DamageBuffer` with the
   attack's travel ticks and on-hit effect.

### `DamageBuffer`: rolled hits in flight

File: [`damage_buffer.rs`](../osrs_core/src/combat/damage_buffer.rs).

A list of `(amount, ticks_remaining, on_land_effect)`. `advance(health, status_tracker)` lands
every hit whose time is up (applying damage and the on-land status effect) and counts the rest
down by one. It returns the amounts that landed, in order, which the world turns into `Hit`
events for the client's hit splats. A hit of 0 is kept and lands, so blocks show a blue splat.

`clear()` wipes everything pending, including hits already rolled. That is the Phoenix necklace's
real mechanic: when it triggers, the whole incoming queue is discarded.

Because the attacker enqueues and the defender advances later in the same tick (phase order,
chapter 2.4), a hit with zero travel lands on the tick it was rolled.

---

## 4.9 Status effects

Files: [`status_effect.rs`](../osrs_core/src/combat/status_effect.rs),
[`status_effect_tracker.rs`](../osrs_core/src/combat/status_effect_tracker.rs).

`StatusEffectDefinition` is `(kind, duration_ticks, magnitude)`, built through named constructors
that reject non-positive numbers. Kinds:

| Kind | Effect while active |
|---|---|
| `Burn` | `magnitude` damage every tick |
| `Corruption` | drains `magnitude` prayer points every tick |
| `DisableRunning` | forces walking |
| `Stun` | cannot attack or start a new move |
| `Freeze` | cannot move (can still attack and pray) |
| `Poison` | every 30 ticks deal `ceil(severity/5)`, severity −1; ends at 0 |
| `Venom` | every 30 ticks deal 6, 8, ... up to 20; never ends on its own |

`StatusEffectTracker` holds the active ones for one actor.

* Re-applying a kind **refreshes** its duration and magnitude rather than stacking.
* Poison and venom have special rules: poison immunity blocks both; a venomed target cannot be
  poisoned (since April 2024 poison no longer downgrades venom); venom replaces an active poison;
  re-poisoning keeps the higher severity.
* `advance(health, prayer_pool)` runs once per tick: pulse every effect, then count it down and
  drop expired ones. Corruption needs a prayer pool; NPCs pass `None` and are unaffected.
* Cures and immunity come from consumables (`cure_poison`, `cure_venom`,
  `grant_poison_immunity`).

---

## 4.10 Ground-targeted attacks

Files: [`ground_attack.rs`](../osrs_core/src/combat/ground_attack.rs),
[`ground_target.rs`](../osrs_core/src/combat/ground_target.rs).

A `Special` attack of kind `Ground` is aimed at the ground, not at the player: it covers a set of tiles and,
when its own telegraph ends, deals its damage to **whoever is standing on them then**, and is
gone. Falling rocks, grasping claws and rock falls are all this. It telegraphs, is chosen by
patterns, and has graphics and sounds like any other attack, but it never rolls accuracy and has
no style or damage type.

| `GroundAttack` field | Meaning |
|---|---|
| `telegraph_ticks` | ticks the tiles stay marked; it resolves, and is gone, when they end |
| `target: GroundTarget` | an `anchor` (`SelfTile` or `Target`, as for a reaction) plus a `TileSelection` |
| `damage: GroundDamage` | `Fixed(n)`, or `Uniform { max }` (0..=max, one draw) per target |
| `prayer_reduction` | optionally one protection prayer that cuts the damage but never negates it (Duke Sucellus' aftershock): the correct prayer lets `penetration` of it through |

| `TileSelection` | Covers |
|---|---|
| `Offsets(Vec<(dx, dz)>)` | each offset from the anchor: squares (`GroundTarget::square(anchor, 1)` is the 3x3), crosses, lines, claw patterns; need not be contiguous |
| `Scatter { radius, count }` | `count` distinct random tiles within `radius` of the anchor |

The tiles come from the attacker's and target's tiles as they were at selection. That tile is all
that is taken from the target: the attack carries no target and no target stats.

When the telegraph ends, `fire_at_ground` announces the attack like any other (animation, sound)
and lands it at once: `targets_on_tiles` finds everything alive whose footprint overlaps the tiles,
those a monster's attack may hurt (only the player so far) are kept, and each takes the rolled
damage times the prayer reduction's multiplier into its damage buffer. It is part of its
attacker's own turn, so an attacker that dies mid-telegraph takes it with it.

Anything that outlives its telegraph (Hunllef's tornadoes, Purutu's orbs, fire walls) is a
different thing and is not this: it lives in the world as an entity. See issue #72.

---

## 4.11 Small pieces

* [`heal_resolver::resolve(amount, health)`](../osrs_core/src/combat/heal_resolver.rs) heals and
  returns how much was actually restored (clamped at max).
* [`PrayerPool`](../osrs_core/src/combat/prayer_pool.rs) is current/max prayer points with
  clamped drain and restore, the same shape as `Health`.
* [`test_support.rs`](../osrs_core/src/combat/test_support.rs) (tests only) provides a
  `FakeCombatTarget` with health, a view and a damage buffer.
