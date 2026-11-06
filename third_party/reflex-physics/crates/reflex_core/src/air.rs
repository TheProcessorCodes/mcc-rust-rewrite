//! Air step (`0043f2e0`, spec 8.2) and the grounded bookkeeping slot 12 (`00443610`) does for it. Decoded from
//! the asm (local notes `local\notes\0043f2e0.md`). Corrections to the spec: the 10 rad/s clamp and the
//! AngularRateDamping run on every air step (not only with sticks); the pitch axis is normalize(up x v) (right
//! of travel); the lag filters freeze, not reset, on idle steps; trick kicks are applied here.

use crate::controls::Filter;
use crate::curve::Curve;
use crate::math::{Quat, Vec3};
use crate::multibody::System;

const D2R: f64 = 0.017_453_292_4;
/// Roll is added along (0, 0, 1); the original's w lane is junk and ignored here.
const ROLL_AX: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

/// Air tuning (`AirBorne_*`).
#[derive(Clone, Debug)]
pub struct AirParams {
    pub course_correction: f64,
    /// (roll, pitch, yaw) per-step multipliers.
    pub damping: [f64; 3],
    pub damping_takeoff: [f64; 3],
    /// Velocity curves LX, LY, RX, RY.
    pub velocity_curves: [Curve; 4],
    /// Lag time constants (roll, pitch, yaw).
    pub rpy_tau: [f64; 3],
}

/// State the air step keeps (sim+0x8b70..0x8d78).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AirState {
    pub air_time: f64,
    pub stick_time: f64,
    pub roll: Filter,
    pub pitch: Filter,
    pub yaw: Filter,
    /// Previous squared sticks LX, LY, RX, RY.
    pub prev_sq: [f64; 4],
    /// Right-stick diagonal term and its rate.
    pub diag: f64,
    pub diag_rate: f64,
    pub fade_a: f64,
    pub fade_b: f64,
    /// Pitch axis, world (set on the ground).
    pub pitch_axis: Vec3,
    /// Auto-level target orientation.
    pub target: Quat,
    /// Fade override enable / pending (sim+0x8c28 / 0x8c2c).
    pub fade_override: bool,
    pub fade_pending: bool,
    /// Lock-zone flags (left, right), set by the ground trick function.
    pub zone_left: bool,
    pub zone_right: bool,
    /// Trick A: armed, ran, strength, budget. Trick B: armed, ran, flick s, pull t2, budget, direction k.
    pub trick_a: (bool, bool, f64, i32),
    pub trick_b: (bool, bool, f64, f64, i32, f64),
}

impl Default for AirState {
    fn default() -> Self {
        AirState {
            air_time: 0.0,
            stick_time: 0.0,
            roll: Filter::new(0.1),
            pitch: Filter::new(0.1),
            yaw: Filter::new(0.1),
            prev_sq: [0.0; 4],
            diag: 0.0,
            diag_rate: 0.0,
            fade_a: 1.0,
            fade_b: 1.0,
            pitch_axis: Vec3::X,
            target: Quat::IDENTITY,
            fade_override: false,
            fade_pending: true,
            zone_left: false,
            zone_right: false,
            trick_a: (false, false, 0.0, 0),
            trick_b: (false, false, 0.0, 0.0, 0, 1.0),
        }
    }
}

/// Raw sticks: left X/Y (steer), right X/Y (rider).
#[derive(Clone, Copy, Debug, Default)]
pub struct Sticks {
    pub lx: f64,
    pub ly: f64,
    pub rx: f64,
    pub ry: f64,
}

/// Circle-to-square map (`00440c40`).
pub fn square(x: f64, y: f64) -> (f64, f64) {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    let r = (x * x + y * y).sqrt();
    let th = y.atan2(x);
    let p34 = 3.0 * FRAC_PI_4;
    if (-FRAC_PI_4..FRAC_PI_4).contains(&th) {
        (r, th / FRAC_PI_4 * r)
    } else if (FRAC_PI_4..p34).contains(&th) {
        ((FRAC_PI_2 - th) * r / FRAC_PI_4, r)
    } else if (-p34..-FRAC_PI_4).contains(&th) {
        ((th + FRAC_PI_2) * r / FRAC_PI_4, -r)
    } else {
        let y2 = if th > 0.0 { (PI - th) * r / FRAC_PI_4 } else { (-PI - th) * r / FRAC_PI_4 };
        (-r, y2)
    }
}

/// Right-stick "diagonalness" r (1 - cos 4 theta), signed by RX.
pub fn diag(rx: f64, ry: f64) -> f64 {
    let th4 = ry.atan2(rx) * 4.0;
    let c = (rx * rx + ry * ry).sqrt() * (1.0 - th4.cos());
    if rx < 0.0 {
        -c
    } else {
        c
    }
}

fn safe_normalize(v: Vec3) -> Vec3 {
    let l = v.length();
    if l > 0.0 {
        v / l
    } else {
        Vec3::ZERO
    }
}

