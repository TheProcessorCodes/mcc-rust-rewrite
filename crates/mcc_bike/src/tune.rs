//! The physics tuning menu: 0 on the dirt bike (LB + X on a pad) opens a
//! column of sliders over the ride (`reflex::Tuning`); 0 or Esc (B, or
//! LB + X again, on a pad) closes it. The bike stands still while it is open.
//!
//! The first slider picks the model the others start from: Unleashed's
//! Pro 250 as it is, Reflex v1 (`reflex.rs`), or Reflex itself (`exact.rs`,
//! Reflex's own simulation; only with Reflex installed). Reflex itself takes
//! only its own three sliders (engine power, gearing, traction) and none of
//! the others: it rides as the game does. Drag a slider with the
//! mouse, or pick one with W / S, the up / down arrows or the d-pad and move
//! it with A / D, left / right. Right-click a slider, or press Backspace (X
//! on a pad), to put it back to stock; Delete (Y on a pad) puts them all
//! back. Closing the menu saves the values to `bike-tuning.json` in the
//! data folder, and the next start loads them.
//!
//! Below the sliders are five setup slots, kept in `bike-setups.json` in
//! the data folder. Pick the "Save to slot" or "Load slot" row, choose a slot with
//! A / D and press Enter (A on a pad), or click a slot. Shift or Ctrl with
//! 1 to 5 saves to a slot; 1 to 5 alone loads one.

use bevy::input::InputSystems;
use bevy::prelude::*;
use mcc_core::map::MapAppExt;
use bevy::ui::UiGlobalTransform;
use bevy::window::PrimaryWindow;

use mcc_core::pause::{Overlay, OverlayInput, Paused};
use mcc_core::skate::RiderPose;

use super::physics;
use super::reflex::Tuning;
use super::unleashed::{DEG, RPM, Setup};

pub struct TunePlugin;

impl Plugin for TunePlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<TuneMenu>()
            .init_map_resource::<Overlay>()
            .insert_resource(BikeTuning(load()))
            .insert_resource(load_setups())
            .add_systems(mcc_core::map::MapStartup, (spawn_menu, apply))
            .add_systems(PreUpdate, toggle.after(InputSystems).in_set(OverlayInput).run_if(mcc_core::map::playing))
            .add_systems(Update, (adjust, show, apply.run_if(resource_changed::<BikeTuning>)).chain().run_if(mcc_core::map::ready));
    }
}

/// The physics the tuning menu has set; every bike rides on it.
#[derive(Resource, Default)]
pub struct BikeTuning(pub Tuning);

/// Whether the menu is up, the selected slider and the one being dragged.
#[derive(Resource, Default)]
pub struct TuneMenu {
    pub open: bool,
    /// A slider, or `SAVE_ROW` / `LOAD_ROW`.
    selected: usize,
    dragging: Option<usize>,
    /// The slot the save and load rows point at.
    slot: usize,
    /// What the last save or load did, shown under the sliders.
    note: String,
}

/// How many setups can be saved.
pub const SLOTS: usize = 5;
/// The menu rows after the sliders.
const SAVE_ROW: usize = KNOBS.len();
const LOAD_ROW: usize = KNOBS.len() + 1;
const ROWS: usize = KNOBS.len() + 2;

/// The saved setups, slot by slot.
#[derive(Resource, Default)]
pub struct Setups(pub [Option<Tuning>; SLOTS]);

/// A run condition: the menu is closed.
pub fn closed(menu: Res<TuneMenu>) -> bool {
    !menu.open
}

/// Hands the tuning to the physics.
fn apply(tuning: Res<BikeTuning>) {
    physics::set_tuning(&tuning.0);
}

/// Reads a saved JSON file (`game_paths::read_saved`), if it is there.
fn read_json(name: &str) -> Option<serde_json::Value> {
    let text = game_paths::read_saved(name)?;
    let value = serde_json::from_str(&text);
    if value.is_err() {
        warn!("bike: {name} is not valid JSON; ignoring it");
    }
    value.ok()
}

fn write_json(name: &str, value: &serde_json::Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    if let Err(e) = game_paths::write_saved(name, &text) {
        warn!("bike: {e}");
    }
}

/// A tuning from its saved values. Unknown or missing values fall back to
/// stock.
fn from_json(saved: &serde_json::Map<String, serde_json::Value>) -> Tuning {
    let mut t = Tuning::default();
    for k in &KNOBS {
        if let Some(v) = saved.get(k.key).and_then(|v| v.as_f64()) {
            (k.set)(&mut t, (v as f32).clamp(k.range.0, k.range.1));
        }
    }
    t
}

