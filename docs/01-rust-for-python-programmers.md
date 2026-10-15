# 1. Rust (and Bevy) for Python programmers

This chapter is not a general Rust tutorial. It covers the language features this repository
actually uses, with examples taken from its files, and maps each one onto something you already
know from Python. When a later chapter says "see the primer", it means a section here.

---

## 1.1 Building and running: Cargo is pip + venv + pytest + make

| Python | Rust (Cargo) |
|---|---|
| `pyproject.toml` | `Cargo.toml` |
| a package on PyPI | a **crate** on crates.io |
| a monorepo of packages | a **workspace** (the root [`Cargo.toml`](../Cargo.toml) lists `osrs_core`, `osrs_cache`, `osrs_render`, `osrs_viewer`) |
| `python -m mypkg` | `cargo run -p osrs_core` |
| a console script | a **binary** target: `src/main.rs`, or any file in `src/bin/` |
| `pytest` | `cargo test` (compiles with tests included, then runs them) |
| lockfile | `Cargo.lock` (committed, pins every dependency version) |

A crate can be both a library and binaries at once. `osrs_core` is exactly that:
[`src/lib.rs`](../osrs_core/src/lib.rs) is the library (the simulation), and
[`src/main.rs`](../osrs_core/src/main.rs) is a program that uses it (the game client).
[`src/bin/simulate.rs`](../osrs_core/src/bin/simulate.rs) is a second program. Inside `main.rs`,
the library is imported as `osrs_core::...`, just as an external user would.

Rust compiles ahead of time. `cargo build` produces an executable in `target/`. The workspace
`Cargo.toml` sets `opt-level = 1` for our code and `opt-level = 3` for dependencies in debug
builds, because Bevy is unusably slow without optimisation. Add `--release` for full optimisation.

---

## 1.2 Modules, visibility and imports

Python makes a module from every `.py` file automatically. Rust only compiles files you declare.

```rust
// osrs_core/src/lib.rs
pub mod actors;      // compile src/actors/mod.rs (or src/actors.rs) as the module `actors`
pub mod combat;
```

A folder module has a `mod.rs` that plays the role of `__init__.py`. Most `mod.rs` files in this
repo do two things: declare private submodules, then **re-export** the useful names so callers
get a flat namespace:

```rust
// osrs_core/src/tiles/mod.rs
mod direction;            // private: outsiders cannot write tiles::direction::Direction
mod tile_coord;
pub use direction::{Direction, EXPANSION_ORDER};   // ...but they can write tiles::Direction
pub use tile_coord::TileCoord;
```

That is the same idea as `from .direction import Direction` in an `__init__.py`.

Visibility is enforced by the compiler, not by convention like Python's leading underscore:

| Rust | Meaning | Python analogy |
|---|---|---|
| (nothing) | private to this module and its children | `_name`, but actually enforced |
| `pub` | visible to anyone who can see the module | a public name |
| `pub(crate)` | visible anywhere in this crate, not to other crates | "internal" |

Imports use `use`. `crate::` means "from the root of this crate", `super::` means "the parent
module", `self::` means "this module". `use crate::tiles::TileCoord;` is like
`from mypkg.tiles import TileCoord`.

---

## 1.3 Types you will see everywhere

### Integers have explicit sizes

`i32` is a signed 32-bit integer, `u32` unsigned, `u8` a byte, `i64` 64-bit, `usize` the
machine word used for indexing. Python's `int` never overflows; Rust's do. In debug builds an
overflow panics; code that *wants* wrap-around says so with `wrapping_add`, `wrapping_mul`
and friends. You will see those in the .NET RNG port ([`rng.rs`](../osrs_core/src/rng.rs)) and
the cache's string hash ([`osrs_cache/src/lib.rs`](../osrs_cache/src/lib.rs)), where the
original algorithms rely on 32-bit wrap-around.

### Integer division: the single most important difference

This trips up every Python programmer:

