//! The Spartan on foot as a skate skeleton pose, so `mcc_rider` draws him
//! unchanged: standing, walking, running, strafing, crouching and jumping,
//! made up from a gait cycle rather than Halo's own animations.
//!
//! The skater rig (`rig.json`, skate metres) faces +Z with its left at +X
//! and Y up, its soles at y = 0. Poses come out in that frame; `mod.rs`
//! turns and places them in the world.
//!
//! The gait: each foot spends half the cycle planted, sliding back under
//! the body at the ground's speed, and half swinging forward through the
//! air. Two-bone IK puts the ankles there; the hips bob, twist and lean
//! into the run; the arms either swing against the legs or hold a weapon
//! up towards where he aims.

use bevy::math::{Mat4, Quat, Vec2, Vec3, Vec4};

use mcc_bike::pose::{Rig, arc};

/// What the pose is made from, in rig metres and radians.
#[derive(Clone, Copy, Debug, Default)]
pub struct Gait {
    /// The gait cycle, 0..2pi: the left foot plants at 0, the right at pi.
    pub phase: f32,
    /// Ground speed in rig metres per second.
    pub speed: f32,
    /// Which way he moves in his own frame: X left, Y forward (unit length,
    /// or zero standing).
    pub direction: Vec2,
    /// 0 standing .. 1 crouched.
    pub crouch: f32,
    /// 0 on the ground .. 1 well into a jump or a fall.
    pub air: f32,
    /// Rising (+) or falling (-), -1..1, for the legs in the air.
    pub rise: f32,
    /// Knees giving on a landing, 0..1.
    pub land: f32,
    /// Aim above level (+) or below, radians.
    pub pitch: f32,
    /// The weapon in his hands; `None`: arms swing free.
    pub hold: Option<Hold>,
    /// Ground height under the left and right foot, relative to the soles.
    pub ground: [f32; 2],
}

/// A weapon held at the ready, pointing where he aims.
///
/// "Grip space" is the weapon's model space (Halo world units, X along the
/// barrel, Y left, Z up) moved so its `right_hand` marker is the origin.
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    /// The weapon's `left_hand` marker in grip space.
    pub left_hand: Vec3,
    /// Rig metres per world unit.
    pub scale: f32,
}

/// Where the grip sits, in the chest's frame turned to the aim: right of
/// the middle, a little below the chest, close in so the off hand reaches
/// the fore grip (the skater's arms are short for a Spartan's rifle).
const GRIP: Vec3 = Vec3::new(-0.07, -0.13, 0.08);

/// The wrist sits behind the hand's grip marker along the barrel: the
/// Spartan's `right_hand` marker is 0.0465 wu out from `r_hand`.
const WRIST_BEHIND: f32 = 0.0465;

/// Grip space to his rig frame: barrel (+X) forward (+Z), left (+Y) to
/// his left (+X), up (+Z) up (+Y).
fn grip_axes(scale: f32) -> Mat4 {
    Mat4::from_cols(Vec4::Z * scale, Vec4::X * scale, Vec4::Y * scale, Vec4::W)
}

/// Where the aim pitch turns the chest and arms: about the upper chest.
const CHEST: Vec3 = Vec3::new(0.0, 1.35, 0.0);

/// One step's length (half the cycle) for a ground speed, rig metres.
pub fn step_length(speed: f32) -> f32 {
    (0.3 + 0.24 * speed).clamp(0.3, 1.6)
}

impl Gait {
    /// Moves the cycle on by `dt` seconds at the current speed.
    pub fn advance(&mut self, dt: f32) {
        if self.speed < 0.05 {
            return;
        }
        let cycle = 2.0 * step_length(self.speed);
        self.phase = (self.phase + std::f32::consts::TAU * self.speed * dt / cycle) % std::f32::consts::TAU;
    }

