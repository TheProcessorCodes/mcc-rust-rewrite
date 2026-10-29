//! The Spartan's body on the map's collision: a capsule of spheres that
//! floats a step's height over the ground, held up by rays under the feet.
//!
//! Renderer frame (metres, Y up), the same `World` the bike rides on. The
//! capsule is three spheres stacked from just above the step height to the
//! top of the head; anything lower than the step is left to the ground rays,
//! which is what lets the Spartan walk up stairs and kerbs without
//! catching. Ground steeper than `max_slope` does not hold him: he slides
//! off it and the spheres push him away.
//!
//! Halo's numbers are in world units (1 wu = 3.048 m); `Tuning::default`
//! converts them.

use bevy::math::{Vec2, Vec3};

use crate::bike::physics::{Hit, World};

/// Halo world units to metres.
const WU: f32 = blam_geometry::WORLD_UNIT_METRES;

/// How the Spartan moves. Speeds in m/s, accelerations in m/s^2, sizes in
/// metres.
#[derive(Clone, Copy, Debug)]
pub struct Tuning {
    /// Running forward, backward and sideways at full stick.
    pub forward: f32,
    pub backward: f32,
    pub sideways: f32,
    /// Changing speed on the ground and in the air.
    pub ground_accel: f32,
    pub air_accel: f32,
    /// Upward speed a jump starts with.
    pub jump: f32,
    pub gravity: f32,
    /// Cosine of the steepest ground that holds him.
    pub max_slope: f32,
    /// The tallest ledge he walks up without jumping.
    pub step: f32,
    pub radius: f32,
    pub stand_height: f32,
    pub crouch_height: f32,
    /// Speed while crouched, as a share of running.
    pub crouch_speed: f32,
    /// Seconds to crouch or stand.
    pub crouch_time: f32,
    /// Eye height standing and crouched.
    pub stand_eye: f32,
    pub crouch_eye: f32,
}

impl Default for Tuning {
    /// Valhalla's numbers (`blam movement riverworld`), for when the map's
    /// own cannot be read.
    fn default() -> Self {
        Tuning {
            forward: 2.25 * WU,
            backward: 2.0 * WU,
            sideways: 2.0 * WU,
            ground_accel: 9.6 * WU,
            air_accel: 1.05 * WU,
            jump: 3.08 * WU,
            gravity: blam_geometry::movement::GRAVITY * WU,
            max_slope: 50f32.to_radians().cos(),
            step: STEP,
            radius: 0.175 * WU,
            stand_height: 0.725 * WU,
            crouch_height: 0.5 * WU,
            crouch_speed: 0.9 / 2.25,
            crouch_time: 0.2,
            stand_eye: 0.62 * WU,
            crouch_eye: 0.45 * WU,
        }
    }
}

/// The tallest ledge walked up without a jump, metres. Halo has no such
/// number: its capsule's round bottom rides up kerbs and stairs. This is
/// our own choice (`docs/format/movement.md`).
const STEP: f32 = 0.45;

impl Tuning {
    /// The map's own player numbers (`blam_geometry::movement`).
    /// `HALO3_JUMP=<metres>` sets the jump height instead of the tag's
    /// jump speed.
    pub fn from_movement(m: &blam_geometry::Movement) -> Tuning {
        let gravity = m.gravity * WU;
        let mut jump = m.jump_velocity * WU;
        if let Some(h) = std::env::var("HALO3_JUMP").ok().and_then(|s| s.parse::<f32>().ok()).filter(|h| *h > 0.0) {
            jump = (2.0 * gravity * h).sqrt();
        }
        Tuning {
            forward: m.run_forward * WU,
            backward: m.run_backward * WU,
            sideways: m.run_sideways * WU,
            ground_accel: m.run_acceleration * WU,
            air_accel: m.airborne_acceleration * WU,
            jump,
            gravity,
            max_slope: m.max_slope_angle.cos(),
            step: STEP,
            radius: m.radius * WU,
            stand_height: m.height_standing * WU,
            crouch_height: m.height_crouching * WU,
            crouch_speed: m.crouch_forward / m.run_forward.max(1e-3),
            crouch_time: m.crouch_transition_time.max(0.05),
            stand_eye: m.standing_camera_height * WU,
            crouch_eye: m.crouching_camera_height * WU,
        }
    }
}