| Expression | Python | Rust |
|---|---|---|
| `-7 / 2` (integers) | `-4` with `//` (floor) | `-3` with `/` (**truncates toward zero**) |
| `-7 % 2` | `1` | `-1` |
| floor division | `a // b` | `a.div_euclid(b)` |
| floor modulo | `a % b` | `a.rem_euclid(b)` |

OSRS formulas floor. That is why [`rules/formulas.rs`](../osrs_core/src/rules/formulas.rs) and
[`rules/ratio.rs`](../osrs_core/src/rules/ratio.rs) use `div_euclid` wherever a value could be
negative, and why map-square maths uses `x.div_euclid(64)` (see
[`osrs_render/src/streaming.rs`](../osrs_render/src/streaming.rs)). Plain `/` appears where both
operands are known to be non-negative, where the two agree.

### Casting

`x as f32`, `n as usize` and so on are explicit conversions. Python converts silently; Rust never
mixes numeric types without `as`. `as` from float to int truncates toward zero and saturates
rather than overflowing.

### Strings: `String` vs `&str`

`String` is an owned, growable string (like a Python `str` you are holding). `&str` is a borrowed
view into some string's bytes (read-only, no copy). Functions usually *take* `&str` and *store*
`String`. `.to_owned()` or `.to_string()` turns a `&str` into a `String`; `format!(...)` is an
f-string that produces a `String`.

### Collections

| Rust | Python |
|---|---|
| `Vec<T>` | `list` (one element type) |
| `[T; N]` | a fixed-length array, e.g. `[i32; 7]` (stack allocated) |
| `&[T]` | a borrowed view of part or all of a `Vec`/array, a "slice" |
| `HashMap<K, V>` | `dict` (unordered) |
| `BTreeMap<K, V>` | `dict` kept **sorted by key**; used where iteration order must be deterministic, e.g. [`data::Registry`](../osrs_core/src/data/mod.rs) |
| `HashSet<T>` | `set` |
| `VecDeque<T>` | `collections.deque` |
| `(A, B)` | `tuple` |

---

## 1.4 Structs, `impl` blocks and `derive`

A `struct` is a class with only data. Methods live in a separate `impl` block:

```rust
// osrs_core/src/actors/health.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Health {
    max: i32,          // private field: only this module can touch it
    current: i32,
}

impl Health {
    pub fn new(max: i32) -> Self {            // an associated function: like @classmethod / __init__
        assert!(max > 0, "max health must be positive, was {max}");
        Self { max, current: max }
    }
    pub fn damage(&mut self, amount: i32) {   // a method that mutates: takes `&mut self`
        if amount <= 0 { return; }
        self.current = (self.current - amount).max(0);
    }
    pub fn is_dead(&self) -> bool {           // a method that only reads: takes `&self`
        self.current <= 0
    }
}
```

* There is no `__init__`; `new` is just a naming convention for a constructor function.
* `Self` means "this type".
* The last expression of a block, written without a semicolon, is its return value.
* `#[derive(...)]` asks the compiler to generate standard behaviour, much like `@dataclass`
  generates `__eq__` and `__repr__`:

| derive | gives you | Python analogy |
|---|---|---|
| `Debug` | `{:?}` formatting | `__repr__` |
| `Clone` | `.clone()`, an explicit deep-ish copy | `copy.deepcopy` (explicit) |
| `Copy` | values are copied implicitly on assignment (only for small plain data) | how `int` behaves |
| `PartialEq`, `Eq` | `==` | `__eq__` |
| `Hash` | usable as a `HashMap` key | `__hash__` |
| `PartialOrd`, `Ord` | `<`, sorting, `BTreeMap` keys | `__lt__` etc. |
| `Default` | `T::default()` | a no-argument constructor with defaults |
| `Serialize`, `Deserialize` | JSON (de)serialisation via serde | pydantic models |
| `Component`, `Resource` | Bevy ECS registration (see 1.13) | |

