//! Object collision models and damage sections on the Warthog run of
//! 120_halo (`docs/format/halo-script.md` 7, 8). Skips quietly when the map
//! is not installed.

use std::path::PathBuf;

use blam_geometry::{CollisionSource, Kind, Options, objects};
use glam::Vec3;

fn path(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps")
    });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

const PLATE: &str = r"objects\levels\solo\120_halo\plates\plate_flat\plate_flat";

#[test]
fn plate_collision_and_damage_sections() {
    let Some(p) = path("120_halo") else { return };
    let cache = blam_cache::CacheFile::open(p).unwrap();
    let plate = cache.tags.find("scen", PLATE).expect("plate_flat");
    let tris = objects::collision_model(&cache, plate).unwrap();
    // 10 convex surfaces of a slab: fanned into 2 triangles each, at most.
    assert!(!tris.is_empty() && tris.len() <= 20, "{} triangles", tris.len());
    let (lo, hi) = objects::bounds(&tris).unwrap();
    let size = hi - lo;
    // A floor tile: wide and thin.
    assert!(size.x > 1.0 && size.y > 1.0 && size.z < size.x, "plate box {lo} .. {hi}");

    let sections = objects::damage_sections(&cache, plate).unwrap();
    let names: Vec<&str> = sections.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["destroy", "fall", "fall_immediate"]);
    let effect = |i: Option<u32>| i.and_then(|i| cache.tags.get(i)).map(|t| t.name().to_string());
    assert_eq!(
        effect(sections[1].responses[0].effect).as_deref(),
        Some(r"objects\levels\solo\120_halo\plates\plate_flat\plate_flat_fall")
    );
    assert_eq!(
        effect(sections[0].responses[0].effect2).as_deref(),
        Some(r"levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion")
    );
    assert_eq!(sections[1].responses[0].unknown_60, 1.0);
    assert_eq!(sections[2].responses[0].unknown_60, 0.0);
}

/// The Warthog run's floor is the plates: under plate `100_001a` the
/// structure BSP has no floor of its own, so once the plate's collision is
/// gone the player drops.
#[test]
fn warthog_run_floor_is_the_plates() {
    let Some(p) = path("120_halo") else { return };
    let options = Options {
        bsps: Some(vec![7]),
        ..Options::default()
    };
    let cache = blam_cache::CacheFile::open(&p).unwrap();
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let plate = scene
        .objects
        .iter()
        .position(|o| o.name.as_deref() == Some("100_001a"))
        .expect("100_001a placed");
    let o = &scene.objects[plate];
    assert_eq!(o.kind, Kind::Scenery);
    assert_eq!(o.folder, Some(1));
    assert_eq!(o.bsps, 1 << 7);
    assert!(!o.starts_absent());
    let centre = o.transform.transform_point3(Vec3::ZERO);

    let added = objects::add_collision(&cache, &mut scene);
    assert!(added > 1000, "{added} object collision triangles");
    // Straight down from above the plate's centre: what is hit first?
    let down = |t: &blam_geometry::CollisionTriangle| -> Option<f32> {
        let [a, b, c] = t.points;
        let origin = centre + Vec3::Z * 2.0;
        let dir = -Vec3::Z;
        let e1 = b - a;
        let e2 = c - a;
        let pv = dir.cross(e2);
        let det = e1.dot(pv);
        if det.abs() < 1e-9 {
            return None;
        }
        let s = origin - a;
        let u = s.dot(pv) / det;
        let q = s.cross(e1);
        let v = dir.dot(q) / det;
        let d = e2.dot(q) / det;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && d > 0.0).then_some(d)
    };
    let mut hits: Vec<(f32, CollisionSource)> = scene
        .collision
        .triangles
        .iter()
        .filter_map(|t| down(t).map(|d| (d, t.source)))
        .collect();
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    let first = hits.first().expect("something under the plate");
    assert_eq!(first.1.object(), Some(plate as u32), "first hit {hits:?}");
    // No BSP surface within 20 wu under it.
    assert!(
        hits.iter().all(|(d, s)| s.object().is_some() || *d > 20.0),
        "BSP floor under the plate: {hits:?}"
    );
}
