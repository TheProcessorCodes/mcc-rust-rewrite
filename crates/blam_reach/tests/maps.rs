//! Decodes the user's MCC Reach multiplayer maps, when installed, and
//! prints what each holds (counts get pinned once checked).

use std::path::PathBuf;

fn maps_dir() -> PathBuf {
    std::env::var_os("HALOREACH_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\haloreach\maps")
    })
}

pub const MAPS: [&str; 20] = [
    "dlc_slayer", "cex_beaver_creek", "50_panopticon", "70_boneyard", "cex_headlong", "dlc_invasion", "condemned",
    "45_launch_station", "forge_halo", "trainingpreserve", "cex_hangemhigh", "cex_damnation", "30_settlement",
    "52_ivory_tower", "cex_timberland", "cex_prisoner", "35_island", "20_sword_slayer", "dlc_medium", "45_aftship",
];

#[test]
fn every_multiplayer_map_decodes() {
    let dir = maps_dir();
    if !dir.join("20_sword_slayer.map").exists() {
        return;
    }
    for name in MAPS {
        let path = dir.join(format!("{name}.map"));
        assert!(blam_reach::is_reach(&path), "{name} is not recognised as Reach");
        let cache = blam_cache::CacheFile::open(&path).unwrap();
        let mut scene = blam_geometry::Scene::default();
        let stats = blam_reach::decode_into(&cache, &mut scene).unwrap_or_else(|e| panic!("{name}: {e}"));
        let per: Vec<String> = stats
            .bsps
            .iter()
            .map(|(i, _, s)| format!("bsp{i} {}c/{}i of {}m", s.cluster_placements, s.instance_placements, s.meshes))
            .collect();
        println!(
            "{name}: {} meshes, {} placements, {} triangles, {} collision, {} spawns, {} starts, {} materials, {:?}, objects {:?} skipped {} failed {:?}",
            scene.meshes.len(),
            scene.placements.len(),
            scene.triangle_count(),
            stats.collision_triangles,
            scene.spawns.len(),
            scene.starts.len(),
            scene.materials.len(),
            per,
            stats.objects.placed,
            stats.objects.skipped,
            stats.objects.models_failed
        );
        assert!(scene.triangle_count() > 0 && stats.collision_triangles > 0, "{name}: empty");
    }
}
