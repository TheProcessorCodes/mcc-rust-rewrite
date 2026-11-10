//! The pause menu: Escape, or Start on a pad, stops the game and lists the
//! controls on four tabs: free roam, on foot, the dirt bike and skating
//! (keyboard and controller), with the bike's engine volume slider on
//! the dirt bike tab; a fifth, Sound, has the volume sliders (`sound_tab`).
//! Click a tab, or change tab with LB / RB, the d-pad, A / D or the arrow
//! keys (on the Sound tab the arrows and the d-pad move the sliders; LB / RB
//! and A / D still change tab). The same button carries on; Q, or View
//! (Back) on a pad, goes back to the map menu (or quits the game when it
//! was started on a map). On a campaign level R, or Y, restarts the level.
//! Both ask first: the second press of the same button does it (`Asking`).
//!
//! Pausing stops virtual time, and the systems that read input or move
//! things run only `while running`, so nothing reacts to keys behind the
//! menu. Drawing goes on, so the frozen frame stays on screen.
//!
//! The dirt bike tab has the engine sound's volume slider: drag or click
//! it, or d-pad up / down, the up / down arrows or W / S; it is saved when
//! the game carries on (`EngineVolume`, `bike-sound.json`). Under it the
//! physics switch: Arcade (the bike's own model, Reflex v1 or Unleashed as
//! the tuning menu has it) or Reflex (Reflex's own simulation); X on a pad,
//! P, or a click flips it, and it is saved at once (`bike-tuning.json`).
//! It is only there with MX vs ATV Reflex set up (it is optional); the main
//! menu's Settings has the same switch.
//!
//! With skating off (`SkateEnabled`), the Skate tab says why and how to
//! turn it on instead of listing the buttons: turned off in Settings, or
//! Skate 3 not set up (skipped in the first-run setup,
//! `game_paths::skate_ready`), which Settings can add. The skateboard's
//! rows (J) on the other tabs say the same in short.
//!
//! The skate buttons are the stock Skate 3 layout the engine maps them
//! through (`skate-core` `input/controller.rs` and `gameplay_map.rs`: A and
//! X push, B brakes, Y steps off, RB grabs the world, the triggers grab).

use bevy::input::InputSystems;
use bevy::prelude::*;
use mcc_core::map::MapAppExt;
use bevy::ui::UiGlobalTransform;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use mcc_audio::{Channel, SoundLevels};
use mcc_core::bike::EngineVolume;
use mcc_core::pause::Paused;

pub struct PausePlugin;

impl Plugin for PausePlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Paused>()
            .init_map_resource::<crate::finish::RunFinished>()
            .init_map_resource::<ShownTab>()
            .init_map_resource::<Asking>()
            .init_map_resource::<SliderInput>()
            .insert_resource(EngineVolume::load())
            .add_systems(mcc_core::map::MapStartup, spawn_menu)
            .add_systems(
                PreUpdate,
                (toggle, forget_ask, quit, restart_level, show_ask, switch_tab, sound_tab, engine_slider, physics_switch)
                    .chain()
                    .after(InputSystems)
                    .after(mcc_core::pause::OverlayInput)
                    // Not behind the loading screen: play has not begun.
                    .run_if(mcc_core::map::playing),
            );
        if std::env::var_os("MCC_SHOT_PAUSE_TAB").is_some() {
            app.add_systems(Update, draw_on_shot_camera.run_if(mcc_core::map::ready));
        }
    }
}

