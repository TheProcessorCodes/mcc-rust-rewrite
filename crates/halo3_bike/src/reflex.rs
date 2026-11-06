//! "Reflex v1": MX vs ATV Unleashed's model (`unleashed.rs`, `physics.rs`)
//! pulled towards stock MX vs ATV Reflex's mx450f.
//!
//! The way the bike is simulated stays Unleashed's (placed lean, springs
//! along the ground's normal, grip near the centre of gravity, the clutch
//! and automatic box, the wheelie hold, the damped air control). What
//! changes are the numbers, and four small rules Reflex has and Unleashed
//! does not:
//! - the bars follow Reflex's steering-for-speed curve, the stick squared
//!   and lagged, and the rider leaning sideways adds to them;
//! - the lean follows Reflex's lean-for-speed curve, and the rider leaning
//!   into the turn leans the bike further (x1.55 at full both);
//! - sideways grip is capped at Reflex's 1.3 x load and a tyre past it
//!   slides at 0.9 of that, instead of Unleashed's curve that keeps climbing
//!   to 5 x load; the curve is stiffer, so it reaches the cap at small slip;
//! - Reflex's mass, inertia, springs, dampers, travel, brakes, engine curve,
//!   gears and drag;
//! - the shape of the bike drawn, Reflex's Dakumoto 450F: where its axles
//!   sit, how big its tyres are (`DAKUMOTO_450F` below).
//!
//! Reflex's numbers come from the stock mx450f's vehicle data (lean,
//! suspension and tyres, engine), the same ones the earlier Reflex-style
//! bike used
//! (`backup/bike-physics-reflex-2026-10-28`).

use bevy::math::Vec3;

use super::unleashed::{DEG, MPH, Setup};

/// Reflex's tyre radius, which its gear ratios work through. Our rear
/// tyre is the drawn one (0.335 m), so the ratios are scaled to give the
/// same road speed at the same rpm.
const REFLEX_GEAR_RADIUS: f32 = 0.317;

/// The Dakumoto 450F as Reflex draws it (`MXs` / `HR_MXs`, `NeptuneF`),
/// measured from its bind pose with `reflex_assets` (`cargo test -p
/// reflex_assets -- --nocapture` prints it), metres from the model's
/// origin, which is the centre of gravity: front axle, rear axle, front
/// and rear tyre radius (to the knobs).
///
/// Unleashed's Pro 250 has a 1.57 m wheelbase and 0.366 m tyres; the 450F
/// is 1.46 m and 0.345 / 0.335 m, with its axles about 2 cm lower. The
/// ground sits 0.60 m under the centre of gravity on both.
pub const DAKUMOTO_450F: (Vec3, Vec3, f32, f32) = (
    Vec3::new(0.0, -0.252, 0.697),
    Vec3::new(0.0, -0.266, -0.764),
    0.345,
    0.335,
);

/// A curve in (mph, degrees) as (m/s, radians).
fn mph_deg(curve: &[(f32, f32)]) -> Vec<(f32, f32)> {
    curve.iter().map(|&(mph, deg)| (mph * MPH, deg * DEG)).collect()
}

