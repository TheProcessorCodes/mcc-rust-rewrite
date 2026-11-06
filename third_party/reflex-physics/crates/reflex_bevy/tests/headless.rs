//! Headless apps (no window) running the plugin on fixed 1/60 s frames. Skipped when the user's Reflex vehicle
//! file is not available (`REFLEX_VEHICLE`, default the stock mx450f JSON dump).

use std::time::Duration;

use bevy::prelude::*;
use bevy::time::{TimePlugin, TimeUpdateStrategy};
use reflex_bevy::{GroundQuery, Part, ReflexBike, ReflexBikeState, ReflexGround, ReflexInput, ReflexPart, ReflexPlugin, TriangleGround};

fn vehicle_path() -> Option<String> {
    let p = std::env::var("REFLEX_VEHICLE")
        .unwrap_or_else(|_| r"D:\reflex-tools\physics\reflex\dumps\stock-json\mx450f_vehicle.json".into());
    if std::path::Path::new(&p).exists() {
        Some(p)
    } else {
        eprintln!("skipped: no Reflex vehicle file at {p} (set REFLEX_VEHICLE)");
        None
    }
}

/// A flat square of ground at height 0, 400 m across.
fn flat() -> TriangleGround {
    let mut g = TriangleGround::new();
    let s = 200.0;
    let (a, b, c, d) = (Vec3::new(-s, 0.0, -s), Vec3::new(s, 0.0, -s), Vec3::new(s, 0.0, s), Vec3::new(-s, 0.0, s));
    // counter-clockwise seen from above
    g.add_triangle(a, d, c);
    g.add_triangle(a, c, b);
    g
}

fn app(ground: ReflexGround) -> App {
    let mut app = App::new();
    app.add_plugins((TaskPoolPlugin::default(), TimePlugin, TransformPlugin))
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)))
        .insert_resource(ground)
        .add_plugins(ReflexPlugin);
    app
}

fn run(app: &mut App, frames: usize) {
    for _ in 0..frames {
        app.update();
    }
}

fn spawn_bike(app: &mut App, path: &str, at: Transform) -> Entity {
    app.world_mut().spawn((ReflexBike::from_file(path).unwrap(), at)).id()
}

fn state(app: &App, e: Entity) -> ReflexBikeState {
    *app.world().get::<ReflexBikeState>(e).unwrap()
}

fn transform(app: &App, e: Entity) -> Transform {
    *app.world().get::<Transform>(e).unwrap()
}

#[test]
fn settles_upright_on_its_suspension() {
    let Some(path) = vehicle_path() else { return };
    let mut app = app(ReflexGround::from_triangles(flat()));
    let bike = spawn_bike(&mut app, &path, Transform::from_xyz(3.0, 0.0, -4.0));
    run(&mut app, 180);
    let t = transform(&app, bike);
    let s = state(&app, bike);
    eprintln!("chassis at {:?}, up {:?}, speed {:.3}", t.translation, t.rotation * Vec3::Y, s.speed);
    assert!((t.rotation * Vec3::Y).y > 0.99, "upright");
    assert!((0.45..0.75).contains(&t.translation.y), "chassis height {}", t.translation.y);
    assert!((t.translation.xz() - Vec2::new(3.0, -4.0)).length() < 0.2, "stayed at its spawn");
    assert!(s.speed < 0.3 && !s.airborne && s.wheels.iter().all(|w| w.contact));
    assert_eq!(s.gear, 1);
}

