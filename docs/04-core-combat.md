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

File: [`attack_definition.rs`](../osrs_core/src/combat/attack_definition.rs).

| Field | Meaning |
|---|---|
| `name` | for logs |
| `style` | `Melee`, `Ranged` or `Magic` ([`AttackStyle`](../osrs_core/src/combat/attack_style.rs)) |
| `damage_type` | which defence bonus it rolls against; defaults from the style (melee defaults to slash) |
| `range` | a melee-style attack with `range` 1 or less must be orthogonally adjacent to the attacker's footprint; anything else is within Chebyshev `range` (a halberd is melee style with range 2) |
| `telegraph_ticks` | visible wind-up before it resolves |
| `impact_delay_ticks` | extra ticks between the telegraph ending and the roll ("Doom-style") |
| `projectile_travel_ticks` | ticks between the roll and the damage landing |
| `max_hit` | a [`MaxHitSource`](../osrs_core/src/combat/max_hit_source.rs) (below) |
| `modifiers`, `flags` | conditional effects and the flags they test (chapter 3.4) |
| `hit_count` | separate rolls per use (dragon claws: 4) |
| `always_hits` | skip the accuracy roll entirely |
| `prayer_penetration` | the fraction of damage that gets through the *correct* protection prayer: 0 = fully blocked, 1 = ignores prayer |
| `reaction` | what the player must do to avoid it ([`ReactionRequirement`](../osrs_core/src/combat/reaction_requirement.rs)) |
| `on_hit_status_effect` | poison, freeze, burn... applied when it lands |
| `ground_target` | aim at tiles instead of at the player: it hits whoever stands on them when `impact_delay_ticks` is up (see 4.10) |
| `cooldown_ticks` | wait after resolving before the next telegraph may start |

`AttackDefinition::new` takes the required fields and fills the rest with defaults; the `with_*`
methods set the others (primer 1.4). They validate their input and panic on nonsense, because a
bad attack is a bug in data or code, not something a player can cause.

Definitions are shared as `Rc<AttackDefinition>` (`.shared()`), because the pattern that owns the
list, the state machine currently telegraphing it and the queue that later rolls it all refer to
the same attack.

### `MaxHitSource`

| Variant | Base max hit |
|---|---|
| `Derived` | melee/ranged formula from the attacker's effective strength and strength bonus |
| `Fixed(n)` | a monster's listed max hit |
| `Spell { base }` | a spell's fixed base, scaled only by Magic Damage % |
| `PoweredStaff { divisor, offset, gear_bonus_scale }` | `floor(magic / divisor) + offset`, then Magic Damage % (the gear part multiplied by `gear_bonus_scale`, capped at +100 %) |

### `with_attack_speed`: turning OSRS attack speed into a cooldown

OSRS gives each attack a speed: the number of ticks between consecutive hits. An attack without a
telegraph resolves on the tick it is chosen; one with a telegraph spends `telegraph_ticks` winding
up first (4.4). `with_attack_speed(speed)` subtracts that wind-up:

```
busy           = telegraph_ticks
cooldown_ticks = max(0, speed - 1 - busy)
```

The test `attack_speed_spaces_consecutive_resolves_exactly_that_many_ticks_apart` proves that
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
          └──────────── cooldown_remaining = attack.cooldown_ticks ◄──────────┘ (> 0: return false)
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
* **Timing arithmetic.** With `telegraph_ticks = 0` (most monster melee), an attack chosen on
  tick T resolves on T itself: the swing and its damage roll happen together, as in the game.
  With `telegraph_ticks = 3`, it resolves on T+3. That is why "busy" in 4.2 is `telegraph_ticks`.
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

Files: [`reaction_requirement.rs`](../osrs_core/src/combat/reaction_requirement.rs),
[`reaction_resolver.rs`](../osrs_core/src/combat/reaction_resolver.rs),
[`player_reaction_state.rs`](../osrs_core/src/combat/player_reaction_state.rs),
[`protection_prayer.rs`](../osrs_core/src/combat/protection_prayer.rs).

An attack's `reaction` says what the player must do:

| `ReactionKind` | Damage multiplier |
|---|---|
| `None` | 1 |
| `PrayerMatch` | `prayer_penetration` if the player's active protection prayer equals `required_prayer`, else 1 |
| `MoveToTile` | 0 if the player is at least `move_away_distance` (Chebyshev) from the hazard's origin tile, else 1. The origin is the attacker's or target's tile **at selection time** (`HazardAnchor`). |
| `Interrupt` | 1 (not implemented yet) |

