//! "Reflex": the bike on MX vs ATV Reflex's own simulation, not a model
//! with Reflex's numbers. `reflex_core` (vendored in
//! `third_party/reflex-physics`) reimplements the game's four-body bike
//! step by step and was checked frame by frame against recordings of the
//! game; `reflex_bevy` puts it in Bevy's frame. The Dakumoto 450F's values
//! (`mx450f_vehicle`) are imported once from the player's own Reflex install
//! (`reflex_assets::imported`), so without Reflex this model is not offered
//! and the bike rides Reflex v1.
//!
//! The rest of the game keeps reading `physics::Bike`: after every frame
//! the simulation's pose, speeds, wheels, engine and crash are written into
//! it, and whatever moved the `Bike` from outside since (a platform under
//! it, a gravity lift, a launch speed) is handed back to the simulation.
//! The ground is the bike's own collision world (`physics::World`), moving
//! objects included.

use std::cell::RefCell;
use std::f32::consts::PI;
use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bevy::log::{info, warn};
use bevy::math::{Quat, Vec3};
use reflex_bevy::convert::{dir_to_reflex, pos_to_bevy, pos_to_reflex, quat_to_reflex};
use reflex_bevy::ground::reflex_triangle;
use reflex_bevy::reflex_core::bike::{FRONT, REAR};
use reflex_bevy::reflex_core::{self, Vehicle};
use reflex_bevy::{ReflexBike, ReflexBikeState, ReflexInput};

use super::physics::{Bike, Controls, FRAME_LIFT, Hit, World, front_leg, rear_leg};
use super::surface::Surface;

/// The bike class read from Reflex: the Dakumoto 450F the model draws.
const CLASS: &str = reflex_assets::imported::CLASS;

static ON: AtomicBool = AtomicBool::new(false);
/// Counts the changes, so a bike sees when the choice changed under it.
static GENERATION: AtomicU64 = AtomicU64::new(0);
/// The tuning menu's multipliers for Reflex itself (1 = as the game ships it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tune {
    /// The engine's torque curve.
    pub power: f32,
    /// The gear ratios (Reflex's garage gearing: below 1 taller).
    pub gearing: f32,
    /// Both tyres' grip along the ground (drive and brakes): `StaticMuRoll`,
    /// which the sliding grip is a share of.
    pub traction: f32,
}

impl Tune {
    pub const STOCK: Tune = Tune {
        power: 1.0,
        gearing: 1.0,
        traction: 1.0,
    };
}

static TUNE: RwLock<Tune> = RwLock::new(Tune::STOCK);

