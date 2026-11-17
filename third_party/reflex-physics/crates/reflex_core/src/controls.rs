//! Slot 9 (`0044c170`): controls, from the per-frame input block to the ctl outputs the later stages read
//! (spec 5; `docs\fields\lean.md`, `engine.md`, `rider.md`, `susp-tires.md`). Runs at the start of every substep.
//! Checked against OpenReflex `bike_controls.c` (0044c170 the controls, 0044e0d0 rider off).
//!
//! Filter states are `{value, tau, alpha}` first-order filters at sim+0x5c34 (front brake), 0x5c48 (rear
//! brake), 0x5c5c (steering), 0x5c70 (lean), 0x5c84 (steering counter-steer offset), 0x5c98 (rider fore/aft for grip),
//! 0x5cac (torque feedback).
//!
//! Not ported: the throttle cut by the timer sim+0x68 (it only runs while the flags sim+0x5c / 0x60 are set, which
//! free ride never sets), the manual gearbox pulses, the pose override steering (sim+0x2de0, handled by `Hold`).

use crate::bike::{CHASSIS, FRONT};
use crate::curve::Curve;
use crate::math::Vec3;
use crate::multibody::System;
use crate::params::Params;
use crate::sim::{Controls, Ground, Inputs};
use crate::tyre::Tyre;
use crate::DT;

use std::f64::consts::{FRAC_PI_2, TAU};

/// First-order filter `y += (target - y) * min(dt, tau) / tau` (`0042d6b0`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Filter {
    pub value: f64,
    pub tau: f64,
}

impl Filter {
    pub fn new(tau: f64) -> Self {
        Filter { value: 0.0, tau }
    }
    pub fn step(&mut self, target: f64, dt: f64) -> f64 {
        self.value += (target - self.value) * dt.min(self.tau) / self.tau;
        self.value
    }
}

/// Slot 9 state that persists between substeps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControlState {
    pub front_brake: Filter,
    pub rear_brake: Filter,
    pub steer: Filter,
    pub lean: Filter,
    pub steer_angle: Filter,
    pub rider_y: Filter,
    pub torque_fb: Filter,
    /// Jump-lip helper flags: takeoff (sim+0x8c24) and slide assist (sim+0x8c20, cuts side grip this step).
    pub lip_takeoff: bool,
    pub slide_assist: bool,
    /// Front brake lever as the input block holds it (sim+0x24): latched once per frame, then eased by the steer
    /// stick in place after every substep, so the easing compounds over a frame's substeps (`0044c170`).
    pub front_brake_in: f64,
    /// Phase of the rolling-backwards steering wobble (sim+0x8db0) and of the rough-ground head shake (sim+0x8db4).
    pub back_phase: f64,
    pub shake_phase: f64,
    /// Lean hold (sim+0x8db8): set when rolling backwards faster than 30 ft/s, cleared once the bike is upright
    /// again; while set the ground lean is not driven (see `lean::ground_step`, which reads it as `LeanState::hold`).
    pub lean_hold: bool,
}

impl ControlState {
    pub fn new(p: &Params) -> Self {
        ControlState {
            front_brake: Filter::new(p.brake_time_constant),
            rear_brake: Filter::new(p.brake_time_constant),
            steer: Filter::new(p.steering_tau),
            lean: Filter::new(p.lean_tau),
            // tau 0.05 (009d3bfc), 0.2, 0.25 (`0043bb..` filter setup)
            steer_angle: Filter::new(0.05),
            rider_y: Filter::new(0.2),
            torque_fb: Filter::new(0.25),
            lip_takeoff: false,
            slide_assist: false,
            front_brake_in: 0.0,
            back_phase: 0.0,
            shake_phase: 0.0,
            lean_hold: false,
        }
    }

    /// Once per rendered frame, when the input block is set (`0044bff0` / `0044bfb0`).
    pub fn latch(&mut self, inputs: &Inputs) {
        self.front_brake_in = inputs.front_brake;
    }
}

/// Everything slot 9 reads besides its own state.
pub struct ControlInputs<'a> {
    pub inputs: &'a Inputs,
    pub sys: &'a System,
    pub tyres: &'a [Tyre; 2],
    pub airborne: bool,
    pub crashed: bool,
    /// Front / rear wheel spin relative to the chassis (slot 17 of the previous step).
    pub spin_rel: [f64; 2],
    pub up_filt: Vec3,
    pub ground: &'a dyn Ground,
}

