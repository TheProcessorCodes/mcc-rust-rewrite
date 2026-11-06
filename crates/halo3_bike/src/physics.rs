//! The dirt bike as a rigid body on two suspension wheels, ridden the way MX
//! vs ATV Unleashed rides it (`unleashed.rs` holds the stock Pro 250's
//! numbers, `docs/bike-unleashed.md` the model in plain words).
//!
//! Plain Rust over glam, no bevy systems: `mod.rs` feeds it the controller
//! and draws the result. Everything is in the renderer frame (metres, Y up).
//!
//! The bike's own frame: +X left, +Y up, +Z forward (the skater rig's
//! frame too, so the rider pose needs no axis swap). Its origin sits
//! `FRAME_LIFT` above the static centre of gravity of bike and rider (where
//! the frame, pegs and probes below were laid out); the rider's weight
//! shift moves the real centre fore and aft.
//!
//! What makes it Unleashed rather than a free bicycle:
//! - on the ground the bike cannot roll by itself. The bars and the lean are
//!   set from the stick (lean grows with speed up to 45° at 35 mph), the
//!   tyres only push and yaw it;
//! - each wheel is a spring along the ground's normal; the tyres' grip acts
//!   near the centre of gravity, so it steers the bike without tipping it;
//!   sideways grip follows slip speed on a curve that peaks and fades;
//! - a five-speed engine with a slipping clutch and automatic shifts drives
//!   the rear wheel; too much torque spins it, too much brake locks it;
//! - pulling back on the gas holds a wheelie at an angle set by the stick,
//!   full forward on the brakes holds a stoppie;
//! - in the air the stick turns the bike against heavy damping, and let go
//!   it levels and lines up with where it is going.

use std::sync::{Arc, RwLock};

use bevy::math::{Mat3, Mat4, Quat, Vec2, Vec3, Vec4};
use blam_geometry::column_grid::ColumnGrid;

use super::exact::{self, Exact};
use super::reflex::Tuning;
use super::unleashed::{RPM, Setup, Tyre, ft, lerp_clamp};

pub const GRAVITY: f32 = 9.81;

/// How far the bike frame's origin sits above the centre of gravity.
pub const FRAME_LIFT: f32 = 0.135;

/// An axle from `unleashed.rs` (from the centre of gravity) in the bike frame.
fn frame(axle: Vec3) -> Vec3 {
    axle - Vec3::Y * FRAME_LIFT
}

static SETUP: RwLock<Option<Arc<Setup>>> = RwLock::new(None);

/// The bike every rider rides: Reflex v1 until the tuning menu says otherwise.
pub fn setup() -> Arc<Setup> {
    if let Some(s) = SETUP.read().unwrap_or_else(|e| e.into_inner()).as_ref() {
        return s.clone();
    }
    let s = Arc::new(Tuning::default().setup());
    *SETUP.write().unwrap_or_else(|e| e.into_inner()) = Some(s.clone());
    s
}

/// Rides on `tuning` from the next step on.
pub fn set_tuning(tuning: &Tuning) {
    exact::set(
        tuning.model >= 1.5,
        exact::Tune {
            power: tuning.reflex_power,
            gearing: tuning.reflex_gearing,
            traction: tuning.reflex_traction,
        },
    );
    *SETUP.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(tuning.setup()));
}

/// Seconds lying in the dirt before the bike comes back.
pub const CRASH_SECONDS: f32 = 2.0;

/// One suspension leg and its wheel, for drawing and the rider.
#[derive(Clone, Copy, Debug)]
pub struct Leg {
    /// The axle at full extension, bike frame.
    pub extended: Vec3,
    /// The direction the axle is drawn moving as the suspension compresses.
    pub axis: Vec3,
    pub travel: f32,
    pub radius: f32,
    /// Tread width, metres (drawn only).
    pub width: f32,
}

/// The rake of the forks: the steering axis leans back this far from
/// vertical (29.2°, the 450F's steering head).
pub const RAKE: f32 = 0.51;

/// Where the steering axis runs through, bike frame: the 450F's top
/// triple clamp (`OBJ_UPR_Forks`).
pub const STEER_PIVOT: Vec3 = Vec3::new(0.0, 0.194, 0.338);

pub fn steering_axis() -> Vec3 {
    Vec3::new(0.0, RAKE.cos(), -RAKE.sin())
}

fn leg(axle: Vec3, t: &Tyre, width: f32) -> Leg {
    Leg {
        extended: axle,
        axis: Vec3::Y,
        travel: t.strut,
        radius: t.radius,
        width,
    }
}

pub fn rear_leg() -> Leg {
    let s = setup();
    leg(frame(s.rear_axle), &s.rear, 0.13)
}

pub fn front_leg() -> Leg {
    let s = setup();
    leg(frame(s.front_axle), &s.front, 0.11)
}

/// How far above the extended axle a wheel's ray starts, so a wheel pushed
/// past full travel still finds the ground.
const RAY_HEADROOM: f32 = 0.6;

/// Where the rider stands, bike frame (render metres): footpegs (left;
/// mirror X for the right), half the bar width, and the hips in the attack
/// position. Pegs and grips are the 450F's (`NDE_*_Foot`, `NDE_*_Hand`);
/// the hips keep their old place over the pegs.
pub const PEG: Vec3 = Vec3::new(0.183, -0.285, -0.211);
pub const BAR_HALF_WIDTH: f32 = 0.29;
pub const HIPS: Vec3 = Vec3::new(0.0, 0.515, -0.39);

/// Where the forks are drawn from: the front axle at full extension, so
/// the bars and the triple clamp stay put whatever the spring.
pub fn fork_base() -> Vec3 {
    front_leg().extended
}

/// The middle of the bars with them straight: between the 450F's grips.
pub fn bar_centre() -> Vec3 {
    Vec3::new(0.0, 0.46, 0.172)
}

/// Probe spheres on the frame and the rider: bike frame centre, radius,
/// and whether touching anything with it is a crash.
/// Placed on the 450F: 0.30 m of ground clearance under the skid plate.
pub const PROBES: [(Vec3, f32, bool); 6] = [
    (Vec3::new(0.0, -0.30, 0.0), 0.14, false),  // skid plate
    (Vec3::new(0.0, 0.17, -0.98), 0.12, false), // rear fender
    (Vec3::new(0.0, 0.10, 0.85), 0.12, false),  // front fender
    (Vec3::new(0.0, 0.25, -0.30), 0.16, false), // seat
    (Vec3::new(0.0, 0.47, 0.20), 0.16, true),   // bars and hands
    (Vec3::new(0.0, 1.18, 0.20), 0.22, true),   // the rider's head, at RIDER_SIZE
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
    /// Held from a standstill with both wheels down, backs the bike up instead.
    pub brake: f32,
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
    triangles: Vec<Triangle>,
    grid: ColumnGrid,
    /// Per triangle, the scene object it belongs to (`u32::MAX`: the
    /// structure BSP's own); empty when none does. Objects whose collision
    /// a script took away (`halo3_core::objects`) are skipped.
    owners: Vec<u32>,
    /// Objects a script has moved (`halo3_core::objects::object_poses`),
    /// renderer frame: their triangles stay in the grid where the scenario
    /// put them and are moved as they are looked at.
    moved: RwLock<Moved>,
}

