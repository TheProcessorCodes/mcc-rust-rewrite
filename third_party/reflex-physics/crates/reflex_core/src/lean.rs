//! Ground lean (spec 5.5, 8.1): the filtered up vector (`0043e9f0`) and `00441300`, the grounded lean
//! constraint with its chassis velocity fix-ups and wheelie / stoppie pitch-rate limiters. Decoded from the
//! asm (local notes `local\notes\00441300.md`); the spec's 8.1 omits parts E and F.
//!
//! Order: A geometry (contact points, contact line), B gate, C measured lean, D lean hold or the kinematic
//! rotation, E chassis velocity fix-ups (skipped when the gate fails), F wheelie / stoppie limiters (always).

use crate::bike::{CHASSIS, FRONT, REAR};
use crate::controls::Filter;
use crate::math::{Quat, Vec3};
use crate::multibody::System;
use crate::tyre::Tyre;

const DEG: f64 = std::f64::consts::PI / 180.0;

/// Steps the up-vector filter (each axis separately, tau = `VehicleSim_UpVectorTimeConstant`) toward world up,
/// or, grounded with |steer| >= 0.02, toward the contact normal under the touching tyre(s).
pub fn step_up_vector(up: &mut Vec3, tyres: &[Tyre; 2], airborne: bool, steer: f64, tau: f64, dt: f64) {
    let target = if !airborne && steer.abs() >= 0.02 {
        match (tyres[0].contact, tyres[1].contact) {
            (true, true) => (tyres[0].normal + tyres[1].normal) * 0.5,
            (true, false) => tyres[0].normal,
            (false, true) => tyres[1].normal,
            (false, false) => Vec3::Y,
        }
    } else {
        Vec3::Y
    };
    let a = dt.min(tau) / tau;
    *up += (target - *up) * a;
}

/// State `00441300` keeps between substeps.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LeanState {
    /// Lean hold (sim+0x8db8): no rotation, the command follows the measured lean.
    pub hold: bool,
    /// Wheelie / stoppie timers (sim+0x8da8 / 0x8dac) and their "held > 0.25 s" latches (sim+0x2eb8 / 0x2eb4).
    pub wheelie_timer: f64,
    pub stoppie_timer: f64,
    pub wheelie_latch: bool,
    pub stoppie_latch: bool,
    /// Removed roll rate (sim+0x8b60), body frame.
    pub removed_roll: Vec3,
}

/// Everything `00441300` reads besides the bodies and its own state.
pub struct LeanInputs<'a> {
    pub tyres: &'a [Tyre; 2],
    pub up_filt: Vec3,
    /// Commanded lean (ctl+0x64).
    pub lean_cmd: f64,
    /// |left stick X| copy (ctl+0x68).
    pub stick_lx: f64,
    /// Rider stick Y (sim+0x38).
    pub rider_y: f64,
    /// sim+0x2ec0 is non-zero (an input / assist vector, not traced).
    pub input_active: bool,
    /// Clutch engagement e = clamp((ClutchResetTime - timer) / ClutchResetTime, 0, 1).
    pub clutch_engagement: f64,
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    if t <= 0.0 {
        a
    } else if t >= 1.0 {
        b
    } else {
        a + (b - a) * t
    }
}

/// One call of `00441300` (grounded, not crashed). Writes `lean_cmd` and the lean filter value when the lean is
/// held or the gate fails.
pub fn ground_step(sys: &mut System, st: &mut LeanState, li: &LeanInputs, lean_cmd: &mut f64, lean_filter: &mut Filter, dt: f64) {
    st.removed_roll = Vec3::ZERO;
    let (t_front, t_rear) = (&li.tyres[0], &li.tyres[1]);

    // A geometry
    let q0 = sys.bodies[CHASSIS].q;
    let x_w = q0.rotate(Vec3::X);
    let up_w = q0.rotate(Vec3::Y);
    let p_front = sys.bodies[FRONT].x + q0.rotate(t_front.arm_chassis);
    let p_rear = sys.bodies[REAR].x + q0.rotate(t_rear.arm_chassis);
    let line = p_front - p_rear;
    let dir = if line.length_squared() > 0.0 { line.normalize() } else { Vec3::ZERO };
    let a_b = q0.inv_rotate(dir);
    let v0 = sys.bodies[CHASSIS].v;
    let c = if v0.length_squared() > 0.0 { v0.normalize().dot(dir) } else { 0.0 };
    let slope = dir.y;

    // B gate, C measured lean
    let gate = up_w.y >= 0.0 && slope.abs() <= 0.95;
    let phi = li.up_filt.dot(x_w).clamp(-1.0, 1.0).asin();

    if !gate || st.hold {
        *lean_cmd = phi;
        lean_filter.value = phi;
    } else {
        // D2 kinematic rotation by the full error about the contact line, pivot = rear contact point. Each body
        // rotates in its own frame (axis R_i a_b), its orientation right-multiplied by the rotation.
        let ax = if a_b.length_squared() > 0.0 { a_b.normalize() } else { Vec3::Z };
        let rq = Quat::from_axis_angle(ax, li.lean_cmd - phi);
        for b in &mut sys.bodies {
            let n = b.q.inv_rotate(b.x - p_rear);
            b.x = p_rear + b.q.rotate(rq.rotate(n));
            b.q = b.q * rq;
        }
        let w0 = sys.bodies[CHASSIS].w;
        let d = a_b.dot(w0);
        sys.bodies[CHASSIS].w = w0 - a_b * d;
        st.removed_roll = a_b * d;
    }
    if gate {
        velocity_fixups(sys, st, li, c);
    }
    pitch_limiters(sys, st, li, dir, dt);
}

