//! `reflex_bevy`: MX vs ATV Reflex's dirt bike (the `reflex_core` simulation) in Bevy 0.20, with no physics
//! engine. A game conforms to this crate's contract:
//!
//! 1. **Add [`ReflexPlugin`]** and provide a [`ReflexGround`] resource (see [`ground`]: triangles, the game's own
//!    collision through [`GroundQuery`], or tagged Bevy meshes with the `mesh` feature).
//! 2. **Spawn a bike**: an entity with [`ReflexBike`] (built from the player's own Reflex vehicle file) and a
//!    `Transform` whose origin is on the riding surface, facing its -Z. The bike starts at rest there.
//! 3. **Drive it**: write its [`ReflexInput`] every frame before [`ReflexSet::Step`] (or add
//!    `ReflexDefaultControls` to use the built-in keyboard / gamepad mapping, feature `controls`, on by default).
//! 4. **Show it**: the bike entity's `Transform` follows the chassis. Entities with a [`ReflexPart`] follow one
//!    body or linkage each (chassis, rider, wheels with their spin, steering, swingarm, hubs): put the game's
//!    models on them. With the `gizmos` feature, `ReflexDebugDrawPlugin` draws a stick model.
//! 5. **Read it**: [`ReflexBikeState`] has speed, rpm, gear, crash / air state, steer and lean, and per wheel the
//!    contact point, normal, load and grip.
//!
//! Timing: the simulation runs in fixed 7.5 ms substeps with a carry accumulator, like the original, from the
//! frame's `Time` delta in `Update` (inputs are latched once per rendered frame, as in Reflex). Poses are
//! interpolated between the last two substeps. Frames longer than 0.1 s are clamped.
//!
//! Units: metres, seconds, radians, newtons, Bevy's frame (+Y up, -Z forward), unless a name says otherwise.
//!
//! Nothing from Reflex is bundled: the vehicle values are read from the user's own files at run time
//! ([`ReflexBike::from_file`]: the stock JSON or XML vehicle dumps, or an in-memory tuning dump).
//!
//! The smallest integration:
//!
//! ```no_run
//! use bevy::prelude::*;
//! use reflex_bevy::{Part, ReflexBike, ReflexBikeState, ReflexGround, ReflexInput, ReflexPart, ReflexPlugin, ReflexSet, TriangleGround};
//!
//! fn setup(mut commands: Commands) {
//!     // the level's collision, once (metres, Bevy frame, counter-clockwise seen from the riding side)
//!     let mut ground = TriangleGround::new();
//!     let (a, b, c, d) = (Vec3::new(-100., 0., -100.), Vec3::new(100., 0., -100.), Vec3::new(100., 0., 100.), Vec3::new(-100., 0., 100.));
//!     ground.add_triangle(a, d, c);
//!     ground.add_triangle(a, c, b);
//!     commands.insert_resource(ReflexGround::from_triangles(ground));
//!
//!     // the bike, on the ground at the origin, facing -Z; the game's wheel model follows the front wheel
//!     let bike = ReflexBike::from_file("path/to/the/players/mx450f_vehicle.json").expect("vehicle file");
//!     let bike = commands.spawn((bike, Transform::IDENTITY)).id();
//!     commands.spawn((ReflexPart { bike, part: Part::FrontWheel }, ChildOf(bike) /* , the wheel's Mesh3d ... */));
//! }
//!
//! fn drive(keys: Res<ButtonInput<KeyCode>>, mut bikes: Query<&mut ReflexInput>) {
//!     for mut input in &mut bikes {
//!         input.throttle = if keys.pressed(KeyCode::KeyW) { 1.0 } else { 0.0 };
//!     }
//! }
//!
//! fn hud(bikes: Query<&ReflexBikeState>) {
//!     for s in &bikes {
//!         println!("{:.1} m/s, gear {}, {:.0} rpm", s.forward_speed, s.gear, s.rpm);
//!     }
//! }
//!
//! App::new()
//!     .add_plugins((MinimalPlugins, ReflexPlugin))
//!     .add_systems(Startup, setup)
//!     .add_systems(Update, (drive.before(ReflexSet::Step), hud.after(ReflexSet::Sync)))
//!     .run();
//! ```

