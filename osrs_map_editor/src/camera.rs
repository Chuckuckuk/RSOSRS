//! The editor's camera: `osrs_render`'s fly camera, looking while the right mouse button is held
//! (begun over the world), flying unless a text field has the keys or Ctrl is held (shortcuts).

use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};
pub use osrs_render::fly::FlyCamera;
use osrs_render::fly::grab_cursor;

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
    commands.spawn((Camera3d::default(), look, FlyCamera::facing(&look, 20.0)));
}

/// Moves the camera to look at tile (`x`, `z`).
pub fn jump_to(transform: &mut Transform, camera: &mut FlyCamera, x: i32, z: i32) {
    *transform = looking_at(x, z);
    camera.face(transform);
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
        grab_cursor(&mut cursor, *looking);
    }
    if *looking {
        cam.look(&mut tf, motion.delta);
    }
    if hover.in_world {
        cam.zoom(scroll.delta.y, 500.0);
    }
    // typing in a text field is not flying, nor is Ctrl held for a shortcut
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if ctrl || contexts.ctx_mut().is_ok_and(|ctx| ctx.egui_wants_keyboard_input()) {
        return;
    }
    cam.fly(&mut tf, &keys, time.delta_secs());
}
