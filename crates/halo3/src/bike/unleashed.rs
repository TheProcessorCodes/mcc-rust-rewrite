//! MX vs ATV Unleashed's dirt bike model, as numbers and small formulas.
//!
//! Unleashed keeps each bike's tuning in an INI-style `.phy` file
//! (`Data\Veh\mx_Pro250A.phy` in `common.pak`) in feet, pounds, mph and
//! degrees. The values below are the stock Pro 250's, and every formula
//! here was written from our own reading of how the game uses them
//! (`docs/bike-unleashed.md` has the model in plain words and the sources).
//! `Setup::pro_250()` converts everything to SI once; the formulas that
//! were tuned against feet per second (the tyre curves, the brake fade)
//! take their speed in ft/s through `ft()`.

use bevy::math::Vec3;

/// Feet to metres, and the other conversions the `.phy` units need.
pub const FT: f32 = 0.3048;
pub const MPH: f32 = 0.44704;
pub const LBF: f32 = 4.448_222;
/// lbf·ft to N·m, and slug·ft² to kg·m² (the same factor).
pub const LBF_FT: f32 = 1.355_818;
pub const SLUG: f32 = 14.593_9;
pub const DEG: f32 = std::f32::consts::PI / 180.0;
/// The game's gravity, ft/s².
const G_FT: f32 = 32.2;

/// A speed in m/s as ft/s, for the curves tuned in feet.
pub fn ft(v: f32) -> f32 {
    v / FT
}

/// `y0` at `x0` to `y1` at `x1`, held at the ends.
pub fn lerp_clamp(x: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    if x1 == x0 {
        return y0;
    }
    let t = ((x - x0) / (x1 - x0)).clamp(0.0, 1.0);
    y0 + (y1 - y0) * t
}

/// One of the tyre's sideways grip curves: grip (force over load) against
/// sideways slip speed in ft/s. It rises about linearly, peaks and falls
/// away slowly once the tyre is really sliding.
#[derive(Clone, Copy, Debug)]
pub struct SideCurve {
    pub round_off: f32,
    pub drop_off: f32,
    pub scale: f32,
    pub speed_scale: f32,
}

impl SideCurve {
    /// Grip against slip `v` (ft/s): negative, it pushes back against the slide.
    pub fn grip(&self, v: f32) -> f32 {
        let k = self.speed_scale;
        -k * self.scale * v / (1.0 + (k * self.round_off * v).abs().powf(self.drop_off))
    }
}

/// One wheel's tyre and suspension (SI unless noted).
#[derive(Clone, Copy, Debug)]
pub struct Tyre {
    pub radius: f32,
    /// How far the strut can compress before the frame bottoms out, m.
    pub strut: f32,
    /// Most compression speed the damper sees, m/s.
    pub max_compression_rate: f32,
    /// How fast the wheel drops back out (drawing only), m/s.
    pub max_decompression_rate: f32,
    /// Spring rate as a multiple of the bike's weight per metre (k = this · m · g).
    pub spring: f32,
    /// Damping as a share of critical, extending and compressing.
    pub bounce: f32,
    pub rebound: f32,
    /// Where the tyre's grip (not its spring) acts on the frame, bike frame
    /// from the centre of gravity: barely below and inside the wheelbase, so
    /// grip turns the bike far less than it would from the contact patch.
    pub force_position: Vec3,
    /// Grip as a multiple of load: most driving or braking force before the
    /// tyre breaks loose, and what it gives while loose.
    pub max_drive: f32,
    pub skid_drive: f32,
    /// Sideways grip rolling, while spinning or locked, and while braking.
    pub side: SideCurve,
    pub slipping: SideCurve,
    pub braking: SideCurve,
    /// Brake at full pedal and rolling friction, N·m.
    pub max_braking_torque: f32,
    pub wheel_friction: f32,
}

impl Tyre {
    /// The wheel's own spin inertia, kg·m²: Unleashed takes the wheel as a
    /// disc half a foot thick (0.25·π·r⁴ in slug·ft²).
    pub fn wheel_inertia(&self) -> f32 {
        let r = self.radius / FT;
        0.25 * std::f32::consts::PI * r.powi(4) * LBF_FT
    }
}

