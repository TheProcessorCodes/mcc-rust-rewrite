//! The dirt bike as a rigid body: two raycast suspension wheels with tyre
//! friction, a handlebar, a rider whose weight moves with the right stick,
//! and a few probe spheres so the frame and the rider can hit things.
//!
//! Plain Rust over glam, no bevy systems: `mod.rs` feeds it the controller
//! and draws the result. Everything is in the renderer frame (metres, Y up).
//!
//! The bike's own frame: +X left, +Y up, +Z forward (the skater rig's
//! frame too, so the rider pose needs no axis swap). Its origin is the
//! centre of mass of bike and rider together.
//!
//! The model, in the spirit of MX vs ATV Reflex's dual-stick riding:
//! - one long gear: the rear tyre pushes with a constant force up to the top
//!   speed (linear acceleration), limited by grip;
//! - the left stick turns the bars; the bike leans into the turn by itself
//!   (a balance controller aims for the lean that matches speed and steer);
//! - the right stick moves the rider: on the ground his weight presses the
//!   front or back down (wheelies, stoppies) and side to side tightens the
//!   turn; in the air it rotates the bike (pitch, lean and whip).

use std::collections::HashMap;

use bevy::math::{Mat3, Quat, Vec2, Vec3};

pub const GRAVITY: f32 = 9.81;

/// Bike and rider together, kg.
pub const MASS: f32 = 195.0;
/// The rider's share, for the weight shift.
pub const RIDER_MASS: f32 = 90.0;
/// Principal moments about the local X (pitch), Y (yaw), Z (roll) axes.
pub const INERTIA: Vec3 = Vec3::new(55.0, 40.0, 22.0);

/// Linear acceleration of the one gear, m/s², and where it stops pulling.
pub const ACCEL: f32 = 7.0;
pub const TOP_SPEED: f32 = 25.0;
/// Backing up (hold A): a gentle push to a walking pace, m/s² and m/s.
pub const REVERSE_ACCEL: f32 = 2.5;
pub const REVERSE_SPEED: f32 = 3.0;
/// Full brakes, m/s², split front / rear.
pub const BRAKE_DECEL: f32 = 9.0;
pub const BRAKE_FRONT: f32 = 0.65;

/// Tyre grip (friction circle) and how hard a tyre resists sliding sideways.
pub const GRIP: f32 = 1.4;
pub const LATERAL_STIFFNESS: f32 = 3200.0;

/// Handlebar: most lock at a standstill, least at speed, and how fast it turns.
pub const STEER_LOCK_SLOW: f32 = 0.6;
pub const STEER_LOCK_FAST: f32 = 0.12;
pub const STEER_RATE: f32 = 4.0;

/// How far the rider moves his weight at full stick, metres (X left, Z forward).
pub const LEAN_REACH: Vec2 = Vec2::new(0.25, 0.30);
/// How hard the weight shift works on the ground (1 = only gravity).
pub const LEAN_GAIN: f32 = 1.6;
/// Angular accelerations the rider gives in the air at full stick, rad/s².
pub const AIR_PITCH: f32 = 9.0;
pub const AIR_ROLL: f32 = 7.0;
pub const AIR_YAW: f32 = 5.0;

/// Ground balance controller (roll), and the most lean it asks for.
pub const ROLL_KP: f32 = 1500.0;
pub const ROLL_KD: f32 = 420.0;
pub const MAX_LEAN: f32 = 0.8;

/// Seconds lying in the dirt before the bike comes back.
pub const CRASH_SECONDS: f32 = 2.0;

/// One suspension leg and its wheel.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    /// The axle at full extension, bike frame.
    pub extended: Vec3,
    /// The direction the axle moves when the suspension compresses.
    pub axis: Vec3,
    pub travel: f32,
    pub radius: f32,
    /// Spring N/m and damper N·s/m.
    pub spring: f32,
    pub damper: f32,
}

/// The rake of the forks: the steering axis leans back this far from vertical.
pub const RAKE: f32 = 0.47;

pub fn steering_axis() -> Vec3 {
    Vec3::new(0.0, RAKE.cos(), -RAKE.sin())
}

