//! Takes the roofs off when the view into a building crosses one, as the client does. Objects
//! that stand between the camera and the character stay: like the real game, geometry may block
//! the camera.
//!
//! The app sets [`OcclusionTarget`] each frame (the point the camera looks at); the system here
//! does the rest against the scene's [`OnPlane`] entities.

use crate::streaming::StreamedWorld;
use crate::world::OnPlane;
use bevy::prelude::*;

/// The client only walks the line of sight for roofs when the camera is not looking steeply
/// down (its pitch below 310 of 2048): from above, only the character's own tile counts.
const ROOF_WALK_MAX_ELEVATION: f32 = 0.95;

/// Where the camera is looking: the character, and the plane he stands on.
#[derive(Resource, Default)]
pub struct OcclusionTarget {
    pub point: Option<Vec3>,
    pub plane: u8,
}

pub struct OcclusionPlugin;

impl Plugin for OcclusionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OcclusionTarget>().add_systems(Update, remove_roofs);
    }
}

/// An entity of a plane above the character, hidden because the roofs are off.
#[derive(Component)]
struct RoofHidden;

/// The world tile under a render-space point.
fn tile_at(p: Vec3) -> (i32, i32) {
    (p.x.floor() as i32, (-p.z).floor() as i32)
}

/// Whether the roofs come off: the character's tile is roofed, or (looking level enough) a tile
/// on the line from the camera to him is, as in the client.
pub fn roofs_off(world: &StreamedWorld, eye: Vec3, target: Vec3) -> bool {
    let (tx, tz) = tile_at(target);
    if world.roofed(tx, tz) {
        return true;
    }
    let to_eye = eye - target;
    let elevation = to_eye.y.atan2(Vec2::new(to_eye.x, to_eye.z).length());
    if elevation >= ROOF_WALK_MAX_ELEVATION {
        return false;
    }
    let flat = Vec2::new(to_eye.x, to_eye.z).length();
    let steps = (flat * 2.0).ceil() as i32;
    (1..=steps).any(|i| {
        let (x, z) = tile_at(target + to_eye * (i as f32 / steps as f32));
        world.roofed(x, z)
    })
}

/// Hides everything above the character's plane while the roofs are off, and brings it back after.
fn remove_roofs(
    mut commands: Commands,
    target: Res<OcclusionTarget>,
    world: Res<StreamedWorld>,
    cams: Query<&Transform, With<Camera3d>>,
    mut applied: Local<Option<bool>>,
    mut things: Query<(Entity, &OnPlane, &mut Visibility, Has<RoofHidden>)>,
    spawned: Query<(), Added<OnPlane>>,
) {
    let (Ok(cam), Some(point)) = (cams.single(), target.point) else { return };
    let off = roofs_off(&world, cam.translation, point);
    if *applied == Some(off) && spawned.is_empty() {
        return;
    }
    *applied = Some(off);
    for (entity, plane, mut visibility, hidden) in &mut things {
        if off && plane.0 > target.plane {
            if !hidden && *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
                commands.entity(entity).insert(RoofHidden);
            }
        } else if hidden {
            *visibility = Visibility::Inherited;
            commands.entity(entity).remove::<RoofHidden>();
        }
    }
}

