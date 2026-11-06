//! The dirt bike: dual-stick motocross riding on the map's collision, with
//! the Spartan standing on it (`physics.rs` for the bike, `pose.rs` for the
//! rider). The bike drawn is Reflex's Dakumoto 450F in the Cortana kit,
//! read from the user's MX vs ATV Reflex install (`model.rs`); without
//! Reflex it is our own stand-in motocrosser (`stand_in.rs`). Until either
//! is built it is a stick model: thin unlit cylinders.
//!
//! B (or the controller's View / Back button) drops the bike under the
//! camera and starts riding; B again goes back to flying.
//!
//! | controller | keyboard | |
//! |---|---|---|
//! | left stick X | A / D | handlebars |
//! | right stick | arrow keys | rider: forward, back, left, right |
//! | RT | W | throttle |
//! | LT | S | brake; held at a standstill, back up to 10 mph |
//! | Y | R | stand the bike back up |
//! | d-pad up | G | jump off on foot, keeping the bike's speed (`mcc_foot`) |
//! | both sticks clicked | J | onto the skateboard, keeping the bike's speed (`mcc_skate`) |
//! | LB + d-pad down | | set the session marker (`marker.rs`) |
//! | LB + d-pad up (hold) | | back to the session marker |
//! | left stick click | H | engine sound on / off (`mcc_audio::engine`) |
//! | | 0 | physics tuning menu (`tune.rs`) |
//!
//! A crash shows CRASHED in orange in the lower left until the bike is
//! put back.
//!
//! Falling into a kill volume or out of the world (`falls.rs`: Guardian's
//! abyss) puts the bike back where it last rode well, as a crash does.
//!
//! `--shot DIR --bike` rides from the first view (or the start spawn) for a
//! couple of seconds, then takes the rider pictures; `MCC_BIKE_THROTTLE`
//! sets the throttle it holds meanwhile (default 0.4).

pub mod exact;
pub mod marker;
pub mod model;
pub mod physics;
pub mod pose;
pub mod reflex;
pub mod stand_in;
pub mod tune;
pub mod unleashed;

use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;
use mcc_core::map::{MapAppExt, Warmup};
use bevy::window::PrimaryWindow;

use blam_geometry::Collision;
use blam_geometry::collision::surface_flags;

use mcc_core::coords::{to_halo, to_render, to_render_dir};
use mcc_core::flag::SessionFlag;
use mcc_core::fly_camera::FlyCamera;
use mcc_core::skate::{
    Bail, Dismount, Mount, Ride, RiderPose, SkateEnabled, Skating, assets_root, bail_pressed, board_pressed, skating_on,
};
use mcc_core::zones::LoadedBsps;

use physics::{Bike, Controls, World};

/// The map's collision (Halo units), handed over by `main`.
#[derive(Resource)]
pub struct BikeInput(pub Arc<Collision>);

/// The collision [`World`] of the loaded BSPs, shared by the bike, the
/// Spartan on foot and the enemies. Built on a thread as soon as the map is
/// in (behind the menu's loading screen, [`Warmup`]) and again when the
/// zone set changes, so getting on the bike does not stop the game.
#[derive(Resource, Default)]
pub struct SharedWorld {
    built: Option<(u32, Arc<World>)>,
    job: Option<(u32, Mutex<Receiver<World>>)>,
}

/// The [`Warmup`] name of the shared world's first build.
const WARMUP: &str = "bike collision";

impl SharedWorld {
    /// The world for `mask` when it is built.
    pub fn ready(&self, mask: u32) -> Option<Arc<World>> {
        self.built.as_ref().filter(|(m, _)| *m == mask).map(|(_, w)| Arc::clone(w))
    }

    /// The world for `mask`: built, finishing the background build of it,
    /// or (no build of it running) built here and now.
    pub fn wait(&mut self, collision: &Collision, mask: u32) -> Arc<World> {
        if let Some(w) = self.ready(mask) {
            return w;
        }
        if let Some((m, job)) = self.job.take() {
            if m == mask
                && let Ok(w) = job.into_inner().unwrap_or_else(|e| e.into_inner()).recv()
            {
                return self.keep(mask, w);
            }
        }
        let w = build_world(collision, mask);
        self.keep(mask, w)
    }

    fn keep(&mut self, mask: u32, world: World) -> Arc<World> {
        let w = Arc::new(world);
        self.built = Some((mask, Arc::clone(&w)));
        w
    }