/// Sag: how far each leg sits into its travel with the rider on.
const SAG: f32 = 0.10;

pub fn rear_leg() -> Leg {
    let axis = Vec3::Y;
    Leg {
        extended: Vec3::new(0.0, -0.30, -0.80) - axis * SAG,
        axis,
        travel: 0.32,
        radius: 0.355,
        spring: MASS * GRAVITY * 0.515 / SAG,
        damper: 1100.0,
    }
}

/// The front leg slides straight up the bike like the rear: the forks are
/// drawn raked, but a raked ray would shrink the tyre by the cosine.
pub fn front_leg() -> Leg {
    let axis = Vec3::Y;
    Leg {
        extended: Vec3::new(0.0, -0.30, 0.85) - axis * SAG,
        axis,
        travel: 0.32,
        radius: 0.37,
        spring: MASS * GRAVITY * 0.485 / SAG,
        damper: 1000.0,
    }
}

/// How far above the top of its travel a wheel's ray starts, so a wheel
/// pushed past the bump stop still finds the ground.
const RAY_HEADROOM: f32 = 0.45;
/// The bump stop: past full travel the frame itself hits.
const BUMP_STOP: f32 = 90000.0;

/// Where the rider stands, bike frame (render metres): footpegs (left;
/// mirror X for the right), the middle of the bars at rest, half the bar
/// width, and the hips in the attack position.
pub const PEG: Vec3 = Vec3::new(0.19, -0.25, -0.12);
pub const BAR_HALF_WIDTH: f32 = 0.40;
pub const HIPS: Vec3 = Vec3::new(0.0, 0.55, -0.30);

/// The middle of the bars: on the steering axis, above the fork top.
pub fn bar_centre() -> Vec3 {
    front_leg().extended + steering_axis() * 1.02 - Vec3::Z * 0.02
}

/// Probe spheres on the frame and the rider: bike frame centre, radius,
/// and whether touching anything with it is a crash.
pub const PROBES: [(Vec3, f32, bool); 6] = [
    (Vec3::new(0.0, -0.40, 0.05), 0.14, false), // skid plate
    (Vec3::new(0.0, 0.05, -1.08), 0.12, false), // rear fender
    (Vec3::new(0.0, 0.10, 0.98), 0.12, false),  // front fender
    (Vec3::new(0.0, 0.18, -0.30), 0.16, false), // seat
    (Vec3::new(0.0, 0.62, 0.40), 0.16, true),   // bars and hands
    (Vec3::new(0.0, 1.30, 0.20), 0.24, true),   // the rider's head
];

const PROBE_SPRING: f32 = 90000.0;
const PROBE_DAMPER: f32 = 5000.0;
const PROBE_FRICTION: f32 = 0.6;

/// The controller, already through deadzones.
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    /// Left stick X: -1 left, +1 right.
    pub steer: f32,
    /// Right stick: X -1 left / +1 right, Y -1 back / +1 forward.
    pub lean: Vec2,
    pub throttle: f32,
    pub brake: f32,
    /// Held: back up instead of driving forward.
    pub reverse: bool,
}

/// A ray or probe hit on the world.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    pub distance: f32,
    pub point: Vec3,
    pub normal: Vec3,
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    a: Vec3,
    b: Vec3,
    c: Vec3,
    normal: Vec3,
    two_sided: bool,
}