Types like `TileCoord`, `CombatStats` and `EquipmentBonuses` derive `Copy`, so they are passed
around by value freely, like Python numbers.

### Builder methods: `with_*`

A pattern you will see constantly is a constructor plus chained `with_*` methods that take
`mut self` and return `Self`:

```rust
// osrs_core/src/sim/npc_factory.rs
AttackDefinition::new(def.name.clone(), def.range, def.telegraph_ticks, def.style)
    .with_damage_type(...)
    .with_max_hit(MaxHitSource::Fixed(def.max_hit))
    .with_attack_speed(def.speed)
```

Rust has no keyword or default arguments, so this is how optional settings are expressed. It
reads like Python keyword arguments in a chain.

### Struct update syntax

`Self { target, ratio: Ratio::new(num, den), ..Self::default() }` builds a struct by naming some
fields and taking the rest from another value. It is like `dataclasses.replace(default, ...)`.

---

## 1.5 Enums are tagged unions, and `match` is exhaustive

A Rust `enum` is much more than Python's `Enum`. Each variant can carry its own data:

```rust
// osrs_core/src/combat/max_hit_source.rs
pub enum MaxHitSource {
    Derived,                                            // no data
    Fixed(i32),                                         // one value
    Spell { base: i32 },                                // named fields
    PoweredStaff { divisor: i32, offset: i32, gear_bonus_scale: i32 },
}
```

The Python equivalent is a family of small dataclasses plus `isinstance` checks, or
`match`/`case` in Python 3.10+. In Rust you take an enum apart with `match`:

```rust
let base = match attack.max_hit {
    MaxHitSource::Fixed(value) => value,
    MaxHitSource::Derived => { /* compute from levels */ }
    MaxHitSource::Spell { base } => magic_scaled(base, ...),
    MaxHitSource::PoweredStaff { divisor, offset, gear_bonus_scale } => { ... }
};
```

The compiler **refuses to build** if a `match` forgets a variant. This is why the code can add a
variant to `WorldEvent` or `ClientCommand` and be told about every place that must handle it.
`_ => ...` is the catch-all, like `case _:`.

`matches!(x, A | B)` is a shorthand that returns a bool. `if let Some(x) = value { ... }` is a
one-armed match.

### `Option<T>` replaces `None` checks

There is no `null`. A value that may be missing has type `Option<T>`, which is an enum with two
variants: `Some(value)` and `None`. You cannot use the value without first handling `None`, so
the classic `AttributeError: 'NoneType' object has no attribute` cannot happen.

Common helpers, all of which you will see:

| Rust | Python equivalent |
|---|---|
| `opt.unwrap_or(d)` | `x if x is not None else d` |
| `opt.map(f)` | `f(x) if x is not None else None` |
| `opt.and_then(f)` | same, where `f` itself returns an Option |
| `opt.is_some_and(\|x\| cond)` | `x is not None and cond(x)` |
| `opt.is_none_or(\|x\| cond)` | `x is None or cond(x)` |
| `opt.filter(\|x\| cond)` | `x if x is not None and cond(x) else None` |
| `a.or(b)` | `a if a is not None else b` |
| `opt.take()` | read the value and leave `None` behind |
| `cond.then(\|\| v)` / `cond.then_some(v)` | `v if cond else None` |
| `opt.expect("msg")` / `unwrap()` | assert it is not None (panic otherwise) |

### `let ... else`: early return on a failed match

```rust
let Some(snapshot) = &server.latest else { return };
```

Read it as: "if `server.latest` is `None`, return now; otherwise bind its contents to `snapshot`".
It replaces the Python pattern `if x is None: return`. It appears in almost every Bevy system.

---

## 1.6 Errors: `Result<T, E>` and the `?` operator

Rust has no exceptions for ordinary failures. A function that can fail returns `Result<T, E>`,
an enum of `Ok(value)` or `Err(error)`.

