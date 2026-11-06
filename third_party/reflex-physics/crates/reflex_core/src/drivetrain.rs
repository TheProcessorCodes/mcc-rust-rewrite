//! Slot 11 drivetrain stages and the slot 15 rolling friction:
//! gearbox (`00445e80` / `004460f0`), engine (`00446340`), brake application (`00447cd0`), drag sum (`0043efa0`),
//! rolling friction / brake lock (`00447810`). The controller outputs they read (throttle, torque feedback,
//! clutch mode, brake torques, drag force) come from slot 9 through [`Controls`].

use crate::bike::{CHASSIS, FRONT, REAR};
use crate::curve::blend3;
use crate::math::Vec3;
use crate::multibody::System;
use crate::params::Params;
use crate::sim::Controls;
use crate::tyre::Tyre;

/// Upshift / downshift delays, s (constructor `0043ac40`).
pub const UPSHIFT_DELAY: f64 = 0.3;
pub const DOWNSHIFT_DELAY: f64 = 0.15;
/// Terrain-drag reference load, lbf.
pub const TERRAIN_DRAG_LOAD: f64 = 173.88;

/// Drivetrain state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Drivetrain {
    /// Engine speed, rad/s (sim+0x8abc).
    pub engine_w: f64,
    /// Gear index 0..4 (sim+0x8ac0).
    pub gear: usize,
    /// Time since the last shift, s (sim+0x8aec).
    pub shift_timer: f64,
    /// Clutch timer, s (sim+0x8ab8).
    pub clutch_timer: f64,
}

fn forward_speed(sys: &System) -> f64 {
    let b = &sys.bodies[CHASSIS];
    b.q.inv_rotate(b.v).z
}

/// Gear with the most wheel torque at the current speed (ties to the lower gear).
fn best_gear(sys: &System, p: &Params) -> usize {
    let ww = (forward_speed(sys) / p.tyres[1].radius).max(0.5);
    let mut best = (0, 0.0);
    for (i, g) in p.gear_ratios.iter().enumerate() {
        let w = (g * ww).clamp(0.0, p.engine_max_speed);
        let score = g * p.torque_curve.eval(w);
        if i == 0 || score > best.1 {
            best = (i, score);
        }
    }
    best.0
}

/// Automatic gearbox, grounded only (`00445e80` on throttle, `004460f0` off throttle).
pub fn gearbox(d: &mut Drivetrain, sys: &System, p: &Params, throttle: f64) {
    let target = best_gear(sys, p);
    if throttle > 0.0 {
        if d.gear < target {
            if d.shift_timer <= UPSHIFT_DELAY {
                return;
            }
            d.gear = target;
        } else if d.gear > target {
            if d.shift_timer <= DOWNSHIFT_DELAY {
                return;
            }
            d.gear -= 1;
        } else {
            return;
        }
        d.shift_timer = 0.0;
    } else if target < d.gear && d.shift_timer > DOWNSHIFT_DELAY {
        d.gear = target;
        d.shift_timer = 0.0;
    }
}

/// Engine and clutch (`00446340`): engine speed, drive torque on the rear wheel about its axle, the
/// torque-feedback reaction on the chassis, the rear spin inertia, and the reverse push.
pub fn engine(d: &mut Drivetrain, sys: &mut System, p: &Params, ctl: &Controls, dt: f64) {
    let wmax = p.engine_max_speed;
    let mut we = d.engine_w.clamp(0.0, wmax);
    // clutch pull toward PeakClutchRPM
    let c = ((p.clutch_reset_time - d.clutch_timer) / p.clutch_reset_time).clamp(0.0, 1.0);
    let cp = c.powf(0.25);
    if cp <= 0.0 {
        we = p.peak_clutch_speed;
    } else if cp < 1.0 {
        we = (we - p.peak_clutch_speed) * cp + p.peak_clutch_speed;
    }
    we = we.clamp(0.0, wmax);

    let g = p.gear_ratios[d.gear.min(4)];
    let spin = sys.bodies[REAR].w.x;
    let t = p.torque_curve.eval(we);
    let dcurve = p.drag_torque_curve.eval(we);
    let mut td = -(dcurve * p.engine_drag_coefficient * g * p.max_drag_torque);
    if spin < 0.0 {
        td = -td;
    }
    let tw = (g * t - td) * ctl.throttle + td;
    let free_drag = -dcurve * p.max_drag_torque;
    let rear_i = p.rear_tire_inertia;
    let (wheel_torque, inertia_x) = match ctl.clutch_mode {
        0 => {
            we = g * spin;
            (tw, rear_i.x + p.engine_inertia + p.drivetrain_inertia)
        }
        1 => {
            let te = (2.0 * g * t - free_drag) * ctl.throttle + free_drag;
            let wf = (te * dt / p.engine_inertia + we) * p.engine_drag_coefficient;
            we = wf + (g * spin - wf) * cp;
            (te + (tw - te) * cp, rear_i.x + p.drivetrain_inertia)
        }
        _ => {
            let te = (t - free_drag) * ctl.throttle + free_drag;
            we += te * dt / p.engine_inertia;
            (0.0, rear_i.x + p.drivetrain_inertia)
        }
    };
    d.engine_w = we.clamp(p.engine_min_speed, wmax);
    sys.bodies[REAR].inertia = Vec3 { x: inertia_x, ..rear_i };
    sys.bodies[REAR].torque.x += wheel_torque;
    sys.bodies[CHASSIS].torque.x += -ctl.torque_fb * wheel_torque;

    if ctl.reverse > 0.0 {
        let s = ((sys.bodies[CHASSIS].v.length() - 2.5) / 10.0).clamp(0.0, 1.0);
        sys.bodies[CHASSIS].apply_force_body(Vec3 { x: 0.0, y: 0.0, z: (1.0 - s) * -150.0 });
    }
}

