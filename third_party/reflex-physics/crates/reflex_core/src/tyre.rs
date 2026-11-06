//! Tyre state and the tyre force law (spec 6.6, `00447f10`; `model\contact-tyres-suspension.md` 2, 4).

use std::f64::consts::PI;

use crate::curve::blend3;
use crate::math::Vec3;
use crate::multibody::Body;
use crate::params::TyreParams;

/// One tyre (index 0 front, 1 rear). The contact record is written at the end of a substep (slot 13) and
/// consumed by the next substep's forces.
#[derive(Clone, Debug, Default)]
pub struct Tyre {
    pub contact: bool,
    /// Ground point P, ground normal n (world).
    pub point: Vec3,
    pub normal: Vec3,
    /// Rim point C + r and rim arm r = R normalize(P - C) (world), at detection time.
    pub rim: Vec3,
    pub arm: Vec3,
    /// The rim arm in the chassis frame at detection (tyre+0x90); the lean constraint rebuilds the rim point
    /// from it.
    pub arm_chassis: Vec3,
    /// Rolling direction l = normalize(axle x n) and lateral s = n x l.
    pub roll_dir: Vec3,
    pub lateral: Vec3,
    /// Suspension shorter than BottomOutLength (set by the suspension force step).
    pub bottomed: bool,
    /// Not clamped this step (read by the lean chain and torque feedback).
    pub roll_grip: bool,
    pub side_grip: bool,
    /// Surface indices (spec 2.3): grip 0 sticky .. 1 slippery, drag 0..1.
    pub grip_index: f64,
    pub drag_index: f64,
    /// Grip modifiers from the controls (spec 5.9).
    pub side_mod: f64,
    pub roll_mod: f64,
    /// Low-passed grip load (terrain drag only) and its time constant.
    pub n_filt: f64,
    pub n_filt_tau: f64,
    /// Contact-point slip velocity of the wheel body: (lateral, normal, longitudinal).
    pub slip: Vec3,
    /// Total force this step (telemetry).
    pub force: Vec3,
    /// Rear only: the doughnut term is strong (|factor| > 0.6, sim+0x8da0); the wreck check skips yaw rates.
    pub doughnut: bool,
}

impl Tyre {
    pub fn new() -> Self {
        Tyre {
            roll_grip: true,
            side_grip: true,
            // the surface of most recorded runs (stock free-ride dirt): grip 0.48, drag 0.25
            grip_index: 0.48,
            drag_index: 0.25,
            side_mod: 1.0,
            roll_mod: 1.0,
            n_filt_tau: 0.1,
            ..Default::default()
        }
    }

    /// Contact record `0042db20` from the wheel centre C, its axle (world) and the ground point / normal.
    pub fn set_contact(&mut self, center: Vec3, axle: Vec3, radius: f64, point: Vec3, normal: Vec3, chassis_q: crate::Quat) {
        self.contact = true;
        self.point = point;
        self.normal = normal;
        let dir = point - center;
        self.arm = if dir.length_squared() > 0.0 { dir.normalize() * radius } else { -normal * radius };
        self.rim = center + self.arm;
        self.arm_chassis = chassis_q.inv_rotate(self.arm);
        self.roll_dir = axle.cross(normal).normalize();
        self.lateral = normal.cross(self.roll_dir);
    }
}

/// Inverse effective mass of body `b` at world offset `r` along unit world direction `e`:
/// 1/m + e . ((I^-1 (r x e)) x r).
fn inv_mass(b: &Body, r: Vec3, e: Vec3) -> f64 {
    let (rb, eb) = (b.q.inv_rotate(r), b.q.inv_rotate(e));
    1.0 / b.mass + eb.dot(rb.cross(eb).div_elem(b.inertia).cross(rb))
}

/// Inputs to the tyre law that come from the controls / input block.
#[derive(Clone, Copy, Debug, Default)]
pub struct TyreInputs {
    /// Processed throttle (ctl+0x00).
    pub throttle: f64,
    /// ctl+0x74 (rider fore/aft, used by the rear side-slip drag cut).
    pub ctl_74: f64,
    /// Raw input block: rear brake sim+0x28, steer X sim+0x2c, rider X sim+0x34, rider Y sim+0x38.
    pub rear_brake: f64,
    pub steer_x: f64,
    pub rider_x: f64,
    pub rider_y: f64,
    /// Chassis forward speed (chassis-frame z), ft/s.
    pub v_fwd: f64,
    pub crashed: bool,
}