fn to_json(t: &Tuning) -> serde_json::Value {
    let map: serde_json::Map<String, serde_json::Value> =
        KNOBS.iter().map(|k| (k.key.to_string(), serde_json::json!((k.get)(t)))).collect();
    serde_json::Value::Object(map)
}

/// The arcade model (Unleashed 0 or Reflex v1 1) to go back to when the
/// physics switch leaves Reflex.
static ARCADE_MODEL: std::sync::Mutex<f32> = std::sync::Mutex::new(1.0);

/// The physics switch of the main menu's Settings and the pause menu:
/// Arcade (the bike's own model, Unleashed or Reflex v1 as the tuning menu
/// last had it) or Reflex (Reflex's own simulation). Saves the tuning when
/// it changes. Reflex needs the player's Reflex install: without it this
/// stays on Arcade and returns false.
pub fn switch_physics(t: &mut Tuning, reflex: bool) -> bool {
    if reflex && super::exact::vehicle().is_none() {
        return false;
    }
    let mut arcade = ARCADE_MODEL.lock().unwrap_or_else(|e| e.into_inner());
    if reflex == (t.model >= 1.5) {
        return true;
    }
    if reflex {
        *arcade = t.model;
        t.model = 2.0;
    } else {
        t.model = *arcade;
    }
    save(t);
    info!("bike: physics switched to {}", if reflex { "Reflex" } else { "Arcade" });
    true
}

/// The tuning ridden last time, or stock.
pub fn load() -> Tuning {
    match read_json("bike-tuning.json") {
        Some(serde_json::Value::Object(saved)) => from_json(&saved),
        Some(_) => {
            warn!("bike: bike-tuning.json is not a JSON object; riding stock");
            Tuning::default()
        }
        None => Tuning::default(),
    }
}

/// Saves the tuning to `bike-tuning.json` (the pause menu's physics switch
/// saves through this too).
pub fn save(t: &Tuning) {
    write_json("bike-tuning.json", &to_json(t));
}

/// The saved setups: `{"slots": [null or a tuning, ...]}`.
fn load_setups() -> Setups {
    let mut setups = Setups::default();
    let Some(value) = read_json("bike-setups.json") else {
        return setups;
    };
    let Some(slots) = value.get("slots").and_then(|s| s.as_array()) else {
        warn!("bike: bike-setups.json has no slots list; ignoring it");
        return setups;
    };
    for (slot, saved) in setups.0.iter_mut().zip(slots) {
        *slot = saved.as_object().map(from_json);
    }
    setups
}

fn save_setups(setups: &Setups) {
    let slots: Vec<_> = setups.0.iter().map(|s| s.as_ref().map_or(serde_json::Value::Null, to_json)).collect();
    write_json("bike-setups.json", &serde_json::json!({ "slots": slots }));
}

/// Whether a slider is off its stock value.
fn differs(k: &Knob, t: &Tuning, stock: &Tuning) -> bool {
    ((k.get)(t) - (k.get)(stock)).abs() > k.step * 0.25
}

/// A saved setup in a few words: its base model and how far it is from stock.
fn describe(t: &Tuning) -> String {
    let model = model_name(t.model);
    // The model slider itself is not a change from that model's stock.
    let stock = Tuning::default();
    match KNOBS[1..].iter().filter(|k| differs(k, t, &stock)).count() {
        0 => format!("{model}, stock"),
        n => format!("{model}, {n} changed"),
    }
}

/// The model slider's value in words.
fn model_name(model: f32) -> &'static str {
    if model >= 1.5 {
        "Reflex"
    } else if model >= 0.5 {
        "Arcade (Reflex v1)"
    } else {
        "Unleashed"
    }
}

/// One slider: a name, what each end does, its range, the step a key press
/// moves it, and how it reads and writes the tuning.
struct Knob {
    /// The name in `bike-tuning.json`.
    key: &'static str,
    group: &'static str,
    name: &'static str,
    low: &'static str,
    high: &'static str,
    range: (f32, f32),
    step: f32,
    get: fn(&Tuning) -> f32,
    set: fn(&mut Tuning, f32),
    /// What the value means, read off the tuned bike, in the units a rider
    /// would use.
    show: fn(&Setup, f32) -> String,
}

const KMH: f32 = 3.6;

fn peak_torque(s: &Setup) -> (f32, f32) {
    s.torque_curve.iter().fold((0.0, 0.0), |a, &(rpm, t)| if t > a.1 { (rpm, t) } else { a })
}

