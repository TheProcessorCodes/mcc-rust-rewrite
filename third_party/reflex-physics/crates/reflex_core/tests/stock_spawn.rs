//! Spawn-and-ride sanity check with a stock vehicle file from the user's own Reflex (not shipped). Set
//! `REFLEX_STOCK_VEHICLE` to a stock JSON / XML vehicle dump to run it; without it the test does nothing.

use reflex_core::bike::{CHASSIS, RIDER};
use reflex_core::{Inputs, PlaneGround, Quat, Sim, Vec3};

fn stock() -> Option<Sim> {
    let path = std::env::var_os("REFLEX_STOCK_VEHICLE")?;
    let sim = Sim::from_file(path, Vec3 { x: 0.0, y: 3.0, z: 0.0 }, Quat::IDENTITY).expect("vehicle file");
    Some(sim)
}

#[test]
fn spawn_settles_then_rides() {
    let Some(mut s) = stock() else { return };
    let ground = PlaneGround { point: Vec3::ZERO, normal: Vec3::Y };
    // the brake-hold latch keeps the brakes on: the dropped bike comes to rest
    for _ in 0..240 {
        s.frame(1.0 / 60.0, Inputs::default(), &ground);
    }
    let b0 = &s.sys.bodies[CHASSIS];
    eprintln!("after 4 s: y {:.3} |v| {:.4} rest {} crashed {}", b0.x.y, b0.v.length(), s.at_rest, s.crashed);
    // at rest the chassis still moves along the stored ground normal (the suspension works)
    assert!(!s.crashed && s.at_rest && b0.v.length() < 0.01);
    let weld = b0.x + b0.q.rotate(s.params.rider_com);
    assert!((s.sys.bodies[RIDER].x - weld).length() < 1e-9);
    for _ in 0..180 {
        s.frame(1.0 / 60.0, Inputs { throttle: 0.6, ..Inputs::default() }, &ground);
    }
    let v = s.forward_speed();
    eprintln!("after 3 s at 60 % throttle: {v:.2} ft/s, crashed {}", s.crashed);
    assert!(!s.crashed && !s.at_rest && v > 10.0);
}