/// What the player asks for this frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// The stick: X right, Y forward, length up to 1.
    pub wish: Vec2,
    /// Where he faces, a heading about +Y (0 looks down +Z).
    pub heading: f32,
    /// Pressed this frame.
    pub jump: bool,
    /// Held.
    pub crouch: bool,
}

/// The Spartan's body.
#[derive(Clone, Debug)]
pub struct Spartan {
    /// The soles of his feet.
    pub position: Vec3,
    pub velocity: Vec3,
    pub heading: f32,
    pub grounded: bool,
    pub ground_normal: Vec3,
    /// 0 standing .. 1 crouched.
    pub crouch: f32,
    /// Seconds since he last stood on the ground.
    pub airborne: f32,
    /// Downward speed of the last landing, and seconds since it.
    pub landing: (f32, f32),
    /// He jumped and has not landed yet.
    pub jumped: bool,
    pub tuning: Tuning,
    /// The last place he stood on walkable ground, and his heading there.
    pub safe: Option<(Vec3, f32)>,
    candidates: Vec<u32>,
    contacts: Vec<(Vec3, f32)>,
}

/// Coyote time: a jump still works this long after walking off a ledge.
const LATE_JUMP: f32 = 0.12;
const MAX_STEP_DT: f32 = 1.0 / 120.0;

pub fn heading_forward(heading: f32) -> Vec3 {
    Vec3::new(heading.sin(), 0.0, heading.cos())
}

impl Spartan {
    pub fn new(position: Vec3, heading: f32) -> Spartan {
        Spartan {
            position,
            velocity: Vec3::ZERO,
            heading,
            grounded: false,
            ground_normal: Vec3::Y,
            crouch: 0.0,
            airborne: 0.0,
            landing: (0.0, 10.0),
            jumped: false,
            tuning: Tuning::default(),
            safe: None,
            candidates: Vec::new(),
            contacts: Vec::new(),
        }
    }

    /// Dropped on the ground under `point` (or left there to fall).
    pub fn dropped(world: &World, point: Vec3, heading: f32) -> Spartan {
        let mut s = Spartan::new(point, heading);
        if let Some(hit) = world.raycast_far(point + Vec3::Y * 0.5, Vec3::NEG_Y, 200.0) {
            s.position = hit.point;
            s.grounded = true;
            s.ground_normal = hit.normal;
        }
        s
    }

    pub fn height(&self) -> f32 {
        let t = &self.tuning;
        t.stand_height + (t.crouch_height - t.stand_height) * self.crouch
    }

    /// The eye, for the first-person camera.
    pub fn eye(&self) -> Vec3 {
        let t = &self.tuning;
        self.position + Vec3::Y * (t.stand_eye + (t.crouch_eye - t.stand_eye) * self.crouch)
    }

    pub fn forward(&self) -> Vec3 {
        heading_forward(self.heading)
    }

    /// Speed along the ground.
    pub fn speed(&self) -> f32 {
        Vec2::new(self.velocity.x, self.velocity.z).length()
    }

    /// Sphere centres of the capsule, above the feet, for a body `height` tall.
    fn spheres(&self, height: f32) -> [f32; 3] {
        let r = self.tuning.radius;
        let low = self.tuning.step + r;
        let high = (height - r).max(low);
        [low, (low + high) * 0.5, high]
    }

    pub fn step(&mut self, world: &World, controls: Controls, dt: f32) {
        if dt <= 0.0 {
            return;
        }
        let reach = self.tuning.stand_height + self.speed() * dt + 1.0;
        let mut candidates = std::mem::take(&mut self.candidates);
        world.gather(self.position, reach, &mut candidates);
        let n = (dt / MAX_STEP_DT).ceil().max(1.0);
        let h = dt / n;
        let mut jump = controls.jump;
        for _ in 0..n as usize {
            self.sub_step(world, &candidates, controls, jump, h);
            jump = false;
        }
        self.candidates = candidates;
        self.landing.1 += dt;
        if self.grounded && self.ground_normal.y >= self.tuning.max_slope {
            self.safe = Some((self.position, self.heading));
        }
    }

    /// Back to the last walkable place.
    pub fn respawn(&mut self) {
        if let Some((p, h)) = self.safe {
            let tuning = self.tuning;
            *self = Spartan::new(p + Vec3::Y * 0.05, h);
            self.tuning = tuning;
            self.safe = Some((p, h));
        }
    }

