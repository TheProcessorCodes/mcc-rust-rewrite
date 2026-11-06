//! Button hints that follow the player's controls: a `KeyHint` text shows
//! its controller button (LB, RB, ...) or its keyboard key (Q, E, ...),
//! whichever was pressed last. Before either is pressed it shows the button
//! when a gamepad is connected, and with no gamepad always the key.

use bevy::prelude::*;

/// A hint's two labels: the controller button and the keyboard key.
#[derive(Component)]
pub struct KeyHint {
    pub pad: &'static str,
    pub keys: &'static str,
}

impl KeyHint {
    /// The label for when a gamepad is (or is not) connected.
    pub fn label(&self, pad: bool) -> &'static str {
        if pad { self.pad } else { self.keys }
    }
}

pub struct KeyHintPlugin;

impl Plugin for KeyHintPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, show);
    }
}

fn show(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut last_pad: Local<Option<bool>>,
    mut hints: Query<(&KeyHint, &mut Text)>,
) {
    if gamepads.iter().any(|p| p.get_just_pressed().next().is_some()) {
        *last_pad = Some(true);
    } else if keys.get_just_pressed().next().is_some() {
        *last_pad = Some(false);
    }
    let pad = !gamepads.is_empty() && last_pad.unwrap_or(true);
    for (hint, mut text) in &mut hints {
        let label = hint.label(pad);
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}
