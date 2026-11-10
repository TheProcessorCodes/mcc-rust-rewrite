//! Master Chief's own skeleton as the rider's rig, for when Skate 3 is not
//! set up ([`HaloRig`]): each skater bone the rider maps (`MAPPING`) put on
//! its Halo joint, in skate metres, so on foot and on the bike the Spartan
//! is posed with his own proportions.
//!
//! Off the board only the joints' places matter: foot and bike pose a bone
//! as a move times its bind, and the rider takes the bind off again, so the
//! binds here are places alone (no turn). With every skate joint on its Halo
//! joint the retarget (`follow`) leaves the Spartan as he is: he faces the
//! way the rider turns him anyway (a quarter turn about Y), every segment
//! already points the right way, and his feet are already flat.
//!
//! Made from the model the map loads from the player's MCC, as each map
//! loads; nothing in it comes from Skate 3 and nothing is written to disk.

use std::f32::consts::FRAC_PI_2;

use bevy::math::{Mat4, Quat, Vec3};
use mcc_core::skate::HaloRig;

use super::{ChiefModel, MAPPING, halo_to_skate};

/// The skater's bones that sit between two Halo joints, put halfway: foot
/// and bike bend them with their neighbours. Skate bone, Halo joints.
const BETWEEN: [(&str, &str, &str); 3] = [
    ("SPINE1", "spine", "spine1"),
    ("SPINE3", "spine1", "neck"),
    ("NECK1", "neck", "head"),
];

/// The board's bones. Off the board only their names count: foot and bike
/// put them under him every frame, for `light_rider` and the `--shot`
/// rider views.
const BOARD: [&str; 3] = ["SKATEBOARD_ROOT", "TRUCK_FRONT", "TRUCK_BACK"];

/// The two-bone limbs foot and bike bend (Halo joints): each segment needs
/// some length.
const LIMBS: [(&str, &str); 8] = [
    ("l_upperarm", "l_forearm"),
    ("l_forearm", "l_hand"),
    ("r_upperarm", "r_forearm"),
    ("r_forearm", "r_hand"),
    ("l_thigh", "l_calf"),
    ("l_calf", "l_foot"),
    ("r_thigh", "r_calf"),
    ("r_calf", "r_foot"),
];

/// The shortest a limb segment may be, rig metres.
const MIN_LIMB: f32 = 0.01;

/// The closest a mapped bone may be to the child it is aligned with, rig
/// metres: the retarget needs the segment's direction.
const MIN_SEGMENT: f32 = 0.001;

/// Halo model space (units, Z up, facing +X, left +Y) to the rig (skate
/// metres at scale `k`, Y up, facing +Z, left +X): `halo_to_skate`, then
/// the quarter turn the rider falls back to for his facing.
pub(crate) fn to_rig(k: f32) -> Mat4 {
    Mat4::from_quat(Quat::from_rotation_y(-FRAC_PI_2)) * halo_to_skate(k)
}

/// The rider's rig from Halo joints, by node name with the bind pose in
/// model space: the 21 bones the rider maps on their joints, SPINE1,
/// SPINE3 and NECK1 between theirs, and the board's three at the origin.
/// Skate metres at scale `k`; the soles land at y = 0, as the model's origin
/// is between his feet. Fails, naming the bone, when a joint is missing or
/// not finite, sits on the child it is aligned with, or a limb has no
/// length.
pub fn halo_rig(nodes: &[(&str, Mat4)], k: f32) -> Result<HaloRig, String> {
    let to_rig = to_rig(k);
    let at = |halo: &str| -> Result<Vec3, String> {
        let (_, world) = nodes
            .iter()
            .find(|(n, _)| *n == halo)
            .ok_or_else(|| format!("no {halo} bone"))?;
        let p = to_rig.transform_point3(world.w_axis.truncate());
        if p.is_finite() { Ok(p) } else { Err(format!("the {halo} bone is not finite")) }
    };
    let count = MAPPING.len() + BETWEEN.len() + BOARD.len();
    let (mut names, mut binds) = (Vec::with_capacity(count), Vec::with_capacity(count));
    for (halo, skate, child) in MAPPING {
        let p = at(halo)?;
        if let Some((halo_child, _)) = child
            && at(halo_child)?.distance(p) < MIN_SEGMENT
        {
            return Err(format!("the {halo} and {halo_child} bones are in the same place"));
        }
        names.push(skate.to_string());
        binds.push(Mat4::from_translation(p));
    }
    for (a, b) in LIMBS {
        if at(a)?.distance(at(b)?) < MIN_LIMB {
            return Err(format!("the {a} bone is too short"));
        }
    }
    for (skate, a, b) in BETWEEN {
        names.push(skate.to_string());
        binds.push(Mat4::from_translation((at(a)? + at(b)?) * 0.5));
    }
    for skate in BOARD {
        names.push(skate.to_string());
        binds.push(Mat4::IDENTITY);
    }
    Ok(HaloRig { names, binds })
}

