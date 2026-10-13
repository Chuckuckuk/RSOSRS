# 5. Movement: collision, pathfinding, actors

Folders: [`collision/`](../osrs_core/src/collision/), [`pathfinding/`](../osrs_core/src/pathfinding/),
[`actors/`](../osrs_core/src/actors/).

OSRS movement has a particular feel: paths are a straight leg followed by a diagonal leg, you
cannot cut corners past walls, NPCs block you but other players do not, and a chasing NPC trails
a step behind. This chapter explains how each of those comes out of the code.

---

## 5.1 Collision flags

File: [`collision_flag.rs`](../osrs_core/src/collision/collision_flag.rs).

Every tile has a 32-bit set of flags. The values are the same ones the real game client uses,
which is why `osrs_cache` can build a grid from the cache and hand it straight over.

| Flag | Bit | Meaning |
|---|---|---|
| `BLOCK_NORTH_WEST` ... `BLOCK_WEST` | `0x1` .. `0x80` | You cannot leave this tile through that edge or corner. |
| `BLOCK_OBJECT` | `0x100` | Solid scenery stands here. |
| `BLOCK_NPC` | `0x200` | (defined, unused: occupancy is tracked separately) |
| `BLOCK_FLOOR_DECORATION` | `0x40000` | A blocking floor decoration. |
| `BLOCK_FLOOR` | `0x200000` | Unwalkable terrain: water, cliffs. |
| `BLOCK_FULL` | object, floor decoration and floor combined | Nothing may enter. |

A wall between two tiles is recorded **twice**: `BLOCK_EAST` on the western tile and `BLOCK_WEST`
on the eastern one. Either side alone is enough to stop a crossing in that direction, so a wall
blocks both ways.

They are plain `u32` constants on an empty struct rather than a proper flag type, because the
map stores and combines raw `u32`s.

---

## 5.2 `CollisionMap`

File: [`collision_map.rs`](../osrs_core/src/collision/collision_map.rs).

A rectangle of tiles with an origin (world tile of its south-west corner), a width and a height.
It stores four flat arrays, indexed `(z - origin_z) * width + (x - origin_x)`:

| Array | Contents |
|---|---|
| `base_flags: Vec<u32>` | the static flags above |
| `heights: Vec<i16>` | per-tile height for step-height checks (all 0 on real terrain today) |
| `npc_occupancy: Vec<u8>` | 1 where an NPC currently stands |
| `player_occupancy: Vec<u8>` | 1 where a player currently stands |

It is built from a [`CollisionMapData`](../osrs_core/src/collision/collision_map_data.rs) (the
raw arrays plus `max_step_height`), and the constructor checks the array lengths.

### Out of bounds is solid

`flags_at` is the hottest function in the project: the pathfinder calls it up to eight times per
tile it explores. It never panics. A tile outside the map reads as `BLOCK_FULL`. This single rule
means "unloaded ground is a wall", which makes map streaming safe: the server cannot walk the
player onto ground it has not loaded.

Only plane 0 is in bounds. Any tile on another plane reads as blocked.

### Occupancy and who respects whom

[`OccupancyMask`](../osrs_core/src/collision/occupancy_mask.rs) is a small bit set: `NPC`,
`PLAYER`, `BLOCKS_PLAYERS`, plus `ALL` (`NPC | PLAYER`) and `NONE`. Movers write their own kind
over their **whole footprint** and check the kinds they *respect*:

| Mover | Writes | Respects (is blocked by) |
|---|---|---|
| player | `PLAYER` | `BLOCKS_PLAYERS` only: players walk through ordinary NPCs and other players |
| NPC | `NPC` (and `BLOCKS_PLAYERS` if its definition sets `blocks_movement`) | `ALL`: other NPCs and players |

So **NPCs never walk through one another**, as if each were a wall, while the player walks through
a man in Lumbridge, or under Jad, as in the game. Only a monster whose data sets `blocks_movement`
stands in the player's way too. These pairings come from the actor's kind and that one flag
(`Actor::respects`, `Actor::occupies_as`), so they can never be mismatched.

