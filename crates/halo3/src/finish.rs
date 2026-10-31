//! The end of a scripted run: when the map's own scripts say the mission is
//! won (`halo3_script::Runtime::mission_over`; on 120_halo, reaching
//! `end_trilogy01` at the mouth of the frigate's hangar), play stops and a
//! completion screen shows the time the run took, with a way to restart
//! the run or quit.
//!
//! The clock starts the first time the player is in control after the
//! world has loaded (riding, on foot or skating; an insertion puts him on
//! the bike at once) and counts game time only: the pause menu stops it,
//! and dying, falling and respawning do not touch it.
//!
//! Restart starts the game again with the same command line (a fresh copy
//! of the program, so the scripts, the floor plates, the AI and the music
//! all begin again), then closes this one.

use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use halo3_core::pause::Paused;
use halo3_core::skate::RiderPose;
use halo3_script::Scripts;

pub struct FinishPlugin;

impl Plugin for FinishPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunClock>()
            .init_resource::<RunFinished>()
            .init_resource::<Picked>()
            .add_systems(Startup, spawn_screen)
            .add_systems(Update, (clock, finish).chain())
            .add_systems(PreUpdate, choose.after(InputSystems));
    }
}

/// The run's clock, in game (virtual) seconds.
#[derive(Resource, Default, Debug)]
pub struct RunClock {
    started: Option<f64>,
    finished: Option<f64>,
}

impl RunClock {
    /// Seconds from the start to the finish, once both happened.
    pub fn time(&self) -> Option<f64> {
        Some(self.finished? - self.started?)
    }
}

/// The run is over and the completion screen is up; the pause menu stays
/// shut.
#[derive(Resource, Default)]
pub struct RunFinished(pub bool);

const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const HOVER: Color = Color::WHITE;

/// The choices, in order.
const CHOICES: [&str; 2] = ["Restart run", "Quit"];

#[derive(Component)]
struct FinishScreen;

#[derive(Component)]
struct TimeText;

#[derive(Component)]
struct Subtitle;

#[derive(Component)]
struct Choice(usize);

/// The highlighted choice, and real seconds the screen has been up (input
/// waits a moment, so a button held at the finish does not pick).
#[derive(Resource, Default)]
struct Picked {
    index: usize,
    shown: f32,
}

/// `m:ss.cc`, or `h:mm:ss.cc` past an hour.
pub fn format_time(seconds: f64) -> String {
    let cs = (seconds.max(0.0) * 100.0).round() as u64;
    let (h, m, s, c) = (cs / 360_000, cs / 6000 % 60, cs / 100 % 60, cs % 100);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}.{c:02}")
    } else {
        format!("{m}:{s:02}.{c:02}")
    }
}

fn text(s: &str, size: f32, colour: Color) -> impl Bundle {
    (Text::new(s), TextFont::from_font_size(size), TextColor(colour))
}

fn spawn_screen(mut commands: Commands) {
    commands
        .spawn((
            FinishScreen,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            GlobalZIndex(110),
            Visibility::Hidden,
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    padding: UiRect::all(Val::Px(28.0)),
                    min_width: Val::Px(520.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.92)),
            ))
            .with_children(|panel| {
                panel.spawn(text("MISSION COMPLETE", 36.0, Color::WHITE));
                panel.spawn((Subtitle, text("", 16.0, DIM)));
                panel.spawn(Node {
                    height: Val::Px(14.0),
                    ..default()
                });
                panel.spawn(text("TIME", 16.0, DIM));
                panel.spawn((TimeText, text("--:--.--", 56.0, LIT)));
                panel
                    .spawn(Node {
                        column_gap: Val::Px(18.0),
                        margin: UiRect::top(Val::Px(18.0)),
                        ..default()
                    })
                    .with_children(|row| {
                        for (i, name) in CHOICES.iter().enumerate() {
                            row.spawn((
                                Choice(i),
                                Node {
                                    padding: UiRect::axes(Val::Px(14.0), Val::Px(6.0)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, if i == 0 { 0.10 } else { 0.0 })),
                                text(name, 24.0, if i == 0 { LIT } else { DIM }),
                            ));
                        }
                    });
                panel.spawn(Node {
                    height: Val::Px(8.0),
                    ..default()
                });
                panel.spawn(text("Enter or A: choose    R: restart    Q or View (Back): quit", 15.0, DIM));
            });
        });
}

/// Starts the clock the first time the player is in control.
fn clock(time: Res<Time<Virtual>>, rider: Option<Res<RiderPose>>, scripts: Option<Res<Scripts>>, mut clock: ResMut<RunClock>) {
    if scripts.is_none() || clock.started.is_some() || !rider.is_some_and(|r| r.active) {
        return;
    }
    clock.started = Some(time.elapsed_secs_f64());
    info!("run clock: started");
}

