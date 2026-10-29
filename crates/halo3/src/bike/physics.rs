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
//! The model follows stock MX vs ATV Reflex's mx450f, as read out of the
//! game's data and code in `D:\reflex-tools\physics` (`reflex\docs\fields\
//! lean.md`, `susp-tires.md`; numbers from `dumps\stock-json`):
//! - Reflex's engine and automatic five-speed: its torque curve through the
//!   gear that gives the most wheel torque for the speed, engine braking off
//!   the throttle, a hard limiter; the tyres decide how much reaches the
//!   ground;
//! - the left stick turns the bars through a speed curve (30 deg standing,
//!   9.5 at 50 mph), squared and lagged;
//! - the lean is placed, not balanced: a lean-for-speed curve times the
//!   stick sets the angle, and the bike is turned about its tyres to it, so
//!   a turn can never tip it over;
//! - each tyre cancels its contact patch's sideways slip, up to grip x load;
//!   past the friction circle it slides, and a sliding tyre gains a side force
//!   that grows with the slide, so slides catch themselves;
//! - the right stick moves the rider: on the ground his weight presses the
//!   front or back down (wheelies, stoppies) and side to side leans the bike
//!   further and tightens the turn; in the air it rotates the bike (pitch,
//!   lean and whip).

use std::collections::HashMap;

use bevy::math::{Mat3, Quat, Vec2, Vec3};

pub const GRAVITY: f32 = 9.81;

/// Bike and rider together, kg: Reflex's chassis 99, rider 58, wheels 19.
pub const MASS: f32 = 177.0;
/// The rider's share, for the weight shift.
pub const RIDER_MASS: f32 = 58.0;
/// Principal moments about the local X (pitch), Y (yaw), Z (roll) axes:
/// Reflex's chassis (13.6 / 19.0 / 13.6) with the rider and the wheels
/// added about the shared centre of mass.
pub const INERTIA: Vec3 = Vec3::new(36.0, 32.0, 28.0);

/// Engine torque by crank speed: rpm, N·m (Reflex stock: 55 N·m peak at
/// about 5400 rpm, nothing left at the limiter).
const TORQUE_CURVE: [(f32, f32); 9] = [
    (0.0, 13.4),
    (1990.0, 40.1),
    (3380.0, 50.6),
    (4613.0, 54.5),
    (6034.0, 54.2),
    (7456.0, 45.0),
    (8467.0, 30.6),
    (9604.0, 14.8),
    (11500.0, 0.0),
];
/// Engine braking off the throttle: this much drag at the crank, N·m, times
/// a share by rpm.
pub const ENGINE_DRAG: f32 = 27.1;
const DRAG_CURVE: [(f32, f32); 6] = [
    (0.0, 0.0),
    (2717.0, 0.162),
    (3918.0, 0.11),
    (6730.0, 0.734),
    (8815.0, 0.734),
    (11500.0, 1.0),
];
pub const IDLE_RPM: f32 = 1500.0;
pub const LIMITER_RPM: f32 = 11500.0;
/// Overall ratios, crank to rear wheel, first to fifth.
pub const GEARS: [f32; 5] = [19.72, 16.11, 13.53, 11.50, 9.63];
/// The rear tyre radius the gearing works through (Reflex's 0.317 m; the
/// drawn tyre is a little bigger).
pub const GEAR_RADIUS: f32 = 0.317;
/// Seconds since the last shift before the box shifts up, or down.
const UPSHIFT_WAIT: f32 = 0.3;
const DOWNSHIFT_WAIT: f32 = 0.15;
/// Ground drag on the whole bike by speed: m/s, N (Reflex stock).
const GROUND_DRAG: [(f32, f32); 4] = [(0.0, 0.0), (17.6, 107.0), (31.4, 458.0), (45.2, 663.0)];
/// Backing up (hold A): a push that fades out between these speeds, N, m/s.
pub const REVERSE_FORCE: f32 = 667.0;
const REVERSE_FADE: (f32, f32) = (0.76, 3.81);

/// The engine's torque at an rpm, N·m.
pub fn engine_torque(rpm: f32) -> f32 {
    lerp_table(&TORQUE_CURVE, rpm)
}

/// The crank speed in a gear for a forward speed, rpm: clutch always in,
/// held up at idle, never past the limiter.
fn gear_rpm(ratio: f32, speed: f32) -> f32 {
    let wheel = (speed / GEAR_RADIUS).max(0.5);
    (wheel * ratio * 60.0 / std::f32::consts::TAU).clamp(IDLE_RPM, LIMITER_RPM)
}