Each overlay **counts** the movers on a tile rather than storing a yes/no, so two NPCs that
overlap (spawned on one tile) do not erase each other's mark when one walks off. A dead NPC takes
its mark off the map (`Mover::leave_map`): a body blocks nobody.

### `can_traverse(from, dir, size, respects)`: the movement rule

This is the function the source calls "the single most bug-prone function in the project". It
answers: can a mover whose footprint is `size × size` tiles, anchored at its south-west tile, take
one step from `from` in direction `dir`?

**Straight steps** (N, S, E, W):

1. For every row (for an east/west move) or column (for north/south) of the footprint, look at the
   tile on the leading edge and the tile just beyond it. The leading tile must not have the exit
   flag (`BLOCK_EAST` when moving east), the tile beyond must not have the entry flag
   (`BLOCK_WEST`), both must be in bounds, and their height difference must be at most
   `max_step_height`.
2. Every tile of the footprint at the new position must be free of `BLOCK_FULL` flags and of
   respected occupancy.

**Diagonal steps** (e.g. NE):

1. **No corner cutting.** Both straight components (N and E) must be legal *from the same starting
   position*. This is why you cannot slip diagonally past the corner of a wall even if the
   diagonal tile itself is open.
2. The whole destination footprint must be enterable (as above).
3. Explicit diagonal flags (rare) are respected on both tiles.

### `merge`: growing the map as squares stream in

`merge(data)` adds a block of tiles to the map. If the block lies outside the current bounds, the
map is reallocated to the bounding rectangle of both, copying the old arrays row by row into
their new positions; newly covered tiles that no block has supplied start as `BLOCK_FULL`. Then
the block's flags and heights overwrite the tiles it covers. Occupancy is preserved.

The server calls this every tick (through its map streamer, chapter 6) for any square near the
player it has not loaded yet. The map only grows; nothing is unloaded.

### Test grids

[`test_grid.rs`](../osrs_core/src/collision/test_grid.rs) (tests only) parses ASCII art into a
map: `.` open, `#` solid, `~` no floor, `|` a wall on the east edge, `_` a wall on the north
edge, digits for heights. The top text row is the north edge. Most movement tests are written as
small pictures this way.

---

## 5.3 Saying where to go: `TargetSpec`

File: [`target_spec.rs`](../osrs_core/src/pathfinding/target_spec.rs).

A path search does not always aim at one tile. `TargetSpec` describes what "arrived" means:

| `TargetKind` | Satisfied when the mover's tile... | Used for |
|---|---|---|
| `Exact` | is that tile | ground clicks, wandering |
| `Rect` | is inside a `w × h` rectangle | |
| `Adjacent` | touches the rectangle orthogonally, and is not inside it, and not diagonal to a corner | melee range, "Talk-to", healers |
| `WithinRange` | is within Chebyshev `range` of a point | |
| `WithinRangeOfRect` | is within Chebyshev `range` of the rectangle and not inside it | ranged and magic attacks on a large NPC |

`distance_to_rect(tile, sw, w, h)` is the Chebyshev distance from a tile to the nearest tile of a
rectangle (0 inside it). The player's attack code uses it to decide the projectile's travel time.

Only `Exact` targets get the unreachable fallback described next.

---

## 5.4 `BfsPathfinder`

File: [`bfs_pathfinder.rs`](../osrs_core/src/pathfinding/bfs_pathfinder.rs).

### Why breadth-first search, not A*

In OSRS every step costs one tick, diagonal or not. Breadth-first search explores tiles in rings
of equal step count, and the first time it reaches a tile is via a shortest path. A* with a
distance heuristic would also find a shortest path, but it would often pick a *different* one of
the many equally short paths, and the result would not look like the game. BFS with a fixed
neighbour order (`EXPANSION_ORDER`: W, E, S, N, SW, SE, NW, NE) reproduces the game's path
shapes.

### The search window

The search is limited to a 128 × 128 window centred on the mover (`WINDOW_RADIUS = 64`). All the
bookkeeping is preallocated once, as flat arrays of 16,384 cells, and reused:

