# 8. `osrs_cache`: reading the game's files

Crate: [`osrs_cache/`](../osrs_cache/). Format findings log: [`osrs_cache/README.md`](../osrs_cache/README.md).

The OSRS client stores all of its assets (maps, 3D models, animations, sprites, definitions of
every item, NPC and object) in a set of binary files called **the cache**. This crate reads that
cache from your RuneLite installation and decodes it into ordinary Rust structs and triangle
meshes. It has no graphics engine dependency; `osrs_render` (chapter 9) takes its output to Bevy.

Much of the format here differs from public documentation, because the cache revision is from
2026. Every such difference was pinned down by writing a "sweep" that decodes **every** record of
a kind and checks that each one consumes exactly its bytes. The integration tests in
[`tests/cache.rs`](../osrs_cache/tests/cache.rs) re-run those sweeps; they skip themselves when no
cache is installed.

Nothing decoded from the cache is written into the repository; it is read at runtime only.

---

## 8.1 Storage layers

```
main_file_cache.idxN  ──►  sector chain in main_file_cache.dat2  ──►  container (compressed)
                                                                        │
                                                                        ▼ decompress
                                                                      group bytes
                                                                        │ split by reference table
                                                                        ▼
                                                                      files ──► typed decoders
```

Vocabulary:

| Term | Meaning |
|---|---|
| **archive** (or index) | a numbered category, one `.idxN` file each: 0 animation frames, 1 frame bases (skeletons), 2 configs, 5 maps, 7 models, 8 sprites, 9 textures, 13 fonts (constants in `idx`) |
| **group** | one compressed blob inside an archive, addressed `(archive, group id)` |
| **file** | a sub-record inside a group; many small definitions are packed into one group |
| **reference table** | a per-archive directory (stored in archive 255) listing each group's id, name hash, CRC, revision and file ids |

### Reading a group: [`cache.rs`](../osrs_cache/src/cache.rs)

`Cache::open(dir)` opens `main_file_cache.dat2`. `Cache::open_default()` opens the *active*
cache (chapter 11.4): `OSRS_CACHE_DIR`, else the one chosen in `~/.rsosrs/active_cache`, else the
installed game's under `%USERPROFILE%\.runelite\jagexcache\oldschool\LIVE` (then
`~/jagexcache/oldschool/LIVE`). `Cache::open_live()` always opens the installed one.

The crate also writes caches, for the map editor: every layer below has an inverse
(`locs::encode`, `cache::join_files`, `container::encode`, `reftable::RefTable::encode`,
`writer::CacheWriter`), described in chapter 11.3. Writing only ever targets a copy; the installed
cache is read only. A cache opened `with_overlay(Arc<overlay::Overlay>)` answers reads of the
groups the overlay holds from memory instead of its files (the map editor's uncompiled edits,
chapter 11.5).

1. **Index entry.** `main_file_cache.idxN` is an array of 6-byte records, one per group: a 24-bit
   length and a 24-bit first sector number. The whole idx file is read once and memoised behind
   a `Mutex`.
2. **Sector chain.** `.dat2` is divided into 520-byte sectors. Each sector starts with a header
   (group id, chunk number, next sector, archive id; 8 bytes, or 10 when the group id needs 32
   bits) followed by data. `read_raw` follows the chain, checking every header, until it has
   `length` bytes. The file handle is behind a `Mutex` because seeking mutates it.
3. **Container.** [`container::decode`](../osrs_cache/src/container.rs) reads
   `[compression u8][length u32][uncompressed length u32, if compressed][payload]`. Compression 0
   is none, 1 is bzip2 (the cache strips the `BZh1` magic, so the decoder puts it back), 2 is
   gzip. An optional [XTEA](../osrs_cache/src/xtea.rs) key decrypts the payload first; this
   revision's maps are not encrypted, so keys are unused.
4. **Splitting files.** A group with several files ends with a trailer: the last byte is a chunk
   count, preceded by `chunks × files` delta-encoded 32-bit sizes. `split_files` reassembles each
   file from its chunks.

`Cache::index(archive)` parses that archive's reference table (protocols 5-7; protocol 7 uses
"big smart" ids) through [`reftable::RefTable`](../osrs_cache/src/reftable.rs), which keeps every
column so a table can be written back byte for byte. `Index::group_by_name` finds a group by the Jagex string hash
([`name_hash`](../osrs_cache/src/lib.rs)).

Convenience methods: `read_group(archive, group)`, `read_files(&index, group)` and
`read_file(&index, group, file)`.

