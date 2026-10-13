//! Map rendering shared by the viewer and the client: loads map squares from the cache and spawns
//! terrain and objects as entities tagged with their plane, plus the systems that animate them
//! (scrolling textures, depth-sorted translucent faces).

use crate::{texture_material, to_bevy_mesh, OBJECT_CULL};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use osrs_cache::collision::CollisionGrid;
use osrs_cache::config::{Configs, ObjectDef};
use osrs_cache::model::Model;
use osrs_cache::scene::{build_loc, contour, footprint, LocMesh};
use osrs_cache::terrain_mesh::{PlacedObjects, TerrainGrid, TerrainMesh};
use osrs_cache::textures::{load_textures, Texture};
use osrs_cache::{idx, Cache};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Texture-widths scrolled per second for each unit of the definition's speed byte. The client's
/// exact rate is not known; tuned by eye.
const SCROLL_PER_SPEED: f32 = 0.08;

/// Adds the systems that animate a spawned world. The camera is any `Camera3d`.
pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ScrollingTextures>().add_systems(Update, (scroll_textures, sort_translucent));
    }
}

/// Which plane an entity belongs to, so the app can hide planes above the viewer.
#[derive(Component)]
pub struct OnPlane(pub u8);

/// Scrolling textures (water, lava...): the shared material's UV transform is moved over time.
#[derive(Resource, Default)]
pub struct ScrollingTextures(pub Vec<(Handle<StandardMaterial>, u8, u8)>);

/// A translucent object's faces, re-sorted back to front as the camera moves (the client paints
/// translucent faces in depth order; one unsorted blended mesh shows through itself wrongly).
#[derive(Component)]
pub struct SortedFaces {
    /// (centroid in object space, vertex indices of the face)
    faces: Vec<(Vec3, [u32; 3])>,
    origin: Vec3,
    last_eye: Option<Vec3>,
}

/// Decoded texture images by id.
#[derive(Resource, Default)]
pub struct TextureStore(pub HashMap<u32, Texture>);

/// Which mesh an object instance uses: object id, kind, orientation, wall displacement (255 for
/// none), and 0 for a mesh shared by every placement with those values, or a number unique to one
/// placement whose mesh is bent onto the terrain under it (contoured ground, e.g. steps on a slope).
pub type ObjectKey = (u32, u8, u8, u8, u32);

/// Marks every entity spawned for a placed scenery object: which object it is and the world tile
/// under its middle (inside its footprint, whatever its size), so one placement can be found
/// again among the thousands of a square, to hide it or put something in its place.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedLoc {
    pub id: u32,
    pub tile: (i32, i32),
}

/// The world tile under a render-space point (an object instance's middle).
pub fn tile_under(point: Vec3) -> (i32, i32) {
    (point.x.floor() as i32, (-point.z).floor() as i32)
}

/// Unique object meshes plus where each is placed.
#[derive(Resource, Default, Clone)]
pub struct ObjectScene {
    pub meshes: HashMap<ObjectKey, LocMesh>,
    pub instances: Vec<(ObjectKey, Vec3, u8)>,
    /// Objects whose left-click option is something other than walking (Open, Chop down...), with
    /// that option: only these have a click box and take clicks from the ground behind them.
    pub left_click: HashMap<u32, String>,
    /// Object names by id, for logging what was clicked.
    pub names: HashMap<u32, String>,
}

impl ObjectScene {
    /// A scene of one object: `mesh` placed with its middle at `origin` on `plane` (a stump put
    /// where a tree was).
    pub fn single(key: ObjectKey, mesh: LocMesh, origin: Vec3, plane: u8) -> ObjectScene {
        ObjectScene {
            meshes: HashMap::from([(key, mesh)]),
            instances: vec![(key, origin, plane)],
            left_click: HashMap::new(),
            names: HashMap::new(),
        }
    }
}

/// Everything decoded for a block of map squares, ready to spawn.
pub struct WorldData {
    pub grid: TerrainGrid,
    /// Walkability of plane 0, for the simulator.
    pub collision: CollisionGrid,
    pub terrain: Vec<TerrainMesh>,
    pub objects: Arc<ObjectScene>,
    pub textures: TextureStore,
}

