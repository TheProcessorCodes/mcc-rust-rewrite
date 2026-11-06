//! The first-run setup screen: the menu opens on it until the player's games
//! are set up (`mcc_setup::needs_setup`), and `mcc-rust-rewrite --setup` brings it
//! back. Three steps, in the menu's own look: Halo MCC, then Skate 3 (the
//! MW2 Skate 3 folder for its converter, then Skate 3 itself, converted
//! there and then), then MX vs ATV Reflex, which can be skipped. Each row's
//! Select opens a folder picker on a thread (`mcc_setup::select_*`, the
//! same ones the settings screen uses); a good choice moves on to the next
//! step that is not done, and once all are done the map menu shows.
//!
//! Keys: up / down (W / S, d-pad, left stick) pick a row, Enter / Space / A
//! choose it, Q / E / LB / RB go between steps (forward only past a finished
//! one), Escape / B go back a step, or close the game on the first. The
//! mouse clicks rows' buttons, the steps and the bar's buttons.

use std::sync::Mutex;

use bevy::prelude::*;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;
use mcc_setup::Missing;

use super::{BAR, DIM, FAINT, LINE, LIT, Menu, Phase, Pick, Picking, refresh_maps, text};

/// A row on the setup screen: a folder to choose, or Reflex declined.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum SetupRow {
    Mcc,
    Converter,
    Skate,
    Reflex,
    NoReflex,
}

impl SetupRow {
    fn name(self) -> &'static str {
        match self {
            SetupRow::Mcc => "Halo MCC",
            SetupRow::Converter => "MW2 Skate 3",
            SetupRow::Skate => "Skate 3",
            SetupRow::Reflex => "MX vs ATV Reflex",
            SetupRow::NoReflex => "Without Reflex",
        }
    }

    fn button(self) -> &'static str {
        match self {
            SetupRow::NoReflex => "SKIP",
            _ => "SELECT",
        }
    }
}

/// The steps in order: the tab's name, the page's title, required or not,
/// what the page says, and its rows.
struct Step {
    tab: &'static str,
    title: &'static str,
    required: bool,
    about: &'static [&'static str],
    rows: &'static [SetupRow],
}

const STEPS: [Step; 3] = [
    Step {
        tab: "HALO MCC",
        title: "Halo: The Master Chief Collection",
        required: true,
        about: &[
            "The maps, their pictures and their sounds are read from your own MCC install (PC) while you play. \
             Nothing is copied out of it or changed in it.",
            "Choose the MCC folder, the one that holds halo3\\maps. Choosing its halo3 or halo3\\maps folder works too.",
        ],
        rows: &[SetupRow::Mcc],
    },
    Step {
        tab: "SKATE 3",
        title: "Skate 3",
        required: true,
        about: &[
            "Skating, the rider and the on-foot Spartan use Skate 3's skater, from your own Xbox 360 copy: the \
             extracted game (default.xex with its data folder) or the folder holding its disc image (.iso).",
            "It is converted once by the Skate 3 converter that comes with MW2 Skate 3, which this game does not \
             ship. Download MW2 Skate 3 from the Releases page of github.com/chasmlol/2010-rust-rewrite-mashup \
             and choose its folder first. Converting takes a few minutes.",
        ],
        rows: &[SetupRow::Converter, SetupRow::Skate],
    },
    Step {
        tab: "MX VS ATV REFLEX",
        title: "MX vs ATV Reflex",
        required: false,
        about: &[
            "With your PC copy of MX vs ATV Reflex the dirt bike is its Dakumoto 450F, with its engine recordings.",
            "Without it the bike is our own model and has no engine sound. You can add Reflex later in Settings.",
        ],
        rows: &[SetupRow::Reflex, SetupRow::NoReflex],
    },
];

/// What the screen says at the top.
const INTRO: &str = "This game is played with your own copies of these games. Choose each one's folder once; \
                     you can change them later in Settings.";