    /// Where a foot is in its step: along the way of travel (rig metres,
    /// forward of the hip +) and lifted off the ground.
    fn foot(&self, phase: f32) -> (f32, f32) {
        use std::f32::consts::PI;
        let step = step_length(self.speed);
        let moving = (self.speed / 1.2).min(1.0);
        let p = phase.rem_euclid(2.0 * PI);
        let (along, lift) = if p < PI {
            // Planted: sliding back under the body.
            (step * 0.5 * (1.0 - 2.0 * p / PI), 0.0)
        } else {
            let t = (p - PI) / PI;
            let s = t * t * (3.0 - 2.0 * t);
            (step * 0.5 * (2.0 * s - 1.0), (0.06 + 0.04 * self.speed).min(0.32) * (PI * t).sin())
        };
        (along * moving, lift * moving)
    }
}

/// Every rig bone posed for `gait`, rig metres, his own frame; and,
/// holding a weapon, grip space to that frame.
pub fn foot_pose(rig: &Rig, g: &Gait) -> (Vec<Mat4>, Option<Mat4>) {
    use std::f32::consts::PI;
    let mut out = rig.bind_pose();
    let speed = g.speed;
    let moving = (speed / 1.2).min(1.0);
    let run = ((speed - 2.0) / 3.0).clamp(0.0, 1.0);
    let dir = Vec3::new(g.direction.x, 0.0, g.direction.y);
    let forwardness = g.direction.y;

    // Hips: down for a crouch, a landing and the run's bent knees; a bob
    // twice a cycle; a sway over the planted foot.
    let bob = (2.0 * g.phase).cos() * (0.012 + 0.025 * run) * moving;
    let drop = 0.03 + 0.38 * g.crouch + 0.06 * run + 0.18 * g.land + 0.05 * g.air;
    let sway = g.phase.sin() * 0.025 * moving * (1.0 - run * 0.5);
    let hip_bind = rig.at("HIPS");
    let hips = hip_bind + Vec3::new(sway, -drop + bob, 0.0) + dir * (0.04 * run);
    // The body leans into the way he moves, more when running; the hips
    // twist with the legs and the shoulders against them.
    let lean = 0.06 + 0.16 * run * forwardness.max(0.0) - 0.06 * run * (-forwardness).max(0.0) + 0.3 * g.crouch;
    let side_lean = -g.direction.x * 0.08 * run;
    let twist = g.phase.sin() * 0.18 * moving * forwardness.abs();
    let pelvis_q = Quat::from_rotation_y(twist) * Quat::from_rotation_x(lean * 0.4);
    let about = |at: Vec3, from: Vec3, q: Quat| Mat4::from_translation(at) * Mat4::from_quat(q) * Mat4::from_translation(-from);
    let pelvis = about(hips, hip_bind, pelvis_q);
    rig.set(&mut out, "HIPS", pelvis * rig.bind("HIPS"));

    // Spine: lean, a counter-twist, and half the aim pitch.
    let pitch = g.pitch.clamp(-1.2, 1.2);
    let aim_share = if g.hold.is_some() { 0.45 } else { 0.25 };
    let spine_q = Quat::from_rotation_z(side_lean)
        * Quat::from_rotation_y(-twist * 0.6 * (1.0 - g.hold.map_or(0.0, |_| 0.7)))
        * Quat::from_rotation_x(lean - pitch * aim_share);
    let upper = about(hips, hip_bind, spine_q);
    for n in ["SPINE", "SPINE1", "SPINE2", "SPINE3", "LEFTSHOULDER", "RIGHTSHOULDER"] {
        rig.set(&mut out, n, upper * rig.bind(n));
    }
    // The head takes the rest of the pitch, and comes back up from the lean.
    let neck = rig.at("NECK");
    let head = upper * about(neck, neck, Quat::from_rotation_x(-(pitch * (1.0 - aim_share)) - lean * 0.8));
    for n in ["NECK", "NECK1", "HEAD"] {
        rig.set(&mut out, n, head * rig.bind(n));
    }

    // Legs.
    for (side, prefix, offset) in [(1.0f32, "LEFT", 0.0), (-1.0, "RIGHT", PI)] {
        let name = |part: &str| format!("{prefix}{part}");
        let foot_bind = rig.bind(&name("FOOT"));
        let ankle_bind = foot_bind.w_axis.truncate();
        let (along, lift) = g.foot(g.phase + offset);
        let ground = g.ground[if side > 0.0 { 0 } else { 1 }];
        let width = 0.02 * g.crouch + 0.03 * g.direction.x.abs() * moving;
        let mut ankle = Vec3::new(ankle_bind.x + side * width, ankle_bind.y + ground + lift, ankle_bind.z) + dir * along;
        // In the air the legs tuck up: knees forward going up, reaching
        // down for the ground coming down.
        let tuck = g.air * (0.30 + 0.12 * g.rise);
        ankle = ankle.lerp(
            Vec3::new(ankle_bind.x * 1.2, ankle_bind.y + tuck, ankle_bind.z + side * 0.12 - 0.05),
            g.air,
        );
        let hip = pelvis.transform_point3(rig.at(&name("UPLEG")));
        let pole = Vec3::new(0.15 * side, 0.0, 1.0);
        let (knee, ankle) = rig.limb(&name("UPLEG"), &name("LEG"), &name("FOOT"), hip, ankle, pole);
        rig.set(&mut out, &name("UPLEG"), rig.aim(&name("UPLEG"), &name("LEG"), hip, knee));
        rig.set(&mut out, &name("LEG"), rig.aim(&name("LEG"), &name("FOOT"), knee, ankle));
        // The foot stays flat on the ground, toes down a little in a swing
        // or a fall.
        let toe = (lift * 1.5 + g.air * 0.4).min(0.6);
        let foot = Mat4::from_translation(ankle) * Mat4::from_quat(Quat::from_rotation_x(toe)) * Mat4::from_translation(-ankle_bind);
        rig.set(&mut out, &name("FOOT"), foot * foot_bind);
        rig.set(&mut out, &name("TOEBASE"), foot * rig.bind(&name("TOEBASE")));
    }

    // Arms.
    let chest = upper.transform_point3(CHEST);
    // The weapon points where he aims whatever the lean.
    let aim = Mat4::from_translation(chest) * Mat4::from_quat(Quat::from_rotation_x(-pitch));
    let weapon = g.hold.map(|h| aim * Mat4::from_translation(GRIP) * grip_axes(h.scale));
    for (side, prefix, offset) in [(1.0f32, "LEFT", 0.0), (-1.0, "RIGHT", PI)] {
        let name = |part: &str| format!("{prefix}{part}");
        let shoulder = upper.transform_point3(rig.at(&name("ARM")));
        let (wrist, pole) = match (g.hold, weapon) {
            (Some(hold), Some(w)) => {
                let marker = if side > 0.0 { hold.left_hand } else { Vec3::ZERO };
                (w.transform_point3(marker - Vec3::X * WRIST_BEHIND), Vec3::new(side * 0.8, -0.6, -0.3))
            }
            _ => {
                // Hanging, swinging against the leg on the same side.
                let swing = -(g.phase + offset).cos() * (0.10 + 0.25 * run) * moving * forwardness.signum();
                let hand = shoulder + Vec3::new(side * (0.10 + 0.04 * run), -0.56 + 0.12 * run, 0.06 + swing);
                (hand, Vec3::new(side * 0.2, 0.0, -1.0))
            }
        };
        let (elbow, wrist) = rig.limb(&name("ARM"), &name("FOREARM"), &name("HAND"), shoulder, wrist, pole);
        rig.set(&mut out, &name("ARM"), rig.aim(&name("ARM"), &name("FOREARM"), shoulder, elbow));
        rig.set(&mut out, &name("FOREARM"), rig.aim(&name("FOREARM"), &name("HAND"), elbow, wrist));
        let turn = arc(rig.at(&name("HAND")) - rig.at(&name("FOREARM")), wrist - elbow);
        let (_, rotation, _) = rig.bind(&name("HAND")).to_scale_rotation_translation();
        rig.set(&mut out, &name("HAND"), Mat4::from_rotation_translation(turn * rotation, wrist));
    }

    // The board bones nothing draws on foot, which other code reads for
    // where he stands and faces: a "board" under his feet along +Z.
    rig.set(&mut out, "SKATEBOARD_ROOT", Mat4::from_translation(Vec3::new(0.0, 0.09, 0.0)));
    rig.set(&mut out, "TRUCK_BACK", Mat4::from_translation(Vec3::new(0.0, 0.06, -0.26)));
    rig.set(&mut out, "TRUCK_FRONT", Mat4::from_translation(Vec3::new(0.0, 0.06, 0.26)));
    (out, weapon)
}
