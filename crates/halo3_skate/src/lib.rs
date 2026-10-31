//! Skating: goal 3. The Skate 3 engine (`skate-host`, linked from the MW2
//! mashup) runs on its own thread, fed the map's collision, its grind rails
//! and the controller; it hands back a posed skeleton and a camera.
//!
//! J (or both sticks clicked) drops the skater where the camera is and starts
//! riding; J again goes back to flying. G (or the d-pad's up) jumps off onto
//! foot, keeping the board's speed (`halo3_foot`). Without a pad the keyboard stands in:
//! WASD is the left stick, the arrow keys the right stick, Space A, F B,
//! R X, T Y, Q / E the bumpers, Z / X the triggers.
//!
//! Units follow `docs/skate-interface.md` section 10. The renderer frame is
//! already the skate engine's (metres, Y up, the same axis swap), so the only
//! conversion is the scale `k`: the engine sees the world shrunk by `k`, so a
//! 1.8 m skater stands as tall as a 2.1 m Spartan. `HALO3_SKATE_SCALE`
//! overrides it.
//!
//! The rider is the map's Spartan on the Skate 3 board (`rider.rs`); a
//! stick figure stands in when either is missing.

use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender};

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;
use halo3_core::map::MapAppExt;
use bevy::window::PrimaryWindow;
use skate_host::bridge::{InputFrame, Pose, PreparedCollision, Session};

use blam_geometry::{Collision, TriggerVolume, rails};

use halo3_core::coords::{to_halo_dir, to_render};
use halo3_core::flag::SessionFlag;
use halo3_core::fly_camera::FlyCamera;
pub use halo3_core::skate::{Respawn, RiderPose, SkateMotion, Skating, assets_root, scale};
use halo3_core::skate::{Bail, Dismount, Mount, Ride, bail_pressed};
use halo3_core::zones::LoadedBsps;

/// The map data the skate world is built from, handed over by `main`: the
/// collision, the trigger volumes (rails in kill volumes are dropped) and the
/// water surfaces (rails under deep water are dropped), all in Halo units.
#[derive(Resource)]
pub struct SkateInput(pub Option<Collision>, pub Vec<TriggerVolume>, pub Vec<[Vec3; 3]>);

pub struct SkatePlugin;

impl Plugin for SkatePlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Host>()
            .init_map_resource::<RiderPose>()
            .init_map_resource::<AutoStart>()
            .init_map_resource::<Respawn>()
            .init_map_resource::<Dismount>()
            .init_map_resource::<Mount>()
            .init_map_resource::<SessionFlag>()
            .add_systems(halo3_core::map::MapStartup, start_worker)
            .init_map_resource::<SkateMotion>()
            .add_systems(
                Update,
                (update.run_if(halo3_core::pause::running), publish_motion, draw_rider, show_status).chain().run_if(halo3_core::map::ready),
            );
    }
}

fn publish_motion(host: Res<Host>, mut motion: ResMut<SkateMotion>) {
    if motion.riding != host.active || motion.speed != host.speed || motion.state != host.state {
        motion.riding = host.active;
        motion.speed = host.speed;
        motion.state.clone_from(&host.state);
    }
}

/// Drop the skater in as soon as the session is ready and ride with the
/// sticks at rest (screenshot mode): a skate-space spawn and heading.
#[derive(Resource, Default)]
pub struct AutoStart(pub Option<([f32; 3], f32)>);

impl AutoStart {
    /// At a Halo position (world units) facing Halo yaw (radians).
    pub fn at(position: Vec3, yaw: f32) -> Self {
        let spawn = (to_render(position + Vec3::Z * 0.3) * scale()).to_array();
        AutoStart(Some((spawn, yaw + std::f32::consts::FRAC_PI_2)))
    }
}

enum Job {
    /// Build the skate world from the collision of these BSPs (bit `n`:
    /// BSP `n`). The first makes the session; later ones build the new
    /// collision on a helper thread and swap it in while the skater rides
    /// on (`Install`).
    World(u32),
    /// Collision built for request `n` (the latest `World` wins).
    Install(u64, Box<PreparedCollision>, usize, usize),
    Activate(u64, [f32; 3], f32, f32),
    Step(u64, f32, InputFrame, f32),
    Suspend,
}

