//! Button hints that follow the player's controls: a `KeyHint` text shows
//! its controller wording (LB, RB, Start, ...) or its keyboard wording (Q,
//! E, Esc, ...), whichever was pressed last. Before either is pressed it
//! shows the controller one when a gamepad is connected, and with no
//! gamepad always the keyboard one.
//!
//! `MCC_HINTS=keys` or `MCC_HINTS=pad` (dev, for headless shots) fixes the
//! wording.
//!
//! `badge` draws a bar button's badge: the pad's round coloured button with
//! its letter, or a key cap with the key's name.

use bevy::prelude::*;

/// A hint's two wordings: for the controller and for the keyboard.
#[derive(Component)]
pub struct KeyHint {
    pub pad: &'static str,
    pub keys: &'static str,
}

impl KeyHint {
    /// The wording for when the controller (or the keyboard) is in use.
    pub fn label(&self, pad: bool) -> &'static str {
        if pad { self.pad } else { self.keys }
    }
}

/// A badge (`badge`): round in the pad's colour, or a light key cap.
#[derive(Component)]
pub struct KeyBadge(pub Color);

const SIZE: f32 = 26.0;
const KEY_CAP: Color = Color::srgb(0.82, 0.85, 0.90);

/// A text with its controller and keyboard wordings (keyboard to start).
pub fn hint(pad: &'static str, keys: &'static str, size: f32, colour: Color) -> impl Bundle {
    (KeyHint { pad, keys }, Text::new(keys), TextFont::from_font_size(size), TextColor(colour))
}

/// A bar button's badge: the pad's `letter` on a round `colour` button, or
/// `key` on a key cap.
pub fn badge(parent: &mut ChildSpawnerCommands, letter: &'static str, key: &'static str, colour: Color) {
    parent
        .spawn((
            KeyBadge(colour),
            Node {
                min_width: Val::Px(SIZE),
                height: Val::Px(SIZE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(KEY_CAP),
        ))
        .with_child(hint(letter, key, 15.0, Color::BLACK));
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
    mut was_pad: Local<Option<bool>>,
    mut hints: Query<(&KeyHint, &mut Text)>,
    mut badges: Query<(Ref<KeyBadge>, &mut Node, &mut BackgroundColor)>,
) {
    if gamepads.iter().any(|p| p.get_just_pressed().next().is_some()) {
        *last_pad = Some(true);
    } else if keys.get_just_pressed().next().is_some() {
        *last_pad = Some(false);
    }
    let pad = match std::env::var("MCC_HINTS").as_deref() {
        Ok("keys") => false,
        Ok("pad") => true,
        _ => !gamepads.is_empty() && last_pad.unwrap_or(true),
    };
    for (hint, mut text) in &mut hints {
        let label = hint.label(pad);
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
    let switched = *was_pad != Some(pad);
    *was_pad = Some(pad);
    for (badge, mut node, mut background) in &mut badges {
        if !switched && !badge.is_added() {
            continue;
        }
        if pad {
            node.width = Val::Px(SIZE);
            node.padding = UiRect::ZERO;
            node.border_radius = BorderRadius::MAX;
            background.0 = badge.0;
        } else {
            node.width = Val::Auto;
            node.padding = UiRect::horizontal(Val::Px(8.0));
            node.border_radius = BorderRadius::all(Val::Px(4.0));
            background.0 = KEY_CAP;
        }
    }
}
