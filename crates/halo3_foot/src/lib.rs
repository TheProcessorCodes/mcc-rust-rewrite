//! On foot: the Spartan walking, running, crouching and jumping on the
//! map's collision, Halo style (`physics.rs` for the body, `pose.rs` for
//! the skeleton `halo3_rider` draws).
//!
//! F (or the d-pad's up) drops him on the ground under the camera and F
//! again goes back to flying. G (or the d-pad's up) while skating or on the
//! bike jumps off onto foot, keeping the ride's speed (`Dismount`). The camera sits over his right shoulder; T
//! (or the d-pad's down) puts it in his eyes.
//!
//! | controller | keyboard and mouse | |
//! |---|---|---|
//! | left stick | W A S D | move |
//! | right stick | mouse | aim |
//! | A | Space | jump |
//! | left stick click | Ctrl / C (hold) | crouch |
//! | d-pad down | T | first / third person |
//! | d-pad up | F | on foot / flying |
//! | View / Back | B | back on the bike, keeping his speed |
//! | both sticks clicked | J | back on the board |
//!
//! The map's gravity lifts (`halo3_core::lifts`) carry him up.
//! Kill volumes, the bottom of the world (`halo3_core::falls`) and water over his
//! head put him back where he last stood.
//!
//! `--shot DIR --foot` stands him at the first view (or the start spawn)
//! for a couple of seconds, then takes the rider pictures;
//! `HALO3_FOOT_WALK=x,y` holds the stick meanwhile (X right, Y forward).

pub mod physics;
pub mod pose;

use bevy::input::gamepad::GamepadButton as B;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use halo3_bike::physics::World;
use halo3_bike::pose::Rig;
use halo3_core::coords::{to_halo, to_render, to_render_dir};
use halo3_core::fly_camera::FlyCamera;
use halo3_core::skate::{Bail, Dismount, Mount, Ride, RiderPose, Skating, assets_root, dpad_up_alone};
use halo3_core::zones::LoadedBsps;

use physics::{Controls, Spartan};
use pose::Gait;

/// Start on foot at once at a Halo position (world units) facing a Halo yaw
/// (radians): screenshot mode and `--foot`.
#[derive(Resource, Default)]
pub struct AutoStart(pub Option<(Vec3, f32)>);

impl AutoStart {
    pub fn at(position: Vec3, yaw: f32) -> Self {
        AutoStart(Some((position, yaw)))
    }
}

/// How the Spartan moves: the map's own player numbers, read by `main`.
#[derive(Resource, Default, Clone, Copy)]
pub struct FootTuning(pub physics::Tuning);

pub struct FootPlugin;

impl Plugin for FootPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OnFoot>()
            .init_resource::<FootTuning>()
            .init_resource::<AutoStart>()
            .init_resource::<Dismount>()
            .init_resource::<Mount>()
            .add_systems(Startup, setup)
            .add_systems(Update, (walk.run_if(halo3_core::pause::running), show_status).chain());
    }
}

/// Field of view (vertical) in the eyes and over the shoulder.
const FIRST_FOV: f32 = 55.0;
const THIRD_FOV: f32 = 60.0;

#[derive(Resource, Default)]
pub struct OnFoot {
    world: Option<World>,
    /// The BSPs `world` was built from.
    mask: Option<u32>,
    pub spartan: Option<Spartan>,
    rig: Option<Rig>,
    gait: Gait,
    /// Where he aims: the camera's yaw and pitch (`FlyCamera` convention).
    pub yaw: f32,
    pub pitch: f32,
    pub first_person: bool,
    /// The stick held in screenshot mode.
    held: Option<Vec2>,
    /// The camera's height above the feet, smoothed over steps.
    eye_lift: Option<f32>,
    /// Third-person camera distance, eased back out after a wall.
    boom: f32,
    /// Set by `weapons`: the weapon in his hands, its `left_hand` marker
    /// in grip space (`pose::Hold`). `None`: empty hands.
    pub holding: Option<Vec3>,
    /// The weapon's grip space to the renderer frame this frame, while he
    /// holds one.
    pub weapon_world: Option<Mat4>,
    /// Zoomed in through a scope: the view from his eyes, narrower.
    pub zoom: Option<f32>,
}