/// Brake torques from the controls (`00447cd0`): rear on the rear wheel's axle with the opposite on the chassis;
/// front on the front wheel's axle with the reaction rotated from the wheel frame into the chassis frame.
pub fn brakes(sys: &mut System, ctl: &Controls) {
    sys.bodies[REAR].torque.x += ctl.brake_rear;
    sys.bodies[CHASSIS].torque.x -= ctl.brake_rear;
    sys.bodies[FRONT].torque.x += ctl.brake_front;
    let reaction = sys.bodies[FRONT].q.rotate(Vec3 { x: -ctl.brake_front, y: 0.0, z: 0.0 });
    let c = &mut sys.bodies[CHASSIS];
    c.torque += c.q.inv_rotate(reaction);
}

/// Drag sum (`0043efa0`): slot-9 drag force, water drag and per-wheel terrain drag (computed from each wheel's
/// own velocity, scaled by its filtered load) on the chassis; the extra wheel forces on the wheels.
pub fn drag(sys: &mut System, p: &Params, tyres: &[Tyre; 2], ctl: &Controls, water: f64, wheel_extra: [Vec3; 2], crashed: bool) {
    let mut f = ctl.drag - sys.bodies[CHASSIS].v * water;
    for (i, wi) in [(1, REAR), (0, FRONT)] {
        let t = &tyres[i];
        if !t.contact {
            continue;
        }
        let tp = &p.tyres[i];
        let (k_drag, k_vel) = (blend3(&tp.terrain_drag, t.drag_index), blend3(&tp.terrain_velocity_drag, t.drag_index));
        let v = sys.bodies[wi].v;
        let dir = if v.length_squared() > 0.0 { v.normalize() } else { Vec3::ZERO };
        let load = (t.n_filt / TERRAIN_DRAG_LOAD).clamp(0.0, 2.0);
        f += (dir * -k_drag - v * k_vel) * load;
    }
    let scale = if crashed { 0.0 } else { 1.0 };
    sys.bodies[CHASSIS].apply_force(f * scale, Vec3::ZERO);
    sys.bodies[REAR].apply_force(wheel_extra[0] * scale, Vec3::ZERO);
    sys.bodies[FRONT].apply_force(wheel_extra[1] * scale, Vec3::ZERO);
}

/// Rolling friction and low-speed brake lock (`00447810`), per wheel on the wheel's angular velocity relative to
/// the chassis (expressed in the wheel frame): locked to the chassis below 10 rad/s with the brake input pressed,
/// else scaled by `WheelRollingFrictionTorsionDamping(|relative|)`. Returns true when both wheels are locked
/// (parked, sim+0xa8).
pub fn rolling_friction(sys: &mut System, p: &Params, front_brake: f64, rear_brake: f64) -> bool {
    let chassis_omega = sys.bodies[CHASSIS].omega();
    let mut locked = 0;
    for (wi, lever) in [(FRONT, front_brake), (REAR, rear_brake)] {
        let w = &mut sys.bodies[wi];
        let base = w.q.inv_rotate(chassis_omega);
        let rel = w.w - base;
        let mag = rel.length();
        if lever > 0.0 && mag < 10.0 {
            w.w = base;
            locked += 1;
        } else {
            w.w = base + rel * p.rolling_friction_curve.eval(mag);
        }
    }
    locked == 2
}