```rust
// osrs_core/src/sim/loadout.rs
pub fn equip(&mut self, registry: &Registry, item_id: &str) -> Result<(), String> {
    let item = registry.item(item_id).ok_or_else(|| format!("unknown item '{item_id}'"))?;
    self.equipped.insert(item.slot, item.id.clone());
    Ok(())
}
```

* `ok_or_else` turns an `Option` into a `Result`.
* The trailing `?` means "if this is `Err`, return that error from the current function right now;
  otherwise unwrap the `Ok` value". It is the explicit, visible version of letting an exception
  propagate.
* `Result<(), String>` means "nothing on success, a message on failure". The simulation uses
  plain `String` errors for rule rejections ("Piety needs level 70 Prayer"), and the server sends
  those back to the player as chat messages.

`osrs_cache` defines its own error enum ([`lib.rs`](../osrs_cache/src/lib.rs)) with `Io`, `Format`
and `Missing` variants and an alias `type Result<T> = std::result::Result<T, Error>`.
`impl From<std::io::Error> for Error` lets `?` convert I/O errors automatically.

**Panics** are Rust's version of an uncaught exception that is not meant to be caught. The code
uses `assert!`, `expect` and `unwrap` for *programmer errors* (a monster defined with 0 hitpoints,
an attack with `hit_count` 0), never for bad input from the player. This matches the C# original,
which threw `ArgumentException` for the same cases.

---

## 1.7 Ownership and borrowing: the thing Python hides from you

In Python every variable is a reference to a heap object, and the garbage collector decides when
objects die. In Rust every value has exactly **one owner**. When the owner goes out of scope, the
value is freed. You can lend a value out in two ways:

| Syntax | Name | Rule |
|---|---|---|
| `&T` | shared borrow | any number at once, read-only |
| `&mut T` | exclusive borrow | exactly one at a time, and no `&T` alongside it |

The compiler checks these rules at compile time (the "borrow checker"). The payoff is no data
races and no use-after-free. The cost is that some designs that are natural in Python are
rejected, and the code is shaped around that.

### How this shapes the simulation

In the C# original, objects held references to each other: an attack held its attacker, a mover
held the collision map. Under Rust's rules, if the `World` owns the collision map and an NPC also
holds a mutable reference to it, nobody else can touch the map. So the Rust port follows one rule,
recorded in [`PARITY_PLAN.md`](../osrs_core/PARITY_PLAN.md): **the `World` owns everything, and
everything else refers to other things by ID or receives what it needs as an argument.**

```rust
// osrs_core/src/actors/mover.rs: the map is passed in, not stored
pub fn step(&mut self, map: &mut CollisionMap, steps_per_tick: i32) { ... }

// osrs_core/src/combat/pending_attack_queue.rs: the attacker is an ID, and the caller
// supplies a function that can answer "is this ID dead?"
attacker: Option<EntityId>,
pub fn advance(&mut self, ..., attacker_is_dead: &dyn Fn(EntityId) -> bool) { ... }
```

### The "take it out of its slot" trick

[`sim/world.rs`](../osrs_core/src/sim/world.rs) stores NPCs as `Vec<Option<Box<Entity>>>`. To
run one NPC's turn, it *removes* the NPC from its slot (`take()` leaves `None` behind), runs the
turn with `&mut self` (the whole world) and `&mut entity` as two separate things, then puts the
NPC back. Without this, "mutate this NPC" and "mutate the world that contains this NPC" would be
two overlapping `&mut` borrows, which the compiler forbids. The same trick appears as
`std::mem::take(&mut self.scheduler)` in `World::run_tick`.

`std::mem::take(&mut x)` replaces `x` with its default (empty) value and returns the old value.
It is the standard way to move something out of a struct you only have `&mut` access to. You will
see it used on message buffers too (`take_messages`, `take_events`): "give me everything and
leave an empty list".

### Lifetimes: the `'a` in signatures

