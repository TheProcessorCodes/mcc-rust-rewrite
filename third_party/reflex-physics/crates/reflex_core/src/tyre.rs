//! Tyre state and the tyre force law (spec 6.6, `00447f10`; `model\contact-tyres-suspension.md` 2, 4),
//! checked against OpenReflex bike_tire.c.
//!
//! Axis names: the game's record calls W+120 (`axle x n`, our `roll_dir` l) "lateral" and W+140 (`n x l`, our
//! `lateral` s) "rolling". Our names follow the directions themselves, so the C's lateral impulse is our
//! longitudinal force and its W+c0 flag is our `roll_grip`; its rolling impulse is our side force and W+c4 is
//! our `side_grip`.

use crate::curve::{blend3, lerp_clamped};
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
        // OpenReflex bike_tire.c 00447f10, airborne: only W+c0 (the rolling-direction flag, our roll_grip) is
        // set; W+c4 (side_grip) keeps the 0 slot 13 wrote when contact was lost. The contact-frame velocity
        // W+80 is zeroed and the load decays towards 0.
        t.roll_grip = true;
        t.slip = Vec3::ZERO;
        t.n_filt += (0.0 - t.n_filt) * dt.min(t.n_filt_tau) / t.n_filt_tau;
        return;
    }
    let g = t.grip_index;
    let (n, l, s, pr) = (t.normal, t.roll_dir, t.lateral, t.rim);

    // 1 roll weight: the angle of the chassis x axis from the horizontal, through the game's arcsine
    // polynomial (OpenReflex bike_math.h bike_asin_approx); lean = (pi - angle) / pi, blend = 1 - (1 - lean)^2.
    let a = asin_approx(chassis.q.rotate(Vec3::X).y);
    let tt = (PI_F32 - a) / PI_F32;
    let wgt = 1.0 - (1.0 - tt) * (1.0 - tt);

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
        // wheelspin: lateral share bound (OpenReflex bike_tire.c 00447f10, the `body == 3 && 0 > rear_lat_speed`
        // block). The chassis' sideways speed and the rider's x lean pick the starting bound (5 by default,
        // towards 3 when leaning against the slide, towards 8 when leaning with it); rider y then pulls it to 3.
        let bound = wheelspin_bound(chassis.q.inv_rotate(chassis.v).x, inp.rider_x, inp.rider_y, inp.steer_x);
        lat_n = (-bound).max(lat_n.min(bound));
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
        // the throttle blend is Lerp_Clamped (00430810), so throttle outside [0, 1] is held at the ends
        lat_extra *= 1.0 - lerp_clamped(inp.throttle, 1.0, 0.5) * inp.ctl_74.max(0.0);
        lat_lim = (1.0 - inp.rear_brake) * lat_lim + inp.rear_brake * side_lim;
    }

    // 7 per-axis clamp. A grip flag drops whenever the demand is outside its limit, even when the slide-drag
    // offset leaves the side force where it was (00447f10 clears W+c0 / W+c4 on the limit test, not on a change).
    let mut f_long = f_long_req;
    t.roll_grip = true;
    if long_lim < f_long_req {
        f_long = f_long_req.min(long_lim);
        t.roll_grip = false;
    } else if f_long_req < -long_lim {
        f_long = f_long_req.max(-long_lim);
        t.roll_grip = false;
    }
    let mut f_lat = f_lat_req;
    t.side_grip = true;
    if lat_lim < f_lat_req {
        f_lat = f_lat_req.min(lat_lim + lat_extra);
        t.side_grip = false;
    } else if f_lat_req < -lat_lim {
        f_lat = f_lat_req.max(-lat_lim + lat_extra);
        t.side_grip = false;
    }

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

/// 3.1415927 (00977498), pi as the game stores it.
const PI_F32: f64 = 3.141_592_741_012_573_2;

