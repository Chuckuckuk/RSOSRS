# 10. Testing, running and common changes

---

## 10.1 Commands

| Task | Command |
|---|---|
| Run the game client | `cargo run -p osrs_core --release` |
| Start the client in a fight (Jad) | `cargo run -p osrs_core --release -- --scenario jad` |
| ...from the healer phase | `cargo run -p osrs_core --release -- --scenario jad_healers` |
| Package the Jad fight as one .exe (needs the game cache) | `./package_jad.sh` (writes `dist/RSOSRS-Jad.exe`) |
| Run the map viewer | `cargo run -p osrs_viewer --release -- 49 49 3 3` |
| Run a headless fight | `cargo run -p osrs_core --bin simulate -- whip_vs_zombie --ticks 60` |
| List bundled fights | `cargo run -p osrs_core --bin simulate -- --list` |
| Run a cache probe | `cargo run -p osrs_cache --release --bin tile_probe -- 3222 3218` |
| Write a game sound effect to a WAV to listen to | `cargo run -p osrs_cache --release --bin sound_probe -- 4 163 out.wav` |
| All tests | `cargo test` |
| One crate's tests | `cargo test -p osrs_core` |
| Only the library's unit tests | `cargo test -p osrs_core --lib` |
| Tests whose name contains a word | `cargo test -p osrs_core prayer` |
| Show `println!` output from tests | `cargo test -- --nocapture` |
| Build API docs from the doc comments | `cargo doc --no-deps --open` |

`--release` matters for anything that draws: debug builds of the cache decoders and Bevy are much
slower. The first build compiles Bevy and takes several minutes; later builds are incremental.

Everything that reads the game cache needs RuneLite's cache at
`%USERPROFILE%\.runelite\jagexcache\oldschool\LIVE`.

---

## 10.2 How the tests are organised

Rust has two kinds of tests, and this repository uses both.

**Unit tests** live at the bottom of the file they test, inside `#[cfg(test)] mod tests { ... }`.
They can reach private items. Most files in `osrs_core/src/` have them, and they double as
executable documentation: a test name like
`a_correct_protect_from_melee_blocks_a_melee_npcs_hits` states a rule, and its body shows the rule
in action. When a chapter of this guide is unclear, the tests in the matching file are the next
place to look.

**Integration tests** live in a crate's `tests/` folder. Each file is compiled as a separate
program that sees only the crate's public API, like an external user.

| File | Checks |
|---|---|
| [`osrs_core/tests/simulation.rs`](../osrs_core/tests/simulation.rs) | the data-driven rules end to end: attack cadence, stances, ammo, spells, powered staves, Salve vs Slayer helm, attribute-keyed gear, halberd-only and leaf-bladed-only monsters, specials, regen, prayer drain, potions, consumable delays, run energy, poison, the Phoenix necklace, aggression and tolerance, the twisted bow, and that every bundled scenario replays identically |
| [`osrs_core/tests/lumbridge_collision.rs`](../osrs_core/tests/lumbridge_collision.rs) | the server walking on real Lumbridge collision never stands on a blocked tile or crosses a wall |
| [`osrs_core/tests/walk_to_draynor.rs`](../osrs_core/tests/walk_to_draynor.rs) | walking from Lumbridge to Draynor with squares streamed in as the player goes, and that unloaded ground is unwalkable |
| [`osrs_cache/tests/cache.rs`](../osrs_cache/tests/cache.rs) | format sweeps: every model, object definition, identity kit, sequence and frame base decodes cleanly; Lumbridge terrain and collision; a square decoded alone matches a larger block; the default character builds and animates |
| [`osrs_render/tests/pick.rs`](../osrs_render/tests/pick.rs) | picking against real Lumbridge geometry |

Tests that need the game cache start with a helper like
`let Some(cache) = cache() else { return; };`, so they **pass silently** when no cache is
installed. A green run on a machine without the cache therefore says nothing about decoding.

### Test helpers worth knowing

* `ScriptedRng` ([`rng.rs`](../osrs_core/src/rng.rs)) forces exact rolls.
* Sound tests: `osrs_cache::sound` has a sweep that decodes every synth sound in the cache, and
  `sim/scenarios.rs` checks that every sound id the bundled data names exists there
  (`every_sound_the_data_names_is_a_synth_sound_in_the_cache`) and that the right sounds come out
  of a fight (`sounds_of` collects the `WorldEvent::Sound`s of a tick).