/// The colour of a note that is a problem.
const ERROR: Color = Color::srgb(0.95, 0.47, 0.43);

#[derive(Component)]
pub(super) struct SetupScreen;
/// A step's name in the row along the top.
#[derive(Component)]
pub(super) struct StepLabel(usize);
/// A step's page.
#[derive(Component)]
pub(super) struct StepPage(usize);
#[derive(Component)]
pub(super) struct RowNode(SetupRow);
#[derive(Component)]
pub(super) struct RowValue(SetupRow);
#[derive(Component)]
pub(super) struct RowButton(SetupRow);
#[derive(Component)]
pub(super) struct RowButtonLabel(SetupRow);
#[derive(Component)]
pub(super) struct SetupNote;
/// The bar's "STEP 1 OF 3".
#[derive(Component)]
pub(super) struct StepCount;
#[derive(Component, Clone, Copy, PartialEq)]
pub(super) enum BarButton {
    Choose,
    Back,
}
#[derive(Component)]
pub(super) struct BarLabel(BarButton);

/// Whether a step's game is set up (Reflex: chosen or declined).
fn step_done(step: usize) -> bool {
    match step {
        0 => mcc_setup::mcc_ready().is_ok(),
        1 => mcc_setup::skate_ready(),
        _ => mcc_setup::reflex_answered(),
    }
}

/// The first step not done yet; None once every game is set up.
fn first_open_step() -> Option<usize> {
    (0..STEPS.len()).find(|&s| !step_done(s))
}

/// The row a step starts on: on Skate 3, Skate 3 itself once the converter
/// is known.
fn first_row(step: usize) -> usize {
    usize::from(step == 1 && mcc_setup::converter().is_some())
}

fn go(menu: &mut Menu, step: usize) {
    menu.step = step.min(STEPS.len() - 1);
    menu.setting = first_row(menu.step);
}

fn say(menu: &mut Menu, note: impl Into<String>, error: bool) {
    menu.note = note.into();
    menu.note_is_error = error;
}

/// Opens the screen on `step`, or the first one not done.
pub(super) fn open(menu: &mut Menu, step: Option<usize>) {
    menu.phase = Phase::Setup;
    go(menu, step.unwrap_or_else(|| first_open_step().unwrap_or(0)));
    say(menu, "", false);
    if menu.step == 0
        && let Err(Missing::Unreadable(e)) = mcc_setup::mcc_ready()
    {
        say(menu, e, true);
    }
}

/// What a row shows: the saved folder, or why there is none.
fn row_value(row: SetupRow) -> (String, bool) {
    let shown = |p: std::path::PathBuf| (p.display().to_string(), true);
    let not_set = || ("Not set".to_string(), false);
    match row {
        SetupRow::Mcc => match mcc_setup::mcc_ready() {
            Ok(()) => game_paths::mcc_root().map_or_else(not_set, shown),
            Err(Missing::NotSet) => not_set(),
            Err(Missing::Unreadable(_)) => ("Cannot be read (see below)".to_string(), false),
        },
        SetupRow::Converter => mcc_setup::converter().map_or_else(not_set, shown),
        SetupRow::Skate if mcc_setup::skate_ready() => game_paths::settings()
            .get(game_paths::key::SKATE_SOURCE)
            .map_or_else(|| ("Converted".to_string(), true), |s| (s.to_string(), true)),
        SetupRow::Skate => not_set(),
        SetupRow::Reflex => game_paths::reflex_root().map_or_else(not_set, shown),
        SetupRow::NoReflex if game_paths::reflex_skipped() => {
            ("Chosen: the bike is our own model, with no engine sound".to_string(), true)
        }
        SetupRow::NoReflex => ("Ride our own bike, with no engine sound".to_string(), false),
    }
}

