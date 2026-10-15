//! A world that is loaded one map square at a time, as the player moves through it.
//!
//! Decoding runs on its own thread ([`SquareStream`]) and hands back plain data
//! ([`SquareData`]); the app spawns that into the scene, and takes it out again when the player
//! has left it behind. Render space is the world's own: tile `(x, z)` is at Bevy `(x, _, -z)`, so
//! every square sits where it really is and nothing has to be re-based when one comes or goes.

use crate::pick::GroundSurface;
use crate::world::{load_objects, model_loader, spawn_parts, MaterialCache, ObjectScene, ScrollingTextures, TextureStore};
use bevy::prelude::*;
use osrs_cache::config::Configs;
use osrs_cache::model::Model;
use osrs_cache::anim::{Animations, Clip};
use osrs_cache::scene::{build_loc, build_loc_with, LocMesh};
use osrs_cache::squares::{MapSource, SQUARE};
use osrs_cache::terrain_mesh::TerrainMesh;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Instant;

/// Corner heights along one side of a square: its 64 tiles need 65 corners.
const CORNERS: usize = SQUARE as usize + 1;

/// One map square, decoded and ready to spawn. Positions are relative to the square's
/// south-west corner.
pub struct SquareData {
    /// Map square coordinates.
    pub square: (i32, i32),
    /// Whether the cache has this square at all; a square that is missing is flat and empty.
    pub present: bool,
    /// Terrain meshes of the four planes.
    pub terrain: Vec<TerrainMesh>,
    pub objects: Arc<ObjectScene>,
    /// Ground-level corner heights, client units (negative is up), index `x * 65 + z`.
    pub heights: Vec<i32>,
    /// The same corners on the plane above: the deck of a bridge is drawn (and walked on) at
    /// these heights instead of the ground's.
    pub deck_heights: Vec<i32>,
    /// Whether the tile at index `x * 64 + z` is a bridge deck, so its surface is `deck_heights`.
    pub bridges: Vec<bool>,
    /// Whether the ground-level tile at index `x * 64 + z` has a roof over it (tile flag 4): the
    /// client takes the roofs off when the view into the building crosses such a tile.
    pub roofs: Vec<bool>,
}

/// Which map square a world tile is in.
pub fn square_of(x: i32, z: i32) -> (i32, i32) {
    (x.div_euclid(SQUARE), z.div_euclid(SQUARE))
}

/// Render-space position of a square's south-west corner.
pub fn square_offset(square: (i32, i32)) -> Vec3 {
    Vec3::new((square.0 * SQUARE) as f32, 0.0, -((square.1 * SQUARE) as f32))
}

/// Decodes squares. Not `Send` (models are shared with `Rc`); make one on the thread that uses it.
pub struct SquareLoader {
    source: MapSource,
    texture_hsl: HashMap<u32, u16>,
    models: HashMap<u32, Option<Rc<Model>>>,
    /// The cache's item definitions, decoded the first time an item is drawn on the ground.
    items: Option<HashMap<u32, osrs_cache::item::ItemDef>>,
}

impl SquareLoader {
    /// A loader reading the active cache (`Cache::open_default`).
    pub fn open() -> osrs_cache::Result<SquareLoader> {
        SquareLoader::from_cache(osrs_cache::Cache::open_default()?)
    }

    /// A loader reading `cache` (one with an overlay, say: the map editor's uncompiled edits).
    pub fn from_cache(cache: osrs_cache::Cache) -> osrs_cache::Result<SquareLoader> {
        let source = MapSource::from_cache(cache)?;
        let texture_hsl = Configs::new(source.cache())?.texture_average_hsl()?;
        Ok(SquareLoader { source, texture_hsl, models: HashMap::new(), items: None })
    }

    /// The left-click option of every object that has one, and the names of all objects: needed
    /// once, by whatever picks objects out of the scene.
    pub fn object_info(&self) -> (HashMap<u32, String>, HashMap<u32, String>) {
        let defs = self.source.object_defs();
        let left_click = defs.values().filter_map(|d| Some((d.id, d.left_click()?.to_owned()))).collect();
        let names = defs.values().filter(|d| !d.name.is_empty()).map(|d| (d.id, d.name.clone())).collect();
        (left_click, names)
    }

