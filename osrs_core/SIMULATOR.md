# The data-driven OSRS simulator

`osrs_core` runs OSRS combat from data. Formulas live in code (`src/rules/`), every number a fight
depends on lives in JSON (`data/`), and the server-authoritative tick loop (`src/sim/`) ties them
together. `OSRS_COMBAT_REFERENCE.md` (in `../unity_context/`) is the specification; its
*Implementation map* says where each section is implemented.

```
cargo run --bin simulate -- --list                       # bundled scenarios
cargo run --bin simulate -- whip_vs_zombie --ticks 60    # run one (name or path), verbose
cargo run --bin simulate -- my_fight.json --data ./my_data --quiet
cargo test                                               # unit tests + tests/simulation.rs
```

## Layers

| Layer | Path | Knows about |
|---|---|---|
| Rules | `src/rules/` | Pure functions and counters: effective level, rolls, max hits, regen/drain/poison rates, combat level, twisted bow. No entities, no RNG. |
| Model | `src/model/` | Vocabulary: `DamageType`, `EquipmentBonuses`, `Attribute`, `PrayerEffects`, `Modifier`. `Copy`/serde types. |
| Combat | `src/combat/` | `AttackDefinition`, `CombatStats`, the damage resolver (`analyze` = deterministic half, `roll` = + dice), buffers, status effects. |
| Data | `src/data/` + `data/*.json` | Serde definitions, `Registry` (load, merge, cross-validate), `ScenarioDef`. |
| Sim | `src/sim/` | `World`, `Player` (+ `player_runtime`), entities from data, `Server` (authoritative ticks), scenario builder. |

Nothing outside `sim/` mutates a fight. A client sends `ClientCommand`s to `sim::Server` and reads
`Snapshot`s; rejected commands come back as `Snapshot::messages`.

## Data files

Every `*.json` may hold any mix of `items`, `sets`, `prayers`, `spells`, `npcs`, `consumables`;
`Registry::load_dir` merges a directory and reports **all** problems at once. Unknown keys are
errors (a typo never silently becomes a default). Ids must be unique across files. Every definition
has an optional `notes` string — use it for provenance ("from the wiki's Bandos page", "unverified").

### Items (`items.json`)

```jsonc
{ "id": "abyssal_whip", "name": "Abyssal whip", "slot": "weapon",   // head cape neck ammo weapon body shield legs hands feet ring; omit for an item that is only carried (a vial)
  "bonuses": { "attack": {"slash": 82}, "defence": {}, "melee_strength": 82,
               "ranged_strength": 0, "magic_damage_permille": 0, "prayer": 0 },
  "weight_kg": 0.45,
  "tags": ["regen_bracelet"],                 // lightbearer hitpoints_cape ring_of_endurance phoenix_necklace leaf_bladed ...
  "modifiers": [ /* always-on, see Modifiers */ ],
  "weapon": {
    "category": "whip", "attack_speed": 4, "attack_range": 1,   // range 1 = adjacent melee
    "stances": [ { "name": "Flick", "kind": "accurate", "damage_type": "slash",
                   "bonuses": {"attack": 3}, "speed_delta": 0, "range_delta": 0 } ],
    "special": { "name": "Energy Drain", "energy_cost": 50, "hit_count": 1, "attack_speed": null,
                 "always_hits": false, "damage_type": null, "max_hit": null,
                 "modifiers": [ {"target": "attack_roll", "ratio": {"num": 5, "den": 4}} ] },
    "powered": { "powered_staff": {"divisor": 3, "offset": -2, "gear_bonus_scale": 1} },  // powered staves only
    "can_cast": false,                // staves and wands that cast from the spellbook
    "ammo_family": "bolt",            // bows and crossbows; the ammo slot must match
    "thrown": false,                  // darts/knives: the weapon supplies the ranged strength
    "attack_sound": 2720 }            // cache sound effect (archive 4) heard as it swings; a stance may have its own "sound"
  "ammo": { "family": "bolt", "weight": "standard" } }  // ammo items: light | standard | heavy
```

Summed `bonuses` of everything worn (plus complete sets) are the player's equipment bonuses.

### Sets (`sets.json`)

`{ "id", "pieces": [["alt_a","alt_b"], ["only_this"]], "bonuses": {...}, "modifiers": [...] }` —
active only while every piece (any alternative of it) is worn. Void, Dharok's, Graceful.

### Modifiers

