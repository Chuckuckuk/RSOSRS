# 2. Architecture

This chapter is the map. It explains how the five crates divide the work, what runs on which
thread, how a click becomes movement on screen, and the several coordinate systems you have to
keep straight.

---

## 2.1 The five crates

```
   osrs_map_editor (bin)   osrs_viewer (bin)          osrs_core (bin: client)
        │      │                  │                       │          │
        │      └────────────────► ▼                       ▼          │
        │                     osrs_render  ◄──────────────┘          │
        │                         │                                  ▼
        │                         ▼                           osrs_core (lib: simulation)
        │                     osrs_cache  ◄───────────────────────── (tests only use osrs_cache;
        └──────────────────────► (and osrs_core lib, for npcs/gathering data)   the lib itself does not)
```

| Crate | Kind | Depends on | Responsibility |
|---|---|---|---|
| [`osrs_cache`](../osrs_cache/) | library + ~30 probe binaries | `bzip2`, `flate2`, `serde` | Read the OSRS cache files from disk and decode them into plain Rust data: terrain grids, object placements, models, animations, item/NPC definitions, sprites, fonts. Also builds lit triangle meshes and collision flags, and writes edited copies of a cache for the map editor. **No graphics engine.** |
| [`osrs_render`](../osrs_render/) | library | `bevy`, `osrs_cache` | Turn `osrs_cache` meshes into Bevy entities and materials, animate them, stream map squares in on a background thread, and convert mouse rays into tiles and objects. |
| [`osrs_core`](../osrs_core/) | library | `serde`, `serde_json` (and Bevy for the binary) | **Library:** the game simulation and server. Pure logic, deterministic, testable without a window. |
| | binary `osrs_core` | the library, `osrs_render`, `osrs_cache`, `bevy` | **The game client.** Starts the server on a thread, logs in, and draws what the server reports. |
| | binary `simulate` | the library | Runs a JSON-described fight headlessly and prints the result. |
| [`osrs_viewer`](../osrs_viewer/) | binary | `bevy`, `osrs_render`, `osrs_cache` | A free-flying camera over a block of map squares, for debugging rendering. |
| [`osrs_map_editor`](../osrs_map_editor/) | binary | `bevy`, `bevy_egui`, `osrs_render`, `osrs_cache`, `osrs_core` | Places and removes scenery, resource nodes, NPC spawns and minimap icons, and compiles the edits into a new cache the game can be switched to (chapter 11). |

A detail that surprises people: `osrs_core/Cargo.toml` lists Bevy and `osrs_cache` as
dependencies, but nothing under `osrs_core/src/` *other than the client modules* imports them. The
simulation library (`lib.rs` and everything it declares) uses only `serde`. The client modules
(`main.rs`, `ground_click.rs`, `npcs.rs`, ...) are declared by `main.rs`, not by `lib.rs`, so they
belong to the binary only. When this guide says "osrs_core", it means the library unless it says
"the client".

### Inside `osrs_core` (the library)

The library is layered. Each layer only uses the layers above it in this table.

