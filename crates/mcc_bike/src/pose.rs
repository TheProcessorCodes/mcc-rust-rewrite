//! The rider on the bike, as a skate skeleton pose so `rider.rs` can draw
//! the Spartan on it unchanged.
//!
//! One stiff stance, the motocross attack position: standing on the pegs,
//! knees bent, chest over the tank, hands on the grips. The hips move with
//! the right stick and the torso tilts with them; two-bone IK keeps the
//! feet on the pegs and the hands on the grips (which turn with the bars).
//!
//! The rig (Skate 3's `rig.json`, or without Skate 3 Master Chief's own
//! skeleton, `mcc_core::skate::HaloRig`; skate metres) faces +Z with its
//! left at +X and Y up: the bike's own frame. Poses come out in skate metres
//! in the bike frame; `mod.rs` places them in the world.

use std::collections::HashMap;

use bevy::math::{Mat4, Quat, Vec2, Vec3};
use serde::Deserialize;

use super::physics::{HIPS, PEG};

#[derive(Deserialize)]
struct RigBone {
    name: String,
    bind: [f64; 16],
}

/// The rig's bind pose: Skate 3's skater, or Master Chief's own skeleton.
pub struct Rig {
    pub names: Vec<String>,
    bind: Vec<Mat4>,
    index: HashMap<String, usize>,
}

/// Where the rider's hands and feet go and how he leans, bike frame,
/// render metres.
pub struct Stance {
    pub grips: [Vec3; 2],
    /// The rider's weight shift, -1..1: X right, Y forward.
    pub lean: Vec2,
    /// The bike's axles and the ground under them, for the board bones
    /// other code reads (`TRUCK_*`, `SKATEBOARD_ROOT`).
    pub contacts: [Vec3; 2],
}

/// The torso's forward tilt at rest, radians.
const TORSO: f32 = 0.78;

/// The rider's size on the bike against the Spartan's own (7 ft, 2.1 m):
/// rider-sized (about 1.8 m) on the real-sized 450F. Only the bike draws
/// him smaller; on foot and on the board he keeps his size.
pub const RIDER_SIZE: f32 = 0.85;

/// The bones the poses need: the trunk and both sides' limbs.
const REQUIRED: [&str; 16] = [
    "HIPS",
    "SPINE",
    "NECK",
    "HEAD",
    "LEFTARM",
    "LEFTFOREARM",
    "LEFTHAND",
    "LEFTUPLEG",
    "LEFTLEG",
    "LEFTFOOT",
    "RIGHTARM",
    "RIGHTFOREARM",
    "RIGHTHAND",
    "RIGHTUPLEG",
    "RIGHTLEG",
    "RIGHTFOOT",
];

/// The limbs the IK bends, on each side: upper, middle and end bone.
const LIMBS: [[&str; 3]; 2] = [["ARM", "FOREARM", "HAND"], ["UPLEG", "LEG", "FOOT"]];

/// The shortest a limb segment may be, skate metres.
const MIN_LIMB: f32 = 0.01;

