//! How a crate scripts let go moves: the Warthog run's tunnel girders.
//!
//! What the map says (`docs/format/halo-script.md` 8): each girder's
//! physics model (`phmo`) has a small anchor body in the ceiling (node 0)
//! tied to the girder (node 1, `hinge`) by one constraint at the girder's
//! end, with twist, cone and plane limits. The girder's `fall` damage
//! section *loosens* that constraint and `destroy` *destroys* it; the
//! scripts call `fall` on the girders (`fall_off`), and `fall` then
//! `destroy` on one (`tunnel_fall_away 100_hw_ceiling10`). Nothing in the
//! scripts pushes or spins them.
//!
//! So a loosened girder swings down about its hinge ([`Swing`]) until the
//! constraint's limits or the road stop it, bounces off and comes to rest
//! hanging there; a girder with no constraint left falls free and tumbles
//! ([`Body`]), bouncing and sliding on the floor until it sleeps. Friction
//! and restitution are the physics model's (0.8 and 0.35 for the girder).
//! Everything else here (the corners standing in for the shape, the
//! damping, the sleep thresholds, the small push a freed crate gets) is our
//! own, not Havok's.

use glam::{Mat3, Mat4, Quat, Vec3};

use crate::objects::GRAVITY;

/// Sub-steps per 30 Hz tick.
pub const SUBSTEPS: usize = 4;
/// A body slower than this (wu/s, rad/s) for [`SLEEP_AFTER`] seconds while
/// touching something goes to sleep.
const SLEEP_SPEED: f32 = 0.25;
const SLEEP_AFTER: f32 = 0.5;
/// Below this closing speed (wu/s) a contact does not bounce, so a body at
/// rest does not jitter.
const BOUNCE_ABOVE: f32 = 1.0;
/// Angular damping, per second (our choice: a little, so a spin dies out).
const SPIN_DAMPING: f32 = 0.3;
/// A swing slower than this (rad/s) while held by a limit or the floor for
/// [`SLEEP_AFTER`] seconds comes to rest.
const SWING_REST: f32 = 0.05;

/// What a moving crate needs from the world: per corner of its collision
/// at `pose`, how far above the floor it is (negative: sunk in by that
/// much) and the floor's normal there; the floor under each corner is
/// looked for below where it was at `free` (a pose known to be clear) or
/// is now, whichever is higher. Corners with no floor under them are left
/// out.
pub trait Floor {
    fn gaps(&self, pose: Mat4, free: Mat4) -> Vec<(Vec3, f32, Vec3)>;
}

/// The corners' inertia about `centre` per unit mass, treating the mass as
/// spread evenly over them.
fn inertia(corners: &[Vec3], centre: Vec3) -> Mat3 {
    let mut i = Mat3::ZERO;
    for c in corners {
        let r = *c - centre;
        i += Mat3::from_diagonal(Vec3::splat(r.length_squared())) - Mat3::from_cols(r * r.x, r * r.y, r * r.z);
    }
    i * (1.0 / corners.len().max(1) as f32)
}

/// The middle of the corners' box.
pub fn centre(corners: &[Vec3]) -> Vec3 {
    let (lo, hi) = corners.iter().fold((Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY)), |(lo, hi), p| {
        (lo.min(*p), hi.max(*p))
    });
    (lo + hi) * 0.5
}

/// A crate swinging down about the pivot of a loosened constraint.
#[derive(Clone, Debug)]
pub struct Swing {
    /// Where it was when it let go.
    pub from: Mat4,
    pub pivot: Vec3,
    /// Horizontal, signed so a positive angle lowers its middle.
    pub axis: Vec3,
    pub angle: f32,
    pub rate: f32,
    /// The constraint's limit on the angle (radians, from its rest).
    pub limit: f32,
    /// Inertia about the axis through the pivot, per unit mass.
    k: f32,
    /// Its middle at `from`.
    middle: Vec3,
    restitution: f32,
    slow: f32,
    /// Come to rest: hangs where it is and moves no more.
    pub resting: bool,
}

impl Swing {
    /// `corners`: its collision's corners at `from`, world frame.
    /// `twist_axis`, `plane_axis`: the constraint's axes (world, at
    /// `from`); `cone`, `plane`: its limits.
    pub fn new(from: Mat4, corners: &[Vec3], pivot: Vec3, twist_axis: Vec3, plane_axis: Vec3, cone: f32, plane: (f32, f32), restitution: f32) -> Swing {
        let middle = centre(corners);
        let r = middle - pivot;
        let axis = Vec3::Z.cross(r).normalize_or(Vec3::Y);
        // Which way gravity turns it.
        let torque = axis.dot(r.cross(Vec3::NEG_Z));
        let axis = if torque < 0.0 { -axis } else { axis };
        let k = corners.iter().map(|c| (*c - pivot).cross(axis).length_squared()).sum::<f32>() / corners.len().max(1) as f32;
        // The furthest it can turn before the twist axis leaves the cone or
        // the plane band.
        let allowed = |a: f32| {
            let t = Quat::from_axis_angle(axis, a) * twist_axis;
            t.angle_between(twist_axis) <= cone + 1e-4 && {
                let off = t.dot(plane_axis).clamp(-1.0, 1.0).asin();
                off >= plane.0 - 1e-4 && off <= plane.1 + 1e-4
            }
        };
        let step = 0.25f32.to_radians();
        let mut limit = 0.0;
        while limit + step <= std::f32::consts::PI && allowed(limit + step) {
            limit += step;
        }
        Swing {
            from,
            pivot,
            axis,
            angle: 0.0,
            rate: 0.0,
            limit,
            k: k.max(1e-3),
            middle,
            restitution,
            slow: 0.0,
            resting: false,
        }
    }

