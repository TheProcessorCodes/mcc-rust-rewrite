//! The rider on the bike, as a skate skeleton pose so `rider.rs` can draw
//! the Spartan on it unchanged.
//!
//! One stiff stance, the motocross attack position: standing on the pegs,
//! knees bent, chest over the tank, hands on the grips. The hips move with
//! the right stick and the torso tilts with them; two-bone IK keeps the
//! feet on the pegs and the hands on the grips (which turn with the bars).
//!
//! The skater rig (`rig.json`, skate metres) faces +Z with its left at +X
//! and Y up: the bike's own frame. Poses come out in skate metres in the
//! bike frame; `mod.rs` places them in the world.

use std::collections::HashMap;

use bevy::math::{Mat4, Quat, Vec2, Vec3};
use serde::Deserialize;

use super::physics::{HIPS, PEG};

#[derive(Deserialize)]
struct RigBone {
    name: String,
    bind: [f64; 16],
}

/// The skater rig's bind pose.
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

impl Rig {
    pub fn load(path: &std::path::Path) -> Result<Rig, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let bones: Vec<RigBone> =
            serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let names: Vec<String> = bones.iter().map(|b| b.name.clone()).collect();
        let bind = bones
            .iter()
            .map(|b| Mat4::from_cols_array(&b.bind.map(|v| v as f32)))
            .collect();
        let index = names.iter().enumerate().map(|(i, n)| (n.clone(), i)).collect();
        let rig = Rig { names, bind, index };
        for n in ["HIPS", "SPINE", "NECK", "HEAD", "LEFTARM", "LEFTFOREARM", "LEFTHAND", "LEFTUPLEG", "LEFTLEG", "LEFTFOOT"] {
            if !rig.index.contains_key(n) {
                return Err(format!("{}: no bone {n}", path.display()));
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

    /// Every rig bone posed, skate metres, bike frame (`k` is the skate scale).
    pub fn pose(&self, k: f32, stance: &Stance) -> Vec<Mat4> {
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
        out
    }

    /// Two-bone IK from `root` towards `target`, bending towards `pole`:
    /// the middle joint and where the end lands.
    pub fn limb(&self, a: &str, b: &str, c: &str, root: Vec3, target: Vec3, pole: Vec3) -> (Vec3, Vec3) {
        let l1 = self.at(a).distance(self.at(b));
        let l2 = self.at(b).distance(self.at(c));
        let d = target - root;
        let dir = d.normalize_or(Vec3::NEG_Y);
        let dist = d.length().clamp((l1 - l2).abs() + 1e-3, l1 + l2 - 1e-4);
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
