//! Typed vehicle values, read once from a [`Vehicle`] so the substep does no key lookups.
//! Units as stored (spec 1). Per-surface keys stay as (Sticky, Normal, Slippery) triples for [`blend3`].

use crate::curve::{blend3, Curve};
use crate::math::Vec3;
use crate::vehicle::Vehicle;

/// Per-tyre keys (`FrontTire_*` / `RearTire_*`).
#[derive(Clone, Debug)]
pub struct TyreParams {
    pub radius: f64,
    pub static_mu_roll: [f64; 3],
    pub static_mu_side: [f64; 3],
    pub dynamic_mu_roll: [f64; 3],
    pub dynamic_mu_side: [f64; 3],
    pub side_slip_drag: [f64; 3],
    pub terrain_drag: [f64; 3],
    pub terrain_velocity_drag: [f64; 3],
    /// Rear only (front: zeros).
    pub doughnut: [f64; 3],
    pub static_mu_roll_curve: Curve,
    pub static_mu_side_curve: Curve,
    pub side_slip_drag_curve: Curve,
    pub lean_angle_mu_side_curve: Curve,
    pub rider_shift_mu_roll_curve: Curve,
    pub rider_shift_mu_side_curve: Curve,
}

/// Per-axle suspension keys.
#[derive(Clone, Debug)]
pub struct SuspensionParams {
    pub k_spring: f64,
    pub damp_compression: f64,
    pub damp_decompression: f64,
    pub rest_length: f64,
    pub bottom_out_length: f64,
}

/// Garage multipliers (sim+0x2ef0..0x2efc), 1.0 = stock. They scale the values the sim runs on: suspension
/// spring and damper rates, overall gear ratios, the brake time constant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Garage {
    pub suspension: f64,
    pub gearing: f64,
    pub brakes: f64,
}

impl Default for Garage {
    fn default() -> Self {
        Garage { suspension: 1.0, gearing: 1.0, brakes: 1.0 }
    }
}

#[derive(Clone, Debug)]
pub struct Params {
    pub air: crate::air::AirParams,
    pub gravity: Vec3,
    pub tyres: [TyreParams; 2],
    pub suspension: [SuspensionParams; 2],
    pub speed_filter_tau: f64,
    pub up_vector_tau: f64,
    pub brake_time_constant: f64,
    pub front_brake_max: f64,
    pub rear_brake_max: f64,
    pub front_brake_curve: Curve,
    pub rear_brake_curve: Curve,
    pub rolling_friction_curve: Curve,
    /// Overall ratio per gear (GearRatio_i x PrimaryReduction x FinalRatio; garage gearing scale 1).
    pub gear_ratios: [f64; 5],
    pub torque_curve: Curve,
    pub drag_torque_curve: Curve,
    pub engine_drag_coefficient: f64,
    pub max_drag_torque: f64,
    /// rad/s
    pub engine_max_speed: f64,
    pub engine_min_speed: f64,
    pub peak_clutch_speed: f64,
    pub clutch_reset_time: f64,
    pub engine_inertia: f64,
    pub drivetrain_inertia: f64,
    pub rear_tire_inertia: Vec3,
    pub min_torque_feedback: f64,
    pub max_torque_feedback: f64,
    pub steering_tau: f64,
    pub lean_tau: f64,
    pub max_steer_curve: Curve,
    pub theta_lean_max_curve: Curve,
    pub ground_drag_curve: Curve,
    pub air_drag_curve: Curve,
    pub rider_com: Vec3,
    pub rider_lean_fb: f64,
    pub rider_lean_lr: f64,
}

fn triple(v: &Vehicle, key: &str) -> [f64; 3] {
    match v.get(key) {
        Some(crate::Value::Triple(t)) => *t,
        _ => [v.scalar(key); 3],
    }
}

fn curve(v: &Vehicle, key: &str) -> Curve {
    v.curve(key).cloned().unwrap_or_else(|| Curve::new(&[(0.0, 1.0), (1.0, 1.0), (2.0, 1.0)], 0.0, 0.0))
}

impl TyreParams {
    fn load(v: &Vehicle, p: &str) -> Self {
        let k = |s: &str| format!("{p}Tire_{s}");
        TyreParams {
            radius: v.scalar(&k("Radius")),
            static_mu_roll: triple(v, &k("StaticMuRoll")),
            static_mu_side: triple(v, &k("StaticMuSideSlip")),
            dynamic_mu_roll: triple(v, &k("DynamicMuRollMultiplier")),
            dynamic_mu_side: triple(v, &k("DynamicMuSideMultiplier")),
            side_slip_drag: triple(v, &k("SideSlipDragForce")),
            terrain_drag: triple(v, &k("TerrainDrag")),
            terrain_velocity_drag: triple(v, &k("TerrainVelocityDrag")),
            doughnut: if p == "Rear" { triple(v, &k("DoughnutForce")) } else { [0.0; 3] },
            static_mu_roll_curve: curve(v, &k("StaticMuRollCurve")),
            static_mu_side_curve: curve(v, &k("StaticMuSideSlipCurve")),
            side_slip_drag_curve: curve(v, &k("SideSlipDragForceCurve")),
            lean_angle_mu_side_curve: curve(v, &k("LeanAngleMuSideModifierCurve")),
            rider_shift_mu_roll_curve: curve(v, &k("RiderShiftMuRollModifierCurve")),
            rider_shift_mu_side_curve: curve(v, &k("RiderShiftMuSideModifierCurve")),
        }
    }