impl OnFoot {
    /// The collision he walks on, once built.
    pub fn world(&self) -> Option<&World> {
        self.world.as_ref()
    }

    /// Where the camera looks.
    pub fn aim_rotation(&self) -> Quat {
        Quat::from_euler(EulerRot::YXZ, self.yaw, self.pitch, 0.0)
    }
}

fn setup(mut foot: ResMut<OnFoot>) {
    match Rig::load(&assets_root().join("rig.json")) {
        Ok(r) => foot.rig = Some(r),
        Err(e) => warn!("on foot: {e}; the Spartan is left off"),
    }
}

/// Halo yaw (radians, in the XY plane) to the camera's yaw about +Y.
fn camera_yaw_of_halo_yaw(yaw: f32) -> f32 {
    let d = to_render_dir(Vec3::new(yaw.cos(), yaw.sin(), 0.0));
    (-d.x).atan2(-d.z)
}

fn dead(v: Vec2, zone: f32) -> Vec2 {
    let len = v.length();
    if len < zone {
        Vec2::ZERO
    } else {
        v / len * ((len.min(1.0) - zone) / (1.0 - zone))
    }
}

fn cursor_grabbed(windows: &Query<&CursorOptions, With<PrimaryWindow>>) -> bool {
    windows.single().is_ok_and(|c| c.grab_mode != CursorGrabMode::None)
}