    /// The mesh of object `id` placed as shape `kind` turned `orientation` quarter turns, on its
    /// own (an object put in place of another at run time: a tree's stump). `None` if it has no
    /// model.
    pub fn build_object(&mut self, id: u32, kind: u8, orientation: u8) -> Option<LocMesh> {
        let def = self.source.object_defs().get(&id)?;
        let mut load_model = model_loader(self.source.cache(), &mut self.models);
        build_loc(&mut load_model, def, kind, orientation, None, &self.texture_hsl).filter(|mesh| !mesh.is_empty())
    }

    /// [`SquareLoader::build_object`] for an object that has an animation (a fire's flames): the
    /// mesh keeps its rig and the animation is baked against it. `None` for the clip when the
    /// object does not move, or its sequence does not fit the model.
    pub fn build_animated_object(&mut self, id: u32, kind: u8, orientation: u8) -> (Option<LocMesh>, Option<Clip>) {
        let Some(def) = self.source.object_defs().get(&id) else { return (None, None) };
        let Some(sequence) = def.anim else { return (self.build_object(id, kind, orientation), None) };
        let mut load_model = model_loader(self.source.cache(), &mut self.models);
        let mesh = build_loc_with(&mut load_model, def, kind, orientation, None, &self.texture_hsl, true)
            .filter(|mesh| !mesh.is_empty());
        let clip = mesh.as_ref().and_then(|m| m.rig.as_ref()).and_then(|rig| {
            let mut animations = Animations::new(self.source.cache()).ok()?;
            Clip::bake(&mut animations, sequence, rig).ok()
        });
        (mesh, clip)
    }

    /// The mesh of cache item `id` as it lies on the ground (its inventory model, resized and
    /// recoloured as its definition says). `None` if it has no model.
    pub fn build_item(&mut self, id: u32) -> Option<LocMesh> {
        if self.items.is_none() {
            self.items = Some(Configs::new(self.source.cache()).ok()?.items().ok()?);
        }
        let def = self.items.as_ref()?.get(&id)?;
        osrs_cache::item_icon::icon_mesh(self.source.cache(), def, &self.texture_hsl).filter(|mesh| !mesh.is_empty())
    }

    /// Decodes square (`sx`, `sy`).
    pub fn load(&mut self, sx: i32, sy: i32) -> osrs_cache::Result<SquareData> {
        let started = Instant::now();
        let around = self.source.neighbourhood(sx, sy)?;
        let side = SQUARE as usize;
        // the middle square of the 3x3 block that was decoded
        let (x0, z0) = (side, side);

        let present = around.exists();
        let terrain: Vec<TerrainMesh> = if present {
            (0..4).map(|plane| around.grid.build_mesh_window(plane, x0, z0, side, side)).collect()
        } else {
            (0..4).map(|_| TerrainMesh::default()).collect()
        };
        let objects = if present && std::env::var("OSRS_NO_OBJECTS").is_err() {
            load_objects(
                self.source.cache(),
                &around.grid,
                self.source.object_defs(),
                &around.placed,
                &self.texture_hsl,
                &mut self.models,
                Some((x0 as i32, z0 as i32)),
                false,
            )?
        } else {
            ObjectScene::default()
        };
        let mut heights = Vec::with_capacity(CORNERS * CORNERS);
        let mut deck_heights = Vec::with_capacity(CORNERS * CORNERS);
        for x in 0..CORNERS {
            for z in 0..CORNERS {
                heights.push(around.grid.height_at(0, (x0 + x) as i32, (z0 + z) as i32));
                deck_heights.push(around.grid.height_at(1, (x0 + x) as i32, (z0 + z) as i32));
            }
        }
        // a tile whose plane-1 flags carry the bridge bit is drawn on the ground level, from the
        // heights of plane 1
        let mut bridges = Vec::with_capacity(side * side);
        for x in 0..side {
            for z in 0..side {
                bridges.push(present && around.grid.tile(1, x0 + x, z0 + z).settings & 2 != 0);
            }
        }
        let mut roofs = Vec::with_capacity(side * side);
        for x in 0..side {
            for z in 0..side {
                roofs.push(present && around.grid.tile(0, x0 + x, z0 + z).settings & 4 != 0);
            }
        }
        eprintln!("square ({sx}, {sy}) loaded in {:.2?}", started.elapsed());
        Ok(SquareData { square: (sx, sy), present, terrain, objects: Arc::new(objects), heights, deck_heights, bridges, roofs })
    }
}

