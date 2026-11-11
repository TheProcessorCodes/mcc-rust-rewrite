//! Gravity lifts and other pushing volumes (`blam_geometry::lift`) in the
//! renderer's frame, for the movement code: walking and the dirt bike ask
//! [`Lifts::push`] each step what the map's lifts do to them. The Skate 3
//! engine runs its own physics on a worker thread and follows a trajectory
//! it predicts once the board is in the air, so a push spread over many
//! steps is lost; the board takes the lift's push at once instead
//! ([`Lifts::kick`], `mcc_skate`).
//!
//! A launch pad (Valhalla's man cannons, `LiftVolume::launch_speed`) throws
//! every mode at once ([`Lifts::launch`]), at a speed scaled for the mode's
//! gravity so the flight is as long as Halo's.

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

/// The gravity a body thrown by a Halo 3 man cannon falls under, m/s^2:
/// Halo 3's 4.1712 wu/s^2, with no air drag. A launch is scaled for the
/// gravity of whoever takes it ([`launch_scale`]), so each mode's flight
/// covers the distance Halo's does.
pub const HALO_GRAVITY: f32 = 4.1712 * WORLD_UNIT_METRES;

/// What a launch speed is multiplied by for a body that falls at `gravity`
/// (m/s^2) to fly as far as Halo's player does: a throw's range is
/// `v^2 sin(2a) / g`, so the speed goes with the root of the gravity. The
/// flight takes as much longer or shorter.
pub fn launch_scale(gravity: f32) -> f32 {
    if gravity > 0.0 { (gravity / HALO_GRAVITY).sqrt() } else { 1.0 }
}

impl Lifts {
    /// The push on a body centred at `position` moving at `velocity`
    /// (renderer metres, m/s) over the next `dt` seconds, touching volumes
    /// within `margin` metres. `vehicle` skips volumes that ignore vehicles.
    /// Launch pads are not in it ([`Lifts::launch`]).
    pub fn push(&self, position: Vec3, velocity: Vec3, dt: f32, margin: f32, vehicle: bool) -> Option<Push> {
        let (p, v, margin) = (to_halo(position), to_halo(velocity), margin / WORLD_UNIT_METRES);
        let mut out: Option<(Vec3, bool)> = None;
        for l in &self.0 {
            if (vehicle && l.push.ignores_vehicles) || l.launch_speed().is_some() || !l.touches(p, margin) {
                continue;
            }
            let (a, g) = out.get_or_insert((Vec3::ZERO, false));
            *a += l.acceleration(p, v, dt);
            *g |= l.push.negates_gravity;
        }
        let (a, negates_gravity) = out?;
        Some(Push {
            acceleration: to_render_dir(a) * WORLD_UNIT_METRES,
            negates_gravity,
        })
    }

    /// The velocity change (renderer m/s) the launch pads
    /// (`LiftVolume::launch_speed`: Valhalla's man cannons) give a body
    /// centred at `position` moving at `velocity`, touching them within
    /// `margin` metres: up to the pad's top speed along its direction, all
    /// at once, scaled for a body that falls at `gravity` m/s^2
    /// ([`launch_scale`]). The player's rides take it as a player does:
    /// the pads' own vehicle volumes (a weaker push for a Mongoose) are
    /// not read. `None` when it touches none or is already as fast.
    pub fn launch(&self, position: Vec3, velocity: Vec3, margin: f32, gravity: f32) -> Option<Vec3> {
        self.thrown(position, velocity, margin, gravity, true)
    }

    /// What the lifts do to a body on foot that falls at `gravity` m/s^2
    /// over the next `dt` seconds: a launch pad's throw ([`Lifts::launch`])
    /// as the acceleration that gives it within the step, else the lifts'
    /// push ([`Lifts::push`]).
    pub fn walk(&self, position: Vec3, velocity: Vec3, dt: f32, margin: f32, gravity: f32) -> Option<Push> {
        if let Some(throw) = self.launch(position, velocity, margin, gravity) {
            return Some(Push {
                acceleration: throw / dt.max(1e-4),
                negates_gravity: false,
            });
        }
        self.push(position, velocity, dt, margin, false)
    }