    fn sub_step(&mut self, world: &World, candidates: &[u32], c: Controls, jump: bool, dt: f32) {
        let t = self.tuning;
        self.heading = c.heading;

        // Crouching, and standing back up only where there is room.
        let want = if c.crouch { 1.0 } else { 0.0 };
        if want < self.crouch {
            let top = self.position + Vec3::Y * self.spheres(t.stand_height)[2];
            world.sphere(candidates, top, t.radius * 0.95, &mut self.contacts);
            if self.contacts.iter().all(|(n, _)| n.y > -0.2) {
                self.crouch = (self.crouch - dt / t.crouch_time).max(want);
            }
        } else {
            self.crouch = (self.crouch + dt / t.crouch_time).min(want);
        }

        // Running: the stick in his facing, each axis at its own top speed.
        let fwd = heading_forward(self.heading);
        let right = fwd.cross(Vec3::Y);
        let wish = c.wish.clamp_length_max(1.0);
        let along = if wish.y >= 0.0 { t.forward } else { t.backward };
        let scale = 1.0 + (t.crouch_speed - 1.0) * self.crouch;
        let target = (fwd * wish.y * along + right * wish.x * t.sideways) * scale;
        let accel = if self.grounded { t.ground_accel } else { t.air_accel };
        let flat = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
        let change = (target - flat).clamp_length_max(accel * dt);
        // In the air, letting go of the stick keeps his momentum.
        let change = if self.grounded || wish != Vec2::ZERO { change } else { Vec3::ZERO };
        self.velocity.x += change.x;
        self.velocity.z += change.z;

        if jump && (self.grounded || (self.airborne < LATE_JUMP && !self.jumped)) {
            self.velocity.y = t.jump;
            self.grounded = false;
            self.jumped = true;
        }
        if self.grounded {
            // Along the ground, so running downhill does not bounce.
            let n = self.ground_normal;
            self.velocity -= n * self.velocity.dot(n);
        } else {
            self.velocity.y -= t.gravity * dt;
        }
        self.position += self.velocity * dt;

        // The body against walls and ceilings.
        let height = self.height();
        for _ in 0..3 {
            let mut moved = false;
            for centre in self.spheres(height) {
                world.sphere(candidates, self.position + Vec3::Y * centre, t.radius, &mut self.contacts);
                for &(n, depth) in &self.contacts {
                    // Walkable ground under the body is the rays' to hold.
                    let n = if self.grounded && n.y >= t.max_slope {
                        continue;
                    } else if n.y > 0.0 && n.y < t.max_slope {
                        // A steep face pushes sideways, never up.
                        let side = Vec3::new(n.x, 0.0, n.z);
                        if side.length_squared() < 1e-6 {
                            continue;
                        }
                        side.normalize()
                    } else {
                        n
                    };
                    self.position += n * depth;
                    let into = self.velocity.dot(n);
                    if into < 0.0 {
                        self.velocity -= n * into;
                    }
                    moved = true;
                }
            }
            if !moved {
                break;
            }
        }

        // The ground under the feet.
        let rising = self.velocity.y > 0.5;
        let reach_up = t.step + t.radius;
        let snap = if self.grounded { t.step } else { 0.0 } + (-self.velocity.y).max(0.0) * dt;
        let ground = if rising { None } else { self.ground(world, candidates, reach_up, reach_up + snap) };
        match ground {
            Some(hit) if hit.normal.y >= t.max_slope => {
                if !self.grounded {
                    self.landing = ((-self.velocity.y).max(0.0), 0.0);
                    self.jumped = false;
                }
                self.position.y = hit.point.y;
                self.grounded = true;
                self.ground_normal = hit.normal;
                self.airborne = 0.0;
                let into = self.velocity.dot(hit.normal);
                if into < 0.0 {
                    self.velocity -= hit.normal * into;
                }
            }
            Some(hit) => {
                // Too steep: slide off it, never through it.
                self.position.y = self.position.y.max(hit.point.y);
                self.grounded = false;
                self.ground_normal = hit.normal;
                let into = self.velocity.dot(hit.normal);
                if into < 0.0 {
                    self.velocity -= hit.normal * into;
                }
                self.airborne += dt;
            }
            None => {
                self.grounded = false;
                self.airborne += dt;
            }
        }
    }

