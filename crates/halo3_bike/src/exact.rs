//! "Reflex": the bike on MX vs ATV Reflex's own simulation, not a model
//! with Reflex's numbers. `reflex_core` (vendored in
//! `third_party/reflex-physics`) reimplements the game's four-body bike
//! step by step and was checked frame by frame against recordings of the
//! game; `reflex_bevy` puts it in Bevy's frame. The Dakumoto 450F's values
//! (`mx450f_vehicle`) are read from the player's own Reflex install
//! (`reflex_assets::vehicle`), so without Reflex this model is not offered
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
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use bevy::log::{info, warn};
use bevy::math::{Quat, Vec3};
use reflex_bevy::convert::{dir_to_reflex, pos_to_bevy, pos_to_reflex, quat_to_reflex};
use reflex_bevy::ground::reflex_triangle;
use reflex_bevy::reflex_core::bike::{FRONT, REAR};
use reflex_bevy::reflex_core::{self, Vehicle};
use reflex_bevy::{ReflexBike, ReflexBikeState, ReflexInput};

use super::physics::{Bike, Controls, FRAME_LIFT, Hit, World, front_leg, rear_leg};

/// The bike class read from Reflex: the Dakumoto 450F the model draws.
const CLASS: &str = "mx450f";

static ON: AtomicBool = AtomicBool::new(false);
/// Counts the switches, so a bike sees when the choice changed under it.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Rides bikes on Reflex's simulation (when the vehicle values could be
/// read); a bike already out switches at its next step.
pub fn set_on(on: bool) {
    if ON.swap(on, Ordering::Relaxed) != on {
        GENERATION.fetch_add(1, Ordering::Relaxed);
    }
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

/// The 450F's values from the player's Reflex install, read once.
/// `HALO3_REFLEX_VEHICLE` (a developer override) names a vehicle dump file
/// instead (the stock JSON or XML dumps `reflex_core` reads).
pub fn vehicle() -> Option<&'static Vehicle> {
    static VEHICLE: OnceLock<Option<Vehicle>> = OnceLock::new();
    VEHICLE.get_or_init(load).as_ref()
}

fn load() -> Option<Vehicle> {
    if let Some(path) = std::env::var_os("HALO3_REFLEX_VEHICLE") {
        return Vehicle::load(&path)
            .inspect_err(|e| warn!("bike: HALO3_REFLEX_VEHICLE: {e}"))
            .ok();
    }
    let root = reflex_assets::find_reflex()?;
    let xml = reflex_assets::vehicle::xml(&root, CLASS)
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
        let vehicle = vehicle()?.clone();
        Some(Exact {
            bike: ReflexBike::new(vehicle),
            state: ReflexBikeState::default(),
            placed: false,
            written: (bike.position, bike.rotation, bike.velocity),
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
            ..Default::default()
        };
        let ground = Ground {
            world,
            scratch: RefCell::new((Vec::new(), Vec::new())),
        };
        self.bike.step(dt, &input, &ground, &mut self.state);
        // The rider's weight follows the stick as the old model's did.
        let k = 1.0 - (-8.0 * dt).exp();
        bike.lean += (controls.lean - bike.lean) * k;
        self.write(bike);
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
            });
        }
        self.written = (bike.position, bike.rotation, bike.velocity);
    }

    /// What the simulation did last frame (speed, rpm, gear, wheels).
    pub fn state(&self) -> &ReflexBikeState {
        &self.state
    }

    /// A hard landing or a big pitch / yaw rate opened Reflex's recovery
    /// window: the right-stick flick that saves it and the seconds left.
    /// Not flicked in time, the bike wrecks.
    pub fn save_prompt(&self) -> Option<(reflex_core::wreck::Save, f32)> {
        self.bike.sim().wreck.save_prompt().map(|(s, t)| (s, t as f32))
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
