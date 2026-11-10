//! Halo 3's man cannons and gravity lifts read from the placed objects'
//! physics models (`objects::lifts`, `docs/format/halo2.md` 12.4). Skips
//! quietly when a map is not installed.

use std::path::PathBuf;

use blam_geometry::{LiftShape, Options, lift, objects};
use glam::Vec3;

fn scene(name: &str) -> Option<(blam_cache::CacheFile, blam_geometry::Scene)> {
    let p: PathBuf = game_paths::halo3_maps()?.join(format!("{name}.map"));
    if !p.exists() {
        return None;
    }
    let cache = blam_cache::CacheFile::open(&p).unwrap();
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &Options::default(), &mut scene).unwrap();
    Some((cache, scene))
}

/// Valhalla's man cannons: a box each on the pad, throwing a player 60
/// wu/s^2 up to 11 wu/s along the `fire` marker, out over the map and
/// upwards; the vehicle-only phantom is left out.
#[test]
fn valhalla_man_cannons() {
    let Some((cache, scene)) = scene("riverworld") else { return };
    let lifts = objects::lifts(&cache, &scene);
    let cannons: Vec<_> = lifts.iter().filter(|l| l.name.starts_with("man_cannon_river ")).collect();
    assert!(cannons.len() >= 2, "{lifts:#?}");
    for c in &cannons {
        assert_eq!(c.push.direction, (60.0, 11.0), "{c:?}");
        assert!(c.push.ignores_vehicles);
        assert!(matches!(c.shape, LiftShape::Box { .. }));
        // Out and up.
        assert!(c.direction.z > 0.2 && c.direction.z < 0.95, "{c:?}");
        // A body standing on the pad is pushed along it.
        let LiftShape::Box { centre, .. } = c.shape else { unreachable!() };
        let (a, _) = lift::acceleration(&lifts, centre, Vec3::ZERO, 1.0 / 60.0, 0.0, false).expect("on the pad");
        assert!(a.dot(c.direction) > 50.0, "{a} along {}", c.direction);
    }
}

/// The equipment and vehicles that carry phantoms but are not lifts for a
/// player (bubble shields, Phantom dropships) are never placed as lifts,
/// and every map's volumes read without error.
#[test]
fn multiplayer_lifts_read() {
    for name in ["construct", "guardian", "isolation", "chill"] {
        let Some((cache, scene)) = scene(name) else { continue };
        let lifts = objects::lifts(&cache, &scene);
        assert!(!lifts.is_empty(), "{name}: no lifts");
        for l in &lifts {
            assert!(l.direction.is_finite() && (l.direction.length() - 1.0).abs() < 1e-3, "{name}: {l:?}");
        }
    }
}