/// The whole bike (SI unless noted).
#[derive(Clone, Debug)]
pub struct Setup {
    /// Bike and rider together, kg, and gravity's multiplier.
    pub mass: f32,
    pub gravity_scale: f32,
    /// Principal moments about the bike's X (pitch), Y (yaw), Z (roll), kg·m².
    pub inertia: Vec3,
    /// Yaw rate cap, rad/s (pitch and roll are capped at 20).
    pub yaw_rate_limit: f32,
    /// How far the rider moves the centre of gravity at full stick, m.
    pub cofg_travel_back: f32,
    pub cofg_travel_forward: f32,
    /// Axles at full extension, bike frame from the static centre of gravity.
    pub front_axle: Vec3,
    pub rear_axle: Vec3,
    pub front: Tyre,
    pub rear: Tyre,
    /// Bar lock, rad, and its share of that at two speeds (m/s).
    pub max_steer: f32,
    pub steer_speed: [f32; 2],
    pub steer_scale: [f32; 2],
    /// Engine: (rpm, torque N·m) points, gears and final drive.
    pub torque_curve: Vec<(f32, f32)>,
    pub gears: Vec<f32>,
    pub final_ratio: f32,
    pub min_rpm: f32,
    pub idle_throttle: f32,
    /// Engine braking: N·m per rpm².
    pub engine_braking: f32,
    /// kg·m² (engine side of the clutch).
    pub engine_inertia: f32,
    /// Clutch grip locked and slipping, N·m.
    pub clutch_static: f32,
    pub clutch_kinetic: f32,
    pub upshift_rpm: f32,
    pub downshift_rpm: f32,
    /// Backing up: target speed (negative, m/s) and gain (N per m/s).
    pub reverse_speed: f32,
    pub reverse_gain: f32,
    /// Lean: most lean (rad), reached at `max_roll_speed` (m/s), and how fast
    /// the bike follows (1/s). Below `stand_upright[0]` the bike leans from
    /// the vertical, above `stand_upright[1]` from the ground's normal.
    pub max_roll: f32,
    pub max_roll_speed: f32,
    pub roll_gain: f32,
    pub stand_upright: [f32; 2],
    /// Wheelie hold: most pitch (rad), gains, and the speed it fades out at.
    pub max_pitch: f32,
    pub pitch_gain: f32,
    pub pitch_rate_gain: f32,
    pub wheelie_top_speed: f32,
    /// Stoppie hold, the same way.
    pub max_stoppie_pitch: f32,
    pub stoppie_pitch_gain: f32,
    pub stoppie_top_speed: f32,
    pub stoppie_yaw_rate_scale: f32,
    /// Air drag: force = 0.5 · drag · v², N with v in m/s.
    pub drag: f32,
    /// In the air: damping (N·m per rad/s), the rider's torques at full stick
    /// (bike X, Y, Z, N·m), a sideways push (N), and the gas and rear brake
    /// twisting the frame (N·m).
    pub air_damping: f32,
    pub air_moment: Vec3,
    pub air_force: f32,
    pub gas_gyro: f32,
    pub brake_gyro: f32,
    /// With the stick let go the bike levels its lean and lines up with
    /// where it is going.
    pub unroll_gain: f32,
    pub unroll_torque: f32,
    pub unyaw_gain: f32,
    pub unyaw_torque: f32,
    /// How fast the filtered controls may move, per second.
    pub steer_rate: f32,
    pub lean_rate: f32,
    pub pedal_rate: f32,
}

fn side(round_off: f32, drop_off: f32, scale: f32, speed_scale: f32) -> SideCurve {
    SideCurve {
        round_off,
        drop_off,
        scale,
        speed_scale,
    }
}