pub mod convert;
pub mod ground;
#[cfg(feature = "controls")]
pub mod input;
#[cfg(feature = "gizmos")]
pub mod debug;
#[cfg(feature = "mesh")]
pub mod mesh;

use std::path::Path;

use bevy::ecs::hierarchy::ChildOf;
use bevy::prelude::*;
use reflex_core::bike::{CHASSIS, FRONT, REAR, RIDER};
use reflex_core::{Garage, Inputs, Sim, Vehicle, FT_TO_M, RPM_PER_RAD_S};

pub use ground::{GroundQuery, ReflexGround, TriangleGround};
#[cfg(feature = "controls")]
pub use input::{ReflexDefaultControls, ReflexInputMap};
pub use reflex_core;

use convert::{angvel_to_bevy, dir_to_bevy, pos_to_bevy, pos_to_reflex, quat_to_bevy, quat_to_reflex};

/// Longest frame the bikes catch up on (the core also caps a frame at 6 substeps = 45 ms of simulated time).
pub const MAX_FRAME_DT: f32 = 0.1;
const LBF_TO_N: f64 = 4.448_221_6;

/// Adds the bike systems to `Update`, in the order of [`ReflexSet`]. Inserts an empty [`ReflexGround`] unless the
/// game already did.
#[derive(Default)]
pub struct ReflexPlugin;

impl Plugin for ReflexPlugin {
    fn build(&self, app: &mut App) {
        if !app.world().contains_resource::<ReflexGround>() {
            app.init_resource::<ReflexGround>();
        }
        #[cfg(feature = "controls")]
        app.init_resource::<ReflexInputMap>()
            .add_systems(Update, input::default_controls.in_set(ReflexSet::Input));
        app.configure_sets(Update, (ReflexSet::Input, ReflexSet::Step, ReflexSet::Sync).chain())
            .add_systems(Update, (place_new_bikes, step_bikes).chain().in_set(ReflexSet::Step))
            .add_systems(Update, sync_parts.in_set(ReflexSet::Sync));
    }
}

/// System sets, run in this order in `Update`. Write [`ReflexInput`] before `Step`; read poses and
/// [`ReflexBikeState`] after `Sync` (e.g. a follow camera, sound).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReflexSet {
    /// The built-in controls write `ReflexInput` here.
    Input,
    /// New bikes are placed and every bike simulates one rendered frame.
    Step,
    /// `ReflexPart` entities follow their bike.
    Sync,
}

/// One Reflex bike. Spawn it with a `Transform` on the riding surface (origin on the ground, facing -Z); the
/// entity's `Transform` then follows the chassis every frame. Also inserts [`ReflexInput`] and
/// [`ReflexBikeState`] when they are missing.
#[derive(Component, Clone)]
#[require(ReflexInput, ReflexBikeState, Transform)]
pub struct ReflexBike {
    sim: Box<Sim>,
    /// Where `ReflexInput::respawn` puts the bike (set to the spawn transform; change it for checkpoints).
    pub respawn_point: Transform,
    /// Height of the chassis above the respawn point's origin when placed (m). The suspension settles from there.
    pub spawn_height: f32,
    placed: bool,
}

impl ReflexBike {
    /// A bike from a vehicle file of the user's own Reflex data (stock JSON / XML dump, or a tuning dump).
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, reflex_core::vehicle::LoadError> {
        Ok(Self::new(Vehicle::load(path)?))
    }

    /// A bike from vehicle values already loaded.
    pub fn new(vehicle: Vehicle) -> Self {
        ReflexBike {
            sim: Box::new(Sim::new(vehicle, reflex_core::Vec3::ZERO, reflex_core::Quat::IDENTITY)),
            respawn_point: Transform::IDENTITY,
            spawn_height: 0.6,
            placed: false,
        }
    }

    /// Garage multipliers (1 = stock): suspension stiffness, gearing, brakes.
    pub fn with_garage(mut self, suspension: f64, gearing: f64, brakes: f64) -> Self {
        self.sim.set_garage(Garage { suspension, gearing, brakes });
        self
    }