/// What came back from the loading thread.
pub enum Loaded {
    Ready(SquareData),
    Failed((i32, i32), String),
    /// The mesh asked for with [`SquareStream::request_object`] (`None` if it has no model).
    Object { id: u32, kind: u8, orientation: u8, mesh: Option<LocMesh>, clip: Option<Clip> },
    /// The mesh asked for with [`SquareStream::request_item`].
    Item { id: u32, mesh: Option<LocMesh> },
}

/// What the loading thread is asked for.
enum Request {
    Square((i32, i32)),
    Object { id: u32, kind: u8, orientation: u8 },
    Item { id: u32 },
}

/// A thread that decodes squares (and single objects) on request. Dropping the handle stops it.
pub struct SquareStream {
    requests: Sender<Request>,
    results: Receiver<Loaded>,
}

impl SquareStream {
    /// A loading thread reading the active cache.
    pub fn spawn() -> SquareStream {
        SquareStream::spawn_with(SquareLoader::open)
    }

    /// A loading thread reading `cache`.
    pub fn spawn_with_cache(cache: osrs_cache::Cache) -> SquareStream {
        SquareStream::spawn_with(move || SquareLoader::from_cache(cache))
    }

    /// A loading thread whose loader `make` builds (on that thread: a loader cannot move between
    /// threads).
    fn spawn_with(make: impl FnOnce() -> osrs_cache::Result<SquareLoader> + Send + 'static) -> SquareStream {
        let (request_tx, request_rx) = mpsc::channel::<Request>();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            let mut loader = match make() {
                Ok(loader) => loader,
                Err(e) => {
                    // nothing can be loaded; report every request as failed
                    for request in request_rx {
                        let result = match request {
                            Request::Square(square) => Loaded::Failed(square, e.to_string()),
                            Request::Object { id, kind, orientation } => Loaded::Object { id, kind, orientation, mesh: None, clip: None },
                            Request::Item { id } => Loaded::Item { id, mesh: None },
                        };
                        let _ = result_tx.send(result);
                    }
                    return;
                }
            };
            for request in request_rx {
                let result = match request {
                    Request::Square((sx, sy)) => match loader.load(sx, sy) {
                        Ok(data) => Loaded::Ready(data),
                        Err(e) => Loaded::Failed((sx, sy), e.to_string()),
                    },
                    Request::Object { id, kind, orientation } => {
                        let (mesh, clip) = loader.build_animated_object(id, kind, orientation);
                        Loaded::Object { id, kind, orientation, mesh, clip }
                    }
                    Request::Item { id } => Loaded::Item { id, mesh: loader.build_item(id) },
                };
                if result_tx.send(result).is_err() {
                    return;
                }
            }
        });
        SquareStream { requests: request_tx, results: result_rx }
    }

    pub fn request(&self, square: (i32, i32)) {
        let _ = self.requests.send(Request::Square(square));
    }

    /// Asks for the mesh of one object placed as `kind`, turned `orientation`; it comes back from
    /// [`SquareStream::poll`] as [`Loaded::Object`].
    pub fn request_object(&self, id: u32, kind: u8, orientation: u8) {
        let _ = self.requests.send(Request::Object { id, kind, orientation });
    }

    /// Asks for the mesh of cache item `id` lying on the ground; it comes back from
    /// [`SquareStream::poll`] as [`Loaded::Item`].
    pub fn request_item(&self, id: u32) {
        let _ = self.requests.send(Request::Item { id });
    }

    /// Every result that has come back since the last call.
    pub fn poll(&self) -> Vec<Loaded> {
        self.results.try_iter().collect()
    }
}

/// The squares that are currently in the scene, with what is needed to stand things on them.
#[derive(Resource, Default)]
pub struct StreamedWorld {
    squares: HashMap<(i32, i32), Resident>,
}

