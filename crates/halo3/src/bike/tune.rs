//! The physics tuning menu: 0 on the dirt bike opens a column of sliders
//! over the ride (`physics::Tuning`); 0 or Esc (B on a pad) closes it. The
//! bike stands still while it is open.
//!
//! Drag a slider with the mouse, or pick one with W / S, the up / down
//! arrows or the d-pad and move it with A / D, left / right. Right-click a
//! slider, or press Backspace (X on a pad), to put it back to stock; Delete
//! (Y on a pad) puts them all back. The values last until the game closes.

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use super::physics::{self, Tuning};

pub struct TunePlugin;

impl Plugin for TunePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TuneMenu>()
            .add_systems(Startup, spawn_menu)
            // Before the pause menu, so Esc closes this one first.
            .add_systems(PreUpdate, toggle.after(InputSystems).before(crate::pause::toggle))
            .add_systems(Update, (adjust, show).chain());
    }
}

/// Whether the menu is up, the selected slider and the one being dragged.
#[derive(Resource, Default)]
pub struct TuneMenu {
    pub open: bool,
    selected: usize,
    dragging: Option<usize>,
}

/// A run condition: the menu is closed.
pub fn closed(menu: Res<TuneMenu>) -> bool {
    !menu.open
}

/// One slider: a name, what each end does, its range, the step a key press
/// moves it, and how it reads and writes the tuning.
struct Knob {
    group: &'static str,
    name: &'static str,
    low: &'static str,
    high: &'static str,
    range: (f32, f32),
    step: f32,
    get: fn(&Tuning) -> f32,
    set: fn(&mut Tuning, f32),
    /// What the value means, in the units a rider would use.
    show: fn(f32) -> String,
}

const KMH: f32 = 3.6;

const KNOBS: [Knob; 14] = [
    Knob {
        group: "Engine and brakes",
        name: "Engine power",
        low: "less",
        high: "more",
        range: (2.0, 16.0),
        step: 0.5,
        get: |t| t.accel,
        set: |t, v| t.accel = v,
        show: |v| format!("{v:.1} m/s², 0-50 km/h in {:.1} s", 50.0 / KMH / v),
    },
    Knob {
        group: "",
        name: "Top speed",
        low: "slower",
        high: "faster",
        range: (10.0, 45.0),
        step: 1.0,
        get: |t| t.top_speed,
        set: |t, v| t.top_speed = v,
        show: |v| format!("{:.0} km/h", v * KMH),
    },
    Knob {
        group: "",
        name: "Brake power",
        low: "weaker",
        high: "stronger",
        range: (3.0, 20.0),
        step: 0.5,
        get: |t| t.brake_decel,
        set: |t, v| t.brake_decel = v,
        show: |v| format!("{v:.1} m/s², 50-0 km/h in {:.1} s", 50.0 / KMH / v),
    },
    Knob {
        group: "",
        name: "Brake bias",
        low: "more rear",
        high: "more front",
        range: (0.2, 0.9),
        step: 0.05,
        get: |t| t.brake_front,
        set: |t, v| t.brake_front = v,
        show: |v| format!("front {:.0} % / rear {:.0} %", v * 100.0, (1.0 - v) * 100.0),
    },
    Knob {
        group: "Tyres",
        name: "Grip",
        low: "less grip",
        high: "more grip",
        range: (0.6, 3.0),
        step: 0.05,
        get: |t| t.grip,
        set: |t, v| t.grip = v,
        show: |v| format!("friction {v:.2} x the load"),
    },
    Knob {
        group: "",
        name: "Slide feel",
        low: "smooth, free",
        high: "sharp, catches",
        range: (1000.0, 9000.0),
        step: 200.0,
        get: |t| t.lateral_stiffness,
        set: |t, v| t.lateral_stiffness = v,
        show: |v| format!("side force {v:.0} N per m/s of slip"),
    },
    Knob {
        group: "Steering and lean",
        name: "Steering lock",
        low: "less lock",
        high: "more lock",
        range: (0.4, 1.8),
        step: 0.05,
        get: |t| t.steer_lock,
        set: |t, v| t.steer_lock = v,
        show: |v| {
            let deg = |a: f32| (a * v).to_degrees();
            format!(
                "full stick {:.0} deg standing, {:.0} deg at 50 km/h",
                deg(physics::STEER_LOCK_SLOW),
                deg(physics::STEER_LOCK_FAST)
            )
        },
    },
    Knob {
        group: "",
        name: "Steering quickness",
        low: "lazier",
        high: "quicker",
        range: (1.0, 12.0),
        step: 0.5,
        get: |t| t.steer_rate,
        set: |t, v| t.steer_rate = v,
        show: |v| format!("bars turn {:.0} deg/s", v.to_degrees()),
    },
    Knob {
        group: "",
        name: "Lean depth",
        low: "shallower",
        high: "deeper",
        range: (0.3, 1.2),
        step: 0.05,
        get: |t| t.max_lean,
        set: |t, v| t.max_lean = v,
        show: |v| format!("leans over to {:.0} deg at most", v.to_degrees()),
    },
    Knob {
        group: "",
        name: "Lean quickness",
        low: "lazier",
        high: "quicker",
        range: (0.3, 3.0),
        step: 0.1,
        get: |t| t.lean_speed,
        set: |t, v| t.lean_speed = v,
        show: |v| format!("x{v:.1} the stock lean speed"),
    },
    Knob {
        group: "",
        name: "Rider-lean influence",
        low: "less (stick)",
        high: "more (rider)",
        range: (0.0, 4.0),
        step: 0.1,
        get: |t| t.lean_gain,
        set: |t, v| t.lean_gain = v,
        show: |v| format!("the rider's weight counts x{v:.1}"),
    },
    Knob {
        group: "Suspension",
        name: "Stiffness",
        low: "softer",
        high: "stiffer",
        range: (0.4, 2.5),
        step: 0.05,
        get: |t| t.spring,
        set: |t, v| t.spring = v,
        show: |v| {
            let r = physics::rear_leg().spring * v / 1000.0;
            let f = physics::front_leg().spring * v / 1000.0;
            format!("springs front {f:.1} / rear {r:.1} kN/m")
        },
    },
    Knob {
        group: "",
        name: "Damping",
        low: "bouncier",
        high: "calmer",
        range: (0.3, 3.0),
        step: 0.1,
        get: |t| t.damper,
        set: |t, v| t.damper = v,
        show: |v| {
            let r = physics::rear_leg().damper * v;
            let f = physics::front_leg().damper * v;
            format!("dampers front {f:.0} / rear {r:.0} N·s/m")
        },
    },
    Knob {
        group: "Air",
        name: "Air rotation",
        low: "lazier",
        high: "snappier",
        range: (0.3, 2.5),
        step: 0.05,
        get: |t| t.air,
        set: |t, v| t.air = v,
        show: |v| format!("x{v:.2} the stock flip and whip speed"),
    },
];