/// The setup screen: the welcome, the steps, a page per step with its rows,
/// the note, and the bar.
pub(super) fn spawn(screen: &mut ChildSpawnerCommands) {
    let wide = || Node { width: Val::Percent(92.0), max_width: Val::Vh(165.0), ..default() };
    screen
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                margin: UiRect::top(Val::Vh(4.0)),
                padding: UiRect::bottom(Val::Px(12.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..wide()
            },
            BorderColor::all(LINE),
        ))
        .with_children(|head| {
            head.spawn(text("WELCOME", 22.0, LIT));
            head.spawn(text(INTRO, 17.0, DIM));
        });
    // The steps, drawn like the map menu's tabs.
    screen
        .spawn((
            Node {
                margin: UiRect::top(Val::Vh(3.0)),
                column_gap: Val::Px(36.0),
                align_items: AlignItems::FlexEnd,
                border: UiRect::bottom(Val::Px(1.0)),
                ..wide()
            },
            BorderColor::all(LINE),
        ))
        .with_children(|tabs| {
            for (i, step) in STEPS.iter().enumerate() {
                tabs.spawn((
                    StepLabel(i),
                    Node {
                        padding: UiRect::new(Val::Px(2.0), Val::Px(2.0), Val::Px(6.0), Val::Px(10.0)),
                        border: UiRect::bottom(Val::Px(3.0)),
                        margin: UiRect::bottom(Val::Px(-1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::NONE),
                    text(&format!("{}  {}", i + 1, step.tab), 22.0, DIM),
                ));
            }
        });
    for (i, step) in STEPS.iter().enumerate() {
        screen
            .spawn((
                StepPage(i),
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    margin: UiRect::top(Val::Vh(3.5)),
                    display: Display::None,
                    ..wide()
                },
            ))
            .with_children(|page| {
                page.spawn(Node { column_gap: Val::Px(18.0), align_items: AlignItems::Baseline, ..default() })
                    .with_children(|title| {
                        title.spawn(text(step.title, 30.0, Color::WHITE));
                        title.spawn(text(if step.required { "REQUIRED" } else { "OPTIONAL" }, 15.0, FAINT));
                    });
                for line in step.about {
                    page.spawn((Node { max_width: Val::Px(1180.0), ..default() }, text(line, 18.0, DIM)));
                }
                page.spawn(Node { height: Val::Px(14.0), ..default() });
                for &row in step.rows {
                    spawn_row(page, row);
                }
            });
    }
    screen.spawn((SetupNote, Node { margin: UiRect::top(Val::Px(18.0)), ..wide() }, text("", 17.0, DIM)));
    screen.spawn(Node { flex_grow: 1.0, ..default() });
    // The bar, as on the map menu: where you are, then the buttons.
    screen
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Vh(11.0),
                padding: UiRect::horizontal(Val::Percent(4.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(40.0),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(BAR),
            BorderColor::all(LINE),
        ))
        .with_children(|bar| {
            bar.spawn((Node { flex_grow: 1.0, ..default() }, StepCount, text("", 18.0, DIM)));
            for (button, letter, colour, key) in [
                (BarButton::Choose, "A", Color::srgb(0.37, 0.75, 0.29), "Enter"),
                (BarButton::Back, "B", Color::srgb(0.85, 0.33, 0.31), "Esc"),
            ] {
                bar.spawn((
                    button,
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(10.0),
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(10.0)),
                        ..default()
                    },
                ))
                .with_children(|b| {
                    b.spawn((
                        Node {
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(colour),
                    ))
                    .with_child(text(letter, 15.0, Color::BLACK));
                    let lit = if button == BarButton::Choose { LIT } else { DIM };
                    b.spawn((BarLabel(button), text("", 20.0, lit)));
                    b.spawn(text(&format!("({key})"), 15.0, FAINT));
                });
            }
        });
}

