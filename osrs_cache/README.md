# osrs_cache

Reader and scene builder for the Old School RuneScape game cache, plus `osrs_viewer`
(Bevy) that renders map squares straight from the cache you already have installed
(`%USERPROFILE%\.runelite\jagexcache\oldschool\LIVE`). Nothing from the cache is
redistributed; assets are read at runtime.

    cargo run -p osrs_viewer --release -- 50 50 1 1     # Lumbridge square, with objects
    cargo run -p osrs_viewer --release -- 49 49 3 3     # 3x3 squares
    OSRS_NO_OBJECTS=1 ...                               # terrain only
    OSRS_SCREENSHOT=out.png ...                         # save a frame and exit
    cargo run -p osrs_cache --release --bin terrain_png 49 49 3 3 out.png 6   # top-down raster

Controls: WASD, Q/E down/up, hold right mouse to look, Shift fast, scroll = speed,
1-4 = viewer plane, R = toggle roofs (on: all planes drawn; off: planes above the viewer plane hidden).
Env: `OSRS_PLANE`, `OSRS_ROOFS=0|1` set the initial state.

## Layers
| module | what |
|---|---|
| `cache`, `container`, `xtea` | idx/dat2 sectors, reference tables, bzip2/gzip containers, XTEA |
| `terrain`, `locs` | map square terrain (file 0) and object placements (file 1) |
| `config` | underlays, overlays, object definitions, texture average colours |
| `model` | v2/v3 (and legacy) model geometry |
| `terrain_mesh` | 11x11 underlay blending, lighting, the 13 overlay tile shapes |
| `scene` | per-object lit/rotated meshes, planar UVs for textured faces |
| `textures` | texture definitions -> paletted sprites -> RGBA images |

## This cache revision differs from what older docs describe
- Map groups are keyed `(x << 8) | y` in archive 5 (no name hashes); files: 0 terrain, 1 locs, 2..4 other.
  Maps are **not** XTEA-encrypted any more.
- Terrain opcodes are **u16** and overlay ids are u16.
- Object definitions: model lists are opcodes 6 (typed) / 7 (untyped) with **u32** model ids;
  opcode 101 is a multi-option menu entry; 90/93/95/96 carry 0/6/1/1 skipped bytes.
- Models are only `ff fe` (v2) / `ff fd` (v3). v3: one type byte per textured triangle comes first
  (before the vertex flags), face render types precede the face-compress array, the header's
  `L` field is the textured-triangle section size, and `extra`=1 adds an `M`-byte section before
  the face indices. Faces with render type bit 1 store a texture id in `color`.
- v3 colour array comes *after* the L-byte textured-triangle section; v2 has it before. In v3,
  faces with render-type bit 1 and no texture are white placeholders (0x007f) and are not drawn.
- Texture images: definition bytes 0..2 are the sprite group (archive 8), 2..4 the average HSL.
  Sprites: paletted, index 0 transparent, trailer holds frame size/offsets (first frame used).
- Texture mapping: v2 faces use `render>>2` as the texture-triangle index; v3 stores one byte per
  textured face (1-based, 0 = own vertices) in the `L` section. Triangles (P,M,N u16) trail the z
  stream, 6 bytes for type 0 and 19 for type 2 (13 uninterpreted bytes). u runs P->M, v runs P->N.
- Texture definitions: bytes 2..4 are the average colour as 16-bit HSL.

## Object placement rules (match the client)
- Orientation 0..3 = quarter turns, `(x,z) -> (z,-x)` per step; types 4/9 etc. keep it axis-aligned.
- Kind 2 (L-shaped corner wall) is two arms: the model mirrored at `o`, plus unmirrored at `o+1`.
- Definition flag `rotated` (opcode 62) mirrors the model (z negated, winding swapped).
- Kind 9 (diagonal wall) is modelled on the diagonal already; only kind 11 (diagonal scenery) gets
  the extra 45 degrees.
- Wall decorations (kinds 4-8) all draw the type-4 model. Kind 4 sits flush; kind 5 is pushed
  inward by the same-tile wall's `decor_displacement` (opcode 28, default 16). Kinds 6/7/8 are the
  diagonal form (model orientation `o+4`: turned 45 degrees, shifted (45,-45), then quarter turns);
  6 and 8 are offset by half the displacement toward `DIAG[o]`, 7 and the second model of 8 use the
  opposite side `(o+2)&3` with no offset.
