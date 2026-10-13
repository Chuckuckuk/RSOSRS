# 9. `osrs_render` and `osrs_viewer`

Crates: [`osrs_render/`](../osrs_render/), [`osrs_viewer/`](../osrs_viewer/).

`osrs_cache` produces plain data (positions, colours, indices). `osrs_render` turns that data into
Bevy entities and keeps them animated, and it is shared by the game client and the viewer so both
draw the world identically. It also contains the two pieces of client logic that need the decoded
world: mouse picking and map streaming.

---

## 9.1 Meshes and materials: [`lib.rs`](../osrs_render/src/lib.rs)

**Everything is unlit.** The client's lighting has already been baked into vertex colours by
`osrs_cache` (chapter 8.6), so every material is `StandardMaterial { unlit: true, ... }` and the
mesh's vertex colours provide all the shading. Normals are filled with a dummy "up" vector because
Bevy requires the attribute.

| Function | Purpose |
|---|---|
| `to_bevy_mesh(&MeshData) -> Mesh` | positions, colours, dummy normals, indices, and UVs when present |
| `texture_material(tex, cull, ...)` | an unlit material with the texture image (repeat wrapping, nearest magnification, alpha-masked if the texture has transparent pixels) |
| `vertex_color_material(blend, ...)` | an unlit vertex-colour material, opaque or alpha-blended |
| `spawn_loc_mesh(..., &LocMesh, transform) -> Entity` | a parent entity with one child per part: opaque, translucent, and one per texture |
| `spawn_animated(..., &LocMesh, clips, transform) -> Entity` | the same, plus an `AnimatedModel` component playing `clips[0]` |
| `replace_animated(...)` | swap a live animated model's mesh and clips in place (a character that put on armour), keeping its entity, transform and current clip |

`OBJECT_CULL` is `Some(Face::Back)`: object faces are one-sided like the client's. Terrain is not
culled because its triangle winding is not consistent.

### `AnimatedModel` and `ModelAnimationPlugin`

```rust
pub struct AnimatedModel {
    parts: Vec<(Handle<Mesh>, Vec<u32>)>,  // each spawned mesh + per-vertex index into the rig
    clips: Vec<Clip>,                      // baked animations (chapter 8.7)
    active: usize, ticks: f32,             // which clip, how far into it (20 ms client ticks)
    shown: Option<(usize, usize)>,         // (clip, frame) currently in the meshes
    looping: bool,
}
```

* `play(i)` loops clip `i` from its start (no-op if it is already looping).
* `play_once(i)` plays clip `i` once and holds its last frame (attacks, deaths).
* `clip_seconds(i)` is a clip's length.

The `play_animations` system advances `ticks` by 50 per second, finds the current frame with
`Clip::frame_at`, and only if the frame changed rewrites each part's vertex positions from the
baked frame (converting model units to tiles with the usual `[x/128, -y/128, -z/128]`). The
`source` index list maps each mesh vertex back to its rig vertex, because the mesh duplicates
vertices per face while the rig does not. Lighting stays as baked in the bind pose, which is also
what the real client does.

---

## 9.2 Map rendering: [`world.rs`](../osrs_render/src/world.rs)

### Loading a fixed block (viewer and tests)

`load_world(rx, ry, nx, ny) -> WorldData` decodes a block of squares in one go: the
`TerrainGrid`, a terrain mesh per plane, the `CollisionGrid`, the object scene and every texture.
`OSRS_NO_OBJECTS` and `OSRS_NO_TEXTURES` skip parts for faster debugging. `WorldData` offers
`ground_at(plane, x, z)` and `surface_y(plane, x, z)` (bilinear height), with bridge decks read
from the plane above.

### `ObjectScene` and `load_objects`

Many placements share the same mesh (every tree of one kind in one orientation looks the same).
`ObjectScene` stores each **unique** mesh once, keyed by `ObjectKey` = `(object id, kind,
orientation, wall displacement, instance)`, and a list of instances `(key, position, plane)`.
The last field is 0 for a shared mesh; a contoured object (below) gets a number of its own.

`load_objects(...)` walks the placements, works out each wall decoration's stand-off from the wall
on the same tile (255 in the key means "no wall here"), builds each unique mesh with
`osrs_cache::scene::build_loc`, and places each instance at the centre of its rotated footprint,
at the average height of the footprint's four corners. Objects on bridge tiles read their heights
from the deck's plane.

**Contoured ground.** Some objects are meant to lie on the terrain rather than stand rigid on it:
the definition's opcode 21 (or 81) sets `ObjectDef::contoured_ground`, the client's
`contouredGround`. The steps up to Lumbridge Castle's door are like this: each step tile is a flat
ribbed tread (object 3684, a floor decoration) that only becomes a staircase when bent onto the
slope. For such a placement the shared mesh is copied and `osrs_cache::scene::contour` raises each
vertex by how far the ground under it (`TerrainGrid::surface_height`, blended between the tile's
corners) sits above the placement height. With 0 the whole model follows the ground; with `n > 0`
only its lowest `n / 65536` of height does, fading out upward. Without this the treads sat level
and poked out of the hill as tilted wedges.

With a `window`, only one square's placements are
kept, relative to its corner (used by streaming). Decoded models are cached in a
`HashMap<u32, Option<Rc<Model>>>` across calls.

### Spawning: `spawn_parts`

`spawn_parts(..., terrain, objects, offset) -> Vec<Entity>` spawns everything and returns every
entity it made, so a streamed square can be despawned as a unit.

* One terrain mesh per plane (plus textured overlay meshes), each tagged `OnPlane(p)`.
* Per object instance: the shared opaque mesh, the shared textured meshes, and a **per-instance**
  copy of the translucent mesh with a `SortedFaces` component.
