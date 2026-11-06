//! Built-in controls (optional): put [`ReflexDefaultControls`] on a bike and its [`ReflexInput`] is written from
//! the keyboard and every connected gamepad each frame, with Reflex's pad layout and deadzones. Games with their
//! own input write `ReflexInput` themselves instead.
//!
//! | pad | keyboard (default [`ReflexInputMap`]) | |
//! |---|---|---|
//! | RT | W | throttle |
//! | LT | S | both brakes |
//! | left stick X | A / D | steer |
//! | right stick | arrow keys | rider lean (x: left / right, y: forward / back) |
//! | A (south) | X | brake / reverse button |
//! | RB | E | clutch |
//! | Back (select) | Backspace | respawn |

use bevy::input::gamepad::{Gamepad, GamepadAxis, GamepadButton};
use bevy::prelude::*;

use crate::ReflexInput;

/// Marks a bike whose [`ReflexInput`] the built-in controls write.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ReflexDefaultControls;

/// Keyboard keys of the built-in controls.
#[derive(Resource, Clone, Debug)]
pub struct ReflexInputMap {
    pub throttle: KeyCode,
    pub brake: KeyCode,
    pub steer_left: KeyCode,
    pub steer_right: KeyCode,
    pub lean_left: KeyCode,
    pub lean_right: KeyCode,
    pub lean_forward: KeyCode,
    pub lean_back: KeyCode,
    pub reverse: KeyCode,
    pub clutch: KeyCode,
    pub respawn: KeyCode,
}

impl Default for ReflexInputMap {
    fn default() -> Self {
        ReflexInputMap {
            throttle: KeyCode::KeyW,
            brake: KeyCode::KeyS,
            steer_left: KeyCode::KeyA,
            steer_right: KeyCode::KeyD,
            lean_left: KeyCode::ArrowLeft,
            lean_right: KeyCode::ArrowRight,
            lean_forward: KeyCode::ArrowUp,
            lean_back: KeyCode::ArrowDown,
            reverse: KeyCode::KeyX,
            clutch: KeyCode::KeyE,
            respawn: KeyCode::Backspace,
        }
    }
}

/// XInput's stick deadzone as Reflex applies it: |v| <= 0.24 reads 0, the rest is rescaled to 0..1
/// (a stick at 0.6 arrives as 0.474).
pub fn stick_deadzone(v: f32) -> f32 {
    const DZ: f32 = 0.24;
    if v.abs() <= DZ {
        0.0
    } else {
        v.signum() * ((v.abs() - DZ) / (1.0 - DZ)).min(1.0)
    }
}

/// Reflex's trigger reading: under 30/255 reads 0, the rest as is.
pub fn trigger_deadzone(v: f32) -> f32 {
    if v < 30.0 / 255.0 {
        0.0
    } else {
        v.min(1.0)
    }
}

pub(crate) fn default_controls(
    keys: Option<Res<ButtonInput<KeyCode>>>,
    map: Res<ReflexInputMap>,
    pads: Query<&Gamepad>,
    mut bikes: Query<&mut ReflexInput, With<ReflexDefaultControls>>,
) {
    if bikes.is_empty() {
        return;
    }
    let key = |k: KeyCode| keys.as_ref().is_some_and(|keys| keys.pressed(k));
    let key_now = |k: KeyCode| keys.as_ref().is_some_and(|keys| keys.just_pressed(k));
    let axis = |a: f32, b: f32| a - b;
    let on = |k: KeyCode| if key(k) { 1.0 } else { 0.0 };
    let mut i = ReflexInput {
        throttle: on(map.throttle),
        brake: on(map.brake),
        steer: axis(on(map.steer_right), on(map.steer_left)),
        lean: Vec2::new(axis(on(map.lean_right), on(map.lean_left)), axis(on(map.lean_forward), on(map.lean_back))),
        reverse: key(map.reverse),
        clutch: key(map.clutch),
        respawn: key_now(map.respawn),
    };
    // pads add to the keyboard; the strongest value wins per input
    let max_abs = |a: f32, b: f32| if b.abs() > a.abs() { b } else { a };
    for pad in &pads {
        let ax = |a: GamepadAxis| pad.get(a).unwrap_or(0.0);
        let btn = |b: GamepadButton| pad.get(b).unwrap_or(0.0);
        i.throttle = i.throttle.max(trigger_deadzone(btn(GamepadButton::RightTrigger2)));
        i.brake = i.brake.max(trigger_deadzone(btn(GamepadButton::LeftTrigger2)));
        i.steer = max_abs(i.steer, stick_deadzone(ax(GamepadAxis::LeftStickX)));
        i.lean.x = max_abs(i.lean.x, stick_deadzone(ax(GamepadAxis::RightStickX)));
        i.lean.y = max_abs(i.lean.y, stick_deadzone(ax(GamepadAxis::RightStickY)));
        i.reverse |= pad.pressed(GamepadButton::South);
        i.clutch |= pad.pressed(GamepadButton::RightTrigger);
        i.respawn |= pad.just_pressed(GamepadButton::Select);
    }
    for mut input in &mut bikes {
        *input = i;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadzones_match_reflex() {
        assert_eq!(stick_deadzone(0.2), 0.0);
        assert!((stick_deadzone(0.6) - 0.4737).abs() < 1e-3);
        assert!((stick_deadzone(-1.0) + 1.0).abs() < 1e-6);
        assert_eq!(trigger_deadzone(0.1), 0.0);
        assert!((trigger_deadzone(0.5) - 0.5).abs() < 1e-6);
    }
}