- Objects on a bridge tile are moved down one plane but stand on the deck: heights are read from
  the original (plane + 1) terrain, not the plane they are drawn on.
- Debug: `find_bridges rx ry` maps bridge deck tiles of a square. `OSRS_LOOK=x,z,dist` aims the viewer camera at a grid tile; `find_locs rx ry kind|255
  [x z radius]` lists placements of a kind (255 = any) optionally near a tile.

## Terrain details that are easy to get wrong
- An overlay coloured `#FF00FF` (magenta) is the map makers' "draw nothing here" marker. It fills the
  upper-plane tiles over a bridge and gaps in rooftops (the most common overlay on planes 1-3). The client skips
  those triangles; painting them as a colour gave pale lilac "sheets" floating over bridges and roofs.
  `overlay_census` counts overlays by colour per plane; `tile_probe` dumps one tile on every plane.
- A bridge deck tile is drawn at ground level from plane 1's heights (and walked on at them); the plane-0
  tile under it is the riverbed.

## Texture scrolling and translucency
- Texture definitions end `[transparent u8][scroll direction u8][scroll speed u8]` (direction 1-4).
  Only ids 17, 24, 34, 40, 56, 57, 59 and 99-115 scroll; the Lumbridge river texture (id 1) is
  static in this cache. The viewer moves the shared material's UV transform
  (`SCROLL_PER_SPEED`, tuned by eye; direction-to-axis mapping is unverified).
- Object faces are back-face culled (`OBJECT_CULL` in the viewer). Some models are double-skinned: the
  same quad twice with opposite winding and mirrored texture coordinates (e.g. object 10459, a
  water tile). Drawing both makes them z-fight and the scroll looks like it flows both ways.
  Terrain meshes are not culled (their triangle winding is not consistent).
- Translucent object faces are re-sorted far-to-near per instance when the camera moves.
  The client does not draw water translucent, so terrain water stays opaque.

## Sound effects

`osrs_cache::sound` decodes archive 4, the synth sound effects (instruments of oscillators shaped by
envelopes, a gate, an echo and a filter), and `SoundEffect::mix` renders them to 8-bit PCM at
22,050 Hz with the client's own integer arithmetic. 12,147 of 12,163 groups decode exactly; the
other 16 (11988 and up) are Vorbis data. Renders match the wiki's recordings (0.98 to 0.998
correlation). `cargo run -p osrs_cache --bin sound_probe 4 2676 pfm.wav` writes one out to listen to.

## Known gaps
- All 62,050 models decode, but the textured-triangle data itself (texture-space coordinates) and
  the v3 'extra' section are skipped, not interpreted; their order among the prefix arrays is a guess.
- Skipped when rendering: object animations/morphs, per-face
  priorities and mipmaps. `OSRS_NO_TEXTURES=1` falls
  back to average colours.
- Planes/roofs/bridges: all 4 planes are meshed; objects on bridge tiles drop one plane and the
  plane-1 deck is drawn on plane 0, as in the client. Roof hiding is plane-based, not per-tile.
- Overlay tile shape rotation direction and lighting constants are matched by eye to the game.

## Game client character
`osrs_core` (the Bevy client binary) draws the default character with the same pipeline as the viewer:
`osrs_cache::kit` decodes identity kits (config group 3: opcode 1 body part, 5 u32 model list, 40/41
recolour/retexture, 70 head model, 3 hidden-in-designer) and `build_player` lights the merged kit models
(ambient 64, contrast 850, light -30,-50,-30); `osrs_render` turns meshes into Bevy entities and is shared
with `osrs_viewer` (`osrs_render::world` loads map squares and spawns terrain/objects for both; the client spawns the
character on tile 3222,3218, Lumbridge square, in a 3x3 block of squares from 49,49; the sim server runs in the
background with no client connected). Default male kits are hair 0, jaw 10, torso 18, arms 26, hands 33, legs 36, feet 42.
Gaps: animations (bind pose only), appearance colour palettes, per-face priorities, female/equipment models.

    cargo run -p osrs_core --release        # right mouse orbits, scroll zooms; OSRS_SCREENSHOT=out.png