### Reading bytes: [`buf.rs`](../osrs_cache/src/buf.rs)

`Buf` is a big-endian cursor over a byte slice, like a minimal `io.BytesIO` with
`struct.unpack('>H', ...)` built in. Reading past the end returns zeros instead of panicking, so
a truncated record degrades rather than crashing the viewer.

The cache's variable-length integer encodings ("smarts"):

| Method | Encoding |
|---|---|
| `smart_u` | 1 byte if the first byte is below 128, else 2 bytes minus 32768 |
| `smart_s` | the signed version: 1 byte minus 64, or 2 bytes minus 49152 |
| `big_smart` | 2 bytes, or 4 bytes (31 bits) when the top bit is set |
| `smart_chain` | sum `smart_u` values while they equal 32767 (used for large id deltas) |
| `string` | bytes up to a 0 terminator (cp1252) |

---

## 8.2 Opcode-based definitions

Most definitions (objects, NPCs, items, identity kits, sequences, spot animations, hit splats,
health bars, underlays, overlays) share one format: a stream of `[opcode u8][payload]` records
ending with opcode 0. A decoder is a `loop { match b.u8() { 0 => break, 1 => ..., 2 => ... } }`.

When a decoder meets an opcode it does not know, it **stops** and records it in an
`unknown_opcode` field rather than guessing the payload size and misreading everything after it.
Probe binaries such as `npc_probe` and `item_census` count how many definitions stop early and
at which opcode. The README lists the revision-specific opcode layouts that were discovered this
way (for example, model id lists widened from 16 to 32 bits in opcodes 6/7 for objects and 61 for
NPCs).

[`Configs`](../osrs_cache/src/config.rs) holds the config archive's reference table and offers a
loader per definition type (`objects()`, `npcs()`, `items()`, `identkits()`, `spotanims()`,
`hitsplats()`, `underlays()`, `overlays()`, `texture_average_hsl()`), each returning a
`HashMap<u32, Def>`. The config group numbers are in the `cfg` module of
[`lib.rs`](../osrs_cache/src/lib.rs).

---

## 8.3 Maps

A map square's group id in archive 5 is `(square_x << 8) | square_y`. File 0 is terrain and file
1 is object placements.

### Terrain: [`terrain.rs`](../osrs_cache/src/terrain.rs)

4 planes × 64 × 64 tiles, stored plane by plane, x-major. Each tile is a sequence of 16-bit
opcodes:

| Opcode | Meaning |
|---|---|
| 0 | end of tile; height comes from noise (plane 0) or the plane below minus 240 |
| 1 | next byte is an explicit height (×8), then end of tile |
| 2..=49 | overlay: a 16-bit overlay id follows; shape = `(op - 2) / 4`, rotation = `(op - 2) & 3` |
| 50..=81 | settings flags = `op - 49` (bit 1 blocked, bit 2 bridge, ...) |
| 82.. | underlay id = `op - 81` |

Tiles without an explicit height use the client's procedural noise function, ported exactly
(`noise_height`, from the world coordinates). Heights are stored negative-up.

### Placements: [`locs.rs`](../osrs_cache/src/locs.rs)

Delta-encoded: an object id delta (`smart_chain`), then that object's placements as position
deltas (`smart_u`) until 0. A position packs plane, x and z into 14 bits; each placement's
attribute byte packs the **kind** (upper 6 bits: 0-3 walls, 4-8 wall decorations, 9 diagonal
walls, 10-11 scenery, 12-21 roofs and other, 22 floor decoration) and the **orientation** (lower
2 bits: quarter turns).

### Grids of squares: [`terrain_mesh.rs`](../osrs_cache/src/terrain_mesh.rs)

`TerrainGrid::load(cache, sx, sy, nx, ny, underlays, overlays)` stitches `nx × ny` squares into one
grid of tiles, indexed `[plane][x][z]`. Squares missing from the cache are recorded in `missing`
and left flat and empty. `PlacedObjects::load` collects every placement in grid coordinates and
applies the **bridge rule**: an object on a tile whose plane-1 settings have the bridge bit moves
down one plane (objects already on plane 0 there are dropped), because the client draws a bridge
deck at ground level.

### Neighbourhoods: [`squares.rs`](../osrs_cache/src/squares.rs)

