//! The reflection cube maps' tags and capture points
//! (`docs/format/cubemaps.md` 1 and 2), on the user's own maps. Skips
//! quietly when a map is not installed.

use std::collections::BTreeSet;

use blam_geometry::Options;

fn scene(name: &str) -> Option<blam_geometry::Scene> {
    let path = game_paths::halo3_map(name)?;
    let options = Options {
        objects: false,
        collision: false,
        ..Options::default()
    };
    Some(blam_geometry::load_map_with(&path, &options).expect("map decodes"))
}

#[test]
fn valhalla_cubes() {
    let Some(scene) = scene("riverworld") else { return };
    let bsp = &scene.bsps[0];
    assert!(bsp.cubemap_bitmap.is_some(), "riverworld_riverworld_cubemaps");
    assert_eq!(bsp.cubemaps.len(), 61);
    let clusters: BTreeSet<u32> = bsp.cubemaps.iter().map(|p| p.cluster).collect();
    assert_eq!(clusters.len(), 26, "every cluster lists a cube");
    let cubes: BTreeSet<u32> = bsp.cubemaps.iter().map(|p| p.cube).collect();
    assert_eq!((cubes.len(), cubes.last().copied()), (55, Some(54)));
    // On riverworld the scenario-wide number is the bitmap.
    assert!(bsp.cubemaps.iter().all(|p| p.id == p.cube));
    let c2: Vec<u32> = bsp.cubemaps.iter().filter(|p| p.cluster == 2).map(|p| p.cube).collect();
    assert_eq!(c2, [23, 27, 53, 54]);
    let p = bsp.cubemaps.iter().find(|p| p.cube == 23).unwrap().position;
    assert!((p.x - 84.75).abs() < 0.01 && (p.y - -102.22).abs() < 0.01 && (p.z - 5.83).abs() < 0.01);
}

#[test]
fn halo_cubes_number_across_bsps() {
    let Some(scene) = scene("120_halo") else { return };
    // BSP 0's tag holds 6 cubes; its points name them 0..5 locally while
    // their scenario numbers run to 71, and cube 70 is shared with BSP 7.
    let b0 = scene.bsps.iter().find(|b| b.index == 0).unwrap();
    let local: BTreeSet<u32> = b0.cubemaps.iter().map(|p| p.cube).collect();
    assert_eq!(local, (0..6).collect());
    let ids: BTreeSet<u32> = b0.cubemaps.iter().map(|p| p.id).collect();
    assert_eq!(ids, [2, 3, 4, 5, 70, 71].into_iter().collect());
    let b7 = scene.bsps.iter().find(|b| b.index == 7).unwrap();
    assert!(b7.cubemaps.iter().any(|p| p.id == 70));
    // 120_bsp_050 has no cube map tag.
    let b5 = scene.bsps.iter().find(|b| b.index == 5).unwrap();
    assert!(b5.cubemap_bitmap.is_none() && b5.cubemaps.is_empty());
}
