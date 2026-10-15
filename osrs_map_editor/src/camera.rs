//! A free-flying camera, as in `osrs_viewer`: WASD to move, Q/E down and up, hold the right mouse
//! button to look around, Shift for speed, the wheel to change it.

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

#[derive(Component)]
pub struct FlyCamera {
    pub speed: f32,
    yaw: f32,
    pitch: f32,
}

/// Whether every plane is drawn (roofs on), or only those up to the plane being edited.
#[derive(Resource)]
pub struct ViewState {
    pub roofs: bool,
}

/// The camera looking down at world tile (`x`, `z`) from a little south of it.
pub fn looking_at(x: i32, z: i32) -> Transform {
    let target = Vec3::new(x as f32 + 0.5, 0.0, -(z as f32 + 0.5));
    Transform::from_translation(target + Vec3::new(0.0, 28.0, 22.0)).looking_at(target, Vec3::Y)
}

pub fn spawn_camera(commands: &mut Commands, at: (i32, i32)) {
    let look = looking_at(at.0, at.1);
    let (yaw, pitch, _) = look.rotation.to_euler(EulerRot::YXZ);
    commands.spawn((Camera3d::default(), look, FlyCamera { speed: 20.0, yaw, pitch }));
}

/// Moves the camera to look at tile (`x`, `z`).
pub fn jump_to(transform: &mut Transform, camera: &mut FlyCamera, x: i32, z: i32) {
    *transform = looking_at(x, z);
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    camera.yaw = yaw;
    camera.pitch = pitch;
}

#[allow(clippy::too_many_arguments)]
pub fn fly_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    hover: Res<crate::world::Hover>,
    mut contexts: bevy_egui::EguiContexts,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut cams: Query<(&mut Transform, &mut FlyCamera)>,
    mut looking: Local<bool>,
) {
    let Ok((mut tf, mut cam)) = cams.single_mut() else { return };
    // looking starts only over the world, but keeps going over a panel once begun
    if buttons.just_pressed(MouseButton::Right) && hover.in_world {
        *looking = true;
    }
    if !buttons.pressed(MouseButton::Right) {
        *looking = false;
    }
    if let Ok(mut cursor) = windows.single_mut() {
        cursor.grab_mode = if *looking { CursorGrabMode::Locked } else { CursorGrabMode::None };
        cursor.visible = !*looking;
    }
    if *looking {
        cam.yaw -= motion.delta.x * 0.003;
        cam.pitch = (cam.pitch - motion.delta.y * 0.003).clamp(-1.55, 1.55);
        tf.rotation = Quat::from_euler(EulerRot::YXZ, cam.yaw, cam.pitch, 0.0);
    }
    if scroll.delta.y != 0.0 && hover.in_world {
        cam.speed = (cam.speed * (1.0 + scroll.delta.y * 0.1)).clamp(0.5, 500.0);
    }
    // typing in a text field is not flying
    if contexts.ctx_mut().is_ok_and(|ctx| ctx.egui_wants_keyboard_input()) {
        return;
    }
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl {
        return;
    }
    let mut dir = Vec3::ZERO;
    let fwd = *tf.forward();
    let right = *tf.right();
    if keys.pressed(KeyCode::KeyW) {
        dir += fwd;
    }
    if keys.pressed(KeyCode::KeyS) {
        dir -= fwd;
    }
    if keys.pressed(KeyCode::KeyD) {
        dir += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        dir -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        dir += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        dir -= Vec3::Y;
    }
    let boost = if keys.pressed(KeyCode::ShiftLeft) { 4.0 } else { 1.0 };
    tf.translation += dir.normalize_or_zero() * cam.speed * boost * time.delta_secs();
}