impl WorldData {
    /// The plane whose heights tile (`x`, `z`) of ground level is drawn at: a bridge deck is
    /// drawn from the plane above, not from the ground beneath it.
    fn surface_plane(&self, plane: usize, x: i32, z: i32) -> usize {
        let on_deck = plane == 0
            && x >= 0
            && z >= 0
            && x < self.grid.width as i32
            && z < self.grid.depth as i32
            && self.grid.tile(1, x as usize, z as usize).settings & 2 != 0;
        if on_deck { 1 } else { plane }
    }

    /// Bevy position of the ground at the middle of tile `(x, z)` (grid coordinates, i.e. world
    /// tile minus the loaded block's south-west corner) on `plane`. Bevy z is minus north.
    pub fn ground_at(&self, plane: usize, x: i32, z: i32) -> Vec3 {
        let plane = self.surface_plane(plane, x, z);
        let h = (self.grid.height_at(plane, x, z)
            + self.grid.height_at(plane, x + 1, z)
            + self.grid.height_at(plane, x, z + 1)
            + self.grid.height_at(plane, x + 1, z + 1)) as f32
            / 4.0;
        Vec3::new(x as f32 + 0.5, -h / 128.0, -(z as f32 + 0.5))
    }

    /// Bevy y of the terrain surface at a point (grid tile units, bilinear between tile corners),
    /// or `None` outside the loaded block.
    pub fn surface_y(&self, plane: usize, x: f32, z: f32) -> Option<f32> {
        let (w, d) = (self.grid.width as f32, self.grid.depth as f32);
        if x < 0.0 || z < 0.0 || x >= w - 1.0 || z >= d - 1.0 {
            return None;
        }
        let (ix, iz) = (x.floor() as i32, z.floor() as i32);
        let (fx, fz) = (x - ix as f32, z - iz as f32);
        let plane = self.surface_plane(plane, ix, iz);
        let h = |dx: i32, dz: i32| self.grid.height_at(plane, ix + dx, iz + dz) as f32;
        let south = h(0, 0) * (1.0 - fx) + h(1, 0) * fx;
        let north = h(0, 1) * (1.0 - fx) + h(1, 1) * fx;
        Some(-(south * (1.0 - fz) + north * fz) / 128.0)
    }

    /// Centre of the loaded area and its larger side in tiles.
    pub fn center_and_extent(&self) -> (Vec3, f32) {
        let (w, d) = (self.grid.width as f32, self.grid.depth as f32);
        (Vec3::new(w / 2.0, 0.0, -d / 2.0), w.max(d))
    }
}

/// Load `nx` x `ny` map squares starting at square `(rx, ry)`. `OSRS_NO_OBJECTS` and
/// `OSRS_NO_TEXTURES` skip those parts.
pub fn load_world(rx: i32, ry: i32, nx: usize, ny: usize) -> osrs_cache::Result<WorldData> {
    let t0 = std::time::Instant::now();
    let cache = Cache::open_default()?;
    let cfg = Configs::new(&cache)?;
    let grid = TerrainGrid::load(&cache, rx, ry, nx, ny, cfg.underlays()?, cfg.overlays()?)?;
    if !grid.missing.is_empty() {
        eprintln!("map squares missing from cache: {:?}", grid.missing);
    }
    let terrain: Vec<TerrainMesh> = (0..4).map(|p| grid.build_mesh(p)).collect();
    eprintln!(
        "terrain: {}x{} tiles, triangles per plane {:?} in {:.2?}",
        grid.width,
        grid.depth,
        terrain.iter().map(|m| m.indices.len() / 3).collect::<Vec<_>>(),
        t0.elapsed()
    );
    let defs = cfg.objects()?;
    let placed = PlacedObjects::load(&cache, &grid)?;
    let collision = CollisionGrid::build(&grid, &placed, &defs);
    let blocked = collision.flags.iter().filter(|f| **f != 0).count();
    eprintln!("collision: {} of {} tiles carry flags", blocked, collision.flags.len());
    let objects = if std::env::var("OSRS_NO_OBJECTS").is_ok() {
        ObjectScene::default()
    } else {
        let tex = cfg.texture_average_hsl()?;
        load_objects(&cache, &grid, &defs, &placed, &tex, &mut HashMap::new(), None, true)?
    };
    let textures = if std::env::var("OSRS_NO_TEXTURES").is_ok() {
        TextureStore::default()
    } else {
        TextureStore(load_textures(&cache)?)
    };
    Ok(WorldData { grid, collision, terrain, objects: Arc::new(objects), textures })
}