enum Reply {
    Ready(usize, usize),
    /// The ride goes on on new collision.
    Swapped(usize, usize),
    Activated(u64, Pose),
    Pose(u64, Pose),
    /// The engine's session marker moved here (skate space, 0.2 m above
    /// the deck).
    Marker(Vec3),
    Error(String),
}

#[derive(Resource)]
struct Host {
    scale: f32,
    send: Option<Sender<Job>>,
    receive: Option<Mutex<Receiver<Reply>>>,
    ready: bool,
    active: bool,
    epoch: u64,
    packet: u32,
    /// Riding with neutral input (screenshot mode).
    neutral: bool,
    speed: f32,
    /// The skater's feet and velocity in the renderer frame (m, m/s), for
    /// jumping off onto foot.
    feet: Vec3,
    velocity: Vec3,
    state: String,
    status: String,
    /// The BSPs the skate world was last asked to hold.
    world: Option<u32>,
    /// A new zone set's collision is being built to swap in.
    swapping: bool,
    /// The removed- plus moved-objects generations the skate world was
    /// built with (`halo3_core::objects`).
    removed: u64,
}

impl Default for Host {
    fn default() -> Self {
        Host {
            scale: scale(),
            send: None,
            receive: None,
            ready: false,
            active: false,
            epoch: 0,
            packet: 0,
            neutral: false,
            speed: 0.0,
            feet: Vec3::ZERO,
            velocity: Vec3::ZERO,
            state: String::new(),
            status: String::new(),
            world: None,
            swapping: false,
            removed: 0,
        }
    }
}

/// The collision as the skate engine wants it: skate metres, no degenerate
/// or repeated triangles. Halo already winds faces counter-clockwise seen
/// from the side you stand on, which is what the engine expects.
fn skate_triangles(collision: &Collision, k: f32) -> Vec<[[f32; 3]; 3]> {
    let mut seen = std::collections::HashSet::new();
    let key = |p: Vec3| (p * 1000.0).round().as_ivec3().to_array();
    collision
        .triangles
        .iter()
        .filter_map(|t| {
            let p = t.points.map(|p| to_render(p) * k);
            let [a, b, c] = p;
            let area2 = (b - a).cross(c - a).length_squared();
            if !area2.is_finite() || area2 <= 1e-9 {
                return None;
            }
            let mut sorted = p.map(key);
            sorted.sort_unstable();
            seen.insert(sorted).then(|| p.map(|v| v.to_array()))
        })
        .collect()
}

/// The skate engine's triangles and grind rails for the BSPs in `mask`
/// (rails inside kill volumes dropped), skate metres.
#[allow(clippy::type_complexity)]
fn skate_world(
    collision: &Collision,
    volumes: &[TriggerVolume],
    water: &[[Vec3; 3]],
    mask: u32,
    k: f32,
) -> (Vec<[[f32; 3]; 3]>, Vec<Vec<[f32; 3]>>) {
    let mut loaded = collision.of_bsps(mask);
    // Objects a script destroyed or dropped (`halo3_core::objects`).
    let removed = halo3_core::objects::removed_objects();
    if !removed.is_empty() {
        loaded.triangles.retain(|t| t.source.object().is_none_or(|o| !removed.contains(&o)));
    }
    // Objects a script moved (an island tilting): where they are now.
    let poses = halo3_core::objects::object_poses();
    if !poses.is_empty() {
        let poses: std::collections::HashMap<u32, bevy::math::Mat4> = poses.into_iter().collect();
        for t in &mut loaded.triangles {
            if let Some(m) = t.source.object().and_then(|o| poses.get(&o)) {
                t.points = t.points.map(|p| m.transform_point3(p));
            }
        }
    }
    let triangles = skate_triangles(&loaded, k);
    let (found, _) = rails::find_rails(&loaded);
    let found = rails::outside_kill_volumes(found, volumes);
    let found = rails::out_of_deep_water(found, water, rails::DEEP_WATER);
    let rails = found
        .iter()
        .map(|r| r.iter().map(|p| (to_render(*p) * k).to_array()).collect())
        .collect();
    (triangles, rails)
}