Streaming loads one square at a time, but a square cannot be decoded alone: underlay colours blend
over an 11 × 11 tile window, lighting reads neighbouring heights, and scenery near a border can
stand across it. So `MapSource::neighbourhood(sx, sy)` decodes the square **with its eight
neighbours** (a 192 × 192 tile grid) and callers keep only the middle 64 × 64. Tests check that a
square decoded this way matches the same tiles of a larger block exactly, for both terrain
triangles and collision.

`MapSource` holds the cache plus the underlay, overlay and object definitions, decoded once.
`MapSource::collision(sx, sy)` returns the middle square's collision flags.

### Collision: [`collision.rs`](../osrs_cache/src/collision.rs)

`CollisionGrid::build(grid, placed, object_defs)` reproduces the client's plane-0 collision, using
the same bit values as `osrs_core::collision::CollisionFlag`:

* **Terrain**: a tile with settings bit 1 gets `FLOOR`. Under a bridge deck the deck's flags (from
  plane 1) apply instead.
* **Walls** (kinds 0-3), if the definition's clip type is not 0: straight walls set an edge bit on
  their tile and the opposite bit on the neighbour; diagonal corner pieces set corner bits; L-shaped
  corners set two edges.
* **Scenery** (kinds 9-21), if clip type is not 0: `OBJECT` over the whole (rotated) footprint.
* **Floor decorations** (22): `FLOOR_DECORATION` only when the definition's opcode 19 is 1.

Doors are baked as they are placed (a closed door is a wall); opening one moves its wall at run
time on the server (`osrs_core::sim::objects`), with the same edge table
(`collision::wall_edges`, shared by both crates). Only plane 0 is produced. The minimap draws
such changes too: `MinimapSource::render_changed` swaps placements before drawing a square.

---

## 8.4 Definitions used for maps

[`config.rs`](../osrs_cache/src/config.rs):

* `Underlay`: an RGB colour (and rarely a texture). Underlays are the blended ground colour.
* `Overlay`: an RGB colour or a texture id, whether it hides the underlay, a secondary colour. The
  overlay colour `#FF00FF` (magenta) is the map makers' "draw nothing here" marker.
* `ObjectDef`: name, model ids and their types, size, clip type (`interact_type`: opcode 17 sets
  0, the default is 2), `interactive_flag` (opcode 19), ambient and contrast, wall decoration
  displacement, menu options by slot (opcodes 30-34), recolour and retexture tables, scale,
  offset, mirroring, map icon, and "transforms" (objects that change form with game state).
  `left_click()` returns the first menu option unless it is empty, "Hidden" or "Walk here".

---

## 8.5 Models: [`model.rs`](../osrs_cache/src/model.rs)

A model (archive 7) is an indexed triangle mesh: vertex arrays `vx, vy, vz` (client units, y down),
face index arrays `fa, fb, fc`, and per-face arrays for colour (16-bit HSL), alpha, render type,
texture id, priority and animation labels ("skins") per vertex and face.

Models come in wire versions marked by their last two bytes. This cache only contains `ff fe` (v2)
and `ff fd` (v3); legacy decoding exists too. The header is read **backwards** from the end, and
gives the counts and section sizes. The body is a set of parallel streams; vertex positions are
delta-encoded signed smarts selected by per-vertex flag bits, and face indices use a small
"compression type" state machine that reuses previous vertices. v3 moved several sections relative
to v2 (the README lists them). One section (texture-space triangle records) has a variable
encoding that is not fully decoded; `build` tries a few candidate section sizes until the vertex
streams line up. All 62,050 models in the cache decode, and a test asserts zero failures.

`Model::recolor(from, to)` applies a definition's recolour table.

### Colour: Jagex HSL

Colours are packed 16-bit HSL: 6 bits hue, 3 bits saturation, 7 bits lightness.
`hsl_to_rgb(hsl, brightness)` converts to RGB in 0..1, with the client's gamma (`brightness` 0.8).
`rgb_to_hsl16` packs the other way. Lighting works by scaling the 7-bit lightness.

---

## 8.6 Meshes ready for rendering

### Terrain: `TerrainGrid::build_mesh_window`

[`terrain_mesh.rs`](../osrs_cache/src/terrain_mesh.rs) follows the client's scene builder:

1. **Underlay blending.** Each tile's underlay colour is the average hue, saturation and lightness
   of the underlays in the 11 × 11 tiles around it (hue weighted), as the client does. This is what
   makes grass fade smoothly into dirt.
2. **Lighting.** A per-corner intensity: ambient 96 plus the dot product of a light direction
   `(-50, -10, -50)` with the height-map normal (from neighbouring height differences).
