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

/// Chassis-frame suspension geometry derived from the vehicle keys (`0043b750`). Attachment points have
/// `MassProps_MainBodyCMOffset` subtracted, so the chassis origin is its CM.
#[derive(Clone, Debug)]
pub struct Geometry {
    pub cm_offset: Vec3,
    /// Rider weld at rest (`Rider_RiderCenterOfMass` - CM offset [L: subtraction not traced for the rider]).
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
    pub rear_spring_top_y: f64,
    pub rear_rest_length: f64,
}

impl Geometry {
    pub fn from_vehicle(v: &Vehicle) -> Self {
        let cm = Vec3::from_array(v.vec3("MassProps_MainBodyCMOffset"));
        let rake = v.scalar("FrontSuspension_RakeAngle");
        Geometry {
            cm_offset: cm,
            rider_rest: Vec3::from_array(v.vec3("Rider_RiderCenterOfMass")) - cm,
            fork_top: Vec3::from_array(v.vec3("FrontSuspension_ForkAttachmentPoint")) - cm,
            fork_axis: v3(0.0, rake.cos(), -rake.sin()),
            rake_q: Quat::from_axis_angle(Vec3::X, -rake),
            front_rest_length: v.scalar("FrontSuspension_SpringRestLength"),
            swingarm_pivot: Vec3::from_array(v.vec3("RearSuspension_SwingArmMainBodyAttachmentVector")) - cm,
            swingarm_length: v.scalar("RearSuspension_SwingArmLength"),
            rear_spring_top_y: v.scalar("RearSuspension_SpringYAttachmentPoint") - cm.y,
            rear_rest_length: v.scalar("RearSuspension_SpringRestLength"),
        }
    }

    /// Rear slider anchor (spring top) for the swingarm straight back. Slot 16 re-aims it every step from the
    /// arc about the pivot (milestone 4).
    pub fn rear_anchor(&self) -> Vec3 {
        v3(self.swingarm_pivot.x, self.rear_spring_top_y, self.swingarm_pivot.z - self.swingarm_length)
    }
}

/// Builds the four bodies and three joints at rest lengths, chassis at `x` / `q`, everything at rest.
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
        anchor: g.rear_anchor(),
        axis: Vec3::Y,
        base: Quat::IDENTITY,
    });

    let local = [
        Vec3::ZERO,
        g.rider_rest,
        g.fork_top - g.fork_axis * g.front_rest_length,
        g.rear_anchor() - Vec3::Y * g.rear_rest_length,
    ];
    for (i, (b, l)) in sys.bodies.iter_mut().zip(local).enumerate() {
        b.x = x + q.rotate(l);
        b.q = if i == FRONT { q * g.rake_q } else { q };
        b.prev_x = b.x;
        b.prev_q = b.q;
    }
    (sys, g)
}