fn start_worker(mut host: ResMut<Host>, mut input: ResMut<SkateInput>, loaded: Res<LoadedBsps>) {
    let volumes = std::mem::take(&mut input.1);
    let water = std::mem::take(&mut input.2);
    let Some(collision) = input.0.take() else {
        return;
    };
    let root = assets_root();
    if !root.join("private").join("game.json").exists() {
        host.status = format!("skate assets not found at {}", root.display());
        warn!("{}; set HALO3_SKATE_ASSETS", host.status);
        return;
    }
    let k = host.scale;
    let (send, jobs) = mpsc::channel::<Job>();
    let (publish, replies) = mpsc::channel::<Reply>();
    let resend = send.clone();
    let collision = std::sync::Arc::new(collision);
    let volumes = std::sync::Arc::new(volumes);
    let water = std::sync::Arc::new(water);
    let spawned = std::thread::Builder::new()
        .name("halo3-skate".into())
        // The engine recurses deeply.
        .stack_size(32 * 1024 * 1024)
        .spawn(move || {
            let result = (|| -> Result<(), String> {
                let mut session: Option<Session> = None;
                let mut epoch = 0;
                let mut accumulated = 0.0;
                // The latest world request, for dropping stale installs.
                let mut request = 0u64;
                // The session marker last reported.
                let mut marker: Option<Vec3> = None;
                while let Ok(job) = jobs.recv() {
                    let job = match job {
                        Job::World(mask) if session.is_some() => {
                            // Riding: build beside the simulation and swap.
                            request += 1;
                            let n = request;
                            let builder = session.as_ref().unwrap().collision_builder();
                            let (collision, volumes, water, resend) = (
                                std::sync::Arc::clone(&collision),
                                std::sync::Arc::clone(&volumes),
                                std::sync::Arc::clone(&water),
                                resend.clone(),
                            );
                            let spawned = std::thread::Builder::new()
                                .name("halo3-skate-collision".into())
                                .stack_size(32 * 1024 * 1024)
                                .spawn(move || {
                                    let started = std::time::Instant::now();
                                    let (triangles, rails) = skate_world(&collision, &volumes, &water, mask, k);
                                    let counts = (triangles.len(), rails.len());
                                    match builder.build(triangles, rails) {
                                        Ok(prepared) => {
                                            eprintln!(
                                                "skate: collision for BSP mask {mask:#x} built in {:.1} s ({} triangles, {} rails)",
                                                started.elapsed().as_secs_f32(),
                                                counts.0,
                                                counts.1
                                            );
                                            let _ = resend.send(Job::Install(n, Box::new(prepared), counts.0, counts.1));
                                        }
                                        Err(e) => eprintln!("skate: collision for BSP mask {mask:#x}: {e}"),
                                    }
                                });
                            if let Err(e) = spawned {
                                eprintln!("skate: collision thread: {e}");
                            }
                            continue;
                        }
                        Job::Install(n, prepared, t, r) => {
                            if n == request
                                && let Some(session) = session.as_mut()
                            {
                                session.install_collision(*prepared)?;
                                if publish.send(Reply::Swapped(t, r)).is_err() {
                                    return Ok(());
                                }
                            }
                            continue;
                        }
                        // Only the latest world request matters: skip ahead
                        // when the zone set was changed several times.
                        Job::World(mask) => {
                            let mut mask = mask;
                            let mut next = None;
                            for j in jobs.try_iter() {
                                match j {
                                    Job::World(m) => mask = m,
                                    Job::Install(..) => {}
                                    other => {
                                        next = Some(other);
                                        break;
                                    }
                                }
                            }
                            // Free the old world before building the next.
                            drop(session.take());
                            let started = std::time::Instant::now();
                            let (triangles, rails) = skate_world(&collision, &volumes, &water, mask, k);
                            let rails_found = started.elapsed().as_secs_f32();
                            let counts = (triangles.len(), rails.len());
                            session = Some(Session::new(&root, triangles, rails, [0.0; 3], 0.0)?);
                            eprintln!(
                                "skate: session ready in {:.1} s ({} triangles, {} rails found in {rails_found:.1} s)",
                                started.elapsed().as_secs_f32(),
                                counts.0,
                                counts.1
                            );
                            if publish.send(Reply::Ready(counts.0, counts.1)).is_err() {
                                return Ok(());
                            }
                            match next {
                                Some(j) => j,
                                None => continue,
                            }
                        }
                        other => other,
                    };
                    let Some(session) = session.as_mut() else {
                        continue;
                    };
                    match job {
                        Job::World(_) | Job::Install(..) => {}
                        Job::Activate(new_epoch, spawn, heading, aspect) => {
                            epoch = new_epoch;
                            accumulated = 0.0;
                            session.set_aspect_ratio(aspect);
                            let p = session.activate(spawn, heading)?;
                            if publish.send(Reply::Activated(epoch, p)).is_err() {
                                break;
                            }
                        }
                        Job::Suspend => {
                            accumulated = 0.0;
                            session.suspend_input();
                        }
                        Job::Step(request, dt, input, aspect) => {
                            if request != epoch {
                                continue;
                            }
                            session.set_aspect_ratio(aspect);
                            session.collect(input, dt);
                            accumulated = (accumulated + dt).min(0.15);
                            let mut advanced = false;
                            // The engine's camera can change the period (slow motion).
                            while accumulated >= session.period() {
                                accumulated -= session.period();
                                session.advance()?;
                                advanced = true;
                            }
                            if advanced {
                                let p = session.pose();
                                if !p.root.is_finite() || p.bones.iter().any(|b| !b.is_finite()) {
                                    return Err("the skate engine published a non-finite pose".into());
                                }
                                if publish.send(Reply::Pose(epoch, p)).is_err() {
                                    break;
                                }
                                let now = session.session_marker();
                                if now != marker {
                                    marker = now;
                                    if let Some(m) = now
                                        && publish.send(Reply::Marker(m)).is_err()
                                    {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                let _ = publish.send(Reply::Error(e));
            }
        });
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    let _ = send.send(Job::World(mask));
    host.world = Some(mask);
    match spawned {
        Ok(_) => {
            host.send = Some(send);
            host.receive = Some(Mutex::new(replies));
            host.status = "skate: loading".into();
        }
        Err(e) => host.status = format!("skate thread: {e}"),
    }
}

/// A controller's state in XInput's layout (raw: the engine applies its own
/// deadzones).
fn pad_frame(pad: &Gamepad, packet: u32) -> InputFrame {
    const BITS: [(B, u16); 14] = [
        (B::DPadUp, 0x0001),
        (B::DPadDown, 0x0002),
        (B::DPadLeft, 0x0004),
        (B::DPadRight, 0x0008),
        (B::Start, 0x0010),
        (B::Select, 0x0020),
        (B::LeftThumb, 0x0040),
        (B::RightThumb, 0x0080),
        (B::LeftTrigger, 0x0100),
        (B::RightTrigger, 0x0200),
        (B::South, 0x1000),
        (B::East, 0x2000),
        (B::West, 0x4000),
        (B::North, 0x8000),
    ];
    let buttons = BITS
        .iter()
        .filter(|(b, _)| pad.pressed(*b))
        .fold(0, |bits, (_, bit)| bits | bit);
    let trigger = |b: B| {
        let v = pad.get(b).unwrap_or(if pad.pressed(b) { 1.0 } else { 0.0 });
        (v.clamp(0.0, 1.0) * 255.0).round() as u8
    };
    let axis = |v: f32| (v.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let (l, r) = (pad.left_stick(), pad.right_stick());
    InputFrame::from_pad(
        buttons,
        [trigger(B::LeftTrigger2), trigger(B::RightTrigger2)],
        [axis(l.x), axis(l.y)],
        [axis(r.x), axis(r.y)],
        packet,
    )
}

/// The keyboard as a pad, for playing without one.
fn keyboard_frame(keys: &ButtonInput<KeyCode>, packet: u32) -> InputFrame {
    const BITS: [(KeyCode, u16); 6] = [
        (KeyCode::Space, 0x1000),
        (KeyCode::KeyF, 0x2000),
        (KeyCode::KeyR, 0x4000),
        (KeyCode::KeyT, 0x8000),
        (KeyCode::KeyQ, 0x0100),
        (KeyCode::KeyE, 0x0200),
    ];
    let buttons = BITS
        .iter()
        .filter(|(k, _)| keys.pressed(*k))
        .fold(0, |bits, (_, bit)| bits | bit);
    let axis = |neg: KeyCode, pos: KeyCode| {
        (keys.pressed(pos) as i16 - keys.pressed(neg) as i16) * 32767
    };
    let trigger = |k: KeyCode| if keys.pressed(k) { 255 } else { 0 };
    InputFrame::from_pad(
        buttons,
        [trigger(KeyCode::KeyZ), trigger(KeyCode::KeyX)],
        [axis(KeyCode::KeyA, KeyCode::KeyD), axis(KeyCode::KeyS, KeyCode::KeyW)],
        [
            axis(KeyCode::ArrowLeft, KeyCode::ArrowRight),
            axis(KeyCode::ArrowDown, KeyCode::ArrowUp),
        ],
        packet,
    )
}

#[allow(clippy::too_many_arguments)]
fn update(
    mut commands: Commands,
    mut host: ResMut<Host>,
    mut rider: ResMut<RiderPose>,
    mut auto: ResMut<AutoStart>,
    mut respawn: ResMut<Respawn>,
    (mut dismount, mut mount): (ResMut<Dismount>, ResMut<Mount>),
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, &mut Transform, &mut FlyCamera, &mut Projection)>,
    loaded: Res<LoadedBsps>,
    mut flag: ResMut<SessionFlag>,
) {
    let host = &mut *host;
    let Some(receive) = host.receive.as_ref() else {
        return;
    };
    // Another zone set: rebuild the skate world from its collision.
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    let mut new_world = host.world != Some(mask);
    // Floor plates a script dropped, objects it moved: rebuild once the
    // changes settle for a second (the build runs beside the ride). Both
    // counters only grow, so their sum changes whenever either does.
    let (removed, changed) = halo3_core::objects::removed_generation();
    let (posed, pose_changed) = halo3_core::objects::pose_generation();
    let removed = removed + posed;
    let changed = changed.max(pose_changed);
    if removed != host.removed
        && host.ready
        && !host.swapping
        && changed.is_none_or(|t| t.elapsed().as_secs_f32() > 1.0)
    {
        host.removed = removed;
        if host.world.is_some() {
            new_world = true;
        }
    }
    let aspect = windows
        .single()
        .map_or(16.0 / 9.0, |w| w.width() / w.height().max(1.0));
    let pad = gamepads.iter().next();
    let toggle = keys.just_pressed(KeyCode::KeyJ)
        || pad.is_some_and(|p| {
            (p.just_pressed(B::LeftThumb) && p.pressed(B::RightThumb))
                || (p.just_pressed(B::RightThumb) && p.pressed(B::LeftThumb))
        });
    let Ok((camera, mut transform, mut fly, mut projection)) = cameras.single_mut() else {
        return;
    };

    // Replies first, so a pose that arrived this frame is drawn this frame.
    let mut failed = None;
    let mut latest = None;
    for reply in receive.lock().unwrap().try_iter() {
        match reply {
            Reply::Ready(t, r) => {
                host.ready = true;
                host.status = format!("skate: ready ({t} triangles, {r} rails), J to ride");
            }
            Reply::Swapped(t, r) => {
                host.swapping = false;
                host.status = format!("skate: new zone set ({t} triangles, {r} rails)");
                info!("{}", host.status);
            }
            Reply::Activated(e, p) | Reply::Pose(e, p) if e == host.epoch => latest = Some(p),
            Reply::Activated(..) | Reply::Pose(..) => {}
            Reply::Marker(m) => {
                // The deck rides about 0.1 m up: the flag goes in the ground.
                flag.0 = Some((m - Vec3::Y * 0.3) / host.scale);
            }
            Reply::Error(e) => failed = Some(e),
        }
    }
    if let Some(e) = failed {
        error!("skate: {e}");
        host.status = format!("skate failed: {e}");
        host.send = None;
        host.receive = None;
        host.ready = false;
        host.active = false;
        rider.active = false;
        commands.entity(camera).remove::<Skating>();
        return;
    }
    let k = host.scale;
    if host.active
        && let Some(p) = latest
    {
        host.speed = p.velocity.length() / k;
        host.feet = p.root.w_axis.truncate() / k;
        host.velocity = p.velocity / k;
        host.state = p.state;
        rider.active = true;
        rider.scale = k;
        rider.names = p.names;
        // The bones are relative to the root (IW4L draws the body at
        // `root`), not world matrices.
        rider.bones = p.bones.iter().map(|b| p.root * *b).collect();
        rider.camera = p.camera.map(|(position, basis, fov)| {
            let view = Transform::from_translation(position / k)
                .looking_to(basis.z_axis.normalize(), basis.y_axis.normalize());
            (view, fov.to_radians())
        });
        if rider.drive_camera
            && let Some((view, fov)) = rider.camera
        {
            *transform = view;
            if let Projection::Perspective(lens) = &mut *projection {
                lens.fov = fov;
            }
        }
    }

    let Some(send) = host.send.clone() else {
        return;
    };
    if new_world && host.ready {
        // The session exists: the new collision is built beside it and
        // swapped in; the ride goes on meanwhile.
        host.world = Some(mask);
        host.swapping = true;
        host.status = "skate: building the new collision".into();
        let _ = send.send(Job::World(mask));
    } else if new_world {
        host.world = Some(mask);
        host.ready = false;
        host.status = "skate: loading the new zone set".into();
        if host.active {
            host.epoch = host.epoch.wrapping_add(1);
            host.active = false;
            rider.active = false;
            rider.bones.clear();
            commands.entity(camera).remove::<Skating>();
        }
        let _ = send.send(Job::World(mask));
        return;
    }
    if host.ready
        && !host.active
        && let Some((spawn, heading)) = auto.0.take()
    {
        host.epoch = host.epoch.wrapping_add(1);
        host.active = true;
        host.neutral = true;
        rider.skated = 0.0;
        commands.entity(camera).insert(Skating);
        let _ = send.send(Job::Activate(host.epoch, spawn, heading, aspect));
        return;
    }
    if let Some((position, yaw)) = respawn.0.take()
        && host.ready
        && host.active
    {
        // A new epoch drops poses still in flight from the old spot.
        host.epoch = host.epoch.wrapping_add(1);
        let AutoStart(Some((spawn, heading))) = AutoStart::at(position, yaw) else {
            return;
        };
        let _ = send.send(Job::Activate(host.epoch, spawn, heading, aspect));
        return;
    }
    // Straight from walking onto the board, facing the way he moved or
    // looked; the engine puts the wheels on the ground below.
    if let Some((Ride::Board, bail)) = mount.0 {
        mount.0 = None;
        if !host.ready || host.active {
            warn!("skate: not ready yet; {}", host.status);
            return;
        }
        let flat = Vec3::new(bail.velocity.x, 0.0, bail.velocity.z);
        let look = Quat::from_euler(EulerRot::YXZ, bail.yaw, 0.0, 0.0) * Vec3::NEG_Z;
        let f = to_halo_dir(if flat.length() > 1.0 { flat } else { look });
        let heading = f.y.atan2(f.x) + std::f32::consts::FRAC_PI_2;
        let spawn = ((bail.position + Vec3::Y * 0.3) * k).to_array();
        host.epoch = host.epoch.wrapping_add(1);
        host.active = true;
        host.neutral = false;
        rider.skated = 0.0;
        commands.entity(camera).insert(Skating);
        let _ = send.send(Job::Activate(host.epoch, spawn, heading, aspect));
        return;
    }
    // Jumping off onto foot, keeping the board's momentum.
    if host.active && !host.neutral && bail_pressed(&keys, pad) {
        host.epoch = host.epoch.wrapping_add(1);
        host.active = false;
        rider.active = false;
        rider.bones.clear();
        rider.camera = None;
        let _ = send.send(Job::Suspend);
        commands.entity(camera).remove::<Skating>();
        dismount.0 = Some(Bail::new(host.feet, host.velocity, &transform));
        return;
    }
    if toggle && host.ready && !rider.bike && !rider.foot {
        host.epoch = host.epoch.wrapping_add(1);
        if host.active {
            host.active = false;
            rider.active = false;
            rider.bones.clear();
            let _ = send.send(Job::Suspend);
            commands.entity(camera).remove::<Skating>();
            // Fly on from where the skate camera left off.
            let f = transform.forward().as_vec3();
            fly.yaw = (-f.x).atan2(-f.z);
            fly.pitch = f.y.clamp(-1.0, 1.0).asin();
            transform.rotation = fly.rotation();
            if let Projection::Perspective(lens) = &mut *projection {
                lens.fov = 75f32.to_radians();
            }
        } else {
            // Drop the skater under the camera, facing the way it looks; the
            // engine puts the wheels on the ground below.
            let f = to_halo_dir(transform.forward().as_vec3());
            let heading = f.y.atan2(f.x) + std::f32::consts::FRAC_PI_2;
            let spawn = (transform.translation * k).to_array();
            host.active = true;
            host.neutral = false;
            rider.skated = 0.0;
            commands.entity(camera).insert(Skating);
            let _ = send.send(Job::Activate(host.epoch, spawn, heading, aspect));
        }
        return;
    }
    if host.active {
        host.packet = host.packet.wrapping_add(1);
        let frame = match pad {
            _ if host.neutral => InputFrame::from_pad(0, [0, 0], [0, 0], [0, 0], host.packet),
            Some(p) => pad_frame(p, host.packet),
            None => keyboard_frame(&keys, host.packet),
        };
        let dt = time.delta_secs().min(0.1);
        rider.skated += dt;
        let _ = send.send(Job::Step(host.epoch, dt, frame, aspect));
    }
}

/// The skater as lines between joints, the board in orange.
fn draw_rider(rider: Res<RiderPose>, mut gizmos: Gizmos) {
    if !rider.active || rider.drawn || rider.bones.is_empty() {
        return;
    }
    const BODY: [(&str, &str); 22] = [
        ("HIPS", "SPINE"),
        ("SPINE", "SPINE1"),
        ("SPINE1", "SPINE2"),
        ("SPINE2", "SPINE3"),
        ("SPINE3", "NECK"),
        ("NECK", "NECK1"),
        ("NECK1", "HEAD"),
        ("SPINE3", "LEFTSHOULDER"),
        ("LEFTSHOULDER", "LEFTARM"),
        ("LEFTARM", "LEFTFOREARM"),
        ("LEFTFOREARM", "LEFTHAND"),
        ("SPINE3", "RIGHTSHOULDER"),
        ("RIGHTSHOULDER", "RIGHTARM"),
        ("RIGHTARM", "RIGHTFOREARM"),
        ("RIGHTFOREARM", "RIGHTHAND"),
        ("HIPS", "LEFTUPLEG"),
        ("LEFTUPLEG", "LEFTLEG"),
        ("LEFTLEG", "LEFTFOOT"),
        ("LEFTFOOT", "LEFTTOEBASE"),
        ("HIPS", "RIGHTUPLEG"),
        ("RIGHTUPLEG", "RIGHTLEG"),
        ("RIGHTLEG", "RIGHTFOOT"),
    ];
    const BOARD: [(&str, &str); 5] = [
        ("TRUCK_FRONT", "TRUCK_BACK"),
        ("TRUCK_FRONT", "LEFT_WHEELFRONT"),
        ("TRUCK_FRONT", "RIGHT_WHEELFRONT"),
        ("TRUCK_BACK", "LEFT_WHEELBACK"),
        ("TRUCK_BACK", "RIGHT_WHEELBACK"),
    ];
    let at = |name: &str| {
        let i = rider.names.iter().position(|n| n == name)?;
        Some(rider.bones[i].w_axis.truncate() / rider.scale)
    };
    for (pairs, colour) in [
        (&BODY[..], Color::srgb(0.2, 0.9, 0.3)),
        (&BOARD[..], Color::srgb(1.0, 0.55, 0.1)),
    ] {
        for (a, b) in pairs {
            if let (Some(a), Some(b)) = (at(a), at(b)) {
                gizmos.line(a, b, colour);
            }
        }
    }
    if let Some(head) = at("HEAD") {
        gizmos.sphere(Isometry3d::from_translation(head), 0.15, Color::srgb(0.2, 0.9, 0.3));
    }
}

/// The skate status in the window title.
fn show_status(
    host: Res<Host>,
    rider: Res<RiderPose>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
) {
    if rider.bike || rider.foot {
        return;
    }
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let base = window.title.split(" | ").next().unwrap_or_default().to_string();
    let status = if host.active {
        format!(
            "skating {:.1} m/s {}",
            host.speed,
            host.state.chars().take(48).collect::<String>()
        )
    } else {
        host.status.clone()
    };
    let title = if status.is_empty() { base } else { format!("{base} | {status}") };
    if window.title != title {
        window.title = title;
    }
}