3. **Tile shapes.** An overlay tile has one of 13 shapes (half-tile diagonals, corners, paths).
   `SHAPE_VERTICES` and `SHAPE_TRIANGLES` list each shape's vertices and triangles, marking each
   triangle as overlay or underlay coloured. These tables were extracted from the RuneLite client's
   bytecode. Rotation turns the shape in quarter turns.
4. **Skips.** Magenta overlays and their underlay triangles are not drawn.
5. **Planes and bridges.** Plane 0's mesh includes plane-1 bridge deck tiles drawn at plane 1's
   heights; plane 1's mesh excludes them.
6. **Output.** A `TerrainMesh`: positions, linear RGBA vertex colours, indices, and textured overlay
   triangles grouped by texture id with tile-based UVs.

A window (`x0, z0, w, d`) builds only part of the grid while still reading the tiles around it, so
adjacent windows meet without seams. `build_mesh(plane)` is the whole grid.

### Objects: [`scene.rs`](../osrs_cache/src/scene.rs)

`build_loc(def, kind, orientation, ...)` builds the mesh for one placed object, following the
client's `getModel` and `light` routines:

1. **Choose models.** `model_ids_for(def, kind)`: typed models are matched by kind (every wall
   decoration kind draws the type-4 model); untyped ones are used for ordinary and diagonal
   scenery.
2. **Merge** the models (`build_models`), skipping v3 placeholder faces.
3. **Recolour, scale and offset** from the definition.
4. **Mirror** if the definition is flagged rotated, or for one arm of an L-shaped corner wall
   (negate z, swap face winding).
5. **Diagonal forms.** Wall decorations of kinds 6-8 are turned 45° and shifted onto the diagonal;
   diagonal scenery (kind 11) gets an extra 45°.
6. **Rotate** by the orientation's quarter turns.
7. **Light** with gouraud shading: vertex normals are the sum of adjacent face normals, intensity
   is `ambient + dot(light, normal) / (contrast × |light| × |normal| / 256²)`, and the lightness
   of each vertex colour is scaled by it. Flat-shaded faces use the face normal.
8. **Texture coordinates** for textured faces: a planar projection where u runs from the
   texture triangle's P to M and v from P to N (least squares in the face plane).
9. **Split** into a `LocMesh`: `opaque`, `translucent` (faces with alpha) and `textured` (one
   `MeshData` per texture id). Each output vertex records which merged vertex it came from
   (`MeshData::source`), and with `keep_rig` the merged vertices and their labels are kept as a
   `Rig` so animations can re-pose them later.

`Shape` carries the per-caller lighting: objects use ambient `64 + def.ambient`, contrast
`768 + 5 × def.contrast`; players and NPCs use ambient 64, contrast 850 and light `(-30, -50, -30)`.

`footprint(def, orientation)` returns the rotated `(size_x, size_z)` of an object, used by
collision and placement.

---

## 8.7 Animations: [`anim.rs`](../osrs_cache/src/anim.rs)

OSRS animates models with a label-based skeleton rather than bones:

* every **vertex** carries a label byte (its "skin");
* a **frame base** (archive 1) lists transform slots, each with a type (origin, translate, rotate,
  scale, alpha) and the labels it affects;
* a **frame** (archive 0) lists `(slot, dx, dy, dz)` operands against one base;
* a **sequence** (config group 12) lists frames with durations in 20 ms client ticks.

`pose(positions, skin, base, frame)` applies a frame to a copy of the bind pose, exactly like the
client's `Model.animate`:

* **origin**: set the pivot to the mean position of the labelled vertices plus the operand;
* **translate**: add the operand to labelled vertices;
* **rotate**: rotate labelled vertices about the pivot by angles `(value & 255) × 8` out of 2048,
  in the order z, x, y (the z rotation has the opposite sense to the other two; this was verified
  by eye, since the other sign crosses the legs);
* **scale**: scale about the pivot by `operand / 128`;
* **alpha**: ignored.

`Clip::bake(animations, sequence_id, rig)` poses the rig once per frame and stores every posed
vertex array. Playing a clip later is just copying the right frame's positions into the mesh
(chapter 9). `frame_at(ticks)` finds the frame showing at a time into the looping clip.

`Animations` lazily decodes and caches sequences, frames and bases. Revision notes: sequences start
with a one-byte opcode 16; opcodes 13 and up are sound tables (decoding stops there); frame bases
have two extra trailing bytes; about 127 bases use a newer float-based skeletal format that is not
supported.

---

## 8.8 Characters, NPCs, items and graphics

