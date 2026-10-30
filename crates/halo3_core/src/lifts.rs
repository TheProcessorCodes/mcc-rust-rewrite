//! Gravity lifts and other pushing volumes (`blam_geometry::lift`) in the
//! renderer's frame, for the movement code: walking and the dirt bike ask
//! [`Lifts::push`] each step what the map's lifts do to them. The Skate 3
//! engine runs its own physics on a worker thread and takes no outside
//! push, so the board rides over a lift pad.

use bevy::prelude::*;

use blam_geometry::{LiftVolume, WORLD_UNIT_METRES};

use crate::coords::{to_halo, to_render_dir};

/// The map's pushing volumes, Halo world units.
#[derive(Resource, Clone, Default)]
pub struct Lifts(pub Vec<LiftVolume>);

/// What the lifts do to a body this step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Push {
    /// Renderer frame, m/s^2, gravity not included.
    pub acceleration: Vec3,
    /// Gravity is off while inside.
    pub negates_gravity: bool,
}

impl Lifts {
    /// The push on a body centred at `position` moving at `velocity`
    /// (renderer metres, m/s) over the next `dt` seconds, touching volumes
    /// within `margin` metres. `vehicle` skips volumes that ignore vehicles.
    pub fn push(&self, position: Vec3, velocity: Vec3, dt: f32, margin: f32, vehicle: bool) -> Option<Push> {
        if self.0.is_empty() {
            return None;
        }
        let (a, negates_gravity) = blam_geometry::lift::acceleration(
            &self.0,
            to_halo(position),
            to_halo(velocity),
            dt,
            margin / WORLD_UNIT_METRES,
            vehicle,
        )?;
        Some(Push {
            acceleration: to_render_dir(a) * WORLD_UNIT_METRES,
            negates_gravity,
        })
    }
}