impl Setup {
    /// The stock Pro 250 (`mx_Pro250A.phy`), friction zone 1 (on-track dirt).
    pub fn pro_250() -> Setup {
        let weight_lb = 420.0;
        let mass = weight_lb / G_FT * SLUG;
        // Bike.vue: the wheel nodes over the model's origin (on the ground);
        // the centre of gravity is 2 ft up.
        let cofg_y = 2.0;
        let axle = |forward: f32, up: f32| Vec3::new(0.0, (up - cofg_y) * FT, forward * FT);
        let common = |spring: f32, damping: f32, force_z: f32, braking: SideCurve| Tyre {
            radius: 1.2 * FT,
            strut: 1.0 * FT,
            max_compression_rate: 20.0 * FT,
            max_decompression_rate: 4.0 * FT,
            spring: spring / FT,
            bounce: damping,
            rebound: damping,
            force_position: Vec3::new(0.0, -0.25 * FT, force_z * FT),
            max_drive: 2.0,
            skid_drive: 1.9,
            side: side(4.0, 1.5, 38.2, 0.005),
            slipping: side(4.0, 1.5, 38.2, 0.005),
            braking,
            max_braking_torque: 200.0 * LBF_FT,
            wheel_friction: 10.0 * LBF_FT,
        };
        // HP at rpm; torque = HP · 5252 / rpm (lbf·ft).
        let hp = [
            (1000.0, 15.0),
            (4000.0, 20.0),
            (5000.0, 25.0),
            (6000.0, 34.0),
            (7000.0, 45.0),
            (7500.0, 49.0),
            (8000.0, 47.0),
            (8500.0, 50.0),
            (9000.0, 47.0),
            (9500.0, 39.0),
        ];
        Setup {
            mass,
            gravity_scale: 1.03,
            inertia: Vec3::new(22.4, 18.4, 10.9) * LBF_FT,
            yaw_rate_limit: 3.0,
            cofg_travel_back: 1.1 * FT,
            cofg_travel_forward: 0.05 * FT,
            front_axle: axle(2.5225, 1.2332),
            rear_axle: axle(-2.6256, 1.3030),
            front: common(1.65, 0.3, 0.75, side(4.0, 1.6, 37.0, 0.0035)),
            rear: common(2.55, 0.4, -0.75, side(4.0, 1.6, 37.0, 0.0035)),
            max_steer: 30.0 * DEG,
            steer_speed: [0.0, 75.0 * MPH],
            steer_scale: [0.9, 0.3],
            torque_curve: hp.iter().map(|&(rpm, hp)| (rpm, hp * 5252.0 / rpm * LBF_FT)).collect(),
            gears: vec![1.8, 1.47, 1.21, 1.0, 0.869],
            final_ratio: 10.538,
            min_rpm: 4500.0,
            idle_throttle: 0.01,
            engine_braking: 0.000_000_05 * LBF_FT,
            engine_inertia: 0.005 * LBF_FT,
            clutch_static: 70.0 * LBF_FT,
            clutch_kinetic: 60.0 * LBF_FT,
            upshift_rpm: 8500.0,
            downshift_rpm: 6500.0,
            reverse_speed: -10.0 * MPH,
            // 30 lbf per ft/s.
            reverse_gain: 30.0 * LBF / FT,
            max_roll: 45.0 * DEG,
            max_roll_speed: 35.0 * MPH,
            roll_gain: 5.0,
            stand_upright: [10.0 * MPH, 30.0 * MPH],
            max_pitch: 40.0 * DEG,
            pitch_gain: 0.8,
            pitch_rate_gain: 750.0,
            wheelie_top_speed: 120.0 * MPH,
            max_stoppie_pitch: 88.0 * DEG,
            stoppie_pitch_gain: 1.75,
            stoppie_top_speed: 10.0 * MPH,
            stoppie_yaw_rate_scale: 1.0,
            // 0.5 · 0.025 · v² lbf with v in ft/s.
            drag: 0.025 * LBF / (FT * FT),
            air_damping: 450.0 * LBF_FT,
            air_moment: Vec3::new(-400.0, 500.0, -150.0) * LBF_FT,
            air_force: 125.0 * LBF,
            gas_gyro: 175.0 * LBF_FT,
            brake_gyro: 400.0 * LBF_FT,
            unroll_gain: 1.9,
            unroll_torque: 50.0 * LBF_FT,
            unyaw_gain: 2.3,
            unyaw_torque: 500.0 * LBF_FT,
            steer_rate: 4.0,
            lean_rate: 3.5,
            pedal_rate: 10.0,
        }
    }

    /// Spring rate of a tyre, N/m.
    pub fn spring_rate(&self, t: &Tyre) -> f32 {
        t.spring * self.mass * crate::bike::physics::GRAVITY
    }

    /// Damping of a tyre at `ratio` of critical, N·s/m.
    pub fn damping(&self, t: &Tyre, ratio: f32) -> f32 {
        2.0 * ratio * (self.spring_rate(t) * self.mass).sqrt()
    }

    /// Engine torque from the curve at `rpm`, N·m.
    pub fn curve_torque(&self, rpm: f32) -> f32 {
        let c = &self.torque_curve;
        if rpm <= c[0].0 {
            return c[0].1;
        }
        for w in c.windows(2) {
            if rpm <= w[1].0 {
                return lerp_clamp(rpm, w[0].0, w[0].1, w[1].0, w[1].1);
            }
        }
        c[c.len() - 1].1
    }

    pub fn max_rpm(&self) -> f32 {
        self.torque_curve[self.torque_curve.len() - 1].0
    }

    /// Engine turns per wheel turn in `gear` (0-based).
    pub fn ratio(&self, gear: usize) -> f32 {
        self.final_ratio * self.gears[gear.min(self.gears.len() - 1)]
    }
}

/// rad/s to rpm.
pub const RPM: f32 = 60.0 / std::f32::consts::TAU;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_grip_peaks_near_max_side_force() {
        // MaxSideForce1 is 4.9: the curve should top out about there.
        let c = Setup::pro_250().front.side;
        let peak = (1..400).map(|v| -c.grip(v as f32)).fold(0.0, f32::max);
        assert!((4.6..5.3).contains(&peak), "peak {peak}");
        // Pushes back against the slide, both ways.
        assert!(c.grip(10.0) < 0.0 && c.grip(-10.0) > 0.0);
    }

    #[test]
    fn mass_and_springs() {
        let s = Setup::pro_250();
        assert!((s.mass - 190.5).abs() < 0.5, "{}", s.mass);
        // Static sag under its share of the weight: a few centimetres.
        let k = s.spring_rate(&s.front);
        let sag = s.mass * 9.81 * 0.5 / k;
        assert!((0.05..0.12).contains(&sag), "front sag {sag}");
    }

    #[test]
    fn peak_torque_at_the_wheel() {
        let s = Setup::pro_250();
        let peak = (4500..9500).step_by(100).map(|r| s.curve_torque(r as f32)).fold(0.0, f32::max);
        // About 34 lbf·ft at 7500 rpm (49 hp), within the rev range.
        assert!((44.0..48.0).contains(&peak), "{peak}");
        assert!((s.ratio(0) - 18.97).abs() < 0.05);
    }
}