| Array | Python equivalent | Purpose |
|---|---|---|
| `visit_stamp: Vec<i32>` | `visited` set | A cell is visited this search if its stamp equals the search's `generation` number. Incrementing `generation` "clears" the whole set in O(1). |
| `distances: Vec<u16>` | `dist` dict | steps from the start |
| `came_from: Vec<Direction>` | `parent` dict | the direction used to arrive (breadcrumb) |
| `queue: Vec<usize>` + head/count | `collections.deque` | a ring buffer of cell indices |

A cell is marked visited when it is **enqueued**, not when it is dequeued. That guarantees each
cell enters the queue at most once, so the ring buffer can never overflow.

### `find_path(map, start, target, size, respects, &mut out_path) -> bool`

```
if start already satisfies target: success, empty path
mark start, enqueue
while queue not empty:
    cur = dequeue
    for dir in EXPANSION_ORDER:
        if not map.can_traverse(cur, dir, size, respects): continue
        nxt = cur + dir; skip if outside window or already visited
        mark nxt, record distance and came_from, enqueue
        if target satisfied by nxt: reconstruct and return success
if target is Exact: try the fallback
return failure (no movement)
```

Path reconstruction walks the breadcrumbs from the found cell back to the start, then reverses
the list. The start tile is not included in the result.

### The unreachable fallback

If you click a tile that cannot be reached (inside a building, across a river), the game still
walks you somewhere sensible. For `Exact` targets only, the pathfinder then looks at every
**visited** cell within 10 tiles of the destination and picks the one closest to it (straight-line
distance squared), breaking ties by fewer steps. If the destination is outside the window, it is
clamped to the window's edge first, which produces the game's "walk part of the way, click again"
behaviour for far clicks. The radius of 10 is a guess and is marked as tunable.

### Greedy single steps

`try_step_towards(map, from, target, size, respects)` and `try_step_away(...)` do not search.
They try the one direct direction (possibly diagonal) toward or away from a tile, then the x axis
(east/west) alone, then the z axis (north/south) alone, and return the first legal one or `None`.
One exception: a mover **diagonally adjacent** to its target (any tile of its body touching the
target only at a corner) tries its east/west step and nothing else. If that is blocked it stays
put, even when the north/south step is open. This is OSRS's "dumb" NPC
pathing: monsters "always move directly toward their target and do not detour around obstacles"
(wiki, *Safespot*). The direction is taken from the mover's whole footprint, so a 5×5 Jad with
the target level with one of his rows steps straight toward it.

Because nothing ever detours, the game's well-known exploits fall out of these rules on their own,
and tests in `actors/mover.rs` and `sim/scenarios.rs` pin each one:

* **Corner safespot.** A melee monster that ends up diagonal to the player with its east/west step
  blocked can never move again: it does not try north/south, even if that tile is free. Melee needs
  a side, not a corner (`TargetSpec::adjacent` is orthogonal), so it never attacks, while the
  player can shoot it.
* **Corner trap.** With the player in a corner only two sides are open. The first two melee
  monsters take them; the others walk up diagonally and stop there, their east/west step blocked
  by another monster (NPCs block one another and never path round: the Colosseum "corner trap").