/// The gear with the most torque at the wheel for a speed (Reflex's
/// automatic): ties go to the lower gear.
fn best_gear(gearing: f32, speed: f32) -> usize {
    let mut best = (0, f32::MIN);
    for (i, &g) in GEARS.iter().enumerate() {
        let ratio = g * gearing;
        let score = ratio * engine_torque(gear_rpm(ratio, speed));
        if score > best.1 + 1e-3 {
            best = (i, score);
        }
    }
    best.0
}
/// Full brakes, m/s², split front / rear: Reflex's 339 N·m front and
/// 475 N·m rear at the tyre radii, a rear-biased pair.
pub const BRAKE_DECEL: f32 = 13.8;
pub const BRAKE_FRONT: f32 = 0.39;

/// Tyre grip on normal ground, front and rear alike (Reflex 1.3).
pub const GRIP: f32 = 1.3;
/// A sliding tyre keeps this share of its grip...
pub const SLIDE_GRIP: f32 = 0.9;
/// ...plus a side force that grows with the slide (Reflex 300 lbf), N,
/// reached at `SLIDE_CURVE`'s last speed.
pub const SLIDE_FORCE: f32 = 1334.0;
/// The side force's share by sideways slide speed, m/s.
const SLIDE_CURVE: [(f32, f32); 5] = [(0.0, 0.0), (4.5, 0.26), (8.9, 0.72), (13.3, 0.93), (17.9, 1.0)];
/// A sliding tyre's grip along its rolling direction.
const SLIDE_LONG: f32 = 0.8;

const MPH: f32 = 0.44704;

/// Full-stick bar angle by forward speed: mph, degrees (Reflex stock).
const STEER_CURVE: [(f32, f32); 8] = [
    (0.0, 30.0),
    (10.0, 29.3),
    (20.0, 27.5),
    (30.0, 23.5),
    (40.0, 16.1),
    (50.0, 9.5),
    (60.0, 6.7),
    (80.0, 4.7),
];
/// The most the bars ever turn.
const STEER_MAX: f32 = 30.0 * std::f32::consts::PI / 180.0;
/// Seconds the bars lag the stick.
pub const STEER_LAG: f32 = 0.15;

/// Full-stick lean by forward speed: mph, degrees (Reflex stock).
const LEAN_CURVE: [(f32, f32); 7] = [
    (0.0, 5.6),
    (10.0, 13.0),
    (20.0, 17.0),
    (30.0, 20.0),
    (40.0, 24.0),
    (50.0, 27.0),
    (80.0, 34.0),
];
/// Seconds the lean lags its target.
pub const LEAN_LAG: f32 = 0.3;
/// How much the rider leaning into a turn adds: to the lean (full rider
/// lean with full steer leans x1.55) and to the bar angle.
pub const RIDER_LEAN: f32 = 0.55;
pub const RIDER_STEER: f32 = 0.2;

/// A curve given in mph and degrees, at a speed in m/s, in radians.
fn mph_curve(curve: &[(f32, f32)], speed: f32) -> f32 {
    lerp_table(curve, speed.abs() / MPH).to_radians()
}

/// Piecewise linear through `table`, held flat past both ends.
fn lerp_table(table: &[(f32, f32)], x: f32) -> f32 {
    let (first, last) = (table[0], table[table.len() - 1]);
    if x <= first.0 {
        return first.1;
    }
    for w in table.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x <= x1 {
            return y0 + (y1 - y0) * (x - x0) / (x1 - x0);
        }
    }
    last.1
}

/// The bars at full stick for a speed (m/s), radians, before tuning.
pub fn steer_lock(speed: f32) -> f32 {
    mph_curve(&STEER_CURVE, speed)
}

/// The lean at full stick for a speed (m/s), radians, before tuning.
pub fn lean_for(speed: f32) -> f32 {
    mph_curve(&LEAN_CURVE, speed)
}

/// How far the rider moves his weight at full stick, metres (X left, Z forward).
pub const LEAN_REACH: Vec2 = Vec2::new(0.25, 0.30);
/// How hard the weight shift works on the ground (1 = only gravity):
/// Reflex's rider is light (58 kg) but a body of his own that throws the
/// bike about, more than his weight alone would.
pub const LEAN_GAIN: f32 = 2.5;
/// Angular accelerations the rider gives in the air at full stick, rad/s².
pub const AIR_PITCH: f32 = 9.0;
pub const AIR_ROLL: f32 = 7.0;
pub const AIR_YAW: f32 = 5.0;

