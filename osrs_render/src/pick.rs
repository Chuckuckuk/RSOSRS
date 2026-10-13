//! Mouse picking: turns a camera ray into the ground tile under it, or the interactive object
//! standing in front of that ground.
//!
//! Ground uses the terrain's corner heights (bilinear within a tile). Objects are tested
//! triangle by triangle against their lit meshes, but only objects whose left-click option is a
//! real action: in the game a scenery piece without one (a fence, a rug) has no click box and does
//! not take clicks, and nor does one whose default option is just "Walk here".

use crate::world::{ObjectScene, WorldData};
use bevy::prelude::*;
use osrs_cache::scene::{LocMesh, MeshData};
use std::collections::HashMap;
use std::sync::Arc;

use crate::world::ObjectKey;

/// What a click landed on.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    /// Ground tile in the world's tile coordinates (for a streamed world, the world tile; for a
    /// block loaded whole, relative to its south-west corner).
    Ground { x: i32, z: i32 },
    /// The first object along the click whose left-click option is not walking, nearer than the
    /// ground behind it, with that option ("Open", "Chop down"...).
    Object { id: u32, name: String, action: String },
    /// The ray never reached loaded ground.
    Nothing,
}

/// Something with a terrain surface a ray can hit.
pub trait GroundSurface {
    /// Bevy y of the terrain surface at a point (tile units, bilinear between tile corners), or
    /// `None` where no terrain is loaded.
    fn surface_y(&self, plane: usize, x: f32, z: f32) -> Option<f32>;
}

impl GroundSurface for WorldData {
    fn surface_y(&self, plane: usize, x: f32, z: f32) -> Option<f32> {
        WorldData::surface_y(self, plane, x, z)
    }
}

struct Target {
    /// Who placed it (a map square), so a square's targets can be dropped together.
    owner: (i32, i32),
    scene: Arc<ObjectScene>,
    key: ObjectKey,
    id: u32,
    /// Whether its left-click option is a real action, so a left-click lands on it. Other named
    /// objects (a fence, a rug) are still there to be examined from the right-click menu.
    intercepts: bool,
    origin: Vec3,
    min: Vec3,
    max: Vec3,
}

/// Prebuilt click boxes of the interactive objects on one plane.
pub struct Picker {
    plane: u8,
    targets: Vec<Target>,
    left_click: HashMap<u32, String>,
    names: HashMap<u32, String>,
}

impl Picker {
    /// Click boxes of everything in a world that was loaded whole.
    pub fn new(world: &WorldData, plane: u8) -> Picker {
        let mut picker = Picker::streamed(plane, world.objects.left_click.clone(), world.objects.names.clone());
        picker.add(OWNER_WHOLE_WORLD, &world.objects, Vec3::ZERO);
        picker
    }

    /// An empty picker for a world that is streamed in: `left_click` maps the ids of objects that
    /// have a left-click option to it, and `names` gives every object's name, for the whole game
    /// rather than for any one square.
    pub fn streamed(plane: u8, left_click: HashMap<u32, String>, names: HashMap<u32, String>) -> Picker {
        Picker { plane, targets: Vec::new(), left_click, names }
    }

    /// Adds the click boxes of `objects` (positions as the scene spawns them, plus `offset`),
    /// tagged `owner` so [`Picker::remove`] can take them out again.
    pub fn add(&mut self, owner: (i32, i32), objects: &Arc<ObjectScene>, offset: Vec3) {
        let mut bounds: HashMap<ObjectKey, (Vec3, Vec3)> = HashMap::new();
        for (key, origin, p) in &objects.instances {
            // only named objects have a menu at all
            if *p != self.plane || !self.names.contains_key(&key.0) {
                continue;
            }
            let (min, max) = *bounds.entry(*key).or_insert_with(|| {
                let mut lo = Vec3::splat(f32::MAX);
                let mut hi = Vec3::splat(f32::MIN);
                for mesh in mesh_parts(&objects.meshes[key]) {
                    for v in &mesh.positions {
                        lo = lo.min(Vec3::from(*v));
                        hi = hi.max(Vec3::from(*v));
                    }
                }
                (lo, hi)
            });
            if min.x <= max.x {
                let origin = *origin + offset;
                self.targets.push(Target {
                    owner,
                    scene: Arc::clone(objects),
                    key: *key,
                    id: key.0,
                    intercepts: self.left_click.contains_key(&key.0),
                    origin,
                    min: min + origin,
                    max: max + origin,
                });
            }
        }
    }

    /// Drops the click boxes added under `owner`.
    pub fn remove(&mut self, owner: (i32, i32)) {
        self.targets.retain(|t| t.owner != owner);
    }

    /// How many placed objects take a left-click (have a click box in the game's sense).
    pub fn pick_targets(&self) -> usize {
        self.targets.iter().filter(|t| t.intercepts).count()
    }

    /// Walks the ray from the camera: the first object with a left-click option that it meets
    /// before the ground is what the click selects; objects without one (or whose option is just
    /// "Walk here") are passed through. If nothing intercepts it, the tile where it ends.
    pub fn pick(&self, ground: &impl GroundSurface, ray: Ray3d) -> Pick {
        match self.pick_all(ground, ray) {
            None => Pick::Nothing,
            Some(Click { object: Some((id, name, action)), .. }) => Pick::Object { id, name, action },
            Some(Click { tile: (x, z), object: None, .. }) => Pick::Ground { x, z },
        }
    }