/// Builds the object meshes and instances of `placed`.
///
/// With `window` set to the grid tile of a square's south-west corner, only that 64x64 square's
/// placements are kept and their positions are relative to the corner (the rest of the grid is
/// there for the heights and walls around the border). `models` caches decoded models between
/// calls; `with_info` also fills in the objects' left-click options and their names.
#[allow(clippy::too_many_arguments)]
pub(crate) fn load_objects(
    cache: &Cache,
    grid: &TerrainGrid,
    defs: &HashMap<u32, ObjectDef>,
    placed: &PlacedObjects,
    tex: &HashMap<u32, u16>,
    model_cache: &mut HashMap<u32, Option<Rc<Model>>>,
    window: Option<(i32, i32)>,
    with_info: bool,
) -> osrs_cache::Result<ObjectScene> {
    let t0 = std::time::Instant::now();
    let square = osrs_cache::squares::SQUARE;

    let mut load_model = model_loader(cache, model_cache);

    let mut scene = ObjectScene::default();
    if with_info {
        scene.left_click = defs.values().filter_map(|d| Some((d.id, d.left_click()?.to_owned()))).collect();
        scene.names = defs.values().filter(|d| !d.name.is_empty()).map(|d| (d.id, d.name.clone())).collect();
    }
    let mut skipped = 0usize;
    // wall decorations stand off from the wall on their tile by that wall's decor displacement
    let mut wall_displacement: HashMap<(u8, i32, i32), u8> = HashMap::new();
    for (gx, gz, p) in &placed.placements {
        if p.kind <= 3 {
            if let Some(d) = defs.get(&p.id).and_then(|d| d.decor_displacement) {
                wall_displacement.insert((p.plane, *gx, *gz), d);
            }
        }
    }

    let (ox, oz) = window.unwrap_or((0, 0));
    for (gx, gz, p) in &placed.placements {
        if window.is_some() && (*gx < ox || *gx >= ox + square || *gz < oz || *gz >= oz + square) {
            continue;
        }
        let plane = p.plane as usize;
        let Some(def) = defs.get(&p.id) else { continue };
        let disp = (matches!(p.kind, 5 | 6 | 8))
            .then(|| wall_displacement.get(&(p.plane, *gx, *gz)).copied())
            .flatten();
        // 255 marks "no wall on this tile" (client default displacement)
        let key = (p.id, p.kind, p.orientation, disp.unwrap_or(255), 0);
        if !scene.meshes.contains_key(&key) {
            match build_loc(&mut load_model, def, p.kind, p.orientation, disp, tex) {
                Some(m) => {
                    scene.meshes.insert(key, m);
                }
                None => {
                    skipped += 1;
                    scene.meshes.insert(key, LocMesh::default());
                }
            }
        }
        if scene.meshes[&key].is_empty() {
            continue;
        }
        let (fx, fz) = footprint(def, p.orientation);
        let (x, z) = (*gx, *gz);
        // objects on a bridge tile were moved down a plane, but stand on the deck's heights
        let on_bridge = grid.tile(1, x as usize, z as usize).settings & 2 != 0;
        let hp = (plane + on_bridge as usize).min(3);
        let h = (grid.height_at(hp, x, z)
            + grid.height_at(hp, x + fx as i32, z)
            + grid.height_at(hp, x, z + fz as i32)
            + grid.height_at(hp, x + fx as i32, z + fz as i32))
            / 4;
        let mut key = key;
        if let Some(contoured) = def.contoured_ground {
            // this placement gets its own copy of the mesh, bent onto the ground under it
            let mut mesh = scene.meshes[&key].clone();
            let (ax, az) = (x as f32 + fx as f32 / 2.0, z as f32 + fz as f32 / 2.0);
            contour(&mut mesh, contoured, |dx, dz| (h as f32 - grid.surface_height(hp, ax + dx, az - dz)) / 128.0);
            key.4 = scene.instances.len() as u32 + 1;
            scene.meshes.insert(key, mesh);
        }
        scene.instances.push((
            key,
            Vec3::new((x - ox) as f32 + fx as f32 / 2.0, -(h as f32) / 128.0, -((z - oz) as f32 + fz as f32 / 2.0)),
            p.plane,
        ));
    }
    eprintln!(
        "objects: {} placements, {} unique meshes ({} had no model), loaded in {:.2?}",
        scene.instances.len(),
        scene.meshes.len(),
        skipped,
        t0.elapsed()
    );
    Ok(scene)
}