/// The rear wheel and the engine turning it, as felt at the axle, kg·m².
/// Speeding it up or slowing it down twists the frame the other way: gas in
/// the air lifts the front, the rear brake drops it.
pub const WHEEL_INERTIA: f32 = 0.45;
/// How fast the unloaded rear wheel spins up on full gas in the air, rad/s²
/// (as far as the limiter in the gear it is in).
pub const WHEEL_REV_ACCEL: f32 = 40.0;
/// The rear brake locks the wheel in the air, rad/s², and with neither
/// pedal the engine slows it gently (time constant, seconds).
pub const WHEEL_BRAKE_DECEL: f32 = 300.0;
pub const WHEEL_COAST_SECONDS: f32 = 4.0;

/// Seconds lying in the dirt before the bike comes back.
pub const CRASH_SECONDS: f32 = 2.0;

/// The values the tuning menu (`tune.rs`) can change while riding. The
/// default is the stock bike: the constants above.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    /// Scale the engine's torque, the gear ratios and the engine braking.
    pub torque: f32,
    pub gearing: f32,
    pub engine_brake: f32,
    pub brake_decel: f32,
    pub brake_front: f32,
    pub grip: f32,
    /// The sliding side force, N.
    pub slide_force: f32,
    /// Scales the steering curve.
    pub steer_lock: f32,
    pub steer_lag: f32,
    /// Scales the lean curve.
    pub lean_depth: f32,
    pub lean_lag: f32,
    /// Scales what the rider's sideways lean adds to the lean and the bars.
    pub rider_lean: f32,
    pub lean_gain: f32,
    /// Scale the suspension springs and dampers.
    pub spring: f32,
    pub damper: f32,
    /// Scales the rider's spin in the air.
    pub air: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Tuning {
            torque: 1.0,
            gearing: 1.0,
            engine_brake: 1.0,
            brake_decel: BRAKE_DECEL,
            brake_front: BRAKE_FRONT,
            grip: GRIP,
            slide_force: SLIDE_FORCE,
            steer_lock: 1.0,
            steer_lag: STEER_LAG,
            lean_depth: 1.0,
            lean_lag: LEAN_LAG,
            rider_lean: 1.0,
            lean_gain: LEAN_GAIN,
            spring: 1.0,
            damper: 1.0,
            air: 1.0,
        }
    }
}

/// One suspension leg and its wheel.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    /// The axle at full extension, bike frame.
    pub extended: Vec3,
    /// The direction the axle moves when the suspension compresses.
    pub axis: Vec3,
    pub travel: f32,
    pub radius: f32,
    /// Tread width, metres (drawn only).
    pub width: f32,
    /// Spring N/m, and the damper compressing (bump) and extending
    /// (rebound), N·s/m.
    pub spring: f32,
    pub bump: f32,
    pub rebound: f32,
}

/// The rake of the forks: the steering axis leans back this far from vertical.
pub const RAKE: f32 = 0.47;

pub fn steering_axis() -> Vec3 {
    Vec3::new(0.0, RAKE.cos(), -RAKE.sin())
}

/// Where the axles sit with the rider on: below the centre of mass, and
/// along the bike either side of it. A long wheelbase (1.75 m) for a
/// steadier ride.
const AXLE_Y: f32 = -0.30;
const REAR_Z: f32 = -0.85;
const FRONT_Z: f32 = 0.90;

/// The share of the weight each wheel carries, from the lever arms.
fn front_share() -> f32 {
    -REAR_Z / (FRONT_Z - REAR_Z)
}

/// A leg whose axle sits at `rest` under its share of the weight: full
/// extension is that far down again by the sag the spring gives.
fn leg(rest: Vec3, share: f32, spring: f32) -> Leg {
    let axis = Vec3::Y;
    let sag = MASS * GRAVITY * share / spring;
    Leg {
        extended: rest - axis * sag,
        axis,
        travel: 0.0,
        radius: 0.0,
        width: 0.0,
        spring,
        bump: 0.0,
        rebound: 0.0,
    }
}

/// Reflex's rear: a 13.9 kN/m (950 lbf/ft) spring, 584 / 1167 N·s/m.
pub fn rear_leg() -> Leg {
    Leg {
        travel: 0.37,
        radius: 0.355,
        width: 0.13,
        bump: 584.0,
        rebound: 1167.0,
        ..leg(Vec3::new(0.0, AXLE_Y, REAR_Z), 1.0 - front_share(), 13860.0)
    }
}