| Module | Decodes | Builds |
|---|---|---|
| [`kit.rs`](../osrs_cache/src/kit.rs) | identity kits (config group 3): body part, models, recolours | `build_player(cache, appearance, worn, texture_hsl)`: slot by slot (head, cape, amulet, weapon, torso, shield, arms, legs, hair, hands, feet, jaw), the worn item's models if any, else the kit's, skipping slots a worn item covers (a full helm hides hair and jaw); each group recoloured with its own table; merged and lit as a player. `Appearance::default_male()` picks kits 0, 10, 18, 26, 33, 36, 42. `WornPart::of(item)` describes what an item puts on. |
| [`npc.rs`](../osrs_cache/src/npc.rs) | NPC definitions (config group 9): models, size, combat level, stand/walk/turn/run sequence ids, menu options, recolours, scale, lighting, transforms | `build_npc(cache, def, texture_hsl)`: merged, scaled, lit, rig kept |
| [`item.rs`](../osrs_cache/src/item.rs) | item definitions (config group 10): name, inventory model and its camera, stackable, options, noted links, "pile of N" models, worn models and slots | `ids_by_name`, `pile_item(quantity)`, `first_option`, `menu_options` |
| [`item_icon.rs`](../osrs_cache/src/item_icon.rs) | | the 36 × 32 inventory picture, rasterised on the CPU: the lit model is rolled, yawed, offset and pitched by the definition's camera, projected with focal length 512 about (16, 16), back-face culled and z-buffered; then a black outline and a dark drop shadow are added |
| [`spotanim.rs`](../osrs_cache/src/spotanim.rs) | spot animations / graphics (config group 13): model, sequence, scale, recolours | `build_spotanim`, `build_spotanim_with_rig` (arrows in flight, the arrow being drawn) |
| [`hitsplat.rs`](../osrs_cache/src/hitsplat.rs) | hit splat definitions (config group 32); type 16 (your damage) and 12 (your block) are redirects chosen by a client setting | `load_hitsplat(kind)`: the middle sprite, text colour and duration |
| [`healthbar.rs`](../osrs_cache/src/healthbar.rs) | health bar definitions (config group 33): front and back sprites, width, duration | `load_healthbar(id)` |
| [`font.rs`](../osrs_cache/src/font.rs) | bitmap fonts: a sprite group per font (one frame per character) plus a metrics file whose first 256 bytes are character advances | `Font::render(text, rgb, shadow)` into a `Texture` |
| [`textures.rs`](../osrs_cache/src/textures.rs) | texture definitions (archive 9: sprite id, average HSL, transparency, scroll direction and speed) and sprites (archive 8: paletted, palette index 0 transparent, trailer with sizes and offsets) | `load_textures`, `load_sprite`, `decode_sprite` into RGBA `Texture`s |

### Sound effects: [`sound.rs`](../osrs_cache/src/sound.rs)

Archive 4 holds the game's synthesised sound effects, one group per sound id. A `SoundEffect` is up
to ten `Instrument`s plus loop points. Each instrument has:

* up to ten oscillators, each with a volume, a detune and a start delay;
* pitch and volume `Envelope`s (piecewise-linear curves; the pitch envelope's `form` picks the
  waveform: square, sine, saw or noise);
* optional vibrato, tremolo and an on/off gate, each a pair of envelopes;
* an echo (delay and feedback);
* a pole/zero `Filter` whose coefficients move along their own envelope.

`SoundEffect::mix` renders the sound exactly as the client's `SoundEffect.mix` does: integer
arithmetic, the 32768-entry sine table, noise from `java.util.Random(0)`, 8-bit saturating mix at
22,050 Hz. The renders match the OSRS Wiki's recordings of the same ids: cross-correlation 0.98
for Protect from Melee (2676) and 0.998 for the zombie's death (922). In the sweep test
(`every_synth_sound_in_the_cache_decodes_to_its_last_byte`), 12,147 of the 12,163 groups decode to
their last byte. The other 16 (ids 11988 and up) hold Vorbis packets rather than instruments, so
they are not read. Sequences in this revision carry no sound tables, so frame sounds are not
decoded: the server plays combat sounds itself.

`sound_probe 4 <id> out.wav` mixes one sound and writes it as a WAV file to listen to.

### Bundles: running without the game installed: [`bundle.rs`](../osrs_cache/src/bundle.rs)

