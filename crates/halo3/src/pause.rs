//! The pause menu: Escape, or Start on a pad, stops the game and lists the
//! controls on four tabs: free roam and on foot (keyboard and controller), the dirt bike
//! and skating (controller only). Click a tab, or change tab with LB / RB,
//! the d-pad, A / D or the arrow keys. The same button carries on; Q, or View (Back) on a pad, quits the
//! game.
//!
//! Pausing stops virtual time, and the systems that read input or move
//! things run only `while running`, so nothing reacts to keys behind the
//! menu. Drawing goes on, so the frozen frame stays on screen.
//!
//! The dirt bike tab has the engine sound's volume slider: drag or click
//! it, or d-pad up / down, the up / down arrows or W / S; it is saved when
//! the game carries on (`EngineVolume`, `bike-sound.json`).
//!
//! The skate buttons are the stock Skate 3 layout the engine maps them
//! through (`skate-core` `input/controller.rs` and `gameplay_map.rs`: A and
//! X push, B brakes, Y steps off, RB grabs the world, the triggers grab).

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use halo3_core::bike::EngineVolume;
use halo3_core::pause::Paused;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Paused>()
            .init_resource::<ShownTab>()
            .insert_resource(EngineVolume::load())
            .add_systems(Startup, spawn_menu)
            .add_systems(
                PreUpdate,
                (toggle, quit, switch_tab, engine_slider)
                    .chain()
                    .after(InputSystems)
                    .after(halo3_core::pause::OverlayInput),
            );
    }
}

#[derive(Component)]
pub struct PauseMenu;

/// Which tab is up, an index into `TABS`.
#[derive(Resource, Default)]
struct ShownTab(usize);

/// A tab's page of controls.
#[derive(Component)]
struct TabPage(usize);

/// A tab's name in the row along the top.
#[derive(Component)]
struct TabLabel(usize);

/// The engine volume slider's bar, its filled part and its figure.
#[derive(Component)]
struct VolumeBar;
#[derive(Component)]
struct VolumeFill;
#[derive(Component)]
struct VolumeText;

const BIKE_TAB: usize = 2;

const TABS: [&str; 4] = ["Free roam", "On foot", "Dirt bike", "Skate"];

/// Free-roam controls: keyboard, controller, what it does.
const FREE_ROAM: &[[&str; 3]] = &[
    ["Esc", "Start", "pause / resume"],
    ["W A S D", "left stick", "move"],
    ["mouse", "right stick", "look around (click to take the mouse back)"],
    ["Space / E", "right trigger", "up"],
    ["C / Q", "left trigger", "down"],
    ["Shift (hold)", "A (hold)", "fast"],
    ["Alt (hold)", "", "slow"],
    ["F", "d-pad up", "get out and walk (on foot)"],
    ["J", "click both sticks", "enter / exit skate mode"],
    ["B", "View / Back", "get on / off the dirt bike"],
    ["V", "", "show / hide collision"],
    ["Page Down / Page Up", "", "next / previous zone set"],
    ["Home", "", "load every BSP"],
    ["P", "", "print the camera position to the console"],
    ["Insert", "d-pad left", "spawn the enemy Spartan where you look"],
    ["Delete", "d-pad right", "remove the enemy Spartan"],
];

/// On-foot controls (`foot/mod.rs`): keyboard, controller, what it does.
const FOOT: &[[&str; 3]] = &[
    ["F", "d-pad up", "get on foot / back to flying"],
    ["B", "View / Back", "back on the dirt bike, keeping your speed"],
    ["J", "click both sticks", "back on the skateboard"],
    ["W A S D", "left stick", "move"],
    ["mouse", "right stick", "aim"],
    ["Space", "A", "jump"],
    ["Ctrl / C (hold)", "left stick click", "crouch"],
    ["T", "d-pad down", "first / third person"],
    ["Insert", "d-pad left", "spawn the enemy Spartan where you look"],
    ["Delete", "d-pad right", "remove the enemy Spartan"],
    ["Esc", "Start", "pause"],
];

/// Dirt bike controls (`bike/mod.rs`): controller, what it does.
const BIKE: &[[&str; 2]] = &[
    ["left stick (left / right)", "steer"],
    ["right stick", "lean the rider forward, back, left, right"],
    ["right trigger", "throttle"],
    ["left trigger", "brake"],
    ["A (hold)", "back up"],
    ["Y", "stand the bike back up"],
    ["d-pad up (G)", "jump off on foot, keeping the bike's speed"],
    ["LB + d-pad down", "set the session marker"],
    ["LB + d-pad up (hold)", "back to the session marker"],
    ["View / Back", "get off the bike"],
    ["click left stick (H)", "engine sound: Reflex's 450F, synth, off"],
    ["0 (keyboard)", "physics tuning menu"],
    ["Start", "pause"],
];