/// Road speed at the top of a gear, km/h.
fn gear_top(s: &Setup, gear: usize) -> f32 {
    s.max_rpm() / RPM / s.ratio(gear) * s.rear.radius * KMH
}

const KNOBS: [Knob; 23] = [
    Knob {
        key: "model",
        group: "Model",
        name: "Base physics",
        low: "Unleashed",
        high: "Reflex",
        range: (0.0, 2.0),
        step: 1.0,
        get: |t| t.model,
        set: |t, v| t.model = v.round(),
        show: |s, v| {
            if v >= 1.5 {
                if super::exact::vehicle().is_some() {
                    "Reflex: the game's own simulation, 450F (the other sliders do nothing)".into()
                } else {
                    "Reflex: needs MX vs ATV Reflex installed; riding Reflex v1".into()
                }
            } else {
                format!("{}: {:.0} kg with the rider", s.name, s.mass)
            }
        },
    },
    Knob {
        key: "reflex_power",
        group: "Reflex itself",
        name: "Reflex engine power",
        low: "less",
        high: "more",
        range: (0.5, 2.0),
        step: 0.05,
        get: |t| t.reflex_power,
        set: |t, v| t.reflex_power = v,
        show: |_, v| match super::exact::peak_torque() {
            Some((nm, rpm)) => format!("peak {:.0} N·m at {rpm:.0} rpm (x{v:.2})", nm * v),
            None => "needs MX vs ATV Reflex installed".into(),
        },
    },
    Knob {
        key: "reflex_gearing",
        group: "",
        name: "Reflex gearing",
        low: "taller",
        high: "shorter",
        range: (0.6, 1.5),
        step: 0.01,
        get: |t| t.reflex_gearing,
        set: |t, v| t.reflex_gearing = v,
        show: |_, v| match super::exact::top_of_fifth(v) {
            Some(kmh) => format!("{:.0} km/h ({:.0} mph) at the limiter in 5th (x{v:.2})", kmh, kmh / 1.609),
            None => "needs MX vs ATV Reflex installed".into(),
        },
    },
    Knob {
        key: "reflex_traction",
        group: "",
        name: "Reflex traction",
        low: "spins",
        high: "hooks up",
        range: (0.5, 3.0),
        step: 0.05,
        get: |t| t.reflex_traction,
        set: |t, v| t.reflex_traction = v,
        show: |_, v| format!("x{v:.2} the tyres' grip for drive and brakes (corners as Reflex)"),
    },
    Knob {
        key: "power",
        group: "Engine and brakes",
        name: "Engine power",
        low: "less",
        high: "more",
        range: (0.3, 2.5),
        step: 0.05,
        get: |t| t.power,
        set: |t, v| t.power = v,
        show: |s, v| {
            let (rpm, nm) = peak_torque(s);
            format!("peak {nm:.0} N·m at {rpm:.0} rpm, x{v:.2}")
        },
    },
    Knob {
        key: "gearing",
        group: "",
        name: "Gearing",
        low: "taller (faster)",
        high: "shorter (punchier)",
        range: (0.6, 1.6),
        step: 0.02,
        get: |t| t.gearing,
        set: |t, v| t.gearing = v,
        show: |s, _| {
            format!(
                "limiter at {:.0} km/h in 1st, {:.0} km/h in 5th",
                gear_top(s, 0),
                gear_top(s, s.gears.len() - 1)
            )
        },
    },
    Knob {
        key: "engine_brake",
        group: "",
        name: "Engine braking",
        low: "freewheels",
        high: "holds back",
        range: (0.0, 4.0),
        step: 0.1,
        get: |t| t.engine_brake,
        set: |t, v| t.engine_brake = v,
        show: |s, _| {
            let rpm = 8000.0;
            format!("{:.0} N·m drag at {rpm:.0} rpm off the gas", s.engine_braking * rpm * rpm)
        },
    },
    Knob {
        key: "brakes",
        group: "",
        name: "Brake power",
        low: "weaker",
        high: "stronger",
        range: (0.3, 2.5),
        step: 0.05,
        get: |t| t.brakes,
        set: |t, v| t.brakes = v,
        show: |s, _| {
            format!(
                "front {:.0} / rear {:.0} N·m",
                s.front.max_braking_torque, s.rear.max_braking_torque
            )
        },
    },
    Knob {
        key: "grip",
        group: "Tyres",
        name: "Grip",
        low: "less grip",
        high: "more grip",
        range: (0.5, 2.5),
        step: 0.05,
        get: |t| t.grip,
        set: |t, v| t.grip = v,
        show: |s, _| {
            let side = if s.rear.side_cap.is_finite() {
                format!("{:.2}", s.rear.side_cap)
            } else {
                "uncapped".into()
            };
            format!("drive {:.2} / side {side} x the load", s.rear.max_drive)
        },
    },
    Knob {
        key: "side_stiffness",
        group: "",
        name: "Side stiffness",
        low: "softer, drifty",
        high: "stiffer, planted",
        range: (0.3, 4.0),
        step: 0.1,
        get: |t| t.side_stiffness,
        set: |t, v| t.side_stiffness = v,
        show: |s, _| {
            // The slip at which the side grip reaches the load.
            let c = &s.rear.side;
            let at = (1..2000)
                .map(|i| i as f32 * 0.01)
                .find(|&v| -c.grip(super::unleashed::ft(v) * s.rear.slip_scale) >= 1.0);
            match at {
                Some(v) => format!("full grip at {:.1} km/h of sideways slip", v * KMH),
                None => "never reaches full grip".into(),
            }
        },
    },
    Knob {
        key: "slide_keep",
        group: "",
        name: "Slide grip",
        low: "slides run on",
        high: "slides catch",
        range: (0.5, 1.2),
        step: 0.05,
        get: |t| t.slide_keep,
        set: |t, v| t.slide_keep = v,
        show: |s, _| {
            if s.rear.side_cap.is_finite() {
                format!("a sliding tyre keeps {:.0} % of its grip", s.rear.slide_keep * 100.0)
            } else {
                "no effect: Unleashed's grip has no cap".into()
            }
        },
    },
    Knob {
        key: "steer_lock",
        group: "Steering and lean",
        name: "Steering lock",
        low: "less lock",
        high: "more lock",
        range: (0.4, 1.8),
        step: 0.05,
        get: |t| t.steer_lock,
        set: |t, v| t.steer_lock = v,
        show: |s, _| {
            let deg = |kmh: f32| s.steer_lock(kmh / KMH) / DEG;
            format!(
                "full stick {:.0} / {:.0} / {:.0} deg at 0 / 50 / 80 km/h",
                deg(0.0),
                deg(50.0),
                deg(80.0)
            )
        },
    },
    Knob {
        key: "steer_lag",
        group: "",
        name: "Steering lag",
        low: "quicker",
        high: "lazier",
        range: (-0.01, 0.5),
        step: 0.01,
        get: |t| t.steer_lag,
        set: |t, v| t.steer_lag = if v < 0.0 { -0.01 } else { v },
        show: |s, v| {
            let own = if v < 0.0 { " (the model's own)" } else { "" };
            if s.steer_lag > 0.0 {
                format!("the bars lag the stick {:.2} s{own}", s.steer_lag)
            } else {
                format!("the bars follow the stick at once{own}")
            }
        },
    },
    Knob {
        key: "lean_depth",
        group: "",
        name: "Lean depth",
        low: "shallower",
        high: "deeper",
        range: (0.4, 2.0),
        step: 0.05,
        get: |t| t.lean_depth,
        set: |t, v| t.lean_depth = v,
        show: |s, _| {
            let deg = |kmh: f32| s.lean_for(kmh / KMH) / DEG;
            format!("full stick leans {:.0} / {:.0} deg at 20 / 60 km/h", deg(20.0), deg(60.0))
        },
    },
    Knob {
        key: "lean_speed",
        group: "",
        name: "Lean speed",
        low: "lazier",
        high: "snappier",
        range: (0.3, 3.0),
        step: 0.05,
        get: |t| t.lean_speed,
        set: |t, v| t.lean_speed = v,
        show: |s, _| format!("the lean lags {:.2} s", 1.0 / s.roll_gain),
    },
    Knob {
        key: "rider_lean",
        group: "",
        name: "Rider-lean influence",
        low: "less (stick)",
        high: "more (rider)",
        range: (0.0, 2.5),
        step: 0.05,
        get: |t| t.rider_lean,
        set: |t, v| t.rider_lean = v,
        show: |s, _| {
            format!(
                "full rider lean adds {:.0} % lean and {:.0} % bars",
                s.rider_lean * 100.0,
                s.rider_steer * 100.0
            )
        },
    },
    Knob {
        key: "weight_shift",
        group: "Rider",
        name: "Weight shift",
        low: "less",
        high: "more",
        range: (0.0, 3.0),
        step: 0.1,
        get: |t| t.weight_shift,
        set: |t, v| t.weight_shift = v,
        show: |s, _| {
            format!(
                "moves the centre of gravity {:.0} cm back / {:.0} cm forward",
                s.cofg_travel_back * 100.0,
                s.cofg_travel_forward * 100.0
            )
        },
    },
    Knob {
        key: "wheelie_hold",
        group: "",
        name: "Wheelie hold",
        low: "looser",
        high: "firmer",
        range: (0.0, 3.0),
        step: 0.1,
        get: |t| t.wheelie_hold,
        set: |t, v| t.wheelie_hold = v,
        show: |s, v| format!("x{v:.1}: holds up to {:.0} deg nose up", s.max_pitch / DEG),
    },
    Knob {
        key: "spring",
        group: "Suspension",
        name: "Stiffness",
        low: "softer",
        high: "stiffer",
        range: (0.4, 2.5),
        step: 0.05,
        get: |t| t.spring,
        set: |t, v| t.spring = v,
        show: |s, _| {
            format!(
                "springs front {:.1} / rear {:.1} kN/m",
                s.spring_rate(&s.front) / 1000.0,
                s.spring_rate(&s.rear) / 1000.0
            )
        },
    },
    Knob {
        key: "damper",
        group: "",
        name: "Damping",
        low: "bouncier",
        high: "calmer",
        range: (0.3, 3.0),
        step: 0.1,
        get: |t| t.damper,
        set: |t, v| t.damper = v,
        show: |s, _| {
            let (f, r) = (&s.front, &s.rear);
            format!(
                "bump / rebound front {:.0} / {:.0}, rear {:.0} / {:.0} N·s/m",
                s.damping(f, f.rebound),
                s.damping(f, f.bounce),
                s.damping(r, r.rebound),
                s.damping(r, r.bounce)
            )
        },
    },
    Knob {
        key: "air_spin",
        group: "Air",
        name: "Air rotation",
        low: "lazier",
        high: "snappier",
        range: (0.3, 3.0),
        step: 0.05,
        get: |t| t.air_spin,
        set: |t, v| t.air_spin = v,
        show: |s, _| format!("full stick pitches with {:.0} N·m", s.air_moment.x.abs()),
    },
    Knob {
        key: "air_damping",
        group: "",
        name: "Air damping",
        low: "spins on",
        high: "stops quickly",
        range: (0.1, 3.0),
        step: 0.05,
        get: |t| t.air_damping,
        set: |t, v| t.air_damping = v,
        show: |s, _| {
            let rate = s.air_moment.x.abs() / s.air_damping;
            format!("full stick flips at up to {:.0} deg/s", rate / DEG)
        },
    },
    Knob {
        key: "gravity",
        group: "",
        name: "Gravity",
        low: "floaty",
        high: "heavy",
        range: (0.5, 1.5),
        step: 0.01,
        get: |t| t.gravity,
        set: |t, v| t.gravity = v,
        show: |s, _| format!("{:.2} g", s.gravity_scale),
    },
];