/// `MCC_SHOT_PAUSE_TAB` (dev): a headless shot draws into an image, not a
/// window, so the menu is put on the 3D camera to be in the picture.
fn draw_on_shot_camera(
    mut commands: Commands,
    menus: Query<Entity, (With<PauseMenu>, Without<UiTargetCamera>)>,
    cameras: Query<Entity, With<Camera3d>>,
) {
    let Some(camera) = cameras.iter().next() else {
        return;
    };
    for menu in &menus {
        commands.entity(menu).insert(UiTargetCamera(camera));
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

/// The physics switch's two choices, `true` for Reflex.
#[derive(Component)]
struct PhysicsChoice(bool);

const BIKE_TAB: usize = 2;
/// The Skate tab's index in `TABS`.
const SKATE_TAB: usize = 3;

const TABS: [&str; 5] = ["Free roam", "On foot", "Dirt bike", "Skate", "Sound"];

/// The Sound tab's index in `TABS`.
const SOUND_TAB: usize = 4;

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
    ["J", "click both sticks", "back on the skateboard, keeping your speed"],
    ["W A S D", "left stick", "move"],
    ["mouse", "right stick", "aim"],
    ["Space", "A", "jump"],
    ["Ctrl / C (hold)", "left stick click", "crouch"],
    ["T", "d-pad down", "first / third person"],
    ["Q", "B", "melee (lunges at a foe a step or two away)"],
    ["G", "left trigger", "throw a grenade"],
    ["X", "LB", "frag or plasma grenades"],
    ["Insert", "d-pad left", "spawn the enemy Spartan where you look"],
    ["Delete", "d-pad right", "remove the enemy Spartan"],
    ["Esc", "Start", "pause"],
];

/// Dirt bike controls (`bike/mod.rs`): keyboard (`mcc_bike` `read_controls`),
/// controller, what it does. The session marker is on the pad only.
const BIKE: &[[&str; 3]] = &[
    ["A / D", "left stick", "steer"],
    ["arrow keys", "right stick", "lean the rider forward, back, left, right"],
    ["W", "right trigger", "throttle"],
    ["S", "left trigger", "brake; held at a standstill, back up"],
    ["E (hold)", "RB (hold)", "clutch (Reflex physics)"],
    ["R", "Y", "stand the bike back up"],
    ["G", "d-pad up", "jump off on foot, keeping the bike's speed"],
    ["J", "click both sticks", "onto the skateboard, keeping the bike's speed"],
    ["", "LB + d-pad down", "set the session marker"],
    ["", "LB + d-pad up (hold)", "back to the session marker (set on the bike or the board)"],
    ["B", "View / Back", "get off the bike"],
    ["H", "click left stick", "engine sound on / off"],
    ["0", "LB + X", "physics tuning menu"],
    ["Esc", "Start", "pause"],
];

/// Skate controls: keyboard (`mcc_skate` `keyboard_frame`, the keyboard as
/// a pad), controller, what it does. The session marker is on the pad only.
const SKATE: &[[&str; 3]] = &[
    ["W A S D", "left stick", "steer and lean; spin in the air"],
    ["arrow keys", "right stick", "flick-it tricks: pull back, flick forward to ollie"],
    ["Space", "A", "push"],
    ["R", "X", "push with the other foot (mongo)"],
    ["F", "B", "brake; in the air, take a foot off"],
    ["Z / X", "LT / RT", "crouch; in the air, grab left / right hand"],
    ["E (hold)", "RB (hold)", "grab a ledge or coping (hand plants)"],
    ["T", "Y", "step off the board / back on"],
    ["Space or R", "A or X", "get up after a bail"],
    ["", "LB + d-pad down", "set the session marker"],
    ["", "LB + d-pad up (hold)", "back to the session marker (set on the bike or the board)"],
    ["R", "X", "off the board: jump"],
    ["Z / X", "LT / RT", "off the board: drop / throw the board"],
    ["G", "d-pad up", "jump off on foot, keeping the board's speed"],
    ["B", "View / Back", "onto the dirt bike, keeping the board's speed"],
    ["J", "click both sticks", "back to flying"],
    ["Esc", "Start", "pause"],
];

/// Column widths of the dirt bike and skate tabs: keyboard, controller, what.
const RIDE_WIDTHS: [f32; 3] = [130.0, 230.0, 550.0];

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

/// A tab page: a header row, then the controls, then `extra`. Shown when
/// `index` is `shown`.
#[allow(clippy::too_many_arguments)]
fn page<const N: usize>(
    panel: &mut ChildSpawnerCommands,
    index: usize,
    shown: usize,
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
                display: if index == shown { Display::Flex } else { Display::None },
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
        r.spawn(crate::key_hint::hint("d-pad up / down, or drag", "W / S or up / down, or drag", 16.0, DIM));
    });
    // Only with Reflex set up: without it there is nothing to switch to.
    if mcc_bike::exact::vehicle().is_some() {
        physics_row(p);
    }
}