    /// Respawn automatically (upright, where it lies) one second after a wreck, as Reflex does. On by default;
    /// off: the game decides (see [`ReflexBikeState::crashed`] and [`ReflexBike::respawn_at`]).
    pub fn with_auto_respawn(mut self, on: bool) -> Self {
        self.sim.auto_respawn = on;
        self
    }

    /// Puts the bike at rest at `at` (origin on the riding surface, facing its -Z), `spawn_height` above it.
    /// Clears the wreck.
    pub fn respawn_at(&mut self, at: Transform) {
        let up = at.rotation * Vec3::Y;
        let origin = at.translation + up * self.spawn_height;
        self.sim.reset(pos_to_reflex(origin), quat_to_reflex(at.rotation));
        self.placed = true;
    }

    /// Chassis pose to draw this frame (interpolated between the last two substeps).
    pub fn chassis_transform(&self) -> Transform {
        self.body_transform(CHASSIS)
    }

    /// The pose of a body or linkage, in world space.
    pub fn part_transform(&self, part: Part) -> Transform {
        let s = &*self.sim;
        let g = &s.geometry;
        let (cx, cq) = s.render_pose(CHASSIS);
        let steering = cq * reflex_core::Quat::from_axis_angle(g.fork_axis, s.ctl.steer_angle);
        let swingarm = || {
            // pitch about the chassis X axis so the part's +Z (Bevy) points from the pivot to the rear axle
            let d = cq.inv_rotate(s.render_pose(REAR).0 - cx) - g.swingarm_pivot;
            let a = d.y.atan2(-d.z);
            cq * reflex_core::Quat::from_axis_angle(reflex_core::Vec3::X, a)
        };
        let t = |x: reflex_core::Vec3, q: reflex_core::Quat| Transform::from_translation(pos_to_bevy(x)).with_rotation(quat_to_bevy(q));
        match part {
            Part::Chassis => self.body_transform(CHASSIS),
            Part::Rider => self.body_transform(RIDER),
            Part::FrontWheel => self.body_transform(FRONT),
            Part::RearWheel => self.body_transform(REAR),
            Part::Steering => t(cx + cq.rotate(g.fork_top), steering),
            Part::FrontHub => t(s.render_pose(FRONT).0, steering),
            Part::SwingArm => t(cx + cq.rotate(g.swingarm_pivot), swingarm()),
            Part::RearHub => t(s.render_pose(REAR).0, swingarm()),
        }
    }

    fn body_transform(&self, i: usize) -> Transform {
        let (x, q) = self.sim.render_pose(i);
        Transform::from_translation(pos_to_bevy(x)).with_rotation(quat_to_bevy(q))
    }

    /// Tyre radii (front, rear), m.
    pub fn wheel_radii(&self) -> [f32; 2] {
        let r = |i: usize| (self.sim.params.tyres[i].radius * FT_TO_M) as f32;
        [r(0), r(1)]
    }

    /// The simulation, in Reflex units and frame (ft, +Z forward), for tools and advanced use.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// Mutable access to the simulation (Reflex units and frame).
    pub fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
    }
}

/// The rider's inputs for the next frame. Sticks and triggers in -1..1 / 0..1, already past the game's
/// deadzone (Reflex applies XInput's 0.24 deadzone to sticks and ignores triggers under 30/255; the built-in
/// controls do the same).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ReflexInput {
    /// 0..1.
    pub throttle: f32,
    /// Both brake levers, 0..1 (Reflex's default pad profile puts both on LT).
    pub brake: f32,
    /// Handlebars, -1 left .. +1 right.
    pub steer: f32,
    /// Rider lean: x -1 left .. +1 right, y -1 back .. +1 forward.
    pub lean: Vec2,
    /// Reflex's brake / reverse button: brakes, and backs up from a standstill.
    pub reverse: bool,
    /// Clutch held (releasing it at revs dumps the clutch).
    pub clutch: bool,
    /// Put the bike back at its `respawn_point` this frame (ignored for the first 0.5 s of a wreck).
    pub respawn: bool,
}