```rust
pub fn weapon_item<'a>(&self, registry: &'a Registry) -> Option<&'a ItemDef>
```

`'a` is a name for "how long a borrow lives". This signature promises that the returned reference
points into `registry`, so it is valid as long as `registry` is. You do not need to reason about
lifetimes to read this code: treat `'a` as a label tying an output reference to the input it came
from. Bevy's `SystemParam` structs carry `'w, 's` lifetimes for the same reason.

### `clone()` is explicit copying

Because of single ownership, giving a second part of the program its own copy of a `String` or
`Vec` needs `.clone()`. Python shares the same object instead. When you see `.clone()`, read it as
"make an independent copy here".

### `Cow`: borrow if you can, copy if you must

`Cow<'a, T>` ("clone on write") is either a borrow or an owned value.
[`player_runtime.rs`](../osrs_core/src/sim/player_runtime.rs) uses `Cow<WeaponDef>` so that the
wielded weapon is borrowed from the registry (no copy), while bare hands are a freshly built
`WeaponDef` owned on the spot. Both look the same to the caller.

---

## 1.8 Shared ownership: `Box`, `Rc`, `Arc`, `RefCell`, `Mutex`

Sometimes one owner is not enough.

| Type | What it is | When this code uses it |
|---|---|---|
| `Box<T>` | a single-owner pointer to a heap value | to store trait objects (`Box<dyn Fn(...)>`) and large entities |
| `Rc<T>` | reference-counted shared pointer, **single thread only** | `Rc<AttackDefinition>` shared by a pattern, a state machine and a queue; `Rc<Registry>` shared by the world; `Rc<Model>` in decoders |
| `Arc<T>` | the thread-safe version of `Rc` | `Arc<ObjectScene>` sent from the loading thread to the render thread |
| `RefCell<T>` | lets you mutate through a shared pointer, checked at runtime | only in one test ([`walk_to_draynor.rs`](../osrs_core/tests/walk_to_draynor.rs)) |
| `Mutex<T>` | a lock | `osrs_cache::Cache` locks its open file handle |

`Rc::clone(&x)` does **not** copy the data; it bumps a reference count, exactly like assigning a
Python object to a second variable. The code writes `Rc::clone(&x)` rather than `x.clone()` to
make that cheapness obvious.

Python objects behave like `Rc<RefCell<T>>` all the time. Rust makes you opt in, and the parity
plan deliberately keeps `Rc<RefCell<..>>` out of the core logic.

### `Send`: why some things must be built on a specific thread

`Rc` is not thread-safe, so any type containing an `Rc` cannot be moved to another thread (it is
"not `Send`"). This has visible consequences:

* `World` contains `Rc`s, so it cannot be built on the client thread and handed to the server
  thread. Instead [`spawn_server`](../osrs_core/src/sim/server.rs) takes a *closure that builds*
  the server and runs that closure on the new thread.
* `SquareLoader` keeps decoded models in `Rc`s, so each loading thread opens its own.
* Bevy requires ordinary resources to be shareable between threads; anything that is not goes in a
  "non-send resource" instead (see 1.13).

---

## 1.9 Traits: Rust's interfaces

A `trait` is a set of methods a type promises to implement. It is like a Python
`typing.Protocol` or an abstract base class, except that the compiler checks it.

```rust
// osrs_core/src/combat/attack_pattern.rs
pub trait AttackPattern {
    fn select_next(&self, context: &AttackSelectionContext, rng: &mut dyn Rng) -> Rc<AttackDefinition>;
    fn engages(&self, _self_tile: TileCoord, _target_tile: TileCoord) -> bool { true }  // default method
}

impl AttackPattern for FixedRotationPattern { fn select_next(...) { ... } }
impl AttackPattern for WeightedRandomPattern { fn select_next(...) { ... } }
```

There are two ways to use a trait:

* **Generics** (`fn f<T: Trait>(x: T)` or `impl Trait`): the compiler generates a specialised copy
  per concrete type. Fast, but each call site has one fixed type.