/// The moved objects as last read: the pose generation, then per object
/// its motion and the inverse.
#[derive(Default)]
struct Moved {
    generation: u64,
    objects: Vec<(u32, Mat4, Mat4)>,
}

impl Moved {
    fn motion(&self, owner: u32) -> Option<&Mat4> {
        self.objects.iter().find(|(o, _, _)| *o == owner).map(|(_, m, _)| m)
    }
}

/// Renderer frame from Halo frame, as a matrix (`halo3_core::coords::to_render`).
fn render_from_halo() -> Mat4 {
    let s = blam_geometry::WORLD_UNIT_METRES;
    Mat4::from_cols(
        Vec4::new(s, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -s, 0.0),
        Vec4::new(0.0, s, 0.0, 0.0),
        Vec4::W,
    )
}

impl World {
    /// Triangles in the renderer frame, wound counter-clockwise seen from the
    /// side you stand on, with their two-sided flag.
    pub fn new(triangles: impl IntoIterator<Item = ([Vec3; 3], bool)>) -> World {
        let mut world = World::with_owners(triangles.into_iter().map(|(p, two)| (p, two, u32::MAX)));
        world.owners.clear();
        world
    }

    /// The same, with the scene object each triangle belongs to
    /// (`u32::MAX` for none).
    pub fn with_owners(triangles: impl IntoIterator<Item = ([Vec3; 3], bool, u32)>) -> World {
        let mut out = Vec::new();
        let mut owners = Vec::new();
        let mut grid = ColumnGrid::new(4.0);
        for ([a, b, c], two_sided, owner) in triangles {
            let normal = (b - a).cross(c - a);
            if !normal.is_finite() || normal.length_squared() < 1e-10 {
                continue;
            }
            let i = out.len() as u32;
            owners.push(owner);
            out.push(Triangle {
                a,
                b,
                c,
                normal: normal.normalize(),
                two_sided,
            });
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            grid.insert(i, [lo.x, lo.z], [hi.x, hi.z]);
        }
        if owners.iter().all(|o| *o == u32::MAX) {
            owners.clear();
        }
        World {
            triangles: out,
            grid,
            owners,
            moved: RwLock::new(Moved::default()),
        }
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.triangles.len()
    }

    /// Triangles whose columns come within `radius` of `centre` (X, Z), once each.
    pub fn gather(&self, centre: Vec3, radius: f32, out: &mut Vec<u32>) {
        self.gather_along(centre, radius, (centre, centre), out);
    }

    /// The same, for what lies along the segment `along` (inside the
    /// columns round `centre`): the segment picks the cells of moved
    /// objects, whose columns are tilted against the world's.
    fn gather_along(&self, centre: Vec3, radius: f32, along: (Vec3, Vec3), out: &mut Vec<u32>) {
        out.clear();
        self.grid.query(
            [centre.x - radius, centre.z - radius],
            [centre.x + radius, centre.z + radius],
            |i| out.push(i),
        );
        if !self.owners.is_empty() {
            let reach = radius - Vec2::new(along.1.x - along.0.x, along.1.z - along.0.z).length() * 0.5;
            self.gather_moved(along, reach.max(0.0), out);
        }
        out.sort_unstable();
        out.dedup();
        if !self.owners.is_empty() {
            halo3_core::objects::retain_present(out, |i| self.owners[i as usize]);
        }
    }

    /// Re-reads the moved objects when they changed.
    fn refresh_moved(&self) {
        let (generation, _) = halo3_core::objects::pose_generation();
        if self.moved.read().unwrap_or_else(|e| e.into_inner()).generation == generation {
            return;
        }
        let c = render_from_halo();
        let c_inv = c.inverse();
        let objects = halo3_core::objects::object_poses()
            .into_iter()
            .map(|(o, m)| {
                let r = c * m * c_inv;
                (o, r, r.inverse())
            })
            .collect();
        *self.moved.write().unwrap_or_else(|e| e.into_inner()) = Moved { generation, objects };
    }

    /// Adds the triangles of moved objects that are within `radius` of the
    /// segment `along` where the objects are now: looked up in the grid
    /// around the segment taken back to where each object was placed (the
    /// motions are rigid).
    fn gather_moved(&self, along: (Vec3, Vec3), radius: f32, out: &mut Vec<u32>) {
        self.refresh_moved();
        let moved = self.moved.read().unwrap_or_else(|e| e.into_inner());
        if moved.objects.is_empty() {
            return;
        }
        for (object, _, inverse) in &moved.objects {
            let (a, b) = (inverse.transform_point3(along.0), inverse.transform_point3(along.1));
            let p = (a + b) * 0.5;
            let radius = radius + Vec2::new(b.x - a.x, b.z - a.z).length() * 0.5;
            self.grid.query([p.x - radius, p.z - radius], [p.x + radius, p.z + radius], |i| {
                if self.owners[i as usize] == *object {
                    out.push(i);
                }
            });
        }
    }