* `collision::test_grid::parse` ([`test_grid.rs`](../osrs_core/src/collision/test_grid.rs)) builds a
  collision map from ASCII art.
* `dev_open_floor_world` ([`sim/scenarios.rs`](../osrs_core/src/sim/scenarios.rs)) gives an empty
  flat arena.
* `combat::test_support::FakeCombatTarget` is a bare defender.
* `Server::advance(0.6)` runs exactly one tick in a test; `Server::advance(0.0)` on a fresh server
  runs the first tick (the driver starts primed).

### Determinism in tests

Because a fixed seed gives a fixed fight (chapter 2.5), tests can assert exact outcomes. If you
change the order in which random numbers are drawn, tests that pinned specific damage values will
fail even though nothing is "wrong". That is intentional: it makes accidental changes to roll
order visible.

---

## 10.3 Debugging the client with environment variables

Set these in PowerShell with `$env:NAME = "value"` before `cargo run`.

| Variable | Effect |
|---|---|
| `OSRS_SPAWN=x,z` | enter the world on another tile (e.g. `3150,3235`, near Draynor) |
| `OSRS_NAME=name` | login name |
| `OSRS_WALK_TO=x,z` | ask the server to walk there at startup |
| `OSRS_WALK_RUN=0` | with `OSRS_WALK_TO`, walk rather than run |
| `OSRS_ATTACK=id` | attack NPC entity `id` at startup (the man is usually 1) |
| `OSRS_SEED=n` | seed of the open world's random rolls; by default it comes from the clock (and is printed), so each launch rolls differently |
| `OSRS_DO="t:cmd:args;..."` | timed server commands (`t` = seconds since start), to chain actions without clicking: `interact:id,x,z,option` (an option on scenery object `id` over tile (x, z)) and `walk:x,z`. E.g. `OSRS_SPAWN=3231,3207` with `OSRS_DO="2:interact:1535,3234,3207,Open;6:walk:3237,3207;11:interact:1536,3233,3207,Close"` opens the house door, walks out and closes it behind you |
| `OSRS_WIELD=0,1` | wield the items in these inventory slots at startup |
| `OSRS_TAB=n` | open interface tab `n` (0-13) first |
| `OSRS_LAYOUT=classic`, `modern` or `fixed` | interface layout: resizable classic (default), resizable modern, or the fixed 765x503 one (the 3D view then fills only its viewport widget) |
| `OSRS_WINDOW=WxH` | window size in physical pixels (scale factor forced to 1): `900x600` shows the modern layout's narrow stone arrangement (two rows, under 948 px wide), `765x503` the fixed layout at 1:1 |
| F10 (key, not a variable) | cycles the layout at run time: fixed, classic, modern (stands in for the Settings pane) |
| `OSRS_UI_SCALE=n` | whole-number interface scale in screen pixels (default: the largest that fits 765x503 in the window) |
| `OSRS_MAX_PLANE=n` | hide terrain and scenery above plane `n` |
| `OSRS_ORBIT=yaw,pitch,dist` | starting camera |
| `OSRS_SCREENSHOT=path.png` | save a screenshot and quit |
| `OSRS_SCREENSHOT_DELAY=s` | when to take it (default 8 s; early frames are blank while shaders compile) |
| `OSRS_NO_OBJECTS=1` | skip scenery (viewer and square loader) |
| `OSRS_NO_TEXTURES=1` | flat colours instead of textures (viewer) |
| `OSRS_INPUT=...` | scripted mouse input; a temporary hook marked for removal, not for routine use |

Console prefixes tell you who printed a line: `[server]` from the server thread, `[client]` from
the client, `[game]` for chat messages the server sent.

---

## 10.4 Recipes

### Add an item, monster, prayer, spell or potion

No Rust changes. Add a JSON entry to a file in [`osrs_core/data/`](../osrs_core/data/) following
[`SIMULATOR.md`](../osrs_core/SIMULATOR.md). The id must be unique across every file. Run
`cargo test -p osrs_core`: the test `the_bundled_dataset_loads_and_validates` loads and validates
the whole bundle, and an unknown key or a dangling reference fails with a message naming it.