/// Rides bikes on Reflex's simulation (when the vehicle values could be
/// read), tuned by `tune`; a bike already out takes the change at its next
/// step.
pub fn set(on: bool, tune: Tune) {
    let mut held = TUNE.write().unwrap_or_else(|e| e.into_inner());
    let changed = ON.swap(on, Ordering::Relaxed) != on || *held != tune;
    *held = tune;
    if changed {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
}

/// Switches Reflex's simulation on or off, keeping the tuning.
pub fn set_on(on: bool) {
    set(on, tune());
}

fn tune() -> Tune {
    *TUNE.read().unwrap_or_else(|e| e.into_inner())
}

/// The 450F's values with `tune`'s power and traction in them.
fn tuned(tune: Tune) -> Option<Vehicle> {
    let mut v = vehicle()?.clone();
    if let Some(reflex_core::Value::Curve(c)) = v.values.get_mut("Engine_TorqueCurve") {
        let k = tune.power as f64;
        c.y.iter_mut().for_each(|y| *y *= k);
        c.mleft *= k;
        c.mright *= k;
    }
    let k = tune.traction as f64;
    for key in ["FrontTire_StaticMuRoll", "RearTire_StaticMuRoll"] {
        match v.values.get_mut(key) {
            Some(reflex_core::Value::Triple(t)) => t.iter_mut().for_each(|x| *x *= k),
            Some(reflex_core::Value::Scalar(x)) => *x *= k,
            _ => {}
        }
    }
    Some(v)
}

/// The stock 450F's peak torque: N·m and rpm.
pub fn peak_torque() -> Option<(f32, f32)> {
    let c = vehicle()?.curve("Engine_TorqueCurve")?;
    let (lo, hi) = (c.x.first().copied()?, c.x.last().copied()?);
    let (rpm, lbf_ft) = (0..=400)
        .map(|i| {
            let x = lo + (hi - lo) * i as f64 / 400.0;
            (x, c.eval(x))
        })
        .fold((0.0, f64::MIN), |a, b| if b.1 > a.1 { b } else { a });
    Some(((lbf_ft * 1.355_818) as f32, rpm as f32))
}

/// Road speed at the limiter in 5th with the gear ratios times `gearing`, km/h.
pub fn top_of_fifth(gearing: f32) -> Option<f32> {
    let p = reflex_core::Params::with_garage(
        vehicle()?,
        reflex_core::Garage {
            gearing: gearing as f64,
            ..Default::default()
        },
    );
    let ratio = *p.gear_ratios.last()?;
    let ft_s = p.engine_max_speed / ratio * p.tyres[1].radius;
    Some((ft_s * reflex_core::FT_TO_M * 3.6) as f32)
}

/// Changes each time [`set_on`] switches.
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// Whether bikes ride on Reflex's simulation: chosen in the tuning menu and
/// the 450F's values read.
pub fn on() -> bool {
    ON.load(Ordering::Relaxed) && vehicle().is_some()
}

/// The 450F's values imported from the player's Reflex
/// (`reflex_assets::imported`), read once they are there (tried again when
/// Reflex is set up, so one chosen in Settings counts without a restart). `MCC_REFLEX_VEHICLE` (a developer
/// override) names a vehicle dump file instead (the stock JSON or XML dumps
/// `reflex_core` reads).
pub fn vehicle() -> Option<&'static Vehicle> {
    /// The values once read, else the source last tried.
    static VEHICLE: Mutex<(Option<&'static Vehicle>, Option<Option<PathBuf>>)> = Mutex::new((None, None));
    let mut held = VEHICLE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(v) = held.0 {
        return Some(v);
    }
    // Without the override only whether Reflex is there matters: the path is a marker.
    let source = std::env::var_os("MCC_REFLEX_VEHICLE")
        .map(PathBuf::from)
        .or_else(|| reflex_assets::imported::available().then(|| PathBuf::from("imported")));
    if held.1.as_ref() == Some(&source) {
        return None;
    }
    held.0 = load(source.as_deref()).map(|v| &*Box::leak(Box::new(v)));
    held.1 = Some(source);
    held.0
}

fn load(source: Option<&std::path::Path>) -> Option<Vehicle> {
    let source = source?;
    if std::env::var_os("MCC_REFLEX_VEHICLE").is_some() {
        return Vehicle::load(source)
            .inspect_err(|e| warn!("bike: MCC_REFLEX_VEHICLE: {e}"))
            .ok();
    }
    let xml = reflex_assets::imported::vehicle_xml()
        .inspect_err(|e| warn!("bike: Reflex's {CLASS} physics: {e}"))
        .ok()?;
    let v = Vehicle::from_xml_str(&xml)
        .inspect_err(|e| warn!("bike: Reflex's {CLASS} physics: {e}"))
        .ok()?;
    info!("bike: Reflex's {CLASS} physics read ({} values)", v.values.len());
    Some(v)
}

/// Halo's bike faces +Z, Reflex's (in Bevy's frame) -Z: half a turn about Y.
fn flip() -> Quat {
    Quat::from_rotation_y(PI)
}

/// The simulation of one bike and what was last written into its `Bike`.
#[derive(Clone)]
pub struct Exact {
    bike: ReflexBike,
    state: ReflexBikeState,
    /// Placed on the ground yet (the first step does it, with the world).
    placed: bool,
    /// The `Bike`'s position, rotation and velocity as last written.
    written: (Vec3, Quat, Vec3),
    /// What the ground under each wheel (front, rear) was made of last
    /// step, where it touched.
    ground: [Option<Surface>; 2],
}

impl std::fmt::Debug for Exact {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exact").field("state", &self.state).finish_non_exhaustive()
    }
}

/// The bike's collision world as Reflex's ground.
struct Ground<'a> {
    world: &'a World,
    scratch: RefCell<(Vec<u32>, Vec<[Vec3; 3]>)>,
}