/// Orientation with body axes X = r, Y = u, Z = f (columns), as `0043f2e0` builds it.
fn quat_from_axes(r: Vec3, u: Vec3, f: Vec3) -> Quat {
    let t_x = (r.x + 1.0 - u.y - f.z) * 0.25;
    let t_y = (1.0 - r.x + u.y - f.z) * 0.25;
    let t_z = (1.0 - r.x - u.y + f.z) * 0.25;
    let t_w = (r.x + 1.0 + u.y + f.z) * 0.25;
    let mx = t_x.max(t_y).max(t_z).max(t_w);
    if mx == t_w {
        let w = t_w.sqrt();
        let k = 0.25 / w;
        Quat::new((u.z - f.y) * k, (f.x - r.z) * k, (r.y - u.x) * k, w)
    } else if mx == t_x {
        let x = t_x.sqrt();
        let k = 0.25 / x;
        Quat::new(x, (u.x + r.y) * k, (f.x + r.z) * k, (u.z - f.y) * k)
    } else if t_y > t_z {
        let y = t_y.sqrt();
        let k = 0.25 / y;
        Quat::new((u.x + r.y) * k, y, (f.y + u.z) * k, (f.x - r.z) * k)
    } else {
        let z = t_z.sqrt();
        let k = 0.25 / z;
        Quat::new((f.x + r.z) * k, (f.y + u.z) * k, z, (r.y - u.x) * k)
    }
}

/// Grounded bookkeeping of `00443610`: lag filters to 0, stick history, fades to 1, stick time to 0, pitch axis
/// from the travel direction (or body X when the path is steep), auto-level target = current orientation.
pub fn ground_update(st: &mut AirState, sys: &System, s: Sticks) {
    st.air_time = 0.0;
    st.stick_time = 0.0;
    st.roll.value = 0.0;
    st.pitch.value = 0.0;
    st.yaw.value = 0.0;
    let (slx, sly) = square(s.lx, s.ly);
    let (srx, sry) = square(s.rx, s.ry);
    st.prev_sq = [slx, sly, srx, sry];
    st.diag = diag(s.rx, s.ry);
    st.fade_a = 1.0;
    st.fade_b = 1.0;
    let b0 = &sys.bodies[0];
    let vh = safe_normalize(b0.v);
    st.pitch_axis = if (3.0 * vh.y * vh.y).sqrt() < 0.95 {
        safe_normalize(Vec3::Y.cross(vh))
    } else {
        b0.q.rotate(Vec3::X)
    };
    st.target = b0.q;
    st.trick_a.1 = false;
    st.trick_b.1 = false;
}

/// Kinematic auto-level: rotate every body toward the target by `t` of the remaining angle (orientation
/// post-multiplied; positions rotated about the chassis as a world rotation, as the original does).
fn slerp_bodies(sys: &mut System, target: Quat, t: f64) {
    let q0 = sys.bodies[0].q;
    let mut d = q0.conj() * target;
    if d.w < 0.0 {
        d = Quat::new(-d.x, -d.y, -d.z, -d.w);
    }
    let d = d.normalize();
    let e = Quat::IDENTITY.slerp(d, t).normalize();
    let p0 = sys.bodies[0].x;
    for b in &mut sys.bodies {
        b.q = b.q * e;
        b.x = p0 + e.rotate(b.x - p0);
    }
}