    pub fn pose_at(&self, angle: f32) -> Mat4 {
        Mat4::from_translation(self.pivot) * Mat4::from_axis_angle(self.axis, angle) * Mat4::from_translation(-self.pivot) * self.from
    }

    pub fn pose(&self) -> Mat4 {
        self.pose_at(self.angle)
    }

    /// Velocity of a point of it, world frame.
    pub fn velocity_at(&self, p: Vec3) -> Vec3 {
        (self.axis * self.rate).cross(p - self.pivot)
    }

    /// The angular velocity, world frame.
    pub fn spin(&self) -> Vec3 {
        self.axis * self.rate
    }

    /// One tick.
    pub fn step(&mut self, dt: f32, floor: &dyn Floor) {
        if self.resting {
            return;
        }
        let h = dt / SUBSTEPS as f32;
        let mut held = false;
        for _ in 0..SUBSTEPS {
            let r = Quat::from_axis_angle(self.axis, self.angle) * (self.middle - self.pivot);
            let torque = self.axis.dot(r.cross(Vec3::new(0.0, 0.0, -GRAVITY)));
            self.rate += torque / self.k * h;
            let mut next = self.angle + self.rate * h;
            if next > self.limit || next < -self.limit {
                next = next.clamp(-self.limit, self.limit);
                self.rate = -self.rate * self.restitution;
                held = true;
            }
            let sunk = floor.gaps(self.pose_at(next), self.pose()).iter().any(|(_, g, _)| *g < -0.01);
            if sunk {
                // It hits the floor: back off it.
                self.rate = -self.rate * self.restitution;
                held = true;
                continue;
            }
            self.angle = next;
        }
        if held && self.rate.abs() < SWING_REST {
            self.slow += dt;
            if self.slow >= SLEEP_AFTER {
                self.rate = 0.0;
                self.resting = true;
            }
        } else {
            self.slow = 0.0;
        }
    }
}

/// A crate falling free: a rigid body with its collision's corners as
/// contact points.
#[derive(Clone, Debug)]
pub struct Body {
    /// Where it was when it let go, and its middle then.
    pub from: Mat4,
    origin: Vec3,
    pub position: Vec3,
    pub velocity: Vec3,
    pub rotation: Quat,
    pub spin: Vec3,
    /// Inverse inertia per unit mass, world axes, at `from`.
    inv_inertia: Mat3,
    friction: f32,
    restitution: f32,
    slow: f32,
    pub asleep: bool,
    /// Seconds since it let go.
    pub t: f32,
    /// Whether it has touched anything yet.
    pub touched: bool,
}

impl Body {
    pub fn new(from: Mat4, corners: &[Vec3], velocity: Vec3, spin: Vec3, friction: f32, restitution: f32) -> Body {
        let origin = centre(corners);
        let i = inertia(corners, origin);
        let inv = if i.determinant().abs() > 1e-6 { i.inverse() } else { Mat3::IDENTITY };
        Body {
            from,
            origin,
            position: origin,
            velocity,
            rotation: Quat::IDENTITY,
            spin,
            inv_inertia: inv,
            friction,
            restitution,
            slow: 0.0,
            asleep: false,
            t: 0.0,
            touched: false,
        }
    }

    fn pose_of(&self, position: Vec3, rotation: Quat) -> Mat4 {
        Mat4::from_translation(position) * Mat4::from_quat(rotation) * Mat4::from_translation(-self.origin) * self.from
    }

    pub fn pose(&self) -> Mat4 {
        self.pose_of(self.position, self.rotation)
    }

    fn inv_inertia_world(&self) -> Mat3 {
        let r = Mat3::from_quat(self.rotation);
        r * self.inv_inertia * r.transpose()
    }