fn eval(c: &Curve, x: f64) -> f64 {
    c.eval(x)
}

/// `00430810` Lerp_Clamped: a + (b - a) * clamp(t, 0, 1).
fn lerp(t: f64, a: f64, b: f64) -> f64 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

/// Moves `f` towards `target` by at most `max_step` (`0044c170`'s inlined step).
fn step_towards(f: f64, target: f64, max_step: f64) -> f64 {
    let d = target - f;
    let sg = if d >= 0.0 { 1.0 } else { -1.0 };
    f + sg * d.abs().min(max_step)
}

/// Brake torque against a wheel's spin relative to the chassis (`0044c170`): spin > 0 gives -curve(spin),
/// otherwise +curve(-spin) (so zero spin pushes the positive way).
fn brake_torque(curve: &Curve, max: f64, lever: f64, spin: f64) -> f64 {
    if spin > 0.0 {
        -(eval(curve, spin) * (max * lever))
    } else {
        eval(curve, -spin) * (max * lever)
    }
}

/// Target of the steering offset filter (sim+0x5c84) when rolling forwards (`0044c170`): the front wheel turns
/// into a slide by 0.75 x the chassis slide angle (signed angle of the chassis-frame horizontal velocity from
/// forward, faded in from 5 to 20 ft/s), capped at 5 deg (20 deg airborne) x (1 - |steer stick|). Gives 0 when
/// not moving forwards (the backward wobble and the head shake are in [`step`]). `v_local` is the chassis
/// velocity in the chassis frame.
pub fn counter_steer(v_local: Vec3, steer: f64, airborne: bool) -> f64 {
    if v_local.z <= 0.0 {
        return 0.0;
    }
    let slide = v_local.x.atan2(v_local.z);
    let fade = lerp((v_local.length() - 5.0) / 15.0, 0.0, 0.75);
    let cap = if airborne { 20f64 } else { 5f64 }.to_radians() * (1.0 - steer.abs());
    (fade * slide).clamp(-cap, cap)
}

/// Steering wobble while rolling backwards with a wheel down (`0044c170`): amplitude (|v_z| - 15) / 30 in 0..1,
/// phase advancing 20 rad/s. Returns the offset target from the phase before the advance.
fn backward_wobble(phase: &mut f64, v_z: f64, dt: f64) -> f64 {
    let amp = ((v_z.abs() - 15.0) / 30.0).clamp(0.0, 1.0);
    // bike_cos_poly(x) is cos(pi - x) = -cos(x); -cos(phase + pi/2) = sin(phase)
    let w = -(*phase + FRAC_PI_2).cos() * amp;
    *phase = (dt * 20.0 + *phase) % TAU;
    w
}

/// Rough-ground head shake (`0044c170`): front wheel down on ground with drag index > 0.25. Fades in to 10 ft/s
/// forward and out by 50; 10 rad/s, rising to 20 as it fades out. Replaces the counter-steer target.
fn head_shake(phase: &mut f64, drag_index: f64, v_z: f64, dt: f64) -> f64 {
    let (k, fast) = if v_z < 10.0 { (v_z / 10.0, false) } else { (1.0 - v_z / 50.0, true) };
    let k = k.clamp(0.0, 1.0);
    let w = -(*phase + FRAC_PI_2).cos();
    let corr = w * ((drag_index - 0.25) * 0.225 * k);
    let freq = if fast { lerp(1.0 - k, 10.0, 20.0) } else { 10.0 };
    *phase = if k > 0.0 { (freq * dt + *phase) % TAU } else { 0.0 };
    corr
}

