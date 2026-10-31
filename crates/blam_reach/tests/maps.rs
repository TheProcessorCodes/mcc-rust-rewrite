//! Decodes the user's MCC Reach multiplayer maps, when installed, and pins
//! what each holds (MCC update 13, build `Jun 21 2023 15:35:31`). Each was
//! also drawn headlessly (`halo3 hr:NAME --shot`) and looked at.

use std::path::PathBuf;

/// The player's Reach maps (`game_paths`); empty before setup, so the
/// tests skip.
fn maps_dir() -> PathBuf {
    game_paths::reach_maps().unwrap_or_default()
}

/// (map, meshes, placements, drawn triangles, collision triangles, spawn points).
const MAPS: [(&str, usize, usize, usize, usize, usize); 20] = [
    ("dlc_slayer", 889, 3995, 1299675, 321135, 164),
    ("cex_beaver_creek", 428, 1076, 568004, 430824, 238),
    ("50_panopticon", 1063, 7069, 2139245, 547584, 242),
    ("70_boneyard", 844, 7904, 1925517, 678258, 339),
    ("cex_headlong", 762, 2537, 860731, 420752, 396),
    ("dlc_invasion", 684, 3915, 1325931, 330346, 287),
    ("condemned", 993, 2417, 1357824, 475037, 251),
    ("45_launch_station", 575, 8563, 1711444, 911340, 227),
    ("forge_halo", 171, 2292, 1088220, 910858, 19),
    ("trainingpreserve", 895, 3340, 1901349, 383454, 315),
    ("cex_hangemhigh", 828, 1181, 583705, 376794, 300),
    ("cex_damnation", 475, 895, 685622, 383940, 235),
    ("30_settlement", 491, 3140, 1015072, 375752, 246),
    ("52_ivory_tower", 449, 2554, 1031454, 184082, 221),
    ("cex_timberland", 457, 1016, 825079, 287246, 335),
    ("cex_prisoner", 495, 1348, 755894, 316944, 304),
    ("35_island", 398, 2730, 1494257, 788304, 362),
    ("20_sword_slayer", 582, 1881, 840749, 457190, 203),
    ("dlc_medium", 300, 1512, 609073, 301710, 240),
    ("45_aftship", 264, 687, 389763, 270804, 184),
];

#[test]
fn every_multiplayer_map_decodes() {
    let dir = maps_dir();
    if !dir.join("20_sword_slayer.map").exists() {
        return;
    }
    for (name, meshes, placements, triangles, collision, spawns) in MAPS {
        let path = dir.join(format!("{name}.map"));
        assert!(blam_reach::is_reach(&path), "{name} is not recognised as Reach");
        let cache = blam_cache::CacheFile::open(&path).unwrap();
        let mut scene = blam_geometry::Scene::default();
        let stats = blam_reach::decode_into(&cache, &mut scene).unwrap_or_else(|e| panic!("{name}: {e}"));
        let per: Vec<String> = stats
            .bsps
            .iter()
            .map(|(i, _, s)| format!("bsp{i} {}c/{}i of {}m, {} lit", s.cluster_placements, s.instance_placements, s.meshes, s.lightmapped))
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
        assert_eq!(
            (scene.meshes.len(), scene.placements.len(), scene.triangle_count(), stats.collision_triangles, scene.spawns.len()),
            (meshes, placements, triangles, collision, spawns),
            "{name}"
        );
        assert!(stats.objects.models_failed.is_empty(), "{name}");
        assert!(stats.bsps[0].2.lightmapped > 0, "{name}: the main BSP has no lightmapped placement");
    }
}

/// Sword Base: the header, the instance and collision resources and a
/// Reach bitmap, against `docs/format/reach.md`.
#[test]
fn sword_base_pieces() {
    let path = maps_dir().join("20_sword_slayer.map");
    if !path.exists() {
        return;
    }
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    assert_eq!(cache.engine(), blam_cache::Engine::Reach);
    assert_eq!(cache.header.section_file_base(blam_cache::header::SECTION_DEBUG), 0xA000);
    assert_eq!(cache.tags.len(), 10462);
    let resources = cache.resources().unwrap();
    let bsps = blam_reach::sbsp::structure_bsps(&cache).unwrap();
    assert_eq!(bsps.len(), 3);
    let instances = blam_reach::sbsp::instances(&cache, &resources, bsps[0].sbsp).unwrap();
    assert_eq!(instances.len(), 1795);
    assert_eq!(instances.iter().filter(|i| i.flags & blam_reach::sbsp::Instance::RENDER_ONLY != 0).count(), 372);
    let lightmap = cache
        .tags
        .find("bitm", r"levels\multi\20_sword_slayer\20_sword_slayer_lightmap_20_sword_slayer_16f_lp_array_dualvmf_pixel_intensity_dxt5")
        .unwrap();
    let t = blam_texture::bitmap::decode(&cache, &resources, lightmap.index, 0).unwrap();
    assert_eq!((t.width, t.height, t.layers, t.format), (1536, 1536, 3, blam_texture::Format::Bc3));
}