const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const CHANGED: Color = Color::srgb(0.55, 0.85, 1.0);
const TRACK: Color = Color::srgb(0.22, 0.25, 0.30);
const FILL: Color = Color::srgb(0.85, 0.55, 0.15);
const TRACK_WIDTH: f32 = 240.0;

#[derive(Component)]
struct Menu;

/// A slider's row (index into `KNOBS`), for the highlight.
#[derive(Component)]
struct Row(usize);

/// The part of a slider the mouse grabs.
#[derive(Component)]
struct Track(usize);

/// The filled part of a slider, its width the value.
#[derive(Component)]
struct Fill(usize);

/// The stock value's tick on a slider.
#[derive(Component)]
struct Stock;

/// The value read out beside a slider.
#[derive(Component)]
struct Readout(usize);

/// The line at the bottom: how many values differ from stock.
#[derive(Component)]
struct Summary;

fn text(s: impl Into<String>, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(colour))
}

fn cell(width: f32) -> Node {
    Node {
        width: Val::Px(width),
        ..default()
    }
}

/// Where `v` sits along a knob's range, 0 to 1.
fn fraction(k: &Knob, v: f32) -> f32 {
    ((v - k.range.0) / (k.range.1 - k.range.0)).clamp(0.0, 1.0)
}

fn spawn_menu(mut commands: Commands) {
    let stock = Tuning::default();
    commands
        .spawn((
            Menu,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            GlobalZIndex(90),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::all(Val::Px(24.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.90)),
            ))
            .with_children(|panel| {
                panel.spawn(text("BIKE PHYSICS", 32.0, Color::WHITE));
                panel.spawn(text(
                    "Drag a slider, or W / S to pick and A / D to move (d-pad on a pad).  \
                     Right-click or Backspace: back to stock.  Delete: all to stock.  0 or Esc: ride.",
                    15.0,
                    DIM,
                ));
                for (i, k) in KNOBS.iter().enumerate() {
                    if !k.group.is_empty() {
                        panel.spawn((
                            Node {
                                margin: UiRect::top(Val::Px(10.0)),
                                ..default()
                            },
                            text(k.group, 17.0, DIM),
                        ));
                    }
                    panel
                        .spawn((
                            Row(i),
                            Node {
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|r| {
                            r.spawn(cell(190.0)).with_child(text(k.name, 18.0, Color::WHITE));
                            r.spawn(Node {
                                width: Val::Px(110.0),
                                justify_content: JustifyContent::End,
                                margin: UiRect::right(Val::Px(10.0)),
                                ..default()
                            })
                            .with_child(text(k.low, 14.0, DIM));
                            // The track, tall enough to grab, with the bar inside.
                            r.spawn((
                                Track(i),
                                Node {
                                    width: Val::Px(TRACK_WIDTH),
                                    height: Val::Px(22.0),
                                    align_items: AlignItems::Center,
                                    ..default()
                                },
                            ))
                            .with_children(|t| {
                                t.spawn((
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Px(8.0),
                                        ..default()
                                    },
                                    BackgroundColor(TRACK),
                                ))
                                .with_children(|bar| {
                                    let at = fraction(k, (k.get)(&stock)) * 100.0;
                                    bar.spawn((
                                        Fill(i),
                                        Node {
                                            width: Val::Percent(at),
                                            height: Val::Percent(100.0),
                                            ..default()
                                        },
                                        BackgroundColor(FILL),
                                    ));
                                    bar.spawn((
                                        Stock,
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: Val::Percent(at),
                                            top: Val::Px(-4.0),
                                            width: Val::Px(2.0),
                                            height: Val::Px(16.0),
                                            ..default()
                                        },
                                        BackgroundColor(Color::WHITE),
                                    ));
                                });
                            });
                            r.spawn(Node {
                                width: Val::Px(110.0),
                                margin: UiRect::left(Val::Px(10.0)),
                                ..default()
                            })
                            .with_child(text(k.high, 14.0, DIM));
                            r.spawn(cell(400.0)).with_child((Readout(i), text("", 15.0, DIM)));
                        });
                }
                panel.spawn((
                    Summary,
                    Node {
                        margin: UiRect::top(Val::Px(12.0)),
                        ..default()
                    },
                    text("", 15.0, DIM),
                ));
            });
        });
}

