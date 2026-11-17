//! The four-body bike (spec 7, `model\loop-integrator.md` 3): 0 chassis, 1 rider, 2 front wheel, 3 rear wheel.
//! Rider = fixed joint; each wheel = sliding rotor on the chassis. Fork, legs and swingarm are geometry only.

use crate::math::{v3, Quat, Vec3};
use crate::multibody::{Body, Joint, System};
use crate::vehicle::Vehicle;

pub const CHASSIS: usize = 0;
pub const RIDER: usize = 1;
pub const FRONT: usize = 2;
pub const REAR: usize = 3;

pub const RIDER_JOINT: usize = 0;
pub const FRONT_JOINT: usize = 1;
pub const REAR_JOINT: usize = 2;

/// Chassis-frame suspension geometry derived from the vehicle keys (OpenReflex build_vehicle_sim.c `0043b750`).
/// The fork and swingarm attachment points have `MassProps_MainBodyCMOffset` subtracted, so the chassis origin is
/// its CM; the rider weld and the rear spring's setup height do not.
#[derive(Clone, Debug)]
pub struct Geometry {
    pub cm_offset: Vec3,
    /// Rider weld at rest: `Rider_RiderCenterOfMass` as it stands, without the CM offset (`0043b750` S+6060 =
    /// S+5540; slot 9 `0044c170` rewrites it every step as the same value plus the lean offsets).
    pub rider_rest: Vec3,
    /// Fork top (steering head) and the fork axis, pointing up the fork (Y rotated back by the rake).
    pub fork_top: Vec3,
    pub fork_axis: Vec3,
    /// Rake rotation about X mapping chassis Y onto the fork axis; the front joint's base frame is
    /// steer (about the fork axis) x rake.
    pub rake_q: Quat,
    pub front_rest_length: f64,
    /// Swingarm pivot and length; the rear slider runs along chassis Y through the rear spring top.
    pub swingarm_pivot: Vec3,
    pub swingarm_length: f64,
    /// Rear spring top height as slot 16 (`0043e200`) places it every step: `SpringYAttachmentPoint` - CM y.
    pub rear_spring_top_y: f64,
    /// The same height as the setup and the spawn (`0043b750`, `0044a650`) place the rear slider before slot 16
    /// first runs: `SpringYAttachmentPoint` without the CM offset. The spawned rear wheel hangs from it.
    pub rear_spring_top_y_setup: f64,
    pub rear_rest_length: f64,
}

impl Geometry {
    pub fn from_vehicle(v: &Vehicle) -> Self {
        let cm = Vec3::from_array(v.vec3("MassProps_MainBodyCMOffset"));
        let rake = v.scalar("FrontSuspension_RakeAngle");
        Geometry {
            cm_offset: cm,
            rider_rest: Vec3::from_array(v.vec3("Rider_RiderCenterOfMass")),
            fork_top: Vec3::from_array(v.vec3("FrontSuspension_ForkAttachmentPoint")) - cm,
            fork_axis: v3(0.0, rake.cos(), -rake.sin()),
            rake_q: Quat::from_axis_angle(Vec3::X, -rake),
            front_rest_length: v.scalar("FrontSuspension_SpringRestLength"),
            swingarm_pivot: Vec3::from_array(v.vec3("RearSuspension_SwingArmMainBodyAttachmentVector")) - cm,
            swingarm_length: v.scalar("RearSuspension_SwingArmLength"),
            rear_spring_top_y: v.scalar("RearSuspension_SpringYAttachmentPoint") - cm.y,
            rear_spring_top_y_setup: v.scalar("RearSuspension_SpringYAttachmentPoint"),
            rear_rest_length: v.scalar("RearSuspension_SpringRestLength"),
        }
    }

    /// Rear slider anchor (spring top) for the swingarm straight back, at slot 16's height. Slot 16 re-aims it
    /// every step from the arc about the pivot.
    pub fn rear_anchor(&self) -> Vec3 {
        v3(self.swingarm_pivot.x, self.rear_spring_top_y, self.swingarm_pivot.z - self.swingarm_length)
    }

    /// The rear slider anchor as the setup / spawn write it (S+77d0 in `0043b750` / `0044a650`): swingarm straight
    /// back, height `SpringYAttachmentPoint` without the CM offset.
    pub fn rear_anchor_setup(&self) -> Vec3 {
        v3(self.swingarm_pivot.x, self.rear_spring_top_y_setup, self.swingarm_pivot.z - self.swingarm_length)
    }
}

/// Builds the four bodies and three joints at rest lengths, chassis at `x` / `q`, everything at rest, the way the
/// spawn places them (OpenReflex build_vehicle_place.c `0044a650`): rear wheel at the setup anchor minus the rest
/// length along chassis Y, front wheel at the fork top minus the rest length along the fork, rider and both
/// wheels with the chassis orientation; then slot 16 (front steer frame, rear anchor on the swingarm arc) and the
/// joint pose projection (node slot 6), which welds the rider and puts the wheels on their slider lines.
pub fn build_system(v: &Vehicle, x: Vec3, q: Quat) -> (System, Geometry) {
    let g = Geometry::from_vehicle(v);
    let mut sys = System::new(Vec3::from_array(v.vec3("VehicleSim_Gravity")));
    let body = |m: &str, i: &str| Body::new(v.scalar(m), Vec3::from_array(v.vec3(i)));
    sys.add_body(body("MassProps_MainBodyMass", "MassProps_MainBodyInertia"));
    sys.add_body(body("MassProps_RiderBodyMass", "MassProps_RiderBodyInertia"));
    sys.add_body(body("MassProps_FrontTireMass", "MassProps_FrontTireInertia"));
    sys.add_body(body("MassProps_RearTireMass", "MassProps_RearTireInertia"));

    sys.joints.push(Joint::Fixed { parent: CHASSIS, child: RIDER, offset: g.rider_rest, rel: Quat::IDENTITY });
    sys.joints.push(Joint::SlidingRotor {
        parent: CHASSIS,
        child: FRONT,
        anchor: g.fork_top,
        axis: g.fork_axis,
        base: g.rake_q,
    });
    sys.joints.push(Joint::SlidingRotor {
        parent: CHASSIS,
        child: REAR,
        anchor: g.rear_anchor_setup(),
        axis: Vec3::Y,
        base: Quat::IDENTITY,
    });

    // `0044a650`: the rider starts at the chassis position (node slot 6 below moves it onto the weld); the
    // front wheel's orientation is the chassis's (the projection then keeps the twist that undoes the rake, so
    // it stays the chassis's).
    let local = [
        Vec3::ZERO,
        Vec3::ZERO,
        g.fork_top - g.fork_axis * g.front_rest_length,
        g.rear_anchor_setup() - Vec3::Y * g.rear_rest_length,
    ];
    for (b, l) in sys.bodies.iter_mut().zip(local) {
        b.x = x + q.rotate(l);
        b.q = q;
    }
    // slot 16 then node slot 6, as the spawn calls them
    crate::suspension::geometry(&mut sys, &g, 0.0);
    sys.project_positions();
    sys.save_prev_poses();
    (sys, g)
}