* Materials come from a `MaterialCache` resource so that one terrain material, one opaque, one
  blended and one per texture are shared by every square, rather than duplicated per square.
* Textures that scroll (water, lava) are registered in `ScrollingTextures`.

### Systems added by `WorldRenderPlugin`

| System | What it does |
|---|---|
| `scroll_textures` | moves each scrolling material's UV transform over time in its direction (`SCROLL_PER_SPEED` is tuned by eye). Only directions 1 (fountain and waterfall water, which must fall: toward -v in our UVs) and 4 (sideways: the lava family 99-115) occur |
| `sort_translucent` | when the camera has moved, re-sorts each visible translucent object's faces far-to-near (by face centroid distance) and rewrites the index buffer; the client paints translucent faces in depth order, and an unsorted blended mesh shows through itself wrongly |

### Tile grid overlay

`square_tile_grid_mesh(&SquareData)` and `tile_grid_mesh(&WorldData, plane)` build line meshes
along every tile edge at terrain height (bridge decks at deck height), lifted 0.03 tiles to avoid
z-fighting. The `TileGrid` marker lets apps toggle them (G in the client).

---

## 9.3 Streaming squares: [`streaming.rs`](../osrs_render/src/streaming.rs)

The client does not load a fixed block. It loads squares as the player walks.

| Type | Role |
|---|---|
| `SquareData` | one decoded square, ready to spawn: four terrain meshes, the object scene, ground and deck corner heights (65 × 65), and which tiles are bridge decks. Plain data, `Send`. |
| `SquareLoader` | decodes squares: `load(sx, sy)` decodes the 3 × 3 neighbourhood and keeps the middle (chapter 8.3). Holds `Rc<Model>`s, so it is not `Send` and must be built on the thread that uses it. `object_info()` returns every object's left-click option and name. |
| `SquareStream` | a thread owning a `SquareLoader`; `request(square)` and `poll() -> Vec<Loaded>` (`Ready(SquareData)` or `Failed(square, reason)`) |
| `StreamedWorld` (resource) | the squares currently in the scene, each with its heights and its entities |

`StreamedWorld` methods: `spawn_square`, `attach(square, entity)` (despawn this with the square),
`remove(square)` (returns the entities to despawn), `contains`, `squares`, and the ground queries
`tile_corners(x, z)`, `ground_at(x, z)` and `surface_y(plane, x, z)`.

`square_of(x, z)` is floor division by 64; `square_offset(square)` is a square's south-west corner
in render space. Since render space is world tile space (with z flipped), squares are spawned at
their real location and nothing ever needs re-positioning.

Only plane 0 is streamed for heights and picking.

---

## 9.4 Mouse picking: [`pick.rs`](../osrs_render/src/pick.rs)

Picking turns a camera ray (from `Camera::viewport_to_world`) into what the player clicked.

**Ground.** `pick_ground` marches along the ray in quarter-tile steps until it passes below the
terrain surface (`GroundSurface::surface_y`, a trait implemented by both `WorldData` and
`StreamedWorld`), then bisects 24 times between the last point above and the first point below to
find the crossing precisely. It gives up after 400 tiles.

**Objects.** A `Picker` holds a `Target` per named object instance on one plane: its axis-aligned
bounding box, a reference to its mesh, and whether it **intercepts** left-clicks. In OSRS, only
objects whose first option is a real action (Open, Chop down) take a left-click; a fence or a rug
has no click box. Named objects that do not intercept can still be examined from the right-click
menu.

`pick_all(ground, ray) -> Option<Click>`:

1. Find the ground hit and its distance along the ray.
2. For each target, a cheap ray/box test (`ray_aabb`, the slab method); skip it if the box is
   behind the ground or farther than the best hits so far.
3. Otherwise test every triangle (`ray_triangle`, the Möller-Trumbore algorithm, two-sided) and
   track the nearest hit among all named objects and among intercepting ones.

`Click` reports the ground tile, the intercepting object `(id, name, option)`, the nearest
examinable object `(id, name)`, and the three distances, so the caller can compare an NPC's
distance against them. `pick(...)` is a simplified wrapper returning a `Pick` enum.

For streamed worlds, `Picker::streamed(plane, left_click, names)` starts empty and
`add(square, objects, offset)` / `remove(square)` follow the squares in and out.

Tests in [`osrs_render/tests/pick.rs`](../osrs_render/tests/pick.rs) check picking against real
Lumbridge geometry.

---

## 9.5 `osrs_viewer`

File: [`osrs_viewer/src/main.rs`](../osrs_viewer/src/main.rs).

A debugging tool that loads a fixed block with `load_world` and lets you fly over it.

```
cargo run -p osrs_viewer --release -- 50 50 1 1     # Lumbridge square
cargo run -p osrs_viewer --release -- 49 49 3 3     # 3x3 squares (default)
```

| Input | Effect |
|---|---|
| WASD | move |
| Q / E | down / up |
| hold right mouse | look around (the cursor is grabbed) |
| Shift | move faster |
| scroll wheel | change speed |
| 1-4 | viewer plane: with roofs off, planes above it are hidden |
| R | toggle roofs (all planes drawn) |

Environment: `OSRS_PLANE`, `OSRS_ROOFS=0|1`, `OSRS_LOOK=x,z,dist` (aim at a tile),
`OSRS_SCREENSHOT=path.png` and `OSRS_SCREENSHOT_DELAY`.

Its systems are `setup` (spawn the world and a `FlyCamera`), `fly_camera`, `view_controls`
(planes and roofs, by toggling `Visibility` on `OnPlane` entities) and `screenshot_and_exit`.