impl Rig {
    /// Skate 3's skater, from `rig.json`.
    pub fn load(path: &std::path::Path) -> Result<Rig, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bones: Vec<RigBone> =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let names: Vec<String> = bones.iter().map(|b| b.name.clone()).collect();
        let bind = bones
            .iter()
            .map(|b| Mat4::from_cols_array(&b.bind.map(|v| v as f32)))
            .collect();
        Rig::from_binds(names, bind, &path.display().to_string())
    }

    /// A rig from its bones' names and bind poses (skate metres), `what`
    /// naming it in errors. Fails when a bone the poses need is missing, a
    /// bind is not finite or a limb has no length.
    pub fn from_binds(names: Vec<String>, bind: Vec<Mat4>, what: &str) -> Result<Rig, String> {
        if names.len() != bind.len() {
            return Err(format!("{what}: {} names for {} bones", names.len(), bind.len()));
        }
        let index = names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let rig = Rig { names, bind, index };
        for n in REQUIRED {
            if !rig.index.contains_key(n) {
                return Err(format!("{what}: no bone {n}"));
            }
        }
        if let Some(i) = rig.bind.iter().position(|m| !m.is_finite()) {
            return Err(format!("{what}: bone {} is not finite", rig.names[i]));
        }
        for prefix in ["LEFT", "RIGHT"] {
            for limb in LIMBS {
                let [a, b, c] = limb.map(|part| format!("{prefix}{part}"));
                for (from, to) in [(&a, &b), (&b, &c)] {
                    if rig.at(from).distance(rig.at(to)) < MIN_LIMB {
                        return Err(format!("{what}: bone {from} has no length"));
                    }
                }
            }
        }
        Ok(rig)
    }

    pub fn bind(&self, name: &str) -> Mat4 {
        self.index.get(name).map_or(Mat4::IDENTITY, |&i| self.bind[i])
    }

    pub fn at(&self, name: &str) -> Vec3 {
        self.bind(name).w_axis.truncate()
    }

    /// Every bone at its bind pose, for a pose to start from.
    pub fn bind_pose(&self) -> Vec<Mat4> {
        self.bind.clone()
    }

    /// Sets bone `name` in a pose made from `bind_pose`.
    pub fn set(&self, pose: &mut [Mat4], name: &str, m: Mat4) {
        if let Some(&i) = self.index.get(name) {
            pose[i] = m;
        }
    }

    /// Every rig bone posed, skate metres, bike frame (`k` is the skate scale),
    /// at `RIDER_SIZE`: posed full size to targets moved out by 1 / size, then
    /// shrunk about the bike's origin, so hands and feet land on the bike.
    pub fn pose(&self, k: f32, stance: &Stance) -> Vec<Mat4> {
        let k = k / RIDER_SIZE;
        let lean = stance.lean.clamp_length_max(1.0);
        let mut out = self.bind.clone();
        let mut set = |name: &str, m: Mat4| {
            if let Some(&i) = self.index.get(name) {
                out[i] = m;
            }
        };

        // Hips: over the pegs, moved with the stick (X is left).
        let hips = (HIPS + Vec3::new(-lean.x * 0.16, -0.06 * lean.length(), lean.y * 0.20)) * k;
        let hip_bind = self.at("HIPS");
        let tilt = TORSO + lean.y * 0.35;
        let roll = lean.x * 0.25;
        let about_hips = |q: Quat| {
            Mat4::from_translation(hips) * Mat4::from_quat(q) * Mat4::from_translation(-hip_bind)
        };
        let pelvis = about_hips(Quat::from_rotation_x(tilt * 0.45));
        let upper = about_hips(Quat::from_rotation_z(roll) * Quat::from_rotation_x(tilt));
        set("HIPS", pelvis * self.bind("HIPS"));
        for n in ["SPINE", "SPINE1", "SPINE2", "SPINE3", "LEFTSHOULDER", "RIGHTSHOULDER"] {
            set(n, upper * self.bind(n));
        }
        // The head comes back up to look down the track.
        let neck = self.at("NECK");
        let head = upper
            * Mat4::from_translation(neck)
            * Mat4::from_quat(Quat::from_rotation_x(-tilt * 0.75))
            * Mat4::from_translation(-neck);
        for n in ["NECK", "NECK1", "HEAD"] {
            set(n, head * self.bind(n));
        }

        for (side, prefix) in [(1.0f32, "LEFT"), (-1.0, "RIGHT")] {
            let name = |part: &str| format!("{prefix}{part}");
            // Legs: ankle just above and behind the peg.
            let peg = Vec3::new(PEG.x * side, PEG.y, PEG.z);
            let ankle = (peg + Vec3::new(0.0, 0.12, -0.06)) * k;
            let hip = pelvis.transform_point3(self.at(&name("UPLEG")));
            let pole = Vec3::new(0.25 * side, 0.0, 1.0);
            let (knee, ankle) = self.limb(&name("UPLEG"), &name("LEG"), &name("FOOT"), hip, ankle, pole);
            set(&name("UPLEG"), self.aim(&name("UPLEG"), &name("LEG"), hip, knee));
            set(&name("LEG"), self.aim(&name("LEG"), &name("FOOT"), knee, ankle));
            let foot = self.bind(&name("FOOT"));
            let shift = Mat4::from_translation(ankle - foot.w_axis.truncate());
            set(&name("FOOT"), shift * foot);
            set(&name("TOEBASE"), shift * self.bind(&name("TOEBASE")));

            // Arms: wrists at the grips, elbows out and back.
            let grip = stance.grips[if side > 0.0 { 0 } else { 1 }];
            let wrist = (grip + Vec3::new(0.0, 0.03, -0.06)) * k;
            let shoulder = upper.transform_point3(self.at(&name("ARM")));
            let pole = Vec3::new(side, -0.4, -0.5);
            let (elbow, wrist) = self.limb(&name("ARM"), &name("FOREARM"), &name("HAND"), shoulder, wrist, pole);
            set(&name("ARM"), self.aim(&name("ARM"), &name("FOREARM"), shoulder, elbow));
            set(&name("FOREARM"), self.aim(&name("FOREARM"), &name("HAND"), elbow, wrist));
            // The hand carries on along the forearm.
            let turn = arc(self.at(&name("HAND")) - self.at(&name("FOREARM")), wrist - elbow);
            let hand = self.bind(&name("HAND"));
            let (_, rotation, _) = hand.to_scale_rotation_translation();
            set(&name("HAND"), Mat4::from_rotation_translation(turn * rotation, wrist));
        }

        // The board bones, which nothing draws on the bike but other code
        // reads for where the rider is: trucks at the tyres' contact points.
        let [rear, front] = stance.contacts.map(|p| p * k);
        set("SKATEBOARD_ROOT", Mat4::from_translation((rear + front) * 0.5));
        set("TRUCK_BACK", Mat4::from_translation(rear));
        set("TRUCK_FRONT", Mat4::from_translation(front));
        let shrink = Mat4::from_scale(Vec3::splat(RIDER_SIZE));
        out.into_iter().map(|m| shrink * m).collect()
    }

    /// Two-bone IK from `root` towards `target`, bending towards `pole`:
    /// the middle joint and where the end lands.
    pub fn limb(&self, a: &str, b: &str, c: &str, root: Vec3, target: Vec3, pole: Vec3) -> (Vec3, Vec3) {
        let l1 = self.at(a).distance(self.at(b));
        let l2 = self.at(b).distance(self.at(c));
        let d = target - root;
        let dir = d.normalize_or(Vec3::NEG_Y);
        // In reach and never folded flat. A limb too short for both (which
        // `from_binds` refuses) reaches out straight rather than panicking.
        let lo = (l1 - l2).abs() + 1e-3;
        let dist = d.length().clamp(lo, (l1 + l2 - 1e-4).max(lo));
        let cos = ((l1 * l1 + dist * dist - l2 * l2) / (2.0 * l1 * dist)).clamp(-1.0, 1.0);
        let sin = (1.0 - cos * cos).max(0.0).sqrt();
        let bend = (pole - dir * pole.dot(dir)).normalize_or(dir.any_orthonormal_vector());
        let middle = root + dir * (l1 * cos) + bend * (l1 * sin);
        (middle, root + dir * dist)
    }

    /// Bone `a` at `from`, turned so its segment to `b` points at `to`.
    pub fn aim(&self, a: &str, b: &str, from: Vec3, to: Vec3) -> Mat4 {
        let (_, rotation, _) = self.bind(a).to_scale_rotation_translation();
        let turn = arc(self.at(b) - self.at(a), to - from);
        Mat4::from_rotation_translation(turn * rotation, from)
    }
}