struct Resident {
    present: bool,
    heights: Vec<i32>,
    deck_heights: Vec<i32>,
    bridges: Vec<bool>,
    roofs: Vec<bool>,
    entities: Vec<Entity>,
}

impl StreamedWorld {
    /// Spawns `data` into the scene at its place in the world and remembers it. Returns the
    /// entities made, so the caller can add its own to the same square (and they all leave with it).
    #[allow(clippy::too_many_arguments)]
    pub fn spawn_square(
        &mut self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        materials: &mut Assets<StandardMaterial>,
        images: &mut Assets<Image>,
        scrolling: &mut ScrollingTextures,
        shared: &mut MaterialCache,
        textures: &TextureStore,
        data: &SquareData,
    ) {
        let entities = spawn_parts(
            commands,
            meshes,
            materials,
            images,
            scrolling,
            shared,
            textures,
            &data.terrain,
            &data.objects,
            square_offset(data.square),
        );
        self.squares
            .insert(
                data.square,
                Resident {
                    present: data.present,
                    heights: data.heights.clone(),
                    deck_heights: data.deck_heights.clone(),
                    bridges: data.bridges.clone(),
                    roofs: data.roofs.clone(),
                    entities,
                },
            );
    }

    /// Adds entities that belong to a loaded square (a debug overlay, say) so they are despawned
    /// with it.
    pub fn attach(&mut self, square: (i32, i32), entity: Entity) {
        if let Some(resident) = self.squares.get_mut(&square) {
            resident.entities.push(entity);
        }
    }

    /// Forgets a square and returns its entities, for the caller to despawn.
    pub fn remove(&mut self, square: (i32, i32)) -> Vec<Entity> {
        self.squares.remove(&square).map(|r| r.entities).unwrap_or_default()
    }

    pub fn contains(&self, square: (i32, i32)) -> bool {
        self.squares.contains_key(&square)
    }

    pub fn squares(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.squares.keys().copied()
    }

    /// Whether world tile (`x`, `z`) has a roof over it, if its square is loaded.
    pub fn roofed(&self, x: i32, z: i32) -> bool {
        let Some(resident) = self.squares.get(&square_of(x, z)) else { return false };
        let (lx, lz) = (x.rem_euclid(SQUARE) as usize, z.rem_euclid(SQUARE) as usize);
        resident.roofs.get(lx * SQUARE as usize + lz).copied().unwrap_or(false)
    }

    /// Heights (client units) of the four corners of world tile (`x`, `z`) as it is drawn, in the
    /// order south-west, south-east, north-west, north-east, if its square is loaded. A bridge
    /// deck is drawn from the heights of the plane above, not the ground under it.
    pub fn tile_corners(&self, x: i32, z: i32) -> Option<[i32; 4]> {
        let resident = self.squares.get(&square_of(x, z))?;
        if !resident.present {
            return None;
        }
        let (lx, lz) = (x.rem_euclid(SQUARE) as usize, z.rem_euclid(SQUARE) as usize);
        let heights = if resident.bridges[lx * SQUARE as usize + lz] { &resident.deck_heights } else { &resident.heights };
        let at = |dx: usize, dz: usize| heights[(lx + dx) * CORNERS + lz + dz];
        Some([at(0, 0), at(1, 0), at(0, 1), at(1, 1)])
    }

    /// The four outer corners (south-west, south-east, north-east, north-west, so they draw as a
    /// loop) in render space of the `w` x `h` block of tiles whose south-west tile is (`x`, `z`),
    /// raised `lift` tiles off the ground, if its squares are loaded. A tile outline is a 1 x 1
    /// block.
    pub fn block_corners(&self, x: i32, z: i32, w: i32, h: i32, lift: f32) -> Option<[Vec3; 4]> {
        let y = |tx: i32, tz: i32, corner: usize| self.tile_corners(tx, tz).map(|c| -(c[corner] as f32) / 128.0 + lift);
        Some([
            Vec3::new(x as f32, y(x, z, 0)?, -(z as f32)),
            Vec3::new((x + w) as f32, y(x + w - 1, z, 1)?, -(z as f32)),
            Vec3::new((x + w) as f32, y(x + w - 1, z + h - 1, 3)?, -((z + h) as f32)),
            Vec3::new(x as f32, y(x, z + h - 1, 2)?, -((z + h) as f32)),
        ])
    }

