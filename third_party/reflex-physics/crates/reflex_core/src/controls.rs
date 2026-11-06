//! Slot 9 (`0044c170`): controls, from the per-frame input block to the ctl outputs the later stages read
//! (spec 5; `docs\fields\lean.md`, `engine.md`, `rider.md`, `susp-tires.md`). Runs at the start of every substep.
//!
//! Filter states are `{value, tau, alpha}` first-order filters at sim+0x5c34 (front brake), 0x5c48 (rear
//! brake), 0x5c5c (steering), 0x5c70 (lean), 0x5c84 (steering counter-steer offset), 0x5c98 (rider fore/aft for grip),
//! 0x5cac (torque feedback).
//!
//! Not implemented yet: the throttle attenuator (normally 0), the rough-ground head shake, the lean-hold test.

use crate::bike::{CHASSIS, FRONT};
use crate::curve::Curve;
use crate::math::Vec3;
use crate::multibody::System;
use crate::params::Params;
use crate::sim::{Controls, Ground, Inputs};
use crate::tyre::Tyre;
use crate::DT;

/// First-order filter `y += (target - y) * min(dt, tau) / tau`.
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
}

impl ControlState {
    pub fn new(p: &Params) -> Self {
        ControlState {
            front_brake: Filter::new(p.brake_time_constant),
            rear_brake: Filter::new(p.brake_time_constant),
            steer: Filter::new(p.steering_tau),
            lean: Filter::new(p.lean_tau),
            steer_angle: Filter::new(0.05),
            rider_y: Filter::new(0.2),
            torque_fb: Filter::new(0.25),
            lip_takeoff: false,
            slide_assist: false,
        }
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

/// Target of the steering offset filter (sim+0x5c84, `0044cc74`..`0044cf8f`): the front wheel turns into a
/// slide by 0.75 x the chassis slide angle (signed angle of the chassis-frame horizontal velocity from forward,
/// faded in from 5 to 20 ft/s), capped at 5 deg (20 deg airborne) x (1 - |steer stick|). Moving backwards gives 0
/// (measured). On rough ground (front tyre down, drag index > 0.25) the original replaces this with an oscillating
/// head shake (phase sim+0x8db4); not implemented. `v_local` is the chassis velocity in the chassis frame.
pub fn counter_steer(v_local: Vec3, steer: f64, airborne: bool) -> f64 {
    if v_local.z <= 0.0 {
        return 0.0;
    }
    let slide = v_local.x.atan2(v_local.z);
    let fade = ((v_local.length() - 5.0) / 15.0).clamp(0.0, 1.0);
    let cap = if airborne { 20f64 } else { 5f64 }.to_radians() * (1.0 - steer.abs());
    (0.75 * fade * slide).clamp(-cap, cap)
}

/// One substep of slot 9. `ctl.lean_input` must hold the value the frame started with (the steer stick) or the
/// previous substep's edit: the lean chain edits it in place, so it compounds over a frame's substeps.
/// Returns the grip modifiers (side, roll) for the front and rear tyre (spec 5.9).
pub fn step(st: &mut ControlState, ctl: &mut Controls, clutch_timer: &mut f64, p: &Params, ci: &ControlInputs) -> [(f64, f64); 2] {
    let inp = ci.inputs;
    let b0 = &ci.sys.bodies[CHASSIS];
    let v_fwd = b0.q.inv_rotate(b0.v).z;
    let speed = b0.v.length();
    let s = inp.steer_x;
    let (l, ry) = (inp.rider_x, inp.rider_y);

    // throttle, reverse (spec 5.2)
    let mut throttle = inp.throttle;
    if inp.rear_brake > 0.0 || inp.brake_button > 0.0 {
        throttle = 0.0;
    }
    ctl.reverse = if inp.brake_button > 0.0 && v_fwd < 2.0 && inp.rear_brake <= 0.01 { 1.0 } else { 0.0 };
    ctl.throttle = throttle;

    // brake levers: smoothed on application, instant on release
    let lever = |f: &mut Filter, target: f64| {
        if target < f.value {
            f.value = target;
        } else {
            f.step(target, DT);
        }
        f.value
    };
    let front_lever = lever(&mut st.front_brake, inp.front_brake);
    let rear_lever = lever(&mut st.rear_brake, inp.rear_brake);
    let [wf, wr] = ci.spin_rel;
    let front = if ci.airborne {
        0.0
    } else {
        -wf.signum() * p.front_brake_curve.eval(wf.abs()) * p.front_brake_max * front_lever * (1.0 + ry) * (1.0 - s.abs())
    };
    ctl.brake_front = front;
    ctl.brake_rear = -wr.signum() * p.rear_brake_curve.eval(wr.abs()) * p.rear_brake_max * rear_lever;

    // clutch
    if inp.clutch {
        ctl.clutch_mode = 2;
        *clutch_timer = p.clutch_reset_time * ((speed - 5.0) / 20.0).min(1.0);
    } else if *clutch_timer > 0.0 && ci.tyres[1].contact {
        ctl.clutch_mode = 1;
        *clutch_timer -= DT;
    } else {
        ctl.clutch_mode = 0;
        *clutch_timer = -0.1;
    }

    // steering (spec 5.3, `0044c170` lines ~397-425): the target s|s| is limited to within 100 dt of the
    // current filter value, then lagged (so one substep moves at most 100 dt x dt/tau); speed curve; rider X
    // term; counter-steer offset (filter sim+0x5c84, see `counter_steer`); clamp 30 deg.
    let err = s * s.abs() - st.steer.value;
    let filtered = st.steer.step(st.steer.value + err.clamp(-100.0 * DT, 100.0 * DT), DT);
    let curve = eval(&p.max_steer_curve, v_fwd);
    let mut angle = filtered * curve;
    if l * filtered <= -0.05 {
        angle *= 1.0 - 0.2 * l.abs();
    } else {
        angle += 0.2 * curve * l;
    }
    let cs = counter_steer(b0.q.inv_rotate(b0.v), s, ci.airborne);
    let err = cs - st.steer_angle.value;
    let offset = st.steer_angle.step(st.steer_angle.value + err.clamp(-50.0 * DT, 50.0 * DT), DT);
    ctl.steer_angle = (angle + offset).clamp(-0.523_598_73, 0.523_598_73);

    // lean input chain (spec 5.4), in place
    let mut u = ctl.lean_input;
    let ft = &ci.tyres[0];
    if ft.contact && !ft.side_grip && ft.slip.x.abs() > 1.0 {
        let x = ft.slip.x;
        let w = -x.signum() * ((x.abs() - 1.0) / 10.0).clamp(0.0, 2.0);
        u = u * (1.0 - w.abs()) + w;
    }
    if (u.abs() > 0.05 && l * u > 0.0) || (u.abs() <= 0.05 && l * filtered > -0.05) {
        u += 0.55 * l;
    }
    u += (1.5 * u - u) * (s.abs() * ry.max(0.0)).clamp(0.0, 1.0);
    // jump-lip helper (`0044c170` ~0x0044d81b): rising faster than 15 ft/s, probe a horizontal segment from the
    // front wheel, raised 1..7 ft, 25 ft along the velocity's x / z; no terrain means leaving a lip
    st.lip_takeoff = false;
    st.slide_assist = false;
    if b0.v.y > 15.0 {
        let d = if speed > 0.0 { b0.v / speed } else { Vec3::ZERO };
        let h = (1.0 + 6.0 * (b0.v.y - 15.0) / 25.0).clamp(1.0, 7.0);
        let front = ci.sys.bodies[FRONT].x;
        let a = Vec3 { x: front.x, y: front.y + h, z: front.z };
        let b = Vec3 { x: front.x + d.x * 25.0, y: front.y + h, z: front.z + d.z * 25.0 };
        if !ci.ground.segment_hits(a, b) {
            st.lip_takeoff = true;
            u += 1.5 * l;
            st.slide_assist = l.abs() > 0.6 && s.abs() > 0.6;
        }
    }
    ctl.lean_input = u;

    // lean target and lag (spec 5.5); in the air the filter follows the measured roll
    let measured = ci.up_filt.dot(b0.q.rotate(Vec3::X)).clamp(-1.0, 1.0).asin();
    let lean_target = if ci.airborne { measured } else { -eval(&p.theta_lean_max_curve, v_fwd) * u };
    ctl.lean_cmd = st.lean.step(lean_target, DT);

    // torque feedback (spec 5.7)
    let k_target = if ci.airborne {
        p.max_torque_feedback
    } else {
        let (base, floor) = if ry >= 0.0 {
            (p.min_torque_feedback, 0.1)
        } else {
            (p.min_torque_feedback + (p.max_torque_feedback - p.min_torque_feedback) * ry.abs(), 0.5)
        };
        let rt = &ci.tyres[1];
        let slip_term = if !rt.side_grip && rt.contact { ((rt.slip.x.abs() - 1.0) / 25.0).clamp(0.0, 1.0) } else { 0.0 };
        let mut k = base * (1.0 - s.abs().max(slip_term)).max(floor);
        if rt.contact {
            k *= (1.0 - rt.slip.z.abs() / 40.0).clamp(0.1, 1.0);
        }
        k
    };
    ctl.torque_fb = st.torque_fb.step(k_target, DT);

    // drag (spec 5.8)
    let c = if ci.airborne { &p.air_drag_curve } else { &p.ground_drag_curve };
    ctl.drag = if speed > 0.0 { b0.v * (-eval(c, v_fwd) / speed) } else { Vec3::ZERO };

    // stick copies (ctl+0x68 LX, ctl+0x74 RY) and the rider fore/aft filter (grip modifiers)
    ctl.stick_lx = s;
    ctl.ctl_74 = ry;
    let ry_dz = if ry.abs() < 0.02 { 0.0 } else { ry };
    let y_f = st.rider_y.step(ry_dz, DT);

    // grip modifiers (spec 5.9)
    let signed_l = if ctl.lean_cmd < 0.0 { l } else { -l };
    let mut mods = [(0.0, 0.0); 2];
    for (i, tp) in p.tyres.iter().enumerate() {
        let roll = eval(&tp.static_mu_roll_curve, v_fwd) * eval(&tp.rider_shift_mu_roll_curve, y_f);
        let side = eval(&tp.static_mu_side_curve, v_fwd)
            * eval(&tp.lean_angle_mu_side_curve, signed_l)
            * eval(&tp.rider_shift_mu_side_curve, y_f);
        // slide assist after a lip: rear side grip x0.4, front x0.8 (0x0044de7f)
        let cut = if st.slide_assist { [0.8, 0.4][i] } else { 1.0 };
        mods[i] = (side * cut, roll);
    }
    mods
}
