//! Gravity lifts and other pushing volumes (`blam_geometry::lift`) in the
//! renderer's frame, for the movement code: walking and the dirt bike ask
//! [`Lifts::push`] each step what the map's lifts do to them. The Skate 3
//! engine runs its own physics on a worker thread and follows a trajectory
//! it predicts once the board is in the air, so a push spread over many
//! steps is lost; the board takes the lift's push at once instead
//! ([`Lifts::kick`], `mcc_skate`).

use bevy::prelude::*;

use blam_geometry::{LiftVolume, WORLD_UNIT_METRES};

use crate::coords::{to_halo, to_halo_dir, to_render_dir};

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

    /// The velocity change (renderer m/s) that brings a body centred at
    /// `position` moving at `velocity` to the top speed of every volume it
    /// touches within `margin` metres, along that volume's direction, all
    /// at once: what the lift would do over its first fraction of a second
    /// (Valhalla's cannon: 60 wu/s^2 up to 11 wu/s, 0.18 s). Volumes with
    /// no top speed give a tenth of a second of their acceleration. The
    /// pulls towards the lift's line and middle are left out. Vehicles'
    /// volumes are skipped as for a player. `None` when it touches none or
    /// is already as fast.
    pub fn kick(&self, position: Vec3, velocity: Vec3, margin: f32) -> Option<Vec3> {
        let p = to_halo(position);
        let mut v = to_halo_dir(velocity) / WORLD_UNIT_METRES;
        let mut any = false;
        for l in &self.0 {
            let (accel, top) = l.push.direction;
            if accel == 0.0 || !l.touches(p, margin / WORLD_UNIT_METRES) {
                continue;
            }
            let d = l.direction * accel.signum();
            let gain = if top > 0.0 { (top - v.dot(d)).max(0.0) } else { accel.abs() * 0.1 };
            if gain > 0.0 {
                v += d * gain;
                any = true;
            }
        }
        any.then(|| to_render_dir(v) * WORLD_UNIT_METRES - velocity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blam_geometry::{LiftPush, LiftShape, LiftVolume};

    /// A cannon pad round the origin throwing along +X and up.
    fn cannon() -> Lifts {
        Lifts(vec![LiftVolume {
            name: "cannon".into(),
            shape: LiftShape::Box {
                centre: Vec3::ZERO,
                axes: [Vec3::X, Vec3::Y, Vec3::Z],
                half: Vec3::splat(0.5),
            },
            origin: Vec3::ZERO,
            direction: Vec3::new(1.0, 0.0, 1.0).normalize(),
            push: LiftPush {
                direction: (60.0, 11.0),
                ..Default::default()
            },
        }])
    }

    #[test]
    fn a_kick_brings_the_body_to_the_top_speed_at_once() {
        let lifts = cannon();
        let k = lifts.kick(Vec3::ZERO, Vec3::ZERO, 0.0).expect("on the pad");
        let along = to_render_dir(Vec3::new(1.0, 0.0, 1.0).normalize());
        assert!((k.dot(along) - 11.0 * WORLD_UNIT_METRES).abs() < 1e-3, "{k}");
        // Already that fast: nothing more.
        assert!(lifts.kick(Vec3::ZERO, k, 0.0).is_none());
        // Off the pad: nothing.
        assert!(lifts.kick(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, 0.0).is_none());
    }
}