/// [`halo_rig`] from the Spartan's own skeleton.
pub fn chief_rig(chief: &ChiefModel, k: f32) -> Result<HaloRig, String> {
    let nodes: Vec<(&str, Mat4)> = chief.parts.bones.iter().map(|b| (b.name.as_str(), b.world)).collect();
    halo_rig(&nodes, k)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blam_geometry::Bone;
    use mcc_core::skate::DEFAULT_SCALE;

    /// A made-up biped (our own round numbers, not any game's): name,
    /// parent and model-space joint, Halo units, X forward, Y left, Z up,
    /// the origin between the feet. Parents come first, as in a model.
    const BIPED: [(&str, Option<u16>, [f32; 3]); 21] = [
        ("pelvis", None, [0.0, 0.0, 0.33]),
        ("l_thigh", Some(0), [0.0, 0.035, 0.31]),
        ("r_thigh", Some(0), [0.0, -0.035, 0.31]),
        ("spine", Some(0), [0.0, 0.0, 0.37]),
        ("l_calf", Some(1), [0.005, 0.04, 0.17]),
        ("r_calf", Some(2), [0.005, -0.04, 0.17]),
        ("spine1", Some(3), [0.0, 0.0, 0.45]),
        ("l_foot", Some(4), [-0.005, 0.04, 0.03]),
        ("r_foot", Some(5), [-0.005, -0.04, 0.03]),
        ("l_clavicle", Some(6), [0.0, 0.02, 0.55]),
        ("neck", Some(6), [0.0, 0.0, 0.57]),
        ("r_clavicle", Some(6), [0.0, -0.02, 0.55]),
        ("l_toe", Some(7), [0.05, 0.04, 0.005]),
        ("r_toe", Some(8), [0.05, -0.04, 0.005]),
        ("l_upperarm", Some(9), [0.0, 0.08, 0.55]),
        ("head", Some(10), [0.005, 0.0, 0.61]),
        ("r_upperarm", Some(11), [0.0, -0.08, 0.55]),
        ("l_forearm", Some(14), [0.0, 0.09, 0.45]),
        ("r_forearm", Some(16), [0.0, -0.09, 0.45]),
        ("l_hand", Some(17), [0.01, 0.09, 0.35]),
        ("r_hand", Some(18), [0.01, -0.09, 0.35]),
    ];

    /// The biped as model bones, each turned some way of its own (Halo's
    /// bones are), which the rig must not take on.
    fn biped() -> Vec<Bone> {
        let mut bones: Vec<Bone> = Vec::new();
        for (i, (name, parent, p)) in BIPED.iter().enumerate() {
            let turn = Quat::from_rotation_z(0.3 * i as f32) * Quat::from_rotation_x(0.1 * i as f32);
            let world = Mat4::from_rotation_translation(turn, Vec3::from(*p));
            let parent_world = parent.map_or(Mat4::IDENTITY, |p| bones[p as usize].world);
            let (_, rotation, position) = (parent_world.inverse() * world).to_scale_rotation_translation();
            bones.push(Bone {
                name: name.to_string(),
                parent: *parent,
                position,
                rotation,
                world,
            });
        }
        bones
    }

    fn nodes(bones: &[Bone]) -> Vec<(&str, Mat4)> {
        bones.iter().map(|b| (b.name.as_str(), b.world)).collect()
    }

    fn at(rig: &HaloRig, name: &str) -> Vec3 {
        let i = rig.names.iter().position(|n| n == name).unwrap_or_else(|| panic!("no {name}"));
        rig.binds[i].w_axis.truncate()
    }

    #[test]
    fn every_bone_once_placed_without_a_turn() {
        let rig = halo_rig(&nodes(&biped()), DEFAULT_SCALE).expect("a rig");
        assert_eq!(rig.names.len(), 27);
        assert_eq!(rig.binds.len(), 27);
        for (_, skate, _) in MAPPING {
            assert_eq!(rig.names.iter().filter(|n| *n == skate).count(), 1, "{skate}");
        }
        for (name, m) in rig.names.iter().zip(&rig.binds) {
            assert!(m.is_finite(), "{name}");
            let turn = bevy::math::Mat3::from_mat4(*m);
            assert!(turn.abs_diff_eq(bevy::math::Mat3::IDENTITY, 0.0), "{name} is turned");
        }
    }

    #[test]
    fn faces_forward_with_the_left_at_plus_x_and_y_up() {
        let rig = halo_rig(&nodes(&biped()), DEFAULT_SCALE).expect("a rig");
        assert!(at(&rig, "LEFTUPLEG").x > 0.0 && at(&rig, "RIGHTUPLEG").x < 0.0);
        assert!(at(&rig, "LEFTTOEBASE").z > at(&rig, "LEFTFOOT").z);
        assert!(at(&rig, "HEAD").y > at(&rig, "NECK").y && at(&rig, "NECK").y > at(&rig, "HIPS").y);
        // Halo units to skate metres: 0.33 units of hip height.
        let hips = 0.33 * blam_geometry::WORLD_UNIT_METRES * DEFAULT_SCALE;
        assert!((at(&rig, "HIPS").y - hips).abs() < 1e-5);
        // Halfway bones halfway.
        let spine1 = (at(&rig, "SPINE").y + at(&rig, "SPINE2").y) * 0.5;
        assert!((at(&rig, "SPINE1").y - spine1).abs() < 1e-5);
    }

    #[test]
    fn a_missing_or_shapeless_limb_is_refused() {
        let mut bones = biped();
        bones.retain(|b| b.name != "l_calf");
        let e = halo_rig(&nodes(&bones), DEFAULT_SCALE).expect_err("no calf");
        assert!(e.contains("l_calf"), "{e}");

        let mut bones = biped();
        let thigh = bones[1].world;
        bones[4].world = thigh;
        let e = halo_rig(&nodes(&bones), DEFAULT_SCALE).expect_err("calf on the thigh");
        assert!(e.contains("l_thigh"), "{e}");

        let mut bones = biped();
        bones[19].world.w_axis.x = f32::NAN;
        let e = halo_rig(&nodes(&bones), DEFAULT_SCALE).expect_err("hand not finite");
        assert!(e.contains("l_hand"), "{e}");
    }

    /// The point of the rig: on it the retarget changes nothing, so every
    /// mapped joint at its bind pose is the Spartan's own joint, turned to
    /// face +Z, in skate metres.
    #[test]
    fn the_retarget_leaves_him_as_he_is() {
        let bones = biped();
        let k = DEFAULT_SCALE;
        let rig = halo_rig(&nodes(&bones), k).expect("a rig");
        let bind = |name: &str| rig.names.iter().position(|n| n == name).map(|i| rig.binds[i]);
        let follow = crate::follow(&bones, bind, k);
        let to_rig = to_rig(k);
        for (bone, f) in bones.iter().zip(&follow) {
            let Some((target, delta)) = f else {
                panic!("{} follows nothing", bone.name);
            };
            let skate = MAPPING.iter().find(|m| m.0 == bone.name).map(|m| m.1);
            assert_eq!(Some(target.as_str()), skate, "{}", bone.name);
            let joint = bind(target).expect("bound") * *delta * bone.world;
            let own = to_rig * bone.world;
            assert!(joint.abs_diff_eq(own, 1e-4), "{}: {joint} against {own}", bone.name);
        }
    }
}