## Animations
- `osrs_cache::anim`: sequences (config group 12), frames (archive 0, group = frame set, file = frame) and
  frame bases (archive 1, one file per group). `Clip::bake` poses a model's rig with every frame of a sequence;
  `osrs_render::spawn_animated` + `ModelAnimationPlugin` play it by rewriting vertex positions (lighting stays
  baked from the bind pose, like the client).
- Sequence ops in this revision: a leading one-byte op 16; ops 13+ are sound tables (variable entries, skipped
  by stopping there); frames are `[u16 base][u8 slots][slot flags][smart operands]`; frame bases end with two
  extra zero bytes. ~127 bases (and the sequences using them, op 14) are a float-based skeletal system and
  are not decoded.
- Transforms mirror the client: type 0 sets the origin (mean of the label group + operand), 1 translates,
  2 rotates (`(v & 255) * 8` of 2048, order z, x, y; the z turn is `x' = x*cos + y*sin, y' = y*cos - x*sin`,
  the opposite sense to the x and y turns - verified by eye: the other sign crosses the legs), 3 scales about the origin, 5 (alpha) is ignored.
- The player plays sequence 808 idle, 819 walk, 824 run; the client picks by whether the character is still
  advancing to the server tile (with a short linger so it does not flicker to idle between ticks) and the
  snapshot `is_running`. Walking covers one tile per tick, running two. `OSRS_WALK_RUN=0` with `OSRS_WALK_TO`
  walks. `OSRS_ORBIT=yaw,pitch,dist` aims the client camera for screenshots.
- Not done: walk/run/combat selection (needs equipment + weapon stances), animation interleave masks,
  per-face priorities and face alpha frames.