/// The game's inlined arcsine (OpenReflex bike_math.h `bike_asin_approx`, used by 00447f10 and others):
/// a rational function of y^2 for |s| below 0.7071 (009ddfc8), and pi/2 - asin(sqrt(1 - s^2)) above.
/// Returns the arcsine of |s|, so it is never negative. f64 throughout; the original rounds to float between
/// steps.
pub(crate) fn asin_approx(s: f64) -> f64 {
    let x = s.abs();
    let high = x >= 0.707_106_828_689_575_2; // 0x3f350481
    let y = if high { (1.0 - s * s).abs().sqrt() } else { x };
    let y2 = y * y;
    let y4 = y2 * y2;
    let p = (0.564_809_322_357_177_7 - y2 * 0.049_928_508_699_178_7) * y4; // 009777b0 / 009777a8
    let q = 1.0 - y2 * 1.443_035_483_360_290_5; // 009777a0
    let r = 1.0 / (p + q);
    let n1 = (0.399_303_406_476_974_5 - y2 * 0.019_378_423_690_795_9) * y4; // 00977798 / 00977790
    let n0 = 1.0 - y2 * 1.276_368_737_220_764_2; // 00977788
    let a = r * ((n1 + n0) * y);
    if high {
        1.570_796_370_506_286_6 - a // 00977510
    } else {
        a
    }
}

/// The rear tyre's wheelspin bound on the side share of the demand (OpenReflex bike_tire.c 00447f10).
/// `v_side` is the chassis velocity along its own x axis; rider x / y and steer x are the raw input block
/// (sim+0x34 / 0x38 / 0x2c). The C also builds a factor k from the tilt of the contact normal, but its
/// clamp (`p >= 1 ? 1 : 1 < p ? p : p != p ? p : 1`) gives 1 for every finite tilt, so the "3" ends stay 3.
fn wheelspin_bound(v_side: f64, rider_x: f64, rider_y: f64, steer_x: f64) -> f64 {
    let ry = if 0.0 > rider_y { 0.0 } else { rider_y };
    let low = 3.0; // k * 3 (0092fe48) with k = 1
    let forward = 0.0 <= v_side;
    let backward = v_side < 0.0;
    let mut limit_a = 5.0; // 00935890
    if forward && 0.0 > rider_x {
        let b = ((1.0 - ry) * steer_x).max(0.0);
        limit_a = lerp_clamped(-(rider_x + b), 5.0, low);
    } else if backward && rider_x > 0.0 {
        let b = ((1.0 - ry) * steer_x).min(0.0);
        limit_a = lerp_clamped(rider_x + b, 5.0, low);
    } else if forward && rider_x > 0.0 {
        limit_a = lerp_clamped(rider_x, 5.0, 8.0); // 009322b8
    } else if backward && 0.0 > rider_x {
        limit_a = lerp_clamped(-rider_x, 5.0, 8.0);
    }
    lerp_clamped(ry, limit_a, low)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asin_approx_close_to_asin() {
        for i in 0..=100 {
            let s = -1.0 + 0.02 * i as f64;
            let want = s.abs().asin();
            assert!((asin_approx(s) - want).abs() < 2e-4, "s={s}: {} vs {want}", asin_approx(s));
        }
    }

    #[test]
    fn wheelspin_bound_cases() {
        // no lean: 5, pulled to 3 by rider y
        assert_eq!(wheelspin_bound(1.0, 0.0, 0.0, 0.0), 5.0);
        assert_eq!(wheelspin_bound(1.0, 0.0, 1.0, 0.0), 3.0);
        assert!((wheelspin_bound(1.0, 0.0, 0.5, 0.0) - 4.0).abs() < 1e-12);
        // negative rider y counts as 0
        assert_eq!(wheelspin_bound(1.0, 0.0, -1.0, 0.0), 5.0);
        // leaning with the slide opens it towards 8
        assert!((wheelspin_bound(1.0, 0.5, 0.0, 0.0) - 6.5).abs() < 1e-12);
        assert!((wheelspin_bound(-1.0, -1.0, 0.0, 0.0) - 8.0).abs() < 1e-12);
        // leaning against it closes it towards 3; steering the same way adds to the lean
        assert!((wheelspin_bound(1.0, -0.5, 0.0, 0.0) - 4.0).abs() < 1e-12);
        // ... and steering away from the lean takes some of it back
        assert!((wheelspin_bound(1.0, -0.5, 0.0, 0.25) - 4.5).abs() < 1e-12);
        assert!((wheelspin_bound(-1.0, 0.5, 0.0, -0.25) - 4.5).abs() < 1e-12);
        // steering towards the lean is ignored (b is held at 0)
        assert!((wheelspin_bound(1.0, -0.5, 0.0, -0.5) - 4.0).abs() < 1e-12);
    }
}
