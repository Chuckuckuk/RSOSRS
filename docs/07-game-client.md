# 7. The game client

Entry point: [`osrs_core/src/main.rs`](../osrs_core/src/main.rs). The client modules live next to
it in `osrs_core/src/` but belong to the binary, not the library (chapter 2.1).

The client has one job: show what the server says, and turn the player's mouse into
`ClientCommand`s. It never decides game outcomes. Read the Bevy section of the primer (1.13)
first if systems and queries are new to you.

```
cargo run -p osrs_core --release
```

---

## 7.1 Start-up

`main()` first reads the command line (`launch`). With `--scenario <name | file.json>` the client
starts in that fight instead of Lumbridge: `load_scenario` finds a bundled scenario (`jad`, ...)
or reads the file, and the player's entry tile becomes the scenario's `location` plus its
`start`. Anything else on the command line, or an unknown scenario, stops the program with a
message. The result is kept as the `Launch` resource.

```
cargo run -p osrs_core --release -- --scenario jad
```

Then it does three things before the window opens.

**1. Decode the opening scene (`load_scene`).** On the main thread, synchronously:

* the 3x3 block of map squares around the spawn tile (`OSRS_SPAWN`, default `(3222, 3218)` in
  Lumbridge), via a `SquareLoader` (chapter 9);
* the left-click option and name of every object definition (for picking and examine);
* every texture image;
* the default male character (`build_player` with no worn items) and its ten animation clips,
  baked against its rig (`PLAYER_CLIPS`: idle 808, walk 819, run 824, walk back/left/right
  820-822, turn on the spot 823, punch 422, block 424, death 836);
* the two hit-splat pictures, the `p11_full` font, and the standard health bar.

Any failure here (most often: no game cache found) prints a message and exits.

**2. Start the server thread** with `spawn_server` (chapter 6.6). The closure it passes runs on
the server thread and:

* opens its own `MapSource` and builds a `CollisionMap` from the 3x3 squares around the spawn, so
  whatever starts near the player stands on known ground;
* **with a scenario**: builds the world with `build_scenario_on` (chapter 6.7) and starts the tick
  driver **paused**. A scenario is a fight that begins at once, and the client needs several
  seconds to load and compile shaders; `start_scenario` (below) lets it go;
* **without one**: creates the `World` with seed 12345 and a new-account player at the spawn
  (with Firemaking raised to 50), spawns a "man" NPC on the nearest open tile to `(spawn.x + 4, spawn.z - 3)`, and gives the
  player a starter kit (bronze sword and helm, whip, magic shortbow, sharks, karambwans, prayer
  potions, 150 bronze arrows);
* tells the server every object's name (for examine);
* installs a **map streamer**: before each tick it merges the collision of any square within one
  square of the player that has not been loaded yet (`CollisionMap::merge`). A square that cannot
  be read is solid. Heights are left at 0, so step height never blocks real terrain.

**3. Build the Bevy `App`:**

```rust
app.add_plugins(DefaultPlugins...)
   .add_plugins((WorldRenderPlugin, ModelAnimationPlugin, GroundClickPlugin, ChatPlugin,
                 CombatFxPlugin, InterfacePlugin, PlayerLookPlugin, ProjectilePlugin, TileMarksPlugin))
   .insert_resource(...)       // Scene, FirstSquares, Textures, ObjectInfo, GroundPicker, GridOverlay...
   .add_systems(Startup, setup)
   .add_systems(Update, (scripted_input, poll_server, start_scenario, sync_player, npcs::sync_npcs,
                         map_stream::stream_map, limit_planes, orbit_camera, toggle_grid,
                         screenshot_and_exit).chain());
app.world_mut().insert_non_send(ServerLink { ... });     // the channel to the server
app.world_mut().insert_non_send(SquareStream::spawn());  // the map-square loading thread
app.world_mut().insert_non_send(npcs::NpcViews::spawn()); // the NPC-model loading thread
```

The `.chain()` makes the core loop run in a fixed order every frame: read snapshots, move the
player, move the NPCs, stream map squares, then the camera.

`start_scenario` does nothing outside a scenario. In one, it waits until every monster in the
server's first snapshot is drawn, then `SCENARIO_GRACE` (3 s) more, and sends
`ClientCommand::SetPaused(false)`; after 30 s it starts the fight regardless, so a monster that
cannot be drawn does not hold it forever.

### `setup` (runs once)

* Sends `Login { name }` (`OSRS_NAME`, default "Player"). The id comes back later in a snapshot.
* Applies debugging environment variables: `OSRS_WALK_TO`, `OSRS_WALK_RUN`, `OSRS_WIELD`,
  `OSRS_ATTACK` (chapter 10).
* Spawns the nine opening squares.
* Spawns the character with `spawn_animated` and attaches the `Player` and `Fighter` components.
* Spawns the orbit camera aimed at the character's middle.

---

### Launch modes

* **Open world** (no arguments): the 3x3 block of map squares around the player, the tile grid
  overlay on (G toggles it).
* **`--scenario <name>`**: a bundled or file scenario. One with a `location` sits on a real place
  (Jad in the Fight Cave) and loads and draws only the square it is in
  (`map_stream::StreamRadius` 0, on the server side too), with the tile grid off (`OSRS_GRID=1`
  turns it on at the start). The server starts paused until the fight is on screen.