/// A row, as on the settings screen: the game, its folder, its button.
fn spawn_row(page: &mut ChildSpawnerCommands, row: SetupRow) {
    page.spawn((
        RowNode(row),
        Node {
            align_items: AlignItems::Center,
            column_gap: Val::Px(24.0),
            padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(6.0)),
            ..default()
        },
        BackgroundColor(Color::NONE),
        BorderColor::all(Color::NONE),
    ))
    .with_children(|r| {
        r.spawn((Node { width: Val::Px(240.0), flex_shrink: 0.0, ..default() }, text(row.name(), 22.0, Color::WHITE)));
        r.spawn((RowValue(row), Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), ..default() }, text("", 18.0, DIM)));
        r.spawn((
            RowButton(row),
            Node {
                padding: UiRect::axes(Val::Px(26.0), Val::Px(9.0)),
                border: UiRect::all(Val::Px(2.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(LINE),
        ))
        .with_child((RowButtonLabel(row), text(row.button(), 18.0, DIM)));
    });
}

/// The row with the focus.
fn focused(menu: &Menu) -> SetupRow {
    let rows = STEPS[menu.step].rows;
    rows[menu.setting.min(rows.len() - 1)]
}

/// Keys, pad and mouse on the setup screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut menu: ResMut<Menu>,
    targets: Query<(Option<&RowButton>, Option<&StepLabel>, Option<&BarButton>, &ComputedNode, &UiGlobalTransform)>,
    mut exit: MessageWriter<AppExit>,
    mut stick: Local<[bool; 2]>,
) {
    let y = gamepads
        .iter()
        .map(|p| p.get(GamepadAxis::LeftStickY).unwrap_or(0.0))
        .fold(0.0, |m: f32, v| if v.abs() > m.abs() { v } else { m });
    let held = [y > 0.5, y < -0.5];
    let pushed = [held[0] && !stick[0], held[1] && !stick[1]];
    *stick = held;
    // A picker or a conversion is running: wait for it.
    if menu.picking.is_some() || menu.setup_finished {
        return;
    }
    let key = |k: &[KeyCode]| keys.any_just_pressed(k.iter().copied());
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    let rows = STEPS[menu.step].rows.len();
    if key(&[KeyCode::Escape, KeyCode::Backspace]) || pad(GamepadButton::East) {
        back(&mut menu, &mut exit);
        return;
    }
    if key(&[KeyCode::ArrowUp, KeyCode::KeyW]) || pad(GamepadButton::DPadUp) || pushed[0] {
        menu.setting = menu.setting.saturating_sub(1);
    }
    if key(&[KeyCode::ArrowDown, KeyCode::KeyS]) || pad(GamepadButton::DPadDown) || pushed[1] {
        menu.setting = (menu.setting + 1).min(rows - 1);
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let tab_key = keys.just_pressed(KeyCode::Tab);
    if key(&[KeyCode::KeyQ]) || (tab_key && shift) || pad(GamepadButton::LeftTrigger) {
        let step = menu.step.saturating_sub(1);
        go(&mut menu, step);
        say(&mut menu, "", false);
    }
    if (key(&[KeyCode::KeyE]) || (tab_key && !shift) || pad(GamepadButton::RightTrigger)) && step_done(menu.step) {
        let step = menu.step + 1;
        go(&mut menu, step);
        say(&mut menu, "", false);
    }
    if key(&[KeyCode::Enter, KeyCode::NumpadEnter, KeyCode::Space]) || pad(GamepadButton::South) {
        let row = focused(&menu);
        choose(&mut menu, row);
        return;
    }
    if !buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(at) = windows.single().ok().and_then(Window::physical_cursor_position) else {
        return;
    };
    let Some((row, step, bar, ..)) = targets.iter().find(|(r, s, b, node, t)| {
        (r.is_some() || s.is_some() || b.is_some()) && node.size().x > 0.0 && node.contains_point(**t, at)
    }) else {
        return;
    };
    if let Some(RowButton(row)) = row {
        if let Some(i) = STEPS[menu.step].rows.iter().position(|r| r == row) {
            menu.setting = i;
            choose(&mut menu, *row);
        }
    } else if let Some(StepLabel(i)) = step {
        // Back to any step, forward only past finished ones.
        if (0..*i).all(step_done) {
            go(&mut menu, *i);
            say(&mut menu, "", false);
        }
    } else if let Some(bar) = bar {
        match bar {
            BarButton::Choose => {
                let row = focused(&menu);
                choose(&mut menu, row);
            }
            BarButton::Back => back(&mut menu, &mut exit),
        }
    }
}

/// Back a step, or out of the game from the first.
fn back(menu: &mut Menu, exit: &mut MessageWriter<AppExit>) {
    if menu.step == 0 {
        exit.write(AppExit::Success);
    } else {
        let step = menu.step - 1;
        go(menu, step);
        say(menu, "", false);
    }
}

/// A row chosen: open its folder picker on a thread ([`picked`] takes the
/// outcome), or decline Reflex at once.
fn choose(menu: &mut Menu, row: SetupRow) {
    match row {
        SetupRow::NoReflex => match mcc_setup::skip_reflex() {
            Ok(()) => {
                say(menu, "Playing without Reflex: the bike is our own model.", false);
                advance(menu, row);
            }
            Err(e) => say(menu, e, true),
        },
        SetupRow::Skate if mcc_setup::converter().is_none() => {
            menu.setting = 0;
            say(menu, "Choose your MW2 Skate 3 folder first: its converter turns Skate 3 into what this game reads.", true);
        }
        _ => {
            let (send, receive) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = send.send(match row {
                    SetupRow::Mcc => mcc_setup::select_mcc(),
                    SetupRow::Converter => mcc_setup::select_converter(),
                    SetupRow::Skate => mcc_setup::select_skate(),
                    _ => mcc_setup::select_reflex(),
                });
            });
            let note = match row {
                SetupRow::Skate => {
                    "Choose your Skate 3 folder in the window that opened. Converting it takes a few minutes.".to_string()
                }
                _ => format!("Choose your {} folder in the window that opened.", row.name()),
            };
            say(menu, note, false);
            menu.picking = Some(Picking { what: Pick::Setup(row), result: Mutex::new(receive) });
        }
    }
}

