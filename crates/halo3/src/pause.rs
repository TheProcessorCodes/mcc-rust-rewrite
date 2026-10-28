//! The pause menu: Escape, or Start on a pad, stops the game and lists the
//! controls for flying about (the skate and bike controls are not listed).
//! The same button carries on.
//!
//! Pausing stops virtual time, and the systems that read input or move
//! things run only `while running`, so nothing reacts to keys behind the
//! menu. Drawing goes on, so the frozen frame stays on screen.

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .add_systems(Startup, spawn_menu)
            .add_systems(PreUpdate, toggle.after(InputSystems));
    }
}

#[derive(Resource, Default)]
pub struct Paused(pub bool);

/// A run condition for systems that should stop behind the menu.
pub fn running(paused: Res<Paused>) -> bool {
    !paused.0
}

#[derive(Component)]
struct PauseMenu;

/// Free-roam controls: (keyboard, controller, what it does).
const CONTROLS: &[(&str, &str, &str)] = &[
    ("Esc", "Start", "pause / resume"),
    ("W A S D", "left stick", "move"),
    ("mouse (hold right button)", "right stick", "look around"),
    ("Tab", "", "mouselook on / off"),
    ("Space / E", "right trigger", "up"),
    ("C / Q", "left trigger", "down"),
    ("Shift (hold)", "A (hold)", "fast"),
    ("Alt (hold)", "", "slow"),
    ("J", "click both sticks", "enter / exit skate mode"),
    ("V", "", "show / hide collision"),
    ("Page Down / Page Up", "", "next / previous zone set"),
    ("Home", "", "load every BSP"),
    ("P", "", "print the camera position to the console"),
];

fn spawn_menu(mut commands: Commands) {
    let text = |s: &str, size: f32, colour: Color| {
        (
            Text::new(s),
            TextFont::from_font_size(size),
            TextColor(colour),
        )
    };
    let dim = Color::srgb(0.65, 0.70, 0.78);
    let cell = |width: f32| Node {
        width: Val::Px(width),
        ..default()
    };
    commands
        .spawn((
            PauseMenu,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(28.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.92)),
            ))
            .with_children(|panel| {
                panel.spawn(text("PAUSED", 36.0, Color::WHITE));
                panel.spawn(text("Esc or Start to resume", 16.0, dim));
                panel.spawn(Node {
                    height: Val::Px(12.0),
                    ..default()
                });
                let header = |row: &mut ChildSpawnerCommands| {
                    row.spawn(cell(260.0)).with_child(text("Keyboard", 18.0, dim));
                    row.spawn(cell(200.0)).with_child(text("Controller", 18.0, dim));
                    row.spawn(cell(320.0));
                };
                panel.spawn(Node::default()).with_children(header);
                for (key, pad, action) in CONTROLS {
                    panel.spawn(Node::default()).with_children(|row| {
                        row.spawn(cell(260.0)).with_child(text(key, 18.0, Color::WHITE));
                        row.spawn(cell(200.0)).with_child(text(pad, 18.0, Color::WHITE));
                        row.spawn(cell(320.0)).with_child(text(action, 18.0, Color::WHITE));
                    });
                }
            });
        });
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut menus: Query<&mut Visibility, With<PauseMenu>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        || gamepads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    if !pressed {
        return;
    }
    paused.0 = !paused.0;
    if paused.0 {
        time.pause();
        // Give the mouse back while the menu is up.
        if let Ok(mut cursor) = windows.single_mut() {
            cursor.grab_mode = CursorGrabMode::None;
            cursor.visible = true;
        }
    } else {
        time.unpause();
    }
    for mut v in &mut menus {
        *v = if paused.0 { Visibility::Visible } else { Visibility::Hidden };
    }
}