To make a monster visible in the client, set `appearance` to its cache NPC id (find it with
`cargo run -p osrs_cache --release --bin npc_probe -- "Goblin"`) and `animations` to its attack,
defend and death sequence ids. An attack with its own animation (Jad's stomp) names it in the
attack's `animation`. To fight it in the client, write a scenario with a `location` on the real
map and start the client with `--scenario` (chapter 7.1); `data/scenarios/jad.json` is the example.
To have it stand about in the open world instead, add a `spawn_npc_def` call in the
server-building closure in [`osrs_core/src/main.rs`](../osrs_core/src/main.rs).

### Doors and gates

[`data/doors.json`](../osrs_core/data/doors.json) is generated from the whole map; don't edit it
by hand. Regenerate it with

    cargo run -p osrs_cache --release --bin door_table > osrs_core/data/doors.json

It finds every straight-wall placement whose only option is "Open", pairs it with the "Close"
object of the same name (same models nearby, else the very next ids), and detects double doors
and gates from two leaves side by side. On stderr it reports how many placements it covers and
what it leaves out, and why (walk-through doors #25, doors with more than Open #26, curtains #27).
`tests/doors.rs` checks every entry against the cache. `obj_actions <id>...` or
`obj_actions --named "Gate"` shows an object's options when a pairing looks wrong. Check a door in
the client with `OSRS_SPAWN` and `OSRS_DO`; near buildings the camera often sits in a roof (#28),
so a chained `Close` (answered "Nothing interesting happens." only if the open did not happen)
is the more reliable check.

### Add a gathering resource (another tree, a rock)

Usually no Rust changes. Add its tool rows to [`data/tools.json`](../osrs_core/data/tools.json)
(if new) and a node to [`data/gathering.json`](../osrs_core/data/gathering.json): its scenery ids
and their depleted forms (check them with `find_locs` / `obj_info`), requirements, the per-tool
`[low, high]` chances from the wiki's chart, XP in tenths, product, depletion rule and respawn
range. `the_bundled_dataset_loads_and_validates` checks the references. Only a resource that runs
out in a new way (a fishing spot that moves) needs a new `DepletionDef` variant.

### Add a conditional gear effect

Usually just a `Modifier` on the item (chapter 3.4). Pick the `target` for the roll step it
changes, a `condition`, and a `group` if it must not stack with something else. Add a new
`Condition` variant or flag only if no existing one can express it; new flags are added in
`Player::plan_attack` ([`player_runtime.rs`](../osrs_core/src/sim/player_runtime.rs)).

### Add a new formula

1. Write a pure function in [`rules/`](../osrs_core/src/rules/), using integer maths and
   `div_euclid` where the game floors.
2. Test it with values quoted from the OSRS Wiki.
3. Call it from `combat::damage_resolver` or `sim::player_runtime`.
4. Update [`unity_context/OSRS_COMBAT_REFERENCE.md`](../unity_context/OSRS_COMBAT_REFERENCE.md) in
   the same change.

### Add per-tick state

Player resources belong in `Player::run_regen`; timed effects in `StatusEffectTracker`; NPC combat
timing in `NpcCombatState`. Keep RNG draws in a deliberate, documented order.

### Add a client command

1. Add a variant to `ClientCommand` in [`sim/server.rs`](../osrs_core/src/sim/server.rs).
2. Handle it in `Server::handle_command` (the compiler will insist; primer 1.5), usually by calling
   a `World::player_*` method that returns `Result<(), String>` so rejections reach the chat.
3. If the client needs new information back, add a field to `PlayerSnapshot` or `EntitySnapshot`
   and fill it in `Server::snapshot`.
4. Send it from the client, typically from `perform` in
   [`ground_click.rs`](../osrs_core/src/ground_click.rs).

### Decode a new cache structure

1. Write a probe binary in [`osrs_cache/src/bin/`](../osrs_cache/src/bin/) that dumps the raw
   bytes of a few records.
2. Write the decoder as an opcode loop (chapter 8.2) that stops at unknown opcodes.
3. Write a sweep that decodes **every** record and checks exact byte consumption; iterate until
   it reports zero failures. Add it to [`osrs_cache/tests/cache.rs`](../osrs_cache/tests/cache.rs).
4. Note the format findings in [`osrs_cache/README.md`](../osrs_cache/README.md).