    pub fn mu(a: &[f64; 3], grip_index: f64) -> f64 {
        blend3(a, grip_index)
    }
}

impl SuspensionParams {
    fn load(v: &Vehicle, p: &str) -> Self {
        let k = |s: &str| v.scalar(&format!("{p}Suspension_{s}"));
        SuspensionParams {
            k_spring: k("ShockKSpring"),
            damp_compression: k("ShockDampCompression"),
            damp_decompression: k("ShockDampDeCompression"),
            rest_length: k("SpringRestLength"),
            bottom_out_length: k("BottomOutLength"),
        }
    }
}

impl Params {
    /// Values as the sim runs them with garage multipliers applied (`0043be80`).
    pub fn with_garage(v: &Vehicle, g: Garage) -> Self {
        let mut p = Self::from_vehicle(v);
        for s in &mut p.suspension {
            s.k_spring *= g.suspension;
            s.damp_compression *= g.suspension;
            s.damp_decompression *= g.suspension;
        }
        for r in &mut p.gear_ratios {
            *r *= g.gearing;
        }
        p.brake_time_constant *= g.brakes;
        p
    }

    pub fn from_vehicle(v: &Vehicle) -> Self {
        let v3a = |k: &str| v.vec3(k);
        Params {
            air: crate::air::AirParams {
                course_correction: v.scalar("AirBorne_CourseCorrectionForce"),
                damping: v3a("AirBorne_AngularRateDamping"),
                damping_takeoff: v3a("AirBorne_AngularRateDampingOnTakeoff"),
                velocity_curves: [
                    curve(v, "AirBorne_LeftStickXVelocityCurve"),
                    curve(v, "AirBorne_LeftStickYVelocityCurve"),
                    curve(v, "AirBorne_RightStickXVelocityCurve"),
                    curve(v, "AirBorne_RightStickYVelocityCurve"),
                ],
                rpy_tau: v3a("AirBorne_RPYTimeConstants"),
            },
            gravity: Vec3::from_array(v.vec3("VehicleSim_Gravity")),
            tyres: [TyreParams::load(v, "Front"), TyreParams::load(v, "Rear")],
            suspension: [SuspensionParams::load(v, "Front"), SuspensionParams::load(v, "Rear")],
            speed_filter_tau: 1.0,
            up_vector_tau: v.scalar("VehicleSim_UpVectorTimeConstant"),
            brake_time_constant: v.scalar("Brakes_TimeConstant"),
            front_brake_max: v.scalar("Brakes_FrontBrakeMaxTorque"),
            rear_brake_max: v.scalar("Brakes_RearBrakeMaxTorque"),
            front_brake_curve: curve(v, "Brakes_FrontBrakeTorqueCurve"),
            rear_brake_curve: curve(v, "Brakes_RearBrakeTorqueCurve"),
            rolling_friction_curve: curve(v, "Brakes_WheelRollingFrictionTorsionDamping"),
            gear_ratios: {
                let base = v.scalar("DriveTrain_PrimaryReduction") * v.scalar("DriveTrain_FinalRatio");
                std::array::from_fn(|i| v.scalar(&format!("DriveTrain_GearRatio{}", i + 1)) * base)
            },
            torque_curve: curve(v, "Engine_TorqueCurve"),
            drag_torque_curve: curve(v, "Engine_MaximumDragTorqueCurve"),
            engine_drag_coefficient: v.scalar("Engine_DragCoefficient"),
            max_drag_torque: v.scalar("Engine_MaximumDragTorque"),
            engine_max_speed: v.scalar("Engine_MaximumSpeed") / crate::RPM_PER_RAD_S,
            engine_min_speed: v.scalar("Engine_MinimumSpeed") / crate::RPM_PER_RAD_S,
            peak_clutch_speed: v.scalar("Engine_PeakClutchRPM") / crate::RPM_PER_RAD_S,
            clutch_reset_time: v.scalar("Engine_ClutchResetTime"),
            engine_inertia: v.scalar("MassProps_EngineInertia"),
            drivetrain_inertia: v.scalar("MassProps_DriveTrainInertia"),
            rear_tire_inertia: Vec3::from_array(v.vec3("MassProps_RearTireInertia")),
            min_torque_feedback: v.scalar("Engine_MinimumTorqueFeedback"),
            max_torque_feedback: v.scalar("Engine_MaximumTorqueFeedback"),
            steering_tau: v.scalar("VehicleSim_SteeringTimeConstant"),
            lean_tau: v.scalar("VehicleSim_BikeLeanTimeConstant"),
            max_steer_curve: curve(v, "VehicleSim_MaxSteeringAngleCurve"),
            theta_lean_max_curve: curve(v, "VehicleSim_ThetaLeanMaxCurve"),
            ground_drag_curve: curve(v, "VehicleSim_GroundDragCurve"),
            air_drag_curve: curve(v, "VehicleSim_AirBorneDragCurve"),
            rider_com: Vec3::from_array(v.vec3("Rider_RiderCenterOfMass")),
            rider_lean_fb: v.scalar("Rider_LeanForwardBack"),
            rider_lean_lr: v.scalar("Rider_LeanLeftRight"),
        }
    }
}