| Layer | Folder | What it knows |
|---|---|---|
| Geometry | [`tiles/`](../osrs_core/src/tiles/) | Tile coordinates and the eight directions. |
| Randomness | [`rng.rs`](../osrs_core/src/rng.rs) | A bit-exact port of .NET's seeded random generator, and a scripted fake for tests. |
| Rules | [`rules/`](../osrs_core/src/rules/) | Pure OSRS formulas: effective levels, rolls, hit chance, max hit, regeneration, drain, poison. No entities, no randomness. |
| Vocabulary | [`model/`](../osrs_core/src/model/) | Damage types, bonus tables, monster attributes, prayer effects and the `Modifier` primitive. |
| Collision | [`collision/`](../osrs_core/src/collision/) | Which tile edges are walls, what is solid, who is standing where. |
| Pathfinding | [`pathfinding/`](../osrs_core/src/pathfinding/) | BFS shortest paths and greedy single steps. |
| Actors | [`actors/`](../osrs_core/src/actors/) | Something that occupies tiles and has health; walking it along a path; NPC movement policies. |
| Combat | [`combat/`](../osrs_core/src/combat/) | Attack definitions, attack selection, the roll pipeline, delayed hits, status effects. |
| Encounters | [`encounters/`](../osrs_core/src/encounters/) | Boss phases and triggers ("at 50% HP swap moveset"). |
| Interaction | [`interaction/`](../osrs_core/src/interaction/) | Right-click menu options and the "entry swapper". Ported from C#; not wired into the client. |
| Ticking | [`tick/`](../osrs_core/src/tick/) | The order of phases within a 600 ms game tick, and turning wall-clock time into ticks. |
| Data | [`data/`](../osrs_core/src/data/) + [`data/*.json`](../osrs_core/data/) | JSON definitions of items, sets, prayers, spells, monsters, consumables, scenarios; the `Registry` that loads and validates them. |
| Simulation | [`sim/`](../osrs_core/src/sim/) | The `World` that owns and ticks everything, the `Player`, NPC entities, and the `Server` boundary. |

[`core_log.rs`](../osrs_core/src/core_log.rs) defines a two-method logging trait ported from C#.
Nothing implements it yet.

---

## 2.2 What runs where at runtime (the client)

When you run `cargo run -p osrs_core --release`, these threads exist:

| Thread | Created by | Owns | Talks via |
|---|---|---|---|
| **Main (Bevy)** | `main()` | The Bevy `App`: every entity, mesh, material, the UI | Polls the channels below once per frame. |
| **Server** | [`spawn_server`](../osrs_core/src/sim/server.rs) | The `Server`, which owns the `World` and the `TickDriver`. Opens its own `MapSource` to read collision from the cache. | Receives `ClientCommand`s, sends `Snapshot`s. |
| **Square loader** | [`SquareStream::spawn`](../osrs_render/src/streaming.rs) | A `SquareLoader` (open cache, model cache) | Receives square coordinates, sends decoded `SquareData`. |
| **NPC looks** | [`NpcViews::spawn`](../osrs_core/src/npcs.rs) | An open cache and every NPC definition | Receives a cache NPC id, sends a lit mesh plus baked animations. |
| **Player looks** | [`PlayerLooks::spawn`](../osrs_core/src/player_look.rs) | An open cache, item definitions | Receives "the worn items changed", sends a rebuilt character. |
| Bevy's own worker pool | Bevy | | Runs systems in parallel where allowed. |

Each worker thread exists so that slow cache decoding never stalls a frame. Each owns its own
`Cache` handle, partly because decoded models are held in `Rc` (not thread-safe; see primer
section 1.8).

Before the window even opens, `main()` calls `load_scene()`, which decodes the 3x3 block of map
squares around the spawn point, the default character, its ten animation clips, the hit-splat
sprites, the font and the health-bar sprites. The first frame therefore has a world to show.

---

## 2.3 The server boundary

The client never touches the `World` directly. The only things that cross from client to server
are values of [`ClientCommand`](../osrs_core/src/sim/server.rs) and the only things that come
back are [`Snapshot`](../osrs_core/src/sim/server.rs)s. Both are plain owned data (no `Rc`, no
references), so they could be serialised and sent over a network unchanged. Today they cross an
in-process channel.

```
client frame N                         server thread                         client frame N+k
──────────────                         ─────────────                         ────────────────
click ground
  └─ send WalkTo{tile} ──────────────►  handle_command: player.pending_click = tile
                                        ...600 ms of wall-clock passes...
                                        TickDriver says "run 1 tick"
                                        map streamer merges collision squares
                                        World::run_tick():
                                          PlayerQueue: pending_click → BFS path
                                          PlayerMovement: step 1 or 2 tiles
                                        snapshot(): player tile, step_tiles, events...
                                       ──────────────────────────────────────►  poll_server
                                                                                sync_player walks the
                                                                                model through step_tiles
```

