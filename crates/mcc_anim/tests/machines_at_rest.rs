//! Machines put in their pose at rest (`place_machines`) stay where the
//! scenario placed them, give or take a wu: the Warthog run's islands once
//! dropped 56 wu out of sight when their `device:position` track (an
//! overlay) was read as whole values. Skips quietly when the map is not
//! installed.

use std::path::PathBuf;

fn map() -> Option<PathBuf> {
    let dir = game_paths::halo3_maps()?;
    let p = dir.join("120_halo.map");
    p.exists().then_some(p)
}

#[test]
fn machines_stay_where_they_are_placed() {
    let Some(path) = map() else { return };
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let mut scene = blam_geometry::Scene::default();
    let options = blam_geometry::Options {
        collision: false,
        ..Default::default()
    };
    blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
    let before: Vec<_> = scene.objects.iter().map(|o| o.transform.w_axis.truncate()).collect();
    mcc_anim::devices::place_machines(&cache, &mut scene);
    let mut islands = 0;
    for (o, was) in scene.objects.iter().zip(&before) {
        if o.kind != blam_geometry::Kind::Machine {
            continue;
        }
        let moved = o.transform.w_axis.truncate().distance(*was);
        assert!(moved < 1.0, "{:?} ({}) moved {moved} wu", o.name, o.tag_path);
        islands += usize::from(o.tag_path.ends_with("island_test"));
    }
    assert_eq!(islands, 4, "the run's four islands");
}