/// Decodes models by id from `cache`, keeping each in `models` so it is decoded once (`None`: it
/// does not decode).
pub(crate) fn model_loader<'a>(
    cache: &'a Cache,
    models: &'a mut HashMap<u32, Option<Rc<Model>>>,
) -> impl FnMut(u32) -> Option<Rc<Model>> + 'a {
    move |id| {
        models
            .entry(id)
            .or_insert_with(|| {
                let data = cache.read_group(idx::MODELS, id).ok()?;
                Model::decode(id, &data).ok().map(Rc::new)
            })
            .clone()
    }
}

/// Materials shared by everything spawned into one world, so that spawning map square after map
/// square does not make a copy of the terrain, object and texture materials for each.
#[derive(Resource, Default)]
pub struct MaterialCache {
    terrain: Option<Handle<StandardMaterial>>,
    opaque: Option<Handle<StandardMaterial>>,
    blend: Option<Handle<StandardMaterial>>,
    textured: HashMap<(u32, bool), Option<Handle<StandardMaterial>>>,
}

/// Spawn terrain and objects of `world`. Entities carry [`OnPlane`]; scrolling textures register in
/// `scrolling`.
pub fn spawn_world(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    scrolling: &mut ScrollingTextures,
    world: &WorldData,
) {
    spawn_parts(
        commands,
        meshes,
        materials,
        images,
        scrolling,
        &mut MaterialCache::default(),
        &world.textures,
        &world.terrain,
        &world.objects,
        Vec3::ZERO,
    );
}