/// The map's collision in a flat grid of columns (X, Z), for short rays and
/// small spheres.
pub struct World {
    cell: f32,
    triangles: Vec<Triangle>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl World {
    /// Triangles in the renderer frame, wound counter-clockwise seen from the
    /// side you stand on, with their two-sided flag.
    pub fn new(triangles: impl IntoIterator<Item = ([Vec3; 3], bool)>) -> World {
        let cell = 4.0;
        let mut out = Vec::new();
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for ([a, b, c], two_sided) in triangles {
            let normal = (b - a).cross(c - a);
            if !normal.is_finite() || normal.length_squared() < 1e-10 {
                continue;
            }
            let i = out.len() as u32;
            out.push(Triangle {
                a,
                b,
                c,
                normal: normal.normalize(),
                two_sided,
            });
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            for x in (lo.x / cell).floor() as i32..=(hi.x / cell).floor() as i32 {
                for z in (lo.z / cell).floor() as i32..=(hi.z / cell).floor() as i32 {
                    grid.entry((x, z)).or_default().push(i);
                }
            }
        }
        World {
            cell,
            triangles: out,
            grid,
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Triangles whose columns come within `radius` of `centre` (X, Z), once each.
    pub fn gather(&self, centre: Vec3, radius: f32, out: &mut Vec<u32>) {
        out.clear();
        let c = self.cell;
        for x in ((centre.x - radius) / c).floor() as i32..=((centre.x + radius) / c).floor() as i32 {
            for z in ((centre.z - radius) / c).floor() as i32..=((centre.z + radius) / c).floor() as i32 {
                if let Some(list) = self.grid.get(&(x, z)) {
                    out.extend_from_slice(list);
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// The nearest front face along a ray (`dir` unit length) among `candidates`.
    pub fn raycast(&self, candidates: &[u32], origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for &i in candidates {
            let t = &self.triangles[i as usize];
            let facing = dir.dot(t.normal);
            if facing >= 0.0 && !t.two_sided {
                continue;
            }
            let e1 = t.b - t.a;
            let e2 = t.c - t.a;
            let p = dir.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-9 {
                continue;
            }
            let inv = 1.0 / det;
            let s = origin - t.a;
            let u = s.dot(p) * inv;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = dir.dot(q) * inv;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let d = e2.dot(q) * inv;
            if d < 0.0 || d > max || best.is_some_and(|b| b.distance <= d) {
                continue;
            }
            let normal = if facing > 0.0 { -t.normal } else { t.normal };
            best = Some(Hit {
                distance: d,
                point: origin + dir * d,
                normal,
            });
        }
        best
    }

    /// The same, gathering its own candidates (for occasional long rays).
    pub fn raycast_far(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let mut candidates = Vec::new();
        let end = origin + dir * max;
        let mid = (origin + end) * 0.5;
        let reach = Vec2::new(end.x - origin.x, end.z - origin.z).length() * 0.5 + 0.1;
        self.gather(mid, reach, &mut candidates);
        self.raycast(&candidates, origin, dir, max)
    }

    /// Where a sphere overlaps the world: per touching face, the push-out
    /// normal and depth. Faces seen from behind are ignored unless two-sided;
    /// near-parallel contacts are merged, keeping the deepest.
    pub fn sphere(&self, candidates: &[u32], centre: Vec3, radius: f32, out: &mut Vec<(Vec3, f32)>) {
        out.clear();
        for &i in candidates {
            let t = &self.triangles[i as usize];
            let side = (centre - t.a).dot(t.normal);
            if side.abs() >= radius || (side < 0.0 && !t.two_sided) {
                continue;
            }
            let q = closest_point(centre, t.a, t.b, t.c);
            let d = centre - q;
            let dist2 = d.length_squared();
            if dist2 >= radius * radius {
                continue;
            }
            let dist = dist2.sqrt();
            let normal = if dist > 1e-5 {
                d / dist
            } else if side < 0.0 {
                -t.normal
            } else {
                t.normal
            };
            let depth = radius - dist;
            match out.iter_mut().find(|(n, _)| n.dot(normal) > 0.95) {
                Some(c) if c.1 < depth => *c = (normal, depth),
                Some(_) => {}
                None => out.push((normal, depth)),
            }
        }
    }
}

/// The closest point to `p` on triangle `abc`.
fn closest_point(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && d4 - d3 >= 0.0 && d5 - d6 >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// One wheel as last simulated, for drawing and the rider.
#[derive(Clone, Copy, Debug, Default)]
pub struct WheelState {
    /// How far into its travel the suspension is.
    pub compression: f32,
    pub contact: Option<Hit>,
}

/// The bike's state. `steer` is the bar angle in radians, positive to the left.
#[derive(Clone, Debug)]
pub struct Bike {
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    /// World frame, rad/s.
    pub spin: Vec3,
    pub steer: f32,
    /// The rider's weight shift, smoothed from the stick (same axes).
    pub lean: Vec2,
    /// Rear, front.
    pub wheels: [WheelState; 2],
    /// Seconds left lying in the dirt.
    pub crashed: Option<f32>,
    /// Where to come back after a crash: position and heading, recorded
    /// while both wheels are down, the oldest about two seconds back.
    safe: Vec<(Vec3, f32)>,
    safe_clock: f32,
    candidates: Vec<u32>,
    contacts: Vec<(Vec3, f32)>,
}

/// A rotation that faces the bike along `heading` (radians about +Y, 0 is +Z).
pub fn heading_rotation(heading: f32) -> Quat {
    Quat::from_rotation_y(heading)
}

impl Bike {
    /// A bike standing on its wheels at rest with its centre of mass at `position`.
    pub fn new(position: Vec3, heading: f32) -> Bike {
        Bike {
            position,
            rotation: heading_rotation(heading),
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            steer: 0.0,
            lean: Vec2::ZERO,
            wheels: [WheelState::default(); 2],
            crashed: None,
            safe: Vec::new(),
            safe_clock: 0.0,
            candidates: Vec::new(),
            contacts: Vec::new(),
        }
    }

    /// Drops the bike onto the ground under `point` (a camera, a spawn),
    /// facing `heading`.
    pub fn dropped(world: &World, point: Vec3, heading: f32) -> Bike {
        let rest = rest_height();
        let ground = world
            .raycast_far(point + Vec3::Y * 1.0, Vec3::NEG_Y, 400.0)
            .map(|h| h.point);
        let at = match ground {
            Some(g) => g + Vec3::Y * (rest + 0.05),
            None => point,
        };
        Bike::new(at, heading)
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    /// Heading about +Y of the bike's forward direction.
    pub fn heading(&self) -> f32 {
        let f = self.forward();
        f.x.atan2(f.z)
    }

    pub fn speed(&self) -> f32 {
        self.velocity.dot(self.forward())
    }

    pub fn grounded(&self) -> usize {
        self.wheels.iter().filter(|w| w.contact.is_some()).count()
    }

    /// A point in the bike's frame, in the world.
    pub fn to_world(&self, local: Vec3) -> Vec3 {
        self.position + self.rotation * local
    }

    /// The axle of a leg (0 rear, 1 front) at its current compression, bike frame.
    pub fn axle(&self, leg: usize) -> Vec3 {
        let l = if leg == 0 { rear_leg() } else { front_leg() };
        l.extended + l.axis * self.wheels[leg].compression
    }

    /// The bars' rotation about the steering axis, bike frame.
    pub fn bar_rotation(&self) -> Quat {
        Quat::from_axis_angle(steering_axis(), self.steer)
    }

    /// The grips (left, right) in the bike frame, turned with the bars.
    pub fn grips(&self) -> [Vec3; 2] {
        let centre = bar_centre();
        let left = self.bar_rotation() * Vec3::X * BAR_HALF_WIDTH;
        [centre + left, centre - left]
    }

    /// Advances by `dt` seconds in fixed sub-steps.
    pub fn step(&mut self, world: &World, controls: Controls, dt: f32) {
        const SUB: f32 = 1.0 / 240.0;
        let mut controls = controls;
        if let Some(t) = self.crashed.as_mut() {
            *t -= dt;
            controls = Controls::default();
            if *t <= 0.0 {
                self.respawn(world);
                return;
            }
        }
        let reach = 3.0 + self.velocity.length() * dt;
        let mut candidates = std::mem::take(&mut self.candidates);
        world.gather(self.position, reach, &mut candidates);
        let n = ((dt / SUB).ceil() as usize).clamp(1, 40);
        let h = dt / n as f32;
        for _ in 0..n {
            self.sub_step(world, &candidates, controls, h);
        }
        self.candidates = candidates;
        if !self.position.is_finite() || !self.rotation.is_finite() {
            self.respawn(world);
        }
        // Remember where it was riding well.
        self.safe_clock += dt;
        if self.crashed.is_none() && self.grounded() == 2 && self.up().y > 0.8 && self.safe_clock > 0.25 {
            self.safe_clock = 0.0;
            if self.safe.len() >= 8 {
                self.safe.remove(0);
            }
            self.safe.push((self.position, self.heading()));
        }
    }

    /// Back on the wheels where it last rode well (or upright where it is).
    pub fn respawn(&mut self, world: &World) {
        let (at, heading) = self.safe.first().copied().unwrap_or((self.position, self.heading()));
        let safe = std::mem::take(&mut self.safe);
        *self = Bike::dropped(world, at + Vec3::Y * 0.5, heading);
        self.safe = safe;
    }

    /// Stands the bike up where it is, facing the way it was going.
    pub fn reset_here(&mut self, world: &World) {
        let safe = std::mem::take(&mut self.safe);
        *self = Bike::dropped(world, self.position + Vec3::Y * 0.5, self.heading());
        self.safe = safe;
    }

    fn sub_step(&mut self, world: &World, candidates: &[u32], c: Controls, dt: f32) {
        let r = Mat3::from_quat(self.rotation);
        let up = r * Vec3::Y;
        let fwd = r * Vec3::Z;
        let gravity = Vec3::NEG_Y * GRAVITY;
        let mut force = gravity * MASS;
        let mut torque = Vec3::ZERO;
        let point_velocity = |b: &Bike, p: Vec3| b.velocity + b.spin.cross(p - b.position);
        let speed = self.velocity.dot(fwd);

        // The bars: less lock the faster it goes, turned at a steady rate.
        let fast = (speed.abs() / 14.0).clamp(0.0, 1.0);
        let lock = STEER_LOCK_SLOW + (STEER_LOCK_FAST - STEER_LOCK_SLOW) * fast;
        let target = -c.steer.clamp(-1.0, 1.0) * lock;
        self.steer += (target - self.steer).clamp(-STEER_RATE * dt, STEER_RATE * dt);
        // The rider's weight follows the stick quickly.
        let lean_target = c.lean.clamp_length_max(1.0);
        self.lean += (lean_target - self.lean) * (1.0 - (-12.0 * dt).exp());
        // Body steering: leaning into a turn tightens it.
        let body = -self.lean.x * 0.07 * (speed.abs() / 5.0).clamp(0.0, 1.0);
        let steer = self.steer + body;

        // Wheels.
        let mut ground_normal = Vec3::ZERO;
        let mut ground_point = Vec3::ZERO;
        let legs = [rear_leg(), front_leg()];
        for (i, leg) in legs.iter().enumerate() {
            let axis = r * leg.axis;
            let top = self.to_world(leg.extended + leg.axis * (leg.travel + RAY_HEADROOM));
            let reach = leg.travel + leg.radius + RAY_HEADROOM;
            let Some(hit) = world.raycast(candidates, top, -axis, reach) else {
                self.wheels[i] = WheelState::default();
                continue;
            };
            let s = reach - hit.distance;
            let vc = point_velocity(self, hit.point);
            let s_rate = -vc.dot(axis);
            let mut push = leg.spring * s.min(leg.travel) + leg.damper * s_rate;
            if s > leg.travel {
                push += BUMP_STOP * (s - leg.travel);
            }
            let push = push.max(0.0);
            self.wheels[i] = WheelState {
                compression: s.min(leg.travel),
                contact: Some(hit),
            };
            let n = hit.normal;
            // The ground pushes back along its normal (pushing along the
            // raked fork would shove the bike backwards).
            let f_susp = n * push;
            ground_normal += n;
            ground_point += hit.point;

            // The tyre: which way it rolls, flattened onto the ground.
            let heading = if i == 1 {
                r * (Quat::from_axis_angle(steering_axis(), steer) * Vec3::Z)
            } else {
                fwd
            };
            let long = (heading - n * n.dot(heading)).normalize_or(fwd);
            let lat = n.cross(long);
            // The tyre slides with the bike's travel and yaw, not with its
            // lean: a bike rolls about its tyres, which the balance below
            // stands in for.
            let roll = fwd * self.spin.dot(fwd);
            let vc = self.velocity + (self.spin - roll).cross(hit.point - self.position);
            let v_long = vc.dot(long);
            let v_lat = vc.dot(lat);
            let load = push;
            let mut f_long = -v_long * 6.0;
            if i == 0 && c.reverse {
                let pull = ((REVERSE_SPEED + v_long) / 0.5).clamp(0.0, 1.0);
                f_long -= MASS * REVERSE_ACCEL * pull;
            } else if i == 0 && c.throttle > 0.0 {
                let pull = ((TOP_SPEED - v_long) / 1.5).clamp(0.0, 1.0);
                f_long += c.throttle.min(1.0) * MASS * ACCEL * pull;
            }
            if c.brake > 0.0 {
                let share = if i == 1 { BRAKE_FRONT } else { 1.0 - BRAKE_FRONT };
                let max = c.brake.min(1.0) * MASS * BRAKE_DECEL * share;
                f_long += (-v_long * 4000.0).clamp(-max, max);
            }
            let f_lat = -v_lat * LATERAL_STIFFNESS;
            let mut f_tyre = long * f_long + lat * f_lat;
            let limit = GRIP * load;
            if f_tyre.length() > limit {
                f_tyre = f_tyre.normalize_or_zero() * limit;
            }
            let f = f_susp + f_tyre;
            force += f;
            // Lean is the balance controller's alone (below): the tyres
            // only pitch and yaw the bike.
            let t = (hit.point - self.position).cross(f);
            torque += t - fwd * t.dot(fwd);
        }

        let grounded = self.grounded();
        let lean_local = Vec3::new(-self.lean.x * LEAN_REACH.x, 0.0, self.lean.y * LEAN_REACH.y);
        if grounded > 0 {
            // The rider's weight, moved off the centre.
            let arm = r * lean_local;
            let t = arm.cross(gravity * RIDER_MASS * LEAN_GAIN);
            torque += t - fwd * t.dot(fwd);

            // Balance: lean into the turn as far as speed and steer ask.
            let reference = ground_normal.normalize_or(Vec3::Y);
            let reference = (reference - fwd * reference.dot(fwd)).normalize_or(Vec3::Y);
            let lean_now = fwd.dot(reference.cross(up)).atan2(reference.dot(up));
            // The lean that balances the turn it is really making
            // (tan lean = v * yaw rate / g), so a sliding front leans less.
            let yaw_rate = self.spin.dot(reference);
            let wanted = -(speed.max(0.0) * yaw_rate / GRAVITY).atan();
            // Leaning the rider over adds a little more.
            let wanted = (wanted + self.lean.x * 0.25).clamp(-MAX_LEAN, MAX_LEAN);
            let share = grounded as f32 / 2.0;
            let roll = ROLL_KP * (wanted - lean_now) - ROLL_KD * self.spin.dot(fwd);
            torque += fwd * roll * share;
        } else {
            // In the air the rider throws the bike around.
            let i = INERTIA;
            let local = Vec3::new(
                self.lean.y * AIR_PITCH * i.x,
                (-self.lean.x * AIR_YAW - c.steer * 1.5) * i.y,
                self.lean.x * AIR_ROLL * i.z,
            );
            torque += r * local;
            // A little air drag on the spin so it settles.
            torque -= self.spin * 4.0;
        }

        // The frame and the rider against the world.
        let mut crash = false;
        let mut contacts = std::mem::take(&mut self.contacts);
        let probes = PROBES.iter().map(|&(p, rad, fatal)| (p, rad, fatal, None)).chain(
            // The wheels against walls (relative to the bike), so a tyre
            // into a wall stops rather than climbs.
            (0..2).map(|i| (self.axle(i), legs[i].radius * 0.95, false, Some(up))),
        );
        for (local, radius, fatal, walls_only) in probes {
            let centre = self.to_world(local);
            world.sphere(candidates, centre, radius, &mut contacts);
            for &(n, depth) in &contacts {
                if walls_only.is_some_and(|u: Vec3| n.dot(u) > 0.5) {
                    continue;
                }
                let p = centre - n * (radius - depth);
                let vp = point_velocity(self, p);
                let vn = vp.dot(n);
                let push = (PROBE_SPRING * depth - PROBE_DAMPER * vn).max(0.0);
                let vt = vp - n * vn;
                let rub = vt.normalize_or_zero() * -(PROBE_FRICTION * push).min(vt.length() * 3000.0);
                let f = n * push + rub;
                force += f;
                torque += (p - self.position).cross(f);
                crash |= fatal;
            }
        }
        self.contacts = contacts;
        // Upside down on the ground is a crash too.
        if grounded == 0 && up.y < -0.3 && self.contacts_any(world, candidates) {
            crash = true;
        }
        if crash && self.crashed.is_none() {
            self.crashed = Some(CRASH_SECONDS);
        }

        // Integrate (semi-implicit Euler).
        self.velocity += force / MASS * dt;
        self.position += self.velocity * dt;
        let inertia = r * Mat3::from_diagonal(INERTIA) * r.transpose();
        let inverse = r * Mat3::from_diagonal(INERTIA.recip()) * r.transpose();
        let gyro = self.spin.cross(inertia * self.spin);
        self.spin += inverse * (torque - gyro) * dt;
        self.spin = self.spin.clamp_length_max(20.0);
        let turn = Quat::from_scaled_axis(self.spin * dt);
        if grounded > 0 {
            // A bike leans about its tyres, not its middle: swing the centre
            // of mass around the contact line by the lean part of the turn.
            let pivot = ground_point / grounded as f32;
            let lean = Quat::from_scaled_axis(fwd * self.spin.dot(fwd) * dt);
            let arm = self.position - pivot;
            self.position += lean * arm - arm;
        }
        self.rotation = (turn * self.rotation).normalize();
    }

    /// Whether any frame probe touches the world (for the upside-down test).
    fn contacts_any(&self, world: &World, candidates: &[u32]) -> bool {
        let mut scratch = Vec::new();
        PROBES.iter().any(|&(p, radius, _)| {
            world.sphere(candidates, self.to_world(p), radius, &mut scratch);
            !scratch.is_empty()
        })
    }
}

/// How high the centre of mass sits over flat ground at rest.
pub fn rest_height() -> f32 {
    let l = rear_leg();
    -(l.extended.y + l.axis.y * SAG) + l.radius
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat floor, 400 m square, at y = 0.
    fn floor() -> World {
        let s = 200.0;
        let a = Vec3::new(-s, 0.0, -s);
        let b = Vec3::new(s, 0.0, -s);
        let c = Vec3::new(s, 0.0, s);
        let d = Vec3::new(-s, 0.0, s);
        // Counter-clockwise seen from above (+Y).
        World::new([([a, d, c], false), ([a, c, b], false)])
    }

    fn ride(world: &World, bike: &mut Bike, c: Controls, seconds: f32) {
        let frames = (seconds * 60.0) as usize;
        for _ in 0..frames {
            bike.step(world, c, 1.0 / 60.0);
        }
    }

    #[test]
    fn floor_faces_up() {
        let w = floor();
        let hit = w.raycast_far(Vec3::new(1.0, 5.0, 1.0), Vec3::NEG_Y, 10.0).unwrap();
        assert!((hit.distance - 5.0).abs() < 1e-4);
        assert!(hit.normal.y > 0.99);
        // From below, the floor is not there.
        assert!(w.raycast_far(Vec3::new(1.0, -5.0, 1.0), Vec3::Y, 10.0).is_none());
    }

    #[test]
    fn settles_at_rest() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 3.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 3.0);
        assert_eq!(bike.grounded(), 2);
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.99, "up {:?}", bike.up());
        assert!((bike.position.y - rest_height()).abs() < 0.06, "height {}", bike.position.y);
        assert!(bike.velocity.length() < 0.05);
    }

    #[test]
    fn one_long_gear_to_top_speed() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -150.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 1.0);
        let throttle = Controls {
            throttle: 1.0,
            lean: Vec2::new(0.0, 0.6),
            ..default()
        };
        ride(&w, &mut bike, throttle, 1.0);
        let after_one = bike.speed();
        assert!(after_one > ACCEL * 0.6 && after_one < ACCEL * 1.1, "1 s: {after_one} m/s");
        ride(&w, &mut bike, throttle, 4.5);
        let top = bike.speed();
        assert!((top - TOP_SPEED).abs() < 1.5, "top speed {top}");
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);
    }

    #[test]
    fn backs_up_slowly() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(
            &w,
            &mut bike,
            Controls {
                reverse: true,
                ..default()
            },
            5.0,
        );
        let v = bike.speed();
        assert!((v + REVERSE_SPEED).abs() < 0.3, "reversing at {v} m/s");
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);
    }

    #[test]
    fn brakes_stop_it() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -150.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        let gas = Controls {
            throttle: 1.0,
            lean: Vec2::new(0.0, 0.6),
            ..default()
        };
        ride(&w, &mut bike, gas, 2.0);
        ride(
            &w,
            &mut bike,
            Controls {
                brake: 1.0,
                ..default()
            },
            3.0,
        );
        assert!(bike.speed().abs() < 0.3, "still at {} m/s", bike.speed());
        assert!(bike.crashed.is_none());
    }