impl Setup {
    /// The Unleashed Pro 250 with Reflex's mx450f numbers and rules.
    pub fn reflex_v1() -> Setup {
        let base = Setup::pro_250();
        // Chassis 99 kg, rider 58, wheels 19.
        let mass = 177.0;
        let weight = mass * crate::physics::GRAVITY;
        // Reflex gives springs in N/m and dampers in N·s/m; Unleashed's
        // fields are the spring over the bike's weight and the damping as
        // a share of critical (on the whole mass).
        let crit = |k: f32| 2.0 * (k * mass).sqrt();
        let (front_axle, rear_axle, front_radius, rear_radius) = DAKUMOTO_450F;
        let tyre = |base: super::unleashed::Tyre, radius: f32, k: f32, bump: f32, rebound: f32, travel: f32, brake: f32| {
            super::unleashed::Tyre {
                radius,
                strut: travel,
                spring: k / weight,
                // Unleashed's `bounce` damps extension, `rebound` compression.
                bounce: rebound / crit(k),
                rebound: bump / crit(k),
                max_drive: 1.3,
                skid_drive: 1.2,
                max_braking_torque: brake,
                slip_scale: 2.5,
                side_cap: 1.3,
                slide_keep: 0.9,
                ..base
            }
        };
        // Overall ratios, crank to rear wheel, first to fifth.
        let gears = vec![19.72, 16.11, 13.53, 11.50, 9.63];
        Setup {
            name: "Reflex v1",
            mass,
            gravity_scale: 1.0,
            // The chassis (13.6 / 19.0 / 13.6) with the rider and wheels
            // added about the shared centre of mass.
            inertia: bevy::math::Vec3::new(36.0, 32.0, 28.0),
            cofg_travel_back: 0.30,
            cofg_travel_forward: 0.15,
            front_axle,
            rear_axle,
            // Forks 20.2 kN/m, 438 / 876 N·s/m, 0.40 m; shock 13.9 kN/m,
            // 584 / 1167 N·s/m, 0.37 m. Brakes 339 / 475 N·m.
            front: tyre(base.front, front_radius, 20200.0, 438.0, 876.0, 0.40, 339.0),
            rear: tyre(base.rear, rear_radius, 13860.0, 584.0, 1167.0, 0.37, 475.0),
            steer_curve: mph_deg(&[
                (0.0, 30.0),
                (10.0, 29.3),
                (20.0, 27.5),
                (30.0, 23.5),
                (40.0, 16.1),
                (50.0, 9.5),
                (60.0, 6.7),
                (80.0, 4.7),
            ]),
            steer_power: 2.0,
            steer_lag: 0.15,
            rider_steer: 0.2,
            rider_lean: 0.55,
            lean_curve: mph_deg(&[
                (0.0, 5.6),
                (10.0, 13.0),
                (20.0, 17.0),
                (30.0, 20.0),
                (40.0, 24.0),
                (50.0, 27.0),
                (80.0, 34.0),
            ]),
            roll_gain: 1.0 / 0.3,
            // 55 N·m peak about 5400 rpm, nothing left at the 11,500 limiter.
            torque_curve: vec![
                (0.0, 13.4),
                (1990.0, 40.1),
                (3380.0, 50.6),
                (4613.0, 54.5),
                (6034.0, 54.2),
                (7456.0, 45.0),
                (8467.0, 30.6),
                (9604.0, 14.8),
                (11500.0, 0.0),
            ],
            final_ratio: rear_radius / REFLEX_GEAR_RADIUS,
            gears,
            min_rpm: 1500.0,
            // 27 N·m of drag at the limiter.
            engine_braking: 27.1 / (11500.0 * 11500.0),
            // Up past the power peak, down where the lower gear lands below
            // the limiter with room to spare (no hunting).
            upshift_rpm: 8400.0,
            downshift_rpm: 6000.0,
            // Ground drag: 458 N at 31.4 m/s.
            drag: 0.93,
            steer_rate: 8.0,
            ..base
        }
    }
}

/// The values the tuning menu (`tune.rs`) changes, over a base model. All
/// but the model, the steering lag and gravity are multipliers: 1 is the
/// base model as it stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// 0: Unleashed Pro 250 as it is, 1: Reflex v1.
    pub model: f32,
    pub power: f32,
    pub gearing: f32,
    pub engine_brake: f32,
    pub brakes: f32,
    pub grip: f32,
    pub side_stiffness: f32,
    pub slide_keep: f32,
    pub steer_lock: f32,
    /// Seconds; negative means "the model's own".
    pub steer_lag: f32,
    pub lean_depth: f32,
    pub lean_speed: f32,
    pub rider_lean: f32,
    pub weight_shift: f32,
    pub wheelie_hold: f32,
    pub spring: f32,
    pub damper: f32,
    pub air_spin: f32,
    pub air_damping: f32,
    pub gravity: f32,
    /// Reflex itself only (`exact.rs`): its engine torque, and its gear
    /// ratios (below 1 taller: a higher top speed, a softer pull).
    pub reflex_power: f32,
    pub reflex_gearing: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            model: 1.0,
            power: 1.0,
            gearing: 1.0,
            engine_brake: 1.0,
            brakes: 1.0,
            grip: 1.0,
            side_stiffness: 1.0,
            slide_keep: 1.0,
            steer_lock: 1.0,
            steer_lag: -0.01,
            lean_depth: 1.0,
            lean_speed: 1.0,
            rider_lean: 1.0,
            weight_shift: 1.0,
            wheelie_hold: 1.0,
            spring: 1.0,
            damper: 1.0,
            air_spin: 1.0,
            air_damping: 1.0,
            gravity: 1.0,
            reflex_power: 1.0,
            reflex_gearing: 1.0,
        }
    }
}

impl Tuning {
    /// The base model this tuning starts from.
    pub fn base(&self) -> Setup {
        if self.model >= 0.5 { Setup::reflex_v1() } else { Setup::pro_250() }
    }