/// The physics switch row: a label, Arcade and Reflex, how to flip it.
fn physics_row(p: &mut ChildSpawnerCommands) {
    p.spawn(Node {
        margin: UiRect::top(Val::Px(10.0)),
        align_items: AlignItems::Center,
        column_gap: Val::Px(14.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(Node {
            width: Val::Px(246.0),
            ..default()
        })
        .with_child(text("Physics", 18.0, LIT));
        for (choice, name) in [(false, "Arcade"), (true, "Reflex")] {
            r.spawn((
                PhysicsChoice(choice),
                Node {
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
            ))
            .with_child(text(name, 18.0, DIM));
        }
        r.spawn(crate::key_hint::hint("X, or click", "P, or click", 16.0, DIM));
    });
}

/// Why skating is off for this map, which the Skate tab and the
/// skateboard's rows say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SkateOff {
    /// The player turned Skating off in Settings.
    Settings,
    /// Skate 3 is not set up: skipped in the first-run setup, or its
    /// converted data is gone.
    NoSkate3,
}

impl SkateOff {
    /// Why skating is off, or None when it is on. Without Skate 3 set up
    /// that is the reason whatever Settings says, as turning Skating on
    /// would not bring it back.
    fn find(skating: bool, skate_ready: bool) -> Option<SkateOff> {
        match (skating, skate_ready) {
            (true, _) => None,
            (false, true) => Some(SkateOff::Settings),
            (false, false) => Some(SkateOff::NoSkate3),
        }
    }

    /// What a skateboard row in a controls list says in place of what it does.
    fn label(self) -> &'static str {
        match self {
            SkateOff::Settings => "skateboard (skating is off in Settings)",
            SkateOff::NoSkate3 => "skateboard (needs Skate 3)",
        }
    }
}

/// The Skate tab while skating is off: why, and how to turn it on.
fn skate_off_notice(p: &mut ChildSpawnerCommands, off: SkateOff) {
    p.spawn((
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            margin: UiRect::top(Val::Px(6.0)),
            padding: UiRect::axes(Val::Px(20.0), Val::Px(16.0)),
            border: UiRect::left(Val::Px(3.0)),
            ..default()
        },
        BorderColor::all(LIT),
        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.05)),
    ))
    .with_children(|n| {
        match off {
            SkateOff::Settings => {
                n.spawn(text("Skating is turned off", 24.0, LIT));
                n.spawn(text("This map was loaded without the skateboard or its grind rails.", 18.0, Color::WHITE));
                n.spawn(crate::key_hint::hint(
                    "To skate, go back to the map menu, open Settings (X) and turn Skating on.",
                    "To skate, go back to the map menu, open Settings (F1) and turn Skating on.",
                    18.0,
                    Color::WHITE,
                ));
            }
            SkateOff::NoSkate3 => {
                n.spawn(text("Skating needs Skate 3", 24.0, LIT));
                n.spawn(text(
                    "Skate 3 is not set up, so maps load without the skateboard or grind rails.",
                    18.0,
                    Color::WHITE,
                ));
                n.spawn(crate::key_hint::hint(
                    "To add it, go back to the map menu, open Settings (X) and choose Skate 3.",
                    "To add it, go back to the map menu, open Settings (F1) and choose Skate 3.",
                    18.0,
                    Color::WHITE,
                ));
            }
        }
        n.spawn(text("It takes effect on the next map you load.", 16.0, DIM));
    });
}

/// What the session marker's row adds while there is a board to share it with.
const MARKER_SHARED: &str = " (set on the bike or the board)";