    #[test]
    fn steers_and_leans_into_the_turn() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        let gas = Controls {
            throttle: 0.6,
            ..default()
        };
        ride(&w, &mut bike, gas, 1.5);
        let before = bike.heading();
        // Right on the stick: the heading turns right (towards -X, so the
        // angle about +Y falls) and the bike leans right.
        ride(
            &w,
            &mut bike,
            Controls {
                throttle: 0.3,
                steer: 1.0,
                ..default()
            },
            0.8,
        );
        let turned = bike.heading() - before;
        assert!(turned < -0.2, "turned {turned} rad");
        let f = bike.forward();
        let right = Vec3::new(-f.z, 0.0, f.x).normalize();
        assert!(bike.up().dot(right) > 0.2, "lean {:?}", bike.up());
        assert!(bike.crashed.is_none());
        assert_eq!(bike.grounded(), 2);
    }

    #[test]
    fn leaning_back_on_the_gas_lifts_the_front() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -150.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        let mut lifted = false;
        for _ in 0..90 {
            bike.step(
                &w,
                Controls {
                    throttle: 1.0,
                    lean: Vec2::new(0.0, -1.0),
                    ..default()
                },
                1.0 / 60.0,
            );
            lifted |= bike.wheels[1].contact.is_none() && bike.wheels[0].contact.is_some();
        }
        assert!(lifted, "never wheelied");
    }

    #[test]
    fn air_pitch_follows_the_stick() {
        let w = floor();
        let mut bike = Bike::new(Vec3::new(0.0, 60.0, 0.0), 0.0);
        ride(
            &w,
            &mut bike,
            Controls {
                lean: Vec2::new(0.0, 1.0),
                ..default()
            },
            0.5,
        );
        // Forward on the stick: nose down.
        assert!(bike.forward().y < -0.3, "forward {:?}", bike.forward());
    }

    #[test]
    fn landing_upside_down_crashes_and_comes_back() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 1.0);
        bike.position.y += 3.0;
        bike.rotation = Quat::from_rotation_z(std::f32::consts::PI);
        let mut crashed = false;
        for _ in 0..120 {
            bike.step(&w, Controls::default(), 1.0 / 60.0);
            crashed |= bike.crashed.is_some();
        }
        assert!(crashed);
        ride(&w, &mut bike, Controls::default(), CRASH_SECONDS + 1.0);
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);
    }

    #[test]
    fn wall_stops_it() {
        // Floor plus a wall facing -Z at z = 10.
        let s = 50.0;
        let wall = [
            Vec3::new(-s, 0.0, 10.0),
            Vec3::new(s, 0.0, 10.0),
            Vec3::new(s, 10.0, 10.0),
            Vec3::new(-s, 10.0, 10.0),
        ];
        let f = [
            Vec3::new(-s, 0.0, -s),
            Vec3::new(s, 0.0, -s),
            Vec3::new(s, 0.0, s),
            Vec3::new(-s, 0.0, s),
        ];
        let w = World::new([
            ([f[0], f[3], f[2]], false),
            ([f[0], f[2], f[1]], false),
            // Normal -Z: seen from the bike's side.
            ([wall[0], wall[2], wall[1]], false),
            ([wall[0], wall[3], wall[2]], false),
        ]);
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(
            &w,
            &mut bike,
            Controls {
                throttle: 0.5,
                ..default()
            },
            4.0,
        );
        assert!(bike.position.z < 10.0, "through the wall: z {}", bike.position.z);
    }

    fn default() -> Controls {
        Controls::default()
    }
}