    /// Starts building `mask` on a thread unless it is built or building.
    fn request(&mut self, collision: &Arc<Collision>, mask: u32) {
        if self.ready(mask).is_some() || self.job.as_ref().is_some_and(|(m, _)| *m == mask) {
            return;
        }
        let (send, receive) = std::sync::mpsc::channel();
        let collision = Arc::clone(collision);
        let spawned = std::thread::Builder::new()
            .name("mcc-bike-world".into())
            .spawn(move || {
                let _ = send.send(build_world(&collision, mask));
            });
        match spawned {
            Ok(_) => self.job = Some((mask, Mutex::new(receive))),
            Err(e) => warn!("bike: collision thread: {e}"),
        }
    }

    /// Takes in a finished background build.
    fn poll(&mut self) {
        let Some((mask, job)) = self.job.as_ref() else {
            return;
        };
        let mask = *mask;
        let received = job.lock().unwrap_or_else(|e| e.into_inner()).try_recv();
        match received {
            Ok(w) => {
                self.job = None;
                self.keep(mask, w);
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                warn!("bike: the collision build for BSP mask {mask:#x} died");
                self.job = None;
            }
        }
    }
}

/// Keeps the shared world on the loaded zone set, building in the
/// background; holds the loading screen until the first build is in.
fn warm_world(
    mut shared: ResMut<SharedWorld>,
    mut warmup: ResMut<Warmup>,
    input: Option<Res<BikeInput>>,
    loaded: Res<LoadedBsps>,
) {
    let Some(input) = input else {
        warmup.0.remove(WARMUP);
        return;
    };
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    shared.poll();
    if shared.built.is_none() && shared.job.is_none() {
        warmup.0.insert(WARMUP);
    }
    shared.request(&input.0, mask);
    if shared.built.is_some() || shared.job.is_none() {
        warmup.0.remove(WARMUP);
    }
}

/// Start riding at once at a Halo position (world units) facing a Halo yaw
/// (radians). `at`: holding a steady throttle (screenshot mode);
/// `player`: under the player's control (an insertion's start).
#[derive(Resource, Default)]
pub struct AutoStart(pub Option<(Vec3, f32)>, pub bool);

impl AutoStart {
    pub fn at(position: Vec3, yaw: f32) -> Self {
        AutoStart(Some((position, yaw)), true)
    }

    pub fn player(position: Vec3, yaw: f32) -> Self {
        AutoStart(Some((position, yaw)), false)
    }
}

pub struct BikePlugin;

/// The bike's systems (riding, drawing, publishing [`BikeMotion`]); the AI
/// reads the rider after them.
///
/// [`BikeMotion`]: mcc_core::bike::BikeMotion
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct BikeSet;

impl Plugin for BikePlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Riding>()
            .init_map_resource::<AutoStart>()
            .init_map_resource::<StickPool>()
            .init_map_resource::<model::BikeMesh>()
            .init_map_resource::<Dismount>()
            .init_map_resource::<Mount>()
            .init_map_resource::<SessionFlag>()
            .init_map_resource::<mcc_core::bike::BikeMotion>()
            .add_plugins(tune::TunePlugin)
            .init_map_resource::<SharedWorld>()
            .init_map_resource::<mcc_core::ai::PlayerRespawn>()
            .add_systems(mcc_core::map::MapStartup, (setup, model::start, spawn_crashed_text, warm_world))
            .add_systems(
                Update,
                (
                    warm_world,
                    ride.run_if(mcc_core::pause::running).run_if(tune::closed),
                    model::build,
                    draw_bike,
                    model::place,
                    model::light,
                    show_status,
                    show_crashed,
                    publish_motion,
                )
                    .chain()
                    .in_set(BikeSet)
                    .run_if(mcc_core::map::ready),
            );
    }
}

#[derive(Resource, Default)]
struct Riding {
    world: Option<Arc<World>>,
    /// The BSPs `world` was built from.
    mask: Option<u32>,
    bike: Option<Bike>,
    rig: Option<pose::Rig>,
    /// Holding the throttle with the sticks at rest (screenshot mode).
    neutral: Option<f32>,
    /// Left and right trigger axes seen at rest (see `trigger`).
    trigger_axes: [bool; 2],
    /// Chase camera: where it is and the way it looks along the ground.
    eye: Option<Vec3>,
    look: Vec3,
    /// Kept while off the bike, as the skate's is.
    marker: marker::SessionMarker,
}

