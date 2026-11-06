//! Reflex's own bike simulation (`exact.rs`) on a real Halo 3 map's
//! collision: Valhalla's open ground. The bike is dropped on flat ground
//! near the middle, settles on both wheels, then rides on full gas without
//! sinking through the ground or crashing.
//!
//! Skips quietly without MCC's `riverworld.map` or without a Reflex install.

use bevy::math::Vec3;
use halo3_bike::exact;
use halo3_bike::physics::{Bike, Controls, World};

fn step(world: &World, bike: &mut Bike, c: Controls, seconds: f32, lowest: &mut f32) {
    for _ in 0..(seconds * 60.0) as usize {
        bike.step(world, c, 1.0 / 60.0);
        // How far the model's origin came down to the ground under it.
        if let Some(h) = world.raycast_far(bike.position + Vec3::Y, Vec3::NEG_Y, 6.0) {
            *lowest = lowest.min(bike.position.y - h.point.y);
        }
    }
}

#[test]
fn rides_valhalla() {
    let Some(path) = game_paths::halo3_map("riverworld") else {
        eprintln!("no riverworld.map; skipped");
        return;
    };
    if exact::vehicle().is_none() {
        eprintln!("no Reflex install; skipped");
        return;
    }
    let scene = blam_geometry::load_map(&path).expect("riverworld");
    let world = halo3_bike::build_world(&scene.collision, u32::MAX);

    // Flat open ground with room ahead (+Z), searched out from the middle.
    let (lo, hi) = scene.collision.triangles.iter().flat_map(|t| t.points).fold(
        (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(p), hi.max(p)),
    );
    let (lo, hi) = (halo3_core::coords::to_render(lo), halo3_core::coords::to_render(hi));
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let middle = (lo + hi) * 0.5;
    let flat = |x: f32, z: f32| {
        let h = world.raycast_far(Vec3::new(x, hi.y + 5.0, z), Vec3::NEG_Y, hi.y - lo.y + 10.0)?;
        (h.normal.y > 0.97).then_some(h.point)
    };
    let start = (0..400)
        .map(|i| {
            let (r, a) = (i as f32 * 0.5, i as f32 * 2.4);
            (middle.x + r * a.cos(), middle.z + r * a.sin())
        })
        .find_map(|(x, z)| {
            let p = flat(x, z)?;
            // 25 m of flat-ish ground straight ahead.
            (1..=5).all(|k| flat(x, z + k as f32 * 5.0).is_some_and(|q| (q.y - p.y).abs() < 1.5)).then_some(p)
        })
        .expect("no open ground on Valhalla");
    eprintln!("start {start:.2?}");

    let mut bike = Bike::dropped(&world, start + Vec3::Y, 0.0);
    assert!(bike.exact().is_none(), "Reflex physics is off until the tuning menu picks it");
    exact::set_on(true);
    let mut lowest = f32::MAX;
    step(&world, &mut bike, Controls::default(), 2.0, &mut lowest);
    assert!(bike.exact().is_some(), "the switch did not reach the bike");
    let settled = bike.position;
    eprintln!("settled at {settled:.2?}, {} wheels down, up {:.3?}", bike.grounded(), bike.up());
    assert_eq!(bike.grounded(), 2);
    assert!(bike.up().y > 0.95);

    let gas = Controls {
        throttle: 1.0,
        ..Default::default()
    };
    step(&world, &mut bike, gas, 2.5, &mut lowest);
    let moved = bike.position - settled;
    eprintln!(
        "2.5 s of gas: moved {moved:.2?} at {:.1} m/s, gear {}, lowest {lowest:.2} m over the ground, crashed {:?}",
        bike.speed(),
        bike.engine.gear + 1,
        bike.crashed
    );
    assert!(moved.length() > 10.0, "moved only {moved:?}");
    assert!(lowest > 0.3, "sank to {lowest:.2} m over the ground");
    assert!(bike.crashed.is_none());
    exact::set_on(false);
}