/// Reflex's forks: 20.2 kN/m measured straight up (1100 lbf/ft along the
/// raked fork), 438 / 876 N·s/m. The leg slides straight up the bike like
/// the rear: the forks are drawn raked, but a raked ray would shrink the
/// tyre by the cosine.
pub fn front_leg() -> Leg {
    Leg {
        travel: 0.40,
        radius: 0.37,
        width: 0.11,
        bump: 438.0,
        rebound: 876.0,
        ..leg(Vec3::new(0.0, AXLE_Y, FRONT_Z), front_share(), 20200.0)
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

/// Where the forks are drawn from: the front axle 10 cm below its rest
/// height, so the bars and the triple clamp stay put whatever the spring.
pub fn fork_base() -> Vec3 {
    Vec3::new(0.0, AXLE_Y - 0.10, FRONT_Z)
}

/// The middle of the bars: on the steering axis, above the fork top.
pub fn bar_centre() -> Vec3 {
    fork_base() + steering_axis() * 1.02 - Vec3::Z * 0.02
}

/// Probe spheres on the frame and the rider: bike frame centre, radius,
/// and whether touching anything with it is a crash.
pub const PROBES: [(Vec3, f32, bool); 6] = [
    (Vec3::new(0.0, -0.40, 0.05), 0.14, false), // skid plate
    (Vec3::new(0.0, 0.05, -1.13), 0.12, false), // rear fender
    (Vec3::new(0.0, 0.10, 1.03), 0.12, false),  // front fender
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

/// A tyre on the ground this sub-step, for the tyre solve.
#[derive(Clone, Copy, Debug)]
struct Tyre {
    point: Vec3,
    /// Rolling and sideways directions, along the ground.
    long: Vec3,
    lat: Vec3,
    /// The suspension's push, N.
    load: f32,
    /// Drive, brake and rolling drag along `long`, N, before grip.
    f_long: f32,
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
    /// The steer stick, squared and lagged.
    steer_in: f32,
    /// The lean the bike is placed at on the ground, radians, positive right.
    lean_angle: f32,
    /// The ground's up, slowly filtered: what the lean is measured from.
    up_ref: Vec3,
    /// The rider's weight shift, smoothed from the stick (same axes).
    pub lean: Vec2,
    /// Rear, front.
    pub wheels: [WheelState; 2],
    /// The rear wheel's spin, rad/s, positive rolling forward.
    pub wheel_spin: f32,
    /// The gear (0 is first) and the crank speed, rpm.
    pub gear: usize,
    pub rpm: f32,
    /// Seconds since the last shift.
    shift_clock: f32,
    /// Seconds left lying in the dirt.
    pub crashed: Option<f32>,
    pub tuning: Tuning,
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
            steer_in: 0.0,
            lean_angle: 0.0,
            up_ref: Vec3::Y,
            lean: Vec2::ZERO,
            wheels: [WheelState::default(); 2],
            wheel_spin: 0.0,
            gear: 0,
            rpm: IDLE_RPM,
            shift_clock: 0.0,
            crashed: None,
            tuning: Tuning::default(),
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
        let (safe, tuning) = (std::mem::take(&mut self.safe), self.tuning);
        *self = Bike::dropped(world, at + Vec3::Y * 0.5, heading);
        self.safe = safe;
        self.tuning = tuning;
    }

    /// Stands the bike up where it is, facing the way it was going.
    pub fn reset_here(&mut self, world: &World) {
        let (safe, tuning) = (std::mem::take(&mut self.safe), self.tuning);
        *self = Bike::dropped(world, self.position + Vec3::Y * 0.5, self.heading());
        self.safe = safe;
        self.tuning = tuning;
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
        let t = self.tuning;
        let lag = |seconds: f32| 1.0 - (-dt / seconds.max(1e-3)).exp();

        // The rider's weight follows the stick quickly.
        let lean_target = c.lean.clamp_length_max(1.0);
        self.lean += (lean_target - self.lean) * (1.0 - (-12.0 * dt).exp());

        // The bars: the stick squared and lagged, times the lock for the
        // speed. The rider leaning the same way turns them further; leaning
        // against the turn takes some away.
        let stick = c.steer.clamp(-1.0, 1.0);
        self.steer_in += (stick * stick.abs() - self.steer_in) * lag(t.steer_lag);
        let lock = steer_lock(speed) * t.steer_lock;
        let rider = self.lean.x * t.rider_lean;
        let agrees = rider * self.steer_in >= 0.0;
        let bars = if agrees {
            self.steer_in * lock + RIDER_STEER * lock * rider
        } else {
            self.steer_in * lock * (1.0 - RIDER_STEER * rider.abs())
        };
        // Positive is to the left; the stick's positive is right.
        self.steer = -bars.clamp(-STEER_MAX, STEER_MAX);

        // The lean it heads for: the curve for the speed times the steer,
        // more when the rider leans into it. Positive leans right.
        let rider_lean = if agrees { RIDER_LEAN * rider } else { 0.0 };
        let wanted = lean_for(speed) * t.lean_depth * (stick + rider_lean);
        self.lean_angle += (wanted - self.lean_angle) * lag(t.lean_lag);

        // The engine and the automatic box. Any brake shuts the throttle.
        let throttle = if c.brake > 0.0 { 0.0 } else { c.throttle.clamp(0.0, 1.0) };
        self.shift_clock += dt;
        if self.grounded() > 0 {
            let best = best_gear(t.gearing, speed);
            if best > self.gear && throttle > 0.0 && self.shift_clock >= UPSHIFT_WAIT {
                self.gear = best;
                self.shift_clock = 0.0;
            } else if best < self.gear && self.shift_clock >= DOWNSHIFT_WAIT {
                // On the gas one gear at a time; off it, straight down.
                self.gear = if throttle > 0.0 { self.gear - 1 } else { best };
                self.shift_clock = 0.0;
            }
        }
        let ratio = GEARS[self.gear] * t.gearing;
        self.rpm = gear_rpm(ratio, speed);
        // Engine braking fades out as the throttle opens.
        let drag = -ratio * ENGINE_DRAG * t.engine_brake * lerp_table(&DRAG_CURVE, self.rpm);
        let drive = ratio * engine_torque(self.rpm) * t.torque;

        // Wheels: suspension now, the tyres once the other forces are in.
        let mut ground_normal = Vec3::ZERO;
        let mut ground_point = Vec3::ZERO;
        let mut tyres: [Option<Tyre>; 2] = [None, None];
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
            let damper = if s_rate > 0.0 { leg.bump } else { leg.rebound };
            let mut push = leg.spring * t.spring * s.min(leg.travel) + damper * t.damper * s_rate;
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
            force += f_susp;
            let tq = (hit.point - self.position).cross(f_susp);
            torque += tq - fwd * tq.dot(fwd);

            // The tyre: which way it rolls, flattened onto the ground.
            let heading = if i == 1 {
                r * (Quat::from_axis_angle(steering_axis(), self.steer) * Vec3::Z)
            } else {
                fwd
            };
            let long = (heading - n * n.dot(heading)).normalize_or(fwd);
            let v_long = point_velocity(self, hit.point).dot(long);
            // Rolling resistance; the ground drag is on the whole bike below.
            let mut f_long = -v_long * 2.0;
            if i == 0 && c.reverse {
                let (from, to) = REVERSE_FADE;
                let fade = 1.0 - ((v_long.abs() - from) / (to - from)).clamp(0.0, 1.0);
                f_long -= REVERSE_FORCE * if v_long > 0.5 { 0.0 } else { fade };
            } else if i == 0 && v_long > -0.5 {
                // Engine braking only holds back a turning wheel; it fades
                // out as the throttle opens.
                let drag = drag * (v_long / 1.0).clamp(0.0, 1.0);
                f_long += (drag + (drive - drag) * throttle) / GEAR_RADIUS;
            }
            if c.brake > 0.0 {
                let share = if i == 1 { t.brake_front } else { 1.0 - t.brake_front };
                let max = c.brake.min(1.0) * MASS * t.brake_decel * share;
                f_long += (-v_long * 4000.0).clamp(-max, max);
            }
            tyres[i] = Some(Tyre {
                point: hit.point,
                long,
                lat: n.cross(long),
                load: push,
                f_long,
            });
        }

        // The rear wheel. On the ground it rolls with the bike (the tyre
        // force above already pitches the frame). Off the ground nothing
        // holds it, so gas spins it up and the brake locks it, and the frame
        // takes the opposite twist about the axle.
        let rear_radius = legs[0].radius;
        if self.wheels[0].contact.is_some() {
            self.wheel_spin = speed / rear_radius;
        } else {
            let before = self.wheel_spin;
            if c.brake > 0.0 {
                let step = c.brake.min(1.0) * WHEEL_BRAKE_DECEL * dt;
                self.wheel_spin -= self.wheel_spin.clamp(-step, step);
            } else if c.throttle > 0.0 && !c.reverse {
                let limit = LIMITER_RPM / 60.0 * std::f32::consts::TAU / ratio;
                let step = c.throttle.min(1.0) * WHEEL_REV_ACCEL * dt;
                self.wheel_spin = (self.wheel_spin + step).min(limit.max(before));
            } else {
                self.wheel_spin *= (-dt / WHEEL_COAST_SECONDS).exp();
            }
            // The wheel spins about the bike's +X (rolling forward), and
            // +X on the frame is nose down: speeding the wheel up lifts the
            // nose, slowing it drops it.
            let reaction = -WHEEL_INERTIA * (self.wheel_spin - before) / dt;
            torque += r * Vec3::X * reaction;
            // The engine turns with the free wheel.
            self.rpm = (self.wheel_spin * ratio * 60.0 / std::f32::consts::TAU).clamp(IDLE_RPM, LIMITER_RPM);
        }

        let grounded = self.grounded();
        if grounded > 0 {
            // Ground drag, against the way it is going.
            let flat = self.velocity - Vec3::Y * self.velocity.y;
            force -= flat.normalize_or_zero() * lerp_table(&GROUND_DRAG, flat.length());
        }
        let lean_local = Vec3::new(-self.lean.x * LEAN_REACH.x, 0.0, self.lean.y * LEAN_REACH.y);
        if grounded > 0 {
            // The rider's weight, moved off the centre: it pitches the bike
            // (its sideways part is in the lean above).
            let arm = r * lean_local;
            let tq = arm.cross(gravity * RIDER_MASS * t.lean_gain);
            torque += tq - fwd * tq.dot(fwd);
        } else {
            // In the air the rider throws the bike around.
            let i = INERTIA;
            let local = Vec3::new(
                self.lean.y * AIR_PITCH * i.x,
                (-self.lean.x * AIR_YAW - c.steer * 1.5) * i.y,
                self.lean.x * AIR_ROLL * i.z,
            ) * t.air;
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
        // So is landing on its side: the lean is placed only from upright-ish.
        if grounded > 0 && self.lean_now().abs() > 1.2 {
            crash = true;
        }
        if crash && self.crashed.is_none() {
            self.crashed = Some(CRASH_SECONDS);
        }

        // Integrate the forces (semi-implicit Euler).
        self.velocity += force / MASS * dt;
        let inertia = r * Mat3::from_diagonal(INERTIA) * r.transpose();
        let inverse = r * Mat3::from_diagonal(INERTIA.recip()) * r.transpose();
        let gyro = self.spin.cross(inertia * self.spin);
        self.spin += inverse * (torque - gyro) * dt;
        if grounded > 0 {
            // On the ground the lean is placed, not spun up.
            self.spin -= fwd * self.spin.dot(fwd);
        }

        // The tyres, as velocity constraints at the contact patches.
        self.solve_tyres(&tyres, inverse, fwd, t, dt);
        self.spin = self.spin.clamp_length_max(20.0);

        self.position += self.velocity * dt;
        self.rotation = (Quat::from_scaled_axis(self.spin * dt) * self.rotation).normalize();

        if grounded > 0 {
            // The lean: turn the bike about its tyres to the angle it is
            // heading for, measured from the ground (slowly filtered, so a
            // berm leans it with the berm).
            let ground = (ground_normal / grounded as f32).normalize_or(Vec3::Y);
            self.up_ref = (self.up_ref + (ground - self.up_ref) * lag(0.25)).normalize_or(Vec3::Y);
            let fwd = self.forward();
            let up = self.up();
            let reference = (self.up_ref - fwd * self.up_ref.dot(fwd)).normalize_or(Vec3::Y);
            let lean_now = fwd.dot(reference.cross(up)).atan2(reference.dot(up));
            let turn = Quat::from_axis_angle(fwd, self.lean_angle - lean_now);
            let pivot = ground_point / grounded as f32;
            let arm = self.position - pivot;
            self.position += turn * arm - arm;
            self.rotation = (turn * self.rotation).normalize();
        } else {
            // In the air the lean it lands with is where it starts from.
            self.lean_angle = self.lean_now();
            self.up_ref = Vec3::Y;
        }
    }

    /// The lean right now against straight up, radians, positive right.
    fn lean_now(&self) -> f32 {
        let fwd = self.forward();
        let reference = (Vec3::Y - fwd * fwd.y).normalize_or(Vec3::Y);
        fwd.dot(reference.cross(self.up())).atan2(reference.dot(self.up()))
    }

    /// Each tyre stops its patch sliding, sideways and along, as far as its
    /// grip allows: rounds of impulses, rear and front in turn. Driving and
    /// braking come first, up to grip x load; the side grip is what the
    /// friction circle leaves. A tyre that uses all of that is sliding: it
    /// keeps `SLIDE_GRIP` of its side grip plus a side force that grows with
    /// the slide (`SLIDE_LONG` of its grip along, if the slide is along).
    fn solve_tyres(&mut self, tyres: &[Option<Tyre>; 2], inverse: Mat3, fwd: Vec3, t: Tuning, dt: f32) {
        // An impulse at a patch moves the bike and turns it, but never leans
        // it. The lean is placed, so the patch's sideways offset from the
        // centre of mass is not a lever either (it would swing the tail out
        // under power).
        let side = fwd.cross(Vec3::Y).normalize_or(Vec3::X);
        let lever = |point: Vec3| {
            let arm = point - self.position;
            arm - side * arm.dot(side)
        };
        let turn_per = |arm: Vec3, dir: Vec3| {
            let w = inverse * arm.cross(dir);
            w - fwd * w.dot(fwd)
        };
        let mut limits = [0.0f32; 2];
        let mut applied = [0.0f32; 2];
        let mut long = [0.0f32; 2];
        for (i, tyre) in tyres.iter().enumerate() {
            let Some(ty) = tyre else { continue };
            let arm = lever(ty.point);
            let grip = t.grip * ty.load;
            // Driving and braking: past the grip the wheel spins or locks.
            let f_long = if ty.f_long.abs() > grip {
                ty.f_long.signum() * SLIDE_LONG * grip
            } else {
                ty.f_long
            };
            long[i] = f_long;
            limits[i] = (grip * grip - f_long * f_long).max(0.0).sqrt() * dt;
            let j = f_long * dt;
            self.velocity += ty.long * (j / MASS);
            self.spin += turn_per(arm, ty.long) * j;
        }
        // The front may never turn the bike harder than the rear can hold
        // it: past that the front washes wide (the stable way to lose it)
        // rather than flinging the rear out.
        let balance = |limits: &mut [f32; 2]| {
            if let [Some(rear), Some(front)] = tyres {
                let a = (front.point - self.position).dot(fwd).abs().max(0.1);
                let b = (rear.point - self.position).dot(fwd).abs().max(0.1);
                limits[1] = limits[1].min(limits[0] * b / a * 0.95);
            }
        };
        balance(&mut limits);
        for round in 0..8 {
            if round == 4 {
                // The tyres that used all their grip are sliding.
                for (i, tyre) in tyres.iter().enumerate() {
                    let Some(ty) = tyre else { continue };
                    if applied[i].abs() < limits[i] * 0.999 {
                        continue;
                    }
                    let arm = lever(ty.point);
                    let v_lat = (self.velocity + self.spin.cross(arm)).dot(ty.lat);
                    let grip = t.grip * ty.load;
                    let share = applied[i].abs() / applied[i].hypot(long[i] * dt).max(1e-6);
                    let slide = lerp_table(&SLIDE_CURVE, v_lat.abs());
                    limits[i] = (SLIDE_GRIP * grip * share + t.slide_force * slide) * dt;
                }
                balance(&mut limits);
            }
            for (i, tyre) in tyres.iter().enumerate() {
                let Some(ty) = tyre else { continue };
                let arm = lever(ty.point);
                let v_lat = (self.velocity + self.spin.cross(arm)).dot(ty.lat);
                let w = turn_per(arm, ty.lat);
                let k = 1.0 / MASS + ty.lat.dot(w.cross(arm));
                let total = (applied[i] - v_lat / k).clamp(-limits[i], limits[i]);
                let j = total - applied[i];
                applied[i] = total;
                self.velocity += ty.lat * (j / MASS);
                self.spin += w * j;
            }
        }
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
    -AXLE_Y + rear_leg().radius
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat floor, 1200 m square, at y = 0.
    fn floor() -> World {
        let s = 600.0;
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

    /// Full lock and full rider lean at speed, on and off the gas: the rear
    /// stays planted (the video bug: it slid out from about 15 km/h).
    #[test]
    fn hard_turns_keep_the_rear() {
        for (kmh, throttle) in [(20.0f32, 0.0f32), (20.0, 1.0), (30.0, 0.0), (40.0, 0.0), (40.0, 1.0), (60.0, 0.0)] {
            let w = floor();
            let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
            ride(&w, &mut bike, Controls::default(), 0.5);
            bike.velocity = bike.forward() * kmh / 3.6;
            let before = bike.heading();
            let mut worst = 0.0f32;
            for _ in 0..120 {
                let c = Controls {
                    steer: 1.0,
                    lean: Vec2::new(1.0, 0.0),
                    throttle,
                    ..default()
                };
                bike.step(&w, c, 1.0 / 60.0);
                let rear = bike.to_world(bike.axle(0));
                let v = bike.velocity + bike.spin.cross(rear - bike.position);
                let side = bike.rotation * Vec3::X;
                let side = Vec3::new(side.x, 0.0, side.z).normalize();
                worst = worst.max(v.dot(side).abs());
                assert!(bike.crashed.is_none(), "{kmh} km/h: crashed");
            }
            assert!(worst < 0.5, "{kmh} km/h, throttle {throttle}: rear slid {worst} m/s");
            assert!(bike.heading() - before < -0.5, "{kmh} km/h: barely turned");
            assert!(bike.up().y < 0.97, "{kmh} km/h: did not lean");
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
    fn shifts_up_through_the_box() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -150.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 1.0);
        assert_eq!(bike.gear, 0);
        let throttle = Controls {
            throttle: 1.0,
            lean: Vec2::new(0.0, 0.6),
            ..default()
        };
        let mut gear = 0;
        for _ in 0..(14 * 60) {
            bike.step(&w, throttle, 1.0 / 60.0);
            // Up one gear or more at a time, never back down on the gas.
            assert!(bike.gear >= gear, "dropped from {gear} to {}", bike.gear);
            gear = bike.gear;
            assert!(bike.rpm <= LIMITER_RPM + 1.0);
        }
        assert_eq!(bike.gear, 4, "top gear");
        // Reflex's 450 runs out of pull about 32 m/s in fifth.
        let top = bike.speed();
        assert!(top > 27.0 && top < 36.0, "top speed {top}");
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);

        // Off the gas it drops gears and the engine slows it.
        ride(&w, &mut bike, Controls::default(), 6.0);
        assert!(bike.gear < 4, "still in {}", bike.gear);
        assert!(bike.speed() < top - 8.0, "coasting at {}", bike.speed());
    }

    #[test]
    fn first_gear_launch_pulls_hard() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -150.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 1.0);
        let gas = Controls {
            throttle: 1.0,
            lean: Vec2::new(0.0, 0.6),
            ..default()
        };
        ride(&w, &mut bike, gas, 1.0);
        let v = bike.speed();
        assert!(v > 5.0 && v < 13.0, "1 s: {v} m/s");
        assert!(bike.crashed.is_none());
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
        assert!(v < -2.0 && v > -4.0, "reversing at {v} m/s");
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

    /// The nose's pitch (forward.y) after `seconds` in the air with the rear
    /// wheel spinning as if it left the ground at 15 m/s.
    fn air_nose_after(c: Controls, seconds: f32) -> f32 {
        let w = floor();
        let mut bike = Bike::new(Vec3::new(0.0, 60.0, 0.0), 0.0);
        bike.wheel_spin = 15.0 / rear_leg().radius;
        ride(&w, &mut bike, c, seconds);
        bike.forward().y
    }

    #[test]
    fn gas_in_the_air_lifts_the_front_a_little() {
        let coast = air_nose_after(default(), 1.0);
        let gas = air_nose_after(
            Controls {
                throttle: 1.0,
                ..default()
            },
            1.0,
        );
        assert!(gas > coast + 0.05, "gas {gas} coast {coast}");
        // A little, not a backflip.
        assert!(gas < 0.4, "gas {gas}");
    }

    #[test]
    fn rear_brake_in_the_air_drops_the_front_a_little() {
        let coast = air_nose_after(default(), 0.5);
        let brake = air_nose_after(
            Controls {
                brake: 1.0,
                ..default()
            },
            0.5,
        );
        assert!(brake < coast - 0.05, "brake {brake} coast {coast}");
        assert!(brake > -0.4, "brake {brake}");
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

