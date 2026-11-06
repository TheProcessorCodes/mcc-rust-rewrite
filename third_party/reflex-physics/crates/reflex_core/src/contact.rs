//! Wheel contacts (spec 9, `00444b90`, `00438260`, `0042db20`) and ground support's position correction
//! (spec 10, `004456d0`).

use crate::math::{Quat, Vec3};
use crate::sim::{Ground, Triangle};

/// Half size of the query box around the wheel centre, ft.
pub const QUERY_HALF_EXTENT: f64 = 1.2;
/// Above this many triangles in the box the original finds no contact this step.
pub const MAX_TRIANGLES: usize = 400;
/// Ring buffer of crossing points.
pub const MAX_POINTS: usize = 8;
/// Per-triangle AABB pre-test inflation of the radius.
const AABB_INFLATE: f64 = 1.05;

/// Zero-width disc of `radius` in the wheel's local YZ plane (axle = local X) against triangles. Returns the
/// mean crossing point and the normalised mean triangle normal.
pub fn disc_contact(center: Vec3, q: Quat, radius: f64, tris: &[Triangle]) -> Option<(Vec3, Vec3)> {
    if tris.len() > MAX_TRIANGLES {
        return None;
    }
    let mut ring = [(Vec3::ZERO, Vec3::ZERO); MAX_POINTS];
    let mut count = 0usize;
    let r_box = radius * AABB_INFLATE;
    for t in tris {
        let lo = v_min(v_min(t.a, t.b), t.c);
        let hi = v_max(v_max(t.a, t.b), t.c);
        if (0..3).any(|i| hi[i] < center[i] - r_box || lo[i] > center[i] + r_box) {
            continue;
        }
        let (a, b, c) = (q.inv_rotate(t.a - center), q.inv_rotate(t.b - center), q.inv_rotate(t.c - center));
        let n = (b - a).cross(c - a);
        if n.length_squared() == 0.0 {
            continue;
        }
        // plane n.p = k crossed with the wheel plane x = 0: ny y + nz z = k, then with y^2 + z^2 = R^2
        let k = n.dot(a);
        let m2 = n.y * n.y + n.z * n.z;
        if m2 == 0.0 {
            continue;
        }
        let disc = radius * radius - k * k / m2;
        if disc < 0.0 {
            continue;
        }
        let foot = Vec3 { x: 0.0, y: k * n.y / m2, z: k * n.z / m2 };
        let dir = Vec3 { x: 0.0, y: -n.z, z: n.y } / m2.sqrt();
        let h = disc.sqrt();
        for p in [foot + dir * h, foot - dir * h] {
            let inside = (b - a).cross(p - a).dot(n) >= 0.0
                && (c - b).cross(p - b).dot(n) >= 0.0
                && (a - c).cross(p - c).dot(n) >= 0.0;
            if inside {
                ring[count % MAX_POINTS] = (center + q.rotate(p), t.normal);
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    let used = count.min(MAX_POINTS);
    let (sp, sn) = ring[..used].iter().fold((Vec3::ZERO, Vec3::ZERO), |(p, n), e| (p + e.0, n + e.1));
    // Faces of opposite sides (a thin plank, a two-sided surface given both ways round) can cancel: no usable
    // normal, so no contact (normalising it would turn the whole bike into NaN).
    if sn.length_squared() < 1e-12 {
        return None;
    }
    Some((sp / used as f64, sn.normalize()))
}

/// Queries the ground around a wheel and runs [`disc_contact`].
pub fn query(ground: &dyn Ground, center: Vec3, q: Quat, radius: f64, buf: &mut Vec<Triangle>) -> Option<(Vec3, Vec3)> {
    buf.clear();
    ground.triangles(center, QUERY_HALF_EXTENT, buf);
    disc_contact(center, q, radius, buf)
}

fn v_min(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x.min(b.x), y: a.y.min(b.y), z: a.z.min(b.z) }
}

fn v_max(a: Vec3, b: Vec3) -> Vec3 {
    Vec3 { x: a.x.max(b.x), y: a.y.max(b.y), z: a.z.max(b.z) }
}

/// One wheel's input to [`depenetrate`].
pub struct Penetration {
    /// Ground point P and rim point (C + R normalize(P - C)).
    pub point: Vec3,
    pub rim: Vec3,
    pub normal: Vec3,
    pub bottomed: bool,
}

/// Position correction (`004456d0`): returns the shift of the whole bike and, per wheel, how far that wheel
/// slides up its slider along `chassis_up` first. Positions only; velocities are untouched.
pub fn depenetrate(wheels: &[Option<Penetration>; 2], chassis_up: Vec3, crashed: bool) -> (Vec3, [f64; 2]) {
    let mut slide = [0.0; 2];
    let mut sum = Vec3::ZERO;
    let mut n_shifts = 0;
    for (i, w) in wheels.iter().enumerate() {
        let Some(w) = w else { continue };
        let n = w.normal;
        let mut d = w.point - w.rim;
        if crashed || n.y < 0.4 {
            d = n * d.dot(n);
        } else if n.y < 0.6 {
            d = d.lerp(n * d.dot(n), (0.6 - n.y) / 0.2);
        }
        d = d * 0.9;
        let a = d.dot(chassis_up);
        let shift = if d.y < 0.0 {
            -d
        } else if !w.bottomed && a >= 0.0 {
            slide[i] = a;
            d - chassis_up * a
        } else {
            d
        };
        sum += shift;
        n_shifts += 1;
    }
    let shift = if n_shifts > 0 { sum / n_shifts as f64 } else { Vec3::ZERO };
    (shift, slide)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::{Ground, PlaneGround};
    use crate::math::v3;

    #[test]
    fn disc_on_flat_ground() {
        let g = PlaneGround { point: Vec3::ZERO, normal: Vec3::Y };
        let mut buf = vec![];
        // wheel centre 1.1 ft above ground, radius 1.2: penetrates 0.1 ft, contact straight below
        let (p, n) = query(&g, v3(3.0, 1.1, -2.0), Quat::from_axis_angle(Vec3::X, 0.7), 1.2, &mut buf).unwrap();
        assert!((p - v3(3.0, 0.0, -2.0)).length() < 1e-12, "{p:?}");
        assert!((n - Vec3::Y).length() < 1e-12);
        // clear of the ground
        assert!(query(&g, v3(0.0, 1.3, 0.0), Quat::IDENTITY, 1.2, &mut buf).is_none());
        // leaning 30 deg about Z: the crossing points are still symmetric below the centre
        let q = Quat::from_axis_angle(Vec3::Z, 0.5);
        let (p, _) = query(&g, v3(0.0, 1.0, 0.0), q, 1.2, &mut buf).unwrap();
        assert!(p.y.abs() < 1e-12 && p.z.abs() < 1e-12);
        let _ = g.normal;
        let _: &dyn Ground = &g;
    }

    #[test]
    fn depenetration_splits_slide_and_lift() {
        let pen = |rim_y: f64| Penetration { point: Vec3::ZERO, rim: v3(0.0, rim_y, 0.0), normal: Vec3::Y, bottomed: false };
        let (shift, slide) = depenetrate(&[Some(pen(-0.1)), None], Vec3::Y, false);
        assert!((slide[0] - 0.09).abs() < 1e-12 && shift.length() < 1e-12);
        let mut b = pen(-0.1);
        b.bottomed = true;
        let (shift, slide) = depenetrate(&[Some(b), Some(pen(-0.1))], Vec3::Y, false);
        assert!(slide[0] == 0.0 && (slide[1] - 0.09).abs() < 1e-12, "{slide:?}");
        assert!((shift - v3(0.0, 0.045, 0.0)).length() < 1e-12);
    }
}