* **Trait objects** (`dyn Trait`, always behind a pointer such as `&dyn Rng`, `Box<dyn Fn>` or
  `Rc<dyn AttackPattern>`): one piece of code works with any implementing type, chosen at
  runtime. This is how Python's duck typing feels.

This codebase mostly uses trait objects for behaviour that varies per NPC (`Rc<dyn
AttackPattern>`, `Rc<dyn MovementPolicy>`) and for the RNG (`&mut dyn Rng`, so tests can pass a
scripted fake).

Traits you will meet from the standard library:

| Trait | What implementing it gives | Python analogy |
|---|---|---|
| `Display` | `format!("{}", x)` | `__str__` |
| `From<A> for B` | `B::from(a)` and `a.into()` | an alternate constructor |
| `Add<Direction> for TileCoord` | the `+` operator | `__add__` |
| `BitOr`, `BitAnd` | `\|` and `&` | `__or__`, `__and__` |
| `Iterator` | `for` loops and `.map/.filter/...` | `__iter__`/`__next__` |
| `Default` | `T::default()` | |
| `std::error::Error` | can be used as an error type | subclassing `Exception` |

---

## 1.10 Closures and the `Fn` traits

A closure is Python's `lambda`, but it may span many lines and capture variables:

```rust
let level = |skill| self.skills.current(skill).max(1);
```

The type of a closure depends on what it does with what it captures:

| Trait | The closure... | Example in this code |
|---|---|---|
| `Fn` | only reads its captures; can be called many times | weight functions in `WeightedRandomPattern` |
| `FnMut` | mutates its captures; can be called many times | the tick `dispatch` callback, the map streamer |
| `FnOnce` | consumes its captures; can be called once | `SpawnBlueprint`, the `build` closure passed to `spawn_server` |

`move |...| ...` makes the closure take ownership of what it captures, which is required when
the closure is sent to another thread.

`+ 'static` on a closure type (e.g. `impl Fn() + 'static`) means "this closure does not borrow
anything short-lived", which is required if it is going to be stored.

---

## 1.11 Iterators: list comprehensions as method chains

Python comprehensions become chains of iterator adapters, finished by a consumer such as
`collect()`, `sum()`, `any()`, `find()` or `max()`:

```rust
// Python: [i for i in range(len(attacks)) if reachable(i)]
(0..self.attacks.len()).filter(|&i| reachable(i)).collect::<Vec<_>>()

// Python: sum(p.drain_effect for p in prayers if p is not None)
self.active_prayers.iter().filter_map(|id| registry.prayer(id)).map(|p| p.drain_effect).sum::<i32>()
```

| Rust adapter | Python |
|---|---|
| `.iter()` | iterate by reference |
| `.into_iter()` | iterate, consuming the collection |
| `.iter_mut()` | iterate with permission to modify each element |
| `.map(f)` | `map(f, ...)` / comprehension expression |
| `.filter(p)` | comprehension `if` |
| `.filter_map(f)` | map then drop `None`s |
| `.flat_map(f)` / `.flatten()` | nested comprehension |
| `.enumerate()` | `enumerate` |
| `.zip(other)` | `zip` |
| `.rev()` | `reversed` |
| `.position(p)` | index of the first match |
| `.find(p)` | first match, as an `Option` |
| `.any(p)` / `.all(p)` | `any` / `all` |
| `.fold(init, f)` | `functools.reduce` |
| `.collect()` | `list(...)`, `dict(...)`, `set(...)`: the target type decides |
| `retain(p)` / `retain_mut(p)` | in-place `xs[:] = [x for x in xs if p(x)]` |

Iterators are lazy like Python generators: nothing runs until a consumer pulls values.

Ranges: `0..n` is `range(n)`, `0..=n` is `range(n + 1)`.

---

## 1.12 Other syntax worth recognising