* **Snagged on a corner.** A large monster whose way is blocked along one of its rows or columns
  stops dead, even when a single sidestep would get it round ("larger monsters, such as dragons,
  may also end up snagged on a corner").

NPCs chasing this way also "trail one step behind" a moving player.

---

## 5.5 `PathFollower`

File: [`path_follower.rs`](../osrs_core/src/pathfinding/path_follower.rs).

It holds one path (a `Vec<TileCoord>`) and an index into it. `repath` runs a new search and
replaces the path. `advance(steps)` consumes up to `steps` tiles (1 walking, 2 running) and
returns how many it took and where it ended. It also remembers exactly which tiles were stepped
through (`last_consumed`), because the client draws movement through those tiles (chapter 7).
`note_step` records a greedy step the same way, so chasing NPCs report their steps too.
`tile_after(steps)` predicts the tile without moving, which the player's attack code uses to swing
on the same tick it steps into range.

---

## 5.6 `Actor` and `Mover`

Files: [`actor.rs`](../osrs_core/src/actors/actor.rs), [`mover.rs`](../osrs_core/src/actors/mover.rs),
[`health.rs`](../osrs_core/src/actors/health.rs),
[`combat_target_view.rs`](../osrs_core/src/actors/combat_target_view.rs),
[`entity_id.rs`](../osrs_core/src/actors/entity_id.rs).

* **`Actor`** is "something that occupies tiles": its kind (`Player` or `Npc`), footprint size,
  `Health`, `CombatStats`, previous and current tile, and (for an NPC) whether it also stands in
  players' way. Keeping the
  previous tile lets a view interpolate between the two.
* **`CombatTargetView`** is a `Copy` snapshot of an actor (tile, footprint, stats, kind) that other
  code can hold while the actor's own buffers are being mutated. It replaces the C# interface
  `ICombatTarget`, splitting its read-only half from its mutable half to satisfy the borrow rules.
* **`EntityId(u32)`** is the handle by which things refer to each other (primer 1.7).
* **`Mover`** wraps an `Actor` and a `PathFollower` and keeps the map's occupancy overlay in step
  with the actor. It never stores the map; every method that touches it takes `&mut CollisionMap`.

| `Mover` method | Effect |
|---|---|
| `new(actor, map)` | marks the start footprint occupied |
| `request_move(pathfinder, map, target)` | BFS from the current tile into the follower |
| `step(map, n)` | consume up to `n` path tiles, each re-checked against the map as it is now (a step another NPC has moved onto stops the actor short and drops the rest of the path, to be planned again); `n = 0` means "hold still" (and collapses previous = current) |
| `step_towards` / `step_away` | one greedy step |
| `set_blocks_players(map, bool)` | for an NPC: start or stop standing in players' way |
| `leave_map(map)` | take the actor's mark off for good (a body) |
| `destination()`, `tile_after_steps(n)`, `last_step_tiles()` | read-only queries |

**A mover never blocks itself.** Whenever it works out a move (planning a path, a greedy step,
re-checking a path step) it lifts its own mark first and puts it back where it ends up. The C#
marked only the south-west tile of a large NPC, so a 2×2 or larger NPC was blocked by its own
marker when stepping south or west; marking the whole footprint without this would have made it
blocked every way. The test `a_multi_tile_mover_occupies_its_whole_footprint_and_never_blocks_itself`
checks both halves.

---

## 5.7 NPC movement policies

File: [`movement_policy.rs`](../osrs_core/src/actors/movement_policy.rs).

```rust
pub trait MovementPolicy {
    fn step(&self, mover: &mut Mover, pathfinder: &BfsPathfinder, map: &mut CollisionMap,
            target: Option<&CombatTargetView>, target_style: AttackStyle);
}
```

Policies are stateless and shared via `Rc`. Each NPC's movement phase calls its policy once.

| Policy | JSON `movement` | Behaviour |
|---|---|---|
| `StationaryMovementPolicy` | `"stationary"` | never moves |
| `ChaseToMeleeRangeMovementPolicy` | `"chase"` (default) | greedy step toward the target until orthogonally adjacent, then holds |
| `MaintainDistanceMovementPolicy` | `{"maintain_distance": {"distance": n}}` | steps away if closer than `n`, toward if farther, else holds |
| `StyleReactiveMovementPolicy` | `{"style_reactive": {"kite_distance": n}}` | chases a player fighting in melee, keeps distance from one using ranged or magic |

When there is no valid target (none assigned, or it is dead), every policy holds still.

On top of the policy sits one base rule for data-defined monsters: **while one of its attacks can
reach the target, a monster holds still and attacks** (`World::run_boss_movement`, using the
pattern's `reaches`). So Jad, whose ranged and magic reach 15 tiles, stands his ground instead of
walking up to the player, while a melee-only monster still chases until adjacent. A policy can opt
out through `MovementPolicy::holds_when_in_reach` (default `true`); the two kiting policies return
`false`, since backing away from a target they could hit is their whole point.
