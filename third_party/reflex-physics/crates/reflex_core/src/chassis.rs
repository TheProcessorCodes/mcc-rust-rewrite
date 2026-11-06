//! Chassis collision (`00435b00` detection, `00438180` position correction, `004360b0` impulses) and the
//! per-substep crash flags (slot 4 `0043b4a0`, slot 5 `0043b5e0`). Decoded from the asm (local notes
//! `local\notes\crash.md`).
//!
//! The collision geometry is 12 hard-coded points in the chassis frame (the rider has none). The original tests
//! them against the heightfield (vertical query) and against meshes (a segment from the chassis CM to each
//! point); engine ports see only meshes, so every point uses the mesh path: first hit of CM -> point, penetration
//! = normal x |(point - hit) . normal|, and the point counts as touching when the hit lies within 0.75 ft of it.

use crate::bike::{CHASSIS, FRONT, REAR};
use crate::math::{v3, Vec3};
use crate::multibody::System;
use crate::sim::Ground;

pub const POINTS: usize = 12;
/// Points whose contact is a crash (slot 5); 0 and 2 (handlebar ends) only when the takeoff slide assist is off.
const CRASH_POINTS: [usize; 4] = [1, 3, 4, 7];
const BAR_POINTS: [usize; 2] = [0, 2];

/// Contacts found at the end of a substep, consumed by the next substep's impulses.
#[derive(Clone, Debug, Default)]
pub struct ChassisContacts {
    /// Points in the chassis frame as last rebuilt.
    pub local: [Vec3; POINTS],
    /// Point i touches (hit within 0.75 ft of it).
    pub near: [bool; POINTS],
    /// Contacts: world point, penetration vector, normal (at most 12).
    pub hits: Vec<(Vec3, Vec3, Vec3)>,
}

/// Rebuilds the 12 points (slot 32 `0044ef70`): 8 fixed, 4 beside and below the wheel axles; the belly and axle
/// points rise by up to 1.5 ft with speed (full at 10 ft/s) unless crashed or in lean hold.
pub fn points(sys: &System, lift_on: bool) -> [Vec3; POINTS] {
    let b0 = &sys.bodies[CHASSIS];
    let s = b0.v.length();
    let lift = if lift_on { v3(0.0, 1.5 * (s / 10.0).min(1.0), 0.0) } else { Vec3::ZERO };
    let axle = |i: usize| b0.q.inv_rotate(sys.bodies[i].x - b0.x);
    let (f, r) = (axle(FRONT), axle(REAR));
    [
        v3(-1.5, 2.0, 0.75),
        v3(-0.5, 1.3, -3.5),
        v3(1.5, 2.0, 0.75),
        v3(0.5, 1.3, -3.5),
        v3(0.0, 1.0, 3.0),
        v3(0.0, -0.8, 0.5) + lift,
        v3(0.0, -0.8, -1.0) + lift,
        v3(0.0, 0.4, -3.1),
        f + v3(0.45, -0.6, 0.0) + lift,
        f + v3(-0.45, -0.6, 0.0) + lift,
        r + v3(0.45, -0.6, 0.0) + lift,
        r + v3(-0.45, -0.6, 0.0) + lift,
    ]
}

/// Detection at the end of a substep.
pub fn detect(sys: &System, ground: &dyn Ground, lift_on: bool) -> ChassisContacts {
    let b0 = &sys.bodies[CHASSIS];
    let local = points(sys, lift_on);
    let mut c = ChassisContacts { local, ..Default::default() };
    for (i, l) in local.iter().enumerate() {
        let p = b0.x + b0.q.rotate(*l);
        if let Some((hit, n)) = ground.raycast(b0.x, p) {
            if c.hits.len() < POINTS {
                let depth = (p - hit).dot(n).abs();
                c.hits.push((hit, n * depth, n));
            }
            if (hit - p).length() < 0.75 {
                c.near[i] = true;
            }
        }
    }
    c
}

/// Position correction: the mean penetration vector, added to every body (no velocity change).
pub fn depenetrate(sys: &mut System, c: &ChassisContacts) {
    if c.hits.is_empty() {
        return;
    }
    let s = c.hits.iter().fold(Vec3::ZERO, |s, h| s + h.1) / c.hits.len() as f64;
    for b in &mut sys.bodies {
        b.x += s;
    }
}

/// Velocity impulses on the chassis from the previous substep's contacts (restitution 0, friction 2.5 % of the
/// stopping impulse, friction torque scaled by c = 0.05 min(|v|, 100)); the net change is given to all four
/// bodies as one rigid assembly with |dw| <= 15 rad/s.
pub fn impulses(sys: &mut System, c: &ChassisContacts) {
    if c.hits.is_empty() {
        return;
    }
    let b0 = sys.bodies[CHASSIS].clone();
    let (x0, q0) = (b0.x, b0.q);
    let inv_m = 1.0 / b0.mass;
    let inv_i = v3(1.0 / b0.inertia.x, 1.0 / b0.inertia.y, 1.0 / b0.inertia.z);
    let speed = b0.v.length();
    let cf = 5.0 * speed.min(100.0) / 100.0;
    let (mut v, mut w) = (b0.v, b0.w);
    let mut hit = false;
    let k = |r: Vec3, e: Vec3| q0.rotate(inv_i.mul_elem(q0.inv_rotate(r.cross(e)))).cross(r).dot(e);
    for &(point, _, n) in &c.hits {
        let r = point - x0;
        let rb = q0.inv_rotate(r);
        let vp = v + q0.rotate(w.cross(rb));
        let vn = vp.dot(n);
        if vn >= 0.0 {
            continue;
        }
        hit = true;
        let jn = n * (-vn / (inv_m + k(r, n)));
        let vt = vp - n * vn;
        let jt = if vt.length() > 0.0 {
            let t = vt.normalize();
            t * (-(1.0 - 0.975) * vp.dot(t) / (inv_m + k(r, t)))
        } else {
            Vec3::ZERO
        };
        v += (jt + jn) * inv_m;
        let (jn_b, jt_b) = (q0.inv_rotate(jn), q0.inv_rotate(jt));
        w += inv_i.mul_elem((rb * cf).cross(jt_b) + rb.cross(jn_b));
    }
    if !hit {
        return;
    }
    let dv = v - b0.v;
    let mut dw = w - b0.w;
    if dw.length() > 15.0 {
        dw = dw * (15.0 / dw.length());
    }
    for b in &mut sys.bodies {
        let rb = q0.inv_rotate(b.x - x0);
        b.v += dv + q0.rotate(dw.cross(rb));
        b.w += b.q.inv_rotate(q0.rotate(dw));
    }
}

/// Slot 5: a crash-point contact (handlebar ends ignored while the takeoff slide assist is set).
pub fn crash_hit(c: &ChassisContacts, slide_assist: bool) -> bool {
    CRASH_POINTS.iter().any(|&i| c.near[i]) || (!slide_assist && BAR_POINTS.iter().any(|&i| c.near[i]))
}

/// Slot 4: any crash-point contact (unmasked), or stuck: input vector set, slower than 4 ft/s and airborne.
pub fn crash_touch(c: &ChassisContacts, input_active: bool, speed: f64, airborne: bool) -> bool {
    CRASH_POINTS.iter().chain(BAR_POINTS.iter()).any(|&i| c.near[i]) || (input_active && speed < 4.0 && airborne)
}