* **`const`** values are compile-time constants. `const fn` can run at compile time.
* **`#[cfg(test)] mod tests { ... }`** at the bottom of a file holds that file's unit tests; it is
  compiled only by `cargo test`. `#[test]` marks a test function, `#[should_panic]` asserts it
  panics. Tests in a file can reach that file's private items, which Python tests cannot.
* **`include_str!("../../data/items.json")`** embeds a file's contents into the binary at compile
  time. [`Registry::builtin`](../osrs_core/src/data/mod.rs) uses it so the bundled JSON always
  ships with the program.
* **Macros** end in `!`: `println!`, `format!`, `vec![...]`, `assert_eq!`, `matches!`. They
  expand to code at compile time. `eprintln!` prints to stderr.
* **Shadowing**: `let name = name.trim();` declares a new variable with the same name. It is legal
  and common.
* **`impl Trait` in argument position** (`fn f(x: impl Into<String>)`) accepts anything that
  implements the trait. `name: impl Into<String>` lets callers pass `"text"` or a `String`.
* **Labelled blocks and `loop`**: `loop { ... break; }` is `while True`.
* **`#[allow(clippy::too_many_arguments)]`** silences a lint (the linter is called Clippy).
* **`#[serde(...)]` attributes** configure JSON mapping; see 1.14.
* **Doc comments**: `///` documents the next item and `//!` documents the enclosing module. They
  are the docstrings of Rust and are rendered by `cargo doc`.
* **`std::array::from_fn(|_| Vec::new())`** builds a fixed-size array by calling a function per
  slot.

---

## 1.13 Bevy in five minutes

Bevy is a game engine built around an **Entity Component System (ECS)**. The client
([`osrs_core/src/main.rs`](../osrs_core/src/main.rs)), the viewer and `osrs_render` use it. The
project is on Bevy 0.20.

The ECS replaces the object-oriented "a `Player` class with an `update()` method" design with a
database-like one:

| Concept | What it is | Analogy |
|---|---|---|
| **Entity** | an ID, nothing more | a primary key |
| **Component** | a plain struct attached to an entity (`#[derive(Component)]`) | a column in a table |
| **Resource** | a single global value (`#[derive(Resource)]`) | a module-level singleton |
| **System** | an ordinary function that Bevy calls every frame | a scheduled job |
| **Query** | a system parameter that iterates entities having certain components | `SELECT ... WHERE has(A) AND has(B)` |
| **Commands** | a queue of deferred changes (spawn, despawn, insert components) | a batch write applied after the system |
| **Plugin** | a bundle of systems and resources added to the app together | a Flask blueprint |
| **Schedule** | when systems run: `Startup` once, `Update` every frame | |

### Systems get their inputs by type

A system's parameters are not passed by you. Bevy looks at their types and injects them:

```rust
fn orbit_camera(
    buttons: Res<ButtonInput<MouseButton>>,          // read a resource
    motion: Res<AccumulatedMouseMotion>,
    players: Query<&Transform, (With<Player>, Without<OrbitCamera>)>,      // read-only query
    mut cams: Query<(&mut Transform, &mut OrbitCamera), Without<Player>>,  // mutable query
) { ... }
```

* `Res<T>` / `ResMut<T>`: read or write resource `T`.
* `Query<(&A, &mut B), With<C>>`: every entity that has `A`, `B` and `C`. `.single()` gets the one
  matching entity; `for (a, b) in &mut query` iterates.
* `Without<...>` filters are often needed to prove to Bevy that two queries never touch the same
  entity's `Transform` at once, which lets the borrow rules hold.
