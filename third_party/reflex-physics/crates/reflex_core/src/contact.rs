//! Wheel contacts (slot 13 `00444b90`: the terrain query `00444a40`, the disc / triangle test `00438260`, the contact
//! record `0042db20`), the position correction of the touching wheels (`004456d0` with `00431ee0` / `0043cfe0`), the
//! rolling orientation of the wheels (`00445800`) and the resting pin (`00446f80`). Checked against OpenReflex
//! `bike_slot13.c` / `bike_contact.c`.

use crate::bike::{CHASSIS, FRONT, REAR};
use crate::math::{Quat, Vec3};
use crate::multibody::System;
use crate::sim::{Ground, Triangle};

/// Half size of the query box around the wheel centre, ft (`00444a40`: the splat 1.2 [00930064]). The original's
/// height-field query cuts a +-1.2 window in x / z only; a mesh ground cuts a cube, and the disc test's own box
/// (1.05 radius) bounds y anyway.
pub const QUERY_HALF_EXTENT: f64 = 1.2;
/// Above this many triangles in the box the original finds no contact this step (`007c6160`: count > capacity
/// 400 leaves the count at 0).
pub const MAX_TRIANGLES: usize = 400;
/// Ring buffer of crossing points (`00438260`: slot = (slot + 1) & 7, mean of min(hits, 8)).
pub const MAX_POINTS: usize = 8;
/// Per-triangle AABB pre-test inflation of the radius ([009774c8] 1.05).
const AABB_INFLATE: f64 = 1.05;

/// Zero-width disc of `radius` in the wheel's local YZ plane (axle = local X) against triangles (`00438260`).
/// Returns the mean crossing point and the normalised mean triangle normal, both in the world.
///
/// As the original: a triangle counts when its box grown by 1.05 radius holds the centre (inclusive); its plane is
/// crossed with the circle y^2 + z^2 = R^2 only when the quadratic has two distinct roots (discriminant > 0); a
/// root counts when it lies inside the triangle or on an edge; the normal summed is the triangle's own unit normal
/// from its winding (e01 x e02 in the wheel frame), not a stored one.
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
        let n = n.normalize();
        // plane n.p = k crossed with the wheel plane x = 0: ny y + nz z = k, then with y^2 + z^2 = R^2
        let k = n.dot(a);
        let m2 = n.y * n.y + n.z * n.z;
        if m2 == 0.0 {
            continue;
        }
        let disc = radius * radius - k * k / m2;
        // OpenReflex bike_slot13.c 00438260: real roots only when 0 < b^2 - 4ac (a tangent plane is no contact)
        if disc <= 0.0 {
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
                ring[count % MAX_POINTS] = (p, n);
                count += 1;
            }
        }
    }
    if count == 0 {
        return None;
    }
    let used = count.min(MAX_POINTS);
    let (sp, sn) = ring[..used].iter().fold((Vec3::ZERO, Vec3::ZERO), |(p, n), e| (p + e.0, n + e.1));
    // Deliberate deviation: faces of opposite sides (a thin plank, a two-sided surface given both ways round) can
    // cancel. The original would normalise the zero sum; here that is no contact (it would turn the bike into NaN).
    if sn.length_squared() < 1e-12 {
        return None;
    }
    Some((center + q.rotate(sp / used as f64), q.rotate(sn.normalize())))
}

/// Queries the ground around a wheel and runs [`disc_contact`] (`00444a40`).
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
    /// Ground point P (W+50) and rim point (W+60 = C + R normalize(P - C)).
    pub point: Vec3,
    pub rim: Vec3,
    pub normal: Vec3,
    /// The strut below its lower travel limit this step (W+108, set by `0043c590`).
    pub bottomed: bool,
}

/// Share of the penetration corrected per substep ([009cd970] 0.9).
pub const CORRECTION_SCALE: f64 = 0.9;

/// Position correction (`004456d0`, per wheel `00431ee0`, rider off `0043cfe0`): returns the shift of the whole bike
/// and, per wheel, how far that wheel slides up its slider along `chassis_up` first. Positions only; velocities are
/// untouched. `crashed` selects the rider-off variant (only the part along the normal).
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
        d = d * CORRECTION_SCALE;
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

/// Time constant of the six contact-point filters S+8e40 / S+8e4c, s (set at placement, `build_vehicle_place.c`:
/// 0x3dcccccd).
pub const ROLL_FILTER_TAU: f64 = 0.1;