impl ReflexInput {
    fn to_reflex(self) -> Inputs {
        let c = |v: f32| v.clamp(-1.0, 1.0) as f64;
        let brake = self.brake.clamp(0.0, 1.0) as f64;
        Inputs {
            throttle: self.throttle.clamp(0.0, 1.0) as f64,
            front_brake: brake,
            rear_brake: brake,
            brake_button: if self.reverse { 1.0 } else { 0.0 },
            steer_x: c(self.steer),
            steer_y: 0.0,
            rider_x: c(self.lean.x),
            rider_y: c(self.lean.y),
            clutch: self.clutch,
        }
    }
}

/// Per-wheel output of [`ReflexBikeState`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelState {
    /// Touching the ground this frame.
    pub contact: bool,
    /// Contact point and ground normal (world).
    pub point: Vec3,
    pub normal: Vec3,
    /// Tyre load (N, low-passed).
    pub load: f32,
    /// The tyre holds sideways (not sliding).
    pub side_grip: bool,
    /// The tyre holds along its rolling direction (not spinning or locked).
    pub roll_grip: bool,
    /// Contact-patch slip, m/s: sideways (across the tyre) and along it (< 0 = the patch slides back: driving).
    pub slip_lateral: f32,
    pub slip_longitudinal: f32,
    /// Spin, rad/s (+ = rolling forward).
    pub spin: f32,
    /// Suspension length from its top mount to the axle (m) and its rest length.
    pub suspension_length: f32,
    pub suspension_rest: f32,
}

/// What the bike did in the last frame (read after [`ReflexSet::Step`]).
#[derive(Component, Clone, Copy, Debug, Default, PartialEq)]
pub struct ReflexBikeState {
    /// Speed along the chassis' forward axis, m/s (negative backwards), and the total speed.
    pub forward_speed: f32,
    pub speed: f32,
    /// Chassis velocity and angular velocity (world, m/s and rad/s).
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub rpm: f32,
    /// 1..5.
    pub gear: u8,
    pub airborne: bool,
    /// Wrecked (respawns after 1 s when auto respawn is on).
    pub crashed: bool,
    /// Parked: braking at a standstill holds the bike on slopes.
    pub at_rest: bool,
    /// Front wheel steer angle, rad (+ = right), including the counter-steer.
    pub steer_angle: f32,
    /// Commanded lean, rad (+ = leaning right).
    pub lean_command: f32,
    /// Front, rear.
    pub wheels: [WheelState; 2],
    /// The bike was moved this frame (spawn, respawn or the automatic respawn after a wreck): skip motion blur,
    /// snap cameras.
    pub teleported: bool,
    /// Substeps run this frame (0..6) and frames simulated so far.
    pub substeps: u8,
    pub frames: u64,
}

/// A body or linkage of a bike to draw.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// The frame, tank and seat (the bike entity's own transform is this too).
    Chassis,
    /// The rider's mass (moves with rider lean).
    Rider,
    /// The wheels, including their spin about the axle (local X) and, at the front, the steering.
    FrontWheel,
    RearWheel,
    /// Bars and fork crowns: origin at the steering head, turning with the steer about the fork axis.
    Steering,
    /// The front axle with the steering rotation but no spin (fork lowers, brake caliper).
    FrontHub,
    /// Origin at the swingarm pivot, pitched so local +Z points from the pivot to the rear axle.
    SwingArm,
    /// The rear axle with the swingarm rotation but no spin (chain adjuster, caliper).
    RearHub,
}

/// Put this on an entity to make it follow one part of a bike. A child of the bike entity gets a local
/// transform relative to the chassis; any other entity gets a world transform.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
#[require(Transform)]
pub struct ReflexPart {
    pub bike: Entity,
    pub part: Part,
}

fn place_new_bikes(mut bikes: Query<(&mut ReflexBike, &Transform, &mut ReflexBikeState), Added<ReflexBike>>) {
    for (mut bike, at, mut state) in &mut bikes {
        if !bike.placed {
            let at = *at;
            bike.respawn_point = at;
            bike.respawn_at(at);
            state.teleported = true;
        }
    }
}