/// The scripts won the mission: stop play, show the time.
#[allow(clippy::too_many_arguments)]
fn finish(
    scripts: Option<Res<Scripts>>,
    mut time: ResMut<Time<Virtual>>,
    mut clock: ResMut<RunClock>,
    mut finished: ResMut<RunFinished>,
    mut paused: ResMut<Paused>,
    mut picked: ResMut<Picked>,
    mut screens: Query<&mut Visibility, With<FinishScreen>>,
    mut menus: Query<&mut Visibility, (With<crate::pause::PauseMenu>, Without<FinishScreen>)>,
    mut times: Query<&mut Text, (With<TimeText>, Without<Subtitle>)>,
    mut subtitles: Query<&mut Text, (With<Subtitle>, Without<TimeText>)>,
    mut windows: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if finished.0 {
        return;
    }
    let Some(scripts) = scripts else { return };
    let Some(why) = scripts.runtime.mission_over.as_deref() else { return };
    let now = time.elapsed_secs_f64();
    clock.finished = Some(now);
    finished.0 = true;
    *picked = Picked::default();
    match clock.time() {
        Some(t) => info!("run complete ({why}) in {} ({t:.2} s)", format_time(t)),
        None => info!("run complete ({why}); the clock never started"),
    }
    // Play stops as behind the pause menu.
    paused.0 = true;
    time.pause();
    for mut v in &mut menus {
        *v = Visibility::Hidden;
    }
    for mut v in &mut screens {
        *v = Visibility::Visible;
    }
    let shown = clock.time().map_or_else(|| "--:--.--".to_string(), format_time);
    // 120_halo: the outro is the jump into the frigate.
    let line = if scripts.runtime.data.script_index("cin_halo_outro").is_some() {
        "You made it into the Forward Unto Dawn."
    } else {
        "The mission is won."
    };
    for mut t in &mut subtitles {
        t.0 = line.to_string();
    }
    for mut t in &mut times {
        t.0 = shown.clone();
    }
    if let Ok(mut cursor) = windows.single_mut() {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}

/// The completion screen's input: up / down / left / right (keys, d-pad,
/// stick) or the mouse pick a choice; Enter, Space, A or a click take it;
/// R restarts and Q or View (Back) quits at once.
#[allow(clippy::too_many_arguments)]
fn choose(
    finished: Res<RunFinished>,
    real: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut picked: ResMut<Picked>,
    mut choices: Query<(&Choice, &ComputedNode, &UiGlobalTransform, &mut TextColor, &mut BackgroundColor)>,
    mut exit: MessageWriter<AppExit>,
) {
    if !finished.0 {
        return;
    }
    picked.shown += real.delta_secs();
    if picked.shown < 0.6 {
        return;
    }
    let key = |k: &[KeyCode]| k.iter().any(|k| keys.just_pressed(*k));
    let pad = |b: &[GamepadButton]| gamepads.iter().any(|p| b.iter().any(|b| p.just_pressed(*b)));
    let n = CHOICES.len();
    let mut index = picked.index;
    if key(&[KeyCode::ArrowLeft, KeyCode::ArrowUp, KeyCode::KeyA, KeyCode::KeyW])
        || pad(&[GamepadButton::DPadLeft, GamepadButton::DPadUp])
    {
        index = (index + n - 1) % n;
    }
    if key(&[KeyCode::ArrowRight, KeyCode::ArrowDown, KeyCode::KeyD, KeyCode::KeyS])
        || pad(&[GamepadButton::DPadRight, GamepadButton::DPadDown])
    {
        index = (index + 1) % n;
    }
    // The mouse: the choice under it (sizes are in physical pixels).
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    let hovered = cursor.and_then(|at| {
        choices
            .iter()
            .find(|(_, node, tf, ..)| node.contains_point(**tf, at))
            .map(|(c, ..)| c.0)
    });
    let clicked = hovered.filter(|_| mouse.just_pressed(MouseButton::Left));
    if let Some(c) = clicked {
        index = c;
    }
    if picked.index != index {
        picked.index = index;
    }
    let take = clicked.is_some() || key(&[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(&[GamepadButton::South]);
    let action = if key(&[KeyCode::KeyR]) {
        Some(0)
    } else if key(&[KeyCode::KeyQ]) || pad(&[GamepadButton::Select]) {
        Some(1)
    } else {
        take.then_some(index)
    };
    for (c, _, _, mut colour, mut back) in &mut choices {
        let want = if c.0 == index {
            LIT
        } else if hovered == Some(c.0) {
            HOVER
        } else {
            DIM
        };
        if colour.0 != want {
            colour.0 = want;
        }
        let fill = Color::srgba(1.0, 1.0, 1.0, if c.0 == index { 0.10 } else { 0.0 });
        if back.0 != fill {
            back.0 = fill;
        }
    }
    match action {
        Some(0) => {
            restart();
            exit.write(AppExit::Success);
        }
        Some(_) => {
            exit.write(AppExit::Success);
        }
        None => {}
    }
}

/// Starts a fresh copy of the game with the same command line.
fn restart() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(&args).spawn()) {
        Ok(_) => info!("restarting the run: {}", args.join(" ")),
        Err(e) => error!("could not restart the run: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_read_as_minutes_seconds_hundredths() {
        assert_eq!(format_time(0.0), "0:00.00");
        assert_eq!(format_time(61.256), "1:01.26");
        assert_eq!(format_time(599.999), "10:00.00");
        assert_eq!(format_time(3725.5), "1:02:05.50");
    }

    #[test]
    fn clock_needs_a_start_and_a_finish() {
        let mut c = RunClock::default();
        assert_eq!(c.time(), None);
        c.started = Some(10.0);
        assert_eq!(c.time(), None);
        c.finished = Some(250.5);
        assert_eq!(c.time(), Some(240.5));
    }
}