/// Slot 9 while the rider is off the bike (`0044e0d0`): no inputs, drag and grip from the speed curves alone,
/// torque feedback towards its maximum. The steering angle, the lean command, the lean input and the brake /
/// steering / lean filters stay as they were.
fn rider_off(st: &mut ControlState, ctl: &mut Controls, p: &Params, ci: &ControlInputs) -> [(f64, f64); 2] {
    let b0 = &ci.sys.bodies[CHASSIS];
    let v_fwd = b0.q.inv_rotate(b0.v).z;
    let speed = b0.v.length();
    ctl.stick_lx = 0.0;
    ctl.ctl_74 = 0.0;
    let c = if ci.airborne { &p.air_drag_curve } else { &p.ground_drag_curve };
    ctl.drag = if speed > 0.0 { b0.v * (-eval(c, v_fwd) / speed) } else { Vec3::ZERO };
    ctl.throttle = 0.0;
    ctl.reverse = 0.0;
    ctl.torque_fb = st.torque_fb.step(p.max_torque_feedback, DT);
    ctl.brake_rear = 0.0;
    ctl.brake_front = 0.0;
    let mut mods = [(0.0, 0.0); 2];
    for (i, tp) in p.tyres.iter().enumerate() {
        mods[i] = (eval(&tp.static_mu_side_curve, v_fwd), eval(&tp.static_mu_roll_curve, v_fwd));
    }
    mods
}