[`get_damage_multiplier`](../osrs_core/src/combat/reaction_resolver.rs) reads the player's
`PlayerReactionState` (active protection prayer and current fighting style) **at the moment of the
roll**. This is the mechanic behind "prayer flicking": you can hold the wrong prayer for the whole
wind-up and still block the hit by switching on the resolve tick, and you can lose a correct
prayer's protection by dropping it one tick early.
[`prayer_check_timing_tests.rs`](../osrs_core/src/combat/prayer_check_timing_tests.rs) pins this
for all three timing archetypes (4.8).

Data-defined monster attacks with `"protectable": true` (the default) get
`ReactionRequirement::prayer_match(<matching protection>)` automatically.

---

## 4.8 Delayed damage: three timing archetypes

OSRS attacks fall into three timing shapes, and the code supports all of them with two buffers.

| Archetype | Settings | When the roll happens | When damage lands |
|---|---|---|---|
| Traditional | telegraph 0, impact delay 0 | the tick the attack is chosen | after `projectile_travel_ticks` (melee: that same tick) |
| Jad-style | telegraph > 0 | the resolve tick, after a long wind-up | after `projectile_travel_ticks` |
| Doom-style | impact delay > 0 | `impact_delay_ticks` after resolving | after `projectile_travel_ticks` |

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

Files: [`ground_target.rs`](../osrs_core/src/combat/ground_target.rs),
[`pending_ground_strike.rs`](../osrs_core/src/combat/pending_ground_strike.rs).

An attack with `ground_target` set is aimed at the ground, not at the player: it covers a set of
tiles and, once its own `impact_delay_ticks` is up, rolls against **whoever is standing on them
then**. Falling rocks, grasping claws and rock falls are all this. It is a property of an attack, so
it telegraphs, is chosen by patterns, and has damage, style, graphics and sounds like any other.

A `GroundTarget` is an `anchor` (`SelfTile` or `Target`, as for a reaction) plus a `TileSelection`:

| `TileSelection` | Covers |
|---|---|
| `Offsets(Vec<(dx, dz)>)` | each offset from the anchor: squares (`GroundTarget::square(anchor, 1)` is the 3x3), crosses, lines, claw patterns; need not be contiguous |
| `Scatter { radius, count }` | `count` distinct random tiles within `radius` of the anchor |

The tiles are fixed when the attack resolves, from the attacker's and target's tiles as they were at
selection. That tile is all that is taken from the target: the strike carries no target and no
target stats.

When the attack resolves, `fire_at_ground` announces it like any attack (animation, sound) and
queues a `PendingGroundStrike` on the **world** (it has no defender, and must still land if its
attacker dies). `advance_ground_strikes` counts them down from the encounter clock; when one lands,
`targets_on_tiles` finds everything alive whose footprint overlaps its tiles, those a monster's
attack may hurt (only the player so far) are kept, and `roll_on_player` rolls the attack against
each. A strike with no delay lands the tick it resolves. Strikes are cleared when the player
respawns.

Persistent hazards (Hunllef's tornadoes, Purutu's orbs, fire walls) are a different thing and are
not this: they live in the world as entities. See issue #72.

---|---|
| `delay_ticks` | ticks between being conjured and landing |
| `area` | tile offsets it covers around where it stands (`square_area(1)` is the 3x3) |
| `effect` | an ordinary `AttackDefinition` resolved against each valid target found |
| `movement` | `Stationary`, or `HomeOnTile` (one greedy step per tick, ignoring collision) |

It is conjured on `NpcCombatState::hazard_origin` for its attack: the target's tile at selection
when the attack's reaction is anchored to the target, otherwise the conjurer's own tile. That tile
is all that is taken from the target; the hazard carries no target and no target stats. When it
lands, the world asks `targets_on_tiles` who is on its covered tiles, keeps those a monster's
hazard may hurt (only the player so far), and resolves the effect against each with `deliver_attack`.

Repeating, or reacting to a hit (Purutu's pulsing, life-extending orb), is deliberately not part of
a hazard: it would be a behavior of that one hazard. Hazards are cleared when the player respawns.
No bundled content conjures one yet.

---|---|---|
| `HomeOnTarget` | one greedy step toward the player each tick, ignoring collision (it floats) | Purutu's Slicer orb |
| `Stationary` | stays where it was conjured | fire walls, acid pools, gas vents |

Hazards are cleared when the player respawns. No bundled content conjures one yet.

---

## 4.11 Small pieces

* [`heal_resolver::resolve(amount, health)`](../osrs_core/src/combat/heal_resolver.rs) heals and
  returns how much was actually restored (clamped at max).
* [`PrayerPool`](../osrs_core/src/combat/prayer_pool.rs) is current/max prayer points with
  clamped drain and restore, the same shape as `Health`.
* [`test_support.rs`](../osrs_core/src/combat/test_support.rs) (tests only) provides a
  `FakeCombatTarget` with health, a view and a damage buffer.