#[allow(clippy::too_many_arguments)]
pub fn walk(
    mut commands: Commands,
    mut foot: ResMut<OnFoot>,
    (mut auto, mut dismount, mut mount): (ResMut<AutoStart>, ResMut<Dismount>, ResMut<Mount>),
    input: Option<Res<halo3_bike::BikeInput>>,
    tuning: Res<FootTuning>,
    // Paired: a system takes at most 16 parameters.
    (kill, lifts): (Option<Res<halo3_core::falls::KillZones>>, Option<Res<halo3_core::lifts::Lifts>>),
    water: Option<Res<halo3_swim::WaterWorld>>,
    fallback: Option<Res<halo3_core::falls::FallbackSpot>>,
    mut rider: ResMut<RiderPose>,
    loaded: Res<LoadedBsps>,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    motion: Res<AccumulatedMouseMotion>,
    windows: Query<&CursorOptions, With<PrimaryWindow>>,
    gamepads: Query<&Gamepad>,
    mut cameras: Query<(Entity, &mut Transform, &mut FlyCamera, &mut Projection)>,
) {
    let Some(input) = input else {
        return;
    };
    let foot = &mut *foot;
    let pad = gamepads.iter().next();
    let toggle = keys.just_pressed(KeyCode::KeyF) || pad.is_some_and(dpad_up_alone);
    // The bike's and the board's own buttons get back on from walking.
    let ride = if keys.just_pressed(KeyCode::KeyB) || pad.is_some_and(|p| p.just_pressed(B::Select)) {
        Some(Ride::Bike)
    } else if keys.just_pressed(KeyCode::KeyJ)
        || pad.is_some_and(|p| {
            (p.just_pressed(B::LeftThumb) && p.pressed(B::RightThumb))
                || (p.just_pressed(B::RightThumb) && p.pressed(B::LeftThumb))
        })
    {
        Some(Ride::Board)
    } else {
        None
    };
    let Ok((camera, mut transform, mut fly, mut projection)) = cameras.single_mut() else {
        return;
    };

    // The world follows the loaded zone set.
    let mask = if loaded.mask == 0 { u32::MAX } else { loaded.mask };
    let wanted = foot.spartan.is_some() || toggle || auto.0.is_some() || dismount.0.is_some();
    if wanted && foot.mask != Some(mask) {
        foot.world = Some(halo3_bike::build_world(&input.0, mask));
        foot.mask = Some(mask);
    }
    let Some(world) = foot.world.as_ref() else {
        return;
    };

    // Fields one by one: `world` borrows `foot.world` meanwhile.
    macro_rules! start {
        ($spartan:expr, $yaw:expr) => {{
            foot.spartan = Some($spartan);
            foot.yaw = $yaw;
            foot.pitch = 0.0;
            foot.gait = Gait::default();
            foot.eye_lift = None;
            foot.boom = 0.0;
            // Halo's view: from his eyes (T for the shoulder).
            foot.first_person = true;
            rider.skated = 0.0;
            rider.foot = true;
        }};
    }
    if let Some((position, yaw)) = auto.0.take() {
        let cam_yaw = camera_yaw_of_halo_yaw(yaw);
        let s = Spartan::dropped(world, to_render(position) + Vec3::Y, cam_yaw + std::f32::consts::PI);
        start!(s, cam_yaw);
        foot.held = std::env::var("HALO3_FOOT_WALK").ok().map(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            Vec2::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
        });
        // Screenshot walks are shot over the shoulder, to see the body.
        foot.first_person = foot.held.is_none();
        commands.entity(camera).insert(Skating);
    } else if let Some(bail) = dismount.0.take() {
        // Off the board or the bike: he jumps clear, carrying its momentum,
        // still looking where the ride's camera looked.
        let mut s = Spartan::new(bail.position + Vec3::Y * 0.1, bail.yaw + std::f32::consts::PI);
        s.velocity = bail.velocity + Vec3::Y * tuning.0.jump;
        s.jumped = true;
        start!(s, bail.yaw);
        foot.pitch = bail.pitch.clamp(-1.45, 1.45);
        foot.held = None;
        commands.entity(camera).insert(Skating);
        let v = bail.velocity;
        info!("on foot: jumped off at {:.1} m/s", Vec2::new(v.x, v.z).length());
    } else if toggle || (ride.is_some() && foot.spartan.is_some()) {
        if let Some(spartan) = foot.spartan.take() {
            if let Some(ride) = ride {
                let view = rider.camera.map_or(*transform, |(v, _)| v);
                mount.0 = Some((ride, Bail::new(spartan.position, spartan.velocity, &view)));
            }
            rider.foot = false;
            rider.first_person = false;
            rider.active = false;
            rider.bones.clear();
            rider.camera = None;
            commands.entity(camera).remove::<Skating>();
            // Fly on from where the camera was.
            fly.yaw = foot.yaw;
            fly.pitch = foot.pitch;
            transform.rotation = fly.rotation();
            if let Projection::Perspective(lens) = &mut *projection {
                lens.fov = 75f32.to_radians();
            }
            return;
        }
        if rider.active {
            // The d-pad's up is also the board's and the bike's jump off
            // (`Dismount`), which they handle.
            if keys.just_pressed(KeyCode::KeyF) {
                warn!("on foot: G jumps off the board or the bike");
            }
            return;
        }
        let s = Spartan::dropped(world, transform.translation, fly.yaw + std::f32::consts::PI);
        start!(s, fly.yaw);
        foot.held = None;
        commands.entity(camera).insert(Skating);
    }
    let Some(spartan) = foot.spartan.as_mut() else {
        return;
    };
    spartan.tuning = tuning.0;

    // Aim: the mouse while it is held, the right stick.
    let dt = time.delta_secs().min(0.1);
    let mut turn = Vec2::ZERO;
    if cursor_grabbed(&windows) {
        turn -= motion.delta * fly.mouse_sensitivity / foot.zoom.unwrap_or(1.0).max(1.0);
    }
    let mut controls = Controls::default();
    if let Some(pad) = pad {
        let r = dead(pad.right_stick(), 0.15);
        // Halo's stick: gentle near the middle, fast at the edge.
        turn += Vec2::new(-r.x, r.y) * r.length() * 3.2 * dt / foot.zoom.unwrap_or(1.0).max(1.0);
        controls.wish = dead(pad.left_stick(), 0.15);
        controls.jump = pad.just_pressed(B::South);
        controls.crouch = pad.pressed(B::LeftThumb);
        if pad.just_pressed(B::DPadDown) {
            foot.first_person = !foot.first_person;
        }
    }
    foot.yaw += turn.x;
    foot.pitch = (foot.pitch + turn.y).clamp(-1.45, 1.45);
    let key = |k: KeyCode| keys.pressed(k) as i32 as f32;
    let wasd = Vec2::new(key(KeyCode::KeyD) - key(KeyCode::KeyA), key(KeyCode::KeyW) - key(KeyCode::KeyS));
    controls.wish = (controls.wish + wasd).clamp_length_max(1.0);
    controls.jump |= keys.just_pressed(KeyCode::Space);
    controls.crouch |= keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::KeyC]);
    if keys.just_pressed(KeyCode::KeyT) {
        foot.first_person = !foot.first_person;
    }
    if let Some(held) = foot.held {
        controls.wish = held;
    }
    controls.heading = foot.yaw + std::f32::consts::PI;
    match lifts.as_deref() {
        Some(l) => spartan.step_in(world, controls, dt, &|p, v, h, r| l.push(p, v, h, r, false)),
        None => spartan.step(world, controls, dt),
    }

    // Falling out of the world, or under water over his head.
    let feet = to_halo(spartan.position);
    let mut lost = kill.as_deref().and_then(|k| k.deadly(feet)).map(str::to_string);
    if lost.is_none()
        && let Some(w) = water.as_deref()
    {
        let probe = w.probe(feet);
        let head = spartan.height() / blam_geometry::WORLD_UNIT_METRES;
        if probe.surface.is_some_and(|s| s - feet.z > head * 0.9) {
            lost = Some("deep water".into());
        }
    }
    if let Some(what) = lost {
        if spartan.safe.is_some() {
            spartan.respawn();
        } else if let Some(f) = fallback.as_deref() {
            let tuning = spartan.tuning;
            *spartan = Spartan::dropped(world, to_render(f.0) + Vec3::Y, camera_yaw_of_halo_yaw(f.1) + std::f32::consts::PI);
            spartan.tuning = tuning;
        }
        let back = to_halo(spartan.position);
        info!(
            "fall: Spartan into {what} at halo [{:.2}, {:.2}, {:.2}]; back at [{:.2}, {:.2}, {:.2}]",
            feet.x, feet.y, feet.z, back.x, back.y, back.z
        );
        foot.yaw = spartan.heading - std::f32::consts::PI;
        foot.eye_lift = None;
    }
    rider.skated += dt;

    // The gait, in rig metres and his own frame (X left, Y forward).
    let k = rider.scale;
    let fwd = spartan.forward();
    let left = Vec3::Y.cross(fwd);
    let flat = Vec3::new(spartan.velocity.x, 0.0, spartan.velocity.z);
    let speed = if spartan.grounded { flat.length() } else { foot.gait.speed / k };
    let g = &mut foot.gait;
    if spartan.grounded {
        g.direction = Vec2::new(flat.dot(left), flat.dot(fwd)).normalize_or_zero();
    }
    g.speed = g.speed + (speed * k - g.speed) * (1.0 - (-12.0 * dt).exp());
    g.advance(dt);
    g.crouch = spartan.crouch;
    let airborne = !spartan.grounded && (spartan.jumped || spartan.airborne > 0.15);
    let ease = 1.0 - (-10.0 * dt).exp();
    g.air += ((airborne as i32 as f32) - g.air) * ease;
    g.rise = (spartan.velocity.y / spartan.tuning.jump.max(1.0)).clamp(-1.0, 1.0);
    let (impact, since) = spartan.landing;
    g.land = if since < 0.3 { (impact / 9.0).min(1.0) * (1.0 - since / 0.3) } else { 0.0 };
    g.pitch = foot.pitch;
    g.hold = foot.holding.map(|left_hand| pose::Hold {
        left_hand,
        scale: blam_geometry::WORLD_UNIT_METRES * k,
    });
    g.ground = [0.0; 2];
    if spartan.grounded {
        for (i, side) in [(0, 1.0f32), (1, -1.0)] {
            let at = spartan.position + left * (0.13 / k) * side + Vec3::Y * 0.6;
            if let Some(hit) = world.raycast_far(at, Vec3::NEG_Y, 1.2) {
                g.ground[i] = ((hit.point.y - spartan.position.y) * k).clamp(-0.25, 0.25);
            }
        }
    }

    // The Spartan, as skate bones (skate metres, world).
    if let Some(rig) = foot.rig.as_ref() {
        let place = Mat4::from_translation(spartan.position * k) * Mat4::from_quat(Quat::from_rotation_y(spartan.heading));
        let (bones, weapon) = pose::foot_pose(rig, &foot.gait);
        rider.bones = bones.into_iter().map(|m| place * m).collect();
        foot.weapon_world = weapon.map(|w| Mat4::from_scale(Vec3::splat(1.0 / k)) * place * w);
        if rider.names.len() != rig.names.len() {
            rider.names = rig.names.clone();
        }
        rider.active = true;
    }

    // The camera: his eyes, or over his right shoulder. The height follows
    // steps smoothly.
    let lift = spartan.eye().y - spartan.position.y;
    let smooth = match foot.eye_lift {
        Some(prev) if spartan.grounded => {
            let target = spartan.position.y + lift;
            let y = prev + (target - prev) * (1.0 - (-18.0 * dt).exp());
            // Never more than a step behind.
            y.clamp(target - spartan.tuning.step, target + spartan.tuning.step)
        }
        _ => spartan.position.y + lift,
    };
    foot.eye_lift = Some(smooth);
    let eye = Vec3::new(spartan.position.x, smooth, spartan.position.z);
    let rotation = foot.aim_rotation();
    let (view, fov) = if let Some(zoom) = foot.zoom {
        let narrow = 2.0 * ((FIRST_FOV.to_radians() * 0.5).tan() / zoom.max(1.0)).atan();
        (Transform::from_translation(eye).with_rotation(rotation), narrow.to_degrees())
    } else if foot.first_person {
        (Transform::from_translation(eye).with_rotation(rotation), FIRST_FOV)
    } else {
        // Out to the right shoulder, then back from there, stopping short
        // of walls both ways.
        let head = eye + Vec3::Y * 0.15;
        let out = rotation * Vec3::new(0.6, 0.55, 0.0);
        let reach = world
            .raycast_far(head, out.normalize(), out.length() + 0.25)
            .map_or(1.0, |hit| ((hit.distance - 0.25) / out.length()).max(0.0));
        let shoulder = head + out * reach;
        let dir = rotation * Vec3::Z;
        let mut boom = 3.0f32;
        if let Some(hit) = world.raycast_far(shoulder, dir, boom + 0.3) {
            boom = (hit.distance - 0.3).max(0.0);
        }
        // In fast against a wall, out slowly.
        foot.boom = if boom < foot.boom || foot.boom == 0.0 {
            boom
        } else {
            foot.boom + (boom - foot.boom) * (1.0 - (-4.0 * dt).exp())
        };
        (Transform::from_translation(shoulder + dir * foot.boom).with_rotation(rotation), THIRD_FOV)
    };
    // A camera pressed into his helmet sees from his eyes instead.
    rider.first_person = foot.first_person || foot.zoom.is_some() || view.translation.distance(eye) < 0.8;
    rider.camera = Some((view, fov.to_radians()));
    if rider.drive_camera {
        *transform = view;
        if let Projection::Perspective(lens) = &mut *projection {
            lens.fov = fov.to_radians();
        }
    }
}

fn show_status(
    foot: Res<OnFoot>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    time: Res<Time>,
    mut logged: Local<f32>,
) {
    let Some(s) = foot.spartan.as_ref() else {
        return;
    };
    let status = format!(
        "on foot {:.1} m/s{}{}",
        s.speed(),
        if s.grounded { "" } else { ", in the air" },
        if s.crouch > 0.5 { ", crouched" } else { "" },
    );
    // A line a second in screenshot mode, so a headless walk can be read.
    if foot.held.is_some() {
        *logged += time.delta_secs();
        if *logged >= 1.0 {
            *logged = 0.0;
            let p = to_halo(s.position);
            let f = halo3_core::coords::to_halo_dir(s.forward());
            eprintln!(
                "foot: {status}; halo [{:.2}, {:.2}, {:.2}] facing {:.0} degrees",
                p.x,
                p.y,
                p.z,
                f.y.atan2(f.x).to_degrees()
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
