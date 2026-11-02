//! The skater's state other crates read: the pose (`halo3_rider` draws it,
//! `halo3_bike` borrows it), the ride (`halo3_audio`), the respawn
//! request (`halo3_swim`) and the camera marker. The engine that fills
//! them is `halo3_skate`.

use std::path::PathBuf;

use bevy::prelude::*;

/// What IW4L ships: a Spartan-sized skater (`docs/skate-interface.md` 10).
pub const DEFAULT_SCALE: f32 = 33.0 * 0.0254;

/// Whether skating is on (Settings on the main menu, `game_paths::skating_enabled`).
/// Read as a map starts: off, `halo3_skate` builds no grind rails or skate
/// session, `halo3_rider` loads no board, and J does nothing. `main` inserts
/// it and the menu's settings screen changes it; without it, skating is on.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkateEnabled(pub bool);

impl SkateEnabled {
    /// The saved choice.
    pub fn load() -> SkateEnabled {
        SkateEnabled(game_paths::skating_enabled())
    }
}

/// Whether skating is on, for systems that take `Option<Res<SkateEnabled>>`.
pub fn skating_on(enabled: Option<&SkateEnabled>) -> bool {
    enabled.is_none_or(|e| e.0)
}

/// How the ride is going, for the board sounds (`audio/`): whether the
/// skater is riding, the board speed in m/s and the engine's state name.
#[derive(Resource, Default, Clone, Debug)]
pub struct SkateMotion {
    pub riding: bool,
    pub speed: f32,
    pub state: String,
}

/// The latest pose for drawing the rider, in skate space (`docs/skate-interface.md`
/// section 4); divide by `scale` for the renderer frame.
#[derive(Resource)]
pub struct RiderPose {
    pub active: bool,
    pub names: Vec<String>,
    pub bones: Vec<Mat4>,
    pub scale: f32,
    /// The engine's chase camera in the renderer frame, and its vertical FOV
    /// in radians.
    pub camera: Option<(Transform, f32)>,
    /// Whether `update` puts the view camera on the engine's camera.
    pub drive_camera: bool,
    /// Seconds of input fed since the skater was dropped in.
    pub skated: f32,
    /// A model draws the rider: no stick figure.
    pub drawn: bool,
    /// The dirt bike holds the rider (`bike`): no board, no skating.
    pub bike: bool,
    /// The Spartan is on foot (`foot`): no board, no skating.
    pub foot: bool,
    /// The camera is in his eyes: his body is not drawn.
    pub first_person: bool,
}

impl Default for RiderPose {
    fn default() -> Self {
        RiderPose {
            active: false,
            names: Vec::new(),
            bones: Vec::new(),
            scale: scale(),
            camera: None,
            drive_camera: true,
            skated: 0.0,
            drawn: false,
            bike: false,
            foot: false,
            first_person: false,
        }
    }
}

/// Put the riding skater back at a Halo position (world units) facing a
/// Halo yaw (radians), through `Session::activate`; taken by `update`
/// (`swim.rs` uses it after the skater sinks).
#[derive(Resource, Default)]
pub struct Respawn(pub Option<(Vec3, f32)>);

/// The rider jumping off the board or the bike onto his feet, keeping the
/// ride's momentum: set by `halo3_skate` / `halo3_bike` as the ride ends,
/// taken by `halo3_foot`.
#[derive(Resource, Default)]
pub struct Dismount(pub Option<Bail>);

/// Where the rider left the ride, in the renderer frame.
#[derive(Clone, Copy, Debug)]
pub struct Bail {
    /// His feet, metres.
    pub position: Vec3,
    /// The ride's velocity, m/s.
    pub velocity: Vec3,
    /// Where the camera looked: `FlyCamera` yaw and pitch.
    pub yaw: f32,
    pub pitch: f32,
}

impl Bail {
    /// From the view camera at the moment of the bail.
    pub fn new(position: Vec3, velocity: Vec3, view: &Transform) -> Bail {
        let f = view.forward().as_vec3();
        Bail {
            position,
            velocity,
            yaw: (-f.x).atan2(-f.z),
            pitch: f.y.clamp(-1.0, 1.0).asin(),
        }
    }
}

/// The Spartan getting back on the bike or the board straight from walking:
/// set by `halo3_foot` as he leaves his feet, taken by `halo3_bike` /
/// `halo3_skate`. The bike keeps his momentum; the board starts from rest.
#[derive(Resource, Default)]
pub struct Mount(pub Option<(Ride, Bail)>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ride {
    Bike,
    Board,
}

/// The bail button, the same while skating and riding: G, or the d-pad's up.
pub fn bail_pressed(keys: &ButtonInput<KeyCode>, pad: Option<&Gamepad>) -> bool {
    keys.just_pressed(KeyCode::KeyG) || pad.is_some_and(dpad_up_alone)
}

/// The d-pad's up pressed without LB: LB + up is Skate 3's session marker,
/// which must not also switch modes.
pub fn dpad_up_alone(pad: &Gamepad) -> bool {
    pad.just_pressed(GamepadButton::DPadUp) && !pad.pressed(GamepadButton::LeftTrigger)
}

/// Marks the camera while the skate engine drives it.
#[derive(Component)]
pub struct Skating;

/// The skate scale `k`: `HALO3_SKATE_SCALE` or the default.
pub fn scale() -> f32 {
    std::env::var("HALO3_SKATE_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|k: &f32| *k > 0.0)
        .unwrap_or(DEFAULT_SCALE)
}

/// The player's converted Skate 3 data, as first-run setup saved it
/// (`halo3_setup`; `%HALO3_SKATE_ASSETS%` overrides), else the folder setup
/// converts into, which stays empty until the player gives Skate 3.
pub fn assets_root() -> PathBuf {
    game_paths::skate_assets()
        .unwrap_or_else(|| game_paths::settings_dir().join("skate-data").join("assets"))
}
