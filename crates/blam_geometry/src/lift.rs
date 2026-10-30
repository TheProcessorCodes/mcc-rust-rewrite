//! Volumes that push whoever is in them: gravity lifts, man cannons, the
//! lifts in Halo 2's towers. In the game these are Havok phantoms, shapes
//! in an object's physics model that collide with nothing but accelerate
//! what overlaps them (`docs/format/halo2.md` 11 has where Halo 2 keeps
//! them). The reader turns each such shape into a [`LiftVolume`] in world
//! space; the movement code asks [`acceleration`] what it does to a body at
//! a point with a velocity.
//!
//! Halo world units throughout: positions in wu, speeds in wu/s,
//! accelerations in wu/s^2, +Z up.

use glam::Vec3;

/// The shape of a lift volume, world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiftShape {
    /// A box: its centre, its three axes (unit length) and its half sizes
    /// along them.
    Box { centre: Vec3, axes: [Vec3; 3], half: Vec3 },
    /// A capsule: the segment `a`..`b` grown by `radius` (a sphere when
    /// `a == b`).
    Capsule { a: Vec3, b: Vec3, radius: f32 },
}

impl LiftShape {
    /// How far `p` is outside the shape (0 inside).
    pub fn distance(&self, p: Vec3) -> f32 {
        match *self {
            LiftShape::Box { centre, axes, half } => {
                let d = p - centre;
                let local = Vec3::new(d.dot(axes[0]), d.dot(axes[1]), d.dot(axes[2]));
                (local.abs() - half).max(Vec3::ZERO).length()
            }
            LiftShape::Capsule { a, b, radius } => {
                let ab = b - a;
                let t = if ab.length_squared() > 1e-12 {
                    ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                ((p - (a + ab * t)).length() - radius).max(0.0)
            }
        }
    }
}

/// What a phantom does to what is inside it (Halo 2's phantom type). Each
/// pair is an acceleration and the speed it stops pushing at; zero
/// acceleration does nothing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LiftPush {
    /// Along the marker's direction.
    pub direction: (f32, f32),
    /// Towards the line through the marker along its direction.
    pub axis: (f32, f32),
    /// Towards the marker's position.
    pub center: (f32, f32),
    /// Within this distance of the marker (or its line) the centre and
    /// axis pulls stop.
    pub dead_radius: f32,
    /// Gravity is switched off inside (phantom flag bit 2).
    pub negates_gravity: bool,
    /// Vehicles are left alone (flag bit 6).
    pub ignores_vehicles: bool,
}

/// A pushing volume placed in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct LiftVolume {
    /// The object it belongs to and the shape's name, for logs
    /// (`lockout_lift_tube 0 lower_lift`).
    pub name: String,
    pub shape: LiftShape,
    /// The marker the push is aimed by: its position and the direction it
    /// points (unit length).
    pub origin: Vec3,
    pub direction: Vec3,
    pub push: LiftPush,
}

/// How quickly the axis and centre pulls settle a body on the line or the
/// point, seconds. Halo leaves the overshoot to Havok's damping; ours
/// would swing, so the pull aims for this time to arrive instead of its
/// top speed when that is closer. Our own choice.
pub const SETTLE: f32 = 0.2;

impl LiftVolume {
    /// Whether a body at `p` reaching `margin` out from it touches the volume.
    pub fn touches(&self, p: Vec3, margin: f32) -> bool {
        self.shape.distance(p) <= margin
    }

    /// The acceleration (wu/s^2) the volume gives a body at `p` moving at
    /// `v` over the next `dt` seconds, gravity not included. Each pull adds
    /// its acceleration until the speed along it reaches its top speed, and
    /// never pushes past that speed within the step.
    pub fn acceleration(&self, p: Vec3, v: Vec3, dt: f32) -> Vec3 {
        let push = &self.push;
        let dt = dt.max(1e-4);
        let mut a = Vec3::ZERO;
        // Along the direction. A top speed of zero or less is taken as no
        // limit (Dune's watchtower pod has acceleration 3 with top speed 0).
        let (accel, top) = push.direction;
        if accel != 0.0 {
            let d = self.direction * accel.signum();
            let along = v.dot(d);
            let room = if top > 0.0 { ((top - along) / dt).max(0.0) } else { f32::INFINITY };
            a += d * accel.abs().min(room);
        }
        let r = p - self.origin;
        let off_axis = r - self.direction * r.dot(self.direction);
        // The axis pull works across the direction only.
        let across = v - self.direction * v.dot(self.direction);
        a += pull(-off_axis, across, push.axis, push.dead_radius, dt);
        a += pull(-r, v, push.center, push.dead_radius, dt);
        a
    }
}

