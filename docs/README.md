# RSOSRS code guide

This folder explains the Rust code in this repository from the ground up. It is written for a
reader who knows Python well and has general software-engineering experience, but who has not
spent much time in Rust or in game engines.

The guide describes the code as it stands on 2026-10-12. Where it quotes a number or a rule, the
linked source file is the authority; if they ever disagree, trust the code and fix the guide.

## What this project is

RSOSRS is a re-creation of Old School RuneScape (OSRS) in Rust. It has two halves that are kept
deliberately separate:

* **A headless game server.** It is a tick-based simulation of OSRS combat, movement and
  inventory rules, driven by JSON data. It knows nothing about graphics. It lives in the
  `osrs_core` library.
* **A 3D game client.** It reads the real OSRS game files ("the cache") that RuneLite has already
  downloaded to your machine, decodes the maps, models and animations, and draws them with the
  Bevy game engine. It talks to the server only through commands and snapshots, the same way a
  networked client would.

The server logic began life as a C# Unity project (its specs and notes are in
[`../unity_context/`](../unity_context/)). `osrs_core` started as a line-by-line port of that C#,
then grew a data-driven layer and a server boundary of its own.

## Reading order

Read these in order the first time. Each one assumes the ones before it.

| # | File | What it covers |
|---|---|---|
| 1 | [01-rust-for-python-programmers.md](01-rust-for-python-programmers.md) | Every Rust and Bevy idiom this codebase uses, explained with Python comparisons. Skim it, then come back when a later chapter uses something unfamiliar. |
| 2 | [02-architecture.md](02-architecture.md) | The four crates, what depends on what, which threads run at runtime, how data flows, and the coordinate systems. |
| 3 | [03-core-foundations.md](03-core-foundations.md) | `osrs_core`'s bottom layers: tiles, the seeded RNG, the pure formula module (`rules`) and the shared vocabulary (`model`), including the `Modifier` primitive. |
| 4 | [04-core-combat.md](04-core-combat.md) | How an attack is defined, chosen, timed, rolled, delayed and landed. |
| 5 | [05-core-movement.md](05-core-movement.md) | Collision maps, the BFS pathfinder, movers and NPC movement policies. |
| 6 | [06-core-simulation.md](06-core-simulation.md) | The tick scheduler, the JSON data layer, the `World`, the player, the `Server` boundary and the scenario runner. |
| 7 | [07-game-client.md](07-game-client.md) | The Bevy client binary: how it logs in, draws the player and NPCs, handles clicks, shows the interface and plays combat effects. |
| 8 | [08-cache-decoding.md](08-cache-decoding.md) | `osrs_cache`: the OSRS cache file format and every decoder built on it (terrain, models, animations, items...). |
| 9 | [09-rendering-and-viewer.md](09-rendering-and-viewer.md) | `osrs_render` (turning decoded meshes into Bevy entities, picking, map streaming) and the `osrs_viewer` debug tool. |
| 10 | [10-testing-and-workflows.md](10-testing-and-workflows.md) | How the tests are organised, how to run things, the debugging environment variables, and step-by-step recipes for common changes. |
| - | [glossary.md](glossary.md) | OSRS, Rust and project terms in one place. |

## Repository map

```
RSOSRS/
├── Cargo.toml            workspace: lists the four crates, sets build profiles
├── osrs_core/            the game rules and server, plus the game client binary
│   ├── src/lib.rs        library root: the simulation (no graphics)
│   ├── src/main.rs       binary root: the Bevy game client
│   ├── src/bin/simulate.rs   headless fight runner
│   ├── data/             JSON definitions: items, monsters, prayers, spells, scenarios
│   ├── tests/            integration tests
│   ├── SIMULATOR.md      data-format reference
│   └── PARITY_PLAN.md    record of the C# port
├── osrs_cache/           reads and decodes the OSRS game cache (no graphics engine)
│   ├── src/bin/*.rs      ~30 small probe tools used while reverse-engineering formats
│   └── README.md         cache-format findings log
├── osrs_render/          bridges decoded meshes into Bevy; shared by client and viewer
├── osrs_viewer/          a free-flying map viewer for debugging rendering
├── unity_context/        specs and notes from the original C# project
└── shots/                screenshots taken while developing
```

## The big picture in one diagram

```
                ┌──────────────────────── osrs_core (bin: game client) ─────────────────────────┐
                │  Bevy app, main thread                                                         │
 mouse/keys ──► │  ground_click / interface ──► ClientCommand ─┐                                 │
                │                                              │  mpsc channel                   │
                │  sync_player / sync_npcs / combat_fx ◄── Snapshot ◄─┐                          │
                └──────────────────────────────────────────────┼─────┼──────────────────────────┘
                                                               ▼     │
                ┌──────────────── osrs_core (lib) on its own server thread ──────────────────────┐
                │  Server ──► TickDriver (600 ms ticks) ──► World::run_tick()                    │
                │              phases: PlayerQueue, NpcAi, NpcMovement, PlayerMovement, Regen    │
                │  World owns: Player, NPC entities, CollisionMap, BfsPathfinder, RNG, Registry  │
                └────────────────────────────────────────────────────────────────────────────────┘
                         ▲ collision squares (merged as the player walks)
                ┌────────┴──────── osrs_cache ────────┐      ┌──────── osrs_render ────────┐
                │ .dat2/.idx files ─► groups ─► defs, │ ───► │ Bevy meshes, materials,     │
                │ terrain, models, anims, sprites     │      │ picking, square streaming   │
                └─────────────────────────────────────┘      └─────────────────────────────┘
```

Read [02-architecture.md](02-architecture.md) for the full version of this picture.