/// `00447f10` for one tyre. `rear` selects the rear-only terms. Applies the force to the wheel body (or the
/// chassis when crashed) and the bump stop to the chassis.
pub fn tyre_step(t: &mut Tyre, p: &TyreParams, rear: bool, chassis: &mut Body, wheel: &mut Body, inp: &TyreInputs, dt: f64) {
    t.force = Vec3::ZERO;
    if !t.contact {
        t.roll_grip = true;
        t.side_grip = true;
        t.n_filt += (0.0 - t.n_filt) * dt.min(t.n_filt_tau) / t.n_filt_tau;
        return;
    }
    let g = t.grip_index;
    let (n, l, s, pr) = (t.normal, t.roll_dir, t.lateral, t.rim);

    // 1 roll weight
    let a = chassis.q.rotate(Vec3::X).y.clamp(-1.0, 1.0).asin().abs();
    let tt = 1.0 - a / PI;
    let wgt = 1.0 - (a / PI) * (a / PI);

    // 2 normal force (wheel body only) and bump stop (chassis)
    let (rw, r0) = (pr - wheel.x, pr - chassis.x);
    let vel_w = wheel.v + wheel.omega().cross(rw);
    let vel_0 = chassis.v + chassis.omega().cross(r0);
    let n_w = n * ((-vel_w.dot(n)).max(0.0) / dt / inv_mass(wheel, rw, n));
    let f_bump = if t.bottomed { n * ((-vel_0.dot(n)).max(0.0) / dt / inv_mass(chassis, r0, n)) } else { Vec3::ZERO };
    let load = (n_w + f_bump).length();
    t.n_filt += (load - t.n_filt) * dt.min(t.n_filt_tau) / t.n_filt_tau;

    // 3 limits
    let roll_lim = load * t.roll_mod * blend3(&p.static_mu_roll, g);
    let side_lim = load * t.side_mod * blend3(&p.static_mu_side, g);

    // 4 required forces
    let v_long = vel_w.dot(l);
    let v_lat = vel_w.dot(s);
    let f_long_req = -v_long / dt / inv_mass(wheel, rw, l);
    let lat_wheel = -v_lat / dt / inv_mass(wheel, rw, s);
    let lat_chassis = -vel_0.dot(s) / dt / inv_mass(chassis, r0, s);
    let f_lat_req = tt * (lat_wheel + (lat_chassis - lat_wheel) * wgt);

    // 5 slide test: the side limit is the circle radius for both axes
    let mut lat_n = f_lat_req / (side_lim + 1.0);
    let long_n = f_long_req / (side_lim + 1.0);
    if rear && v_long < 0.0 {
        // wheelspin: lateral share bound B. The B0 = 3 / 8 sign cases (spec 6.6 step 6) are not implemented.
        let b = 5.0 + (3.0 - 5.0) * inp.rider_y.max(0.0);
        lat_n = lat_n.clamp(-b, b);
    }
    let ratio = lat_n.hypot(long_n);
    let sliding = (f_lat_req.hypot(f_long_req) / (side_lim + 1.0)) > 1.0;

    // 6 limits
    let (long_lim, mut lat_lim, mut lat_extra) = if sliding {
        let share = if ratio > 0.0 { lat_n / ratio } else { 0.0 };
        let long_lim = blend3(&p.dynamic_mu_roll, g) * roll_lim;
        let lat_lim = (blend3(&p.dynamic_mu_side, g) * share * side_lim).abs();
        let extra = share * blend3(&p.side_slip_drag, g) * p.side_slip_drag_curve.eval(v_lat.abs());
        (long_lim, lat_lim, extra)
    } else {
        (roll_lim, side_lim, 0.0)
    };
    if rear && sliding {
        lat_extra *= 1.0 - (1.0 + (0.5 - 1.0) * inp.throttle) * inp.ctl_74.max(0.0);
        lat_lim += (side_lim - lat_lim) * inp.rear_brake;
    }

    // 7 per-axis clamp
    let mut f_long = f_long_req.clamp(-long_lim, long_lim);
    t.roll_grip = f_long == f_long_req;
    let mut f_lat = if f_lat_req > lat_lim {
        f_lat_req.min(lat_lim + lat_extra)
    } else if f_lat_req < -lat_lim {
        f_lat_req.max(-lat_lim + lat_extra)
    } else {
        f_lat_req
    };
    t.side_grip = f_lat == f_lat_req;

    // 8 rear doughnut
    if rear {
        let spin = ((-v_long - 1.0) / 40.0).clamp(0.0, 1.0);
        let stick = -((inp.steer_x + inp.rider_x + inp.rider_y) / 2.0).clamp(-1.0, 1.0);
        let fade = 1.0 - (inp.v_fwd / 60.0).clamp(0.0, 1.0);
        let factor = spin * stick * fade;
        t.doughnut = factor.abs() > 0.6;
        f_lat += blend3(&p.doughnut, g) * factor;
    }

    // 9 crashed
    if inp.crashed {
        f_lat *= 0.2;
        f_long *= 0.3;
        t.roll_grip = false;
        t.side_grip = false;
    }

    // 10 apply
    let f = s * f_lat + l * f_long + n_w;
    if inp.crashed {
        chassis.apply_force(f, chassis.q.inv_rotate(r0));
    } else {
        wheel.apply_force(f, wheel.q.inv_rotate(rw));
    }
    if f_bump.length_squared() > 0.0 {
        chassis.apply_force(f_bump, chassis.q.inv_rotate(r0));
    }
    t.slip = Vec3 { x: v_lat, y: vel_w.dot(n), z: v_long };
    t.force = f + f_bump;
}