/// The bike's state for the engine sound (`mcc_audio`).
fn publish_motion(riding: Res<Riding>, mut motion: ResMut<mcc_core::bike::BikeMotion>) {
    let m = match riding.bike.as_ref() {
        Some(b) => {
            let s = physics::setup();
            let (min_rpm, max_rpm) = b.exact().map_or((s.min_rpm, s.max_rpm()), |e| e.rpm_range());
            mcc_core::bike::BikeMotion {
                riding: true,
                rpm: b.engine.rpm,
                min_rpm,
                max_rpm,
                throttle: b.gas(),
                speed: b.velocity.length(),
                gear: b.engine.gear,
                grounded: b.grounded(),
                crashed: b.crashed.is_some(),
                clutch_slipping: !b.engine.locked,
                rear_sliding: b.wheels[0].sliding && b.wheels[0].contact.is_some(),
                position: b.position,
                velocity: b.velocity,
                heading: b.heading(),
                upright: b.crashed.is_none() && b.grounded() == 2 && b.up().y > 0.8,
            }
        }
        None => mcc_core::bike::BikeMotion::default(),
    };
    *motion = m;
}

/// The CRASHED text in the lower left, shown while the bike is down.
#[derive(Component)]
struct CrashedText;

const CRASHED_ORANGE: Color = Color::srgb(1.0, 0.55, 0.1);

fn spawn_crashed_text(mut commands: Commands) {
    commands.spawn((
        CrashedText,
        Text::new("CRASHED"),
        TextFont::from_font_size(32.0),
        TextColor(CRASHED_ORANGE),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(24.0),
            bottom: Val::Px(40.0),
            ..default()
        },
        Visibility::Hidden,
    ));
}