impl reflex_core::Ground for Ground<'_> {
    fn triangles(&self, center: reflex_core::Vec3, half_extent: f64, out: &mut Vec<reflex_core::Triangle>) {
        let mut scratch = self.scratch.borrow_mut();
        let (ids, tris) = &mut *scratch;
        let half = (half_extent * reflex_core::FT_TO_M) as f32;
        self.world.triangles_near(pos_to_bevy(center), half, ids, tris);
        out.extend(tris.iter().filter_map(|[a, b, c]| reflex_triangle(*a, *b, *c)));
    }

    fn raycast(&self, a: reflex_core::Vec3, b: reflex_core::Vec3) -> Option<(reflex_core::Vec3, reflex_core::Vec3)> {
        let (from, to) = (pos_to_bevy(a), pos_to_bevy(b));
        let length = from.distance(to);
        if length < 1e-6 {
            return None;
        }
        self.world
            .raycast_far(from, (to - from) / length, length)
            .map(|h| (pos_to_reflex(h.point), dir_to_reflex(h.normal)))
    }
}

impl Exact {
    /// A simulation for `bike`, placed where it stands at its first step.
    pub fn new(bike: &Bike) -> Option<Exact> {
        Exact::tuned(bike, tune())
    }

    /// The same with the tuning given.
    pub fn tuned(bike: &Bike, tune: Tune) -> Option<Exact> {
        Some(Exact {
            bike: ReflexBike::new(tuned(tune)?).with_garage(1.0, tune.gearing as f64, 1.0),
            state: ReflexBikeState::default(),
            placed: false,
            written: (bike.position, bike.rotation, bike.velocity),
            ground: [None; 2],
        })
    }

    /// The chassis (Reflex's centre of mass, Bevy's frame) for a `Bike` pose.
    fn chassis_of(&self, position: Vec3, rotation: Quat) -> (Vec3, Quat) {
        let turn = rotation * flip();
        let root = position - rotation * (Vec3::Y * FRAME_LIFT);
        (root + turn * pos_to_bevy(self.bike.sim().geometry.cm_offset), turn)
    }

    /// Puts the simulation where `bike` is: on the ground under it if that
    /// is close, else in the air as it is, moving at its speed.
    fn place(&mut self, bike: &Bike, world: &World) {
        let (chassis, turn) = self.chassis_of(bike.position, bike.rotation);
        let down = turn * Vec3::NEG_Y;
        let spawn = self.bike.spawn_height;
        match world.raycast_far(chassis, down, spawn + 0.4) {
            Some(hit) => {
                let at = bevy::transform::components::Transform::from_translation(hit.point).with_rotation(turn);
                self.bike.respawn_at(at);
                self.bike.respawn_point = at;
            }
            None => self.bike.sim_mut().reset(pos_to_reflex(chassis), quat_to_reflex(turn)),
        }
        let v = pos_to_reflex(bike.velocity);
        let forward = bike.velocity.dot(bike.forward());
        let radii = self.bike.wheel_radii();
        let sim = self.bike.sim_mut();
        for (i, body) in sim.sys.bodies.iter_mut().enumerate() {
            body.v = v;
            if i == FRONT || i == REAR {
                let r = radii[if i == FRONT { 0 } else { 1 }].max(0.1);
                body.w.x = (forward / r) as f64;
            }
        }
        self.placed = true;
    }

    /// Hands what moved the `Bike` from outside since the last step to the
    /// simulation: a rigid move of the whole bike, and a change of speed.
    fn take_outside_changes(&mut self, bike: &Bike) {
        let (p0, r0, v0) = self.written;
        let moved = bike.position.distance_squared(p0) > 1e-10 || bike.rotation.dot(r0).abs() < 1.0 - 1e-7;
        let sped = bike.velocity.distance_squared(v0) > 1e-8;
        if !moved && !sped {
            return;
        }
        let turn = (bike.rotation * r0.inverse()).normalize();
        let shift = |p: Vec3| bike.position + turn * (p - p0);
        let dv = bike.velocity - v0;
        let sim = self.bike.sim_mut();
        let rturn = quat_to_reflex(turn);
        for body in sim.sys.bodies.iter_mut() {
            if moved {
                body.x = pos_to_reflex(shift(pos_to_bevy(body.x)));
                body.prev_x = pos_to_reflex(shift(pos_to_bevy(body.prev_x)));
                body.q = (rturn * body.q).normalize();
                body.prev_q = (rturn * body.prev_q).normalize();
                body.v = pos_to_reflex(turn * pos_to_bevy(body.v));
            }
            if sped {
                body.v = body.v + pos_to_reflex(dv);
            }
        }
    }