* `Local<T>`: a value private to this system that persists between frames, like a
  function-level `static` (or a closure's captured state).
* `Commands`: deferred world edits.
* `NonSend<T>` / `NonSendMut<T>`: like `Res`, for resources that are not thread-safe. Bevy then runs
  that system on the main thread. The client keeps its server link, the map-square loader and the
  NPC model loader this way, because they hold channel receivers or `Rc` data.
* `Assets<Mesh>`, `Assets<Image>`, `Assets<StandardMaterial>`: Bevy's asset stores. `add` returns a
  `Handle<T>`, a cheap reference-counted ID that components such as `Mesh3d(handle)` hold.

### Ordering

Systems in `Update` may run in parallel and in any order unless told otherwise.
`(a, b, c).chain()` forces sequence. `.after(x)` / `.before(y)` add individual constraints. The
client chains its main systems so that it reads the newest snapshot before moving characters.

### Other Bevy pieces used here

* `Transform` (position, rotation, scale) and `GlobalTransform` (after parent transforms are
  applied). Children move with their parent; `commands.entity(parent).add_child(child)`.
* `Camera3d`, `Mesh3d`, `MeshMaterial3d`, `StandardMaterial` (with `unlit: true` because lighting
  is baked into vertex colours).
* UI: `Node` (a flexbox box), `ImageNode`, `Text`, `TextSpan`, `BackgroundColor`, `GlobalZIndex`.
* `Gizmos`: immediate-mode debug lines, redrawn every frame (used for tile outlines).
* `Time`: `time.delta_secs()` since last frame, `time.elapsed_secs()` since start.
* `MessageWriter<AppExit>`: Bevy 0.20 calls its event queues "messages"; writing `AppExit` quits.
* `SystemParam` derive: bundles several system parameters into one struct so helpers can take
  them together (see `SquareSpawner` in [`map_stream.rs`](../osrs_core/src/map_stream.rs)).

---

## 1.14 serde: pydantic for Rust

`serde` turns Rust types into and out of JSON (and other formats) with derive macros. The data
layer in [`osrs_core/src/data/`](../osrs_core/src/data/) is built on it.

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ItemDef {
    pub id: String,
    pub slot: EquipSlot,
    pub weapon: Option<WeaponDef>,
    ...
}
```

| Attribute | Effect | pydantic analogy |
|---|---|---|
| `#[serde(default)]` on a struct | missing fields take the struct's `Default` value | field defaults |
| `#[serde(default = "one")]` on a field | missing field calls `one()` | `Field(default_factory=...)` |
| `#[serde(deny_unknown_fields)]` | unknown JSON keys are an **error** | `extra = "forbid"` |
| `#[serde(rename_all = "snake_case")]` on an enum | variant `VampyreTier1` is written `"vampyre_tier1"` | aliases |
| `#[serde(from = "Vec<Attribute>", into = ...)]` | (de)serialise via another type | custom validators |

Enums map to JSON in serde's default "externally tagged" style: a variant without data is a
string (`"chase"`), and a variant with data is a one-key object
(`{"maintain_distance": {"distance": 3}}`). That is why the monster `movement` field accepts both
forms.

`serde_json::from_str::<T>(text)` parses; errors carry the line and column.

---

## 1.15 Threads and channels

Python's `threading.Thread` + `queue.Queue` maps directly onto `std::thread::spawn` +
`std::sync::mpsc::channel()` ("multi-producer, single-consumer"):

```rust
let (tx, rx) = mpsc::channel();      // like q = queue.Queue(); tx and rx are the two ends
thread::spawn(move || {              // the closure owns everything it captured
    for request in rx { ... tx2.send(result) ... }   // loop ends when every sender is dropped
});
tx.send(x);                          // q.put(x)
rx.try_recv();                       // q.get_nowait()
rx.try_iter().collect::<Vec<_>>();   // drain everything currently queued
```

Unlike Python there is no global interpreter lock: threads really run in parallel. This repo
uses one pattern over and over: a long-lived worker thread that owns non-thread-safe state
(an open cache, `Rc` models, the `World`), fed by one channel and answering on another. The
client side polls the answer channel once per frame without blocking. Dropping the last sender
closes the channel, which ends the worker's loop, so dropping the handle stops the thread.