/// A picker has finished (`settings_poll`): say how it went and move on.
pub(super) fn picked(menu: &mut Menu, row: SetupRow, outcome: Result<Option<std::path::PathBuf>, String>) {
    match outcome {
        Ok(Some(path)) => {
            eprintln!("setup: {} is {}", row.name(), path.display());
            say(menu, format!("Saved {}.", row.name()), false);
            advance(menu, row);
        }
        Ok(None) => say(menu, "", false),
        Err(e) => {
            eprintln!("setup: {e}");
            say(menu, e, true);
        }
    }
}

/// After a good choice: on to Skate 3 itself after the converter, else the
/// next step not done, else the maps.
fn advance(menu: &mut Menu, row: SetupRow) {
    if row == SetupRow::Converter && !mcc_setup::skate_ready() {
        menu.setting = 1;
        return;
    }
    match first_open_step() {
        Some(step) => go(menu, step),
        None => menu.setup_finished = true,
    }
}

/// Every game is set up: the map menu, built again from the MCC chosen.
pub(super) fn finish(world: &mut World) {
    {
        let mut menu = world.resource_mut::<Menu>();
        menu.setup_finished = false;
        menu.phase = Phase::Choosing;
        menu.setting = 0;
        say(&mut menu, "", false);
    }
    eprintln!("setup: done");
    refresh_maps(world);
}

