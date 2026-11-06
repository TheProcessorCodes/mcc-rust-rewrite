//! Conversion at the engine boundary (spec 1). Reflex runs in ft with +X right, +Y up, +Z forward (left-handed);
//! Bevy in m with +X right, +Y up, -Z forward (right-handed). The map is a mirror of Z plus the unit scale, so a
//! rotation q = (x, y, z, w) maps to (-x, -y, z, w) both ways, and an axial vector (angular velocity) to
//! (-x, -y, z).

use bevy::math::{Quat, Vec3};
use reflex_core::FT_TO_M;

type RVec3 = reflex_core::Vec3;
type RQuat = reflex_core::Quat;

/// Bevy position (m) to Reflex (ft).
pub fn pos_to_reflex(v: Vec3) -> RVec3 {
    let s = 1.0 / FT_TO_M;
    RVec3 { x: v.x as f64 * s, y: v.y as f64 * s, z: -(v.z as f64) * s }
}

/// Reflex position or velocity (ft, ft/s) to Bevy (m, m/s).
pub fn pos_to_bevy(v: RVec3) -> Vec3 {
    Vec3::new((v.x * FT_TO_M) as f32, (v.y * FT_TO_M) as f32, (-v.z * FT_TO_M) as f32)
}

/// A Reflex direction (unit vector) in Bevy's frame.
pub fn dir_to_bevy(v: RVec3) -> Vec3 {
    Vec3::new(v.x as f32, v.y as f32, -v.z as f32)
}

/// A Bevy direction in Reflex's frame.
pub fn dir_to_reflex(v: Vec3) -> RVec3 {
    RVec3 { x: v.x as f64, y: v.y as f64, z: -(v.z as f64) }
}

/// A Reflex angular velocity (rad/s, an axial vector) in Bevy's frame.
pub fn angvel_to_bevy(w: RVec3) -> Vec3 {
    Vec3::new(-w.x as f32, -w.y as f32, w.z as f32)
}

pub fn quat_to_reflex(q: Quat) -> RQuat {
    RQuat::new(-q.x as f64, -q.y as f64, q.z as f64, q.w as f64).normalize()
}

pub fn quat_to_bevy(q: RQuat) -> Quat {
    Quat::from_xyzw(-q.x as f32, -q.y as f32, q.z as f32, q.w as f32).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forward_maps_to_minus_z_and_rotations_agree() {
        let f = pos_to_bevy(RVec3 { x: 0.0, y: 0.0, z: 1.0 / FT_TO_M });
        assert!((f - Vec3::NEG_Z).length() < 1e-6);
        // a Reflex yaw turning +Z toward +X turns Bevy's -Z toward +X
        let q = RQuat::from_axis_angle(RVec3::Y, std::f64::consts::FRAC_PI_2);
        assert!((q.rotate(RVec3::Z) - RVec3::X).length() < 1e-9);
        let b = quat_to_bevy(q);
        assert!((b * Vec3::NEG_Z - Vec3::X).length() < 1e-5);
        assert!(quat_to_reflex(b).dot(q).abs() > 1.0 - 1e-6);
        // rotating a point agrees in both frames
        let p = RVec3 { x: 0.3, y: -0.2, z: 1.7 };
        let q2 = RQuat::from_axis_angle(RVec3 { x: 0.3, y: 0.8, z: -0.5 }.normalize(), 0.9);
        let lhs = pos_to_bevy(q2.rotate(p));
        let rhs = quat_to_bevy(q2) * pos_to_bevy(p);
        assert!((lhs - rhs).length() < 1e-5);
    }
}