/// Spawn one terrain mesh set and one set of objects with `offset` added to every position; returns
/// every entity made, so the whole piece can be despawned together.
#[allow(clippy::too_many_arguments)]
pub fn spawn_parts(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
    scrolling: &mut ScrollingTextures,
    shared: &mut MaterialCache,
    textures: &TextureStore,
    terrain: &[TerrainMesh],
    objects: &ObjectScene,
    offset: Vec3,
) -> Vec<Entity> {
    let mut spawned = Vec::new();
    // Terrain triangles have no consistent winding, so only object materials are back-face culled
    // (models are double-skinned where the client wants a two-sided surface).
    let mut material_for = |id: u32,
                            object: bool,
                            shared: &mut MaterialCache,
                            materials: &mut Assets<StandardMaterial>,
                            images: &mut Assets<Image>| {
        shared
            .textured
            .entry((id, object))
            .or_insert_with(|| {
                let t = textures.0.get(&id)?;
                let cull = if object { OBJECT_CULL } else { None };
                let handle = texture_material(t, cull, images, materials);
                if t.scroll_dir != 0 && t.scroll_speed != 0 {
                    scrolling.0.push((handle.clone(), t.scroll_dir, t.scroll_speed));
                }
                Some(handle)
            })
            .clone()
    };

    let terrain_mat = shared
        .terrain
        .get_or_insert_with(|| {
            materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, cull_mode: None, ..default() })
        })
        .clone();
    for (plane, t) in terrain.iter().enumerate() {
        if t.indices.is_empty() {
            continue;
        }
        let normals = vec![[0.0, 1.0, 0.0]; t.positions.len()];
        let mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, t.positions.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, t.colors.clone())
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
            .with_inserted_indices(Indices::U32(t.indices.clone()));
        spawned.push(
            commands
                .spawn((
                    Mesh3d(meshes.add(mesh)),
                    MeshMaterial3d(terrain_mat.clone()),
                    Transform::from_translation(offset),
                    OnPlane(plane as u8),
                ))
                .id(),
        );
        for (id, m) in &t.textured {
            let Some(mat) = material_for(*id, false, shared, materials, images) else { continue };
            spawned.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(to_bevy_mesh(m))),
                        MeshMaterial3d(mat),
                        Transform::from_translation(offset),
                        OnPlane(plane as u8),
                    ))
                    .id(),
            );
        }
    }

    let opaque_mat = shared
        .opaque
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                cull_mode: OBJECT_CULL,
                ..default()
            })
        })
        .clone();
    let blend_mat = shared
        .blend
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                unlit: true,
                cull_mode: OBJECT_CULL,
                alpha_mode: AlphaMode::Blend,
                ..default()
            })
        })
        .clone();
    type Handles = (Option<Handle<Mesh>>, Vec<(Handle<StandardMaterial>, Handle<Mesh>)>);
    let mut handles: HashMap<ObjectKey, Handles> = HashMap::new();
    for (key, lm) in &objects.meshes {
        let o = (!lm.opaque.is_empty()).then(|| meshes.add(to_bevy_mesh(&lm.opaque)));
        let mut tx = Vec::new();
        for (id, m) in &lm.textured {
            if let Some(mat) = material_for(*id, true, shared, materials, images) {
                tx.push((mat, meshes.add(to_bevy_mesh(m))));
            }
        }
        handles.insert(*key, (o, tx));
    }
    for (key, pos, plane) in &objects.instances {
        let pos = *pos + offset;
        let placed = PlacedLoc { id: key.0, tile: tile_under(pos) };
        let (o, tx) = &handles[key];
        for (mat, h) in tx {
            spawned.push(
                commands
                    .spawn((
                        Mesh3d(h.clone()),
                        MeshMaterial3d(mat.clone()),
                        Transform::from_translation(pos),
                        OnPlane(*plane),
                        placed,
                    ))
                    .id(),
            );
        }
        if let Some(h) = o {
            spawned.push(
                commands
                    .spawn((
                        Mesh3d(h.clone()),
                        MeshMaterial3d(opaque_mat.clone()),
                        Transform::from_translation(pos),
                        OnPlane(*plane),
                        placed,
                    ))
                    .id(),
            );
        }
        // translucent meshes are built per instance (they are depth-sorted individually)
        let tm = &objects.meshes[key].translucent;
        if !tm.is_empty() {
            let faces = tm
                .indices
                .chunks_exact(3)
                .map(|f| {
                    let p = |i: u32| Vec3::from(tm.positions[i as usize]);
                    ((p(f[0]) + p(f[1]) + p(f[2])) / 3.0, [f[0], f[1], f[2]])
                })
                .collect();
            spawned.push(
                commands
                    .spawn((
                        Mesh3d(meshes.add(to_bevy_mesh(tm))),
                        MeshMaterial3d(blend_mat.clone()),
                        Transform::from_translation(pos),
                        OnPlane(*plane),
                        SortedFaces { faces, origin: pos, last_eye: None },
                        placed,
                    ))
                    .id(),
            );
        }
    }
    spawned
}

fn scroll_textures(
    time: Res<Time>,
    scrolling: Res<ScrollingTextures>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let t = time.elapsed_secs();
    for (handle, dir, speed) in &scrolling.0 {
        let amount = t * *speed as f32 * SCROLL_PER_SPEED;
        // direction 1 is how fountain and waterfall water (textures 17, 24, 34, 40, 56, 57, 59)
        // scrolls, and it must fall: in our UVs that is toward -v
        let offset = match dir {
            1 => Vec2::new(0.0, -amount),
            2 => Vec2::new(amount, 0.0),
            3 => Vec2::new(0.0, amount),
            _ => Vec2::new(-amount, 0.0),
        };
        if let Some(mut m) = materials.get_mut(handle) {
            m.uv_transform = bevy::math::Affine2::from_translation(offset);
        }
    }
}

