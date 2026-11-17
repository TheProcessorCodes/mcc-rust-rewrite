//! Chassis collision (`00435b00` detection, `00438180` position correction, `004360b0` impulses) and the
//! per-substep crash flags (slot 4 `0043b4a0`, slot 5 `0043b5e0`). Checked against OpenReflex `bike_slot13.c`,
//! `bike_contact.c`, `bike_response.c`, `bike_post.c` (slot 32) and `phys_pair2.c` (`0086fac0` / `0085ee70`).
//!
//! The collision geometry is 12 hard-coded probe points in the chassis frame (the rider has none). The original tests
//! them two ways: against the height-field terrain (a vertical query under each point, flags S+1af0) and against
//! the meshes of the level's objects (the DynamicRays shape S+20d0: one segment from the chassis origin to each
//! point, flags S+1b54). Engine ports have no height field, so the whole ground goes the mesh way, as the original
//! does it for objects: every front-facing triangle each segment crosses is a contact, with penetration =
//! normal x |point - hit|, and a probe touches when any contact lies within 0.75 ft of it.

use crate::bike::{CHASSIS, FRONT, REAR};
use crate::math::{v3, Vec3};
use crate::multibody::System;
use crate::sim::{Ground, Triangle};

pub const POINTS: usize = 12;
/// Points whose contact is a crash (slot 5); 0 and 2 (handlebar ends) only when the takeoff slide assist is off.
const CRASH_POINTS: [usize; 4] = [1, 3, 4, 7];
const BAR_POINTS: [usize; 2] = [0, 2];

/// Contacts found at the end of a substep, consumed by the next substep's impulses.
#[derive(Clone, Debug, Default)]
pub struct ChassisContacts {
    /// Points in the chassis frame as last rebuilt.
    pub local: [Vec3; POINTS],
    /// Probe i touches (some contact within 0.75 ft of it: S+1b54 + 4i).
    pub near: [bool; POINTS],
    /// Contacts: world point (S+2100), penetration vector (S+21c0), normal (S+2280); at most 12.
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

/// A probe touches when a contact lies closer than this to it, ft ([00935aa8] 0.75).
pub const NEAR: f64 = 0.75;
/// Smallest determinant of the segment / triangle test ([009777c4] 1e-6): back faces and grazing hits do not count.
const DET_EPS: f64 = 1e-6;

/// Segment s -> e against one triangle as `0085ee70` tests it (Moller-Trumbore on the unit direction, one-sided):
/// the hit point and the triangle's unit normal from its winding when the segment enters the front face strictly
/// between its ends.
fn segment_triangle(s: Vec3, e: Vec3, t: &Triangle) -> Option<(Vec3, Vec3)> {
    let d = e - s;
    let len = d.length();
    if len == 0.0 {
        return None;
    }
    let dir = d / len;
    let (e1, e2) = (t.b - t.a, t.c - t.a);
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det < DET_EPS {
        return None;
    }
    let inv = 1.0 / det;
    let sv = s - t.a;
    let u = sv.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = sv.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let dist = e2.dot(q) * inv;
    if !(dist > 0.0 && dist < len && dist < f32::MAX as f64) {
        return None;
    }
    let n = e1.cross(e2);
    Some((s + dir * dist, n / n.length()))
}

/// Detection at the end of a substep (`00435b00`, mesh part, with the pair `0086fac0`): the 12 segments from the
/// chassis origin to the probes against the ground's triangles. Contacts are taken in the pair's order (segment by
/// segment when there are at least as many triangles as segments, else triangle by triangle), at most 12; then
/// every probe within 0.75 ft of any contact is marked touching.
pub fn detect(sys: &System, ground: &dyn Ground, lift_on: bool) -> ChassisContacts {
    let b0 = &sys.bodies[CHASSIS];
    let local = points(sys, lift_on);
    let mut c = ChassisContacts { local, ..Default::default() };
    let world: [Vec3; POINTS] = std::array::from_fn(|i| b0.x + b0.q.rotate(local[i]));
    // one query box around the chassis origin holding every segment (two-sided triangles then face the origin)
    let half = world.iter().fold(0.0f64, |m, p| {
        let d = *p - b0.x;
        m.max(d.x.abs()).max(d.y.abs()).max(d.z.abs())
    });
    let mut tris = Vec::new();
    ground.triangles(b0.x, half, &mut tris);
    let test = |k: usize, t: &Triangle, hits: &mut Vec<(Vec3, Vec3, Vec3)>| {
        if hits.len() >= POINTS {
            return;
        }
        let (s, e) = (b0.x, world[k]);
        let (slo, shi) = (v_min(s, e), v_max(s, e));
        let (tlo, thi) = (v_min(v_min(t.a, t.b), t.c), v_max(v_max(t.a, t.b), t.c));
        if (0..3).any(|i| tlo[i] > shi[i] || slo[i] > thi[i]) {
            return;
        }
        if let Some((hit, n)) = segment_triangle(s, e, t) {
            // depth vec4 = |segment end - hit| (0086fac0), contact = depth x normal (00435b00)
            hits.push((hit, n * (e - hit).length(), n));
        }
    };
    if tris.len() >= POINTS {
        for k in 0..POINTS {
            for t in &tris {
                test(k, t, &mut c.hits);
            }
        }
    } else {
        for t in &tris {
            for k in 0..POINTS {
                test(k, t, &mut c.hits);
            }
        }
    }
    for &(hit, _, _) in &c.hits {
        for (k, p) in world.iter().enumerate() {
            if (hit - *p).length() < NEAR {
                c.near[k] = true;
            }
        }
    }
    c
}

fn v_min(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn v_max(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::Quat;
    use crate::sim::PlaneGround;

    #[test]
    fn belly_probes_through_flat_ground() {
        let vehicle = crate::vehicle::Vehicle::default();
        let (sys, _) = crate::bike::build_system(&vehicle, v3(0.0, 0.5, 0.0), Quat::IDENTITY);
        let g = PlaneGround { point: Vec3::ZERO, normal: Vec3::Y };
        let c = detect(&sys, &g, false);
        // probe 5 (0, -0.8, 0.5) is 0.3 ft under: the segment from the origin enters at 5/8 of its length, and the
        // depth is the rest of the segment, not the vertical 0.3
        let e = v3(0.0, -0.3, 0.5);
        let hit = v3(0.0, 0.0, 0.5 * 0.625);
        let h = c.hits.iter().find(|h| (h.0 - hit).length() < 1e-9).expect("probe 5 contact");
        assert!((h.1 - Vec3::Y * (e - hit).length()).length() < 1e-9, "{h:?}");
        assert!(c.near[5] && c.near[6]);
        // the probes up top are far from every contact
        assert!(!c.near[0] && !c.near[2] && !c.near[4]);
        assert!(c.hits.len() <= POINTS);
        // seen from behind (ground facing down) nothing is hit
        let g = PlaneGround { point: Vec3::ZERO, normal: -Vec3::Y };
        assert!(detect(&sys, &g, false).hits.is_empty());
    }
}