Important consequences:

* **The server is authoritative.** The client cannot move the player, skip a tick or change HP. It
  can only ask.
* **The client draws one tick behind.** When a snapshot says "the player stepped through tiles A
  and B during tick 57", the client spends the next 600 ms walking the model from its current spot
  through A and B. The model therefore arrives exactly as the next tick's steps begin. See
  [chapter 7](07-game-client.md).
* **Rejected commands come back as messages.** If you try to wield an item you cannot, the server
  puts a line in `Snapshot::messages`, which the client prints in the chatbox.
* **Events, not diffs, describe combat.** A snapshot carries a list of `WorldEvent`s (`Attack`,
  `Projectile`, `Hit`, `Died`, `Sound`, ...) that happened since the previous snapshot, in order.
  The client turns them into animations, hit splats, health bars and sound. As in the real game,
  the server decides every sound; the client only plays them.

---

## 2.4 One game tick, end to end

OSRS runs on a 600 ms server tick. Everything that changes game state happens on a tick. Within a
tick, work happens in a fixed sequence of **phases** (see
[`tick/tick_scheduler.rs`](../osrs_core/src/tick/tick_scheduler.rs)):

| # | Phase | Who runs | What happens |
|---|---|---|---|
| 1 | `PlayerQueue` | the player | Attack timer ticks down. A queued walk click becomes a BFS path; or, if attacking, the player paths into range and swings (rolling damage into the target's damage buffer). |
| 2 | `NpcAi` | every non-passive NPC | Aggression check, boss triggers, the telegraph/resolve state machine; resolved attacks are rolled into the player's buffers. |
| 3 | `InteractionCheckPre` | nobody yet | reserved |
| 4 | `NpcMovement` | every NPC | Land hits whose travel time is up, then move (chase, kite, wander or hold still). |
| 5 | `PlayerMovement` | the player | Step 1 tile (walk) or 2 (run) along the path; roll delayed ("Doom-style") attacks that are due; land hits; pulse poison and other status effects. |
| 6 | `InteractionCheckPost` | nobody yet | reserved |
| 7 | `Regen` | encounter clock, then the player | Encounter tick counter and the tolerance timer advance; the player's HP regen, stat drift, prayer drain, run energy and special energy update. |
| - | (after the phases) | the world | `settle_deaths`: deaths noticed last tick are confirmed (emitting `Died`), and new ones are noticed. |

NPCs move before the player ([`NPCS_MOVE_BEFORE_PLAYERS`](../osrs_core/src/tick/tick_scheduler.rs)
is `true`), matching the real server's order. This is why a chasing NPC appears to trail one step
behind.

Within a phase, entities run in the order they were spawned. An entity that moves updates the
occupancy overlay immediately, so entity *k* sees entities *0..k-1* already moved and *k+1..*
not yet moved. This is deliberate and deterministic.

---

## 2.5 Determinism

Every random number in a fight comes from one seeded generator, the encounter's
[`DotNetRandom`](../osrs_core/src/rng.rs), drawn in a fixed order. Given the same seed, the same
commands and the same data, a fight replays tick for tick; the test
`every_bundled_scenario_builds_runs_and_replays_identically` checks this. The generator is a port
of .NET's `System.Random` so that the Rust and the original C# produce identical rolls for the
same seed.

The rule "same order of draws" is why code comments are fussy about exactly when `next_double`
and `next_range` are called (see [chapter 4](04-core-combat.md)). Idle NPC wandering uses a
*separate* generator per NPC, so adding a wandering NPC never changes a fight's rolls.

Iteration order is also kept deterministic: the data registry uses `BTreeMap` (sorted) rather than
`HashMap`, and entities tick in spawn order.

---

## 2.6 Coordinate systems

Four different spaces meet in this code. Most confusing bugs in a project like this are mix-ups
between them.

### Tile space (the simulation)

* A [`TileCoord`](../osrs_core/src/tiles/tile_coord.rs) is `(x, z, plane)` in whole tiles.
* `x` grows **east**, `z` grows **north**. (OSRS itself calls the second axis `y`; this code
  calls it `z` because `y` is "up" in 3D.)
* `plane` is the floor level, 0 to 3. The simulation only walks on plane 0 for now.
* Real world coordinates are used directly: Lumbridge square's centre is tile `(3222, 3218)`.
* Distance for movement and range is **Chebyshev** distance, `max(|dx|, |dz|)`, because a
  diagonal step costs one tick just like a straight one.
* A multi-tile NPC's position is its **south-west** tile; its footprint extends east and north.

### Map squares

The world is cut into 64x64-tile **map squares** (also called regions). Tile `(x, z)` is in square
`(x div 64, z div 64)` using floor division. Lumbridge is square `(50, 50)`. The cache stores
terrain and objects per square, and both the client and the server load squares on demand.

### Client units (the cache)

Inside cache data, one tile is **128 units** horizontally. Heights are also in these units, and
**negative is up** (a hill top has a more negative height than the valley). Model vertices are in
the same units with **y pointing down**.

Angles in the cache and in the old client are measured in **2048ths of a full turn**: 512 is a
quarter turn. Animation rotations are stored as bytes and multiplied by 8 to reach that scale.

### Render space (Bevy)

* One Bevy unit = one tile.
* Bevy's `+y` is up; its `-z` is "forward". The renderer places tile `(x, z)` at Bevy
  `(x, height, -z)`. North in the game is therefore `-z` in Bevy.
* Height conversion: `bevy_y = -client_height / 128`.
* Model vertex conversion (used everywhere a mesh is built): `[vx / 128, -vy / 128, -vz / 128]`.
* The centre of tile `(x, z)` is at Bevy `(x + 0.5, y, -(z + 0.5))`.
* A character's **heading** is a rotation about Bevy's y axis in radians. Heading 0 faces south,
  which is the way models are built (their front is `+z` in Bevy). The heading of a step from tile
  `a` to tile `b` is `atan2(b.x - a.x, -(b.z - a.z))`.

Because render space *is* world tile space (just with z flipped), streamed squares never need to
be re-positioned as the player walks: every square is placed at its true location.

### Screen space (the interface)

The interface is the game's own widget tree from the cache, drawn by a software renderer into one
picture at a whole-number scale (resizable classic layout by default, anchored to the window edges:
chatbox bottom left; minimap, tabs and panel on the right). See chapter 7.8 and
[`interface.rs`](../osrs_core/src/interface.rs).

---

## 2.7 Time scales

| Unit | Length | Used for |
|---|---|---|
| Server tick | 600 ms | All game rules: movement, attacks, regeneration |
| Client tick / cycle | 20 ms | Animation frame durations (`Sequence::delays`), hit splat and health bar durations from the cache |
| Frame | whatever the monitor does | Bevy systems: interpolation, turning, camera |

`TickDriver::default()` is 0.6 s per tick with a catch-up limit of 5 ticks.

---

## 2.8 Where the data comes from

| Data | Source | Read by |
|---|---|---|
| Game rules (stats, max hits, prayers...) | JSON in [`osrs_core/data/`](../osrs_core/data/), compiled into the binary | the server |
| Maps, models, animations, sprites, item and NPC appearances | your local OSRS cache at `%USERPROFILE%\.runelite\jagexcache\oldschool\LIVE`, read at runtime | the client, the viewer, and the server (collision only) |
| Examine texts for scenery | [`osrs_core/data/object_examines.json`](../osrs_core/data/object_examines.json) | the server |

Nothing from the cache is copied into the repository. The JSON monster definitions link to the
cache through the `appearance` field (a cache NPC id) so the client knows which model to draw.
