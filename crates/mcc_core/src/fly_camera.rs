//! A free-flying camera: the noclip of goal 2.
//!
//! Keyboard: WASD moves, Space / C (or E / Q) goes up / down, Shift is fast,
//! Alt is slow. The mouse looks around whenever the window has focus;
//! Escape (the pause menu, `pause.rs`) gives the cursor back and a click
//! takes it again. P prints the camera's place in Halo units. Gamepad: left
//! stick moves, right stick looks, right and left triggers go up and down,
//! the A button is fast.

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::pause::running;
use crate::coords::{to_halo, to_halo_dir};
use crate::skate::Skating;

pub struct FlyCameraPlugin;

impl Plugin for FlyCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (look, grab_cursor, fly, report).chain().run_if(running).run_if(crate::map::ready));
    }
}

#[derive(Component)]
pub struct FlyCamera {
    pub yaw: f32,
    pub pitch: f32,
    /// Metres per second at normal speed.
    pub speed: f32,
    pub mouse_sensitivity: f32,
    pub stick_sensitivity: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        FlyCamera {
            yaw: 0.0,
            pitch: 0.0,
            speed: 25.0,
            mouse_sensitivity: 0.0022,
            stick_sensitivity: 2.4,
        }
    }
}

impl FlyCamera {
    /// A camera at `position` (renderer frame, metres) looking at `target`.
    pub fn looking_at(position: Vec3, target: Vec3) -> (FlyCamera, Transform) {
        let d = (target - position).normalize_or(Vec3::NEG_Z);
        let yaw = (-d.x).atan2(-d.z);
        let pitch = d.y.asin();
        let cam = FlyCamera {
            yaw,
            pitch,
            ..default()
        };
        let t = Transform::from_translation(position).with_rotation(cam.rotation());
        (cam, t)
    }

    pub fn rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

fn cursor_grabbed(cursor: &CursorOptions) -> bool {
    cursor.grab_mode != CursorGrabMode::None
}

fn grab_cursor(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut windows: Query<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    // An overlay menu (the bike's tuning menu) wants the pointer.
    overlay: Option<Res<crate::pause::Overlay>>,
    // Whether Escape has handed the cursor back; a click takes it again.
    mut released: Local<bool>,
    mut was_focused: Local<bool>,
) {
    let Ok((window, mut cursor)) = windows.single_mut() else {
        return;
    };
    // Coming back to the window (alt-tab, click) always takes the mouse.
    if window.focused && !*was_focused {
        *released = false;
    }
    *was_focused = window.focused;
    if keys.just_pressed(KeyCode::Escape) {
        *released = true;
    }
    if mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right) {
        *released = false;
    }
    let want = window.focused && !*released && !overlay.is_some_and(|o| o.0);
    let mode = if want {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != mode {
        cursor.grab_mode = mode;
        cursor.visible = !want;
    }
}

fn look(
    motion: Res<AccumulatedMouseMotion>,
    time: Res<Time>,
    gamepads: Query<&Gamepad>,
    windows: Query<&CursorOptions, With<PrimaryWindow>>,
    mut cameras: Query<(&mut FlyCamera, &mut Transform), Without<Skating>>,
) {
    let grabbed = windows.single().is_ok_and(cursor_grabbed);
    for (mut cam, mut transform) in &mut cameras {
        let mut turn = Vec2::ZERO;
        if grabbed {
            turn -= motion.delta * cam.mouse_sensitivity;
        }
        for pad in &gamepads {
            let x = pad.get(GamepadAxis::RightStickX).unwrap_or(0.0);
            let y = pad.get(GamepadAxis::RightStickY).unwrap_or(0.0);
            let stick = Vec2::new(x, y);
            if stick.length() > 0.15 {
                turn += Vec2::new(-stick.x, stick.y) * cam.stick_sensitivity * time.delta_secs();
            }
        }
        if turn != Vec2::ZERO {
            cam.yaw += turn.x;
            cam.pitch = (cam.pitch + turn.y).clamp(-1.55, 1.55);
            transform.rotation = cam.rotation();
        }
    }
}

fn fly(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    gamepads: Query<&Gamepad>,
    mut cameras: Query<(&FlyCamera, &mut Transform), Without<Skating>>,
) {
    let axis = |neg: KeyCode, pos: KeyCode| {
        (keys.pressed(pos) as i32 - keys.pressed(neg) as i32) as f32
    };
    let mut wish = Vec3::new(
        axis(KeyCode::KeyA, KeyCode::KeyD),
        axis(KeyCode::KeyC, KeyCode::Space) + axis(KeyCode::KeyQ, KeyCode::KeyE),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    let mut fast = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let slow = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    for pad in &gamepads {
        let x = pad.get(GamepadAxis::LeftStickX).unwrap_or(0.0);
        let y = pad.get(GamepadAxis::LeftStickY).unwrap_or(0.0);
        if Vec2::new(x, y).length() > 0.15 {
            wish.x += x;
            wish.z -= y;
        }
        wish.y += pad.get(GamepadButton::RightTrigger2).unwrap_or(0.0)
            - pad.get(GamepadButton::LeftTrigger2).unwrap_or(0.0);
        fast |= pad.pressed(GamepadButton::South);
    }
    if wish == Vec3::ZERO {
        return;
    }
    let wish = wish.clamp_length_max(1.0);
    for (cam, mut transform) in &mut cameras {
        let speed = cam.speed * if fast { 6.0 } else if slow { 0.15 } else { 1.0 };
        let forward = transform.forward().as_vec3();
        let right = transform.right().as_vec3();
        let step = (right * wish.x + Vec3::Y * wish.y - forward * wish.z) * speed * time.delta_secs();
        transform.translation += step;
    }
}

fn report(keys: Res<ButtonInput<KeyCode>>, cameras: Query<&Transform, With<FlyCamera>>) {
    if !keys.just_pressed(KeyCode::KeyP) {
        return;
    }
    for t in &cameras {
        let p = to_halo(t.translation);
        let f = to_halo_dir(t.forward().as_vec3());
        info!(
            "camera: halo position [{:.3}, {:.3}, {:.3}] wu, facing [{:.3}, {:.3}, {:.3}]; renderer position [{:.2}, {:.2}, {:.2}] m",
            p.x, p.y, p.z, f.x, f.y, f.z, t.translation.x, t.translation.y, t.translation.z
        );
    }
}