    /// Where an object `w` x `h` tiles placed with its south-west tile at (`x`, `z`) stands: the
    /// middle of its footprint, at the average height of the footprint's four outer corners (as
    /// the map loader places objects), if its square is loaded.
    pub fn object_origin(&self, x: i32, z: i32, w: i32, h: i32) -> Option<Vec3> {
        // the outer corners: south-west of the south-west tile, south-east of the south-east one...
        let sw = self.tile_corners(x, z)?[0];
        let se = self.tile_corners(x + w - 1, z)?[1];
        let nw = self.tile_corners(x, z + h - 1)?[2];
        let ne = self.tile_corners(x + w - 1, z + h - 1)?[3];
        let height = (sw + se + nw + ne) / 4;
        Some(Vec3::new(x as f32 + w as f32 / 2.0, -(height as f32) / 128.0, -(z as f32 + h as f32 / 2.0)))
    }

    /// Render-space position of the ground at the middle of a world tile, if it is loaded.
    pub fn ground_at(&self, x: i32, z: i32) -> Option<Vec3> {
        let h: i32 = self.tile_corners(x, z)?.iter().sum();
        Some(Vec3::new(x as f32 + 0.5, -(h as f32) / 4.0 / 128.0, -(z as f32 + 0.5)))
    }
}