The one primitive behind every conditional effect (Void, Slayer helm, Salve, Dragon hunter, Keris,
Dharok's, special attacks, prayers with extra effects):

```jsonc
{ "target": "max_hit",                    // see below
  "ratio": {"num": 7, "den": 6},          // multiplier, floored after each application (default 1/1)
  "add": 0,                               // for the additive targets
  "condition": {"all": [{"flag": "on_task"}, {"style": "melee"}]},   // default: always
  "group": "slayer_or_salve", "priority": 1,   // same group = only the highest priority applies
  "chance_one_in": 51,                    // damage_roll only: proc on a 1-in-N draw
  "scaling": "missing_hitpoints" }        // or twisted_bow_accuracy / twisted_bow_damage: ratio computed at attack time
```

Targets, in the order the resolver applies them: `accuracy_level`, `damage_level` (multiply the
effective level — Void), `attack_roll`, `target_defence_roll` (use ratio < 1 for defence-reducing
specials), `magic_damage_permille` (additive, joins the Magic Damage % sum), `max_hit`,
`max_hit_add`, `damage_roll` (per rolled hit, after the roll).

Conditions: `"always"`, `{"target_has": "undead"}`, `{"style": "melee"}`, `{"damage_type": "stab"}`,
`{"flag": "..."}`, `{"not": c}`, `{"all": [..]}`, `{"any": [..]}`.
Attributes: `undead demon draconic kalphite leafy golem shade spectral vampyre_tier1..3 fiery flying xerician`.

Flags the player's attack carries: `on_task`, `special`, `weapon_category:<c>`, `weapon:<item id>`,
`weapon_tag:<tag>`, `stance:<snake_name>`, `ammo:<family>`, `ammo_item:<id>`, `ammo_weight:<w>`,
`spell:<id>`, `spellbook:<book>`.

### Prayers, spells, consumables

* **Prayer**: `level`, `effects` (percent multipliers `attack_percent` `strength_percent`
  `defence_percent` `ranged_attack_percent` `ranged_strength_percent` `magic_percent`, and
  `magic_damage_permille`), `drain_effect` (the wiki number), `groups` (mutually exclusive set),
  `protects` (`melee`/`ranged`/`magic`), `tags` (`rapid_heal`, `preserve`, `redemption`, ...),
  `sound` (heard as it goes on).
* **Spell**: `base_max_hit`, `level`, `spellbook`, `range`, `cast_speed`, `on_hit` (poison, freeze, ...),
  `level_scaling` (Magic Dart), `requires_target_attribute` (Crumble Undead), `modifiers`,
  `cast_sound` and `hit_sound` (cache sound effects heard as it is cast and as it lands).
* **Consumable**: `delay_group` + `delay_ticks` (foods, potions and combo food are separate groups),
  `attack_delay_ticks`, and `effects`: `heal`, `boost`, `drain`, `restore` (`flat + percent of base`),
  `restore_run_energy`, `restore_special_energy`, `cure_poison`, `cure_venom`, `poison_immunity`,
  `stamina_boost`; an optional `sound` (else the game's eat or drink sound by `delay_group`).
* **Potion doses** are separate items, as in the game, each with its own name (and so its own
  picture in the client): `prayer_potion_4` "Prayer potion(4)", `prayer_potion_3`, ... Each entry
  lists its effects and what it `leaves` once drunk: the next dose down, and for the last dose the
  `vial` item. Food leaves nothing.

### Monsters (`npcs.json`)

```jsonc
{ "id": "tztok_jad", "name": "TzTok-Jad", "combat_level": 702, "hitpoints": 250, "size": 5,
  "levels": {"attack": 640, "strength": 960, "defence": 480, "ranged": 960, "magic": 480},
  "bonuses": { "defence": {"slash": 20} },          // its own attack (rolled with) and defence (defends with) tables
  "attributes": ["demon"],
  "attacks": [ { "name": "Magic", "style": "magic", "damage_type": null, "max_hit": 97, "speed": 8,
                 "range": 15, "telegraph_ticks": 1, "impact_delay_ticks": 0, "projectile_travel_ticks": 2,
                 "prayer_penetration": 0.0, "protectable": true, "weight": 1.0, "hit_count": 1,
                 "modifiers": [], "on_hit": {"poison": {"severity": 20}},
                 "animation": 2656,                    // this attack's own cache sequence (optional)
                 "projectile": 448, "target_graphic": null, "impact_graphic": 157,   // graphics (optional)
                 "sound": 159,                         // cache sound effect heard as it starts (optional)
                 "resolve_sound": null } ],            // ...and one heard on the roll tick (Jad's ranged stomp)
  "rotation": "weighted",                            // or "fixed"
  "movement": "chase",                               // stationary | {"maintain_distance": {"distance": n}} | {"style_reactive": {"kite_distance": n}}
  "aggression": { "aggressive": true, "range": 4, "ignores_tolerance": false, "ignores_combat_level": false },
  "attackable_if": { "any": [ {"not": {"style": "melee"}}, {"flag": "weapon_category:halberd"} ] },
  "appearance": 3127,                                // cache NPC definition to draw it from
  "animations": { "attack": 2655, "defend": 2653, "death": 2654 },
  "sounds": { "attack": null, "hit": 410, "death": null },   // cache sound effects (archive 4); an attack may name its own
  "summons": [ { "npc": "yt_hurkot", "count": 4, "below_hitpoints": 150, "again_once_healed_to_full": true } ],
  "heals_summoner": null,                            // on a helper: {"amount", "every_ticks", "range", "forget_player_beyond", "animation"}
  "tags": [] }
```

An attack can only be chosen while the player is within *its* reach (adjacent for melee, `range`
otherwise); a monster with nothing in reach does not begin an attack. `speed` is OSRS attack speed
(consecutive resolves are exactly that many ticks apart, subject to `telegraph_ticks` fitting).
`prayer_penetration` 0 = the matching protection prayer blocks it, 1 = ignores prayer
("typeless"), between = pierces. The three timing archetypes: traditional (`telegraph_ticks` 0),
Jad-style (`telegraph_ticks` > 0), Doom-style (`impact_delay_ticks` > 0).

An attack with `telegraph_ticks` 0 resolves (rolls and, with no travel, lands) on the tick it is
chosen. The graphics are for clients only: `projectile` flies from the monster to the player and
arrives as the hit lands, `target_graphic` plays on the player as the attack is released (the
rocks of Jad's stomp), `impact_graphic` as the hit lands.

`summons` calls helpers once the monster's hitpoints fall to `below_hitpoints`; they appear around
it. With `again_once_healed_to_full`, the call is answered again (replacing helpers that died) the
next time it falls that low after being healed back to full. A helper with `heals_summoner` walks
up to the monster that called it and heals it `amount` every `every_ticks` ticks while within
`range` tiles. Attacking a helper distracts it: it fights the player and stops healing, until
the player is more than `forget_player_beyond` tiles away.

NPCs never walk through one another (each blocks every other NPC over its whole footprint, like
a wall, until it dies). The player walks through them unless the monster sets `"blocks_movement":
true`.

A monster holds still while any of its attacks can reach the player, and only walks (by its
`movement`) when none can; the kiting policies (`maintain_distance`, `style_reactive`) are the
exception and keep their distance. `animation` names the cache sequence an attack plays when it
differs from `animations.attack`; an attack with a telegraph starts it as the telegraph begins, so
the player sees the warning (Jad's stomp and rear-up) before the roll reads their prayer.

Monsters spawned from data start **passive**; they fight once aggression triggers, the player
attacks them, or a scenario marks them `"hostile": true`.

## Scenarios (`data/scenarios/*.json`)

```jsonc
{ "name": "...", "seed": 12345, "half_extent": 15,
  "player": { "start": [0,0],
              "levels": {"attack": 80, "strength": 80, "defence": 70, "ranged": 1, "magic": 1,
                         "prayer": 52, "hitpoints": 80, "agility": 60},
              "equipment": ["abyssal_whip"], "prayers": ["piety"], "consumables": ["super_strength_4"],
              "stance": 1, "spell": null, "special": false, "on_slayer_task": false,
              "running": true, "attack": 0,          // attack = index into npcs
              "inventory": [ {"item": "shark", "quantity": 6}, {"item": "prayer_potion_4"} ] },
  "npcs": [ {"npc": "zombie", "at": [1,0], "hostile": false, "hitpoints": null} ],  // hitpoints: start wounded
  "location": [2400, 5080] }                         // world tile of (0,0) on the real map (optional)
```

`sim::scenarios::build_scenario` applies it through the same commands a client uses, so anything the
rules refuse (a prayer above the player's level, an unknown item, an inventory that overflows) is
an error. The same seed replays the same fight tick for tick.

Headless runs (`simulate`) put a scenario on a flat floor at the origin. The game client plays one
on the real map with `cargo run -p osrs_core --release -- --scenario <name | file.json>`
(`build_scenario_on`): every `start` and `at` is relative to `location`, the collision is the
cache's, and the fight is held paused until it is on screen. `jad` is the bundled Fight Cave fight; `jad_healers` starts it with Jad on 140 hitpoints, so his
healers come at once.

## Adding things

* **A new item/monster/spell/prayer** — add JSON; `Registry::validate` checks references.
* **A new conditional gear effect** — usually just a `Modifier`; add a flag or condition only if no
  existing one can say it.
* **A new formula** — add a pure function in `rules/` with tests quoting the wiki, then call it from
  `combat::damage_resolver` or `sim::player_runtime`. Update `OSRS_COMBAT_REFERENCE.md` in the same change.
* **A mechanic that needs per-tick state** — it belongs in `Player::run_regen` (player resources),
  `StatusEffectTracker` (timed effects) or the NPC combat state machine.

## Deliberate deviations from the C# parity plan

* The player's attack timer now makes a swing every `speed` ticks (a 4-tick weapon on ticks 1, 5, 9, ...).
  The C# port waited `speed + 1`.
* Magic attacks against a **player** roll against 70% Magic + 30% Defence (the C# used Defence only);
  against a **monster**, its Magic level rather than its Defence.
* **Data-defined** monsters attack with `level + 9` effective levels (the C# used `level + 8`); the
  older `NpcConfig`/`BossConfig` monsters keep `level + 8`.
* Monster attacks have an attack-speed cooldown (`AttackDefinition::cooldown_ticks`); the original
  state machine attacked back to back.