#[test]
fn throttle_drives_along_the_spawn_heading_and_steering_turns() {
    let Some(path) = vehicle_path() else { return };
    let mut app = app(ReflexGround::from_triangles(flat()));
    // facing +X: the spawn's -Z rotated a quarter turn clockwise seen from above
    let heading = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
    let bike = spawn_bike(&mut app, &path, Transform::from_rotation(heading));
    assert!((heading * Vec3::NEG_Z - Vec3::X).length() < 1e-5);
    run(&mut app, 60);
    let start = transform(&app, bike).translation;
    app.world_mut().get_mut::<ReflexInput>(bike).unwrap().throttle = 1.0;
    run(&mut app, 120);
    let s = state(&app, bike);
    let moved = transform(&app, bike).translation - start;
    eprintln!("after 2 s: moved {moved:?}, forward speed {:.1} m/s, gear {}, {:.0} rpm", s.forward_speed, s.gear, s.rpm);
    assert!(moved.x > 8.0 && moved.z.abs() < 1.0, "drove along +X");
    assert!(s.forward_speed > 8.0 && (s.velocity.normalize() - Vec3::X).length() < 0.1);
    let before = transform(&app, bike).rotation * Vec3::NEG_Z;
    {
        let mut i = app.world_mut().get_mut::<ReflexInput>(bike).unwrap();
        i.throttle = 0.4;
        i.steer = 1.0;
    }
    run(&mut app, 60);
    let after = transform(&app, bike).rotation * Vec3::NEG_Z;
    // turning right from +X heads toward +Z: the heading turns clockwise seen from above (cross product -Y)
    assert!(before.cross(after).y < -0.1, "full right steer turns right ({:?} -> {:?})", before, after);
    assert!(state(&app, bike).steer_angle > 0.0, "steer angle + = right");
}

#[test]
fn parts_follow_the_bike_as_children_and_in_world_space() {
    let Some(path) = vehicle_path() else { return };
    let mut app = app(ReflexGround::from_triangles(flat()));
    let bike = spawn_bike(&mut app, &path, Transform::from_xyz(0.0, 0.0, 0.0));
    let child = app.world_mut().spawn((ReflexPart { bike, part: Part::FrontWheel }, ChildOf(bike))).id();
    let free = app.world_mut().spawn(ReflexPart { bike, part: Part::FrontWheel }).id();
    let arm = app.world_mut().spawn(ReflexPart { bike, part: Part::SwingArm }).id();
    let hub = app.world_mut().spawn(ReflexPart { bike, part: Part::RearHub }).id();
    run(&mut app, 90);
    let world_wheel = transform(&app, free);
    let chassis = transform(&app, bike);
    let local = transform(&app, child);
    // the child's local transform composed with the chassis is the world one
    let composed = chassis.mul_transform(local);
    assert!((composed.translation - world_wheel.translation).length() < 1e-4);
    // the front wheel sits ahead of the chassis (-Z) and below it, at its radius above the ground
    let ahead = chassis.rotation.inverse() * (world_wheel.translation - chassis.translation);
    assert!(ahead.z < -0.4 && ahead.y < 0.0, "front wheel local {ahead:?}");
    let r = app.world().get::<ReflexBike>(bike).unwrap().wheel_radii();
    assert!((world_wheel.translation.y - r[0]).abs() < 0.05, "wheel centre at its radius: {} vs {}", world_wheel.translation.y, r[0]);
    // the swingarm's +Z points from the pivot at the rear axle
    let (a, h) = (transform(&app, arm), transform(&app, hub));
    let dir = (h.translation - a.translation).normalize();
    assert!((a.rotation * Vec3::Z - dir).length() < 1e-3, "swingarm +Z {:?} vs pivot->axle {dir:?}", a.rotation * Vec3::Z);
}

/// The game's own collision: an infinite plane at y = 0 through the query trait.
struct Plane;

impl GroundQuery for Plane {
    fn triangles(&self, c: Vec3, h: f32, out: &mut Vec<[Vec3; 3]>) {
        if c.y - h > 0.0 || c.y + h < 0.0 {
            return;
        }
        let s = h * 2.0;
        let (a, b, cc, d) = (
            Vec3::new(c.x - s, 0.0, c.z - s),
            Vec3::new(c.x + s, 0.0, c.z - s),
            Vec3::new(c.x + s, 0.0, c.z + s),
            Vec3::new(c.x - s, 0.0, c.z + s),
        );
        out.push([a, d, cc]);
        out.push([a, cc, b]);
    }