    /// One frame of `dt` seconds: the simulation runs, then `bike` shows it.
    pub fn step(&mut self, bike: &mut Bike, world: &World, controls: Controls, dt: f32) {
        if !self.placed {
            self.place(bike, world);
        } else {
            self.take_outside_changes(bike);
        }
        let input = ReflexInput {
            throttle: controls.throttle.clamp(0.0, 1.0),
            brake: controls.brake.clamp(0.0, 1.0),
            // Reflex steers +1 right; our stick is +1 right too.
            steer: controls.steer,
            lean: controls.lean,
            // Reflex backs up on its own button; ours is the brake held at
            // a standstill, as on the other models.
            reverse: controls.brake > 0.3 && self.state.forward_speed < 0.5,
            clutch: controls.clutch,
            ..Default::default()
        };
        self.set_surface();
        let ground = Ground {
            world,
            scratch: RefCell::new((Vec::new(), Vec::new())),
        };
        self.bike.step(dt, &input, &ground, &mut self.state);
        self.ground = [0, 1].map(|i| {
            let w = &self.state.wheels[i];
            w.contact.then(|| world.surface_at(w.point))
        });
        // The rider's weight follows the stick as the old model's did.
        let k = 1.0 - (-8.0 * dt).exp();
        bike.lean += (controls.lean - bike.lean) * k;
        self.write(bike);
    }

    /// Reflex's surface indices on both tyres: the mean of what the wheels
    /// touched last step, as Reflex averages its two samples (`surface.rs`).
    /// In the air they keep what they had.
    fn set_surface(&mut self) {
        let touched: Vec<Surface> = self.ground.iter().flatten().copied().collect();
        if touched.is_empty() {
            return;
        }
        let n = touched.len() as f64;
        let grip = touched.iter().map(|s| s.grip_index()).sum::<f64>() / n;
        let drag = touched.iter().map(|s| s.drag_index()).sum::<f64>() / n;
        for tyre in self.bike.sim_mut().tyres.iter_mut() {
            tyre.grip_index = grip;
            tyre.drag_index = drag;
        }
    }

    /// The simulation's state, as the rest of the game reads a `Bike`.
    fn write(&mut self, bike: &mut Bike) {
        let s = &self.state;
        let chassis = self.bike.chassis_transform();
        let rotation = (chassis.rotation * flip()).normalize();
        let root = chassis.translation - chassis.rotation * pos_to_bevy(self.bike.sim().geometry.cm_offset);
        bike.position = root + rotation * (Vec3::Y * FRAME_LIFT);
        bike.rotation = rotation;
        bike.velocity = s.velocity;
        bike.spin = s.angular_velocity;
        // Ours turns positive to the left, Reflex's to the right.
        bike.steer = -s.steer_angle;
        bike.wheel_spin = s.wheels[1].spin;
        bike.engine.rpm = s.rpm;
        // The engine sound picks its takes by the throttle the engine gets.
        bike.set_gas(self.bike.sim().ctl.throttle as f32);
        bike.engine.gear = s.gear.saturating_sub(1) as usize;
        bike.engine.locked = true;
        bike.crashed = s.crashed.then(|| self.bike.sim().wreck.wreck_timer.max(0.0) as f32);
        // Ours are rear, front; Reflex's front, rear.
        let into_bike = rotation.inverse();
        for (ours, theirs, leg, part) in [
            (0, 1, rear_leg(), reflex_bevy::Part::RearWheel),
            (1, 0, front_leg(), reflex_bevy::Part::FrontWheel),
        ] {
            let w = &s.wheels[theirs];
            let axle = into_bike * (self.bike.part_transform(part).translation - bike.position);
            let wheel = &mut bike.wheels[ours];
            wheel.compression = (axle.y - leg.extended.y).max(0.0);
            wheel.sliding = w.contact && !(w.side_grip && w.roll_grip);
            wheel.contact = w.contact.then(|| Hit {
                distance: 0.0,
                point: w.point,
                normal: w.normal,
                surface: self.ground[theirs].unwrap_or_default(),
            });
        }
        self.written = (bike.position, bike.rotation, bike.velocity);
    }

    /// What the simulation did last frame (speed, rpm, gear, wheels).
    pub fn state(&self) -> &ReflexBikeState {
        &self.state
    }

