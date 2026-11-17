//! Air step (`0043f2e0`, spec 8.2), the grounded bookkeeping slot 12 (`00443610`) does for it, and the
//! per-frame stunt latches / stick gestures (`00440da0`). Checked against OpenReflex `bike_air.c` and
//! `bike_slot12.c`. Corrections to the spec: the 10 rad/s clamp and the AngularRateDamping run on every air step
//! (not only with sticks); the pitch axis is normalize(up x v) (right of travel); the lag filters freeze, not
//! reset, on idle steps; the gesture moves are applied here.
//!
//! Not ported: the pitch spring of slot 17 (`0043ed40`, S+8d90 / 8d94); it only feeds the rider animation record.
//! The debug pose-freeze key (S+8c28 / 8c2c) keeps its fade behaviour, but its saved orientation (S+8c30) is not
//! kept, so the latches always measure against the take-off orientation.

use crate::controls::Filter;
use crate::curve::Curve;
use crate::math::{Quat, Vec3};
use crate::multibody::System;

/// 00977678
const D2R: f64 = 0.017_453_290_5;
/// The original's forward axis (runtime vec4 00d61680): roll is added along it.
const ROLL_AX: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 1.0 };

/// Air tuning (`AirBorne_*`).
#[derive(Clone, Debug)]
pub struct AirParams {
    pub course_correction: f64,
    /// AirBorne_AngularRateDamping (x, y, z) as stored (S+5680); body w.x is scaled by [1], w.y by [2], w.z by [0].
    pub damping: [f64; 3],
    pub damping_takeoff: [f64; 3],
    /// Velocity curves LX, LY, RX, RY.
    pub velocity_curves: [Curve; 4],
    /// Lag time constants (roll, pitch, yaw).
    pub rpy_tau: [f64; 3],
}

/// The stick gestures `00440da0` records on the ground near a lip with the stunt modifier held, and the two
/// moves the air step plays from them (S+8d40..8d78).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gestures {
    /// "Pull back" armed: rider stick y held below -0.9 (S+8d48); running (S+8d44); strength (S+8d4c, the
    /// strongest pull rate remapped to [0.005, 0.75]); steps left (S+8d50); previous stick y (S+8d54).
    pub pull_armed: bool,
    pub pull_ran: bool,
    pub pull_strength: f64,
    pub pull_budget: i32,
    pub pull_prev_y: f64,
    /// "Flick up and pull back" armed (S+8d60); running (S+8d5c); x / y flick strengths in [0.25, 1]
    /// (S+8d64 / 8d68); steps left (S+8d6c); previous stick y / x (S+8d70 / 8d74); side of the last x flick
    /// (S+8d78, -1 right, +1 left).
    pub flick_armed: bool,
    pub flick_ran: bool,
    pub flick_x: f64,
    pub flick_y: f64,
    pub flick_budget: i32,
    pub flick_prev_y: f64,
    pub flick_prev_x: f64,
    pub side: f64,
}

impl Default for Gestures {
    fn default() -> Self {
        Gestures {
            pull_armed: false,
            pull_ran: false,
            pull_strength: 0.0,
            pull_budget: 0,
            pull_prev_y: 0.0,
            flick_armed: false,
            flick_ran: false,
            flick_x: 0.0,
            flick_y: 0.0,
            flick_budget: 0,
            flick_prev_y: 0.0,
            flick_prev_x: 0.0,
            side: 1.0,
        }
    }
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
    /// Pitch axis, world (S+8bf0, set on the ground).
    pub pitch_axis: Vec3,
    /// Take-off orientation (S+8c10): the auto-level / trick-hold target and the latches' reference.
    pub target: Quat,
    /// Fade override enable / pending (sim+0x8c28 / 0x8c2c; a debug key in the original).
    pub fade_override: bool,
    pub fade_pending: bool,
    /// Stunt cone latches: [0..3] S+8c60.. (the cones at S+8c70.., held with the steering stick left),
    /// [3..6] S+8cd0.. (S+8ce0.., held with it right).
    pub latches: [bool; 6],
    pub gestures: Gestures,
}

impl Default for AirState {
    /// As the simulation is constructed (`0043ac40`): fades 0, pitch axis zero, identity target.
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
            fade_a: 0.0,
            fade_b: 0.0,
            pitch_axis: Vec3::ZERO,
            target: Quat::IDENTITY,
            fade_override: false,
            fade_pending: true,
            latches: [false; 6],
            gestures: Gestures::default(),
        }
    }
}