/// A controls list with the skateboard's rows (J) marked as off, saying why.
/// The session marker's row no longer mentions the board.
fn without_skating<const N: usize>(rows: &[[&'static str; N]], off: Option<SkateOff>) -> Vec<[&'static str; N]> {
    rows.iter()
        .map(|r| {
            let mut r = *r;
            if let Some(off) = off
                && (r[0] == "J" || r[0].ends_with("(J)"))
            {
                r[N - 1] = off.label();
            }
            if off.is_some()
                && let Some(plain) = r[N - 1].strip_suffix(MARKER_SHARED)
            {
                r[N - 1] = plain;
            }
            r
        })
        .collect()
}

fn spawn_menu(
    mut commands: Commands,
    levels: Option<Res<SoundLevels>>,
    map_menu: Option<Res<mcc_core::map::MapMenu>>,
    zones: Option<Res<mcc_world::zones::Zones>>,
    skate: Option<Res<mcc_core::skate::SkateEnabled>>,
) {
    let off = SkateOff::find(mcc_core::skate::skating_on(skate.as_deref()), game_paths::skate_ready());
    // `MCC_SHOT_PAUSE_TAB=N` (dev, for headless shots): the menu is drawn
    // open on tab N (the game is not paused).
    let open_on = std::env::var("MCC_SHOT_PAUSE_TAB").ok().and_then(|s| s.parse::<usize>().ok()).filter(|&t| t < SOUND_TAB);
    let shown = open_on.unwrap_or(0);
    commands.insert_resource(ShownTab(shown));
    let campaign = zones.is_some_and(|z| !z.layout.zone_sets.is_empty());
    let leave = if map_menu.is_some() {
        ("View (Back) to go back to the map menu", "Q to go back to the map menu")
    } else {
        ("View (Back) to quit the game", "Q to quit the game")
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
            if open_on.is_some() { Visibility::Visible } else { Visibility::Hidden },
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
                panel.spawn(crate::key_hint::hint("Start to resume", "Esc to resume", 16.0, DIM));
                panel.spawn((
                    AskHint { what: Asking::Quit, pad: leave.0, keys: leave.1 },
                    crate::key_hint::hint(leave.0, leave.1, 16.0, DIM),
                ));
                if campaign {
                    let restart = ("Y to restart the level from the beginning", "R to restart the level from the beginning");
                    panel.spawn((
                        AskHint { what: Asking::Restart, pad: restart.0, keys: restart.1 },
                        crate::key_hint::hint(restart.0, restart.1, 16.0, DIM),
                    ));
                }
                panel
                    .spawn(Node {
                        column_gap: Val::Px(12.0),
                        margin: UiRect::vertical(Val::Px(12.0)),
                        align_items: AlignItems::Baseline,
                        ..default()
                    })
                    .with_children(|tabs| {
                        tabs.spawn((crate::key_hint::KeyHint { pad: "LB", keys: "A" }, text("A", 16.0, DIM)));
                        for (i, name) in TABS.iter().enumerate() {
                            tabs.spawn((
                                TabLabel(i),
                                Node {
                                    padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                                    ..default()
                                },
                                text(name, 22.0, if i == shown { LIT } else { DIM }),
                            ));
                        }
                        tabs.spawn((crate::key_hint::KeyHint { pad: "RB", keys: "D" }, text("D", 16.0, DIM)));
                    });
                page(
                    panel,
                    0,
                    shown,
                    ["Keyboard", "Controller", ""],
                    [260.0, 200.0, 320.0],
                    &without_skating(FREE_ROAM, off),
                    |_| {},
                );
                page(
                    panel,
                    1,
                    shown,
                    ["Keyboard", "Controller", ""],
                    [260.0, 200.0, 320.0],
                    &without_skating(FOOT, off),
                    |_| {},
                );
                page(
                    panel,
                    BIKE_TAB,
                    shown,
                    ["Keyboard", "Controller", ""],
                    RIDE_WIDTHS,
                    &without_skating(BIKE, off),
                    volume_slider,
                );
                match off {
                    None => page(panel, SKATE_TAB, shown, ["Keyboard", "Controller", ""], RIDE_WIDTHS, SKATE, |_| {}),
                    Some(off) => page(panel, SKATE_TAB, shown, [], [], &[], |p| skate_off_notice(p, off)),
                }
                sound_page(panel, levels.as_deref());
            });
        });
}

/// Width of a slider's bar, px.
const BAR: f32 = 340.0;

/// A slider's name.
#[derive(Component)]
struct SliderLabel(Channel);
/// A slider's bar (clicked and dragged).
#[derive(Component)]
struct SliderTrack(Channel);
/// The filled part of a slider's bar.
#[derive(Component)]
struct SliderFill(Channel);
/// A slider's value, `80 %`.
#[derive(Component)]
struct SliderValue(Channel);

/// The Sound tab: a slider per volume, then how to use them.
fn sound_page(panel: &mut ChildSpawnerCommands, levels: Option<&SoundLevels>) {
    panel
        .spawn((
            TabPage(SOUND_TAB),
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(10.0),
                display: Display::None,
                ..default()
            },
        ))
        .with_children(|p| {
            let Some(levels) = levels else {
                p.spawn(text("Sound is off (MCC_AUDIO=0, or this map has no sound).", 18.0, DIM));
                return;
            };
            for (i, c) in Channel::ALL.into_iter().enumerate() {
                p.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(14.0),
                    ..default()
                })
                .with_children(|r| {
                    r.spawn((
                        Node {
                            width: Val::Px(110.0),
                            ..default()
                        },
                        SliderLabel(c),
                        text(c.label(), 20.0, if i == 0 { LIT } else { Color::WHITE }),
                    ));
                    // The bar: a dark track, the fill, and a tick at 100 %.
                    r.spawn((
                        SliderTrack(c),
                        Node {
                            width: Val::Px(BAR),
                            height: Val::Px(16.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.12)),
                    ))
                    .with_children(|bar| {
                        bar.spawn((
                            SliderFill(c),
                            Node {
                                width: Val::Percent(levels.get(c) / SoundLevels::MAX * 100.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(LIT.with_alpha(0.85)),
                        ));
                        bar.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(100.0 / SoundLevels::MAX),
                                width: Val::Px(2.0),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(Color::WHITE.with_alpha(0.6)),
                        ));
                    });
                    r.spawn((
                        Node {
                            width: Val::Px(100.0),
                            ..default()
                        },
                        SliderValue(c),
                        text(&value_text(levels, c), 20.0, Color::WHITE),
                    ));
                    r.spawn(text(c.about(), 15.0, DIM));
                });
            }
            p.spawn(Node {
                height: Val::Px(8.0),
                ..default()
            });
            for (pad, keys) in [
                (
                    "Up / down (d-pad, left stick): pick a slider. Left / right (d-pad, left stick): move it.",
                    "Up / down (W / S, arrow keys): pick a slider. Left / right (arrow keys): move it.",
                ),
                (
                    "Or click and drag a bar with the mouse. The tick marks 100 %, the game's own mix.",
                    "Or click and drag a bar with the mouse. The tick marks 100 %, the game's own mix.",
                ),
                (
                    "Saved next to the game, in sound-levels.json.",
                    "K mutes and - / = change the master at any time. Saved next to the game, in sound-levels.json.",
                ),
                ("LB / RB: the other tabs.", "A / D: the other tabs."),
            ] {
                p.spawn(crate::key_hint::hint(pad, keys, 15.0, DIM));
            }
        });
}