    /// What a click would select, the nearest named object in its path (for "Examine", whether or
    /// not it takes left-clicks) and the ground tile at the end of the path, which is where
    /// "Walk here" goes even when an object is in front of it. `None` if the ray never reaches
    /// loaded ground.
    pub fn pick_all(&self, ground: &impl GroundSurface, ray: Ray3d) -> Option<Click> {
        let origin = ray.origin;
        let dir = *ray.direction;
        let hit = pick_ground(ground, self.plane as usize, origin, dir)?;
        let ground_t = (hit - origin).dot(dir);

        // nearest hit among objects that take a left-click, and among all named objects
        let mut intercepting: Option<(f32, u32)> = None;
        let mut any: Option<(f32, u32)> = None;
        for t in &self.targets {
            let Some(enter) = ray_aabb(origin, dir, t.min, t.max) else { continue };
            let beaten = |best: Option<(f32, u32)>| best.is_some_and(|(d, _)| enter > d);
            if enter > ground_t || (beaten(any) && (!t.intercepts || beaten(intercepting))) {
                continue;
            }
            for mesh in mesh_parts(&t.scene.meshes[&t.key]) {
                for tri in mesh.indices.chunks_exact(3) {
                    let v = |i: u32| Vec3::from(mesh.positions[i as usize]) + t.origin;
                    if let Some(hit) = ray_triangle(origin, dir, v(tri[0]), v(tri[1]), v(tri[2])) {
                        if hit >= ground_t {
                            continue;
                        }
                        if any.is_none_or(|(d, _)| hit < d) {
                            any = Some((hit, t.id));
                        }
                        if t.intercepts && intercepting.is_none_or(|(d, _)| hit < d) {
                            intercepting = Some((hit, t.id));
                        }
                    }
                }
            }
        }
        let name_of = |id: u32| self.names.get(&id).cloned().unwrap_or_default();
        let object = intercepting.map(|(_, id)| {
            let action = self.left_click.get(&id).cloned().unwrap_or_default();
            (id, name_of(id), action)
        });
        let examinable = any.map(|(_, id)| (id, name_of(id)));
        Some(Click {
            tile: (hit.x.floor() as i32, (-hit.z).floor() as i32),
            object,
            examinable,
            ground_distance: ground_t,
            object_distance: intercepting.map(|(d, _)| d),
            examinable_distance: any.map(|(d, _)| d),
        })
    }
}

/// Everything under a click: the ground tile its path ends on, the object that intercepts it on the
/// way (id, name, left-click option), and the nearest named object in the way (id, name), which
/// can be one that does not intercept.
#[derive(Debug, Clone, PartialEq)]
pub struct Click {
    pub tile: (i32, i32),
    pub object: Option<(u32, String, String)>,
    pub examinable: Option<(u32, String)>,
    /// How far along the ray (world units) the ground, the intercepting object and the
    /// examinable object are, so other things (NPCs) can be ordered against them.
    pub ground_distance: f32,
    pub object_distance: Option<f32>,
    pub examinable_distance: Option<f32>,
}

/// Owner tag of the click boxes of a world that was loaded as one piece.
const OWNER_WHOLE_WORLD: (i32, i32) = (i32::MIN, i32::MIN);

fn mesh_parts(loc: &LocMesh) -> impl Iterator<Item = &MeshData> {
    [&loc.opaque, &loc.translucent].into_iter().chain(loc.textured.values())
}

/// First point where the ray goes below the terrain surface.
fn pick_ground(ground: &impl GroundSurface, plane: usize, origin: Vec3, dir: Vec3) -> Option<Vec3> {
    const STEP: f32 = 0.25;
    const MAX_DISTANCE: f32 = 400.0;
    let below = |t: f32| -> Option<bool> {
        let p = origin + dir * t;
        ground.surface_y(plane, p.x, -p.z).map(|h| p.y <= h)
    };
    let mut t = 0.0;
    let mut last_above = None::<f32>;
    while t < MAX_DISTANCE {
        match below(t) {
            Some(false) => last_above = Some(t),
            Some(true) => {
                // crossed the surface since the last step that was above it
                let (mut lo, mut hi) = (last_above?, t);
                for _ in 0..24 {
                    let mid = (lo + hi) / 2.0;
                    if below(mid) == Some(true) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(origin + dir * hi);
            }
            None => last_above = None,
        }
        t += STEP;
    }
    None
}

/// Distance along the ray at which it enters the box.
pub fn ray_aabb(origin: Vec3, dir: Vec3, min: Vec3, max: Vec3) -> Option<f32> {
    let inv = Vec3::ONE / dir;
    let (t1, t2) = ((min - origin) * inv, (max - origin) * inv);
    let (near, far) = (t1.min(t2).max_element(), t1.max(t2).min_element());
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

/// Moller-Trumbore, two-sided. Distance along the ray, if it hits.
fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    let (e1, e2) = (b - a, c - a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 1e-4).then_some(t)
}