fn show_crashed(riding: Res<Riding>, mut text: Query<&mut Visibility, With<CrashedText>>) {
    let crashed = riding.bike.as_ref().is_some_and(|b| b.crashed.is_some());
    let want = if crashed { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut text {
        v.set_if_neq(want);
    }
}

/// The skate scale `k` the rider is drawn at (`skate.rs`).
fn scale(pose: &RiderPose) -> f32 {
    pose.scale
}

fn setup(
    mut riding: ResMut<Riding>,
    mut pool: ResMut<StickPool>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Reflex's 450F physics values, read before the first ride needs them.
    std::thread::spawn(|| {
        exact::vehicle();
    });
    pool.mesh = meshes.add(Cylinder::new(1.0, 1.0));
    pool.materials = COLOURS
        .iter()
        .map(|&(r, g, b, _)| {
            materials.add(StandardMaterial {
                base_color: Color::srgb(r, g, b),
                unlit: true,
                ..default()
            })
        })
        .collect();
    match pose::Rig::load(&assets_root().join("rig.json")) {
        Ok(r) => riding.rig = Some(r),
        Err(e) => warn!("bike: {e}; the rider is left off"),
    }
}

/// The collision of the loaded BSPs, renderer frame.
pub fn build_world(collision: &Collision, mask: u32) -> World {
    let started = std::time::Instant::now();
    let loaded = collision.of_bsps(mask);
    let world = World::with_owners(loaded.triangles.iter().map(|t| {
        (
            t.points.map(to_render),
            t.flags & surface_flags::TWO_SIDED != 0,
            t.source.object().unwrap_or(u32::MAX),
        )
    }));
    eprintln!(
        "bike: {} collision triangles ready in {:.2} s",
        world.len(),
        started.elapsed().as_secs_f32()
    );
    world
}

/// Halo yaw (radians, in the XY plane) to a heading about the renderer's +Y.
pub fn heading_of_halo_yaw(yaw: f32) -> f32 {
    let d = to_render_dir(Vec3::new(yaw.cos(), yaw.sin(), 0.0));
    d.x.atan2(d.z)
}

/// A radial deadzone, rescaled so the stick still reaches 1.
fn dead(v: Vec2, zone: f32) -> Vec2 {
    let len = v.length();
    if len < zone {
        Vec2::ZERO
    } else {
        v / len * ((len.min(1.0) - zone) / (1.0 - zone))
    }
}

/// How far a trigger is pulled, 0 to 1. XInput pads send the travel on the
/// trigger button. Pads gilrs only knows by its default mapping (DualShock,
/// many third-party pads) send the button as on/off and the travel on the
/// Z axis, -1 at rest to 1 pulled in. bevy reads an axis that has sent
/// nothing yet as 0, half travel, so the axis only counts once it has been
/// seen at rest.
fn trigger(pad: &Gamepad, button: B, axis: GamepadAxis, rested: &mut bool) -> f32 {
    let pulled = pad.get(button).unwrap_or(if pad.pressed(button) { 1.0 } else { 0.0 });
    let mut travel = 0.0;
    if let Some(v) = pad.get(axis) {
        *rested |= v < -0.9;
        if *rested {
            travel = (v + 1.0) * 0.5;
        }
    }
    pulled.max(travel).clamp(0.0, 1.0)
}

/// The trigger-to-throttle curve. Held throttle settles the bike at a speed,
/// and a quarter throttle already rides at half the top speed, so a straight
/// line puts nearly all the control in the first bit of the pull. Squared,
/// half trigger is a quarter throttle and the travel spreads over the speeds.
const THROTTLE_CURVE: f32 = 2.0;

fn read_controls(pad: Option<&Gamepad>, keys: &ButtonInput<KeyCode>, rested: &mut [bool; 2]) -> Controls {
    let key = |k: KeyCode| keys.pressed(k) as i32 as f32;
    let mut c = Controls {
        steer: key(KeyCode::KeyD) - key(KeyCode::KeyA),
        lean: Vec2::new(
            key(KeyCode::ArrowRight) - key(KeyCode::ArrowLeft),
            key(KeyCode::ArrowUp) - key(KeyCode::ArrowDown),
        ),
        throttle: key(KeyCode::KeyW),
        brake: key(KeyCode::KeyS),
    };
    if let Some(pad) = pad {
        let l = dead(pad.left_stick(), 0.12);
        let r = dead(pad.right_stick(), 0.15);
        c.steer = (c.steer + l.x).clamp(-1.0, 1.0);
        c.lean = (c.lean + r).clamp_length_max(1.0);
        let rt = trigger(pad, B::RightTrigger2, GamepadAxis::RightZ, &mut rested[1]);
        c.throttle = c.throttle.max(rt.powf(THROTTLE_CURVE));
        c.brake = c.brake.max(trigger(pad, B::LeftTrigger2, GamepadAxis::LeftZ, &mut rested[0]));
    }
    c
}

#[allow(clippy::too_many_arguments)]
fn ride(
    mut commands: Commands,
    mut riding: ResMut<Riding>,
    mut auto: ResMut<AutoStart>,
    (mut dismount, mut mount): (ResMut<Dismount>, ResMut<Mount>),
    (mut flag, skate_on): (ResMut<SessionFlag>, Option<Res<SkateEnabled>>),
    input: Option<Res<BikeInput>>,
    mut shared: ResMut<SharedWorld>,
    kill: Option<Res<mcc_core::falls::KillZones>>,
    lifts: Option<Res<mcc_core::lifts::Lifts>>,
    fallback: Option<Res<mcc_core::falls::FallbackSpot>>,
    (mut rider, mut player): (ResMut<RiderPose>, ResMut<mcc_core::ai::PlayerRespawn>),
    loaded: Res<LoadedBsps>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    mut cameras: Query<(Entity, &mut Transform, &mut FlyCamera, &mut Projection)>,
) {
    let Some(input) = input else {
        return;
    };
    let riding = &mut *riding;
    let pad = gamepads.iter().next();
    let toggle = keys.just_pressed(KeyCode::KeyB) || pad.is_some_and(|p| p.just_pressed(B::Select));
    let Ok((camera, mut transform, mut fly, mut projection)) = cameras.single_mut() else {
        return;
    };

    // The world follows the loaded zone set.
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    let mounting = matches!(mount.0, Some((Ride::Bike, _)));
    let wanted = riding.bike.is_some() || toggle || auto.0.is_some() || mounting;
    if wanted && riding.mask != Some(mask) {
        riding.world = Some(shared.wait(&input.0, mask));
        riding.mask = Some(mask);
    }
    let Some(world) = riding.world.as_ref() else {
        return;
    };

    if let Some((position, yaw)) = auto.0.take() {
        riding.bike = Some(Bike::dropped(world, to_render(position) + Vec3::Y, heading_of_halo_yaw(yaw)));
        riding.neutral = auto.1.then(|| {
            std::env::var("MCC_BIKE_THROTTLE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.4)
        });
        riding.eye = None;
        rider.skated = 0.0;
        rider.bike = true;
        commands.entity(camera).insert(Skating);
    } else if mounting {
        // Straight from walking or off the board: the bike under him, with
        // his momentum.
        let Some((_, bail)) = mount.0.take() else {
            return;
        };
        let flat = Vec3::new(bail.velocity.x, 0.0, bail.velocity.z);
        let heading = if flat.length() > 1.0 {
            flat.x.atan2(flat.z)
        } else {
            bail.yaw + std::f32::consts::PI
        };
        let mut bike = Bike::dropped(world, bail.position + Vec3::Y, heading);
        // In the air (a jump, a fall): where he is, not on the ground below.
        if bike.position.y < bail.position.y - 0.5 {
            bike = Bike::new(bail.position + Vec3::Y * physics::rest_height(), heading);
        }
        bike.velocity = bail.velocity;
        bike.wheel_spin = bail.velocity.dot(bike.forward()) / physics::rear_leg().radius;
        riding.bike = Some(bike);
        riding.neutral = None;
        riding.eye = None;
        riding.look = Vec3::ZERO;
        rider.skated = 0.0;
        rider.bike = true;
        riding.marker.suspend();
        commands.entity(camera).insert(Skating);
    } else if toggle {
        if riding.bike.take().is_some() {
            rider.bike = false;
            rider.active = false;
            rider.bones.clear();
            rider.camera = None;
            commands.entity(camera).remove::<Skating>();
            // Fly on from where the chase camera left off.
            let f = transform.forward().as_vec3();
            fly.yaw = (-f.x).atan2(-f.z);
            fly.pitch = f.y.clamp(-1.0, 1.0).asin();
            transform.rotation = fly.rotation();
            if let Projection::Perspective(lens) = &mut *projection {
                lens.fov = 75f32.to_radians();
            }
            return;
        }
        if rider.active {
            // On foot or on the board, B is theirs to hand over (`Mount`).
            return;
        }
        let f = transform.forward().as_vec3();
        let heading = if Vec2::new(f.x, f.z).length() > 1e-3 { f.x.atan2(f.z) } else { 0.0 };
        riding.bike = Some(Bike::dropped(world, transform.translation, heading));
        riding.neutral = None;
        riding.eye = None;
        rider.skated = 0.0;
        rider.bike = true;
        riding.marker.suspend();
        commands.entity(camera).insert(Skating);
    }
    // Jumping off onto foot, keeping the bike's momentum.
    if riding.neutral.is_none()
        && bail_pressed(&keys, pad)
        && let Some(bike) = riding.bike.take()
    {
        rider.bike = false;
        rider.active = false;
        rider.bones.clear();
        rider.camera = None;
        // The camera keeps `Skating`: the Spartan takes it over (`Dismount`).
        dismount.0 = Some(Bail::new(bike.position, bike.velocity, &transform));
        return;
    }
    // Straight onto the board, keeping the bike's momentum: `mcc_skate`
    // takes the `Mount`.
    if riding.neutral.is_none()
        && skating_on(skate_on.as_deref())
        && board_pressed(&keys, pad)
        && let Some(bike) = riding.bike.take()
    {
        rider.bike = false;
        rider.active = false;
        rider.bones.clear();
        rider.camera = None;
        // The camera keeps `Skating`: the board takes it over (`Mount`).
        mount.0 = Some((Ride::Board, Bail::new(bike.position, bike.velocity, &transform)));
        return;
    }
    let Some(bike) = riding.bike.as_mut() else {
        return;
    };

    let reset = keys.just_pressed(KeyCode::KeyR) || pad.is_some_and(|p| p.just_pressed(B::North));
    if reset {
        bike.reset_here(world);
    }
    // The rider died (`mcc_ai`): the bike rolls on with nobody at the
    // controls, then comes back under him at his last quiet checkpoint.
    if let Some((at, heading)) = player.bike.take() {
        *bike = Bike::dropped(world, at + Vec3::Y, heading);
        riding.eye = None;
        riding.look = Vec3::ZERO;
        info!("bike: back at the checkpoint");
    }
    let controls = match riding.neutral {
        _ if player.dead => Controls::default(),
        Some(throttle) => Controls {
            throttle,
            ..default()
        },
        None => read_controls(pad, &keys, &mut riding.trigger_axes),
    };
    let dt = time.delta_secs().min(0.1);

    // The session marker: LB + d-pad, as on the board.
    let buttons = pad.map_or_else(marker::MarkerButtons::default, |p| marker::MarkerButtons {
        modifier: p.pressed(B::LeftTrigger),
        set: p.just_pressed(B::DPadDown),
        back: p.pressed(B::DPadUp),
    });
    let upright = bike.crashed.is_none() && bike.grounded() == 2 && bike.up().y > 0.71;
    let state = marker::BikeState {
        position: bike.position,
        heading: bike.heading(),
        can_place: upright,
        can_return: bike.crashed.is_none(),
    };
    match riding.marker.update(buttons, state, dt) {
        marker::Action::Placed => {
            // The flag goes in the ground under the bike.
            let ground = world
                .raycast_far(bike.position + Vec3::Y * 0.5, Vec3::NEG_Y, 10.0)
                .map_or(bike.position - Vec3::Y * physics::rest_height(), |h| h.point);
            flag.0 = Some(ground);
            info!("bike: session marker placed");
        }
        marker::Action::Refused => info!("bike: session marker refused (not upright on both wheels)"),
        marker::Action::Return(m) => {
            *bike = Bike::dropped(world, m.position, m.heading);
            riding.eye = None;
            riding.look = Vec3::ZERO;
            info!("bike: back to the session marker");
        }
        marker::Action::None => {}
    }

    bike.step(world, controls, dt);
    // A gravity lift takes the bike too (unless it ignores vehicles): its
    // push on top of the bike's own physics, and the bike's gravity given
    // back while the lift switches gravity off.
    if let Some(p) = lifts.as_deref().and_then(|l| l.push(bike.position, bike.velocity, dt, 0.5, true)) {
        let mut a = p.acceleration;
        if p.negates_gravity {
            a.y += physics::GRAVITY;
        }
        bike.velocity += a * dt;
    }
    if let Some(kill) = kill.as_deref()
        && let Some(volume) = kill.deadly(to_halo(bike.position))
    {
        let at = to_halo(bike.position);
        bike.respawn(world);
        // Never rode well yet: the start spawn.
        if kill.deadly(to_halo(bike.position)).is_some()
            && let Some(f) = fallback.as_deref()
        {
            *bike = Bike::dropped(world, to_render(f.0) + Vec3::Y, heading_of_halo_yaw(f.1));
        }
        let back = to_halo(bike.position);
        info!(
            "fall: bike into {volume} at halo [{:.2}, {:.2}, {:.2}]; back at [{:.2}, {:.2}, {:.2}]",
            at.x, at.y, at.z, back.x, back.y, back.z
        );
        riding.eye = None;
    }
    rider.skated += dt;

    // The rider, as skate bones (skate metres, world): see `pose.rs`.
    let k = scale(&rider);
    if let Some(rig) = riding.rig.as_ref() {
        let contacts = [0, 1].map(|i| {
            let leg = if i == 0 { physics::rear_leg() } else { physics::front_leg() };
            bike.axle(i) - Vec3::Y * leg.radius
        });
        let stance = pose::Stance {
            grips: bike.grips(),
            lean: bike.lean,
            contacts,
        };
        let place = Mat4::from_translation(bike.position * k) * Mat4::from_quat(bike.rotation);
        rider.bones = rig.pose(k, &stance).into_iter().map(|m| place * m).collect();
        if rider.names.len() != rig.names.len() {
            rider.names = rig.names.clone();
        }
        rider.active = true;
    }

    // Chase camera: behind along the way it is going, smoothed.
    let speed = bike.velocity.length();
    let along = if speed > 3.0 { bike.velocity } else { bike.forward() };
    let along = Vec3::new(along.x, 0.0, along.z).normalize_or(Vec3::Z);
    riding.look = if riding.look == Vec3::ZERO {
        along
    } else {
        riding.look.lerp(along, 1.0 - (-3.0 * dt).exp()).normalize_or(along)
    };
    let target = bike.position + Vec3::Y * 1.0;
    let mut wanted_eye = target - riding.look * 5.5 + Vec3::Y * 1.6;
    // Not inside a hill.
    let back = wanted_eye - target;
    if let Some(hit) = world.raycast_far(target, back.normalize_or(Vec3::Y), back.length()) {
        wanted_eye = target + back.normalize_or(Vec3::Y) * (hit.distance - 0.3).max(0.5);
    }
    let eye = match riding.eye {
        Some(e) => e.lerp(wanted_eye, 1.0 - (-8.0 * dt).exp()),
        None => wanted_eye,
    };
    riding.eye = Some(eye);
    let view = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    let fov = 70f32.to_radians();
    rider.camera = Some((view, fov));
    if rider.drive_camera {
        *transform = view;
        if let Projection::Perspective(lens) = &mut *projection {
            lens.fov = fov;
        }
    }
}

/// The stick colours (sRGB) and how thick each kind of stick is (radius, m).
const COLOURS: [(f32, f32, f32, f32); 5] = [
    (1.0, 0.45, 0.05, 0.022), // frame
    (0.75, 0.75, 0.8, 0.016), // metal
    (0.08, 0.08, 0.08, 0.035), // rubber
    (0.1, 0.1, 0.35, 0.04),   // seat
    (0.9, 0.9, 0.2, 0.01),    // contact marks
];

/// The sticks drawn this frame: ends and colour (index into `COLOURS`).
#[derive(Default)]
struct Sticks(Vec<(Vec3, Vec3, usize)>);

impl Sticks {
    fn line(&mut self, a: Vec3, b: Vec3, colour: usize) {
        self.0.push((a, b, colour));
    }
}

/// Cylinder entities reused frame to frame, per colour.
#[derive(Resource, Default)]
struct StickPool {
    mesh: Handle<Mesh>,
    materials: Vec<Handle<StandardMaterial>>,
    entities: Vec<Vec<Entity>>,
}

/// A circle of `segments` lines around `centre` in the plane across `axis`.
fn ring(gizmos: &mut Sticks, centre: Vec3, axis: Vec3, radius: f32, colour: usize) {
    const SEGMENTS: usize = 28;
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let at = |i: usize| {
        let a = i as f32 / SEGMENTS as f32 * std::f32::consts::TAU;
        centre + (u * a.cos() + v * a.sin()) * radius
    };
    for i in 0..SEGMENTS {
        gizmos.line(at(i), at(i + 1), colour);
    }
}

/// The bike as a stick model: wheels, forks and bars, swingarm, frame,
/// seat, pegs.
fn draw_bike(
    mut commands: Commands,
    riding: Res<Riding>,
    mesh: Res<model::BikeMesh>,
    mut pool: ResMut<StickPool>,
    mut sticks: Query<(&mut Transform, &mut Visibility), Without<FlyCamera>>,
) {
    let mut gizmos = Sticks::default();
    if let Some(bike) = riding.bike.as_ref().filter(|_| !mesh.ready()) {
        bike_sticks(bike, &mut gizmos);
    }
    let pool = &mut *pool;
    if pool.materials.is_empty() {
        return;
    }
    pool.entities.resize(COLOURS.len(), Vec::new());
    let mut used = vec![0usize; COLOURS.len()];
    for &(a, b, colour) in &gizmos.0 {
        let d = b - a;
        let len = d.length();
        if len < 1e-4 {
            continue;
        }
        let radius = COLOURS[colour].3;
        let transform = Transform::from_translation((a + b) * 0.5)
            .with_rotation(Quat::from_rotation_arc(Vec3::Y, d / len))
            .with_scale(Vec3::new(radius, len + radius, radius));
        let n = used[colour];
        used[colour] += 1;
        match pool.entities[colour].get(n) {
            Some(&e) => {
                if let Ok((mut t, mut v)) = sticks.get_mut(e) {
                    *t = transform;
                    *v = Visibility::Visible;
                }
            }
            None => {
                let e = commands
                    .spawn((
                        Mesh3d(pool.mesh.clone()),
                        MeshMaterial3d(pool.materials[colour].clone()),
                        transform,
                        Name::new("bike stick"),
                    ))
                    .id();
                pool.entities[colour].push(e);
            }
        }
    }
    for (colour, list) in pool.entities.iter().enumerate() {
        for &e in &list[used[colour]..] {
            if let Ok((_, mut v)) = sticks.get_mut(e) {
                *v = Visibility::Hidden;
            }
        }
    }
}

fn bike_sticks(bike: &Bike, gizmos: &mut Sticks) {
    let (frame, metal, rubber, seat) = (0, 1, 2, 3);
    let w = |p: Vec3| bike.to_world(p);
    let rot = bike.rotation;
    let bars = bike.bar_rotation();
    let (rear_leg, front_leg) = (physics::rear_leg(), physics::front_leg());
    let (rear, front) = (bike.axle(0), bike.axle(1));
    let axis = physics::steering_axis();

    // Wheels: tyre, rim and spokes; the front turns with the bars.
    for (axle, leg, across) in [
        (rear, rear_leg, Vec3::X),
        (front, front_leg, bars * Vec3::X),
    ] {
        let (centre, radius) = (w(axle), leg.radius);
        let across = rot * across;
        // The tread: two rubber rings, the outer edges `width` apart.
        let edge = (leg.width * 0.5 - COLOURS[rubber].3).max(0.0);
        for s in [edge, -edge] {
            ring(gizmos, centre + across * s, across, radius, rubber);
        }
        ring(gizmos, centre, across, radius * 0.78, metal);
        let u = across.any_orthonormal_vector();
        let v = across.cross(u);
        for s in [u, v, (u + v).normalize(), (u - v).normalize()] {
            gizmos.line(centre - s * radius * 0.78, centre + s * radius * 0.78, metal);
        }
        gizmos.line(centre - across * 0.08, centre + across * 0.08, metal);
    }

    // Forks: two legs from the triple clamp to the axle.
    let clamp = physics::fork_base() + axis * 0.95;
    let side = bars * Vec3::X * 0.09;
    for s in [side, -side] {
        gizmos.line(w(clamp + s), w(front + s), metal);
    }
    gizmos.line(w(clamp + side), w(clamp - side), metal);
    // Bars and grips.
    let [left, right] = bike.grips();
    let centre = physics::bar_centre();
    gizmos.line(w(clamp), w(centre), metal);
    gizmos.line(w(left), w(right), metal);
    gizmos.line(w(left), w(left - bars * Vec3::X * 0.12), rubber);
    gizmos.line(w(right), w(right + bars * Vec3::X * 0.12), rubber);
    // Front fender, turning with the bars.
    let fender = |a: f32| front + bars * (Vec3::new(0.0, a.cos(), a.sin()) * (front_leg.radius + 0.06));
    for i in 0..6 {
        let (a, b) = (-0.9 + i as f32 * 0.3, -0.6 + i as f32 * 0.3);
        gizmos.line(w(fender(a)), w(fender(b)), frame);
    }

    // Frame: head tube, down tube and cradle, top rail, subframe.
    let head_top = physics::fork_base() + axis * 0.92;
    let head_low = physics::fork_base() + axis * 0.70;
    let pivot = Vec3::new(0.0, -0.18, -0.20);
    let cradle = Vec3::new(0.0, -0.40, 0.08);
    let seat_front = Vec3::new(0.0, 0.26, -0.05);
    let seat_back = Vec3::new(0.0, 0.24, -0.95);
    let tail = Vec3::new(0.0, 0.18, -1.15);
    for (a, b) in [
        (head_top, head_low),
        (head_low, cradle),
        (cradle, pivot),
        (head_top, seat_front),
        (seat_front, pivot),
        (pivot, seat_back),
        (seat_back, tail),
    ] {
        gizmos.line(w(a), w(b), frame);
    }
    // Tank and seat.
    gizmos.line(w(head_top - Vec3::Y * 0.02), w(seat_front + Vec3::Y * 0.04), frame);
    gizmos.line(w(seat_front + Vec3::Y * 0.04), w(seat_back + Vec3::Y * 0.04), seat);
    // Engine: a box under the tank.
    let lo = Vec3::new(0.12, -0.38, -0.15);
    let hi = Vec3::new(-0.12, 0.05, 0.20);
    let corner = |x: f32, y: f32, z: f32| w(Vec3::new(x, y, z));
    for (y, z) in [(lo.y, lo.z), (lo.y, hi.z), (hi.y, lo.z), (hi.y, hi.z)] {
        gizmos.line(corner(lo.x, y, z), corner(hi.x, y, z), metal);
    }
    for x in [lo.x, hi.x] {
        let pts = [(lo.y, lo.z), (lo.y, hi.z), (hi.y, hi.z), (hi.y, lo.z), (lo.y, lo.z)];
        for p in pts.windows(2) {
            gizmos.line(corner(x, p[0].0, p[0].1), corner(x, p[1].0, p[1].1), metal);
        }
    }
    // Swingarm (both sides) and shock.
    for s in [0.1f32, -0.1] {
        let x = Vec3::X * s;
        gizmos.line(w(pivot + x), w(rear + x), metal);
    }
    gizmos.line(w(Vec3::new(0.0, 0.18, -0.30)), w(pivot.lerp(rear, 0.35)), metal);
    // Pegs.
    for s in [1.0f32, -1.0] {
        let p = Vec3::new(physics::PEG.x * s, physics::PEG.y, physics::PEG.z);
        gizmos.line(w(p * Vec3::new(0.4, 1.0, 1.0)), w(p), metal);
    }
    // The ground under the tyres while they touch, so landings read.
    for wheel in &bike.wheels {
        if let Some(c) = wheel.contact {
            gizmos.line(c.point, c.point + c.normal * 0.08, 4);
        }
    }
}

fn show_status(
    riding: Res<Riding>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    time: Res<Time>,
    mut logged: Local<f32>,
) {
    let Some(b) = riding.bike.as_ref() else {
        return;
    };
    let status = format!(
            "bike ({}) {:.0} km/h, gear {}, {:.0} rpm, {} wheel{} down{}",
            physics::setup().name,
            b.speed() * 3.6,
            b.engine.gear + 1,
            b.engine.rpm,
            b.grounded(),
            if b.grounded() == 1 { "" } else { "s" },
        if b.crashed.is_some() { ", CRASHED" } else { "" },
    );
    let status = if riding.marker.marker.is_some() { format!("{status}, marker set") } else { status };
    // A line a second in screenshot mode, so a headless ride can be read.
    if riding.neutral.is_some() {
        *logged += time.delta_secs();
        if *logged >= 1.0 {
            *logged = 0.0;
            let p = to_halo(b.position);
            eprintln!(
                "bike: {status}; halo [{:.2}, {:.2}, {:.2}], up {:.2}",
                p.x,
                p.y,
                p.z,
                b.up().y
            );
        }
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let base = window.title.split(" | ").next().unwrap_or_default().to_string();
    let title = format!("{base} | {status}");
    if window.title != title {
        window.title = title;
    }
}