/// One air step. `stick_inhibit` = sim+0x3c; `takeoff` = first airborne step (sim+0x8d9c).
pub fn step(sys: &mut System, st: &mut AirState, p: &AirParams, s: Sticks, stick_inhibit: bool, takeoff: bool, dt: f64) {
    st.air_time += dt;
    // course correction, same dv on every body
    let v0 = sys.bodies[0].v;
    let dir = safe_normalize(Vec3::Y.cross(v0));
    let dv = dir * (s.lx * p.course_correction) * (1.0 / sys.bodies[0].mass) * dt;
    for b in &mut sys.bodies {
        b.v += dv;
    }

    // mix coefficients from the old fades
    let (a, b) = (st.fade_a, st.fade_b);
    let c_lx_roll = 1.2 * a - 1.2;
    let c_rx_roll = -1.08 - b * 0.12;
    let c_rrx_roll = -1.5 * b;
    let c_rlx_roll = -0.15 * a;
    let c_ly_p1 = -0.75 * a + 0.75;
    let c_ry_p2 = b * 1.5 + 0.5;
    let c_rry_p2 = 1.5 * b;
    let c_lx_yaw = -a + 1.0;
    let c_rx_yaw = b * 0.06 + 0.54;
    let c_rlx_yaw = 0.5 * a;
    let c_d_yaw = 0.1 * b;

    let q0 = sys.bodies[0].q;
    let mut w = sys.bodies[0].w;
    let axis_body = q0.inv_rotate(st.pitch_axis);

    // tricks (armed by the ground trick function, not implemented: only their state is carried)
    let sel_a = st.trick_a.0 && !st.trick_b.0;
    let sel_b = !st.trick_a.0 && st.trick_b.0;
    if st.trick_a.1 && st.trick_a.3 > 0 {
        st.trick_a.3 -= 1;
    }
    if st.trick_b.1 && st.trick_b.4 > 0 {
        st.trick_b.4 -= 1;
    }
    if sel_a && st.trick_a.3 > 0 {
        let t = st.trick_a.2;
        st.trick_a.1 = true;
        w += axis_body * (t * -5.0 * D2R);
        let r = if s.ry > 0.9 { dt * 0.45 } else { dt * 0.1 };
        st.trick_a.2 = (t - r).max(0.3);
    } else {
        st.trick_a.0 = false;
        st.trick_a.1 = false;
    }
    if sel_b && st.trick_b.4 > 0 {
        let (sb, t2, k) = (st.trick_b.2, st.trick_b.3, st.trick_b.5);
        st.trick_b.1 = true;
        w += axis_body * (t2 * -10.0 * D2R) + ROLL_AX * (sb * k * D2R) + Vec3::Y * (sb * -0.5 * k * D2R);
        st.trick_b.2 = (sb - dt * 0.1).max(0.15);
        st.trick_b.3 = (t2 - dt * 0.1).max(0.15);
    } else {
        st.trick_b.0 = false;
        st.trick_b.1 = false;
    }

    let lock = (st.zone_left && s.lx < -0.9) || (st.zone_right && s.lx > 0.9);
    let left_used = s.lx.abs() > 0.05 || s.ly.abs() > 0.05;
    let right_used = s.rx.abs() > 0.05 || s.ry.abs() > 0.05;
    let active = (left_used || right_used) && !stick_inhibit && !lock && !st.trick_a.1;

    // fades
    st.fade_a = (a - if left_used { dt * 2.0 } else { dt * 2.5 }).max(0.0);
    st.fade_b = (b - if right_used { dt * 1.5 } else { dt * 2.5 }).max(0.0);
    if st.fade_override {
        if st.fade_pending {
            st.fade_a = 1.0;
            st.fade_b = 1.0;
        }
        if active {
            st.fade_pending = false;
        }
    }

    // stick shaping and outward rates
    let (slx, sly) = square(s.lx, s.ly);
    let (srx, sry) = square(s.rx, s.ry);
    let now = [slx, sly, srx, sry];
    let mut rate = [0.0; 4];
    for i in 0..4 {
        if now[i].abs() > st.prev_sq[i].abs() {
            rate[i] = (now[i] - st.prev_sq[i]) / dt;
        }
        rate[i] *= p.velocity_curves[i].eval((rate[i].abs() / 100.0).min(1.0));
    }
    st.prev_sq = now;
    let [r_lx, _r_ly, r_rx, r_ry] = rate;
    let c = diag(s.rx, s.ry);
    let d = if c.abs() > st.diag.abs() { (c - st.diag) / dt } else { 0.0 };
    st.diag = c;
    st.diag_rate = d;

    // auto-level target while the flight path is shallow
    let vh = safe_normalize(sys.bodies[0].v);
    if (3.0 * vh.y * vh.y).sqrt() < 0.99 {
        let side = safe_normalize(Vec3::Y.cross(vh));
        let u = safe_normalize(vh.cross(side));
        let r = safe_normalize(u.cross(vh));
        st.target = quat_from_axes(r, u, vh);
    }

    let mut wn = w;
    if active {
        st.stick_time += dt;
        let roll = srx * c_rx_roll + slx * c_lx_roll + r_rx * c_rrx_roll + r_lx * c_rlx_roll;
        let mut p1 = sly * c_ly_p1;
        let mut p2 = sry * c_ry_p2 + r_ry * c_rry_p2;
        if p1 > 0.0 {
            p1 *= 0.8;
        }
        if p2 > 0.0 {
            p2 *= 0.8;
        }
        let pitch = if p1.abs() > p2.abs() { p1 } else { p2 };
        let yaw = srx * c_rx_yaw + slx * c_lx_yaw + r_lx * c_rlx_yaw + d * c_d_yaw;
        let roll_f = st.roll.step(roll, dt);
        let pitch_f = st.pitch.step(pitch, dt);
        let yaw_f = st.yaw.step(yaw, dt);
        wn = axis_body * (pitch_f * D2R) + ROLL_AX * (roll_f * D2R) + Vec3::Y * (yaw_f * D2R) + w;
    } else if st.stick_time > 0.25 && !st.trick_b.1 && !st.trick_a.1 {
        slerp_bodies(sys, st.target, if lock { 0.012 } else { 0.008 });
    } else if lock {
        slerp_bodies(sys, st.target, 0.012);
    }

    // clamp and damping on every path; w.x by pitch, w.y by yaw, w.z by roll
    let m = wn.length();
    if m > 10.0 {
        wn = wn * (10.0 / m);
    }
    let mut dmp = p.damping;
    if takeoff {
        for (d, t) in dmp.iter_mut().zip(p.damping_takeoff) {
            *d *= t;
        }
    }
    sys.bodies[0].w = Vec3 { x: wn.x * dmp[1], y: wn.y * dmp[2], z: wn.z * dmp[0] };
}