    /// The bike with this tuning applied.
    pub fn setup(&self) -> Setup {
        let mut s = self.base();
        for (_, t) in s.torque_curve.iter_mut() {
            *t *= self.power;
        }
        // A stronger engine needs a clutch that still holds it.
        s.clutch_static *= self.power.max(1.0);
        s.clutch_kinetic *= self.power.max(1.0);
        s.final_ratio *= self.gearing;
        s.engine_braking *= self.engine_brake;
        for t in [&mut s.front, &mut s.rear] {
            t.max_braking_torque *= self.brakes;
            t.max_drive *= self.grip;
            t.skid_drive *= self.grip;
            t.side_cap *= self.grip;
            for c in [&mut t.side, &mut t.slipping, &mut t.braking] {
                c.scale *= self.grip;
            }
            t.slip_scale *= self.side_stiffness;
            t.slide_keep = (t.slide_keep * self.slide_keep).min(1.0);
            t.spring *= self.spring;
            // Damping is a share of critical, which grows with the spring:
            // keep the dampers' N·s/m as the slider says.
            let keep = self.damper / self.spring.sqrt();
            t.bounce *= keep;
            t.rebound *= keep;
        }
        for (_, a) in s.steer_curve.iter_mut() {
            *a *= self.steer_lock;
        }
        if self.steer_lag >= 0.0 {
            s.steer_lag = self.steer_lag;
        }
        for (_, a) in s.lean_curve.iter_mut() {
            *a *= self.lean_depth;
        }
        s.roll_gain *= self.lean_speed;
        s.rider_lean *= self.rider_lean;
        s.rider_steer *= self.rider_lean;
        s.cofg_travel_back *= self.weight_shift;
        s.cofg_travel_forward *= self.weight_shift;
        s.pitch_rate_gain *= self.wheelie_hold;
        s.air_moment *= self.air_spin;
        s.air_force *= self.air_spin;
        s.air_damping *= self.air_damping;
        s.gravity_scale *= self.gravity;
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reflex_numbers() {
        let s = Setup::reflex_v1();
        assert_eq!(s.mass, 177.0);
        // Static sag under half the weight: a few centimetres.
        let sag = s.mass * 9.81 * 0.5 / s.spring_rate(&s.rear);
        assert!((0.04..0.09).contains(&sag), "rear sag {sag}");
        // The forks' bump damper comes back out in N·s/m.
        let c = s.damping(&s.front, s.front.rebound);
        assert!((c - 438.0).abs() < 1.0, "{c}");
        // 30° of bar standing, 9.5° at 50 mph.
        assert!((s.steer_lock(0.0) - 30.0 * DEG).abs() < 1e-4);
        assert!((s.steer_lock(50.0 * MPH) - 9.5 * DEG).abs() < 1e-4);
        // Fifth at 11,500 rpm is Reflex's road speed (0.317 m tyre).
        let v = 11500.0 / crate::unleashed::RPM / s.ratio(4) * s.rear.radius;
        let reflex = 11500.0 / crate::unleashed::RPM / 9.63 * REFLEX_GEAR_RADIUS;
        assert!((v - reflex).abs() < 0.01, "{v} {reflex}");
    }

    #[test]
    fn shaped_like_the_450f() {
        let s = Setup::reflex_v1();
        let base = s.front_axle.z - s.rear_axle.z;
        assert!((base - 1.461).abs() < 0.005, "wheelbase {base}");
        // Both tyres meet flat ground at the same height, 0.60 m under the
        // centre of gravity, so the bike drawn sits level.
        let front = s.front.radius - s.front_axle.y;
        let rear = s.rear.radius - s.rear_axle.y;
        assert!((front - rear).abs() < 0.01 && (front - 0.60).abs() < 0.01, "{front} {rear}");
        // The skid plate probe clears flat ground at rest by about 0.3 m.
        let ground = -crate::physics::rest_height();
        let skid = crate::physics::PROBES[0];
        let clearance = skid.0.y - skid.1 - ground;
        assert!((0.2..0.35).contains(&clearance), "clearance {clearance}");
    }

    #[test]
    fn stock_tuning_is_the_base() {
        let t = Tuning::default();
        let (a, b) = (t.setup(), Setup::reflex_v1());
        assert_eq!(a.front.spring, b.front.spring);
        assert_eq!(a.rear.bounce, b.rear.bounce);
        assert_eq!(a.steer_lag, b.steer_lag);
        let u = Tuning {
            model: 0.0,
            ..t
        }
        .setup();
        assert_eq!(u.mass, Setup::pro_250().mass);
    }

    #[test]
    fn damper_slider_is_independent_of_the_spring() {
        let t = Tuning {
            spring: 2.0,
            ..Tuning::default()
        };
        let (a, b) = (t.setup(), Setup::reflex_v1());
        let ca = a.damping(&a.front, a.front.rebound);
        let cb = b.damping(&b.front, b.front.rebound);
        assert!((ca - cb).abs() < 0.5, "{ca} {cb}");
    }
}
