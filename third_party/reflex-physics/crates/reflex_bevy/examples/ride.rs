//! Ride the Reflex bike in a small test yard: flat ground, ramps and rollers (Bevy meshes tagged as colliders),
//! a chase camera, the bike drawn with gizmos (contact normals green when the tyre holds, orange when it
//! slides), speed / gear / rpm in the window title.
//!
//! ```text
//! cargo run --release -p reflex_bevy --example ride --features example -- [vehicle file]
//! ```
//! The vehicle file is one of your own Reflex vehicle dumps (default `$REFLEX_VEHICLE`, then the stock mx450f
//! JSON dump of reflex-tools). Controls: see `reflex_bevy::input` (W / RT throttle, S / LT brake, A D / left stick
//! steer, arrows / right stick lean, X / pad A reverse, E / RB clutch, Backspace / Back respawn). F1 toggles the
//! gizmo drawing.

use bevy::prelude::*;
use reflex_bevy::debug::{ReflexDebugDraw, ReflexDebugDrawPlugin};
use reflex_bevy::mesh::{ReflexCollider, ReflexMeshGroundPlugin};
use reflex_bevy::{ReflexBike, ReflexBikeState, ReflexDefaultControls, ReflexPlugin, ReflexSet};

fn vehicle_path() -> String {
    std::env::args()
        .nth(1)
        .or_else(|| std::env::var("REFLEX_VEHICLE").ok())
        .unwrap_or_else(|| r"D:\reflex-tools\physics\reflex\dumps\stock-json\mx450f_vehicle.json".into())
}

fn main() {
    let path = vehicle_path();
    let bike = match ReflexBike::from_file(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("cannot load the Reflex vehicle file '{path}': {e}\npass one of your own vehicle dumps as the first argument or set REFLEX_VEHICLE");
            std::process::exit(1);
        }
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "reflex_bevy ride".into(), ..default() }),
            ..default()
        }))
        .add_plugins((ReflexPlugin, ReflexMeshGroundPlugin, ReflexDebugDrawPlugin))
        .insert_resource(ClearColor(Color::srgb(0.55, 0.7, 0.85)))
        .insert_resource(PendingBike(Some(bike)))
        .add_systems(Startup, setup)
        .add_systems(Update, (chase_camera, title, toggle_gizmos).after(ReflexSet::Sync))
        .run();
}

#[derive(Resource)]
struct PendingBike(Option<ReflexBike>);

#[derive(Component)]
struct Player;

#[derive(Component, Default)]
struct Chase {
    look: Vec3,
    ready: bool,
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut pending: ResMut<PendingBike>,
) {
    let dirt = materials.add(StandardMaterial { base_color: Color::srgb(0.45, 0.36, 0.25), perceptual_roughness: 0.95, ..default() });
    let wood = materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.48, 0.3), ..default() });
    let concrete = materials.add(StandardMaterial { base_color: Color::srgb(0.6, 0.6, 0.62), ..default() });
    let mut solid = |commands: &mut Commands, mesh: Mesh, mat: &Handle<StandardMaterial>, at: Transform| {
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(mat.clone()), at, ReflexCollider::default()));
    };
    // ground: 600 m square, its top at y = 0
    solid(&mut commands, Cuboid::new(600.0, 1.0, 600.0).into(), &dirt, Transform::from_xyz(0.0, -0.5, 0.0));
    // a line of kickers ahead of the start (-Z), each a box pitched up
    for (i, (len, deg)) in [(6.0, 12.0f32), (8.0, 18.0), (10.0, 25.0)].iter().enumerate() {
        let z = -40.0 - 45.0 * i as f32;
        let pitch = deg.to_radians();
        let at = Transform::from_xyz(0.0, len * 0.5 * pitch.sin() - 0.5, z)
            .with_rotation(Quat::from_rotation_x(pitch));
        solid(&mut commands, Cuboid::new(4.0, 1.0, *len).into(), &wood, at);
    }
    // rollers: half-buried cylinders across the track, to the right of the start
    for i in 0..10 {
        let at = Transform::from_xyz(25.0, -0.2, -20.0 - 4.0 * i as f32).with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2));
        solid(&mut commands, Cylinder::new(0.7, 8.0).mesh().resolution(24).into(), &dirt, at);
    }
    // a table-top: up ramp, flat top, down ramp, to the left
    let up = Transform::from_xyz(-25.0, 0.9, -40.0).with_rotation(Quat::from_rotation_x(15f32.to_radians()));
    solid(&mut commands, Cuboid::new(6.0, 1.0, 8.0).into(), &concrete, up);
    solid(&mut commands, Cuboid::new(6.0, 3.0, 14.0).into(), &concrete, Transform::from_xyz(-25.0, 0.4, -54.0));
    let down = Transform::from_xyz(-25.0, 0.9, -68.0).with_rotation(Quat::from_rotation_x(-15f32.to_radians()));
    solid(&mut commands, Cuboid::new(6.0, 1.0, 8.0).into(), &concrete, down);

    commands.spawn((DirectionalLight { illuminance: 12_000.0, shadow_maps_enabled: true, ..default() }, Transform::from_xyz(30.0, 60.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y)));
    commands.spawn((Camera3d::default(), Transform::from_xyz(0.0, 3.0, 8.0).looking_at(Vec3::ZERO, Vec3::Y), Chase::default()));
    if let Some(bike) = pending.0.take() {
        commands.spawn((bike, Transform::from_xyz(0.0, 0.0, 0.0), ReflexDefaultControls, Player));
    }
}

#[allow(clippy::type_complexity)]
fn chase_camera(
    time: Res<Time>,
    bike: Query<(&Transform, &ReflexBikeState), (With<Player>, Without<Chase>)>,
    mut cam: Query<(&mut Transform, &mut Chase)>,
) {
    let (Ok((bike, state)), Ok((mut cam, mut chase))) = (bike.single(), cam.single_mut()) else { return };
    let fwd = bike.rotation * Vec3::NEG_Z;
    let flat = Vec3::new(fwd.x, 0.0, fwd.z).normalize_or(Vec3::NEG_Z);
    let target_pos = bike.translation - flat * 6.0 + Vec3::Y * 2.4;
    let target_look = bike.translation + Vec3::Y * 0.6;
    let k = 1.0 - (-6.0 * time.delta_secs()).exp();
    if state.teleported || !chase.ready {
        cam.translation = target_pos;
        chase.look = target_look;
        chase.ready = true;
    } else {
        cam.translation = cam.translation.lerp(target_pos, k);
        chase.look = chase.look.lerp(target_look, k);
    }
    let look = chase.look;
    cam.look_at(look, Vec3::Y);
}

fn title(bike: Query<&ReflexBikeState, With<Player>>, mut windows: Query<&mut Window>) {
    let (Ok(s), Ok(mut w)) = (bike.single(), windows.single_mut()) else { return };
    w.title = format!(
        "reflex_bevy ride   {:5.1} mph   gear {}   {:5.0} rpm{}",
        s.forward_speed * 2.236_936,
        s.gear,
        s.rpm,
        if s.crashed { "   WRECK" } else if s.airborne { "   AIR" } else { "" }
    );
}

fn toggle_gizmos(keys: Res<ButtonInput<KeyCode>>, mut cfg: ResMut<ReflexDebugDraw>) {
    if keys.just_pressed(KeyCode::F1) {
        cfg.bike = !cfg.bike;
        cfg.contacts = cfg.bike;
    }
}