## Client and mock server
- The client sends `ClientCommand::Login { name }`; the server assigns a `PlayerId` (counter from 1, never
  reused, distinct from the world's `EntityId` for the player), prints `[server] player "name" logged in: ...`
  and reports the id in every later `Snapshot::player_id`. The client's `Player` component holds the id once
  confirmed and walks the character to the snapshot tile. The world has one player slot, so a second login on
  the connection is rejected.
- The mock server's open floor is centred on Lumbridge square (`dev_open_floor_world_at`) and has no real
  collision, so it will walk through walls. `OSRS_WALK_TO=x,z` and `OSRS_NAME` drive it from the client until
  click-to-walk exists.

## Click to walk
- `osrs_render::pick`: camera ray -> ground tile (terrain corner heights, bilinear) or an interactive object
  in front of it (triangle test against objects that have menu actions; plain scenery has no click box).
- Client (`osrs_core/src/ground_click.rs`): left-click ground sends `WalkTo`; right-click (not a drag) opens a
  one-entry "Walk here" menu that sends the same request for the tile under the cursor when it opened;
  clicking away dismisses it. Clicks on interactive objects do nothing yet (logged).
- Debug: `OSRS_INPUT="t:move:x,y;t:press:left|right;t:release:..."` scripts mouse input on a timeline.

## Collision
- `osrs_cache::collision::CollisionGrid` builds plane-0 walkability the way the client does: terrain flag bit 1
  blocks (`FLOOR`; tiles under a bridge deck use the deck's flags instead), walls 0-3 set edge/corner bits on
  both tiles, scenery 9-21 sets `OBJECT` over its footprint when the def's clip type (opcode 17, default 2) is
  non-zero, and floor decorations (22) block only when opcode 19 is 1. Bits match `osrs_core::collision::
  CollisionFlag`, so the client hands the grid straight to the server's `CollisionMap`.
- `osrs_render::world::load_world` builds it into `WorldData::collision`; the client's mock server walks on it
  (`osrs_core/tests/lumbridge_collision.rs` checks no walking onto blocked tiles or through wall edges).
- Doors are baked as placed (closed ones block); the server moves a door's wall when it opens, with the same
  table (`wall_edges`). Not modelled: multi-plane movement, projectile-blocking flags, and
  clicks on a blocked tile simply path to the nearest reachable one.

## Map streaming
- The client no longer loads a fixed block. `osrs_cache::squares::MapSource` decodes one 64x64 square together
  with its eight neighbours (colour blending is 11x11, lighting reads adjacent heights, and scenery spills over
  borders), then keeps only the middle: `TerrainGrid::build_mesh_window` for terrain, `load_objects` with a window
  for scenery, `Neighbourhood::collision` for flags. Tests check a square decoded alone equals the same tiles of a
  5x5 block, for both collision and terrain triangles.
- `osrs_render::streaming`: `SquareLoader` (decoder, thread-local because models are `Rc`), `SquareStream` (the
  loading thread), `StreamedWorld` (loaded squares, ground heights; implements `GroundSurface` for picking).
  Render space is the world's own: tile (x, z) is at Bevy (x, _, -z), so squares never need re-basing.
- `osrs_core/src/map_stream.rs` loads a 3x3 block around the player and drops squares more than 2 away.
- The server does not receive chunks: its own `Server::set_map_streamer` hook merges the same squares into its
  `CollisionMap` (`CollisionMap::merge` grows the bounds; unloaded ground is solid) around the player's tile.
  Limits: the collision map only grows (never unloads) and it is a bounding rectangle, so a very long trip
  costs memory; only plane 0 is streamed for heights and picking.
- `OSRS_SPAWN=x,z` enters the world on another tile (e.g. `3150,3235` then `OSRS_WALK_TO=3100,3245` crosses into
  Draynor and loads squares live).

## NPCs
- `osrs_cache::npc::NpcDef` decodes config group 9. Format notes for this revision: model ids are u32 in opcode
  61 (opcode 62 is the chat-head list; classic 1 is a u16 list), 95 combat level, 74-79 base levels, 114/115 run
  animations, 13/14/17 stand/walk and the three turned walks. About 1.3k of 16.6k definitions still stop at an
  opcode that is not decoded (122, 147, 124, 130...); `npc_probe` lists them. The man (3106, level 2) is clean.
- `build_npc` lights like the player (ambient 64, contrast 850 + the definition's `*5`) and keeps the rig, so
  `Clip::bake` works on it with the stand and walk sequences.
- The server's monster data (`npcs.json`) names the cache definition to draw from with `appearance`, and
  `wander_radius` makes an idle monster stroll: about 1 tick in 12 it picks a tile within the radius of its
  spawn and walks there, one tile a tick, from its own RNG stream (`sim/wander.rs`). A fight overrides it.
- Snapshots list each entity's `step_tiles` and `appearance`; `osrs_core/src/npcs.rs` draws them the way the
  player is drawn (`route.rs`: tile centres, one tick behind the server). NPCs have stand and walk only; no
  turn-on-the-spot, side-steps or running yet, and they are not clickable.

## NPC options and combat
- NPCs are clickable: a box around the model (as wide as it is at its widest) is tested along the click, and an
  NPC nearer than the ground and than any intercepting object takes the click. Left-click does its first option;
  right-click lists the cache's options in order ("Talk-to", "Attack", "Pickpocket"), "Examine", then any object
  options, then "Walk here". NPC names are yellow and its level is coloured by comparison with the player's
  combat level (green far below, yellow equal, red far above).
- "Attack" sends `ClientCommand::Attack`; the server walks the player up and fights (combat log lines arrive as
  messages), and a dead NPC is no longer drawn. Any other option sends `ClientCommand::InteractNpc`: the server
  walks the player up to the NPC (it does nothing more yet) and keeps `face_target` set until a walk click
  replaces it. While `PlayerSnapshot::face_target` is set the client turns the body toward that NPC however it
  is moving, so approaching it at an angle plays the sideways/backwards walk clips. "Examine" asks the server
  (`ExamineNpc`), which answers with the definition's `examine` text or a plain description from the name.
- The menu is as wide as its longest line (`menu_width`); the UI font is monospaced.
- The sim records `WorldEvent`s (`Attack`, `Hit` with damage, `Died`) as it resolves a tick, and snapshots
  carry them. `DamageBuffer::advance` reports what landed and now keeps 0-damage hits so blocks show a splat.
- `osrs_core/src/combat_fx.rs` turns events into: an attack clip for the attacker (player punch 422, NPC
  `animations.attack`), a block clip (424) for whoever was hit and not mid-swing, a turn toward the opponent, a
  splat over the target, a health bar for both fighters for 6 s after they last fought,
  and the death clip, held; an NPC is taken away 2 s after its death animation. `AnimatedModel::play_once`
  plays a clip once and holds its last frame.
- Hit splats are the cache's: config group 32 (`osrs_cache::hitsplat`). Type 16 (your damage) and 12 (your
  block) are pointers (opcode 18, chosen by a client setting) to definitions 28 and 26, whose middle sprites
  are 1359 (red) and 1358 (blue); `%1` is the amount and the duration is 50 client cycles (1 s).
  `hitsplat_probe` prints the definitions and writes the sprites to a PNG.
- The number is drawn in the cache's `p11_full` font (`osrs_cache::font`): the sprites archive has a group of
  that name with one frame per character, the fonts archive (13) a file of the same name whose first 256 bytes
  are each character's advance. Frames sit at an offset inside a shared canvas, so text is each glyph at
  `pen + x_offset`, advancing the pen by the metric; a black shadow is the same text one pixel down and right.
- The health bar is the cache's (`osrs_cache::healthbar`, config group 33, type 0): back sprite 2177 (red)
  drawn whole, front sprite 2176 (green) cut off at `width * hp / max` whole pixels (a UI `ImageNode::rect`).
  Opcode 7/8 are the front/back sprites, 14 the width (30 when absent), 5 the cycles it stays (300 = 6 s).
  Larger monsters use other types (wider bars); every NPC uses type 0 for now.
- A new account (`PlayerConfig::new_account`) has level 1 in every skill, 10 hitpoints and 1 prayer point; the
  live client logs in as one. Hits come from the existing derived-max-hit and accuracy rolls, so a level 1
  hits a man for 0 or 1.
- A death is confirmed a tick after the hit that took the last hitpoint: the sim lists it in `World::dying`, and
  the next tick emits `Died` and reports `is_dead` in the snapshots. Health shows 0 in between and the
  death animation waits for the confirmation.
- NPC combat animations are data (`animations` in `npcs.json`); the cache has none.
- Not done: an NPC that comes back after dying, hit/attack sounds, ranged and magic effects, drawing the
  player's death/respawn, and hiding upper planes when the camera is inside a building.

## Interface, inventory and equipment
- Server: `Player::inventory` is 28 slots of `ItemStack` (ammunition stacks; everything else is one per slot).
  `ClientCommand::InventoryOption { slot, option }` does "Wield"/"Wear" (puts the worn item back in that slot),
  "Eat"/"Drink" (the existing consumable rules, one used up), "Drop" (removed; there are no ground items yet) and
  "Use" (nothing yet); `MoveInventoryItem` swaps slots; `Unequip` now moves the item into the inventory (or says
  there is no room); `ExamineItem` answers with a plain description. Snapshots carry `inventory` and `equipped`
  with names and quantities.
- Item definitions are config group 10 (`osrs_cache::item`). This revision stores the inventory model as an int
  (opcode 44), worn models as int + byte (45, 48) or int (46-54), weight (75) and category (94) as shorts, and has
  flag opcodes 15, 65, 160 and 251 with no data. Opcode 97 is the linked noted/unnoted id and 98 marks a noted
  copy; opcodes 100-109 are *item ids* whose pictures show a pile of that many. `item_census` lists any opcode
  the decoder does not know (a few late ones, 70/82/83/87/116/117, only ever come after the name and options).
- Inventory pictures (`osrs_cache::item_icon`) are drawn on the CPU from the lit model: roll, yaw, then the
  camera offset `(xoff, zoom*sin(pitch) + height/2 + yoff, zoom*cos(pitch) + yoff)` *before* the pitch rotation
  (which turns it into a pure distance of `zoom`), projected with focal length 512 about (16, 16) into 36 x 32,
  back faces culled, z-buffered; then a black outline and a 0x302020 shadow one pixel down-right. Names map to
  ids by lowest non-noted id with that name (`ids_by_name`).
- Interface (`osrs_core/src/interface.rs`): the fixed 765 x 503 layout drawn at the largest whole-number scale
  that fits the window height, hung from the bottom (chatbox left, minimap/tabs/panel right). Cache sprites: 1031
  panel, 1032 tab rows, 1033-1035 pillars, 1017/1018 chatbox, 1026 the open tab's red stone, 898-910 / 168 / 776
  tab icons, 156-166 empty equipment slots, 12 and 169 minimap ring and compass. All fourteen tabs open a pane;
  inventory and worn equipment work, the rest show their name. Left-click on an item does its first cache option;
  right-click lists the cache's options, "Use", "Drop"/its slot-4 option and "Examine" (item names orange).
  Stack sizes are drawn in the cache font, yellow below 100K, white in K, green in M.
- Worn items on the character: this revision's worn-model opcodes are 45/46/47 (male, int, 45 followed by an
  offset byte) and 48/49/50 (female); opcode 13 is the body slot (0 head, 1 cape, 2 amulet, 3 weapon, 4 torso,
  5 shield, 6 arms, 7 legs, 8 hair, 9 hands, 10 feet, 11 jaw) and 14/27 the slots it covers. `build_player`
  fills slot by slot with the worn item's models, else the kit's, skipping covered slots, recolouring each
  group with its own table. `osrs_core/src/player_look.rs` asks a worker thread to rebuild the character (and
  bake its clips) whenever the worn set changes and swaps it in with `osrs_render::replace_animated`.
- Chat carries only what the player should read (rejections, what an item did, examine text). Combat results
  are events (hit splats), and the developer combat log stays on the server (`World::log`).
- Projectiles: `WorldEvent::Projectile { from, to, graphic, ticks }` is sent when the player looses ammunition;
  the graphic is by ammo tier (`projectile_graphic`: bronze 10, iron 9, steel 11, mithril 12, adamant 13, rune 15,
  dragon 1120, bolts 27). Config group 13 (`osrs_cache::spotanim`) gives the model (opcode 3 int) and tier
  recolours; the arrow's head is its +z end, so the client looks away from the flight direction. It flies from
  chest height to the target's over the travel ticks with a slight arc (`osrs_core/src/projectiles.rs`).
- Nocking and release: the draw graphic (19 for bronze arrows, by tier) is a model with its own sequence (366,
  41 client ticks). It is drawn as a child of the archer, 100 units up ("height 100") and eased back 24 units onto
  the bow's grip (the animation holds the head well ahead of it), and the projectile is let go when it has
  played. The arrow leaves from the nocked arrow's place at chest height. The server lands a short shot's hit a
  tick after the attack, before the draw ends, so `projectiles::Arrivals` makes the client hold that hit (splat,
  block, death) until the arrow gets there.
- NPCs do not block movement unless their definition sets `blocks_movement` (the player walks through them);
  `Actor::solid` keeps them out of the occupancy overlay. A greedy (chase) step now reports its step tile, so the
  client walks a chasing NPC instead of snapping it from tile to tile.
- Known gaps: no minimap contents, the minimap and chat buttons do nothing, textured item models draw flat,
  no drag-and-drop yet in the client (the server has `MoveInventoryItem`), no ground items for Drop.

## Drawing movement
- The server moves the player tile by tile and reports `PlayerSnapshot::step_tiles` (the tiles stepped
  through that tick). The client runs through those tile centres in order, one tick behind the server,
  so what is drawn is exactly the game path: a straight leg then a diagonal leg is correct, and corners are
  never cut across scenery. (An any-angle smoothing was tried and removed: it drifted off the server path.)

## Facing and walk animations (from the old client)
Read from the 2004 deobfuscated client (`Pazaz/RS2-194`, `client.java`, `method85`/`method86`); the current
client may differ, but the structure matches the pose-animation set RuneLite exposes:
- Facing target is the compass direction (8 steps of 256/2048) of the next path tile.
- The secondary animation is picked from `target - current facing` (2048 units, positive = to his right):
  within +-256 forward walk (819), run (824) replaces it only there; 256..768 right (822); -768..-256 left (821);
  otherwise backwards (820). Standing while the facing catches up plays turn-on-the-spot (823).
- That client turned at a constant 32/2048 per 20 ms cycle (281 degrees a second, 0.64 s for 180) and
  halved its step size while the facing had not yet reached the target. Observed in the live game: clicking
  back and forth once per tick, the model finishes the turn and gets all the way onto the tile, walking or
  running. So here the turn is eased (gentle start and finish, `TURN_ACCELERATION`/`TURN_JERK`) but sized to
  finish a 180 inside a tick (`MAX_TURN_SPEED` 9 rad/s, ~0.55 s), and movement is never slowed for a turn
  (the old half-pace rule left him short of the tile). The numbers are tuned to that observation.

## Interfaces (archive 3)
- `osrs_cache::interface` decodes every component: group = interface, file = component, full id `group << 16 | file`.
  26,559 components in 973 groups (24,463 IF3 starting `0xff`, 2,096 legacy IF1); `if_sweep` and
  `tests/interface.rs` check that each decode consumes exactly all its bytes: 0 failures.
- The IF3 layout is RuneLite's `decodeIf3` (type, content u16, x/y i16, width u16, height u16 (i16 for lines),
  four mode bytes, parent u16 (0xffff = none, else same group), hidden, per-type block, 24-bit click mask, name,
  ops, drag bytes, target verb, 18 listeners, 3 trigger arrays) with model ids as u32 (RuneLite rev237 too). The
  one difference: type 6 (model) components with a non-zero width or height mode carry **two** u16 after the
  unknown u16, not one per non-zero mode. Seen combinations: modes (1,0) x4, (1,1) x58, (2,1) x1 all take two;
  (0,0) takes none; (0,1) never occurs, so that case is a guess.
