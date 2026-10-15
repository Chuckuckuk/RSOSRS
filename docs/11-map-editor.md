# 11. The map editor and compiled caches

Crate: [`osrs_map_editor/`](../osrs_map_editor/). Cache side: [`osrs_cache`](../osrs_cache/)'s
`project`, `compile`, `writer`, `reftable`, `active` and `overlay` modules.

The map editor places and removes things in the game world:

* scenery objects ("assets");
* resource nodes (objects listed in `data/gathering.json`);
* NPC spawns;
* minimap icons.

It never writes to the installed game's cache. Edits live in a **project** file. **Compiling** a
project builds a **new cache**: a full copy of the base cache with the edits written into it. The
game reads whichever cache is **active**, so you can switch between the original world and any
number of edited ones.

```
 ~/.rsosrs/projects/<name>.json ──compile──► ~/.rsosrs/caches/<cache>/   (full cache + npc_spawns.json)
          ▲                                          ▲
     map editor                       ~/.rsosrs/active_cache ──► the game (Cache::open_default)
          │
 RuneLite LIVE cache (read only)
```

---

## 11.1 Running it

```
cargo run -p osrs_map_editor --release                          # project "default", at Lumbridge
cargo run -p osrs_map_editor --release -- --project castle --at 3222,3218
cargo run -p osrs_map_editor --release -- --project castle --compile castle --activate   # no window
```