fn step_bikes(
    time: Res<Time>,
    ground: Res<ReflexGround>,
    mut bikes: Query<(&mut ReflexBike, &ReflexInput, &mut ReflexBikeState, &mut Transform)>,
) {
    for (mut bike, input, mut state, mut transform) in &mut bikes {
        bike.step(time.delta_secs(), input, ground.as_ground(), &mut state);
        *transform = bike.chassis_transform();
    }
}

impl ReflexBike {
    /// Simulates one rendered frame of `dt` seconds (clamped to [`MAX_FRAME_DT`]) and writes `state`: what
    /// [`ReflexPlugin`] does for every bike entity, for games that keep the bike outside the ECS and step it
    /// from their own systems, on any ground (`ReflexGround::as_ground`, or the game's own
    /// [`reflex_core::Ground`]). A bike never placed (no [`ReflexBike::respawn_at`]) stays at the origin.
    pub fn step(&mut self, dt: f32, input: &ReflexInput, ground: &dyn reflex_core::Ground, state: &mut ReflexBikeState) {
        let dt = dt.min(MAX_FRAME_DT) as f64;
        let mut teleported = std::mem::take(&mut state.teleported);
        if input.respawn && self.sim.manual_respawn_allowed() {
            let at = self.respawn_point;
            self.respawn_at(at);
            teleported = true;
        }
        let was_crashed = self.sim.crashed;
        let n = if dt > 0.0 { self.sim.frame(dt, input.to_reflex(), ground) } else { 0 };
        teleported |= was_crashed && !self.sim.crashed;
        *state = bike_state(self, n, state.frames + 1, teleported);
    }
}

fn bike_state(bike: &ReflexBike, substeps: usize, frames: u64, teleported: bool) -> ReflexBikeState {
    let s = &*bike.sim;
    let b0 = &s.sys.bodies[CHASSIS];
    let lengths = reflex_core::suspension::lengths(&s.sys, &s.geometry);
    let m = |v: f64| (v * FT_TO_M) as f32;
    let wheel = |i: usize, body: usize| {
        let t = &s.tyres[i];
        WheelState {
            contact: t.contact,
            point: if t.contact { pos_to_bevy(t.point) } else { Vec3::ZERO },
            normal: if t.contact { dir_to_bevy(t.normal) } else { Vec3::ZERO },
            load: (t.n_filt * LBF_TO_N) as f32,
            side_grip: t.side_grip,
            roll_grip: t.roll_grip,
            slip_lateral: m(t.slip.x),
            slip_longitudinal: m(t.slip.z),
            spin: s.sys.bodies[body].w.x as f32,
            suspension_length: m(lengths[i]),
            suspension_rest: m(s.params.suspension[i].rest_length),
        }
    };
    ReflexBikeState {
        forward_speed: m(s.forward_speed()),
        speed: m(b0.v.length()),
        velocity: pos_to_bevy(b0.v),
        angular_velocity: angvel_to_bevy(b0.omega()),
        rpm: (s.drive.engine_w * RPM_PER_RAD_S) as f32,
        gear: s.drive.gear as u8 + 1,
        airborne: s.airborne,
        crashed: s.crashed,
        at_rest: s.at_rest,
        steer_angle: s.ctl.steer_angle as f32,
        lean_command: -s.ctl.lean_cmd as f32,
        wheels: [wheel(0, FRONT), wheel(1, REAR)],
        teleported,
        substeps: substeps as u8,
        frames,
    }
}

fn sync_parts(
    bikes: Query<&ReflexBike>,
    mut parts: Query<(&ReflexPart, &mut Transform, Option<&ChildOf>), Without<ReflexBike>>,
) {
    for (part, mut transform, parent) in &mut parts {
        let Ok(bike) = bikes.get(part.bike) else { continue };
        let world = bike.part_transform(part.part);
        *transform = if parent.is_some_and(|p| p.parent() == part.bike) {
            let chassis = bike.chassis_transform();
            let inv = chassis.rotation.inverse();
            Transform::from_translation(inv * (world.translation - chassis.translation)).with_rotation(inv * world.rotation)
        } else {
            world
        };
    }
}
