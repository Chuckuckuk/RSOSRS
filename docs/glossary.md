# Glossary

Terms from OSRS, from Rust and Bevy, and from this project, in alphabetical order within each
group. Chapter references point to where the term is explained in full.

## OSRS game terms

| Term | Meaning |
|---|---|
| **Aggression / tolerance** | Aggressive monsters attack players who come within range. After 10 minutes near them a player becomes "tolerated" and they stop. Monsters below level 63 ignore players more than twice their combat level. (6.4) |
| **Attack roll / defence roll** | `effective level × (equipment bonus + 64)`. The two rolls decide the hit chance. (3.3) |
| **Attack speed** | Ticks between consecutive attacks. A 4-tick weapon swings every 2.4 s. |
| **Attribute** | A monster tag (undead, demon, draconic...) that some gear exploits. (3.4) |
| **Combat level** | A single number summarising combat skills; colours NPC levels in menus. (3.3) |
| **Effective level** | A level after potion boost, prayer, stance bonus and the +8 constant. (3.3) |
| **Hit splat** | The red (damage) or blue (zero) number shown over a character that is hit. |
| **Max hit** | The largest damage one hit can roll; damage is uniform from 0 to max on a successful hit. |
| **Overlay / underlay** | The two layers of ground colour on a tile: a blended base (underlay) and a shaped top layer (overlay: paths, floors, water). (8.6) |
| **Plane** | Floor level 0-3. Bridges and upper storeys are on planes 1-3. |
| **Prayer flicking** | Switching a protection prayer on only for the tick an attack rolls. Works because the prayer is checked at roll time only. (4.7) |
| **Prayer penetration** | The fraction of damage that gets through the correct protection prayer. |
| **Run energy** | Spent while running, regained while walking or standing. (3.3) |
| **Special attack** | A weapon's alternative attack costing special energy. (6.5) |
| **Stance** | A weapon's combat style option (accurate, aggressive, defensive, rapid...). Adds invisible level bonuses and can change damage type, speed or range. |
| **Telegraph** | The visible wind-up before an attack resolves. |
| **Tick** | The 600 ms server step on which every game rule runs. (2.4) |

## Rust and Bevy terms

| Term | Meaning |
|---|---|
| **`&` / `&mut`** | A shared (read-only) or exclusive (read-write) borrow of a value. (1.7) |
| **`Arc` / `Rc`** | Reference-counted shared ownership, thread-safe or not. (1.8) |
| **Borrow checker** | The compiler pass that enforces the borrowing rules. (1.7) |
| **`Box<dyn Trait>`** | A heap-allocated value of any type implementing a trait. (1.9) |
| **Crate** | A Rust package: a library, binaries, or both. (1.1) |
| **`derive`** | Compiler-generated standard trait implementations. (1.4) |
| **ECS** | Entity Component System, Bevy's architecture. (1.13) |
| **Enum** | A tagged union whose variants may carry data. (1.5) |
| **Handle** | Bevy's cheap reference to an asset (mesh, image, material). (1.13) |
| **Non-send resource** | A Bevy resource that is not thread-safe; systems using it run on the main thread. (1.13) |
| **`Option` / `Result`** | "Maybe a value" and "a value or an error". (1.5, 1.6) |
| **Panic** | An unrecoverable error, used for programmer mistakes. (1.6) |
| **`Send`** | A type that may be moved to another thread. `Rc` is not. (1.8) |
| **serde** | Rust's (de)serialisation library. (1.14) |
| **System** | A Bevy function run by the scheduler, with parameters injected by type. (1.13) |
| **Trait** | An interface a type implements. (1.9) |

## Project terms

| Term | Meaning |
|---|---|
| **Appearance** | (server data) the cache NPC id a monster is drawn from. (kit.rs) the kit ids a player character is built from. |
| **Archetype (timing)** | Traditional, Jad-style or Doom-style attack timing. (4.8) |
| **AttackPlan** | The swing a player's current setup would make, built by `plan_attack`. (6.5) |
| **Behavior** | The kind-specific state of an NPC entity: `Npc`, `Boss`, `Healer` or `Passive`. Data-defined fighters are `Boss`. (6.4) |
| **Cache** | The OSRS client's asset files, read from RuneLite's folder. (8) |
| **Client units** | The cache's length unit: 128 per tile, heights negative-up. (2.6) |
| **Clip** | An animation sequence baked against one model's rig: posed vertices per frame. (8.7) |
| **Combatant** | `Player` or `Npc(EntityId)`, used in events. |
| **Container** | A compressed group as stored in the cache. (8.1) |
| **DamageBuffer** | A defender's queue of rolled hits waiting out their travel time. (4.8) |
| **Draw one tick behind** | The client animates each tick's server steps during the following tick. (7.3) |
| **EntityId** | The `u32` handle by which world objects refer to each other; the player is 0. (6.4) |
| **Group / file / archive** | Cache addressing levels. (8.1) |
| **LocMesh** | A decoded, lit object or character mesh split into opaque, translucent and textured parts, optionally with its rig. (8.6) |
| **Map square / region** | A 64 × 64 tile block of the map. (2.6) |
| **Modifier** | The single data primitive for every conditional roll effect. (3.4) |
| **Neighbourhood** | A square decoded together with its eight neighbours. (8.3) |
| **Opcode** | A one-byte (or two-byte) field tag in a cache definition. (8.2) |
| **PendingAttackQueue** | A defender's queue of launched attacks waiting to roll (Doom-style). (4.8) |
| **Phase (tick)** | One of the seven ordered steps within a tick. (2.4) |
| **Phase (boss)** | A named attack pattern and movement policy pair. (6.2) |
| **PlayerId** | The server-assigned session id for a logged-in player; not the same as `EntityId(0)`. (6.6) |
| **Red-x** | NPCs targeting the player hold still while the player walks up to something it clicked. (6.4) |
| **Registry** | All loaded JSON definitions, by id. (6.3) |
| **Render space** | Bevy coordinates: tile `(x, z)` at `(x, y, -z)`, one unit per tile. (2.6) |
| **Rig** | A model's merged vertices and their animation labels, kept so frames can re-pose them. (8.6) |
| **Scenario** | A JSON file describing a complete fight. (6.7) |
| **Snapshot** | The read-only state the server publishes after each tick. (6.6) |
| **Step tiles** | The tiles an actor moved through during one tick, in order. (5.5, 7.3) |
| **Streamer (map)** | The server hook that merges collision squares around the player before each tick. (7.1) |
| **Sweep** | A test that decodes every record of a cache type and checks exact byte consumption. (8) |
| **TargetSpec** | What a path search is trying to reach (exact tile, adjacency, range...). (5.3) |