* **A packed executable** (it carries its own cache, see chapter 8): no arguments means
  `--scenario jad`, and it never reads the installed game.

## 7.2 Receiving the server's state

`ServerLink` (a non-send resource) holds the `ServerHandle`, the latest folded snapshot, how many
ticks it stands for, and events not yet drawn.

**`poll_server`** drains every snapshot that arrived since the last frame. It moves each one's
events into `new_events` and each message into the chat log (and stdout). Then it **folds** them
with `merge_snapshots`: the newest snapshot is kept, but its `step_tiles` (for the player and each
NPC) become the concatenation of every snapshot's steps, oldest first. Without this, a frame that
took longer than one tick would only see the latest tick's steps, and characters would jump over
the tiles in between. `ticks_in_latest` records how many ticks were folded, so the movement speed
can cover them all.

---

## 7.3 Drawing the player: `sync_player`

The `Player` component (in `main.rs`) is the client's view of the player: the server-assigned id,
the last known server tile, the last tick processed, a **route** of tile centres still to walk,
a speed, the current facing, the heading it is turning toward and a `Turn` controller.

Each frame:

1. Copy health and death into the `Fighter` component (for health bars and the death animation),
   and adopt any heading `combat_fx` requested (turning to face an opponent).
2. On the first snapshot with a `player_id`, record it and print the login line.
3. **On a new tick**, if the server reported step tiles: lay a route through their centres
   ([`route::lay_route`](../osrs_core/src/route.rs)), each with the compass heading of that step,
   and set `speed = route_length / (tick_seconds × ticks_in_latest)`. That speed means the model
   finishes walking this tick's steps exactly when the next tick's arrive: the client draws
   **one tick behind** the server. A tile change without step tiles is a teleport and snaps.
4. **Follow the route** ([`route::follow_route`](../osrs_core/src/route.rs)): advance by
   `speed × frame_time`, popping centres as they are reached, keeping the model on the terrain
   surface between centres. It reports the heading of the current step.
5. **Turn.** The target heading is toward the face target NPC if there is one (so approaching an
   NPC at an angle plays the sideways walk), otherwise the current step's heading.
6. **Choose the animation clip** from how far the body still has to turn, in the game's 2048
   units, following the old client's rules:

   | Situation | Clip |
   |---|---|
   | standing, still turning more than 32 units | turn on the spot (823) |
   | standing | idle (808) |
   | moving, within ±256 of facing the way of travel | walk (819), or run (824) if the server moved 2 tiles |
   | moving, 256 to 768 to the right | walk right (822) |
   | moving, 256 to 768 to the left | walk left (821) |
   | otherwise | walk backwards (820) |

   "Moving" lingers 80 ms after the route empties (`WALK_LINGER`) so the pose does not flicker to
   idle between ticks.
7. If `Fighter::clip` says an attack, block or death is playing, that one-shot clip takes over.

The path is drawn **exactly through the tile centres the server stepped on**. OSRS paths really
are a straight leg and then a diagonal leg, and the client must not smooth them; an earlier
any-angle smoothing drifted off the server's path and was removed.

### The turn controller: `Turn`

`Turn::step(current, target, dt)` eases the facing toward the target with limits on angular
speed (`MAX_TURN_SPEED` 9 rad/s), acceleration and jerk (rate of change of acceleration). Speed
builds in a smooth S-curve, holds at the top speed on long turns, and brakes to land on the target
at rest. It always takes the short way round. The constants are tuned so a 180-degree turn
finishes in about 0.55 s, inside one tick, which matches what the live game was observed to do
when clicking back and forth every tick. The tests at the bottom of `main.rs` pin these
properties.

### The camera

`OrbitCamera` holds a target point, yaw, pitch and distance. `orbit_camera` keeps the target on
the character's middle, rotates while the middle mouse button is held (the right button is the
context menu's), and zooms with the scroll wheel. `OSRS_ORBIT=yaw,pitch,distance` sets the starting view.

---

## 7.4 Drawing NPCs: [`npcs.rs`](../osrs_core/src/npcs.rs)

NPCs are drawn from the cache definition named by the snapshot's `appearance` field.

**`NpcViews`** (non-send resource) owns a worker thread. The thread opens the cache, reads every
NPC definition once, and answers requests `(cache NPC id, combat animation ids, attack animation
ids)` with an `NpcLook`: the lit mesh (`build_npc`), five baked clips (stand, walk, attack, defend,
death; any the NPC lacks fall back to standing), then one clip per attack that has an animation of
its own (`EntitySnapshot::attack_animations`; Jad's ranged and magic), its name, menu options,
combat level and bounding box. Those extra clips are found again by sequence id
(`AnimatedModel::clip_of_sequence`), not by position. The
main thread keeps a `HashMap<u32, LookState>` of `Loading`, `Ready` or `Failed` per definition,
so each definition is decoded once however many NPCs share it.

**`sync_npcs`** each frame:

1. Collect finished looks from the thread.
2. Spawn a model for each reported entity that has an appearance, is not dead, stands on a loaded
   square and has its look ready. A monster bigger than one tile is drawn at the middle of its
   footprint (`footprint_centre`), not on the south-west tile the server reports: Jad is 5x5, so
   his model sits 2.5 tiles north-east of his tile's corner. Its routes use the same centring. Attach an `NpcView` component (its own route, speed, facing and
   turn controller, plus name, options, level and bounds for clicking) and a `Fighter`.