fn value_text(levels: &SoundLevels, c: Channel) -> String {
    let muted = c == Channel::Master && levels.muted;
    format!("{:.0} %{}", levels.get(c) * 100.0, if muted { " muted" } else { "" })
}

/// The Sound tab's input state.
#[derive(Resource, Default)]
struct SliderInput {
    /// The picked slider, an index into `Channel::ALL`.
    selected: usize,
    /// The slider the mouse is dragging.
    dragging: Option<Channel>,
    /// The direction held (-1, 0, 1) and seconds until it repeats.
    held: (i32, f32),
    /// The stick is already up or down (one pick per push).
    stick_y: bool,
    /// The tab was up last frame (the key that opened it does not count).
    was_active: bool,
}

/// The Sound tab: picks and moves the sliders by keyboard, pad and mouse,
/// and draws their values (also when K or - / = changed them behind the
/// menu).
#[allow(clippy::too_many_arguments)]
fn sound_tab(
    paused: Res<Paused>,
    finished: Res<crate::finish::RunFinished>,
    shown: Res<ShownTab>,
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    levels: Option<ResMut<SoundLevels>>,
    mut input: ResMut<SliderInput>,
    tracks: Query<(&SliderTrack, &ComputedNode, &UiGlobalTransform)>,
    mut fills: Query<(&SliderFill, &mut Node)>,
    mut values: Query<(&SliderValue, &mut Text)>,
    mut labels: Query<(&SliderLabel, &mut TextColor)>,
) {
    let Some(mut levels) = levels else { return };
    let active = paused.0 && !finished.0 && shown.0 == SOUND_TAB;
    let fresh = active && !input.was_active;
    if input.was_active != active {
        input.was_active = active;
    }
    if !active || fresh {
        if input.dragging.is_some() || input.held != (0, 0.0) {
            input.dragging = None;
            input.held = (0, 0.0);
        }
    } else {
        let n = Channel::ALL.len();
        let pressed = |k: [KeyCode; 2]| k.iter().any(|k| keys.just_pressed(*k));
        let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
        let stick = gamepads
            .iter()
            .map(|p| p.left_stick())
            .find(|v| v.length() > 0.5)
            .unwrap_or(Vec2::ZERO);
        // Picking a slider: one step per press or per push of the stick.
        let stick_y = stick.y.abs() > 0.6 && stick.y.abs() > stick.x.abs();
        let push = stick_y && !input.stick_y;
        if input.stick_y != stick_y {
            input.stick_y = stick_y;
        }
        if pressed([KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) || (push && stick.y > 0.0) {
            input.selected = (input.selected + n - 1) % n;
        }
        if pressed([KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) || (push && stick.y < 0.0) {
            input.selected = (input.selected + 1) % n;
        }
        // Moving it: a step on the press, then repeating while held.
        let stick_x = stick.x.abs() > 0.5 && stick.x.abs() > stick.y.abs();
        let left = keys.pressed(KeyCode::ArrowLeft)
            || gamepads.iter().any(|p| p.pressed(GamepadButton::DPadLeft))
            || (stick_x && stick.x < 0.0);
        let right = keys.pressed(KeyCode::ArrowRight)
            || gamepads.iter().any(|p| p.pressed(GamepadButton::DPadRight))
            || (stick_x && stick.x > 0.0);
        let dir = right as i32 - left as i32;
        let mut step = 0;
        if dir == 0 {
            if input.held != (0, 0.0) {
                input.held = (0, 0.0);
            }
        } else if input.held.0 != dir {
            input.held = (dir, 0.4);
            step = dir;
        } else {
            input.held.1 -= time.delta_secs();
            if input.held.1 <= 0.0 {
                input.held.1 = 0.06;
                step = dir;
            }
        }
        if step != 0 {
            let c = Channel::ALL[input.selected];
            let v = levels.get(c) + step as f32 * SoundLevels::STEP;
            levels.set(c, v);
        }
        // The mouse: press on a bar to pick and set it, drag to move it
        // (node sizes and the cursor are in physical pixels).
        let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
        if mouse.just_pressed(MouseButton::Left) {
            input.dragging = cursor.and_then(|at| {
                tracks
                    .iter()
                    .find(|(_, node, tf)| node.contains_point(**tf, at))
                    .map(|(t, ..)| t.0)
            });
        }
        if !mouse.pressed(MouseButton::Left) && input.dragging.is_some() {
            input.dragging = None;
        }
        if let (Some(c), Some(at)) = (input.dragging, cursor)
            && let Some((_, node, tf)) = tracks.iter().find(|(t, ..)| t.0 == c)
        {
            let i = Channel::ALL.iter().position(|x| *x == c).unwrap_or(0);
            if input.selected != i {
                input.selected = i;
            }
            let width = node.size().x.max(1.0);
            let x0 = tf.translation.x - width / 2.0;
            let v = ((at.x - x0) / width).clamp(0.0, 1.0) * SoundLevels::MAX;
            // Whole 5 % steps, and 100 % snaps from close by.
            let v = if (v - 1.0).abs() < 0.03 {
                1.0
            } else {
                (v / SoundLevels::STEP).round() * SoundLevels::STEP
            };
            if (levels.get(c) - v).abs() > 1e-4 {
                levels.set(c, v);
            }
        }
    }
    if !(levels.is_changed() || input.is_changed()) {
        return;
    }
    let selected = Channel::ALL[input.selected];
    for (f, mut node) in &mut fills {
        let w = Val::Percent(levels.get(f.0) / SoundLevels::MAX * 100.0);
        if node.width != w {
            node.width = w;
        }
    }
    for (v, mut t) in &mut values {
        let s = value_text(&levels, v.0);
        if t.0 != s {
            t.0 = s;
        }
    }
    for (l, mut colour) in &mut labels {
        let want = if l.0 == selected { LIT } else { Color::WHITE };
        if colour.0 != want {
            colour.0 = want;
        }
    }
}

fn toggle(
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut paused: ResMut<Paused>,
    mut time: ResMut<Time<Virtual>>,
    mut menus: Query<&mut Visibility, With<PauseMenu>>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
    finished: Res<crate::finish::RunFinished>,
    volume: Res<EngineVolume>,
) {
    let pressed = keys.just_pressed(KeyCode::Escape)
        || gamepads.iter().any(|p| p.just_pressed(GamepadButton::Start));
    // The run is over: the completion screen (`finish.rs`) has the keys.
    if !pressed || finished.0 {
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

/// What the menu is asking about: leaving and restarting both throw the run
/// away, and Q is a skate key and the fly camera's down, so the first press
/// only asks and a second press of the same button does it.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Asking {
    #[default]
    Nothing,
    Quit,
    Restart,
}

impl Asking {
    /// The key and the pad button that ask, and then confirm.
    fn buttons(self) -> Option<(KeyCode, GamepadButton)> {
        match self {
            Asking::Nothing => None,
            Asking::Quit => Some((KeyCode::KeyQ, GamepadButton::Select)),
            Asking::Restart => Some((KeyCode::KeyR, GamepadButton::North)),
        }
    }

    /// The line shown in place of the hint while asking: controller, keyboard.
    fn question(self) -> (&'static str, &'static str) {
        match self {
            Asking::Nothing => ("", ""),
            Asking::Quit => ("Leave this map? View (Back) again to leave", "Leave this map? Q again to leave"),
            Asking::Restart => ("Restart the level? Y again to restart", "Restart the level? R again to restart"),
        }
    }

    /// A press of `self`'s button: `true` when it was already asked, so do
    /// it; else it is asked now.
    fn pressed(self, asking: &mut Asking) -> bool {
        let confirmed = *asking == self;
        *asking = if confirmed { Asking::Nothing } else { self };
        confirmed
    }
}

/// The quit or restart hint line, with its usual wordings.
#[derive(Component)]
struct AskHint {
    what: Asking,
    pad: &'static str,
    keys: &'static str,
}

/// Any other key or button, or carrying on, takes the question back.
fn forget_ask(
    paused: Res<Paused>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut asking: ResMut<Asking>,
) {
    let Some((key, button)) = asking.buttons() else {
        return;
    };
    let other = keys.get_just_pressed().any(|k| *k != key)
        || gamepads.iter().any(|p| p.get_just_pressed().any(|b| *b != button));
    if !paused.0 || other {
        *asking = Asking::Nothing;
    }
}

/// The hint line being asked about turns into the question, lit.
fn show_ask(asking: Res<Asking>, mut hints: Query<(&AskHint, &mut crate::key_hint::KeyHint, &mut TextColor)>) {
    if !asking.is_changed() {
        return;
    }
    for (hint, mut shown, mut colour) in &mut hints {
        let asked = hint.what == *asking;
        let (pad, keys) = if asked { asking.question() } else { (hint.pad, hint.keys) };
        (shown.pad, shown.keys) = (pad, keys);
        colour.0 = if asked { LIT } else { DIM };
    }
}

/// Q, or View (Back) on a pad, twice (`Asking`), goes back to the map menu
/// (`menu.rs`), or closes the game when it was started on a map.
fn quit(
    mut commands: Commands,
    paused: Res<Paused>,
    map_menu: Option<Res<mcc_core::map::MapMenu>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut asking: ResMut<Asking>,
    mut exit: MessageWriter<AppExit>,
) {
    if !paused.0 {
        return;
    }
    if (keys.just_pressed(KeyCode::KeyQ) || gamepads.iter().any(|p| p.just_pressed(GamepadButton::Select)))
        && Asking::Quit.pressed(&mut asking)
    {
        if map_menu.is_some() {
            commands.insert_resource(mcc_core::map::BackToMenu);
        } else {
            exit.write(AppExit::Success);
        }
    }
}

/// R, or Y on a pad, twice (`Asking`), on a campaign level: the level is
/// loaded again from its start (`finish.rs` `RestartMap`), so objects,
/// scripts, AI, music and the run's clock all begin afresh.
fn restart_level(
    mut commands: Commands,
    paused: Res<Paused>,
    zones: Option<Res<mcc_world::zones::Zones>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut asking: ResMut<Asking>,
) {
    if !paused.0 || !zones.is_some_and(|z| !z.layout.zone_sets.is_empty()) {
        return;
    }
    if (keys.just_pressed(KeyCode::KeyR) || gamepads.iter().any(|p| p.just_pressed(GamepadButton::North)))
        && Asking::Restart.pressed(&mut asking)
    {
        commands.insert_resource(crate::finish::RestartMap);
    }
}

/// A click on a tab shows it; LB / RB, the d-pad, A / D or the arrow keys
/// step through the tabs.
#[allow(clippy::too_many_arguments)]
fn switch_tab(
    paused: Res<Paused>,
    finished: Res<crate::finish::RunFinished>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut shown: ResMut<ShownTab>,
    mut pages: Query<(&TabPage, &mut Node)>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut labels: Query<(&TabLabel, &ComputedNode, &UiGlobalTransform, &mut TextColor)>,
) {
    if !paused.0 || finished.0 {
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
    // On the Sound tab the arrow keys and the d-pad move the sliders, so
    // only LB / RB and A / D change tab there.
    let sliders = shown.0 == SOUND_TAB;
    let wanted = if let Some(tab) = clicked {
        tab
    } else if key(KeyCode::KeyD, if sliders { KeyCode::KeyD } else { KeyCode::ArrowRight })
        || pad(GamepadButton::RightTrigger, if sliders { GamepadButton::RightTrigger } else { GamepadButton::DPadRight })
    {
        (shown.0 + 1) % TABS.len()
    } else if key(KeyCode::KeyA, if sliders { KeyCode::KeyA } else { KeyCode::ArrowLeft })
        || pad(GamepadButton::LeftTrigger, if sliders { GamepadButton::LeftTrigger } else { GamepadButton::DPadLeft })
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

/// The physics switch on the dirt bike tab: X on a pad, P, or a click on a
/// choice. Switching saves the tuning at once; the bike takes it at its
/// next step. Maps without the bike (a `--shot` that doesn't ride) have no
/// tuning, so the switch does nothing there.
#[allow(clippy::too_many_arguments)]
fn physics_switch(
    paused: Res<Paused>,
    shown: Res<ShownTab>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    tuning: Option<ResMut<mcc_bike::tune::BikeTuning>>,
    mut choices: Query<(&PhysicsChoice, &ComputedNode, &UiGlobalTransform, &mut BackgroundColor, &Children)>,
    mut colours: Query<&mut TextColor>,
) {
    let Some(mut tuning) = tuning.filter(|_| !choices.is_empty()) else {
        return;
    };
    let reflex_now = tuning.0.model >= 1.5;
    let mut want = reflex_now;
    if paused.0 && shown.0 == BIKE_TAB {
        if keys.just_pressed(KeyCode::KeyP) || gamepads.iter().any(|p| p.just_pressed(GamepadButton::West)) {
            want = !reflex_now;
        }
        // Node sizes and the cursor are in physical pixels.
        let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
        if let Some(at) = cursor.filter(|_| mouse.just_pressed(MouseButton::Left)) {
            for (choice, node, transform, _, _) in &choices {
                if node.contains_point(*transform, at) {
                    want = choice.0;
                }
            }
        }
    }
    if want != reflex_now && !mcc_bike::tune::switch_physics(&mut tuning.0, want) {
        want = reflex_now;
    }
    for (choice, _, _, mut background, children) in &mut choices {
        let on = choice.0 == want;
        let fill = if on { Color::srgba(1.0, 0.82, 0.35, 0.25) } else { Color::NONE };
        if background.0 != fill {
            background.0 = fill;
        }
        let colour = if on { LIT } else { DIM };
        for &child in children {
            if let Ok(mut c) = colours.get_mut(child)
                && c.0 != colour
            {
                c.0 = colour;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without Skate 3 that is the reason skating is off, whatever
    /// Settings says; with skating on there is nothing to say.
    #[test]
    fn skate_3_missing_is_the_reason_before_settings() {
        assert_eq!(SkateOff::find(true, true), None);
        assert_eq!(SkateOff::find(false, true), Some(SkateOff::Settings));
        assert_eq!(SkateOff::find(false, false), Some(SkateOff::NoSkate3));
        // A `--skate` shot forces skating on.
        assert_eq!(SkateOff::find(true, false), None);
    }

    /// The rows the reason replaces, by their first column.
    fn marked<const N: usize>(rows: &[[&'static str; N]], off: SkateOff) -> Vec<&'static str> {
        without_skating(rows, Some(off))
            .iter()
            .filter(|r| r[N - 1] == off.label())
            .map(|r| r[0])
            .collect()
    }

    /// Every list's skateboard row says why skating is off, the dirt bike's
    /// "(J)" row too, and nothing else changes; with skating on, nothing does.
    #[test]
    fn only_the_skateboard_rows_are_marked() {
        for off in [SkateOff::Settings, SkateOff::NoSkate3] {
            assert_eq!(marked(FREE_ROAM, off), ["J"]);
            assert_eq!(marked(FOOT, off), ["J"]);
            assert_eq!(marked(BIKE, off), ["J"]);
        }
        assert_eq!(without_skating(FREE_ROAM, None), FREE_ROAM);
        assert_eq!(without_skating(FOOT, None), FOOT);
        assert_eq!(without_skating(BIKE, None), BIKE);
        assert_ne!(SkateOff::Settings.label(), SkateOff::NoSkate3.label());
    }

    /// The first press asks, the second of the same button does it, and a
    /// press of the other one asks about that instead.
    #[test]
    fn quit_and_restart_take_two_presses() {
        let mut asking = Asking::Nothing;
        assert!(!Asking::Quit.pressed(&mut asking));
        assert_eq!(asking, Asking::Quit);
        assert!(!Asking::Restart.pressed(&mut asking));
        assert_eq!(asking, Asking::Restart);
        assert!(Asking::Restart.pressed(&mut asking));
        assert_eq!(asking, Asking::Nothing);
    }

    /// The session marker's row mentions the board only while there is one.
    #[test]
    fn the_marker_row_drops_the_board_without_skating() {
        let said = |rows: &[[&'static str; 3]]| rows.iter().any(|r| r[2].ends_with(MARKER_SHARED));
        assert!(said(BIKE));
        assert!(said(&without_skating(BIKE, None)));
        for off in [SkateOff::Settings, SkateOff::NoSkate3] {
            let rows = without_skating(BIKE, Some(off));
            assert!(!said(&rows));
            assert!(rows.iter().any(|r| r[2] == "back to the session marker"));
        }
    }
}