/// Skate controls: controller, what it does.
const SKATE: &[[&str; 2]] = &[
    ["left stick", "steer and lean; spin in the air"],
    ["right stick", "flick-it tricks: pull back, flick forward to ollie"],
    ["A", "push"],
    ["X", "push with the other foot (mongo)"],
    ["B", "brake; in the air, take a foot off"],
    ["LT / RT", "crouch; in the air, grab left / right hand"],
    ["RB (hold)", "grab a ledge or coping (hand plants)"],
    ["Y", "step off the board / back on"],
    ["A or X", "get up after a bail"],
    ["LB + d-pad down", "set the session marker"],
    ["LB + d-pad up (hold)", "back to the session marker"],
    ["off the board: X", "jump"],
    ["off the board: LT / RT", "drop / throw the board"],
    ["d-pad up (G)", "jump off on foot, keeping the board's speed"],
    ["click both sticks", "back to flying"],
    ["Start", "pause"],
];

fn text(s: &str, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(colour))
}

const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const HOVER: Color = Color::WHITE;

/// One row of cells, each a fixed width so the columns line up.
fn row(parent: &mut ChildSpawnerCommands, cells: &[&str], widths: &[f32], colour: Color) {
    parent.spawn(Node::default()).with_children(|r| {
        for (s, w) in cells.iter().zip(widths) {
            r.spawn(Node {
                width: Val::Px(*w),
                ..default()
            })
            .with_child(text(s, 18.0, colour));
        }
    });
}

/// A tab page: a header row, then the controls, then `extra`.
fn page<const N: usize>(
    panel: &mut ChildSpawnerCommands,
    index: usize,
    header: [&str; N],
    widths: [f32; N],
    rows: &[[&str; N]],
    extra: impl FnOnce(&mut ChildSpawnerCommands),
) {
    panel
        .spawn((
            TabPage(index),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                display: if index == 0 { Display::Flex } else { Display::None },
                ..default()
            },
        ))
        .with_children(|p| {
            row(p, &header, &widths, DIM);
            for cells in rows {
                row(p, cells, &widths, Color::WHITE);
            }
            extra(p);
        });
}

/// The engine volume row: a label, the bar, the figure, how to move it.
fn volume_slider(p: &mut ChildSpawnerCommands) {
    p.spawn(Node {
        margin: UiRect::top(Val::Px(14.0)),
        align_items: AlignItems::Center,
        column_gap: Val::Px(14.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(Node {
            width: Val::Px(246.0),
            ..default()
        })
        .with_child(text("Engine volume", 18.0, LIT));
        r.spawn((
            VolumeBar,
            Node {
                width: Val::Px(240.0),
                height: Val::Px(14.0),
                ..default()
            },
            BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.15)),
        ))
        .with_child((
            VolumeFill,
            Node {
                width: Val::Percent(70.0),
                height: Val::Percent(100.0),
                ..default()
            },
            BackgroundColor(LIT),
        ));
        r.spawn((
            VolumeText,
            Node {
                width: Val::Px(56.0),
                ..default()
            },
            text("70 %", 18.0, Color::WHITE),
        ));
        r.spawn(text("d-pad up / down, or drag", 16.0, DIM));
    });
}

fn spawn_menu(mut commands: Commands) {
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
                    min_width: Val::Px(836.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.92)),
            ))
            .with_children(|panel| {
                panel.spawn(text("PAUSED", 36.0, Color::WHITE));
                panel.spawn(text("Esc or Start to resume", 16.0, DIM));
                panel.spawn(text("Q or View (Back) to quit the game", 16.0, DIM));
                panel
                    .spawn(Node {
                        column_gap: Val::Px(12.0),
                        margin: UiRect::vertical(Val::Px(12.0)),
                        align_items: AlignItems::Baseline,
                        ..default()
                    })
                    .with_children(|tabs| {
                        tabs.spawn(text("LB", 16.0, DIM));
                        for (i, name) in TABS.iter().enumerate() {
                            tabs.spawn((
                                TabLabel(i),
                                Node {
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                                    ..default()
                                },
                                text(name, 22.0, if i == 0 { LIT } else { DIM }),
                            ));
                        }
                        tabs.spawn(text("RB", 16.0, DIM));
                    });
                page(
                    panel,
                    0,
                    ["Keyboard", "Controller", ""],
                    [260.0, 200.0, 320.0],
                    FREE_ROAM,
                    |_| {},
                );
                page(
                    panel,
                    1,
                    ["Keyboard", "Controller", ""],
                    [260.0, 200.0, 320.0],
                    FOOT,
                    |_| {},
                );
                page(panel, BIKE_TAB, ["Controller", ""], [260.0, 520.0], BIKE, volume_slider);
                page(panel, 3, ["Controller", ""], [260.0, 520.0], SKATE, |_| {});
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
    volume: Res<EngineVolume>,
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
        volume.save();
    }
    for mut v in &mut menus {
        *v = if paused.0 { Visibility::Visible } else { Visibility::Hidden };
    }
}

