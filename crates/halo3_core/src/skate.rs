//! The skater's state other crates read: the pose (`halo3_rider` draws it,
//! `halo3_bike` borrows it), the ride (`halo3_audio`), the respawn
//! request (`halo3_swim`) and the camera marker. The engine that fills
//! them is `halo3_skate`.

use std::path::PathBuf;

use bevy::prelude::*;

/// What IW4L ships: a Spartan-sized skater (`docs/skate-interface.md` 10).
pub const DEFAULT_SCALE: f32 = 33.0 * 0.0254;
pub const DEFAULT_ASSETS: &str = r"D:\GAME ISO AND ROMS\PC - AI MASHUP\mw2 skate 3\skate-data\assets";

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
        }
    }
}

/// Put the riding skater back at a Halo position (world units) facing a
/// Halo yaw (radians), through `Session::activate`; taken by `update`
/// (`swim.rs` uses it after the skater sinks).
#[derive(Resource, Default)]
pub struct Respawn(pub Option<(Vec3, f32)>);

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

pub fn assets_root() -> PathBuf {
    ["HALO3_SKATE_ASSETS", "IW4L_SKATE_ASSETS"]
        .iter()
        .find_map(std::env::var_os)
        .map_or_else(|| PathBuf::from(DEFAULT_ASSETS), PathBuf::from)
}