- Listeners are `[count]{kind u8: 0 = i32, 1 = string}`; the first argument is the script id (negative ints such
  as -2147483645 are event-parameter placeholders). IF1 layout is RuneLite's `decodeIf1` unchanged.
- `if_dump <group>` prints the tree (parents before children, file order); `if_dump --list`, `--sprites <group>`.

## Minimap
- `osrs_cache::minimap::render_square(cache, sx, sz)` (or `MinimapSource` to reuse the decoded definitions) draws one
  square as 256 x 256 RGBA, 4 px per tile, north up (pixel `(px, py)` is tile `x = sx*64 + px/4`, `z = sz*64 + 63 - py/4`),
  plus `icons`: map function icons for the caller to draw over it (area definitions are config group 35; opcode 1 is the
  sprite group, 3 the name; object def opcode 82 names the area, opcode 68 the `mapscene` sprite frame).
- Tile colour: the 11x11 blended underlay at fixed lightness (`l * 96 / 128`, no height shading); an overlay uses its
  secondary colour (opcode 7) if it has one, else its colour or texture average. Overlay shapes use the 3D view's triangles
  sampled at the 16 pixel centres (overlay wins on shared edges). Magenta overlays draw nothing, so bridge decks over water
  (docks) leave transparent pixels. Walls are 1 px lines (0xEEEEEE, 0xEE0000 when interactive), kind 2 two edges, kinds 1/3
  a corner pixel, kind 9 a diagonal (the diagonal's direction is from memory, unverified). Map scenes are blitted last, centred on the footprint.
- `minimap_png <x> <z> out.png [scale] [noicons]` renders the 3x3 squares around a tile; `area_probe [group]` dumps raw config files.

## Client scripts (archive 12)
- `osrs_cache::clientscript` decodes CS2 scripts (9,972 of 9,973 groups exactly; group 0 is a 2-byte stub) and
  config enums; `cs2_dump <id>...` disassembles (see docs/08-cache-decoding.md). Format = RuneLite's rev237
  `ScriptLoader`: 16-byte trailer (u32 instruction count, then u16 local int/obj/long and arg int/obj/long
  counts), u32 operands below opcode 100 (u8 for `return`/`pop_int`/`pop_object`/`push_null`), u8 from 100 up.
  No format changes beyond rev237 were found; ~380 opcodes (mostly 1xxx/2xxx/3xxx/4xxx additions) are not in
  RuneLite's name table. Item params (opcode 249) are now kept in `ItemDef::params`.