/// Part E: chassis velocity fix-ups, using the post-rotation orientation.
fn velocity_fixups(sys: &mut System, st: &LeanState, li: &LeanInputs, c: f64) {
    let (front, rear) = (&li.tyres[0], &li.tyres[1]);
    let b0 = &sys.bodies[CHASSIS];
    let x_w = b0.q.rotate(Vec3::X);
    let t = (b0.v.length() / 20.0).min(1.0);
    if rear.side_grip && front.side_grip {
        // both tyres gripping: no chassis lateral velocity at all
        let v = b0.v;
        sys.bodies[CHASSIS].v = v - x_w * v.dot(x_w);
        return;
    }
    if sys.bodies[REAR].w.x < -1.0 {
        return;
    }
    let blend = |r_b: f64, r_a: f64| if c > 0.0 { (1.0 - c) * r_b + r_a * c } else { r_b };
    let world = |r_b: f64, r_a: f64| {
        let (r_b, r_a) = if st.stoppie_latch && !li.input_active { (0.5, 0.5) } else { (r_b, r_a) };
        (blend(r_b, r_a), Vec3::Y)
    };
    let no_rear = || {
        let s = li.stick_lx.abs();
        world(lerp(0.85, 0.95, s), lerp(lerp(0.85, 0.9, t), 0.95, s))
    };
    let (f, n) = if front.side_grip {
        if !rear.contact {
            no_rear()
        } else {
            world(lerp(1.0, 0.85, t), lerp(1.0, 0.99, t))
        }
    } else if !rear.contact {
        no_rear()
    } else if !rear.side_grip {
        let r1 = lerp(1.0, 0.9, t);
        (if c > 0.0 { (1.0 - c) * r1 + lerp(1.0, 0.99, t) * c } else { r1 }, front.normal)
    } else {
        let k = if front.contact { 0.99 } else { 0.98 };
        (if c > 0.0 { (1.0 - c) * 0.85 + k * c } else { 0.85 }, rear.normal)
    };
    // keep the fraction f of the world angular velocity along n
    let b0 = &mut sys.bodies[CHASSIS];
    let ww = b0.omega();
    let ww = ww - n * (ww.dot(n) * (1.0 - f));
    b0.w = b0.q.inv_rotate(ww);
}

/// Part F: wheelie (rear only on the ground) and stoppie (front only) pitch-rate limiters and their timers.
fn pitch_limiters(sys: &mut System, st: &mut LeanState, li: &LeanInputs, dir: Vec3, dt: f64) {
    let (front, rear) = (&li.tyres[0], &li.tyres[1]);
    let v = sys.bodies[CHASSIS].v.length();
    let t2 = (v / 60.0).min(1.0);
    let a = ((st.wheelie_timer - 1.0) / 2.0).clamp(0.0, 1.0);
    let s1 = lerp(0.6, 0.9, t2);
    let s2 = lerp(0.85, 1.0, a);
    let s3 = lerp(0.95, 1.0, a);
    let s4 = lerp(s1, 1.0, (-li.rider_y).max(0.0));
    let s5 = lerp(s4, 1.0, a);
    let s6 = lerp(1.0, s5, li.clutch_engagement);
    st.wheelie_latch = false;
    let pitch = dir.y.clamp(-1.0, 1.0).asin();

    if rear.contact && !front.contact {
        let w = &mut sys.bodies[CHASSIS].w;
        if w.x < 0.0 {
            let g = ((pitch - 25.0 * DEG).abs() / (20.0 * DEG)).min(1.0);
            let mut r = lerp(s2, s6, g);
            if pitch >= 45.0 * DEG {
                r = lerp(s3, r, ((50.0 * DEG - pitch) / (5.0 * DEG)).max(0.0));
            }
            w.x = (w.x * r).max(-1.5);
        }
        st.wheelie_timer += dt;
        if st.wheelie_timer > 0.25 {
            st.wheelie_latch = true;
        }
    } else {
        st.wheelie_timer = 0.0;
    }

    let b = ((st.stoppie_timer - 1.0) / 3.0).clamp(0.0, 1.0);
    let nb = 1.0 - b;
    let s = li.stick_lx.abs();
    let u2 = lerp(lerp(1.0, 0.8, nb), 1.0, s);
    let u4 = lerp(lerp(1.0, 0.75, nb), 1.0, s);
    st.stoppie_latch = false;
    if li.input_active {
        st.stoppie_timer = 0.0;
    }
    if front.contact && !rear.contact {
        let q = -pitch;
        let w = &mut sys.bodies[CHASSIS].w;
        if w.x > 0.0 {
            let g = ((q - 30.0 * DEG).abs() / (25.0 * DEG)).min(1.0);
            let mut r = lerp(u2, 1.0, g);
            if q >= 37.5 * DEG {
                r = lerp(u4, r, ((40.0 * DEG - q) / (2.5 * DEG)).max(0.0));
            }
            w.x *= r;
        }
        st.stoppie_timer += dt;
        if st.stoppie_timer > 0.25 {
            st.stoppie_latch = true;
        }
    } else {
        st.stoppie_timer = 0.0;
    }
}