    /// The object a script is moving that the ray first hits, and where
    /// that object is now (its motion from its placement, renderer frame);
    /// `None` when the ray hits nothing or something that does not move.
    /// A rider on it is carried along by the change between two calls.
    pub fn moving_ground(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<(u32, Mat4)> {
        if self.owners.is_empty() {
            return None;
        }
        let mut candidates = Vec::new();
        let end = origin + dir * max;
        let mid = (origin + end) * 0.5;
        let reach = Vec2::new(end.x - origin.x, end.z - origin.z).length() * 0.5 + 0.1;
        self.gather_along(mid, reach, (origin, end), &mut candidates);
        let moved = self.moved.read().unwrap_or_else(|e| e.into_inner());
        if moved.objects.is_empty() {
            return None;
        }
        let hit = self.raycast(&candidates, origin, dir, max)?;
        // The triangle that was hit: the nearest along the ray at that distance.
        let owner = candidates.iter().copied().find(|&i| {
            let t = self.triangle(&moved, i);
            let n = t.normal;
            (hit.point - t.a).dot(n).abs() < 1e-3 && closest_point(hit.point, t.a, t.b, t.c).distance_squared(hit.point) < 1e-6
        })?;
        let object = self.owners[owner as usize];
        moved.motion(object).map(|m| (object, *m))
    }

    /// Triangle `i` where it is now (moved with its object).
    fn triangle(&self, moved: &Moved, i: u32) -> Triangle {
        let t = self.triangles[i as usize];
        if moved.objects.is_empty() || self.owners.is_empty() {
            return t;
        }
        match moved.motion(self.owners[i as usize]) {
            Some(m) => Triangle {
                a: m.transform_point3(t.a),
                b: m.transform_point3(t.b),
                c: m.transform_point3(t.c),
                normal: m.transform_vector3(t.normal).normalize_or(t.normal),
                two_sided: t.two_sided,
            },
            None => t,
        }
    }

    /// The nearest front face along a ray (`dir` unit length) among `candidates`.
    pub fn raycast(&self, candidates: &[u32], origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        let moved = self.moved.read().unwrap_or_else(|e| e.into_inner());
        for &i in candidates {
            let t = &self.triangle(&moved, i);
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

    /// The triangles touching the box `centre ± half` where they are now,
    /// wound counter-clockwise seen from the side you stand on; a two-sided
    /// one is wound to face `centre` (given both ways round, a wheel on it
    /// would average its two normals to nothing). `ids` is scratch.
    pub fn triangles_near(&self, centre: Vec3, half: f32, ids: &mut Vec<u32>, out: &mut Vec<[Vec3; 3]>) {
        out.clear();
        self.gather(centre, half, ids);
        let moved = self.moved.read().unwrap_or_else(|e| e.into_inner());
        let (lo, hi) = (centre - Vec3::splat(half), centre + Vec3::splat(half));
        for &i in ids.iter() {
            let t = self.triangle(&moved, i);
            if t.a.min(t.b).min(t.c).cmpgt(hi).any() || t.a.max(t.b).max(t.c).cmplt(lo).any() {
                continue;
            }
            if t.two_sided && (centre - t.a).dot(t.normal) < 0.0 {
                out.push([t.a, t.c, t.b]);
            } else {
                out.push([t.a, t.b, t.c]);
            }
        }
    }

    /// The same, gathering its own candidates (for occasional long rays).
    pub fn raycast_far(&self, origin: Vec3, dir: Vec3, max: f32) -> Option<Hit> {
        let mut candidates = Vec::new();
        let end = origin + dir * max;
        let mid = (origin + end) * 0.5;
        let reach = Vec2::new(end.x - origin.x, end.z - origin.z).length() * 0.5 + 0.1;
        self.gather_along(mid, reach, (origin, end), &mut candidates);
        self.raycast(&candidates, origin, dir, max)
    }

    /// Where a sphere overlaps the world: per touching face, the push-out
    /// normal and depth. Faces seen from behind are ignored unless two-sided;
    /// near-parallel contacts are merged, keeping the deepest.
    pub fn sphere(&self, candidates: &[u32], centre: Vec3, radius: f32, out: &mut Vec<(Vec3, f32)>) {
        out.clear();
        let moved = self.moved.read().unwrap_or_else(|e| e.into_inner());
        for &i in candidates {
            let t = &self.triangle(&moved, i);
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
    /// How far into its travel the suspension is, as drawn: it extends no
    /// faster than the strut lets it.
    pub compression: f32,
    pub contact: Option<Hit>,
    /// Spinning under power or locked under the brake.
    pub sliding: bool,
}

/// The engine and gearbox.
#[derive(Clone, Copy, Debug)]
pub struct Engine {
    pub rpm: f32,
    /// 0-based.
    pub gear: usize,
    /// The clutch grips without slipping.
    pub locked: bool,
    /// The rear wheel's rpm through the gears last step, to see it cross the engine's.
    wheel_rpm: f32,
    /// What a spinning rear tyre really holds back, at the engine, N·m.
    tyre_load: f32,
}

/// The controls after the bike's own rate limits.
#[derive(Clone, Copy, Debug, Default)]
struct Held {
    /// +1 right.
    steer: f32,
    /// The rider's weight: +1 all the way back.
    back: f32,
    gas: f32,
    brake: f32,
}

/// The bike's state. `steer` is the bar angle in radians, positive to the left.
#[derive(Clone, Debug)]
pub struct Bike {
    /// The bike frame's origin, world.
    pub position: Vec3,
    pub rotation: Quat,
    /// World frame, m/s and rad/s.
    pub velocity: Vec3,
    pub spin: Vec3,
    pub steer: f32,
    /// The rider's weight shift, smoothed from the stick (X right, Y forward).
    pub lean: Vec2,
    /// Rear, front.
    pub wheels: [WheelState; 2],
    /// The rear wheel's spin, rad/s, positive rolling forward.
    pub wheel_spin: f32,
    pub engine: Engine,
    /// Seconds left lying in the dirt.
    pub crashed: Option<f32>,
    held: Held,
    /// Where the real centre of gravity is, bike frame from `position`.
    cog: Vec3,
    /// The wheelie and stoppie holds' integral, N·m, and whether a stoppie is on.
    pitch_integral: f32,
    stoppie: bool,
    /// Backing up: the brake was pulled at a standstill and is still held.
    reversing: bool,
    /// Where to come back after a crash: position and heading, recorded
    /// while both wheels are down, the oldest about two seconds back.
    safe: Vec<(Vec3, f32)>,
    safe_clock: f32,
    candidates: Vec<u32>,
    contacts: Vec<(Vec3, f32)>,
    /// The moving object under the bike and where it was last frame
    /// (`World::moving_ground`), to carry the bike along with it.
    platform: Option<(u32, Mat4)>,
    /// Riding on Reflex's own simulation (`exact.rs`) instead of ours, and
    /// the choice's generation when that was decided.
    pub(crate) exact: Option<Box<Exact>>,
    exact_generation: u64,
}

/// Slower than this along the frame, m/s, is a standstill for backing up.
const STOPPED: f32 = 0.15;

/// A rotation that faces the bike along `heading` (radians about +Y, 0 is +Z).
pub fn heading_rotation(heading: f32) -> Quat {
    Quat::from_rotation_y(heading)
}

/// Moves `v` toward `target` by at most `rate * dt`.
fn toward(v: f32, target: f32, rate: f32, dt: f32) -> f32 {
    v + (target - v).clamp(-rate * dt, rate * dt)
}

impl Bike {
    /// A bike standing on its wheels at rest with its centre of gravity at `position`.
    pub fn new(position: Vec3, heading: f32) -> Bike {
        Bike {
            position,
            rotation: heading_rotation(heading),
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            steer: 0.0,
            lean: Vec2::ZERO,
            wheels: [WheelState::default(); 2],
            wheel_spin: 0.0,
            engine: Engine {
                rpm: setup().min_rpm,
                gear: 0,
                locked: false,
                wheel_rpm: 0.0,
                tyre_load: 0.0,
            },
            crashed: None,
            held: Held::default(),
            cog: Vec3::NEG_Y * FRAME_LIFT,
            pitch_integral: 0.0,
            stoppie: false,
            reversing: false,
            safe: Vec::new(),
            safe_clock: 0.0,
            candidates: Vec::new(),
            platform: None,
            contacts: Vec::new(),
            exact: None,
            exact_generation: exact::generation(),
        }
        .with_exact(exact::on())
    }

    /// The same bike on Reflex's simulation (when it can be had) or on ours.
    pub(crate) fn with_exact(mut self, on: bool) -> Bike {
        self.exact = if on { Exact::new(&self).map(Box::new) } else { None };
        self
    }

    /// Reflex's simulation, when the bike rides on it.
    pub fn exact(&self) -> Option<&Exact> {
        self.exact.as_deref()
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

    /// The gas the engine gets this step, after the pedal's rate limit.
    pub fn gas(&self) -> f32 {
        self.held.gas
    }

    /// Sets what `gas` reports (Reflex's simulation keeps its own throttle).
    pub(crate) fn set_gas(&mut self, gas: f32) {
        self.held.gas = gas;
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
        l.extended + l.axis * self.wheels[leg].compression.min(l.travel)
    }

    /// The bars' rotation about the steering axis, bike frame.
    pub fn bar_rotation(&self) -> Quat {
        Quat::from_axis_angle(steering_axis(), self.steer)
    }

    /// The grips (left, right) in the bike frame, turned with the bars
    /// about the steering axis.
    pub fn grips(&self) -> [Vec3; 2] {
        let turn = |p: Vec3| STEER_PIVOT + self.bar_rotation() * (p - STEER_PIVOT);
        let side = Vec3::X * BAR_HALF_WIDTH;
        [turn(bar_centre() + side), turn(bar_centre() - side)]
    }

    /// Advances by `dt` seconds in fixed sub-steps.
    /// Moves the bike with what it stands on when a script moves that (a
    /// Warthog run island lurching up under it): the springs alone would
    /// let a surface rising metres a second pass through the wheels.
    fn ride_platform(&mut self, world: &World) {
        let up = self.up();
        let ground = world.moving_ground(self.position + up * 0.5, -up, 2.5);
        if let (Some((object, now)), Some((was_on, then))) = (ground, self.platform)
            && object == was_on
        {
            let delta = now * then.inverse();
            self.position = delta.transform_point3(self.position);
            let (_, turn, _) = delta.to_scale_rotation_translation();
            self.rotation = (turn * self.rotation).normalize();
        }
        self.platform = ground;
    }

    pub fn step(&mut self, world: &World, controls: Controls, dt: f32) {
        const SUB: f32 = 1.0 / 240.0;
        // A frame with no time in it (the first frame can be one) would
        // divide by zero in the sub-steps. Seen on Narrows (`chill --bike
        // --view spawn33`, also spawns 10, 31, 32, 34).
        if dt <= 0.0 || !dt.is_finite() {
            return;
        }
        self.ride_platform(world);
        // The tuning menu switched models: carry on from here on the other.
        if exact::generation() != self.exact_generation {
            self.exact_generation = exact::generation();
            self.exact = if exact::on() { Exact::new(self).map(Box::new) } else { None };
            self.crashed = None;
        }
        if let Some(mut exact) = self.exact.take() {
            exact.step(self, world, controls, dt);
            self.exact = Some(exact);
            if !self.position.is_finite() || !self.rotation.is_finite() {
                self.respawn(world);
            }
            self.remember_safe(dt);
            return;
        }
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
        self.remember_safe(dt);
    }

    /// Remembers where it was riding well.
    fn remember_safe(&mut self, dt: f32) {
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

    /// The engine for one step: shifts, turns the clutch and returns the
    /// torque it puts on the rear wheel, N·m.
    fn drive(&mut self, s: &Setup, speed: f32, dt: f32) -> f32 {
        let gas = self.held.gas;
        // Automatic gears, from the road speed: light gas shifts up early
        // and lugs a taller gear, full gas holds each gear to the top. The
        // bands keep their hysteresis across the gear steps (1.22 at most).
        let e = &mut self.engine;
        let road_rpm = speed / s.rear.radius * s.ratio(e.gear) * RPM;
        let up = lerp_clamp(gas, 0.0, 6500.0, 1.0, s.upshift_rpm);
        let down = lerp_clamp(gas, 0.0, 5300.0, 1.0, s.downshift_rpm);
        if road_rpm > up && e.gear + 1 < s.gears.len() {
            e.gear += 1;
        } else if road_rpm < down && e.gear > 0 {
            e.gear -= 1;
        }
        let ratio = s.ratio(e.gear);
        // The clutch is in while it creeps along with no gas.
        let clutch = if speed < 5.0 * super::unleashed::MPH && gas == 0.0 { 1.0 } else { 0.0 };
        let grip = 1.0 - clutch;

        let throttle = (gas + s.idle_throttle).min(1.0);
        let braking = s.engine_braking * e.rpm * e.rpm;
        let engine = throttle * (s.curve_torque(e.rpm) + braking) - braking;
        let wheel_rpm = self.wheel_spin * ratio * RPM;
        if e.locked && (clutch >= 1.0 || engine.abs() > grip * s.clutch_static) {
            e.locked = false;
        }
        let mut through = engine;
        if !e.locked {
            // Slipping, it passes what it can hold, but no more than the
            // engine makes: idling, it cannot kick the bike off the line.
            through = grip
                * if e.rpm < wheel_rpm {
                    -s.clutch_kinetic
                } else {
                    s.clutch_kinetic.min(engine.max(0.0))
                };
            // The wheel's rpm crossing the engine's: they lock together.
            let crossed = (e.wheel_rpm < e.rpm && e.rpm < wheel_rpm) || (e.wheel_rpm > e.rpm && e.rpm > wheel_rpm);
            if grip > 0.0 && crossed {
                e.locked = true;
                through = engine;
            }
        }
        e.wheel_rpm = wheel_rpm;
        if e.locked {
            e.rpm = wheel_rpm;
        } else {
            // A spinning tyre loads the engine with what it really grips.
            let load = if self.wheels[0].sliding && self.wheels[0].contact.is_some() {
                grip * e.tyre_load
            } else {
                through
            };
            e.rpm += (engine - load) / s.engine_inertia * dt * RPM;
        }
        e.rpm = e.rpm.clamp(s.min_rpm, s.max_rpm());
        through * ratio
    }

    fn point_velocity(&self, com: Vec3, p: Vec3) -> Vec3 {
        self.velocity + self.spin.cross(p - com)
    }

    fn sub_step(&mut self, world: &World, candidates: &[u32], c: Controls, dt: f32) {
        let s = setup();
        let r = Mat3::from_quat(self.rotation);
        let (side, up, fwd) = (r.x_axis, r.y_axis, r.z_axis);

        // The brake pulled at a standstill backs up instead, until it is let go.
        let stopped = self.velocity.dot(fwd).abs() < STOPPED && self.wheels.iter().all(|w| w.contact.is_some());
        self.reversing = c.brake > 0.5 && (self.reversing || stopped);
        let brake = if self.reversing { 0.0 } else { c.brake };

        // The controls, through the bike's rate limits.
        let mut held = self.held;
        held.steer = toward(held.steer, c.steer.clamp(-1.0, 1.0), s.steer_rate, dt);
        held.back = toward(held.back, (-c.lean.y).clamp(-1.0, 1.0), s.lean_rate, dt);
        let gas = if self.reversing { 0.0 } else { c.throttle.clamp(0.0, 1.0) };
        held.gas = toward(held.gas, gas, s.pedal_rate, dt);
        held.brake = toward(held.brake, brake.clamp(0.0, 1.0), s.pedal_rate, dt);
        self.held = held;
        self.lean = Vec2::new(toward(self.lean.x, c.lean.x.clamp(-1.0, 1.0), s.lean_rate, dt), -held.back);

        // The rider's weight moves the centre of gravity fore and aft.
        let travel = if held.back >= 0.0 { s.cofg_travel_back } else { s.cofg_travel_forward };
        self.cog = Vec3::new(0.0, -FRAME_LIFT, -held.back * travel);
        let mut com = self.position + r * self.cog;
        let speed = self.velocity.dot(fwd);

        // The bars: set from the stick (and the rider's sideways lean),
        // less lock the faster it goes, lagging if the model says so.
        let stick = (held.steer + s.rider_steer * self.lean.x).clamp(-1.0, 1.0);
        let bars = -stick.signum() * stick.abs().powf(s.steer_power) * s.steer_lock(speed);
        self.steer = if s.steer_lag > 0.0 {
            self.steer + (bars - self.steer) * (dt / s.steer_lag).min(1.0)
        } else {
            bars
        };

        let mut force = Vec3::NEG_Y * GRAVITY * s.gravity_scale * s.mass;
        let mut torque = Vec3::ZERO;
        let drive = self.drive(&s, speed, dt);

        // Wheels: spring along the ground's normal, grip near the middle.
        let mut normals = Vec3::ZERO;
        let mut rear_normal = Vec3::Y;
        for i in 0..2 {
            let (tyre, axle) = if i == 0 { (&s.rear, frame(s.rear_axle)) } else { (&s.front, frame(s.front_axle)) };
            let a = self.position + r * axle;
            let reach = RAY_HEADROOM + tyre.radius + tyre.strut + 0.3;
            let mut hit = world.raycast(candidates, a + up * RAY_HEADROOM, -up, reach);
            // The tyre's lowest point towards that ground, in the wheel's plane.
            let lowest = |n: Vec3| (n - side * side.dot(n)).normalize_or(up);
            if let Some(h) = hit {
                let m = lowest(h.normal);
                if let Some(h2) = world.raycast(candidates, a + m * RAY_HEADROOM, -m, reach) {
                    hit = Some(h2);
                } else if h.normal.dot(m) < 0.2 {
                    hit = None;
                }
            }
            let contact = hit.and_then(|h| {
                let m = lowest(h.normal);
                let facing = m.dot(h.normal);
                if facing < 0.2 {
                    return None;
                }
                let bottom = a - m * tyre.radius;
                let depth = (h.point - bottom).dot(h.normal) / facing;
                (depth > 0.0).then_some((h, bottom + m * depth, depth))
            });
            let before = self.wheels[i].compression;
            let wheel_torque = if i == 0 { drive } else { 0.0 };
            let Some((h, p, mut depth)) = contact else {
                self.wheels[i] = WheelState {
                    compression: (before - tyre.max_decompression_rate * dt).max(0.0),
                    contact: None,
                    sliding: false,
                };
                if i == 0 {
                    // Free in the air: the engine and brake spin it.
                    let turning = self.wheel_spin.clamp(-1.0, 1.0);
                    let t = wheel_torque - (tyre.wheel_friction + held.brake * tyre.max_braking_torque) * turning;
                    let next = self.wheel_spin + t / tyre.wheel_inertia() * dt;
                    // Brakes and friction stop it, never turn it backwards.
                    self.wheel_spin = if next * self.wheel_spin < 0.0 && wheel_torque == 0.0 { 0.0 } else { next };
                }
                continue;
            };
            let n = h.normal;
            normals += n;
            if i == 0 {
                rear_normal = n;
            }
            // Past the strut's travel the frame bottoms out.
            if depth > tyre.strut {
                let excess = depth - tyre.strut;
                self.position += n * excess * 0.9;
                com += n * excess * 0.9;
                let into = self.point_velocity(com, p).dot(n);
                if into < 0.0 {
                    self.velocity -= n * into;
                }
                depth = tyre.strut;
            }
            let vp = self.point_velocity(com, p);
            let rate = (-vp.dot(n)).clamp(-tyre.max_compression_rate, tyre.max_compression_rate);
            let ratio = if rate < 0.0 { tyre.bounce } else { tyre.rebound };
            let load = (s.spring_rate(tyre) * depth + s.damping(tyre, ratio) * rate).max(0.0);
            let f_spring = n * load;
            force += f_spring;
            torque += (p - com).cross(f_spring);

            // The tyre: which way it rolls, flattened onto the ground.
            let heading = if i == 1 { r * (self.bar_rotation() * Vec3::Z) } else { fwd };
            let long = (heading - n * n.dot(heading)).normalize_or(fwd);
            let lat = n.cross(long);
            let v_long = vp.dot(long);
            let v_lat = vp.dot(lat);
            let rolling = (0.3 * ft(v_long)).clamp(-1.0, 1.0);
            let t = wheel_torque - (tyre.wheel_friction + held.brake * tyre.max_braking_torque) * rolling;
            let wanted = t / tyre.radius;
            let limit = tyre.max_drive * load;
            let (f_long, sliding) = if wanted > limit {
                (tyre.skid_drive * load, true)
            } else if wanted < -limit {
                (-tyre.skid_drive * load, true)
            } else {
                (wanted, false)
            };
            // Sideways: grip against slip speed, stiffer when nearly stopped.
            let mut slip = ft(v_lat);
            let rolling_ft = ft(v_long).abs();
            if rolling_ft < 10.0 {
                slip *= lerp_clamp(rolling_ft, 0.0, 5.0, 10.0, 1.0);
            }
            let curve = if held.brake > 0.0 {
                &tyre.braking
            } else if sliding {
                &tyre.slipping
            } else {
                &tyre.side
            };
            // Past the cap the tyre slides and keeps a share of it.
            let mut grip = curve.grip(slip * tyre.slip_scale);
            if grip.abs() > tyre.side_cap {
                grip = grip.signum() * tyre.side_cap * tyre.slide_keep;
            }
            let f_lat = grip * load;
            let f_tyre = long * f_long + lat * f_lat;
            force += f_tyre;
            torque += (r * tyre.force_position).cross(f_tyre);

            if i == 0 {
                self.engine.tyre_load = f_long * tyre.radius / s.ratio(self.engine.gear);
                self.wheel_spin = if sliding {
                    self.wheel_spin + (t - f_long * tyre.radius) / tyre.wheel_inertia() * dt
                } else {
                    v_long / tyre.radius
                };
                // The brake locks the wheel, never turns it against the way it rolls.
                if held.brake > 0.0 && self.wheel_spin * v_long < 0.0 {
                    self.wheel_spin = 0.0;
                }
            }
            self.wheels[i] = WheelState {
                compression: depth.max(before - tyre.max_decompression_rate * dt),
                contact: Some(h),
                sliding,
            };
        }
        // The rev limiter.
        let ratio = s.ratio(self.engine.gear);
        if self.engine.locked && self.wheel_spin * ratio * RPM > s.max_rpm() {
            self.wheel_spin = s.max_rpm() / ratio / RPM;
        }
        let grounded = self.grounded();

        // Wheelie and stoppie holds.
        let pitch = fwd.y.clamp(-1.0, 1.0).asin();
        let nose_up_rate = -self.spin.dot(side);
        let rear_down = self.wheels[0].contact.is_some();
        let front_down = self.wheels[1].contact.is_some();
        if front_down && held.brake > 0.0 && ft(speed) > 15.0 && held.back <= -0.99 {
            self.stoppie = true;
        }
        let wheelie = rear_down && held.gas > 0.0 && ft(speed) > 5.0 && held.brake == 0.0 && held.back >= 0.5;
        let lbf_ft = super::unleashed::LBF_FT;
        if self.stoppie {
            let fade = lerp_clamp(speed, 0.0, 0.0, s.stoppie_top_speed, 1.0);
            let target = fade * (2.0 * held.back + 1.0) * s.max_stoppie_pitch;
            let e = s.stoppie_pitch_gain * (pitch - target) + nose_up_rate;
            self.pitch_integral += e * dt * 500.0 * lbf_ft;
            torque += side * (self.pitch_integral + e * s.pitch_rate_gain * lbf_ft);
            // The bars still turn it about the front tyre.
            let yaw_right = -self.spin.dot(rear_normal);
            let t = (s.stoppie_yaw_rate_scale * held.steer - yaw_right) * 1000.0 * lbf_ft;
            torque -= rear_normal * t;
            self.stoppie = !rear_down && held.brake > 0.0 && held.back <= -0.1 && ft(speed) > 7.0;
        } else if wheelie {
            let target = (2.0 * held.back - 1.0) * s.max_pitch;
            let e = s.pitch_gain * (pitch - target) + nose_up_rate;
            // As strong as the gas held: a crack of throttle barely holds
            // it, and does not wind the hold up for when more gas comes.
            let fade = (1.0 - speed / s.wheelie_top_speed).clamp(0.0, 1.0) * held.gas;
            if fade < 0.05 {
                self.pitch_integral = 0.0;
            } else {
                self.pitch_integral += e * dt * 500.0 * lbf_ft;
            }
            torque += side * (self.pitch_integral + e * s.pitch_rate_gain * lbf_ft) * fade;
        } else {
            self.pitch_integral = 0.0;
        }

        // Backing up: a push towards walking pace backwards.
        if self.reversing && rear_down && speed < 1.0 {
            force += fwd * (s.reverse_speed - speed) * s.reverse_gain;
        }
        // Air drag.
        force -= self.velocity * self.velocity.length() * 0.5 * s.drag;

        if grounded == 0 {
            // The rider throws the bike around; Unleashed's axes are X
            // right, Y up, Z forward with left-handed turns, so yaw and roll
            // flip sign here.
            let spin = r.transpose() * self.spin;
            let x = (c.steer + c.lean.x).clamp(-1.0, 1.0);
            let y = (-c.lean.y).clamp(-1.0, 1.0);
            let m = s.air_moment;
            let mut t = Vec3::new(y * m.x, -x * m.y, -x * m.z);
            let d = s.air_damping;
            if held.steer.abs() >= 0.1 || x.abs() >= 0.1 || fwd.y.abs() >= 0.94 {
                t -= spin * d;
            } else {
                // Let go: level the lean, line up with the way it is going.
                let lean_right = side.y.clamp(-1.0, 1.0).asin();
                t.z -= (s.unroll_gain * lean_right + spin.z) * s.unroll_torque;
                let flat = Vec3::new(self.velocity.x, 0.0, self.velocity.z);
                let facing = Vec3::new(fwd.x, 0.0, fwd.z);
                let off = if ft(flat.length()) > 10.0 && facing.length() > 0.1 {
                    facing.cross(flat).y.atan2(facing.dot(flat))
                } else {
                    0.0
                };
                t.y += (s.unyaw_gain * off - spin.y) * s.unyaw_torque;
                if spin.x.abs() > 0.1 {
                    t.x -= spin.x * d;
                }
                if spin.y.abs() > 0.1 {
                    t.y -= spin.y * d;
                }
            }
            // The rear brake drops the nose, the gas lifts it, each as much
            // as it is held.
            t.x += s.brake_gyro * held.brake - s.gas_gyro * held.gas;
            torque += r * t;
            // And a little sideways push with the stick.
            if ft(self.velocity.length()) > 10.0 {
                let left = Vec3::Y.cross(self.velocity).normalize_or_zero();
                force -= left * x * s.air_force;
            }
        }

        // The frame and the rider against the world.
        let mut crash = false;
        let mut contacts = std::mem::take(&mut self.contacts);
        let legs = [rear_leg(), front_leg()];
        let probes = PROBES.iter().map(|&(p, rad, fatal)| (p, rad, fatal, false)).chain(
            // The wheels against walls, so a tyre into a wall stops rather
            // than climbs.
            (0..2).map(|i| (self.axle(i), legs[i].radius * 0.95, false, true)),
        );
        for (local, radius, fatal, walls_only) in probes {
            let centre = self.to_world(local);
            world.sphere(candidates, centre, radius, &mut contacts);
            for &(n, depth) in &contacts {
                if walls_only && n.dot(up) > 0.5 {
                    continue;
                }
                let p = centre - n * (radius - depth);
                let vp = self.point_velocity(com, p);
                let vn = vp.dot(n);
                let push = (PROBE_SPRING * depth - PROBE_DAMPER * vn).max(0.0);
                let vt = vp - n * vn;
                let rub = vt.normalize_or_zero() * -(PROBE_FRICTION * push).min(vt.length() * 3000.0);
                let f = n * push + rub;
                force += f;
                torque += (p - com).cross(f);
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

        // Integrate. On the ground the bike cannot roll: the lean is set below.
        self.velocity += force / s.mass * dt;
        let i = s.inertia;
        let mut spin = r.transpose() * self.spin;
        let local = r.transpose() * torque;
        let mut accel = (local - spin.cross(i * spin)) / i;
        if grounded > 0 {
            accel.z = 0.0;
            spin.z = 0.0;
        }
        spin += accel * dt;
        spin = spin.clamp(Vec3::new(-20.0, -s.yaw_rate_limit, -20.0), Vec3::new(20.0, s.yaw_rate_limit, 20.0));
        self.spin = r * spin;
        let com = com + self.velocity * dt;
        self.rotation = (Quat::from_scaled_axis(self.spin * dt) * self.rotation).normalize();
        if grounded > 0 {
            let lean = held.steer + s.rider_lean * self.lean.x;
            self.lean_on_ground(lean, speed, normals.normalize_or(Vec3::Y), dt);
        }
        self.position = com - self.rotation * self.cog;
    }

    /// Sets the lean from the bars and the speed: towards `steer` (the
    /// stick, plus the rider's lean) times a lean that grows with speed,
    /// measured from the vertical when slow and from the ground when fast.
    fn lean_on_ground(&mut self, steer: f32, speed: f32, ground: Vec3, dt: f32) {
        let s = setup();
        let upright = speed.signum() * lerp_clamp(speed.abs(), s.stand_upright[0], 0.0, s.stand_upright[1], 1.0);
        let reference = if upright <= 0.0 { Vec3::Y } else { Vec3::Y.lerp(ground, upright) };
        let f = self.forward();
        let Some(across) = (reference - f * reference.dot(f)).try_normalize() else {
            return;
        };
        let up = self.up();
        let now = f.dot(across.cross(up)).atan2(across.dot(up));
        let wanted = steer * s.lean_for(speed);
        let lean = now + (wanted - now) * (s.roll_gain * dt).min(1.0);
        let up = Quat::from_axis_angle(f, lean) * across;
        self.rotation = Quat::from_mat3(&Mat3::from_cols(up.cross(f), up, f)).normalize();
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

/// How high the bike frame's origin sits over flat ground at rest.
pub fn rest_height() -> f32 {
    let s = setup();
    let (f, r) = (frame(s.front_axle), frame(s.rear_axle));
    let base = f.z - r.z;
    let weight = s.mass * GRAVITY * s.gravity_scale;
    let sag = |t: &Tyre, share: f32| share * weight / s.spring_rate(t);
    let front = s.front.radius - f.y - sag(&s.front, -r.z / base);
    let rear = s.rear.radius - r.y - sag(&s.rear, f.z / base);
    (front + rear) * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unleashed::MPH;

    /// A flat floor, 800 m square, at y = 0.
    fn floor() -> World {
        let s = 400.0;
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

    fn gas(throttle: f32) -> Controls {
        Controls {
            throttle,
            ..default()
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
        assert!((bike.position.y - rest_height()).abs() < 0.05, "height {} rest {}", bike.position.y, rest_height());
        assert!(bike.velocity.length() < 0.05);
        assert!((bike.engine.rpm - setup().min_rpm).abs() < 1.0, "idle {}", bike.engine.rpm);
    }

    #[test]
    fn pulls_through_the_gears() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 1.0);
        ride(&w, &mut bike, gas(1.0), 2.0);
        // Reflex v1: the rear tyre (1.3 x load) holds the launch back to
        // about 0.6 g, so 25 mph or so in two seconds.
        let two = bike.speed();
        assert!(two > 9.0 && two < 16.0, "2 s: {two} m/s");
        ride(&w, &mut bike, gas(1.0), 8.0);
        // Reflex's mx450f tops out near 32 m/s (72 mph).
        let ten = bike.speed();
        assert!(ten > 60.0 * MPH && ten < 80.0 * MPH, "10 s: {} mph", ten / MPH);
        assert!(bike.engine.gear >= 3, "gear {}", bike.engine.gear + 1);
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);
        assert!(bike.wheels[1].contact.is_some(), "front up on plain gas");
    }

    /// Partial gas pulls less than full gas and the bike settles slower: the
    /// throttle is not on/off.
    #[test]
    fn partial_throttle_pulls_less() {
        let w = floor();
        let gain = |throttle: f32| {
            let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
            ride(&w, &mut bike, Controls::default(), 1.0);
            while bike.speed() < 14.0 {
                bike.step(&w, gas(1.0), 1.0 / 60.0);
            }
            let start = bike.speed();
            ride(&w, &mut bike, gas(throttle), 3.0);
            bike.speed() - start
        };
        let [quarter, half, full] = [gain(0.25), gain(0.5), gain(1.0)];
        assert!(quarter < half && half < full, "gains {quarter} {half} {full} m/s");
        assert!(quarter < 0.5 * full, "quarter gas {quarter} m/s against full {full}");
        // A crack of gas does not hold the speed up.
        assert!(gain(0.05) < 0.0, "5 % gas gains {} m/s", gain(0.05));
    }

    /// A crack of throttle from a standstill does not kick the bike the
    /// way full throttle does: the slipping clutch passes only what the
    /// engine makes.
    #[test]
    fn partial_throttle_launches_softly() {
        let w = floor();
        let launch = |throttle: f32| {
            let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
            ride(&w, &mut bike, Controls::default(), 1.0);
            ride(&w, &mut bike, gas(throttle), 0.25);
            bike.speed()
        };
        let (crack, full) = (launch(0.05), launch(1.0));
        assert!(crack < 0.3, "5 % gas: {crack} m/s after a quarter second");
        assert!(full > 1.0, "full gas: {full} m/s after a quarter second");
    }

    #[test]
    fn backs_up_slowly() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, Controls { brake: 1.0, ..default() }, 5.0);
        let v = bike.speed();
        assert!((v - setup().reverse_speed).abs() < 0.5, "reversing at {v} m/s");
        assert!(bike.crashed.is_none());
        assert!(bike.up().y > 0.95);
    }

    #[test]
    fn brakes_stop_it() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, gas(1.0), 2.5);
        assert!(bike.speed() > 10.0);
        // Held on past the stop it backs up, so watch for the stop itself.
        let brake = Controls { brake: 1.0, ..default() };
        let mut frames = 0;
        while bike.speed() > 0.0 && frames < 240 {
            bike.step(&w, brake, 1.0 / 60.0);
            frames += 1;
        }
        assert!(frames < 240, "still at {} m/s", bike.speed());
        assert!(bike.crashed.is_none());
    }

    #[test]
    fn braking_never_spins_the_rear_backwards() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, gas(1.0), 2.5);
        let brake = Controls { brake: 1.0, ..default() };
        while bike.speed() > 0.0 {
            bike.step(&w, brake, 1.0 / 60.0);
            assert!(bike.wheel_spin >= 0.0, "rear at {} rad/s, {} m/s", bike.wheel_spin, bike.speed());
        }
    }

    #[test]
    fn brake_held_through_the_stop_backs_up() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, gas(1.0), 1.5);
        let brake = Controls { brake: 1.0, ..default() };
        ride(&w, &mut bike, brake, 0.5);
        assert!(bike.speed() > 1.0, "backing up before it stopped: {} m/s", bike.speed());
        ride(&w, &mut bike, brake, 6.0);
        let v = bike.speed();
        assert!((v - setup().reverse_speed).abs() < 0.5, "reversing at {v} m/s");
        // Let go and it only coasts, slowing down.
        ride(&w, &mut bike, Controls::default(), 1.0);
        assert!(bike.speed() > v + 0.3, "still backing up at {} m/s", bike.speed());
    }

    #[test]
    fn steers_and_leans_into_the_turn() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, gas(0.6), 1.5);
        let before = bike.heading();
        // Right on the stick: the heading turns right (towards -X, so the
        // angle about +Y falls) and the bike leans right.
        ride(
            &w,
            &mut bike,
            Controls {
                throttle: 0.4,
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
    fn holds_a_turn_at_speed() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, 0.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        ride(&w, &mut bike, gas(1.0), 2.0);
        // Half lock at speed for three seconds: round and round, upright.
        ride(
            &w,
            &mut bike,
            Controls {
                throttle: 0.5,
                steer: -0.5,
                ..default()
            },
            3.0,
        );
        assert!(bike.crashed.is_none());
        assert_eq!(bike.grounded(), 2);
        assert!(bike.speed() > 5.0, "{}", bike.speed());
        assert!(bike.position.length() < 120.0, "ran off to {:?}", bike.position);
    }

    #[test]
    fn leaning_back_on_the_gas_lifts_the_front() {
        let w = floor();
        let mut bike = Bike::dropped(&w, Vec3::new(0.0, 1.0, -350.0), 0.0);
        ride(&w, &mut bike, Controls::default(), 0.5);
        // From a standing start: the hold fades out with speed.
        let mut lifted = false;
        for _ in 0..150 {
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
        // Held, not flipped.
        assert!(bike.crashed.is_none());
        assert!(bike.forward().y < 0.8, "forward {:?}", bike.forward());
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
            1.0,
        );
        // Forward on the stick: nose down.
        assert!(bike.forward().y < -0.3, "forward {:?}", bike.forward());
    }

    #[test]
    fn air_lean_levels_itself() {
        let w = floor();
        let mut bike = Bike::new(Vec3::new(0.0, 60.0, 0.0), 0.0);
        bike.rotation = Quat::from_rotation_z(0.6);
        ride(&w, &mut bike, Controls::default(), 1.5);
        assert!(bike.up().y > 0.97, "up {:?}", bike.up());
    }

    /// The nose's pitch (forward.y) after `seconds` in the air.
    fn air_nose_after(c: Controls, seconds: f32) -> f32 {
        let w = floor();
        let mut bike = Bike::new(Vec3::new(0.0, 60.0, 0.0), 0.0);
        ride(&w, &mut bike, c, seconds);
        bike.forward().y
    }

    #[test]
    fn gas_in_the_air_lifts_the_front_a_little() {
        let coast = air_nose_after(default(), 1.0);
        let gas = air_nose_after(gas(1.0), 1.0);
        assert!(gas > coast + 0.05, "gas {gas} coast {coast}");
        // A little, not a backflip.
        assert!(gas < 0.6, "gas {gas}");
    }

    #[test]
    fn rear_brake_in_the_air_drops_the_front() {
        let coast = air_nose_after(default(), 0.5);
        let brake = air_nose_after(
            Controls {
                brake: 1.0,
                ..default()
            },
            0.5,
        );
        assert!(brake < coast - 0.05, "brake {brake} coast {coast}");
        assert!(brake > -0.7, "brake {brake}");
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
        ride(&w, &mut bike, gas(0.5), 4.0);
        assert!(bike.position.z < 10.0, "through the wall: z {}", bike.position.z);
    }

    /// A frame with no time in it, with the wheels off the ground and the
    /// gas on (the headless `--bike` start on Narrows), leaves the bike as
    /// it was instead of filling it with NaN.
    #[test]
    fn zero_time_step_is_harmless() {
        let w = floor();
        let mut bike = Bike::new(Vec3::new(0.0, 3.0, 0.0), 0.0);
        let c = Controls {
            throttle: 0.4,
            ..default()
        };
        bike.step(&w, c, 0.0);
        assert!(bike.position.is_finite() && bike.velocity.is_finite() && bike.spin.is_finite());
        ride(&w, &mut bike, c, 2.0);
        assert!(bike.position.is_finite() && bike.velocity.is_finite());
        assert!(bike.position.y < 3.0, "never fell: y {}", bike.position.y);
    }

    fn default() -> Controls {
        Controls::default()
    }
}