/// Draws the setup screen: shown in [`Phase::Setup`], the step's page, the
/// rows' folders, the focus, the note (the conversion's progress while it
/// runs) and the bar.
#[allow(clippy::type_complexity)]
pub(super) fn show(
    menu: Res<Menu>,
    mut nodes: Query<(Option<&SetupScreen>, Option<&StepPage>, &mut Node)>,
    mut frames: Query<(Option<&StepLabel>, Option<&RowNode>, Option<&RowButton>, Option<&mut BackgroundColor>, &mut BorderColor)>,
    mut texts: Query<(
        AnyOf<(&StepLabel, &RowValue, &RowButtonLabel, &SetupNote, &StepCount, &BarLabel)>,
        &mut Text,
        &mut TextColor,
    )>,
) {
    let converting = menu.picking.as_ref().is_some_and(|p| p.what == Pick::Setup(SetupRow::Skate));
    if !menu.is_changed() {
        // Only the conversion's progress changes meanwhile.
        if converting && menu.phase == Phase::Setup {
            let progress = mcc_setup::progress();
            for ((_, _, _, note, _, _), mut text, _) in &mut texts {
                if note.is_some() && !progress.is_empty() {
                    let line = format!("Converting Skate 3. This takes a few minutes.\n{progress}");
                    if text.0 != line {
                        text.0 = line;
                    }
                }
            }
        }
        return;
    }
    let open = menu.phase == Phase::Setup;
    for (screen, page, mut node) in &mut nodes {
        if screen.is_some() {
            node.display = if open { Display::Flex } else { Display::None };
        } else if let Some(page) = page {
            node.display = if open && page.0 == menu.step { Display::Flex } else { Display::None };
        }
    }
    if !open {
        return;
    }
    let done: Vec<bool> = (0..STEPS.len()).map(step_done).collect();
    let focus = focused(&menu);
    let busy = |row: SetupRow| menu.picking.as_ref().is_some_and(|p| p.what == Pick::Setup(row));
    for (step, row, button, background, mut border) in &mut frames {
        if let Some(step) = step {
            *border = BorderColor::all(if step.0 == menu.step { LIT } else { Color::NONE });
        } else if let Some(row) = row {
            let on = row.0 == focus;
            if let Some(mut background) = background {
                background.0 = if on { BAR } else { Color::NONE };
            }
            *border = BorderColor::all(if on { LINE } else { Color::NONE });
        } else if let Some(button) = button {
            *border = BorderColor::all(if button.0 == focus { LIT } else { LINE });
        }
    }
    let progress = mcc_setup::progress();
    for ((step, value, label, note, count, bar), mut text, mut colour) in &mut texts {
        if let Some(step) = step {
            let mark = if step.0 != menu.step && done[step.0] { "  (done)" } else { "" };
            text.0 = format!("{}  {}{mark}", step.0 + 1, STEPS[step.0].tab);
            colour.0 = if step.0 == menu.step {
                LIT
            } else if step.0 == 0 || done[..step.0].iter().all(|d| *d) {
                DIM
            } else {
                FAINT
            };
        } else if let Some(value) = value {
            let (shown, set) = row_value(value.0);
            text.0 = shown;
            colour.0 = if set { DIM } else { FAINT };
        } else if let Some(label) = label {
            text.0 = if busy(label.0) { "WORKING..." } else { label.0.button() }.to_string();
            colour.0 = if label.0 == focus { LIT } else { DIM };
        } else if note.is_some() {
            text.0 = if converting && !progress.is_empty() {
                format!("Converting Skate 3. This takes a few minutes.\n{progress}")
            } else {
                menu.note.clone()
            };
            colour.0 = if menu.note_is_error && !converting { ERROR } else { DIM };
        } else if count.is_some() {
            text.0 = format!("STEP {} OF {}", menu.step + 1, STEPS.len());
        } else if let Some(bar) = bar {
            text.0 = match bar.0 {
                BarButton::Choose => focus.button(),
                BarButton::Back if menu.step == 0 => "EXIT",
                BarButton::Back => "BACK",
            }
            .to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_step_has_rows_and_text() {
        for step in &STEPS {
            assert!(!step.rows.is_empty() && !step.about.is_empty());
        }
        // MCC and Skate 3 are required; only Reflex can be skipped.
        assert!(STEPS[0].required && STEPS[1].required && !STEPS[2].required);
        assert!(STEPS.iter().flat_map(|s| s.rows).filter(|r| **r == SetupRow::NoReflex).count() == 1);
    }
}