3. Despawn models for entities the server no longer reports.
4. Move each one exactly like the player: route through step tiles one tick behind, `Turn` toward
   the step heading, walk or stand clip, one-shot fight clips from `Fighter`. An NPC with a
   target (`EntitySnapshot::facing`: the player it fights, or the monster it heals) turns toward
   that target instead, every frame, walking or standing, as in the game.
5. A dead NPC is removed 2 s after its death animation ends (waiting at most 6 s for the
   animation: some, Jad's included, hold their last frame for a minute) and remembered in
   `finished` so it is never redrawn.

**`pick_npc(ray_origin, ray_dir, npcs)`** tests the mouse ray against a box around each NPC (as
wide as the model at its widest, so turning does not change what is clickable) and returns the
nearest hit as an `NpcHit`. `NpcHit::options()` lists the cache's real options, dropping blanks,
"Hidden" and "Walk here".

---

## 7.5 Combat effects: [`combat_fx.rs`](../osrs_core/src/combat_fx.rs)

The server reports fights as events. This module turns them into what you see.

**`Fighter`** is a component on the player and every NPC model: the one-shot `Action` it is
playing (clip index and end time), whether it is dead, its health, until when its health bar
shows, and a heading to face. `Fighter::clip(now, death_clip)` tells the sync systems what to
play: the death clip (held forever once started), an attack or block that has not finished, or
`None` (free to walk or stand). It also reports whether this is the first frame of the clip, so
the model is told to start it exactly once.

**`apply_events`** runs after `poll_server` and before the two sync systems. For each event:

| Event | Effect |
|---|---|
| `Attack { attacker, target, animation }` | attacker plays the clip baked from `animation` if it has one (Jad's stomp or rear-up), else its usual attack clip, and turns toward the target; both show health bars for 6 s |
| `Projectile { from, to, graphic, draw, ticks, released }` | queue a `Shot` for the projectile module; a `released` one (after Jad's wind-up) leaves at once instead of partway through the attack animation |
| `Graphic { on, graphic, delay_ticks }` | queue a `Spot`: the graphic plays at the character's feet, moving with it, after the delay, until its own animation ends (Jad's rocks, a spell's splash) |
| `Heal { healer, target, animation, .. }` | the healer turns to its target and plays its healing animation; the target's health bar shows |
| `Hit { target, damage }` | target plays its block clip unless mid-swing; spawn a hit splat (red for damage, blue for 0) |
| `Died { who }` | mark the fighter dead and hide its bar |
| `Sound { sound, from, delay_ticks }` | queue it for `sound_fx` to play after the delay |

**Held events.** For a short bow shot, the server lands the hit before the bow animation would
release the arrow. So if a `Hit` or `Died` event targets an entity that an arrow is still flying
toward (recorded in `projectiles::Arrivals`), the event is held in a `Local` list until the arrow
arrives. Held events for one target keep their order, and a `Sound` straight after a held
`Hit` or `Died` (the target's hit or death sound) is held with it.

**Hit splats** are UI nodes showing the cache's splat sprite with the number drawn in the cache
font (`SplatImages::number` renders and caches one image per amount). `place_splats` projects the
target's chest onto the screen each frame and removes the splat after the cache-defined duration
(1 s).

**Where things sit on a model.** An NPC's `NpcView` keeps the model's bounds, so the health bar
floats `BAR_CLEARANCE` (0.2 tile) above the model's top *as it is posed this frame*
(`AnimatedModel::top`, the highest vertex of the frame being shown, so the bar rises as a
monster rears up and drops as it crouches), hit splats sit at 0.6 of that height,
and a projectile with no draw graphic leaves the monster's head as it is posed when the
projectile leaves (`AnimatedModel::head_front`: the vertices within 1.2 tiles of the top, their
sideways middle and front-most point, 0.6 tile below the top), turned with the model. That follows
Jad's rear-up: his head swings up and back over his body (about 5 tiles up, z 0) as the fireball
goes, where a fixed point would sit at his chest. `NpcView::mouth_at` (0.8 of the height up, 0.9 of
the forward reach out) is only the fallback. `HEAD=1 npc_anim_probe <npc> <sequence>...` prints
the point for every frame. (The player has no `NpcView`: its bar follows its posed model the same way,
and `HEAD_HEIGHT`, 1.75 tiles, is the fallback and where the overhead prayer icon sits. Hit splats
are drawn at 1.44 times the cache sprite size and the overhead icon at 1.44 times its sprite.) One mouth fraction serves every monster; it was tuned on Jad.

**Health bars** are a red back sprite with a green front sprite cut off at
`width × hp / max` whole pixels, using `ImageNode::rect` to crop it. `place_bars` creates one for
any fighter whose `bar_until` is in the future, positions it above the head, and removes it when
the time runs out or the fighter dies.

`pixel_image` converts a cache texture into a Bevy `Image` with nearest-neighbour sampling, so the
pixel art stays crisp when scaled. The interface uses it too.

---

### Sound: [`sound_fx.rs`](../osrs_core/src/sound_fx.rs)

`SoundQueue` collects the sounds `apply_events` hands over, with the entity each comes from and
when it is due. `play_sounds` plays the due ones. The first time a sound id is needed, `SoundBank`
mixes it from the cache (`osrs_cache::sound`) into a `Synth` asset: 8-bit samples at 22,050 Hz.
After that the handle is reused. `Synth` implements Bevy's `Decodable` (registered with
`add_audio_source`), so no audio file format or extra crate is involved. Each sound plays once on
its own despawning `AudioPlayer`. Its volume is `volume_at(tiles)`: 0.5 within a tile of the player,
fading linearly to silence 16 tiles away (Chebyshev distance between the source's model and the
player's).

## 7.6 Projectiles: [`projectiles.rs`](../osrs_core/src/projectiles.rs)

A `Shot` is queued by `apply_events`. `ProjectileArt` (opened at startup, non-send) holds the cache,
every spot-animation definition, and the meshes built so far.

1. **The draw.** If the shot has a draw graphic (the arrow being nocked), it is a model with its
   own animation. It is spawned as a **child of the shooter** (so it moves and turns with them),
   100 units up and eased back 24 units onto the bow's grip, and removed when its animation ends.
   The release point is computed from where the drawn arrow is near the end of that animation.
2. **Release time.** The draw animation's length, or 55 % of the attack animation if there is no
   draw graphic.
3. **Flight.** At release, the projectile model is spawned at the release point (chest height). It
   flies to the target's chest over `max(hit_in - release, 0.25 s)`, following a shallow parabolic
   arc, re-aiming at the target's current position each frame. The model's head is its `+z` end,
   so it is turned to look *away* from the direction of travel.
4. **Arrivals.** The arrival time per target is recorded in `Arrivals` for the held-events logic
   above.

---

## 7.7 Clicking the world: [`ground_click.rs`](../osrs_core/src/ground_click.rs)

**Use.** "Use" on an inventory item (its left click, when the cache's first option is "Use") only sets the `UseSelection` resource: the slot is drawn with a white outline, and the next click decides. Another inventory slot sends `UseItemOnItem`; a ground item sends `UseItemOnGroundItem`; an object (or a fire, which has no click option) sends `UseItemOnObject`; anything else puts the item down (the right-click menu lists `Use A -> B` first and `Cancel` last). Ground items add "Take"/"Light" (the cache's ground options the server handles) to the menu of the tile they lie on, and the left click does the first.


**`GroundPicker`** wraps an `osrs_render::pick::Picker` (chapter 9): click boxes of plane-0
objects, added and removed as squares stream in.

**`pick_at`** casts the camera ray through the cursor and gathers everything under it:

* the ground tile where the ray meets the terrain;
* the nearest object with a real left-click option in front of that ground (it "intercepts" the
  click);
* the nearest named object (examinable, even if it does not intercept);
* the nearest NPC, if nearer than the ground.

**Left-click**, in priority order:

1. If a menu is open, the click belongs to it: perform the entry under the cursor (if any) and
   close it.
2. If the cursor is over the interface (`UiHits::target_at`): a tab selects it (in the modern resizable layout, clicking the open tab closes the side panel); the minimap walks to the tile clicked; the compass turns the camera to face north; an inventory item does its first cache
   option ("Wield", "Eat"...); a worn item is removed.
3. In the world: if an NPC is nearer than any intercepting object and has an option, do its
   default ("Attack" if it has one, else its first). Else if an object intercepts, do its option
   (objects have no interactions yet, so this is logged). Else walk to the tile.

**Modifiers and keys.** Shift-click in the world only walks, to the tile under the cursor,
whatever NPC or object is there. Ctrl-click walks with run and walk swapped for that walk
(`WalkTo { force_inverse: true }`); with Shift it does both. Escape opens the inventory tab, F1
the prayer tab and F2 the Magic tab (`interface::tab_keys`; the spellbook has no contents yet, so
it opens an empty tab).

**Right-click** opens its menu where the button went down, when it comes up. On the interface it lists an item's options plus "Examine". An item's options are its cache definition's, less "Wield"/"Wear"/"Equip"/"Hold" when the snapshot says the server cannot put it on (`ItemView::wearable`: it has no equipment slot in the data, like a vial); the left click is the top remaining option. In
the world it lists the NPC's options (default first) and "Examine", the object's option and its
"Examine", ordered nearest-first, and always ends with "Walk here" to the tile at the end of the
ray. Names are coloured the game's way: objects cyan, NPCs yellow, items orange, and an NPC's
level from green (far below the player's combat level) through yellow (equal) to red (far above).

`MenuAction` enumerates what an entry does; `perform` turns it into a `ClientCommand`:

| Action | Command |
|---|---|
| `Walk(tile)` | `WalkTo { tile }` |
| `Npc { option: "Attack" }` | `Attack { target }` |
| `Npc { other option }` | `InteractNpc { target }` (walk up and face it) |
| `ExamineNpc`, `Examine`, `ExamineItem` | the matching examine command |
| `InventoryOption`, `Unequip` | the matching item command |
| `Object { .. }` | nothing yet (printed) |

The menu itself is a Bevy UI node tree, sized to its longest line (`menu_width`, assuming a
monospace font), clamped inside the window, with the hovered row highlighted by
`highlight_menu_entry`. Like the game's, the menu closes by itself once the cursor strays more
than 10 game pixels outside it (`close_menu_when_left`; `MENU_LEAVE_MARGIN` is that distance in
the menu's own row size) or leaves the window.

---

## 7.8 The interface: [`interface.rs`](../osrs_core/src/interface.rs) and [`panes/`](../osrs_core/src/panes/mod.rs)

The interface is not hand-placed. It is the game's own widget tree from the cache (chapter 8,
`osrs_cache::interface`), drawn by the software renderer `osrs_cache::ui` into one RGBA picture. The
layout is `OSRS_LAYOUT`: **classic** (the default; the resizable layout, group 161, the game's real
layout anchored to the window edges: chatbox bottom left, minimap and orbs top right, the two
rows of stones and the side panel bottom right), **modern** (group 164: one row of stones along the
bottom) or **fixed** (group 548, a 765x503 picture centred in the window).

**Drawing.** `build_interface` (start-up) loads the layout's group and the standard side panes
(`standard_state`), makes the `ClientAssets` (the cache's sprites, fonts and item pictures, plus the
minimap) and spawns one absolutely positioned `ImageNode` (`GlobalZIndex(20)`, below the menu, hit
splats and health bars) that shows the picture. Everything is sized in **physical** pixels so each
picture pixel is whole screen pixels: the scale is `OSRS_UI_SCALE`, else `max(1, floor(min(W/765,
H/503)))`, and the picture is `ceil(window / scale)` pixels (exactly 765x503 in the fixed layout).
Each frame `refresh` builds a `PaneInput` from the latest snapshot, the chat log, the open tab and the
camera, and hashes what it shows (`PaneInput::fingerprint`: window size, tab, inventory, worn items,
prayers, orb values, chat, the minimap's position to a pixel and angle to half a degree, NPC dots).
Only when the hash changed does it run `panes::sync`, draw, and write the pixels into the same `Image`
asset (a new asset only when the size changes). A redraw costs about 3 ms at 960x540 (printed in the
`[ui] draw` log lines), and it happens only while something changes: walking or turning the camera
redraws every frame because the minimap moves.

**Clicks.** The last list of drawn widgets (`Hit`s) is kept in `UiHits`.
`UiHits::target_at(cursor)` finds the topmost interactive widget under the cursor and says what it is:

| Widget | `UiTarget` |
|---|---|
| a tab stone | `Tab(i)` |
| a created child of the inventory (149:0) | `InventorySlot(i)` |
| a worn-slot layer 387:15..25 (each identified by its position in the cache) | `EquipmentSlot(slot)` |
| a prayer cell 541:9.. | `Prayer(i)` |
| a spell icon of the Magic tab (218:4..200) | `Spell(i)` (place in the standard spellbook) |
| the Logout / World Switcher buttons (182:8, 182:3) | `Logout` / `WorldSwitcher` |
| the minimap (inside its round mask) / the compass | `Minimap { x, z }` (the world tile) / `Compass` |
| a combat style button of the Combat Options tab (593:6, 10, 14, 18) | `CombatStyle(slot)` |
| the special attack bar (593:39) / Auto Retaliate (593:32) | `SpecialBar` / `AutoRetaliate` |
| the prayer, run and special attack orbs' click layers (160:20, 28, 36) | `Orb(Prayer / Run / Special)` |
| any other interactive or opaque interface pixel | `Background` |
| the viewport and transparent pixels | none: the world is clicked |

`ground_click` and `tile_marks` rely on this: `Some` means the interface takes the click and the world
does not see it. `Interface { selected, open, layout }` is the open tab. Escape, F1 and F2 open the
inventory, prayer and magic tabs. Only the modern layout closes the side panel when the open tab is
clicked again (the game's scripts do this for the modern and mobile layouts only); the panel and its
pane then hide, the stones stay.

**Switching layouts.** F10 cycles fixed, classic, modern at run time (`interface::switch_layout`; the
game's Settings pane would do this). It rebuilds the `UiState` with `standard_state` for the new layout,
refills the stone map in `UiHits`, opens the panel, adds or removes the fixed layout's UI camera, and the
next frame redraws; `fit_viewport` then cuts the 3D camera to the viewport widget (fixed) or gives it the
window back. `OSRS_WINDOW=WxH` starts the window at an exact size (to see the narrow modern arrangement
or the fixed layout at 1:1).

**Size-dependent layout** (`osrs_cache::ui::arrange`, run before each draw). Modern (164) follows script
920: when the interface width minus the chat holder's 519 is under 429 (width < 948) the 13 stones form
two rows, the tabs 0..6 layer (164:95) 36 px above the other six (164:94) and the panel (164:96) 72 px
above the bottom; otherwise one row (164:95 at 198 from the right) with the panel 36 above
(`modern_arrangement`, a pure function with tests). All three anchor to the bottom right. The static
cache already holds the wide arrangement. The stones in the right layer are in the order the live game
shows (tabs 7, 9, 8, then 11, 12, 13; logout, tab 10, is the X by the minimap). The minimap's mask is
sprite 2154 (resizable) or 2422 (fixed): script 920 sets it, the cache widget still has the old
1178/1183. 2422 is nearly all transparent: in fixed the frame (1182) draws over the map and shapes it.
Script 2396 hides the Store orb (160:43) unless varbit 13037 is set, so `standard_state` hides its
children (it was drawn cut off by the stone row in fixed); the Activity Adviser orb (160:48, script
2480) sits at y 163, below the fixed layout's orb container, so it is clipped away there. The chat
background gets script 923's 1 px line (0x807660) above the typing line next to the parchment.

**The fixed layout's 3D view.** In classic and modern the 3D camera fills the window and the
picture overlays it. In fixed the camera's `viewport` is cut to the viewport widget (content type
1337). Bevy lays UI out inside a camera's viewport, so the fixed layout also spawns a plain `Camera2d`
over the whole window as the default UI camera; the outline gizmos are on their own render layer so
only the 3D camera draws them. A camera with a viewport reports positions from the viewport's corner, so
`ground_click`, `tile_marks` and `combat_fx::screen_point` add or subtract that corner.

**The panes** (`panes/`) are the ports of the game's client scripts (CS2, read with
`cargo run --release -p osrs_cache --bin cs2_dump -- <script id>`). Each writes into `UiState`:
script-created children (`created`, with `created_overrides` to change one) and changes to cache
widgets (`overrides`).

| File | Ports | What it does |
|---|---|---|
| `tabs.rs` | 916, 2646 | only the open tab's pane is shown and its stone drawn red; panel hidden when closed |
| `inventory.rs` | 6007, 6010, 6011 | 28 created 36x32 children of 149:0 at `(16 + i%4*42, 8 + i/4*36)`, each the item picture (with its stack size) or hidden when empty |
| `equipment.rs` | 3281, 3282, 546 | each slot layer 387:15..25 gets the frame (sprite 170), the worn item at (2, 2) and, while empty, the 32x32 silhouette (sprites 156..166) |
| `prayer.rs` | 458, 547, 463 | 29 cells in level order, five to a row 37 px apart: the yellow glow disc (4892) only while the prayer is on, the icon (grey copy when the Prayer level is too low) |
| `orbs.rs` | 449, 446, 82, 447, 2069, 2792 | the value text of the HP, prayer, run and special attack orbs, coloured red through yellow to green, and the fill layer of height `26 * (max - cur) / max`; the status looks: the HP disc green (poison, sprite 1061) or purple (venom, 1102) with the "Cure" button shown, the prayer orb lit (1066, 1058) while any prayer is on, the run discs by walking or running, the special attack orb lit (1608) while switched on and greyed (1064, half transparent, not clickable) without a special weapon |
| `skills.rs` | 2366, 393, 395, 396 | the 24 cells of the Skills tab: frame halves (187, 188), the skill's icon (enum 255), current level at (32, 4) and base level at (44, 16) in yellow font 494, and "Total level: N" |
| `combat.rs` | 7592, 7593, 2486, 324, 325, 187, 189 | the Combat Options tab: weapon name and "Combat Lvl", one button per attack style, the selected one framed red, Auto Retaliate, the special attack bar |
| `magic.rs` | 2262, 2611, 2614, 9448, 2603 | the standard spellbook of the Magic tab: the 69 spells of enum 1982 in a 7 by 10 grid of 24 px icons, lit or greyed by Magic level, the selected spell outlined, the "Filters" button |
| `logout.rs` | 2154, 2243, 2244 | the Logout tab: the pane's layout (text, World Switcher, "Click here to logout") |
| `emotes.rs` | 699, 31 | the Emotes tab: the 56 icons of enum 1001, four to a row, and the scroll bar |
| `music.rs` | 250, 9290, 318, 31 | the Music tab: the track list of database table 44 (all locked, red), its scroll bar, the four mode buttons |
| `chat.rs` | 923 | the newest messages, wrapped, in the lines 162:60.. (font 495, black), newest at the bottom; the "Name: *" typing line 162:58 |
| `minimap.rs` | content 1338/1339 | below |

The cache's own decoration needs no code: the stones, the orbs' frames, the chat tab buttons and the
parchment (script 925's background, sprite 1017, added by `standard_state`) are static widgets. The
side tabs that have no port (quests, settings, account, friends, chat channel) show what the cache's
static widgets hold, which is mostly frames, headings and buttons without their words.

**The Magic tab** (`panes/magic.rs`). The cache has one hidden 24x24 sprite widget per spell
(218:4..200); script 2611 walks enum 1982 (the standard spellbook as a list of "spell items") and, for
each, shows the widget the item's param 596 names, positions it in the grid, and gives it the item's
icon (param 597) or, when the Magic level is below the item's level (param 604), the grey one
(param 598). The grid is the script's arithmetic (`magic::grid`): for 69 spells, 7 columns by 10
rows, 26 px apart across and 24 down, centred in the 184 by 240 pane. Spells are named by the
cache; the server's ids are the names in lower case with underscores (`Spell::server_id`; a test
checks every standard spell of `spells.json` is found, with the same level). Differences from the
game: spells are lit by level only (no rune check, the server has no runes), the hover box
(script 2623) is not drawn, the Filters button does nothing, and the selected spell is outlined
white (the game draws that outline for the spell you picked to cast).

*Clicking.* The spell widgets have a target verb ("Cast") and no ops, so `Hit` now has a `targetable`
flag that makes them take clicks. A spell the server knows (every standard combat spell in
`spells.json`) selects it as the spell the wielded staff casts: `ClientCommand::SelectSpell`, the same
as autocast. The game's "Cast", then click the NPC flow is **not** implemented; selecting is
persistent and the next attack with a casting weapon casts it. Clicking the selected spell again
clears it (menu entry "Deselect"). The menu entry is "Cast" and the spell's name in green. Any other
spell (teleports, alchemy...) answers with a chat line saying it is not available; a level that is too
low is refused by the server with its own message.

**The Logout, Emotes and Music tabs** (`panes/logout.rs`, `emotes.rs`, `music.rs`) draw what their load
scripts build and nothing that needs game state. Logout: "Click here to logout" in the red button
(the button words are content-typed text components, which `ui::draw` now draws like other text);
clicking the Logout or World Switcher button says in the chat box that it is not available. Emotes:
every emote drawn unlocked, no click action. Music: every track listed, locked (red), no playing
track, no scrolling. `panes::scrollbar` builds script 31's scroll bar (track, thumb, two arrows) for
the emote and music panes; it is drawn at the top and cannot be dragged.

**The minimap and compass** (`panes/minimap.rs`). The picture of a map square
(`osrs_cache::minimap`, 4 pixels a tile) is drawn on a worker thread the first time the player is
within 22 tiles of it (a frame never waits), and kept. When the interface is drawn, the minimap
component (a 152x152 widget) is filled pixel by pixel: for each pixel inside the mask sprite (1178: it
is *transparent* inside the round window and opaque outside) the world position it stands for, after
turning by the camera yaw around the player's exact (sub-tile) position, is looked up in the square
pictures; where there are none the pixel is black. Map icons (`MapIcon`) go on at their tiles, NPC
dots (`mapdots` frame 1, yellow) at theirs, and the player (frame 2, white) in the middle, all
upright. The compass is the `compass` sprite turned clockwise by the yaw inside its mask (1179).
With the camera at yaw `y` the screen's up is the world direction `(-sin y, cos y)` (x east, z north).
Clicking the minimap walks to the tile under the cursor; clicking the compass sets the camera's yaw
to 0 (facing north).

**The Skills tab** (`panes/skills.rs`). Script 393 builds every cell from two numbers: the cell's key
and the stat it stands for (enum 681), whose icon is enum 255. The icon sits 25x25 at `(3 + nudge, 4)`
inside a 62x30 cell, with the frame in two 36 px halves behind it and the two levels drawn over: the
*current* level above left of the cell's diagonal and the *base* level below right (the game's
"85/85" look). `PlayerSnapshot.skills` has every skill, in the game's stat order, so a cell's
stat number is the `Skill` index; the total is the sum of all 24 base levels. The snapshot carries
each skill's XP, but the hover tooltip and "Toggle XP" are not drawn yet.

**The Combat Options tab** (`panes/combat.rs`). The attack styles of each weapon type are in the
cache's database table 78 (`osrs_cache::dbtable` reads config group 38; `bin/db_dump` lists a
table): per weapon type up to four `(slot, name, tooltip, icon sprite)` entries, the slot being what
the game's `varp 43` holds for that style (a whip has slots 0, 1 and 3). Script 7593 lays the
buttons out two by two, 71x47 each at x = 20 and 99, y = 45 and 99, in the order of the slots that
exist; staves and wands use a column of 71x32 buttons and add two spell buttons (593:22..31).
Each button is a frame (script 835's grey sprites 1141..1149, or 836's red 1150..1158 for the chosen
style) with the icon and the name inside. Below come Auto Retaliate (the server has no such setting,
so it is drawn "On" and does nothing) and, only when the weapon has a special, the 146 px bar
filled to the special energy, its text yellow while the special is switched on (text colour
`0x000010` otherwise), its fill green while the energy covers the special's cost.

The server and the game disagree on how weapons are sorted, so the client maps between them: the
snapshot says the wielded weapon's `weapon_category`, its `stances` (names in `SetStance` order),
`stance_index` and `special_cost`; `combat::weapon_type` turns the category into the game's weapon
type and `combat::assign` matches the server's styles to the type's slots by name, then by place (the
game's staff "Focus" is the server's "Block"; a powered staff has an Accurate button the server does not
have, which does nothing). `stance_for_slot` is the reverse a click needs.

**Clicks on these.** A style button sends `SetStance { index }`; the special attack bar and the
special orb send `SetSpecial` (only with a special weapon); the run orb sends `SetRunning`. The
right-click menu is the widget's own op ("Toggle Run", "Use" and an orange "Special Attack", the
style's name). The prayer orb takes the click and does nothing (quick prayers are not in the game).

**The prayer book.** `PRAYER_BOOK` lists the 29 standard prayers in the game's order, each with the
server id it toggles and its icon and greyed icon (cache item params 1757 and 1756). `PrayerLevels`
holds each prayer's level from the bundled `Registry`, or `None` for the two the server does not have
yet (Rapid Restore, Protect Item), which are greyed. Left-click sends `ClientCommand::TogglePrayer`;
right-click offers "Activate" or "Deactivate". The server decides: switching on a prayer turns off any
it shares a group with, and a level that is too low is refused with a chat message.

**Item pictures.** The pane ports name the cache item (`ItemIcons::item_named`: the server names items
by its own ids and names; the client finds the cache item by lower-case name, trying `name(4)` and
`name(1)` for potions) and the renderer draws its picture the way the game does
(`osrs_cache::item_icon`, chapter 8) the first time it needs it, caching it. A stack of a few uses
the cache's "pile of N" model. Stack sizes are written in the cache font: yellow below 100,000, white
"K" below 10 million, green "M" above.

**Gaps (TODO).** Orb states (quick prayers, disease, stamina, special attack weapons' own counters), the
skills' XP tooltips, Auto Retaliate as a real setting, the spell highlight of the staff buttons beyond
the two frames, the prayer pane's Info and Filters buttons, the other tabs' contents, scrolling the chat, colours for
non-game messages, hover highlights, and the transparent chatbox of the resizable layout (the
parchment is used, as the game does with its default settings).

---

### The overhead prayer icon: [`overheads.rs`](../osrs_core/src/overheads.rs)

While a prayer with an overhead icon is on (the three protection prayers, Retribution, Smite,
Redemption), its picture from the cache sprite `headicons_prayer` is drawn above the player's
health bar. `OVERHEAD_FRAMES` maps each prayer to its frame of that sprite; the frames were read
from a contact sheet (`FRAMES=1 sprite_sheet`).

---

## 7.9 Other client modules

| Module | What it does |
|---|---|
| [`map_stream.rs`](../osrs_core/src/map_stream.rs) | Keeps the 3x3 block of squares around the player loaded (`RADIUS = 1`). Requests missing squares from the loading thread, nearest first; spawns **at most one** arrived square per frame (spawning is expensive and must not stall frames); despawns squares more than 2 away (one ring of slack so walking along a border does not thrash). `SquareSpawner` is a `SystemParam` bundling everything needed to spawn a square, its click boxes and its tile-grid overlay. |
| [`player_look.rs`](../osrs_core/src/player_look.rs) | When the snapshot's worn items, the attack/block animation ids or the action animation change, sends a `LookKey` to a worker thread that rebuilds the character with the worn items' models and re-bakes all eleven clips (using the weapon's attack and block, and the last action sequence the server named). While the player is at an action, the hands hold what that sequence says (`Sequence::held_items`: a chop holds its axe and empties the weapon hand), as the game draws it. Only the newest request is built. `osrs_render::replace_animated` swaps the new mesh into the existing entity, keeping its position and current clip. |
| [`scenery_swaps.rs`](../osrs_core/src/scenery_swaps.rs) | Diffs `Snapshot::scenery` against what is on screen. For a change, hides the original's entities (found by their `PlacedLoc` tag) and click box, and spawns the replacement at the same spot. Its mesh is built once on the loading thread (`SquareStream::request_object`) and attached to the square so it leaves with it. When the server stops listing the change, the replacement goes and the original comes back. A square that comes back in has its changes applied again. |
| [`ground_scene.rs`](../osrs_core/src/ground_scene.rs) | The same diff for things the map does not have: fires (`Snapshot::spawned`, drawn from their object mesh) and ground items (`Snapshot::ground_items`, drawn from the item's inventory model via `SquareStream::request_item`). Both are one-object scenes attached to their square. An object with a cache animation (a fire's flames, sequence 475) is built with its rig kept (`build_loc_with`), baked into a `Clip` on the loading thread (`Loaded::Object::clip`) and spawned with `spawn_animated`, so it loops. Ground items have no click box: `ground_click` finds them by the tile a click ends on. |
| [`route.rs`](../osrs_core/src/route.rs) | `lay_route` and `follow_route`, shared by player and NPCs (7.3). |
| [`chat.rs`](../osrs_core/src/chat.rs) | `ChatLog` keeps the last 100 messages; the interface draws the newest lines inside the chatbox (`panes::chat`). |
| [`panes/`](../osrs_core/src/panes/mod.rs) | The ports of the game's interface scripts (7.8): one file per pane. |
| [`overheads.rs`](../osrs_core/src/overheads.rs) | The prayer icon over the player's head (cache sprite `headicons_prayer`, drawn at 1.44 times its size, above `HEAD_HEIGHT`), chosen by `overhead_frame` from the active prayers. |
| [`sound_fx.rs`](../osrs_core/src/sound_fx.rs) | Plays the server's sound effects (7.5). |
| [`tile_marks.rs`](../osrs_core/src/tile_marks.rs) | Gizmo outlines: dark blue on the player's server tile, light blue on the tile under the cursor (unless the cursor is over the interface). |

`limit_planes` hides terrain and objects above `OSRS_MAX_PLANE`. `toggle_grid` shows or hides the
tile grid with **G**. `screenshot_and_exit` saves a screenshot after `OSRS_SCREENSHOT_DELAY`
seconds of wall-clock time and quits; it waits on time rather than frames because Bevy compiles
render pipelines asynchronously and early frames are blank.

`scripted_input` replays a timed script of mouse moves and button presses from `OSRS_INPUT`. It is
marked `TODO(remove)`: a temporary debug hook that injects input into the live window, not to be
used unless asked.
