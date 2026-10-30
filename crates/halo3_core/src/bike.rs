//! The dirt bike's state other crates read: the engine sound (`halo3_audio`)
//! follows it. `halo3_bike` fills it every frame.

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
    /// The rear tyre spinning or locked.
    pub rear_sliding: bool,
}
