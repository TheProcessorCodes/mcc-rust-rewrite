//! Suspension forces (spec 6.4, `0043c590`) and suspension geometry (slot 16, `0043e200`).

use crate::bike::{Geometry, CHASSIS, FRONT, FRONT_JOINT, REAR, REAR_JOINT};
use crate::math::{v3, Quat, Vec3};
use crate::multibody::{Joint, System};
use crate::params::SuspensionParams;
use crate::tyre::Tyre;

/// Rear spring top z, ft (hard-coded in `0043c590`).
pub const REAR_SPRING_TOP_Z: f64 = -2.6;

fn anchor_of(sys: &System, j: usize) -> Vec3 {
    match sys.joints[j] {
        Joint::SlidingRotor { anchor, .. } => anchor,
        _ => unreachable!("joint {j} is not a slider"),
    }
}

/// Current spring lengths (front, rear), ft: from the fork top / rear spring top to the axle, chassis frame.
pub fn lengths(sys: &System, g: &Geometry) -> [f64; 2] {
    let rear_anchor = anchor_of(sys, REAR_JOINT);
    let b0 = &sys.bodies[CHASSIS];
    let d = |wi: usize| b0.q.inv_rotate(sys.bodies[wi].x - b0.x);
    [(d(FRONT) - g.fork_top).length(), (d(REAR) - v3(rear_anchor.x, rear_anchor.y, REAR_SPRING_TOP_Z)).length()]
}

/// Spring + damper along each slider: +F on the wheel at its centre, -F on the chassis at the axle point
/// (OpenReflex bike_force.c `0043c590`, checked: F = ((len - rest) k - c v) u, c = ShockDampCompression for
/// v >= 0 and ShockDampDeCompression for v < 0 (S+8ab0 / S+8ab4 front, S+8aa4 / S+8aa8 rear, garage suspension
/// factor applied by `0043be80`). The original has no bump-stop force or preload here: it only raises the travel
/// flags (W+0x108 below BottomOutLength, read by the tyre and contact stages; W+0x10c past FullyExtendedLength,
/// read by nothing in the simulation, not kept). Sets the bottomed flags.
pub fn forces(sys: &mut System, g: &Geometry, p: &[SuspensionParams; 2], tyres: &mut [Tyre; 2]) {
    let rear_anchor = anchor_of(sys, REAR_JOINT);
    for (i, wi) in [(0, FRONT), (1, REAR)] {
        let (b0, bw) = (sys.bodies[CHASSIS].clone(), sys.bodies[wi].clone());
        let d = b0.q.inv_rotate(bw.x - b0.x);
        let (u_body, top) = if i == 0 {
            (g.fork_axis, g.fork_top)
        } else {
            (Vec3::Y, v3(rear_anchor.x, rear_anchor.y, REAR_SPRING_TOP_Z))
        };
        let len = (d - top).length();
        let u = b0.q.rotate(u_body);
        let v = (bw.v - b0.point_velocity(d)).dot(u);
        let sp = &p[i];
        let c = if v >= 0.0 { sp.damp_compression } else { sp.damp_decompression };
        let f = u * (sp.k_spring * (len - sp.rest_length) - c * v);
        sys.bodies[wi].apply_force(f, Vec3::ZERO);
        sys.bodies[CHASSIS].apply_force(-f, d);
        tyres[i].bottomed = len < sp.bottom_out_length;
    }
}

/// Z of the swingarm direction in the chassis frame from the rear axle's offset (y, z) from the pivot
/// (OpenReflex bike_post.c `0043e200`): the offset normalized in the y-z plane, replaced by straight back
/// (0, 0, -1) when its z is above -0.7 (0097781c; the arm folded over or a zero offset).
pub fn swingarm_dir_z(y: f64, z: f64) -> f64 {
    let l = (y * y + z * z).sqrt();
    let nz = if l != 0.0 && l.is_finite() { z / l } else { 0.0 };
    if nz > -0.7 {
        -1.0
    } else {
        nz
    }
}

/// Slot 16: the front joint's base rotation follows the steer angle about the fork axis; the rear slider
/// anchor is re-placed on the swingarm arc from the current axle position (one step behind).
pub fn geometry(sys: &mut System, g: &Geometry, steer_angle: f64) {
    if let Joint::SlidingRotor { base, .. } = &mut sys.joints[FRONT_JOINT] {
        *base = Quat::from_axis_angle(g.fork_axis, steer_angle) * g.rake_q;
    }
    let (b0, br) = (&sys.bodies[CHASSIS], &sys.bodies[REAR]);
    let d = b0.q.inv_rotate(br.x - b0.x) - g.swingarm_pivot;
    let dir_z = swingarm_dir_z(d.y, d.z);
    let anchor = v3(g.swingarm_pivot.x, g.rear_spring_top_y, g.swingarm_pivot.z + dir_z * g.swingarm_length);
    if let Joint::SlidingRotor { anchor: a, .. } = &mut sys.joints[REAR_JOINT] {
        *a = anchor;
    }
}

#[cfg(test)]
mod tests {
    use super::swingarm_dir_z;

    #[test]
    fn swingarm_direction() {
        // straight back and down-back offsets are kept (normalized)
        assert_eq!(swingarm_dir_z(0.0, -2.0), -1.0);
        let z = swingarm_dir_z(-1.0, -1.0);
        assert!((z + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        // above -0.7: replaced by straight back
        assert_eq!(swingarm_dir_z(-1.0, -0.5), -1.0);
        assert_eq!(swingarm_dir_z(1.0, 0.2), -1.0);
        assert_eq!(swingarm_dir_z(0.0, 0.0), -1.0);
    }
}