impl GroundSurface for StreamedWorld {
    /// Streamed worlds keep ground level only: any other plane has no surface.
    fn surface_y(&self, plane: usize, x: f32, z: f32) -> Option<f32> {
        if plane != 0 {
            return None;
        }
        let (ix, iz) = (x.floor() as i32, z.floor() as i32);
        let (fx, fz) = (x - ix as f32, z - iz as f32);
        let [sw, se, nw, ne] = self.tile_corners(ix, iz)?.map(|h| h as f32);
        let south = sw * (1.0 - fx) + se * fx;
        let north = nw * (1.0 - fx) + ne * fx;
        Some(-(south * (1.0 - fz) + north * fz) / 128.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_square(square: (i32, i32), height: i32) -> SquareData {
        SquareData {
            square,
            present: true,
            terrain: Vec::new(),
            objects: Arc::new(ObjectScene::default()),
            heights: vec![height; CORNERS * CORNERS],
            deck_heights: vec![height; CORNERS * CORNERS],
            bridges: vec![false; (SQUARE * SQUARE) as usize],
            roofs: vec![false; (SQUARE * SQUARE) as usize],
        }
    }

    fn resident(data: SquareData) -> Resident {
        Resident {
            present: data.present,
            heights: data.heights,
            deck_heights: data.deck_heights,
            bridges: data.bridges,
            roofs: data.roofs,
            entities: Vec::new(),
        }
    }

    fn world_with(squares: &[((i32, i32), i32)]) -> StreamedWorld {
        let mut world = StreamedWorld::default();
        for &(square, height) in squares {
            world.squares.insert(square, resident(flat_square(square, height)));
        }
        world
    }

    #[test]
    fn squares_sit_at_their_world_position() {
        assert_eq!(square_of(3222, 3218), (50, 50));
        assert_eq!(square_of(3199, 3200), (49, 50));
        assert_eq!(square_of(-1, 0), (-1, 0));
        assert_eq!(square_offset((50, 50)), Vec3::new(3200.0, 0.0, -3200.0));
    }

    #[test]
    fn a_tile_on_the_border_uses_its_squares_own_corner_column_beyond_the_edge() {
        // square 49's corner arrays run one past its last tile: that column is square 50's first
        let mut left = flat_square((49, 50), -128);
        for z in 0..CORNERS {
            left.heights[(CORNERS - 1) * CORNERS + z] = -256;
        }
        let mut world = StreamedWorld::default();
        world.squares.insert((49, 50), resident(left));
        world.squares.insert((50, 50), resident(flat_square((50, 50), -256)));

        // tile 3199 is the last of square 49: west corners -128, east corners (the next square's) -256
        let tile = world.ground_at(3199, 3200).expect("loaded");
        assert_eq!((tile.x, tile.z), (3199.5, -3200.5));
        assert!((tile.y - 1.5).abs() < 1e-6, "{}", tile.y);
    }

    #[test]
    fn ground_beyond_the_loaded_squares_is_not_there() {
        let world = world_with(&[((50, 50), 0)]);
        assert!(world.ground_at(3222, 3218).is_some());
        assert!(world.ground_at(3199, 3218).is_none(), "square 49 is not loaded");
        assert!(world.surface_y(0, 3199.5, 3218.0).is_none());
        assert!(world.surface_y(1, 3222.0, 3218.0).is_none(), "only ground level is kept");
    }

    #[test]
    fn a_roofed_tile_is_found_by_its_world_tile() {
        let mut data = flat_square((50, 50), 0);
        data.roofs[3 * SQUARE as usize + 4] = true;
        let mut world = StreamedWorld::default();
        world.squares.insert((50, 50), resident(data));
        assert!(world.roofed(3200 + 3, 3200 + 4));
        assert!(!world.roofed(3200 + 4, 3200 + 3));
        assert!(!world.roofed(0, 0), "an unloaded square has no roof");
    }

    #[test]
    fn a_missing_square_is_not_ground() {
        let mut world = world_with(&[((50, 50), 0)]);
        world.squares.get_mut(&(50, 50)).unwrap().present = false;
        assert!(world.ground_at(3222, 3218).is_none());
    }

    #[test]
    fn the_surface_blends_between_tile_corners() {
        // heights step from 0 (x = 0) to -128 (x = 1) along the square's first column
        let mut data = flat_square((0, 0), 0);
        for z in 0..CORNERS {
            data.heights[CORNERS + z] = -128;
        }
        let mut world = StreamedWorld::default();
        world.squares.insert((0, 0), resident(data));
        let y = world.surface_y(0, 0.5, 10.0).unwrap();
        assert!((y - 0.5).abs() < 1e-6, "halfway up the slope: {y}");
    }

    #[test]
    fn a_bridge_deck_is_walked_on_at_the_height_it_is_drawn() {
        // ground at 0, the deck two tiles up, a bridge on tile (5, 5) only
        let mut data = flat_square((0, 0), 0);
        data.deck_heights = vec![-256; CORNERS * CORNERS];
        data.bridges[5 * SQUARE as usize + 5] = true;
        let mut world = StreamedWorld::default();
        world.squares.insert((0, 0), resident(data));

        let deck = world.ground_at(5, 5).unwrap();
        assert!((deck.y - 2.0).abs() < 1e-6, "on the deck: {}", deck.y);
        let beside = world.ground_at(6, 5).unwrap();
        assert!(beside.y.abs() < 1e-6, "off the bridge it is the ground: {}", beside.y);
        assert!((world.surface_y(0, 5.5, 5.5).unwrap() - 2.0).abs() < 1e-6);
    }

    #[test]
    fn a_real_bridge_deck_is_above_the_river_bed_it_spans() {
        let Ok(mut loader) = SquareLoader::open() else { return };
        let mut best = (0.0f32, (0, 0), 0usize);
        for square in [(50, 50), (50, 49), (49, 50), (51, 50), (49, 49), (51, 49), (50, 51)] {
            let data = loader.load(square.0, square.1).unwrap();
            let mut count = 0;
            for x in 0..SQUARE as usize {
                for z in 0..SQUARE as usize {
                    if !data.bridges[x * SQUARE as usize + z] {
                        continue;
                    }
                    count += 1;
                    let mean = |h: &Vec<i32>| {
                        let at = |dx: usize, dz: usize| h[(x + dx) * CORNERS + z + dz] as f32;
                        -(at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1)) / 4.0 / 128.0
                    };
                    let rise = mean(&data.deck_heights) - mean(&data.heights);
                    if rise > best.0 {
                        best = (rise, (square.0 * SQUARE + x as i32, square.1 * SQUARE + z as i32), count);
                    }
                }
            }
            eprintln!("square {square:?}: {count} bridge deck tiles");
        }
        eprintln!("largest deck rise {:.2} tiles at {:?}", best.0, best.1);
        assert!(best.0 > 0.5, "some deck near Lumbridge should stand clear of the ground beneath it");
    }
}