impl AirState {
    /// The state after construction and placement at orientation `q` (`0043ac40`, then the placement in
    /// OpenReflex `build_vehicle_place.c`): lag filters 0 with the vehicle's time constants, target identity,
    /// pitch axis = the bike's X axis.
    pub fn placed(q: Quat, rpy_tau: [f64; 3]) -> Self {
        let mut st = AirState::default();
        st.roll.tau = rpy_tau[0];
        st.pitch.tau = rpy_tau[1];
        st.yaw.tau = rpy_tau[2];
        st.pitch_axis = q.rotate(Vec3::X);
        st
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

/// Right-stick "diagonalness" r (1 - cos 4 theta), signed by RX. The original's polynomial evaluates
/// cos(pi - 4 theta) = -cos 4 theta and takes r (1 - (-A)).
pub fn diag(rx: f64, ry: f64) -> f64 {
    let th4 = ry.atan2(rx) * 4.0;
    let c = (rx * rx + ry * ry).sqrt() * (1.0 - th4.cos());
    if rx < 0.0 {
        -c
    } else {
        c
    }
}

/// `00424370`: `value` mapped linearly from [in_lo, in_hi] onto [out_lo, out_hi] and clamped to that range;
/// out_lo for an empty input range.
pub fn remap_clamped(in_lo: f64, in_hi: f64, value: f64, out_lo: f64, out_hi: f64) -> f64 {
    if in_lo == in_hi {
        return out_lo;
    }
    let t = (value - in_lo) / (in_hi - in_lo);
    let (lo, hi) = if out_hi > out_lo { (out_lo, out_hi) } else { (out_hi, out_lo) };
    let mut r = (out_hi - out_lo) * t + out_lo;
    if !(r > lo) {
        r = lo;
    }
    if !(hi > r) {
        r = hi;
    }
    r
}

/// The "stick moved away from rest" rate of `0043f2e0` / `00440da0`: (now - before) / dt when |now| > |before|.
fn flick(now: f64, before: f64, dt: f64) -> Option<f64> {
    (now.abs() > before.abs()).then(|| (now - before) / dt)
}

/// One stunt cone (`0042de50` records at S+8c70.. / 8ce0..): a reference rotation and three limit angles.
struct Cone {
    q: Quat,
    lim: [f64; 3],
}

/// The six cones `0043ac40` writes (0043b271..); the second three mirror the first (y and z negated).
const fn cone(x: f64, y: f64, z: f64, w: f64, lim: [f64; 3]) -> Cone {
    Cone { q: Quat { x, y, z, w }, lim }
}
const CONES: [Cone; 6] = [
    cone(0.017_863, 0.339_95, -0.363_843, 0.867_025, [0.436_332_26, 0.261_799_37, 0.349_065_81]),
    cone(-0.335_083, 0.435_346, -0.403_875, 0.731_491, [0.523_598_73; 3]),
    cone(0.416_745, 0.288_458, -0.251_483, 0.824_543, [0.349_065_81, 0.261_799_37, 0.349_065_81]),
    cone(0.017_863, -0.339_95, 0.363_843, 0.867_025, [0.436_332_26, 0.261_799_37, 0.349_065_81]),
    cone(-0.335_083, -0.435_346, 0.403_875, 0.731_491, [0.523_598_73; 3]),
    cone(0.416_745, -0.288_458, 0.251_483, 0.824_543, [0.349_065_81, 0.261_799_37, 0.349_065_81]),
];

/// The cone limit for an azimuth `a` in (-pi, pi]: linear between `li` and `lj` over each quarter turn, the two
/// swapped between quarters (OpenReflex `phys_quat.c` quarter_limit).
fn quarter_limit(a: f64, li: f64, lj: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, PI};
    let (mut lo, mut hi, mut a) = (li, lj, a);
    if !(0.0..=FRAC_PI_2).contains(&a) {
        if a > FRAC_PI_2 {
            (lo, hi, a) = (lj, li, a - FRAC_PI_2);
        } else if (-FRAC_PI_2..=0.0).contains(&a) {
            (lo, hi, a) = (lj, li, a + FRAC_PI_2);
        } else {
            a += PI;
        }
    }
    (hi - lo) * a / FRAC_PI_2 + lo
}

/// `0042de70`: does the rotation p = conj(reference * cone.q) * q keep the forward axis inside the cone limits
/// (L+10 / L+14 by its azimuth) and the up axis inside (L+18 / L+10)?
fn within_cone(c: &Cone, q: Quat, reference: Quat) -> bool {
    let p = (reference * c.q).conj() * q;
    let w1 = p.rotate(ROLL_AX);
    let in1 = quarter_limit(w1.x.atan2(w1.y), c.lim[0], c.lim[1]) > w1.z.clamp(-1.0, 1.0).acos();
    let w2 = p.rotate(Vec3::Y);
    let in2 = quarter_limit(w2.z.atan2(w2.x), c.lim[2], c.lim[0]) > w2.y.clamp(-1.0, 1.0).acos();
    in1 && in2
}

/// Once per frame after the physics step (`00440da0`, called from the vehicle's frame update `00463900`).
/// In the air the cone latches record which stunt poses the bike has passed through since take-off; on the
/// ground, while leaving a lip (`lip_takeoff`, S+8c24) with the stunt modifier held (`stunt`, S+3c), the rider
/// stick's pull-back and flicks are recorded for the gesture moves. `q` is the chassis orientation, `rx` / `ry`
/// the rider stick (ctl+0x70 / 0x74), `dt` the frame time.
#[allow(clippy::too_many_arguments)]
pub fn stunt_latches(st: &mut AirState, q: Quat, rx: f64, ry: f64, airborne: bool, lip_takeoff: bool, stunt: bool, dt: f64) {
    let g = &mut st.gestures;
    if airborne && !g.pull_ran && !g.flick_ran {
        for (l, c) in st.latches.iter_mut().zip(&CONES) {
            *l = *l || within_cone(c, q, st.target);
        }
    }
    if !lip_takeoff || !stunt {
        if !airborne {
            let budgets = (g.pull_ran, g.pull_budget, g.flick_ran, g.flick_budget);
            *g = Gestures::default();
            (g.pull_ran, g.pull_budget, g.flick_ran, g.flick_budget) = budgets;
        }
        return;
    }
    if airborne {
        return;
    }
    if !g.flick_armed && !g.flick_ran {
        // the strongest pull rate while the stick moves away from rest; past 0.9 from rest counts as rate 0
        let rate = if ry.abs() > 0.9 && g.pull_prev_y.abs() == 0.0 { Some(0.0) } else { flick(ry, g.pull_prev_y, dt) };
        if let Some(rate) = rate {
            let r = remap_clamped(10.0, 40.0, rate.abs(), 0.005, 0.75);
            g.pull_strength = if g.pull_strength > r { g.pull_strength } else { r };
        }
        g.pull_prev_y = ry;
        if -0.9 > ry {
            g.pull_armed = true;
        } else {
            g.pull_armed = false;
            g.pull_strength = 0.0;
        }
    }
    if g.pull_armed || g.pull_ran {
        return;
    }
    if let Some(rate) = flick(rx, g.flick_prev_x, dt) {
        g.side = if rate >= 0.0 { -1.0 } else { 1.0 };
        let r = remap_clamped(100.0, 250.0, rate.abs(), 0.25, 1.0);
        g.flick_x = if g.flick_x > r { g.flick_x } else { r };
    }
    g.flick_prev_x = rx;
    if let Some(rate) = flick(ry, g.flick_prev_y, dt) {
        let r = remap_clamped(100.0, 250.0, rate.abs(), 0.25, 1.0);
        g.flick_y = if g.flick_y > r { g.flick_y } else { r };
    }
    g.flick_prev_y = ry;
    if -0.4 > ry && g.flick_x >= 0.25 {
        g.flick_armed = true;
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

/// Grounded bookkeeping of `00443610`, after the grounded correction: lag filters to 0, stick history, fades to 1,
/// stick time to 0, pitch axis from the travel direction (or body X when the path is steep), take-off orientation =
/// current orientation, cone latches cleared, gesture moves stopped with 600 steps each.
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
    st.target = b0.q;
    st.latches = [false; 6];
    let vh = safe_normalize(b0.v);
    // 009cd9ac = 0.95, compared with sqrt(3 vy^2) as the original does
    st.pitch_axis = if (3.0 * vh.y * vh.y).sqrt() < 0.95 {
        safe_normalize(Vec3::Y.cross(vh))
    } else {
        b0.q.rotate(Vec3::X)
    };
    let g = &mut st.gestures;
    g.pull_ran = false;
    g.pull_budget = 600;
    g.flick_ran = false;
    g.flick_budget = 600;
}

/// `0042ca00` slerp(identity, q, t) along the shortest arc, with linear weights when the arc is under
/// acos(0.99999) (0x3f7fff58).
fn slerp_from_identity(q: Quat, t: f64) -> Quat {
    let sign = if q.w < 0.0 { -1.0 } else { 1.0 };
    let c = sign * q.w;
    let (k0, k1) = if c < 0.999_989_986_4 {
        let sine = (1.0 - c * c).sqrt();
        let angle = sine.atan2(c);
        ((angle * (1.0 - t)).sin() / sine, (angle * t).sin() / sine)
    } else {
        (1.0 - t, t)
    };
    let k1 = k1 * sign;
    Quat::new(k1 * q.x, k1 * q.y, k1 * q.z, k1 * q.w + k0)
}

/// Kinematic auto-level: rotate every body toward the target by `t` of the remaining angle (orientation
/// post-multiplied; positions rotated about the chassis as a world rotation: `0085c530`).
fn slerp_bodies(sys: &mut System, target: Quat, t: f64) {
    let rel = (sys.bodies[0].q.conj() * target).normalize();
    let e = slerp_from_identity(rel, t).normalize();
    let p0 = sys.bodies[0].x;
    for b in &mut sys.bodies {
        b.q = b.q * e;
        b.x = p0 + e.rotate(b.x - p0);
    }
}

/// One air step. `stick_inhibit` = the stunt modifier (sim+0x3c); `takeoff` = first airborne step (sim+0x8d9c).
pub fn step(sys: &mut System, st: &mut AirState, p: &AirParams, s: Sticks, stick_inhibit: bool, takeoff: bool, dt: f64) {
    st.air_time += dt;
    // course correction, same dv on every body (the root's inverse mass, S+120)
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

    // gesture moves recorded on the ground (`00440da0`)
    let g = &mut st.gestures;
    let sel_pull = g.pull_armed && !g.flick_armed;
    let sel_flick = !g.pull_armed && g.flick_armed;
    if g.pull_ran && g.pull_budget > 0 {
        g.pull_budget -= 1;
    }
    if g.flick_ran && g.flick_budget > 0 {
        g.flick_budget -= 1;
    }
    if sel_pull && g.pull_budget > 0 {
        let t = g.pull_strength;
        g.pull_ran = true;
        w += axis_body * (t * -5.0 * D2R);
        let r = if s.ry > 0.9 { dt * 0.45 } else { dt * 0.1 };
        g.pull_strength = (t - r).max(0.3);
    } else {
        g.pull_armed = false;
        g.pull_ran = false;
    }
    if sel_flick && g.flick_budget > 0 {
        let (fx, fy, k) = (g.flick_x, g.flick_y, g.side);
        g.flick_ran = true;
        w += axis_body * (fy * -10.0 * D2R) + ROLL_AX * (fx * k * D2R) + Vec3::Y * (fx * -0.5 * k * D2R);
        g.flick_x = (fx - dt * 0.1).max(0.15);
        g.flick_y = (fy - dt * 0.1).max(0.15);
    } else {
        g.flick_armed = false;
        g.flick_ran = false;
    }
    let (pull_ran, flick_ran) = (g.pull_ran, g.flick_ran);

    // trick hold: a latched cone of the first set with the steering stick past -0.9, or of the second past 0.9
    let left = st.latches[..3].iter().any(|&l| l);
    let right = st.latches[3..].iter().any(|&l| l);
    let lock = (left && s.lx < -0.9) || (right && s.lx > 0.9);
    let left_used = s.lx.abs() > 0.05 || s.ly.abs() > 0.05;
    let right_used = s.rx.abs() > 0.05 || s.ry.abs() > 0.05;
    let active = (left_used || right_used) && !stick_inhibit && !lock && !pull_ran;

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
        rate[i] = flick(now[i], st.prev_sq[i], dt).unwrap_or(0.0);
        rate[i] *= p.velocity_curves[i].eval((rate[i].abs() / 100.0).min(1.0));
    }
    st.prev_sq = now;
    let [r_lx, _r_ly, r_rx, r_ry] = rate;
    let c = diag(s.rx, s.ry);
    let d = flick(c, st.diag, dt).unwrap_or(0.0);
    st.diag = c;
    st.diag_rate = d;

    // take-off orientation re-aimed along the flight path while it is shallow (009d7894 = 0.99)
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
    } else if lock {
        slerp_bodies(sys, st.target, 0.012);
    } else if st.stick_time > 0.25 && !flick_ran && !pull_ran {
        slerp_bodies(sys, st.target, 0.008);
    }