    /// What the lifts do to the velocity of a ride that integrates its own
    /// physics (the dirt bike) over a step it has already made: the launch
    /// pads' throw ([`Lifts::launch`]), else the lifts' push as a vehicle
    /// ([`Lifts::push`]) with the ride's `gravity` given back while a lift
    /// switches gravity off. `None` when it touches nothing.
    pub fn ride(&self, position: Vec3, velocity: Vec3, dt: f32, margin: f32, gravity: f32) -> Option<Vec3> {
        if let Some(throw) = self.launch(position, velocity, margin, gravity) {
            return Some(throw);
        }
        let p = self.push(position, velocity, dt, margin, true)?;
        let mut a = p.acceleration;
        if p.negates_gravity {
            a.y += gravity;
        }
        Some(a * dt)
    }

    /// The velocity change (renderer m/s) that brings a body centred at
    /// `position` moving at `velocity` to the top speed of every volume it
    /// touches within `margin` metres, along that volume's direction, all
    /// at once: a launch pad's throw (scaled for `gravity`, as
    /// [`Lifts::launch`]), and for any other lift what it would do over
    /// its first fraction of a second. Volumes with no top speed give a
    /// tenth of a second of their acceleration. The pulls towards the
    /// lift's line and middle are left out. Vehicles' volumes are skipped
    /// as for a player. `None` when it touches none or is already as fast.
    pub fn kick(&self, position: Vec3, velocity: Vec3, margin: f32, gravity: f32) -> Option<Vec3> {
        self.thrown(position, velocity, margin, gravity, false)
    }

    fn thrown(&self, position: Vec3, velocity: Vec3, margin: f32, gravity: f32, pads_only: bool) -> Option<Vec3> {
        let p = to_halo(position);
        let mut v = to_halo_dir(velocity) / WORLD_UNIT_METRES;
        let mut any = false;
        for l in &self.0 {
            let (accel, top) = l.push.direction;
            let pad = l.launch_speed();
            if accel == 0.0 || (pads_only && pad.is_none()) || !l.touches(p, margin / WORLD_UNIT_METRES) {
                continue;
            }
            let d = l.direction * accel.signum();
            let gain = match pad {
                Some(speed) => (speed * launch_scale(gravity) - v.dot(d)).max(0.0),
                None if top > 0.0 => (top - v.dot(d)).max(0.0),
                None => accel.abs() * 0.1,
            };
            if gain > 0.0 {
                v += d * gain;
                any = true;
            }
        }
        any.then(|| to_render_dir(v) * WORLD_UNIT_METRES - velocity)
    }
}

/// A ride thrown by a launch pad, held to the flight Halo gives a thrown
/// player: no air drag. The dirt bike's own physics (ours and Reflex's)
/// slow it in the air, by a quarter of the distance at a cannon's speed;
/// while it flies from a pad its velocity is put back each step to what
/// the throw and gravity alone make it. The flight ends when a wheel
/// touches down, or when something else changes the ride's velocity (it
/// hit a wall).
#[derive(Clone, Copy, Debug, Default)]
pub struct Flight {
    /// The velocity the thrown ride should have now, and seconds since
    /// the throw.
    thrown: Option<(Vec3, f32)>,
}

/// Seconds after a throw before wheels on the ground end the flight: the
/// ride is still on the pad when it is thrown.
const FLIGHT_LEAVES: f32 = 0.3;
/// The flight is over when the ride's velocity is this far (m/s) from the
/// flight's after a step: more than drag takes.
const FLIGHT_BROKEN: f32 = 2.0;

impl Flight {
    /// What the lifts do to the ride's velocity over a step it has already
    /// made (as [`Lifts::ride`]), and the drag given back while it flies
    /// from a launch pad. `airborne`: no wheel is on the ground.
    #[allow(clippy::too_many_arguments)]
    pub fn ride(&mut self, lifts: &Lifts, position: Vec3, velocity: Vec3, airborne: bool, dt: f32, margin: f32, gravity: f32) -> Option<Vec3> {
        if let Some(throw) = lifts.launch(position, velocity, margin, gravity) {
            // Along the pad's direction only: what the ground under the
            // wheels did to the ride on the pad is not part of the flight.
            let along = throw.normalize_or_zero();
            let thrown = along * (velocity + throw).dot(along);
            self.thrown = Some((thrown, 0.0));
            return Some(thrown - velocity);
        }
        if let Some((v, age)) = &mut self.thrown {
            *age += dt;
            v.y -= gravity * dt;
            // The ride's own step took gravity too; what is left between
            // the two is drag, or a hit.
            if *age < FLIGHT_LEAVES || (airborne && v.distance(velocity) < FLIGHT_BROKEN) {
                return Some(*v - velocity);
            }
            self.thrown = None;
        }
        lifts.ride(position, velocity, dt, margin, gravity)
    }

