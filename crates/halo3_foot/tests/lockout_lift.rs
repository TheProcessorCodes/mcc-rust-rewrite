//! The Spartan stepped into Lockout's gravity lift on the real map's
//! collision, headless: he should ride the shaft up and come out on a
//! higher floor. Skips quietly when the map is not installed
//! (`HALO2_MAPS` overrides the folder).

use std::path::PathBuf;

use bevy::math::Vec3;
use halo3_core::coords::{to_halo, to_render};
use halo3_core::lifts::Lifts;
use halo3_foot::physics::{Controls, Spartan};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO2_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo2\h2_maps_win64_dx11")
    });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

#[test]
fn rides_lockouts_lift_to_the_top() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let options = blam_h2::Options {
        textures: false,
        ..Default::default()
    };
    let scene = blam_h2::decode(&m, &options).expect("decodes").scene;
    let world = halo3_bike::build_world(&scene.collision, u32::MAX);
    let lifts = Lifts(scene.lifts.clone());
    let field = |p: Vec3, v: Vec3, dt: f32, r: f32| lifts.push(p, v, dt, r, false);

    // Dropped on the pad at the lift's foot.
    let mut s = Spartan::dropped(&world, to_render(Vec3::new(-1.5, 12.53, -11.5)), 0.0);
    let start = to_halo(s.position);
    assert!((start.z + 12.2).abs() < 0.3, "pad at {start}");
    let dt = 1.0 / 60.0;
    let mut peak = start.z;
    let mut trace = Vec::new();
    for k in 0..(6.0 / dt) as usize {
        s.step_in(&world, Controls::default(), dt, &field);
        let h = to_halo(s.position);
        peak = peak.max(h.z);
        if k % 15 == 0 {
            trace.push(format!("{:.2}s [{:.2}, {:.2}, {:.2}]{}", k as f32 * dt, h.x, h.y, h.z, if s.grounded { " ground" } else { "" }));
        }
    }
    let end = to_halo(s.position);
    eprintln!("{}", trace.join("\n"));
    eprintln!("start {start}, peak z {peak:.2}, end {end} grounded {}", s.grounded);
    // Up the shaft and out on a floor well above the pad, off the lift.
    assert!(peak > start.z + 5.0, "peak {peak}");
    assert!(s.grounded && end.z > start.z + 4.0, "ended at {end}");
    assert!(!scene.lifts.iter().any(|l| l.touches(end, 0.0)), "still in the lift at {end}");
}