/// 0 opens and closes the menu while riding; Esc or B on a pad close it.
fn toggle(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    paused: Res<crate::pause::Paused>,
    rider: Res<crate::skate::RiderPose>,
    mut menu: ResMut<TuneMenu>,
    mut menus: Query<&mut Visibility, With<Menu>>,
) {
    let zero = keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0);
    let open = if menu.open {
        let escape = keys.just_pressed(KeyCode::Escape);
        if escape {
            // Esc closes this menu, not the game behind it.
            keys.clear_just_pressed(KeyCode::Escape);
        }
        let back = gamepads.iter().any(|p| p.just_pressed(GamepadButton::East));
        !(zero || escape || back) && rider.bike
    } else {
        zero && rider.bike && !paused.0
    };
    if open == menu.open {
        return;
    }
    menu.open = open;
    menu.dragging = None;
    for mut v in &mut menus {
        *v = if open { Visibility::Visible } else { Visibility::Hidden };
    }
}

/// The slider under the cursor and where along it (0 to 1).
fn under_cursor(
    cursor: Option<Vec2>,
    tracks: &Query<(&Track, &ComputedNode, &UiGlobalTransform)>,
    only: Option<usize>,
) -> Option<(usize, f32)> {
    let at = cursor?;
    tracks.iter().find_map(|(track, node, transform)| {
        if only.is_some_and(|i| i != track.0) {
            return None;
        }
        let local = transform.try_inverse()?.transform_point2(at);
        let size = node.size();
        let along = (local.x / size.x + 0.5).clamp(0.0, 1.0);
        // A dragged slider follows the mouse anywhere along the row.
        let inside = local.x.abs() <= size.x * 0.5 + 6.0 && local.y.abs() <= size.y * 0.5;
        (only.is_some() || inside).then_some((track.0, along))
    })
}