    /// Whether the ride is flying from a launch pad.
    pub fn flying(&self) -> bool {
        self.thrown.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blam_geometry::{LiftPush, LiftShape, LiftVolume};

    /// A long cannon round the origin throwing along +X and up: 3.4 wu of
    /// volume along its push, more than the 1 wu its top speed takes.
    fn cannon() -> Lifts {
        Lifts(vec![LiftVolume {
            name: "cannon".into(),
            shape: LiftShape::Box {
                centre: Vec3::ZERO,
                axes: [Vec3::X, Vec3::Y, Vec3::Z],
                half: Vec3::splat(1.2),
            },
            origin: Vec3::ZERO,
            direction: Vec3::new(1.0, 0.0, 1.0).normalize(),
            push: LiftPush {
                direction: (60.0, 11.0),
                ..Default::default()
            },
        }])
    }

    /// Valhalla's pad: the same push in a box 0.67 wu long.
    fn pad() -> Lifts {
        let mut l = cannon();
        l.0[0].shape = LiftShape::Box {
            centre: Vec3::ZERO,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half: Vec3::new(0.57, 0.35, 0.14),
        };
        l.0[0].direction = Vec3::new(0.0, 0.6427876, 0.7660445);
        l.0[0].push.ignores_vehicles = true;
        l
    }

    /// The flight of a point thrown at `v` under `gravity`, back to the
    /// height it left at: how far along the ground, metres.
    fn range(v: Vec3, gravity: f32) -> f32 {
        Vec3::new(v.x, 0.0, v.z).length() * 2.0 * v.y / gravity
    }

    #[test]
    fn a_kick_brings_the_body_to_the_top_speed_at_once() {
        let lifts = cannon();
        let k = lifts.kick(Vec3::ZERO, Vec3::ZERO, 0.0, 9.81).expect("in the cannon");
        let along = to_render_dir(Vec3::new(1.0, 0.0, 1.0).normalize());
        assert!((k.dot(along) - 11.0 * WORLD_UNIT_METRES).abs() < 1e-3, "{k}");
        // Already that fast: nothing more.
        assert!(lifts.kick(Vec3::ZERO, k, 0.0, 9.81).is_none());
        // Outside it: nothing.
        assert!(lifts.kick(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, 0.0, 9.81).is_none());
    }

    #[test]
    fn a_long_cannon_is_no_launch_pad() {
        let lifts = cannon();
        assert_eq!(lifts.0[0].launch_speed(), None);
        assert!(lifts.launch(Vec3::ZERO, Vec3::ZERO, 0.0, HALO_GRAVITY).is_none());
        // It pushes step by step instead.
        let p = lifts.push(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0, 0.0, false).expect("inside");
        assert!((p.acceleration.length() - 60.0 * WORLD_UNIT_METRES).abs() < 1e-2, "{p:?}");
    }

    #[test]
    fn a_launch_pad_throws_at_its_top_speed_at_once() {
        let lifts = pad();
        assert_eq!(lifts.0[0].launch_speed(), Some(11.0));
        // Not pushed step by step: a body crossing the box would leave slow.
        assert!(lifts.push(Vec3::ZERO, Vec3::ZERO, 1.0 / 60.0, 0.0, false).is_none());
        let v = lifts.launch(Vec3::ZERO, Vec3::ZERO, 0.0, HALO_GRAVITY).expect("on the pad");
        assert!((v.length() - 11.0 * WORLD_UNIT_METRES).abs() < 1e-3, "{v}");
        // Halo's flight from it: 11 wu/s at 50 degrees under 4.1712 wu/s^2
        // comes down 28.6 wu (87 m) out.
        let far = range(v, HALO_GRAVITY);
        assert!((far / WORLD_UNIT_METRES - 28.57).abs() < 0.05, "{far}");
        // Already that fast: nothing more. Off the pad: nothing.
        assert!(lifts.launch(Vec3::ZERO, v, 0.0, HALO_GRAVITY).is_none());
        assert!(lifts.launch(Vec3::new(0.0, 10.0, 0.0), Vec3::ZERO, 0.0, HALO_GRAVITY).is_none());
    }

    #[test]
    fn a_launch_flies_as_far_under_any_gravity() {
        let lifts = pad();
        let halo = range(lifts.launch(Vec3::ZERO, Vec3::ZERO, 0.0, HALO_GRAVITY).unwrap(), HALO_GRAVITY);
        // On foot (4.5 wu/s^2), the bike (9.81 m/s^2) and the board (9.81
        // skate m/s^2 in a world shrunk by 0.84).
        for g in [4.5 * WORLD_UNIT_METRES, 9.81, 9.81 / 0.8382] {
            let v = lifts.launch(Vec3::ZERO, Vec3::ZERO, 0.0, g).unwrap();
            assert!((range(v, g) - halo).abs() < 0.05, "{g}: {} against {halo}", range(v, g));
            assert_eq!(lifts.kick(Vec3::ZERO, Vec3::ZERO, 0.0, g), Some(v));
        }
    }

    #[test]
    fn a_ride_is_thrown_by_a_pad_that_ignores_vehicles() {
        let lifts = pad();
        let dt = 1.0 / 60.0;
        let v = lifts.ride(Vec3::ZERO, Vec3::ZERO, dt, 0.0, 9.81).expect("thrown");
        assert_eq!(Some(v), lifts.launch(Vec3::ZERO, Vec3::ZERO, 0.0, 9.81));
        // A lift that switches gravity off gives the ride its gravity back.
        let mut lift = cannon();
        lift.0[0].direction = Vec3::Z;
        lift.0[0].push.direction = (10.0, 4.0);
        lift.0[0].push.negates_gravity = true;
        let v = lift.ride(Vec3::ZERO, Vec3::ZERO, dt, 0.0, 9.81).expect("pushed");
        assert!((v.y - (10.0 * WORLD_UNIT_METRES + 9.81) * dt).abs() < 1e-4, "{v}");
    }

    /// A ride with air drag thrown from the pad: the flight keeps it on
    /// the drag-free path, and lets go when it lands or hits something.
    #[test]
    fn a_thrown_ride_keeps_its_speed_against_drag() {
        let lifts = pad();
        let (g, dt) = (9.81, 1.0 / 60.0);
        let mut flight = Flight::default();
        let (mut p, mut v) = (Vec3::ZERO, Vec3::ZERO);
        let mut far = 0.0;
        for k in 0..600 {
            // The ride's own step: gravity (off the ground) and drag.
            let airborne = p.y > 0.0 || v.y > 0.0;
            if airborne {
                v.y -= g * dt;
                v -= v * v.length() * 0.004 * dt;
                p += v * dt;
            }
            if let Some(dv) = flight.ride(&lifts, p, v, airborne, dt, 0.0, g) {
                v += dv;
            }
            if k > 30 && p.y <= 0.0 {
                far = Vec3::new(p.x, 0.0, p.z).length();
                break;
            }
        }
        let want = 28.57 * WORLD_UNIT_METRES;
        assert!((far - want).abs() < 1.5, "{far} m against {want}");
        // Down: the next step lets go of it.
        assert!(flight.flying());
        flight.ride(&lifts, p, Vec3::ZERO, false, dt, 0.0, g);
        assert!(!flight.flying());
        // A hit in the air (the velocity gone) ends it too.
        let mut v = flight.ride(&lifts, Vec3::ZERO, Vec3::ZERO, false, dt, 0.0, g).expect("thrown");
        for _ in 0..30 {
            v.y -= g * dt;
            assert_eq!(flight.ride(&lifts, Vec3::Y * 20.0, v, true, dt, 0.0, g), Some(Vec3::ZERO));
        }
        assert!(flight.ride(&lifts, Vec3::Y * 20.0, Vec3::ZERO, true, dt, 0.0, g).is_none());
        assert!(!flight.flying());
    }
}