| Option | Meaning |
|---|---|
| `--project <name>` | open `~/.rsosrs/projects/<name>.json`, or start it if it does not exist |
| `--base <dir>` | the cache to edit on (default: the installed game's) |
| `--at x,z` | the tile the camera starts over |
| `--compile <cache>` | no window: compile the project into `~/.rsosrs/caches/<cache>` (it becomes the project's cache) and exit |
| `--activate` | no window: make the project's cache the one the game reads |
| `--play` | no window: start the game on the project's cache, compiling one first (named after the project) if it has none, or rebuilding it if the project has changed since; with `--at`, the game starts on that tile |

| Input | Effect |
|---|---|
| WASD, Q/E | fly; Shift is faster; the wheel changes speed |
| hold right mouse | look around |
| V / B / X | Select, Place, Erase tool |
| left click | select / place / erase, by tool |
| R | turn what is placed next a quarter |
| 1-4 | the plane placements go on (with roofs off, planes above it are hidden) |
| T | roofs (all planes) on or off |
| Delete | remove the selection, or what is under the mouse |
| Ctrl+Z, Ctrl+Y, Ctrl+S | undo, redo, save |
| F5 | play the project (see below) |

The **left panel** holds the tools and the palette, which has four tabs:

* **Objects:** every object definition with a model.
* **Resources:** the objects `gathering.json` makes gatherable.
* **NPCs:** every NPC the cache can draw (it has models of its own and a standing animation),
  one row per name and level. A row with several models ("Bouncer (level 137) ×4") opens a
  **model picker** of thumbnails drawn from each model (`thumbs.rs`); hovering one shows its size
  and menu options. A ★ marks NPCs with combat stats in `npcs.json`; a tick-box lists only those.
* **Map icons:** every map function, with its sprite.

Choosing an entry switches to the Place tool. An object being placed is drawn where it would go,
with its footprint outlined.

The **right panel** has four parts:

* the selection;
* what is under the mouse on the current plane;
* a minimap preview of the square in view, drawn by the game's own minimap code;
* the project: NPC spawns and edited squares, with links to go there, and a "Go to tile" box.

The **Cache** menu does two things:

* It compiles the project. The project is saved first. The compile runs on a thread and takes
  well under a second.
* It chooses the cache the game reads: the installed one ("live"), or any compiled one.

The **Play** menu (F5) starts the game on the project's own cache:
- **The project has a cache with all its edits:** the game starts on it straight away.
- **You have edited since the cache was built:** the editor asks to rebuild it first ("Rebuild
  and play", or "Play the last build"). A compile stores the project's `fingerprint` (a hash of
  its square edits, NPC spawns and markers) in the cache's manifest, and Play compares it with the
  project as it is now.
- **It has none:** the editor asks to create one (named after the project by default), compiles
  it, then starts the game.
- **Where the game starts:** by default you enter the world on the tile the camera is looking at
  ("Start where the camera is looking"); untick it to start at the usual spot.
- **The active cache is not changed:** the game runs as its own process with `OSRS_CACHE_DIR` set
  to the project's cache. It is started with `cargo run -p osrs_core --release` from the workspace
  the editor was built in, so it is built from current source. Without that workspace, the
  `osrs_core` executable beside the editor's is used.

A project's cache is recorded in the project file (`cache`), and every compile of the project
updates it. An older project that names none is matched to a compiled cache by the project name in
the cache's manifest.

Closing the window saves the project.

---

## 11.2 Projects: [`osrs_cache::project`](../osrs_cache/src/project.rs)

```
MapProject {
    name,
    squares: { "sx_sy": SquareEdits { removed: [Placement], added: [Placement] } },
    npc_spawns: [NpcSpawn { npc: "man", x, z, plane }],
    marker_defs: [MarkerDef { id, map_icon }],
    cache: Some("castle"),           // the compiled cache it plays on, once made
}
```

* **Placements** are stored exactly as the map file stores them: object id, plane, square-local
  x and z, kind (shape) and orientation.
  * `SquareEdits::apply` takes one equal placement out for each removed entry, then appends the
    added ones.
  * Removing something the project added just forgets it, and re-adding something it removed
    cancels the removal, so the edits stay minimal.
* **NPC spawns** name an NPC in one of two ways:
  * an id in `data/npcs.json` (`"man"`), a curated monster with combat stats;
  * `cache:<id>` (`"cache:3105"`, Hans), any NPC straight from the cache.

  When the server starts, a `cache:<id>` spawn fights with the stats of the curated monster that
  covers that id: its `appearance` (the model it is drawn as by default) or one of its
  `cache_ids` (the same monster's other models, such as the abyssal demon's 416, 7241 and 11239).
  The spawn is still drawn as model `<id>` (`cache_npc_variant`). A spawn with no curated monster
  is built from the cache alone (`osrs_core::data::cache_npc_def`): its name, combat level, size
  and model. Such an NPC stands where it is put, and "Attack" answers "cannot hurt" until it has
  stats. Registry validation rejects a cache id that two curated monsters both claim.
  * A new project starts with the man who stands in Lumbridge by default, so a compiled cache
    keeps him until he is removed.

### Giving an NPC combat stats, when it needs them

Stats come from the OSRS wiki, one NPC at a time; nothing is fetched in bulk.

1. Find its cache id with the palette (the `#id` after its name) or
   `cargo run -p osrs_cache --release --bin npc_probe -- "<name>"`.
2. Read its wiki infobox (`https://oldschool.runescape.wiki/w/<Name>?action=raw`): the `|id`
   line lists its NPC ids, and the stats are there too.
3. Look up its attack, block and death animations in RuneLite's
   [`gameval/AnimationID.java`](https://github.com/runelite/runelite/blob/master/runelite-api/src/main/java/net/runelite/api/gameval/AnimationID.java)
   (the game's own names, such as `ABYSSAL_ATTACK`). Check them with
   `npc_anim_probe <npc id> <sequences...>`.
4. Add an entry to `osrs_core/data/npcs.json` with `"appearance": <cache id>`, and its other NPC
   ids from the wiki's `|id` lines as `"cache_ids": [...]`. Say in its `notes` where each value
   came from (the abyssal demon is an example).

Every spawn of any of those ids then fights with those stats, drawn as its own model, including
spawns placed before.
* **Minimap icons** are objects in the real game: an object definition's opcode 82 names a map
  function (an area definition, config group 35), and the minimap draws its sprite where the
  object stands. To place an icon the editor puts down an invisible floor decoration (kind 22)
  carrying it:
  * an object the cache already has for that icon, if one has no model, blocks nothing and has no
    options (`compile::existing_marker`);
  * otherwise one the project mints: a `MarkerDef` with the next free object id, compiled into the
    object definitions as `[2 "Map icon"][17][18][82 icon][0]`.

Because a project is a list of edits, not a copy of the map, it can be compiled again after
RuneLite updates the installed cache.

---

## 11.3 Writing a cache

The read side is described in chapter 8. Writing is its inverse, layer by layer.

| Layer | Writer | Inverse of |
|---|---|---|
| placements file | `locs::encode`: ids ascending as `smart_chain` deltas, positions ascending as `smart_u` deltas, attribute `kind << 2 \| orientation` | `locs::decode` |
| a group's files | `cache::join_files`: files back to back plus a one-chunk size trailer | `split_files` |
| container | `container::encode(data, Gzip \| None)` | `container::decode` |
| reference table | `reftable::RefTable` (decode and encode, keeping every column: name hashes, CRCs, the flag 8/2/4 arrays, revisions, file ids) | `parse_reference_table` |
| sectors and index | `writer::CacheWriter::put`: appends the container as a fresh 520-byte sector chain and points the group's 6-byte index entry at it | `Cache::read_raw` |

Tests in [`osrs_cache/tests/writing.rs`](../osrs_cache/tests/writing.rs) hold the encoders to
the real cache:

* every reference table re-encodes byte for byte;
* every map square's placements re-encode to the same placements, and to the same bytes in all but
  one square of about 25,000 (it stores some in an order the encoder does not reproduce).

**`compile::build`** turns a project into finished containers:

1. Each edited square's group (archive 5, group `sx << 8 | sy`) is rebuilt: its terrain file is
   untouched, and its placements file is decoded, edited and re-encoded.
2. If the project mints markers, the object definitions group (archive 2, group 6) is rebuilt with
   them added as new files.
3. Each touched archive's reference table is updated with `RefTable::touch`:
   * the group's CRC-32 is set (of the new container) and its sizes;
   * its revision goes one past the newest;
   * its whirlpool digest is blanked; nothing reads it.
   * The table itself is re-encoded as group 255/archive.

**`compile::compile(project, base, out, progress)`** writes those containers into a new cache:

* It refuses three targets: the base itself, the live cache, and an existing directory that is not
  a compiled cache (it has no `rsosrs_cache.json`).
* `CacheWriter::create` copies the base's `main_file_cache.*` files into `<out>.building`, and the
  containers are appended there.
* Two files are added beside the cache: `npc_spawns.json` and the manifest, `rsosrs_cache.json`
  (base, project, time, counts).
* Last, the staging directory replaces `<out>`. A failed compile never leaves half a cache, and an
  earlier compile of the same name stays whole until the new one is complete.

The old sectors of a rewritten group stay in the data file, unreferenced, so nothing else moves.
Compiling a 250 MB cache takes about a third of a second.

---

## 11.4 Choosing the cache: [`osrs_cache::active`](../osrs_cache/src/active.rs)

`Cache::open_default()` (which every reader in the game calls) now opens the **active** cache:

1. **`OSRS_CACHE_DIR`**, if set: a cache directory, or `live`;
2. else **`~/.rsosrs/active_cache`**: a text file holding a directory or `live`, written by the
   editor's Cache menu and by `--activate`;
3. else the installed game's cache.

A selected directory that holds no cache falls back to the live one, with a warning.
`Cache::open_live()` always opens the installed cache, whichever one is active. The integration
tests that build on it use it. `RSOSRS_HOME` moves `~/.rsosrs` (for experiments and tests).

The game prints `[cache] reading <dir>` at start. The server looks for `npc_spawns.json` beside
the active cache (`active::npc_spawns`):

* **If it is there,** every spawn in it is placed on ground level, at the nearest open tile. The
  server loads each spawn's map square first, however far it is from the player.
* **If not (the live cache),** the old single man appears.

---

## 11.5 Seeing edits before compiling: [`osrs_cache::overlay`](../osrs_cache/src/overlay.rs)

The editor shows its edits through the same decoders the game uses:

* After every change it runs `compile::build_with(project, base, Compression::None)` and puts the
  containers in the **overlay**, a process-wide map of `(archive, group) → container`.
* `Cache::read_raw` answers from the overlay first.
* The changed square and its eight neighbours are then decoded again by the square loader thread.

So terrain, objects, collision and the minimap preview show exactly what a compiled cache will
hold. The base cache the edits are built on is opened with `Cache::ignoring_overlay()`, so edits
are never applied on top of themselves.

The editor also sets `OSRS_CACHE_DIR` to its base cache for its own process. Its loader thread
therefore reads the base plus the overlay, whatever cache the game has been switched to.

---

## 11.6 The editor's code: [`osrs_map_editor/src`](../osrs_map_editor/src/)

| File | What it does |
|---|---|
| `main.rs` | arguments, the headless `--compile`, the Bevy app, `OSRS_SCREENSHOT`, saving on close |
| `catalog.rs` | the palette: object definitions, gathering resources, NPCs, map functions; shapes and footprints |
| `editor.rs` | the `Editor` resource: project, tool settings, the undo stack, every edit, rebuilding the overlay |
| `world.rs` | streaming squares around the camera (`SquareStream`, `StreamedWorld`), reloading changed squares, hover picking (a `Picker` in which every object takes clicks), the placement ghost |
| `tools.rs` | clicks and shortcuts, finding the placement under the mouse, gizmo outlines and posts for NPCs and icons |
| `ui.rs` | the egui panels, dialogs, compile job, the Play menu, labels over NPCs and icon sprites in the world |
| `play.rs` | starting the game on a cache |
| `preview.rs` | the minimap preview (`MinimapSource` through the overlay) and sprite textures |
| `camera.rs` | the fly camera |

The UI is [`bevy_egui`](https://github.com/vladbat00/bevy_egui) 0.43 (egui 0.36).

Limits for now:

* NPCs are placed only on ground level, as the server's map is.
* Only map squares the cache has can be edited.
* NPC spawns are drawn as posts, not models.
* The editor's own preview of a spawn ignores its wander radius (it comes from `npcs.json`).
* An NPC without stats in `npcs.json` cannot be fought, talked to or made to wander.