#[allow(clippy::too_many_arguments)]
fn adjust(
    mut menu: ResMut<TuneMenu>,
    mut tuning: ResMut<super::BikeTuning>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    tracks: Query<(&Track, &ComputedNode, &UiGlobalTransform)>,
) {
    if !menu.open {
        return;
    }
    let key = |a: KeyCode, b: KeyCode| keys.just_pressed(a) || keys.just_pressed(b);
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    let n = KNOBS.len();
    if key(KeyCode::KeyS, KeyCode::ArrowDown) || pad(GamepadButton::DPadDown) {
        menu.selected = (menu.selected + 1) % n;
    }
    if key(KeyCode::KeyW, KeyCode::ArrowUp) || pad(GamepadButton::DPadUp) {
        menu.selected = (menu.selected + n - 1) % n;
    }
    let t = &mut tuning.0;
    let stock = Tuning::default();
    let k = &KNOBS[menu.selected];
    let mut nudge = 0.0;
    if key(KeyCode::KeyD, KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) {
        nudge += 1.0;
    }
    if key(KeyCode::KeyA, KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) {
        nudge -= 1.0;
    }
    if nudge != 0.0 {
        let v = ((k.get)(t) + nudge * k.step).clamp(k.range.0, k.range.1);
        (k.set)(t, v);
    }
    if keys.just_pressed(KeyCode::Backspace) || pad(GamepadButton::West) {
        (k.set)(t, (k.get)(&stock));
    }
    if keys.just_pressed(KeyCode::Delete) || pad(GamepadButton::North) {
        *t = stock;
    }

    // The mouse: press on a slider to grab it, drag to move it.
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    if mouse.just_pressed(MouseButton::Left) {
        menu.dragging = under_cursor(cursor, &tracks, None).map(|(i, _)| i);
    }
    if !mouse.pressed(MouseButton::Left) {
        menu.dragging = None;
    }
    if let Some(i) = menu.dragging {
        menu.selected = i;
        if let Some((_, along)) = under_cursor(cursor, &tracks, Some(i)) {
            let k = &KNOBS[i];
            let raw = k.range.0 + along * (k.range.1 - k.range.0);
            // Snap to the key step, so values stay round.
            let v = ((raw / k.step).round() * k.step).clamp(k.range.0, k.range.1);
            (k.set)(t, v);
        }
    }
    if mouse.just_pressed(MouseButton::Right)
        && let Some((i, _)) = under_cursor(cursor, &tracks, None)
    {
        menu.selected = i;
        (KNOBS[i].set)(t, (KNOBS[i].get)(&stock));
    }
}

/// Fills, readouts and the highlight follow the values.
#[allow(clippy::type_complexity)]
fn show(
    menu: Res<TuneMenu>,
    tuning: Res<super::BikeTuning>,
    mut fills: Query<(&Fill, &mut Node), Without<Stock>>,
    mut rows: Query<(&Row, &mut BackgroundColor)>,
    mut readouts: Query<(&Readout, &mut Text, &mut TextColor), Without<Summary>>,
    mut summary: Query<&mut Text, With<Summary>>,
) {
    if !menu.open {
        return;
    }
    let t = &tuning.0;
    let stock = Tuning::default();
    for (fill, mut node) in &mut fills {
        let k = &KNOBS[fill.0];
        let want = Val::Percent(fraction(k, (k.get)(t)) * 100.0);
        if node.width != want {
            node.width = want;
        }
    }
    for (row, mut bg) in &mut rows {
        let want = if row.0 == menu.selected {
            Color::srgba(1.0, 0.82, 0.35, 0.12)
        } else {
            Color::NONE
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    let mut changed = 0;
    for (r, mut text, mut colour) in &mut readouts {
        let k = &KNOBS[r.0];
        let (v, s) = ((k.get)(t), (k.get)(&stock));
        let differs = (v - s).abs() > k.step * 0.25;
        changed += differs as usize;
        let line = if differs {
            format!("{}  (stock: {})", (k.show)(v), (k.show)(s))
        } else {
            (k.show)(v)
        };
        if text.0 != line {
            text.0 = line;
        }
        let want = if r.0 == menu.selected {
            LIT
        } else if differs {
            CHANGED
        } else {
            DIM
        };
        if colour.0 != want {
            colour.0 = want;
        }
    }
    if let Ok(mut s) = summary.single_mut() {
        let line = match changed {
            0 => "All stock.".to_string(),
            1 => "1 slider differs from stock.".to_string(),
            n => format!("{n} sliders differ from stock."),
        };
        if s.0 != line {
            s.0 = line;
        }
    }
}