A program can carry its own slice of the cache. A recording run
(`OSRS_RECORD_BUNDLE=<file>` on the client) notes every `(archive, group)` read through a `Cache`
(`bundle::record`, called from `Cache::read_raw`), the client adds what it would read later (every
item picture, every sound the data names), and `bundle::write_payload` copies those groups' raw
containers, plus the reference tables that were read, into one file: for the Jad fight about
1,560 groups and 4 MB. The `bundle_pack` tool appends that payload and a 16-byte trailer
(length + `OSRSBNDL`) to a copy of the executable.

At start-up `bundle::install_from_exe` looks for the trailer in the running executable and, if
found, registers the payload. From then on `Cache::open_default` returns a cache backed by it
(`Cache::from_bundle`), so every caller (six of them in the client, plus the map streamer on the
server thread) works unchanged, and a group that was not recorded is an error rather than a
fallback to the installed game. `package_jad.sh` does all of it. The payload holds Jagex's data:
for private use and sharing among friends, never committed.

---

### Client scripts: [`clientscript.rs`](../osrs_cache/src/clientscript.rs)

Archive 12 holds the interface logic (CS2): one group per script id. `decode` is strict (every byte
consumed, instruction count matches); 9,972 of 9,973 groups decode (group 0 is a 2-byte stub).
Layout is RuneLite's "rev237" form: instructions then switch tables then a 16-byte trailer
(`u32` count, local int/string/long counts, int/string/long arg counts, all `u16`) and a `u16` switch
section length; operands are a u32 below opcode 100 (u8 for `return`/`pop_*`/`push_null`, a string for
`sconst`, a u64 for `lconst`) and a u8 from 100 up (0 = the primary dynamic component, 1 = the secondary
one, e.g. `cc_create 1`). Opcode names come from RuneLite (`clientscript_names.rs`); about 380 newer opcodes
have no name and print as `op_N`. `EnumDef`/`load_enum` read enums (config group 8) which scripts use as
lookup tables (`enum(keytype, valtype, enum id, key)`), and `ItemDef::params` holds item params (`oc_param`).
There is no VM: scripts are read with `cs2_dump` and hand-ported. In the disassembly an interface-op's
component id is the *top* of the stack (pushed last); `cc_*` ops act on the active dynamic component.

## 8.9 Probe binaries

[`osrs_cache/src/bin/`](../osrs_cache/src/bin/) holds about thirty small command-line tools used
while reverse-engineering formats. Each file's first doc comment states its usage. A few useful
ones:

| Tool | Purpose |
|---|---|
| `terrain_png rx ry nx ny out.png [px]` | top-down raster of the terrain mesh, to check decoding without a 3D viewer |
| `model_sweep` | decode every model and summarise failures |
| `npc_probe [name]` | list NPC definitions, or count those stopping at unknown opcodes |
| `sound_probe [archive] [group] [out.wav]` | survey an archive; with archive 4 and a path, mix a synth sound into a WAV |
| `sound_features <id>...` | which synth features (waveform, vibrato, tremolo, gate, echo, filter) each sound uses |
| `sound_compare <dir>` | compares renders with `<id>.wav` recordings (8-bit mono 22,050 Hz) sample for sample; 140 of the 142 wiki WAVs tried were byte-identical to the renders |
| `npc_anim_probe <npc id> <sequence>...` | how an NPC's model moves in each sequence (frames, length, highest point, largest vertex move), to tell attack animations apart without a viewer; with `STRIP=out.png` it also draws each sequence as a row of side-on silhouettes; with `HEAD=1` it prints where the front of the head is in every frame (where a monster's projectile leaves from) |
| `item_probe out.png names...` | render inventory pictures to a sheet |
| `tile_probe x z` | dump one world tile on every plane |
| `find_locs rx ry kind [x z r]` | list placements of a kind in a square |
| `sprite_sheet out.png first last` | contact sheet of sprites (`FRAMES=1`: every frame of each) |
| `hitsplat_probe [out.png]` | print splat definitions and write their sprites |
| `cs2_dump <id>...` | disassemble client scripts (archive 12); `--find-calls <id>`, `--find-comp <group>[:<file>]`, `--grep <opcode> [operand]`, `--enum <id>...`, `--item <id>...` (item params), `--census` (opcodes without a name) |
| `db_dump <table>` | decoded database rows (config group 38, `osrs_cache::dbtable`) of one table: table 78 is each weapon type's attack styles; `tables` counts rows per table, `raw <group> <file>` hex-dumps a config file |

Run them with `cargo run -p osrs_cache --release --bin <name> -- <args>`.