    // clamp and damping on every path; w.x by damping[1], w.y by [2], w.z by [0]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remap_clamps_and_maps() {
        assert_eq!(remap_clamped(10.0, 40.0, 0.0, 0.005, 0.75), 0.005);
        assert_eq!(remap_clamped(10.0, 40.0, 100.0, 0.005, 0.75), 0.75);
        assert!((remap_clamped(100.0, 250.0, 175.0, 0.25, 1.0) - 0.625).abs() < 1e-12);
        assert_eq!(remap_clamped(1.0, 1.0, 5.0, 0.3, 0.9), 0.3);
    }

    #[test]
    fn quarter_limit_quadrants() {
        use std::f64::consts::{FRAC_PI_2, PI};
        assert!((quarter_limit(0.0, 1.0, 2.0) - 1.0).abs() < 1e-12);
        assert!((quarter_limit(FRAC_PI_2, 1.0, 2.0) - 2.0).abs() < 1e-12);
        assert!((quarter_limit(PI, 1.0, 2.0) - 1.0).abs() < 1e-12);
        assert!((quarter_limit(-FRAC_PI_2, 1.0, 2.0) - 2.0).abs() < 1e-12);
        assert!((quarter_limit(-PI + 1e-9, 1.0, 2.0) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cone_latches_only_inside_the_pose() {
        let target = Quat::from_axis_angle(Vec3::Y, 0.7);
        for (i, c) in CONES.iter().enumerate() {
            assert!(within_cone(c, target * c.q, target), "cone {i} at its own pose");
        }
        assert!(!CONES.iter().any(|c| within_cone(c, target, target)), "level bike latches nothing");
    }

    #[test]
    fn slerp_from_identity_takes_fraction_of_angle() {
        let q = Quat::from_axis_angle(Vec3::X, 1.0);
        let e = slerp_from_identity(q, 0.25).normalize();
        assert!((e.w.acos() * 2.0 - 0.25).abs() < 1e-9);
        let n = Quat::new(-q.x, -q.y, -q.z, -q.w);
        let e2 = slerp_from_identity(n, 0.25).normalize();
        assert!((e2.dot(e) - 1.0).abs() < 1e-9, "shortest arc");
    }

    #[test]
    fn ground_flick_arms_the_flick_move() {
        let mut st = AirState::default();
        let q = Quat::IDENTITY;
        let dt = 1.0 / 60.0;
        // rider stick flicked right, then pulled back, leaving a lip with the stunt modifier
        stunt_latches(&mut st, q, 0.0, 0.0, false, true, true, dt);
        stunt_latches(&mut st, q, 1.0, 0.0, false, true, true, dt);
        assert_eq!(st.gestures.side, -1.0);
        assert!(st.gestures.flick_x >= 0.25);
        stunt_latches(&mut st, q, 1.0, -0.5, false, true, true, dt);
        assert!(st.gestures.flick_armed && !st.gestures.pull_armed);
        // without the lip the record is cleared on the ground
        stunt_latches(&mut st, q, 0.0, 0.0, false, false, true, dt);
        assert_eq!(st.gestures, Gestures::default());
    }

    #[test]
    fn held_back_arms_the_pull_move() {
        let mut st = AirState::default();
        let q = Quat::IDENTITY;
        stunt_latches(&mut st, q, 0.0, -1.0, false, true, true, 1.0 / 60.0);
        assert!(st.gestures.pull_armed);
        // past 0.9 straight from rest counts as rate 0: the weakest pull
        assert!((st.gestures.pull_strength - 0.005).abs() < 1e-12);
        stunt_latches(&mut st, q, 0.0, -0.5, false, true, true, 1.0 / 60.0);
        assert!(!st.gestures.pull_armed && st.gestures.pull_strength == 0.0);
    }
}