    /// The engine's idle and limiter, rpm.
    pub fn rpm_range(&self) -> (f32, f32) {
        let p = &self.bike.sim().params;
        let rpm = |w: f64| (w * reflex_core::RPM_PER_RAD_S) as f32;
        (rpm(p.engine_min_speed), rpm(p.engine_max_speed))
    }

    /// The forward speed Reflex reports, m/s.
    pub fn forward_speed(&self) -> f32 {
        self.state.forward_speed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floor() -> World {
        let s = 400.0;
        World::new([
            ([Vec3::new(-s, 0.0, -s), Vec3::new(-s, 0.0, s), Vec3::new(s, 0.0, s)], false),
            ([Vec3::new(-s, 0.0, -s), Vec3::new(s, 0.0, s), Vec3::new(s, 0.0, -s)], false),
        ])
    }

    fn exact_bike(world: &World) -> Option<Bike> {
        vehicle()?;
        let bike = Bike::dropped(world, Vec3::new(0.0, 2.0, 0.0), 0.0).with_exact(true);
        assert!(bike.exact().is_some(), "Reflex physics is on but the bike rides without it");
        Some(bike)
    }

    fn ride(world: &World, bike: &mut Bike, c: Controls, seconds: f32) {
        for _ in 0..(seconds * 60.0) as usize {
            bike.step(world, c, 1.0 / 60.0);
        }
    }

    /// A flat floor made all of `surface`.
    fn floor_of(surface: Surface) -> World {
        let s = 400.0;
        World::with_surfaces([
            ([Vec3::new(-s, 0.0, -s), Vec3::new(-s, 0.0, s), Vec3::new(s, 0.0, s)], false, u32::MAX, surface),
            ([Vec3::new(-s, 0.0, -s), Vec3::new(s, 0.0, s), Vec3::new(s, 0.0, -s)], false, u32::MAX, surface),
        ])
    }

    /// Full gas for 4 s from rest on a floor of each kind: Reflex's tyres
    /// get that ground's grip and drag indices, and mud holds the bike back.
    /// Also the arcade model: mud slows it, ice lets it spin up less.
    #[test]
    fn the_ground_under_the_wheels_sets_grip_and_drag() {
        let gas = Controls { throttle: 1.0, ..Default::default() };
        let reflex = |surface: Surface| {
            let world = floor_of(surface);
            let mut bike = exact_bike(&world)?;
            ride(&world, &mut bike, Controls::default(), 1.0);
            ride(&world, &mut bike, gas, 4.0);
            let tyre = &bike.exact().unwrap().bike.sim().tyres[1];
            assert_eq!((tyre.grip_index, tyre.drag_index), (surface.grip_index(), surface.drag_index()), "{surface:?}");
            Some(bike.speed())
        };
        let arcade = |surface: Surface| {
            let world = floor_of(surface);
            let mut bike = Bike::dropped(&world, Vec3::new(0.0, 2.0, -350.0), 0.0).with_exact(false);
            ride(&world, &mut bike, Controls::default(), 1.0);
            ride(&world, &mut bike, gas, 4.0);
            bike.speed()
        };
        let (dirt, mud, ice) = (arcade(Surface::Dirt), arcade(Surface::Mud), arcade(Surface::Ice));
        eprintln!("arcade after 4 s: dirt {dirt:.1} m/s, mud {mud:.1}, ice {ice:.1}");
        assert!(mud < dirt * 0.97, "arcade: mud {mud} dirt {dirt}");
        assert!(ice <= dirt, "arcade: ice {ice} dirt {dirt}");
        let Some(dirt) = reflex(Surface::Dirt) else {
            eprintln!("skipped the Reflex part: Reflex is not set up");
            return;
        };
        let (mud, stone) = (reflex(Surface::Mud).unwrap(), reflex(Surface::Stone).unwrap());
        eprintln!("Reflex after 4 s: dirt {dirt:.1} m/s, mud {mud:.1}, stone {stone:.1}");
        assert!(mud < dirt * 0.97, "Reflex: mud {mud} dirt {dirt}");
        assert!(stone >= dirt * 0.97, "Reflex: stone {stone} dirt {dirt}");
    }

    /// Full gas from a standstill on a long floor: 0-60 and 40-70 mph times, top speed (mph).
    fn run(tune: Tune, lean_forward: f32) -> Option<(f32, f32, f32)> {
        let s = 3000.0;
        let world = World::new([
            ([Vec3::new(-s, 0.0, -s), Vec3::new(-s, 0.0, s), Vec3::new(s, 0.0, s)], false),
            ([Vec3::new(-s, 0.0, -s), Vec3::new(s, 0.0, s), Vec3::new(s, 0.0, -s)], false),
        ]);
        let mut bike = Bike::dropped(&world, Vec3::new(0.0, 2.0, -2900.0), 0.0);
        bike.exact = Some(Box::new(Exact::tuned(&bike, tune)?));
        ride(&world, &mut bike, Controls::default(), 1.0);
        let gas = Controls { throttle: 1.0, lean: bevy::math::Vec2::new(0.0, lean_forward), ..Default::default() };
        let (mut t40, mut t60, mut t70, mut top, mut pitch) = (f32::NAN, f32::NAN, f32::NAN, 0.0f32, 0.0f32);
        for i in 0..(40 * 60) {
            bike.step(&world, gas, 1.0 / 60.0);
            let mph = bike.speed() / 0.44704;
            if t40.is_nan() && mph >= 40.0 {
                t40 = i as f32 / 60.0;
            }
            if t60.is_nan() && mph >= 60.0 {
                t60 = i as f32 / 60.0;
            }
            pitch = pitch.max(bike.forward().y.asin().to_degrees());
            if bike.crashed.is_some() {
                eprintln!("  crashed at {:.2} s", i as f32 / 60.0);
                return Some((f32::NAN, f32::NAN, top));
            }
            if t70.is_nan() && mph >= 70.0 {
                t70 = i as f32 / 60.0;
            }
            top = top.max(mph);
        }
        eprintln!("  highest nose {pitch:.0} deg");
        Some((t60, t70 - t40, top))
    }

    /// 0-60 mph on the arcade model (`physics.rs`) with a tuning, s.
    fn arcade_0_60(t: &crate::reflex::Tuning) -> f32 {
        crate::physics::set_tuning(t);
        let world = floor();
        let mut bike = Bike::dropped(&world, Vec3::new(0.0, 2.0, -350.0), 0.0).with_exact(false);
        ride(&world, &mut bike, Controls::default(), 1.0);
        let gas = Controls { throttle: 1.0, ..Default::default() };
        for i in 0..(15 * 60) {
            bike.step(&world, gas, 1.0 / 60.0);
            if bike.speed() / 0.44704 >= 60.0 {
                return i as f32 / 60.0;
            }
        }
        f32::NAN
    }

    #[test]
    #[ignore = "prints numbers for tuning; run with --ignored --nocapture"]
    fn launch_trace() {
        let stock = crate::reflex::Tuning::default();
        let user = crate::reflex::Tuning { power: 2.5, grip: 2.2, ..stock };
        eprintln!("arcade 0-60: stock {:.2} s, power 2.5 grip 2.2 {:.2} s", arcade_0_60(&stock), arcade_0_60(&user));
        crate::physics::set_tuning(&stock);
        let world = floor();
        let mut bike = Bike::dropped(&world, Vec3::new(0.0, 2.0, -350.0), 0.0);
        bike.exact = Some(Box::new(Exact::tuned(&bike, Tune { power: 1.35, gearing: 0.87, traction: 1.0 }).unwrap()));
        ride(&world, &mut bike, Controls::default(), 1.0);
        let gas = Controls { throttle: 1.0, ..Default::default() };
        for i in 0..(5 * 60) {
            bike.step(&world, gas, 1.0 / 60.0);
            if i % 10 == 0 {
                let e = bike.exact().unwrap();
                let s = e.state();
                let sim = e.bike.sim();
                eprintln!(
                    "{:4.2} s {:5.1} mph gear {} {:5.0} rpm pitch {:5.1} deg front {} rear slip {:6.2} grip {} clutch {:.2} throttle {:.2}",
                    i as f32 / 60.0,
                    bike.speed() / 0.44704,
                    s.gear,
                    s.rpm,
                    bike.forward().y.asin().to_degrees(),
                    s.wheels[0].contact,
                    s.wheels[1].slip_longitudinal,
                    s.wheels[1].roll_grip,
                    sim.drive.clutch_timer,
                    sim.ctl.throttle,
                );
            }
        }
    }

    #[test]
    #[ignore = "prints numbers for tuning; run with --ignored --nocapture"]
    fn speed_table() {
        for (power, gearing, traction) in [
            (1.0, 1.0, 1.0),
            (1.35, 0.87, 1.0),
            (1.35, 0.87, 1.25),
            (1.35, 0.87, 1.5),
            (1.35, 0.87, 2.0),
            (1.6, 0.87, 2.0),
            (2.0, 0.87, 2.5),
        ] {
            for lean in [0.0, 1.0] {
                if let Some((t60, t, top)) = run(Tune { power, gearing, traction }, lean) {
                    eprintln!("power {power:.2} gearing {gearing:.2} traction {traction:.2} lean {lean}: 0-60 {t60:.2} s, 40-70 {t:.2} s, top {top:.1} mph");
                }
            }
        }
    }

    #[test]
    fn settles_upright_on_the_floor() {
        let world = floor();
        let Some(mut bike) = exact_bike(&world) else {
            eprintln!("no Reflex install; skipped");
            return;
        };
        ride(&world, &mut bike, Controls::default(), 3.0);
        let root = bike.to_world(Vec3::NEG_Y * FRAME_LIFT);
        eprintln!(
            "at rest: model origin {:.3} m up, compression rear {:.3} front {:.3}, up {:.3?}",
            root.y, bike.wheels[0].compression, bike.wheels[1].compression, bike.up()
        );
        assert!(bike.up().y > 0.99, "up {:?}", bike.up());
        assert_eq!(bike.grounded(), 2);
        assert!(bike.velocity.length() < 0.2, "still moving at {:?}", bike.velocity);
        assert!(bike.crashed.is_none());
        // The model's origin sits about 0.6 m over the ground, as on the old model.
        let root = bike.to_world(Vec3::NEG_Y * FRAME_LIFT);
        assert!((0.4..0.8).contains(&root.y), "model origin at {:.3} m", root.y);
    }

    #[test]
    fn rides_forward_along_the_heading() {
        let world = floor();
        let Some(mut bike) = exact_bike(&world) else {
            eprintln!("no Reflex install; skipped");
            return;
        };
        ride(&world, &mut bike, Controls::default(), 1.0);
        let start = bike.position;
        let gas = Controls {
            throttle: 1.0,
            ..Default::default()
        };
        ride(&world, &mut bike, gas, 4.0);
        let moved = bike.position - start;
        eprintln!(
            "4 s full gas: moved {moved:.2?}, {:.1} m/s, gear {}, {:.0} rpm",
            bike.speed(),
            bike.engine.gear + 1,
            bike.engine.rpm
        );
        // Heading 0 faces +Z.
        assert!(moved.z > 20.0, "moved {moved:?}");
        assert!(moved.x.abs() < moved.z * 0.2, "drifted {moved:?}");
        assert!(bike.speed() > 10.0, "speed {:.1}", bike.speed());
        assert!(bike.engine.gear >= 1, "still in first");
        // The engine sound hears the gas.
        assert!(bike.gas() > 0.9, "gas {:.2}", bike.gas());
        assert!(bike.up().y > 0.9);
    }

    #[test]
    fn steers_the_way_the_stick_says() {
        let world = floor();
        let Some(mut bike) = exact_bike(&world) else {
            eprintln!("no Reflex install; skipped");
            return;
        };
        ride(&world, &mut bike, Controls::default(), 1.0);
        let gas = Controls {
            throttle: 0.6,
            ..Default::default()
        };
        ride(&world, &mut bike, gas, 2.0);
        let before = bike.heading();
        let right = Controls {
            throttle: 0.6,
            steer: 1.0,
            ..Default::default()
        };
        ride(&world, &mut bike, right, 1.0);
        // Facing +Z with +Y up, the rider's right is -X: the heading
        // (atan2(x, z)) falls turning right.
        let turned = bike.heading() - before;
        assert!(turned < -0.2, "turned {turned:.2} rad");
    }

    #[test]
    fn a_launch_speed_carries_over() {
        let world = floor();
        let Some(mut bike) = exact_bike(&world) else {
            eprintln!("no Reflex install; skipped");
            return;
        };
        bike.velocity = bike.forward() * 12.0;
        bike.step(&world, Controls::default(), 1.0 / 60.0);
        assert!(bike.speed() > 10.0, "speed {:.1}", bike.speed());
    }
}