    /// One tick. Returns whether it touched anything.
    pub fn step(&mut self, dt: f32, floor: &dyn Floor) -> bool {
        let mut touched = false;
        if self.asleep {
            // Still lying on something?
            let gaps = floor.gaps(self.pose(), self.pose());
            if gaps.iter().all(|(_, g, _)| *g > 0.05) {
                self.asleep = false;
                self.slow = 0.0;
            } else {
                return true;
            }
        }
        self.t += dt;
        let h = dt / SUBSTEPS as f32;
        for _ in 0..SUBSTEPS {
            let before = self.pose();
            self.velocity.z -= GRAVITY * h;
            self.position += self.velocity * h;
            self.rotation = (Quat::from_scaled_axis(self.spin * h) * self.rotation).normalize();
            self.spin *= 1.0 - SPIN_DAMPING * h;
            let contacts: Vec<(Vec3, f32, Vec3)> =
                floor.gaps(self.pose(), before).into_iter().filter(|(_, g, _)| *g < 0.0).collect();
            if contacts.is_empty() {
                continue;
            }
            touched = true;
            // Out of the floor along the deepest contact's normal.
            let (_, deepest, n) = contacts.iter().copied().fold((Vec3::ZERO, 0.0, Vec3::Z), |a, c| if c.1 < a.1 { c } else { a });
            self.position += n * (-deepest);
            let inv = self.inv_inertia_world();
            for _ in 0..2 {
                for (corner, _, n) in &contacts {
                    let c = *corner + n * (-deepest);
                    let r = c - self.position;
                    let u = self.velocity + self.spin.cross(r);
                    let vn = u.dot(*n);
                    if vn >= 0.0 {
                        continue;
                    }
                    let e = if vn < -BOUNCE_ABOVE { self.restitution } else { 0.0 };
                    let kn = 1.0 + n.dot((inv * r.cross(*n)).cross(r));
                    let jn = -(1.0 + e) * vn / kn;
                    self.velocity += *n * jn;
                    self.spin += inv * r.cross(*n * jn);
                    // Friction, up to its share of the push.
                    let u = self.velocity + self.spin.cross(r);
                    let vt = u - *n * u.dot(*n);
                    let speed = vt.length();
                    if speed > 1e-4 {
                        let t = vt / speed;
                        let kt = 1.0 + t.dot((inv * r.cross(t)).cross(r));
                        let jt = (speed / kt).min(self.friction * jn);
                        self.velocity -= t * jt;
                        self.spin -= inv * r.cross(t * jt);
                    }
                }
            }
        }
        self.touched |= touched;
        if touched && self.velocity.length() < SLEEP_SPEED && self.spin.length() < SLEEP_SPEED {
            self.slow += dt;
            if self.slow >= SLEEP_AFTER {
                self.velocity = Vec3::ZERO;
                self.spin = Vec3::ZERO;
                self.asleep = true;
            }
        } else {
            self.slow = 0.0;
        }
        touched
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat floor at z = 0 everywhere.
    struct Flat;
    impl Floor for Flat {
        fn gaps(&self, pose: Mat4, _free: Mat4) -> Vec<(Vec3, f32, Vec3)> {
            box_corners().iter().map(|c| pose.transform_point3(*c)).map(|p| (p, p.z, Vec3::Z)).collect()
        }
    }

    fn box_corners() -> Vec<Vec3> {
        let mut v = Vec::new();
        for x in [-5.0, 5.0] {
            for y in [-0.3, 0.3] {
                for z in [-0.5, 0.5] {
                    v.push(Vec3::new(x, y, z));
                }
            }
        }
        v
    }

    #[test]
    fn a_dropped_beam_bounces_and_sleeps_on_the_floor() {
        let from = Mat4::from_translation(Vec3::new(0.0, 0.0, 10.0));
        let corners: Vec<Vec3> = box_corners().iter().map(|c| from.transform_point3(*c)).collect();
        let mut b = Body::new(from, &corners, Vec3::new(0.5, 0.0, 0.0), Vec3::new(0.0, 0.6, 0.2), 0.8, 0.35);
        let mut peak_after_touch: f32 = 0.0;
        for _ in 0..30 * 15 {
            b.step(1.0 / 30.0, &Flat);
            if b.touched {
                peak_after_touch = peak_after_touch.max(b.velocity.z);
            }
            if b.asleep {
                break;
            }
        }
        assert!(b.asleep, "never slept");
        assert!(peak_after_touch > 0.1, "never bounced");
        let low = Flat.gaps(b.pose(), b.pose()).iter().map(|g| g.1).fold(f32::INFINITY, f32::min);
        assert!(low.abs() < 0.05, "rests {low} off the floor");
        assert!(b.rotation.angle_between(Quat::IDENTITY) > 0.01, "never turned");
    }

    #[test]
    fn a_loosened_beam_swings_to_its_limit_and_rests() {
        let from = Mat4::from_translation(Vec3::new(5.0, 0.0, 50.0));
        let corners: Vec<Vec3> = box_corners().iter().map(|c| from.transform_point3(*c)).collect();
        let pivot = Vec3::new(0.0, 0.0, 50.5);
        let mut s = Swing::new(from, &corners, pivot, Vec3::NEG_Z, Vec3::X, 45f32.to_radians(), (-30f32.to_radians(), 30f32.to_radians()), 0.35);
        assert!((s.limit.to_degrees() - 30.0).abs() < 0.5, "limit {}", s.limit.to_degrees());
        for _ in 0..30 * 20 {
            s.step(1.0 / 30.0, &Flat);
        }
        assert!(s.resting);
        assert!((s.angle - s.limit).abs() < 1e-3, "rests at {} of {}", s.angle, s.limit);
        // Its far end went down.
        assert!(s.pose().transform_point3(Vec3::new(5.0, 0.0, 0.0)).z < 50.0 - 4.0);
    }
}