/// The rolling-orientation filters (`00445800`): low-passed front / rear wheel positions.
#[derive(Clone, Copy, Debug, Default)]
pub struct RollState {
    pub filt: [Vec3; 2],
    /// The original starts the filters at the origin; here they start at the wheels on the first update, so a
    /// spawn far from the origin does not whirl the wheels for the first few tenths of a second.
    pub started: bool,
}

/// The filters' step (inline `0042d6b0`, factor = min(tau, dt) / tau): returns the filtered velocities of the
/// front and rear wheel positions, (new - old) / dt.
pub fn roll_velocities(st: &mut RollState, pos: [Vec3; 2], dt: f64) -> [Vec3; 2] {
    if !st.started {
        st.filt = pos;
        st.started = true;
    }
    let k = ROLL_FILTER_TAU.min(dt) / ROLL_FILTER_TAU;
    let mut v = [Vec3::ZERO; 2];
    for i in 0..2 {
        let old = st.filt[i];
        let new = (pos[i] - old) * k + old;
        v[i] = (new - old) / dt;
        st.filt[i] = new;
    }
    v
}

/// One wheel rolling on the ground (`00445800` roll_wheel): the body-frame spin keeps its y / z and takes
/// x = (vel . rolling direction) / radius, and the orientation is integrated from the substep's starting one:
/// q = normalize(q0 + q0 w dt / 2).
pub fn roll_wheel(q_prev: Quat, w_body: Vec3, vel: Vec3, roll_dir: Vec3, radius: f64, dt: f64) -> Quat {
    let c = vel.dot(roll_dir);
    let dq = q_prev * Quat::new(c / radius, w_body.y, w_body.z, 0.0);
    let h = 0.5 * dt;
    Quat::new(q_prev.x + h * dq.x, q_prev.y + h * dq.y, q_prev.z + h * dq.z, q_prev.w + h * dq.w).normalize()
}