/// One substep of slot 9. `ctl.lean_input` must hold the value the frame started with (the steer stick) or the
/// previous substep's edit: the lean chain edits it in place, so it compounds over a frame's substeps.
/// Returns the grip modifiers (side, roll) for the front and rear tyre (spec 5.9).
pub fn step(st: &mut ControlState, ctl: &mut Controls, clutch_timer: &mut f64, p: &Params, ci: &ControlInputs) -> [(f64, f64); 2] {
    if ci.crashed {
        return rider_off(st, ctl, p, ci);
    }
    let inp = ci.inputs;
    let b0 = &ci.sys.bodies[CHASSIS];
    let v_local = b0.q.inv_rotate(b0.v);
    let v_fwd = v_local.z;
    let speed = b0.v.length();
    let s = inp.steer_x;
    let (l, ry) = (inp.rider_x, inp.rider_y);

    // stick copies (ctl+0x68 LX, ctl+0x74 RY)
    ctl.stick_lx = s;
    ctl.ctl_74 = ry;

    // drag (spec 5.8): against the velocity, scaled by a curve of the forward speed
    let c = if ci.airborne { &p.air_drag_curve } else { &p.ground_drag_curve };
    ctl.drag = if speed > 0.0 { b0.v * (-eval(c, v_fwd) / speed) } else { Vec3::ZERO };

    // rider fore/aft for the grip curves: dead zone 0.02 (kept only above it), then filtered
    let ry_dz = if ry.abs() > 0.02 { ry } else { 0.0 };
    let y_f = st.rider_y.step(ry_dz, DT);

    // throttle, reverse (spec 5.2); the reverse button is 0 / 1 in the input block (`00433380`)
    let mut throttle = inp.throttle;
    if inp.rear_brake > 0.0 || inp.brake_button > 0.0 {
        throttle = 0.0;
    }
    ctl.reverse = 0.0;
    if inp.brake_button > 0.0 && v_fwd < 2.0 && inp.rear_brake <= 0.01 {
        throttle = 0.0;
        ctl.reverse = inp.brake_button;
    }
    ctl.throttle = throttle;

    // clutch / engine coupling mode (ctl+0x84) and its timer (sim+0x8ab8)
    if inp.clutch {
        ctl.clutch_mode = 2;
        *clutch_timer = p.clutch_reset_time * ((speed - 5.0) / 20.0).min(1.0);
    } else if *clutch_timer >= 0.0 && ci.tyres[1].contact {
        ctl.clutch_mode = 1;
        *clutch_timer -= DT;
    } else {
        ctl.clutch_mode = 0;
        *clutch_timer = -0.1; // 009cd9dc
    }

    // torque feedback (spec 5.7)
    let k_target = if ci.airborne {
        p.max_torque_feedback
    } else {
        let (base, floor) = if ry < 0.0 {
            (p.min_torque_feedback - (p.max_torque_feedback - p.min_torque_feedback) * ry, 0.5)
        } else {
            (p.min_torque_feedback, 0.1)
        };
        let rt = &ci.tyres[1];
        let mut slip_term = 0.0;
        if !rt.side_grip && rt.contact && rt.slip.x.abs() > 1.0 {
            slip_term = ((rt.slip.x.abs() - 1.0) / 25.0).clamp(0.0, 1.0);
        }
        let mut k = base * (1.0 - s.abs().max(slip_term)).max(floor);
        if rt.contact {
            k *= (1.0 - (rt.slip.z.abs() / 40.0).min(1.0)).clamp(0.1, 1.0);
        }
        k
    };
    ctl.torque_fb = st.torque_fb.step(k_target, DT);

    // brake levers: filtered on application, instant on release (the filter is stepped, then cut back to the
    // lever). The front lever reads 0 in the air.
    let front_in = if ci.airborne { 0.0 } else { st.front_brake_in };
    let lever = |f: &mut Filter, target: f64| {
        let v = f.step(target, DT);
        if v > target {
            f.value = target;
        }
        f.value
    };
    let front_lever = lever(&mut st.front_brake, front_in);
    let rear_lever = lever(&mut st.rear_brake, inp.rear_brake);
    let [wf, wr] = ci.spin_rel;
    ctl.brake_front = brake_torque(&p.front_brake_curve, p.front_brake_max, front_lever, wf) * (ry + 1.0) * (1.0 - s.abs());
    ctl.brake_rear = brake_torque(&p.rear_brake_curve, p.rear_brake_max, rear_lever, wr);

    // steering (spec 5.3): the target s|s| is limited to within 100 dt of the current filter value, then lagged;
    // speed curve; rider X term.
    let target = step_towards(st.steer.value, s * s.abs(), DT * 100.0);
    let filtered = st.steer.step(target, DT);
    let curve = eval(&p.max_steer_curve, v_fwd);
    let mut angle = filtered * curve;
    if l * filtered > -0.05 {
        angle += curve * 0.2 * l;
    } else {
        angle -= l.abs() * (angle * 0.2);
    }

    // steering offset target (filter sim+0x5c84): counter-steer rolling forwards, a wobble rolling backwards, the
    // head shake on rough ground; rate-limited 50 / s, lagged, added, clamp 30 deg (`0044c170`).
    let roll_up = || b0.q.rotate(Vec3::X).y.abs() < 0.5;
    let mut straight_up = false;
    let ft = &ci.tyres[0];
    let mut corr;
    if v_fwd > 0.0 {
        corr = counter_steer(v_local, s, ci.airborne);
        st.back_phase = 0.0;
        if st.lean_hold && roll_up() {
            straight_up = true;
        }
    } else if !ft.contact && !ci.tyres[1].contact {
        corr = 0.0;
        st.back_phase = 0.0;
    } else {
        corr = backward_wobble(&mut st.back_phase, v_fwd, DT);
        let d = -v_fwd - v_local.x.abs();
        if d > 30.0 {
            st.lean_hold = true;
        } else if st.lean_hold && d < 15.0 && roll_up() {
            straight_up = true;
        }
    }
    if ft.contact && ft.drag_index > 0.25 {
        corr = head_shake(&mut st.shake_phase, ft.drag_index, v_fwd, DT);
    } else {
        st.shake_phase = 0.0;
    }
    let target = step_towards(st.steer_angle.value, corr, DT * 50.0);
    let offset = st.steer_angle.step(target, DT);
    ctl.steer_angle = (angle + offset).clamp(-0.523_598_73, 0.523_598_73);

    // the front brake lever in the input block eases off with the steer stick (in place, see `front_brake_in`)
    st.front_brake_in = lerp(s.abs(), st.front_brake_in, st.front_brake_in * 0.0);

    // lean input chain (spec 5.4), in place
    let mut u = ctl.lean_input;
    if ft.contact && !ft.side_grip && ft.slip.x.abs() > 1.0 {
        let x = ft.slip.x;
        let w = -x.signum() * ((x.abs() - 1.0) / 10.0).clamp(0.0, 2.0);
        u = u * (1.0 - w.abs()) + w;
    }
    let push = if u.abs() > 0.05 { l * u > 0.0 } else { l * filtered > -0.05 };
    if push {
        u += l * 0.55;
    }
    u = lerp(s.abs() * ry.max(0.0), u, u * 1.5);
    // jump-lip helper: rising faster than 15 ft/s, probe a horizontal segment from the front wheel, raised
    // 1..7 ft, 25 ft along the velocity's x / z; no terrain means leaving a lip
    st.lip_takeoff = false;
    st.slide_assist = false;
    if b0.v.y > 15.0 {
        let d = if speed > 0.0 { b0.v / speed } else { b0.v };
        let h = (1.0 + 6.0 * (b0.v.y - 15.0) / 25.0).clamp(1.0, 7.0);
        let front = ci.sys.bodies[FRONT].x;
        let a = Vec3 { x: front.x, y: front.y + h, z: front.z };
        let b = Vec3 { x: front.x + d.x * 25.0, y: front.y + h, z: front.z + d.z * 25.0 };
        if !ci.ground.segment_hits(a, b) {
            st.lip_takeoff = true;
            u += l * 1.5;
            st.slide_assist = l.abs() > 0.6 && s.abs() > 0.6;
        }
    }
    ctl.lean_input = u;

    // lean target and lag (spec 5.5): in the air the filter follows the chassis roll against world up; on the
    // ground the lean curve, unless the lean is held (then the ground step sets it; it clears once upright)
    if ci.airborne {
        let measured = b0.q.rotate(Vec3::X).y.clamp(-1.0, 1.0).asin();
        ctl.lean_cmd = st.lean.step(measured, DT);
    } else if !st.lean_hold {
        ctl.lean_cmd = st.lean.step(-(eval(&p.theta_lean_max_curve, v_fwd) * u), DT);
    } else if straight_up {
        st.lean_hold = false;
    }

    // grip modifiers (spec 5.9)
    let signed_l = if ctl.lean_cmd < 0.0 { l } else { -l };
    let mut mods = [(0.0, 0.0); 2];
    for (i, tp) in p.tyres.iter().enumerate() {
        let roll = eval(&tp.static_mu_roll_curve, v_fwd) * eval(&tp.rider_shift_mu_roll_curve, y_f);
        let side = eval(&tp.static_mu_side_curve, v_fwd)
            * eval(&tp.lean_angle_mu_side_curve, signed_l)
            * eval(&tp.rider_shift_mu_side_curve, y_f);
        // slide assist after a lip: rear side grip x0.4 (00935860), front x0.8 (009cc8a8)
        let cut = if st.slide_assist { [0.8, 0.4][i] } else { 1.0 };
        mods[i] = (side * cut, roll);
    }
    mods
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_towards_limits() {
        assert_eq!(step_towards(0.0, 1.0, 0.25), 0.25);
        assert_eq!(step_towards(0.0, -1.0, 0.25), -0.25);
        assert_eq!(step_towards(0.5, 0.6, 0.25), 0.6);
    }

    #[test]
    fn counter_steer_caps() {
        // fast sideways slide: capped at 5 deg on the ground, 20 deg in the air
        let v = v3(10.0, 0.0, 30.0);
        assert!((counter_steer(v, 0.0, false) - 5f64.to_radians()).abs() < 1e-12);
        assert!((counter_steer(v, 0.0, true) - 0.75 * 10f64.atan2(30.0)).abs() < 1e-12);
        assert_eq!(counter_steer(v3(1.0, 0.0, -5.0), 0.0, false), 0.0);
        // below 5 ft/s there is none
        assert_eq!(counter_steer(v3(1.0, 0.0, 3.0), 0.0, false), 0.0);
    }

    #[test]
    fn wobbles() {
        let mut ph = 0.0;
        // below 15 ft/s backwards: no amplitude, but the phase runs
        assert_eq!(backward_wobble(&mut ph, -10.0, DT), 0.0);
        assert!((ph - 20.0 * DT).abs() < 1e-12);
        let mut ph = FRAC_PI_2;
        assert!((backward_wobble(&mut ph, -45.0, DT) - 1.0).abs() < 1e-12);
        let mut ph = FRAC_PI_2;
        // full shake at 10 ft/s: (0.65 - 0.25) * 0.225
        assert!((head_shake(&mut ph, 0.65, 10.0, DT) - 0.4 * 0.225 * 0.8).abs() < 1e-12);
        let mut ph = 1.0;
        assert_eq!(head_shake(&mut ph, 0.65, 0.0, DT), 0.0);
        assert_eq!(ph, 0.0);
    }

    #[test]
    fn brake_torque_sign() {
        let c = Curve::new(&[(0.0, 1.0)], 0.0, 0.0);
        assert_eq!(brake_torque(&c, 100.0, 0.5, 2.0), -50.0);
        assert_eq!(brake_torque(&c, 100.0, 0.5, -2.0), 50.0);
        assert_eq!(brake_torque(&c, 100.0, 0.5, 0.0), 50.0);
    }

    use crate::math::v3;
}