    /// The highest ground under the feet: five rays down from `up` above
    /// the soles, at most `reach` long. A ray that finds walkable ground
    /// beats one that finds a steep face.
    fn ground(&self, world: &World, candidates: &[u32], up: f32, reach: f32) -> Option<Hit> {
        let r = self.tuning.radius * 0.6;
        let mut best: Option<Hit> = None;
        let max_slope = self.tuning.max_slope;
        let ranked = |h: &Hit| (h.normal.y >= max_slope, h.point.y);
        for offset in [Vec3::ZERO, Vec3::X * r, Vec3::NEG_X * r, Vec3::Z * r, Vec3::NEG_Z * r] {
            let origin = self.position + offset + Vec3::Y * up;
            if let Some(hit) = world.raycast(candidates, origin, Vec3::NEG_Y, reach) {
                let better = match &best {
                    None => true,
                    Some(b) => {
                        let (hw, hy) = ranked(&hit);
                        let (bw, by) = ranked(b);
                        (hw && !bw) || (hw == bw && hy > by)
                    }
                };
                if better {
                    best = Some(hit);
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quad(a: Vec3, b: Vec3, c: Vec3, d: Vec3) -> [([Vec3; 3], bool); 2] {
        [([a, b, c], false), ([a, c, d], false)]
    }

    /// A floor at y = 0, wound to face up.
    fn floor_tris(y: f32, x0: f32, x1: f32) -> [([Vec3; 3], bool); 2] {
        quad(
            Vec3::new(x0, y, -50.0),
            Vec3::new(x0, y, 50.0),
            Vec3::new(x1, y, 50.0),
            Vec3::new(x1, y, -50.0),
        )
    }

    fn floor() -> World {
        World::new(floor_tris(0.0, -50.0, 50.0))
    }

    fn run(world: &World, s: &mut Spartan, c: Controls, seconds: f32) {
        let dt = 1.0 / 60.0;
        let mut first = true;
        for _ in 0..(seconds / dt) as usize {
            let mut c = c;
            c.jump &= first;
            first = false;
            s.step(world, c, dt);
        }
    }

    #[test]
    fn floor_faces_up() {
        let w = floor();
        let hit = w.raycast_far(Vec3::new(1.0, 5.0, 1.0), Vec3::NEG_Y, 10.0).unwrap();
        assert!(hit.normal.y > 0.99);
    }

    #[test]
    fn stands_and_runs_at_top_speed() {
        let w = floor();
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        run(&w, &mut s, Controls::default(), 0.5);
        assert!(s.grounded);
        assert!(s.position.y.abs() < 1e-3, "{:?}", s.position);
        let c = Controls {
            wish: Vec2::Y,
            ..default()
        };
        run(&w, &mut s, c, 1.0);
        assert!((s.speed() - s.tuning.forward).abs() < 0.05, "{}", s.speed());
        assert!(s.position.z > 4.0, "ran along +Z: {:?}", s.position);
        assert!(s.position.y.abs() < 1e-3);
    }

    #[test]
    fn heading_turns_the_run() {
        let w = floor();
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let c = Controls {
            wish: Vec2::Y,
            heading: std::f32::consts::FRAC_PI_2,
            ..default()
        };
        run(&w, &mut s, c, 1.0);
        assert!(s.position.x > 4.0 && s.position.z.abs() < 0.1, "{:?}", s.position);
        // Strafing right while facing +X goes towards -Z... no: right of +X is +Z.
        let c = Controls {
            wish: Vec2::X,
            heading: std::f32::consts::FRAC_PI_2,
            ..default()
        };
        let z = s.position.z;
        run(&w, &mut s, c, 1.0);
        assert!(s.position.z > z + 3.0, "{:?}", s.position);
    }

    #[test]
    fn jumps_and_lands() {
        let w = floor();
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        run(&w, &mut s, Controls::default(), 0.2);
        let mut peak: f32 = 0.0;
        let dt = 1.0 / 60.0;
        s.step(&w, Controls { jump: true, ..default() }, dt);
        assert!(!s.grounded);
        for _ in 0..180 {
            s.step(&w, Controls::default(), dt);
            peak = peak.max(s.position.y);
        }
        let t = s.tuning;
        let expected = t.jump * t.jump / (2.0 * t.gravity);
        assert!((peak - expected).abs() < 0.08, "peak {peak}, expected {expected}");
        assert!(s.grounded && s.position.y.abs() < 1e-3);
        assert!(s.landing.0 > 1.0);
    }

    #[test]
    fn walks_up_a_step_but_not_a_wall() {
        // A 0.3 m step at z = 3, and a 1.5 m block at x = 10.
        let mut tris: Vec<([Vec3; 3], bool)> = Vec::new();
        tris.extend(quad(
            Vec3::new(-50.0, 0.0, -50.0),
            Vec3::new(-50.0, 0.0, 3.0),
            Vec3::new(50.0, 0.0, 3.0),
            Vec3::new(50.0, 0.0, -50.0),
        ));
        // Riser facing -Z.
        tris.extend(quad(
            Vec3::new(-50.0, 0.0, 3.0),
            Vec3::new(-50.0, 0.3, 3.0),
            Vec3::new(50.0, 0.3, 3.0),
            Vec3::new(50.0, 0.0, 3.0),
        ));
        tris.extend(quad(
            Vec3::new(-50.0, 0.3, 3.0),
            Vec3::new(-50.0, 0.3, 50.0),
            Vec3::new(50.0, 0.3, 50.0),
            Vec3::new(50.0, 0.3, 3.0),
        ));
        let w = World::new(tris);
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let c = Controls {
            wish: Vec2::Y,
            ..default()
        };
        run(&w, &mut s, c, 1.5);
        assert!(s.position.z > 4.0 && (s.position.y - 0.3).abs() < 1e-3, "{:?}", s.position);

        // A wall across +Z at z = 2, 3 m tall, facing -Z.
        let mut tris: Vec<([Vec3; 3], bool)> = floor_tris(0.0, -50.0, 50.0).to_vec();
        tris.extend(quad(
            Vec3::new(-50.0, 0.0, 2.0),
            Vec3::new(-50.0, 3.0, 2.0),
            Vec3::new(50.0, 3.0, 2.0),
            Vec3::new(50.0, 0.0, 2.0),
        ));
        let w = World::new(tris);
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        run(&w, &mut s, c, 1.5);
        assert!(s.position.z < 2.0 - s.tuning.radius + 0.02, "{:?}", s.position);
        assert!(s.position.z > 1.0, "{:?}", s.position);
        assert!(s.grounded);
    }

    #[test]
    fn slides_off_steep_ground() {
        // A 60-degree slope rising along +Z.
        let rise = 60f32.to_radians().tan();
        let w = World::new(quad(
            Vec3::new(-50.0, 0.0, 0.0),
            Vec3::new(-50.0, 50.0 * rise, 50.0),
            Vec3::new(50.0, 50.0 * rise, 50.0),
            Vec3::new(50.0, 0.0, 0.0),
        ));
        let mut s = Spartan::new(Vec3::new(0.0, 10.0 * rise + 0.01, 10.0), 0.0);
        run(&w, &mut s, Controls::default(), 2.0);
        assert!(s.position.z < 8.0, "slid down: {:?}", s.position);
        // Never through it.
        assert!(s.position.y >= s.position.z * rise - 0.05, "{:?}", s.position);
    }

    #[test]
    fn crouch_is_slower_and_stays_under_a_low_roof() {
        let mut tris: Vec<([Vec3; 3], bool)> = floor_tris(0.0, -50.0, 50.0).to_vec();
        // A roof 1.7 m up over z > 5, facing down.
        tris.extend(quad(
            Vec3::new(-50.0, 1.7, 5.0),
            Vec3::new(50.0, 1.7, 5.0),
            Vec3::new(50.0, 1.7, 50.0),
            Vec3::new(-50.0, 1.7, 50.0),
        ));
        let w = World::new(tris);
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let c = Controls {
            wish: Vec2::Y,
            crouch: true,
            ..default()
        };
        run(&w, &mut s, c, 3.0);
        let t = s.tuning;
        assert!((s.speed() - t.forward * t.crouch_speed).abs() < 0.05, "{}", s.speed());
        assert!(s.position.z > 6.0, "crawled under: {:?}", s.position);
        // Letting go of crouch under the roof keeps him down.
        run(&w, &mut s, Controls::default(), 0.5);
        assert!(s.crouch > 0.99);
        assert!(s.position.y.abs() < 1e-3);
    }

    #[test]
    fn zero_time_step_is_harmless() {
        let w = floor();
        let mut s = Spartan::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        let before = s.position;
        s.step(&w, Controls::default(), 0.0);
        assert_eq!(before, s.position);
    }

    fn default<T: Default>() -> T {
        T::default()
    }
}