/// The resting pin (`00446f80`), every substep while resting with the rider on: the whole bike is turned about the
/// chassis so that the line between its rim points (front, rear: the chassis-frame rim offsets `arms` from the last
/// contacts, W+90, around the wheel centres) points along the contact points saved when it came to rest (`saved`:
/// front, rear, S+8b10 / S+8b00), then moved so the rear rim point lies on the saved rear contact point.
pub fn rest_pin(sys: &mut System, arms: [Vec3; 2], saved: [Vec3; 2]) {
    let rim = |sys: &System, i: usize, wi: usize| sys.bodies[CHASSIS].q.rotate(arms[i]) + sys.bodies[wi].x;
    let a = rim(sys, 0, FRONT) - rim(sys, 1, REAR);
    let b = saved[0] - saved[1];
    if a.length_squared() > 0.0 && b.length_squared() > 0.0 {
        let (a, b) = (a.normalize(), b.normalize());
        let axis = a.cross(b);
        // a zero axis normalises to zero in the original, which leaves the identity rotation
        if axis.length_squared() > 0.0 {
            let r = Quat::from_axis_angle(axis.normalize(), b.dot(a).clamp(-1.0, 1.0).acos());
            // Mechanism_Rotate 0085c530: every body about the root position
            let x0 = sys.bodies[CHASSIS].x;
            for body in &mut sys.bodies {
                body.x = x0 + r.rotate(body.x - x0);
                body.q = (r * body.q).normalize();
            }
        }
    }
    // Mechanism_Translate 0085c500
    let shift = saved[1] - rim(sys, 1, REAR);
    for body in &mut sys.bodies {
        body.x += shift;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3;
    use crate::sim::{Ground, PlaneGround};

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
    fn tangent_plane_is_no_contact() {
        // the circle just touching the plane: one double root, which the original does not count
        let g = PlaneGround { point: Vec3::ZERO, normal: Vec3::Y };
        let mut buf = vec![];
        assert!(query(&g, v3(0.0, 1.0, 0.0), Quat::IDENTITY, 1.0, &mut buf).is_none());
    }

    #[test]
    fn normal_comes_from_the_winding() {
        // a stored normal that disagrees with the winding is ignored, as the original has none
        let t = Triangle { a: v3(-5.0, 0.0, -5.0), b: v3(-5.0, 0.0, 5.0), c: v3(5.0, 0.0, 0.0), normal: -Vec3::Y };
        let (_, n) = disc_contact(v3(0.0, 1.0, 0.0), Quat::IDENTITY, 1.2, &[t]).unwrap();
        assert!((n - Vec3::Y).length() < 1e-12, "{n:?}");
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

    #[test]
    fn roll_filter_and_spin() {
        let mut st = RollState::default();
        let dt = 0.0075;
        // first update starts the filter on the wheels: no velocity
        let v = roll_velocities(&mut st, [v3(0.0, 0.0, 0.0), v3(0.0, 0.0, -4.0)], dt);
        assert!(v[0].length() == 0.0 && v[1].length() == 0.0);
        // a step of 0.1 ft: the filter takes dt / tau of it
        let v = roll_velocities(&mut st, [v3(0.0, 0.0, 0.1), v3(0.0, 0.0, -3.9)], dt);
        assert!((v[0].z - 0.1 * (dt / ROLL_FILTER_TAU) / dt).abs() < 1e-12, "{v:?}");
        // rolling forward (+z) at 10 ft/s on a 1 ft wheel: +10 rad/s about the axle (+x)
        let q = roll_wheel(Quat::IDENTITY, v3(3.0, 0.0, 0.0), v3(0.0, 0.0, 10.0), Vec3::Z, 1.0, dt);
        let expect = Quat::from_axis_angle(Vec3::X, 10.0 * dt);
        assert!((q.x - expect.x).abs() < 1e-4 && (q.w - expect.w).abs() < 1e-4, "{q:?}");
    }

    #[test]
    fn rest_pin_puts_the_rims_on_the_saved_points() {
        let vehicle = crate::vehicle::Vehicle::default();
        let (mut sys, _) = crate::bike::build_system(&vehicle, v3(0.0, 2.0, 0.0), Quat::IDENTITY);
        let arms = [v3(0.0, -1.0, 0.0), v3(0.0, -1.0, 0.0)];
        let rim = |sys: &System, i: usize, wi: usize| sys.bodies[CHASSIS].q.rotate(arms[i]) + sys.bodies[wi].x;
        let (f0, r0) = (rim(&sys, 0, FRONT), rim(&sys, 1, REAR));
        // saved points: the same line tilted 5 deg nose up and moved 0.2 ft forward
        let tilt = Quat::from_axis_angle(Vec3::X, -0.087);
        let saved = [r0 + tilt.rotate(f0 - r0) + v3(0.0, 0.0, 0.2), r0 + v3(0.0, 0.0, 0.2)];
        rest_pin(&mut sys, arms, saved);
        let (f1, r1) = (rim(&sys, 0, FRONT), rim(&sys, 1, REAR));
        assert!((r1 - saved[1]).length() < 1e-9, "{r1:?}");
        assert!(((f1 - r1).normalize() - (saved[0] - saved[1]).normalize()).length() < 1e-9);
    }

    #[test]
    fn parked_on_a_slope_stays_put() {
        // a 10 deg slope, brakes held: the bike settles, comes to rest and the pin holds it (no creep, no NaN).
        // Needs a real vehicle file: REFLEX_VEHICLE, skipped without it.
        let Some(path) = std::env::var_os("REFLEX_VEHICLE").filter(|p| std::path::Path::new(p).exists()) else {
            eprintln!("skipped: set REFLEX_VEHICLE to a Reflex vehicle file");
            return;
        };
        let n = Quat::from_axis_angle(Vec3::X, 0.1745).rotate(Vec3::Y);
        let g = PlaneGround { point: Vec3::ZERO, normal: n };
        let mut sim = crate::Sim::from_file(path, v3(0.0, 3.0, 0.0), Quat::IDENTITY).expect("vehicle file");
        let brakes = crate::Inputs { front_brake: 1.0, rear_brake: 1.0, ..Default::default() };
        for _ in 0..240 {
            sim.frame(1.0 / 60.0, brakes, &g);
        }
        let x0 = sim.sys.bodies[CHASSIS].x;
        for _ in 0..120 {
            sim.frame(1.0 / 60.0, brakes, &g);
        }
        let x1 = sim.sys.bodies[CHASSIS].x;
        assert!(sim.sys.bodies.iter().all(|b| b.x.length().is_finite() && b.q.length().is_finite()));
        assert!(sim.at_rest && (x1 - x0).length() < 0.005, "crept {:?} (at rest {})", x1 - x0, sim.at_rest);
    }
}
