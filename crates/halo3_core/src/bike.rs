//! The dirt bike's state other crates read: the engine sound (`halo3_audio`)
//! follows it. `halo3_bike` fills it every frame. `EngineVolume` is the
//! pause menu's engine slider (`halo3` `pause.rs`), kept in
//! `bike-sound.json` next to the exe.

use bevy::prelude::*;

#[derive(Resource, Default, Clone, Debug)]
pub struct BikeMotion {
    pub riding: bool,
    pub rpm: f32,
    /// Idle and the rev limiter.
    pub min_rpm: f32,
    pub max_rpm: f32,
    /// The gas the engine gets, 0 to 1.
    pub throttle: f32,
    /// m/s.
    pub speed: f32,
    /// 0-based.
    pub gear: usize,
    /// Wheels on the ground.
    pub grounded: usize,
    pub crashed: bool,
    /// The clutch slipping (pulling away, creeping).
    pub clutch_slipping: bool,
    /// The rear tyre spinning or locked.
    pub rear_sliding: bool,
}

/// The engine sound's loudness, 0 to 1.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct EngineVolume(pub f32);

impl Default for EngineVolume {
    fn default() -> Self {
        EngineVolume(0.7)
    }
}

const SAVED: &str = "bike-sound.json";

fn beside_exe() -> Option<std::path::PathBuf> {
    Some(std::env::current_exe().ok()?.parent()?.join(SAVED))
}

impl EngineVolume {
    /// The saved level, or the default.
    pub fn load() -> EngineVolume {
        beside_exe()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v.get("engine_volume")?.as_f64())
            .map_or_else(EngineVolume::default, |v| EngineVolume((v as f32).clamp(0.0, 1.0)))
    }

    pub fn save(self) {
        let Some(path) = beside_exe() else {
            return;
        };
        let text = serde_json::to_string_pretty(&serde_json::json!({ "engine_volume": self.0 })).unwrap_or_default();
        if let Err(e) = std::fs::write(&path, text) {
            warn!("bike: could not save {}: {e}", path.display());
        }
    }
}