pub fn arc(from: Vec3, to: Vec3) -> Quat {
    match (from.try_normalize(), to.try_normalize()) {
        (Some(a), Some(b)) => Quat::from_rotation_arc(a, b),
        _ => Quat::IDENTITY,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physics::{BAR_HALF_WIDTH, bar_centre};

    /// A made-up rig (our own round numbers, not any game's): bone and
    /// joint, skate metres, facing +Z with the left at +X, soles at y = 0.
    /// Places only, as Master Chief's own skeleton has.
    const JOINTS: [(&str, [f32; 3]); 27] = [
        ("HIPS", [0.0, 0.95, 0.0]),
        ("SPINE", [0.0, 1.05, 0.0]),
        ("SPINE1", [0.0, 1.15, 0.0]),
        ("SPINE2", [0.0, 1.25, 0.0]),
        ("SPINE3", [0.0, 1.35, 0.0]),
        ("NECK", [0.0, 1.48, 0.0]),
        ("NECK1", [0.0, 1.52, 0.0]),
        ("HEAD", [0.0, 1.58, 0.01]),
        ("LEFTSHOULDER", [0.05, 1.42, 0.0]),
        ("LEFTARM", [0.19, 1.42, 0.0]),
        ("LEFTFOREARM", [0.21, 1.14, 0.0]),
        ("LEFTHAND", [0.22, 0.88, 0.02]),
        ("RIGHTSHOULDER", [-0.05, 1.42, 0.0]),
        ("RIGHTARM", [-0.19, 1.42, 0.0]),
        ("RIGHTFOREARM", [-0.21, 1.14, 0.0]),
        ("RIGHTHAND", [-0.22, 0.88, 0.02]),
        ("LEFTUPLEG", [0.09, 0.9, 0.0]),
        ("LEFTLEG", [0.1, 0.5, 0.01]),
        ("LEFTFOOT", [0.1, 0.1, -0.01]),
        ("LEFTTOEBASE", [0.1, 0.02, 0.13]),
        ("RIGHTUPLEG", [-0.09, 0.9, 0.0]),
        ("RIGHTLEG", [-0.1, 0.5, 0.01]),
        ("RIGHTFOOT", [-0.1, 0.1, -0.01]),
        ("RIGHTTOEBASE", [-0.1, 0.02, 0.13]),
        ("SKATEBOARD_ROOT", [0.0; 3]),
        ("TRUCK_FRONT", [0.0; 3]),
        ("TRUCK_BACK", [0.0; 3]),
    ];

    fn bones() -> (Vec<String>, Vec<Mat4>) {
        JOINTS.iter().map(|(n, p)| (n.to_string(), Mat4::from_translation(Vec3::from(*p)))).unzip()
    }

    fn rig() -> Rig {
        let (names, bind) = bones();
        Rig::from_binds(names, bind, "test rig").expect("a rig")
    }

    #[test]
    fn from_binds_refuses_what_the_poses_cannot_use() {
        let (mut names, mut bind) = bones();
        let i = names.iter().position(|n| n == "RIGHTLEG").unwrap();
        names.remove(i);
        bind.remove(i);
        let e = Rig::from_binds(names, bind, "test rig").err().expect("no RIGHTLEG");
        assert!(e.contains("RIGHTLEG"), "{e}");

        // A shin of no length: the knee on the ankle.
        let (names, mut bind) = bones();
        let at = |n: &str| names.iter().position(|b| b == n).unwrap();
        bind[at("LEFTLEG")] = bind[at("LEFTFOOT")];
        let e = Rig::from_binds(names.clone(), bind, "test rig").err().expect("no shin");
        assert!(e.contains("LEFTLEG"), "{e}");

        let (names, mut bind) = bones();
        bind[3].w_axis.y = f32::INFINITY;
        assert!(Rig::from_binds(names, bind, "test rig").is_err());

        let (names, mut bind) = bones();
        bind.pop();
        assert!(Rig::from_binds(names, bind, "test rig").is_err());
    }

    /// `limb` on a limb of no length reaches straight, never panicking (it
    /// did, in `f32::clamp`, with its bounds crossed).
    #[test]
    fn a_limb_of_no_length_does_not_panic() {
        let (names, bind) = bones();
        let index = names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let mut rig = Rig { names, bind, index };
        let flat = rig.bind("LEFTARM");
        for n in ["LEFTFOREARM", "LEFTHAND"] {
            let i = rig.index[n];
            rig.bind[i] = flat;
        }
        let (middle, end) = rig.limb("LEFTARM", "LEFTFOREARM", "LEFTHAND", Vec3::ZERO, Vec3::Y, Vec3::Z);
        assert!(!middle.is_nan() && !end.is_nan(), "{middle} {end}");
    }

    #[test]
    fn every_stance_poses_finite() {
        let rig = rig();
        let centre = bar_centre();
        let side = Vec3::X * BAR_HALF_WIDTH;
        for x in [-1.0f32, 0.0, 1.0] {
            for y in [-1.0f32, 0.0, 1.0] {
                let stance = Stance {
                    grips: [centre + side, centre - side],
                    lean: Vec2::new(x, y),
                    contacts: [Vec3::new(0.0, -0.6, -0.75), Vec3::new(0.0, -0.6, 0.75)],
                };
                let pose = rig.pose(mcc_core::skate::DEFAULT_SCALE, &stance);
                assert_eq!(pose.len(), rig.names.len());
                for (name, m) in rig.names.iter().zip(&pose) {
                    assert!(m.is_finite(), "{name} at lean {x}, {y}: {m}");
                }
            }
        }
    }
}