/// Q, or View (Back) on a pad, closes the game from the menu.
fn quit(
    paused: Res<Paused>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut exit: MessageWriter<AppExit>,
) {
    if !paused.0 {
        return;
    }
    if keys.just_pressed(KeyCode::KeyQ) || gamepads.iter().any(|p| p.just_pressed(GamepadButton::Select)) {
        exit.write(AppExit::Success);
    }
}

/// A click on a tab shows it; LB / RB, the d-pad, A / D or the arrow keys
/// step through the tabs.
#[allow(clippy::too_many_arguments)]
fn switch_tab(
    paused: Res<Paused>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut shown: ResMut<ShownTab>,
    mut pages: Query<(&TabPage, &mut Node)>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut labels: Query<(&TabLabel, &ComputedNode, &UiGlobalTransform, &mut TextColor)>,
) {
    if !paused.0 {
        return;
    }
    let key = |a: KeyCode, b: KeyCode| keys.just_pressed(a) || keys.just_pressed(b);
    let pad = |a: GamepadButton, b: GamepadButton| {
        gamepads.iter().any(|p| p.just_pressed(a) || p.just_pressed(b))
    };
    // The tab under the mouse, if any (node sizes are in physical pixels).
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    let hovered = cursor.and_then(|at| {
        labels
            .iter()
            .find(|(_, node, transform, _)| node.contains_point(**transform, at))
            .map(|(label, ..)| label.0)
    });
    let clicked = hovered.filter(|_| mouse.just_pressed(MouseButton::Left));
    let wanted = if let Some(tab) = clicked {
        tab
    } else if key(KeyCode::KeyD, KeyCode::ArrowRight)
        || pad(GamepadButton::RightTrigger, GamepadButton::DPadRight)
    {
        (shown.0 + 1) % TABS.len()
    } else if key(KeyCode::KeyA, KeyCode::ArrowLeft)
        || pad(GamepadButton::LeftTrigger, GamepadButton::DPadLeft)
    {
        (shown.0 + TABS.len() - 1) % TABS.len()
    } else {
        shown.0
    };
    if wanted != shown.0 {
        shown.0 = wanted;
        for (page, mut node) in &mut pages {
            node.display = if page.0 == wanted { Display::Flex } else { Display::None };
        }
    }
    for (label, _, _, mut colour) in &mut labels {
        let want = if label.0 == wanted {
            LIT
        } else if hovered == Some(label.0) {
            HOVER
        } else {
            DIM
        };
        if colour.0 != want {
            colour.0 = want;
        }
    }
}

/// The engine volume slider on the dirt bike tab: a click or drag on the
/// bar sets it; d-pad up / down, the arrows or W / S step it by 5 %.
#[allow(clippy::too_many_arguments)]
fn engine_slider(
    paused: Res<Paused>,
    shown: Res<ShownTab>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    bars: Query<(&ComputedNode, &UiGlobalTransform), With<VolumeBar>>,
    mut fills: Query<&mut Node, With<VolumeFill>>,
    mut figures: Query<&mut Text, With<VolumeText>>,
    mut volume: ResMut<EngineVolume>,
    mut dragging: Local<bool>,
) {
    if !paused.0 || shown.0 != BIKE_TAB {
        *dragging = false;
        return;
    }
    let mut v = volume.0;
    let key = |a: KeyCode, b: KeyCode| keys.just_pressed(a) || keys.just_pressed(b);
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    if key(KeyCode::ArrowUp, KeyCode::KeyW) || pad(GamepadButton::DPadUp) {
        v += 0.05;
    }
    if key(KeyCode::ArrowDown, KeyCode::KeyS) || pad(GamepadButton::DPadDown) {
        v -= 0.05;
    }
    // Node sizes and the cursor are in physical pixels.
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    if let (Some(at), Ok((node, transform))) = (cursor, bars.single()) {
        if mouse.just_pressed(MouseButton::Left) && node.contains_point(*transform, at) {
            *dragging = true;
        }
        if *dragging && mouse.pressed(MouseButton::Left) {
            let width = node.size().x.max(1.0);
            let left = transform.translation.x - width / 2.0;
            v = (at.x - left) / width;
        }
    }
    if !mouse.pressed(MouseButton::Left) {
        *dragging = false;
    }
    // Whole per cents, so the steps land on round figures.
    let v = ((v.clamp(0.0, 1.0) * 100.0).round() / 100.0).clamp(0.0, 1.0);
    if v != volume.0 {
        volume.0 = v;
    }
    for mut fill in &mut fills {
        let want = Val::Percent(v * 100.0);
        if fill.width != want {
            fill.width = want;
        }
    }
    for mut figure in &mut figures {
        let want = format!("{:.0} %", v * 100.0);
        if figure.0 != want {
            figure.0 = want;
        }
    }
}