    fn raycast(&self, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)> {
        if (from.y > 0.0) == (to.y > 0.0) {
            return None;
        }
        let t = from.y / (from.y - to.y);
        Some((from.lerp(to, t), if from.y > 0.0 { Vec3::Y } else { Vec3::NEG_Y }))
    }
}

#[test]
fn ground_query_rides_like_triangles() {
    let Some(path) = vehicle_path() else { return };
    let mut a = app(ReflexGround::from_triangles(flat()));
    let mut b = app(ReflexGround::from_query(Plane));
    let ea = spawn_bike(&mut a, &path, Transform::IDENTITY);
    let eb = spawn_bike(&mut b, &path, Transform::IDENTITY);
    run(&mut a, 60);
    run(&mut b, 60);
    for (app, e) in [(&mut a, ea), (&mut b, eb)] {
        let mut i = app.world_mut().get_mut::<ReflexInput>(e).unwrap();
        i.throttle = 1.0;
        i.steer = 0.3;
    }
    run(&mut a, 120);
    run(&mut b, 120);
    let (pa, pb) = (transform(&a, ea).translation, transform(&b, eb).translation);
    eprintln!("triangles {pa:?}, query {pb:?}");
    assert!((pa - pb).length() < 0.05, "same ride on the same plane");
}

#[test]
fn respawn_input_puts_the_bike_back() {
    let Some(path) = vehicle_path() else { return };
    let mut app = app(ReflexGround::from_triangles(flat()));
    let bike = spawn_bike(&mut app, &path, Transform::from_xyz(10.0, 0.0, 0.0));
    assert!(state(&app, bike).frames == 0);
    run(&mut app, 1);
    assert!(state(&app, bike).teleported, "the spawn counts as a teleport");
    app.world_mut().get_mut::<ReflexInput>(bike).unwrap().throttle = 1.0;
    run(&mut app, 120);
    assert!(transform(&app, bike).translation.distance(Vec3::new(10.0, 0.6, 0.0)) > 5.0);
    {
        let mut i = app.world_mut().get_mut::<ReflexInput>(bike).unwrap();
        i.throttle = 0.0;
        i.respawn = true;
    }
    run(&mut app, 1);
    assert!(state(&app, bike).teleported);
    assert!(transform(&app, bike).translation.xz().distance(Vec2::new(10.0, 0.0)) < 0.1);
    assert!(state(&app, bike).speed < 0.5);
}

#[cfg(feature = "mesh")]
#[test]
fn rides_on_a_tagged_bevy_mesh() {
    use bevy::asset::{AssetPlugin, Assets};
    use bevy::mesh::{Mesh, Mesh3d, MeshPlugin};
    use reflex_bevy::mesh::{ReflexCollider, ReflexMeshGroundPlugin};

    let Some(path) = vehicle_path() else { return };
    let mut app = app(ReflexGround::default());
    app.add_plugins((AssetPlugin::default(), MeshPlugin, ReflexMeshGroundPlugin));
    let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::from(Plane3d::default().mesh().size(400.0, 400.0)));
    // a raised floor: the plane mesh moved up 2 m
    app.world_mut().spawn((Mesh3d(mesh), Transform::from_xyz(0.0, 2.0, 0.0), ReflexCollider::default()));
    let bike = spawn_bike(&mut app, &path, Transform::from_xyz(0.0, 2.0, 0.0));
    run(&mut app, 120);
    let t = transform(&app, bike);
    eprintln!("on the mesh: chassis at {:?}", t.translation);
    assert!((2.45..2.75).contains(&t.translation.y), "resting on the raised mesh, chassis y {}", t.translation.y);
    assert!(state(&app, bike).wheels.iter().all(|w| w.contact));
}
