//! A free-flying camera, shared by the viewer and the map editor: WASD to move, Q/E down and up,
//! the mouse to look, Shift for speed, the wheel to change it. Each program decides when the
//! mouse looks and the keys fly (the editor not while typing, or over its panels), then calls
//! these.

use bevy::prelude::*;

#[derive(Component)]
pub struct FlyCamera {
    /// Tiles a second.
    pub speed: f32,
    yaw: f32,
    pitch: f32,
}

impl FlyCamera {
    /// A camera at `transform`'s facing.
    pub fn facing(transform: &Transform, speed: f32) -> FlyCamera {
        let mut camera = FlyCamera { speed, yaw: 0.0, pitch: 0.0 };
        camera.face(transform);
        camera
    }

    /// Takes up `transform`'s facing (after the camera was moved by other means).
    pub fn face(&mut self, transform: &Transform) {
        let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        self.yaw = yaw;
        self.pitch = pitch;
    }

    /// Turns by a mouse movement of `delta` pixels.
    pub fn look(&mut self, transform: &mut Transform, delta: Vec2) {
        self.yaw -= delta.x * 0.003;
        self.pitch = (self.pitch - delta.y * 0.003).clamp(-1.55, 1.55);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0);
    }

    /// Speeds up or slows down by a wheel turn of `scroll`, up to `max` tiles a second.
    pub fn zoom(&mut self, scroll: f32, max: f32) {
        if scroll != 0.0 {
            self.speed = (self.speed * (1.0 + scroll * 0.1)).clamp(0.5, max);
        }
    }

    /// Moves by the held keys for `dt` seconds.
    pub fn fly(&self, transform: &mut Transform, keys: &ButtonInput<KeyCode>, dt: f32) {
        let (fwd, right) = (*transform.forward(), *transform.right());
        let mut dir = Vec3::ZERO;
        for (key, step) in [
            (KeyCode::KeyW, fwd),
            (KeyCode::KeyS, -fwd),
            (KeyCode::KeyD, right),
            (KeyCode::KeyA, -right),
            (KeyCode::KeyE, Vec3::Y),
            (KeyCode::KeyQ, -Vec3::Y),
        ] {
            if keys.pressed(key) {
                dir += step;
            }
        }
        let boost = if keys.pressed(KeyCode::ShiftLeft) { 4.0 } else { 1.0 };
        transform.translation += dir.normalize_or_zero() * self.speed * boost * dt;
    }
}

/// Locks and hides the cursor while the mouse is looking.
pub fn grab_cursor(cursor: &mut bevy::window::CursorOptions, looking: bool) {
    use bevy::window::CursorGrabMode;
    cursor.grab_mode = if looking { CursorGrabMode::Locked } else { CursorGrabMode::None };
    cursor.visible = !looking;
}