const DIM: Color = Color::srgb(0.65, 0.70, 0.78);
const LIT: Color = Color::srgb(1.0, 0.82, 0.35);
const CHANGED: Color = Color::srgb(0.55, 0.85, 1.0);
const TRACK: Color = Color::srgb(0.22, 0.25, 0.30);
const FILL: Color = Color::srgb(0.85, 0.55, 0.15);
const TRACK_WIDTH: f32 = 220.0;

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

/// A slot's button on the save row or the load row.
#[derive(Component)]
struct SlotCell {
    row: usize,
    slot: usize,
}

/// A slot's description on the load row.
#[derive(Component)]
struct SlotText(usize);

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
                    row_gap: Val::Px(2.0),
                    padding: UiRect::all(Val::Px(20.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.07, 0.10, 0.90)),
            ))
            .with_children(|panel| {
                panel.spawn(text("BIKE PHYSICS", 28.0, Color::WHITE));
                panel.spawn(text(
                    "Drag a slider, or W / S to pick and A / D to move (d-pad on a pad).  \
                     Right-click or Backspace: back to stock.  Delete: all to stock.  0 or Esc: ride.\n\
                     Shift+1 to 5 saves the setup to a slot, 1 to 5 loads one (or pick a slot below and press Enter).",
                    14.0,
                    DIM,
                ));
                for (i, k) in KNOBS.iter().enumerate() {
                    if !k.group.is_empty() {
                        panel.spawn((
                            Node {
                                margin: UiRect::top(Val::Px(6.0)),
                                ..default()
                            },
                            text(k.group, 15.0, DIM),
                        ));
                    }
                    panel
                        .spawn((
                            Row(i),
                            Node {
                                align_items: AlignItems::Center,
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(1.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|r| {
                            r.spawn(cell(180.0)).with_child(text(k.name, 16.0, Color::WHITE));
                            r.spawn(Node {
                                width: Val::Px(110.0),
                                justify_content: JustifyContent::End,
                                margin: UiRect::right(Val::Px(10.0)),
                                ..default()
                            })
                            .with_child(text(k.low, 13.0, DIM));
                            // The track, tall enough to grab, with the bar inside.
                            r.spawn((
                                Track(i),
                                Node {
                                    width: Val::Px(TRACK_WIDTH),
                                    height: Val::Px(20.0),
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
                                width: Val::Px(120.0),
                                margin: UiRect::left(Val::Px(10.0)),
                                ..default()
                            })
                            .with_child(text(k.high, 13.0, DIM));
                            r.spawn(cell(470.0)).with_child((Readout(i), text("", 14.0, DIM)));
                        });
                }
                panel.spawn((
                    Node {
                        margin: UiRect::top(Val::Px(6.0)),
                        ..default()
                    },
                    text("Saved setups", 15.0, DIM),
                ));
                for (row, name, width) in [(SAVE_ROW, "Save to slot", 60.0), (LOAD_ROW, "Load slot", 170.0)] {
                    panel
                        .spawn((
                            Row(row),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(6.0),
                                padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_children(|r| {
                            r.spawn(cell(180.0)).with_child(text(name, 16.0, Color::WHITE));
                            for slot in 0..SLOTS {
                                let mut button = r.spawn((
                                    SlotCell { row, slot },
                                    Node {
                                        width: Val::Px(width),
                                        padding: UiRect::axes(Val::Px(6.0), Val::Px(2.0)),
                                        ..default()
                                    },
                                    BackgroundColor(TRACK),
                                ));
                                if row == LOAD_ROW {
                                    button.with_child((SlotText(slot), text("", 13.0, DIM)));
                                } else {
                                    button.with_child(text(format!("{}", slot + 1), 14.0, Color::WHITE));
                                }
                            }
                        });
                }
                panel.spawn((
                    Summary,
                    Node {
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },
                    text("", 14.0, DIM),
                ));
            });
        });
}

/// LB + X on a pad: X pressed with LB held (LB alone is the session
/// marker's chord, with the d-pad).
fn pad_chord(gamepads: &Query<&Gamepad>) -> bool {
    gamepads.iter().any(|p| p.pressed(GamepadButton::LeftTrigger) && p.just_pressed(GamepadButton::West))
}

/// 0, or LB + X on a pad, opens and closes the menu while riding; Esc or B
/// on a pad close it.
#[allow(clippy::too_many_arguments)]
fn toggle(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    paused: Res<Paused>,
    rider: Res<RiderPose>,
    tuning: Res<BikeTuning>,
    mut menu: ResMut<TuneMenu>,
    mut overlay: ResMut<Overlay>,
    mut menus: Query<&mut Visibility, With<Menu>>,
) {
    let zero = keys.just_pressed(KeyCode::Digit0) || keys.just_pressed(KeyCode::Numpad0) || pad_chord(&gamepads);
    let open = if menu.open {
        let escape = keys.just_pressed(KeyCode::Escape);
        if escape {
            // Esc closes this menu, not the game behind it.
            keys.clear_just_pressed(KeyCode::Escape);
        }
        let back = gamepads.iter().any(|p| p.just_pressed(GamepadButton::East));
        !(zero || escape || back) && rider.bike && !paused.0
    } else {
        zero && rider.bike && !paused.0
    };
    if open == menu.open {
        return;
    }
    if !open {
        save(&tuning.0);
    }
    menu.open = open;
    menu.dragging = None;
    menu.note.clear();
    overlay.0 = open;
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

/// Sets a knob, writing the resource only when the value really changes
/// (so `apply` rebuilds the bike only then).
fn put(tuning: &mut ResMut<BikeTuning>, k: &Knob, v: f32) {
    let mut t = tuning.0;
    (k.set)(&mut t, v);
    if t != tuning.0 {
        tuning.0 = t;
    }
}

/// Saves the tuning to a slot (`SAVE_ROW`) or loads a slot (`LOAD_ROW`).
fn use_slot(
    row: usize,
    slot: usize,
    menu: &mut TuneMenu,
    tuning: &mut ResMut<BikeTuning>,
    setups: &mut ResMut<Setups>,
) {
    let n = slot + 1;
    if row == SAVE_ROW {
        setups.0[slot] = Some(tuning.0);
        save_setups(setups);
        menu.note = format!("Saved to slot {n}.");
    } else if let Some(t) = setups.0[slot] {
        if t != tuning.0 {
            tuning.0 = t;
        }
        menu.note = format!("Loaded slot {n}.");
    } else {
        menu.note = format!("Slot {n} is empty.");
    }
}

/// The slot button under the cursor.
fn slot_under_cursor(
    cursor: Option<Vec2>,
    cells: &Query<(&SlotCell, &ComputedNode, &UiGlobalTransform)>,
) -> Option<(usize, usize)> {
    let at = cursor?;
    cells.iter().find_map(|(cell, node, transform)| {
        let local = transform.try_inverse()?.transform_point2(at);
        let half = node.size() * 0.5;
        (local.x.abs() <= half.x && local.y.abs() <= half.y).then_some((cell.row, cell.slot))
    })
}

const DIGITS: [KeyCode; SLOTS] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5];

#[allow(clippy::too_many_arguments)]
fn adjust(
    mut menu: ResMut<TuneMenu>,
    mut tuning: ResMut<BikeTuning>,
    mut setups: ResMut<Setups>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    tracks: Query<(&Track, &ComputedNode, &UiGlobalTransform)>,
    cells: Query<(&SlotCell, &ComputedNode, &UiGlobalTransform)>,
) {
    if !menu.open {
        return;
    }
    let key = |a: KeyCode, b: KeyCode| keys.just_pressed(a) || keys.just_pressed(b);
    let pad = |b: GamepadButton| gamepads.iter().any(|p| p.just_pressed(b));
    if key(KeyCode::KeyS, KeyCode::ArrowDown) || pad(GamepadButton::DPadDown) {
        menu.selected = (menu.selected + 1) % ROWS;
    }
    if key(KeyCode::KeyW, KeyCode::ArrowUp) || pad(GamepadButton::DPadUp) {
        menu.selected = (menu.selected + ROWS - 1) % ROWS;
    }
    let stock = Tuning::default();
    let mut nudge = 0.0;
    if key(KeyCode::KeyD, KeyCode::ArrowRight) || pad(GamepadButton::DPadRight) {
        nudge += 1.0;
    }
    if key(KeyCode::KeyA, KeyCode::ArrowLeft) || pad(GamepadButton::DPadLeft) {
        nudge -= 1.0;
    }
    if let Some(k) = KNOBS.get(menu.selected) {
        if nudge != 0.0 {
            let v = ((k.get)(&tuning.0) + nudge * k.step).clamp(k.range.0, k.range.1);
            put(&mut tuning, k, v);
        }
        // X, not the LB + X that opens and closes the menu.
        let lb = gamepads.iter().any(|p| p.pressed(GamepadButton::LeftTrigger));
        if keys.just_pressed(KeyCode::Backspace) || (pad(GamepadButton::West) && !lb) {
            put(&mut tuning, k, (k.get)(&stock));
        }
    } else {
        // The save and load rows: A / D pick a slot, Enter uses it.
        menu.slot = (menu.slot as f32 + nudge).rem_euclid(SLOTS as f32) as usize;
        if key(KeyCode::Enter, KeyCode::NumpadEnter) || keys.just_pressed(KeyCode::Space) || pad(GamepadButton::South) {
            let (row, slot) = (menu.selected, menu.slot);
            use_slot(row, slot, &mut menu, &mut tuning, &mut setups);
        }
    }
    if (keys.just_pressed(KeyCode::Delete) || pad(GamepadButton::North)) && tuning.0 != stock {
        tuning.0 = stock;
    }
    // 1 to 5 load a slot; with Shift or Ctrl they save to it.
    let held = |a: KeyCode, b: KeyCode| keys.pressed(a) || keys.pressed(b);
    let saving = held(KeyCode::ShiftLeft, KeyCode::ShiftRight) || held(KeyCode::ControlLeft, KeyCode::ControlRight);
    if let Some(slot) = DIGITS.iter().position(|&d| keys.just_pressed(d)) {
        let row = if saving { SAVE_ROW } else { LOAD_ROW };
        use_slot(row, slot, &mut menu, &mut tuning, &mut setups);
    }

    // The mouse: press on a slider to grab it, drag to move it; click a
    // slot to save or load it.
    let cursor = windows.single().ok().and_then(Window::physical_cursor_position);
    if mouse.just_pressed(MouseButton::Left) {
        menu.dragging = under_cursor(cursor, &tracks, None).map(|(i, _)| i);
        if let Some((row, slot)) = slot_under_cursor(cursor, &cells) {
            menu.selected = row;
            menu.slot = slot;
            use_slot(row, slot, &mut menu, &mut tuning, &mut setups);
        }
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
            put(&mut tuning, k, v);
        }
    }
    if mouse.just_pressed(MouseButton::Right)
        && let Some((i, _)) = under_cursor(cursor, &tracks, None)
    {
        menu.selected = i;
        put(&mut tuning, &KNOBS[i], (KNOBS[i].get)(&stock));
    }
}

/// Fills, readouts and the highlight follow the values.
#[allow(clippy::type_complexity)]
fn show(
    menu: Res<TuneMenu>,
    tuning: Res<BikeTuning>,
    mut fills: Query<(&Fill, &mut Node), Without<Stock>>,
    mut rows: Query<(&Row, &mut BackgroundColor), Without<SlotCell>>,
    mut readouts: Query<(&Readout, &mut Text, &mut TextColor), Without<Summary>>,
    mut summary: Query<&mut Text, With<Summary>>,
    setups: Res<Setups>,
    mut cells: Query<(&SlotCell, &mut BackgroundColor), Without<Row>>,
    mut slot_texts: Query<(&SlotText, &mut Text, &mut TextColor), (Without<Readout>, Without<Summary>)>,
) {
    if !menu.open {
        return;
    }
    let t = &tuning.0;
    let stock = Tuning::default();
    let tuned = t.setup();
    let base = stock.setup();
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
        let off = differs(k, t, &stock);
        changed += off as usize;
        let line = if off {
            format!("{}  (stock: {})", (k.show)(&tuned, v), (k.show)(&base, s))
        } else {
            (k.show)(&tuned, v)
        };
        if text.0 != line {
            text.0 = line;
        }
        let want = if r.0 == menu.selected {
            LIT
        } else if off {
            CHANGED
        } else {
            DIM
        };
        if colour.0 != want {
            colour.0 = want;
        }
    }
    for (cell, mut bg) in &mut cells {
        let want = if menu.selected == cell.row && menu.slot == cell.slot {
            Color::srgba(1.0, 0.82, 0.35, 0.45)
        } else if cell.row == LOAD_ROW && setups.0[cell.slot].is_some() {
            Color::srgb(0.18, 0.32, 0.42)
        } else {
            TRACK
        };
        if bg.0 != want {
            bg.0 = want;
        }
    }
    for (st, mut text, mut colour) in &mut slot_texts {
        let saved = setups.0[st.0].as_ref();
        let line = match saved {
            Some(s) => format!("{}  {}", st.0 + 1, describe(s)),
            None => format!("{}  empty", st.0 + 1),
        };
        if text.0 != line {
            text.0 = line;
        }
        // The slot that matches what is being ridden stands out.
        let want = if saved == Some(t) { Color::WHITE } else { DIM };
        if colour.0 != want {
            colour.0 = want;
        }
    }
    if let Ok(mut s) = summary.single_mut() {
        let mut line = match changed {
            0 => "All stock (Reflex v1).".to_string(),
            1 => "1 slider differs from stock.".to_string(),
            n => format!("{n} sliders differ from stock."),
        };
        if !menu.note.is_empty() {
            line = format!("{line}  {}", menu.note);
        }
        if s.0 != line {
            s.0 = line;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knobs_hold_their_stock_values() {
        let stock = Tuning::default();
        for k in &KNOBS {
            let v = (k.get)(&stock);
            assert!(v >= k.range.0 && v <= k.range.1, "{} stock {v} outside {:?}", k.name, k.range);
            let mut t = stock;
            (k.set)(&mut t, v);
            assert_eq!(t, stock, "{}", k.name);
            // Every readout works on both models.
            let _ = (k.show)(&stock.setup(), v);
            let _ = (k.show)(&Tuning { model: 0.0, ..stock }.setup(), v);
        }
        let mut keys: Vec<_> = KNOBS.iter().map(|k| k.key).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), KNOBS.len());
        // `describe` skips the first knob as the model choice.
        assert_eq!(KNOBS[0].key, "model");
    }

    #[test]
    fn setups_survive_a_round_trip() {
        let stock = Tuning::default();
        let tuned = Tuning { power: 1.5, model: 0.0, ..stock };
        let back = from_json(to_json(&tuned).as_object().unwrap());
        assert_eq!(back, tuned);
        assert_eq!(describe(&stock), "Arcade (Reflex v1), stock");
        assert_eq!(describe(&Tuning { model: 0.0, ..stock }), "Unleashed, stock");
        assert_eq!(describe(&tuned), "Unleashed, 1 changed");
    }
}