/// A pull along `to` (from the body to where it is pulled) with
/// `(acceleration, top speed)`: towards when the acceleration is positive,
/// away when negative. `v` is the body's velocity in the pull's plane.
/// Pulling in steers that whole velocity at the target, so a body that
/// came in sideways settles on the line instead of circling it.
fn pull(to: Vec3, v: Vec3, (accel, top): (f32, f32), dead: f32, dt: f32) -> Vec3 {
    let distance = to.length();
    if accel == 0.0 || distance <= dead.max(1e-4) {
        return Vec3::ZERO;
    }
    let n = to / distance * accel.signum();
    let speed = v.dot(n);
    let top = if top > 0.0 { top } else { f32::INFINITY };
    // Towards: arrive in about `SETTLE` rather than overshoot. Away: just
    // up to the top speed.
    let target = if accel > 0.0 { top.min((distance - dead) / SETTLE) } else { top };
    if accel > 0.0 {
        return ((n * target - v) / dt).clamp_length_max(accel);
    }
    n * ((target - speed) / dt).clamp(0.0, accel.abs())
}

/// The sum of every volume `p` touches (within `margin`), and whether any
/// of them switches gravity off. `None` when it touches none. `vehicle`
/// leaves out volumes that ignore vehicles.
pub fn acceleration(lifts: &[LiftVolume], p: Vec3, v: Vec3, dt: f32, margin: f32, vehicle: bool) -> Option<(Vec3, bool)> {
    let mut out: Option<(Vec3, bool)> = None;
    for l in lifts {
        if (vehicle && l.push.ignores_vehicles) || !l.touches(p, margin) {
            continue;
        }
        let (a, g) = out.get_or_insert((Vec3::ZERO, false));
        *a += l.acceleration(p, v, dt);
        *g |= l.push.negates_gravity;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lockout's lower lift region: a 1.21 x 1.21 x 4.5 wu box over the
    /// pad, straight up 10 wu/s^2 to 4 wu/s, pulled to the middle 10 to 10,
    /// gravity off.
    fn lower() -> LiftVolume {
        LiftVolume {
            name: "lower_lift".into(),
            shape: LiftShape::Box {
                centre: Vec3::new(0.0, 0.0, 2.25),
                axes: [Vec3::X, Vec3::Y, Vec3::Z],
                half: Vec3::new(0.605, 0.605, 2.25),
            },
            origin: Vec3::new(0.01, 0.0, 0.0),
            direction: Vec3::Z,
            push: LiftPush {
                direction: (10.0, 4.0),
                axis: (10.0, 10.0),
                negates_gravity: true,
                ..Default::default()
            },
        }
    }

    #[test]
    fn shapes() {
        let l = lower();
        assert!(l.touches(Vec3::new(0.5, -0.5, 0.1), 0.0));
        assert!(!l.touches(Vec3::new(0.7, 0.0, 1.0), 0.0));
        assert!(l.touches(Vec3::new(0.7, 0.0, 1.0), 0.2));
        assert!(!l.touches(Vec3::new(0.0, 0.0, 4.6), 0.0));
        let pill = LiftShape::Capsule {
            a: Vec3::ZERO,
            b: Vec3::Z * 2.0,
            radius: 0.5,
        };
        assert_eq!(pill.distance(Vec3::new(0.3, 0.0, 1.0)), 0.0);
        assert!((pill.distance(Vec3::new(0.0, 0.0, 3.0)) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn pushes_up_to_its_top_speed_and_no_further() {
        let l = lower();
        let a = l.acceleration(Vec3::new(0.01, 0.0, 0.5), Vec3::ZERO, 1.0 / 60.0);
        assert!((a - Vec3::Z * 10.0).length() < 1e-4, "{a}");
        // Nearly at the top speed: only what is left.
        let a = l.acceleration(Vec3::new(0.01, 0.0, 0.5), Vec3::Z * 3.95, 0.01);
        assert!((a.z - 5.0).abs() < 1e-3, "{a}");
        assert_eq!(l.acceleration(Vec3::new(0.01, 0.0, 0.5), Vec3::Z * 5.0, 0.01), Vec3::ZERO);
    }

    #[test]
    fn rides_up_and_settles_on_the_axis() {
        // A body started off the axis, gravity off inside, as the movement
        // code integrates it.
        let lifts = [lower()];
        let (mut p, mut v) = (Vec3::new(0.4, -0.3, 0.05), Vec3::new(0.5, 0.0, 0.0));
        let dt = 1.0 / 120.0;
        let mut t = 0.0;
        while let Some((a, no_gravity)) = acceleration(&lifts, p, v, dt, 0.0, false) {
            assert!(no_gravity);
            v += a * dt;
            p += v * dt;
            t += dt;
            assert!(t < 3.0, "stuck at {p}");
        }
        assert!((v.z - 4.0).abs() < 1e-3, "left at {v}");
        let off_axis = (p - Vec3::new(0.01, 0.0, p.z)).length();
        assert!(off_axis < 0.05, "left at {p}");
        // 0.4 s to reach 4 wu/s (0.8 wu), then the last 3.7 wu at 4 wu/s.
        assert!((t - 1.33).abs() < 0.05, "{t}");
    }

    #[test]
    fn vehicles_skip_volumes_that_ignore_them() {
        let mut l = lower();
        l.push.ignores_vehicles = true;
        let lifts = [l];
        let p = Vec3::new(0.0, 0.0, 1.0);
        assert!(acceleration(&lifts, p, Vec3::ZERO, 0.01, 0.0, false).is_some());
        assert!(acceleration(&lifts, p, Vec3::ZERO, 0.01, 0.0, true).is_none());
    }
}