/// Re-order each translucent object's faces far-to-near when the camera has moved.
fn sort_translucent(
    cams: Query<&Transform, With<Camera3d>>,
    mut things: Query<(&Mesh3d, &mut SortedFaces, &ViewVisibility)>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Ok(cam) = cams.single() else { return };
    let eye = cam.translation;
    for (mesh, mut sorted, visible) in &mut things {
        if !visible.get() && sorted.last_eye.is_some() {
            continue;
        }
        if sorted.last_eye.is_some_and(|e| e.distance_squared(eye) < 0.05) {
            continue;
        }
        sorted.last_eye = Some(eye);
        let local = eye - sorted.origin;
        let mut order: Vec<(f32, [u32; 3])> =
            sorted.faces.iter().map(|(c, f)| (c.distance_squared(local), *f)).collect();
        order.sort_by(|a, b| b.0.total_cmp(&a.0));
        if let Some(mut m) = meshes.get_mut(&mesh.0) {
            m.insert_indices(Indices::U32(order.into_iter().flat_map(|(_, f)| f).collect()));
        }
    }
}

/// Marker for the tile-boundary line mesh so apps can toggle it.
#[derive(Component)]
pub struct TileGrid;

/// A line mesh along every tile boundary of `plane`, following the terrain heights and lifted
/// slightly so it is not swallowed by the ground.
pub fn tile_grid_mesh(world: &WorldData, plane: usize) -> Mesh {
    grid_lines_mesh(world.grid.width as u32, world.grid.depth as u32, |x, z| {
        world.grid.height_at(plane, x as i32, z as i32)
    })
}

/// The tile grid of one map square, each tile's outline at the height it is drawn at (a bridge
/// deck's from the plane above). Positions are relative to the square's corner.
pub fn square_tile_grid_mesh(data: &crate::streaming::SquareData) -> Mesh {
    const LIFT: f32 = 0.03;
    let side = osrs_cache::squares::SQUARE as usize;
    let stride = side + 1;
    let mut positions = Vec::with_capacity(side * side * 4);
    let mut indices = Vec::with_capacity(side * side * 8);
    for x in 0..side {
        for z in 0..side {
            let heights = if data.bridges[x * side + z] { &data.deck_heights } else { &data.heights };
            let base = positions.len() as u32;
            for (dx, dz) in [(0, 0), (1, 0), (1, 1), (0, 1)] {
                let h = heights[(x + dx) * stride + z + dz] as f32;
                positions.push([(x + dx) as f32, -h / 128.0 + LIFT, -((z + dz) as f32)]);
            }
            indices.extend([base, base + 1, base + 1, base + 2, base + 2, base + 3, base + 3, base]);
        }
    }
    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
}

/// Lines along every boundary of a `w` x `d` block of tiles, at corner heights `height(x, z)`
/// (client units, negative up).
fn grid_lines_mesh(w: u32, d: u32, height: impl Fn(u32, u32) -> i32) -> Mesh {
    /// Tiles above the terrain surface; keeps the lines clear of z-fighting.
    const LIFT: f32 = 0.03;
    let stride = w + 1;
    let mut positions = Vec::with_capacity((stride * (d + 1)) as usize);
    for z in 0..=d {
        for x in 0..=w {
            positions.push([x as f32, -(height(x, z) as f32) / 128.0 + LIFT, -(z as f32)]);
        }
    }
    let at = |x: u32, z: u32| z * stride + x;
    let mut indices = Vec::new();
    for z in 0..=d {
        for x in 0..w {
            indices.extend([at(x, z), at(x + 1, z)]);
        }
    }
    for z in 0..d {
        for x in 0..=w {
            indices.extend([at(x, z), at(x, z + 1)]);
        }
    }
    Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_indices(Indices::U32(indices))
}

/// Spawn the tile grid of `plane` in thin black lines.
pub fn spawn_tile_grid(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    world: &WorldData,
    plane: usize,
) -> Entity {
    let material = materials.add(StandardMaterial {
        base_color: Color::BLACK,
        unlit: true,
        ..default()
    });
    commands
        .spawn((
            Mesh3d(meshes.add(tile_grid_mesh(world, plane))),
            MeshMaterial3d(material),
            Transform::default(),
            TileGrid,
        ))
        .id()
}
