//! Halo: Reach maps in the viewer. `blam_reach` decodes the map into the
//! same `Scene` as Halo 3's, and the Halo 3 path does the rest: the
//! shaders and bitmaps are Halo 3's chain at Reach's offsets
//! (`blam_texture`), so materials resolve and zone sets stream their
//! bitmaps from the open cache file exactly as for a Halo 3 map.
//!
//! What Reach does not get yet (`docs/format/reach.md` 10): its lightmaps
//! (dual vMF, every placement uses the fallback light), camera fx and
//! atmosphere, its sound, its weapons and movement numbers (the rider is
//! Halo 3's Spartan borrowed from Valhalla, on Valhalla's movement).

use crate::{LoadedMap, resolve_map};
use crate::{rider, textures, world};

/// Decodes the map's BSPs, collision and objects into `scene`.
pub fn decode(cache: &blam_cache::CacheFile, scene: &mut blam_geometry::Scene) -> anyhow::Result<()> {
    let stats = blam_reach::decode_into(cache, scene)?;
    for (index, name, s) in &stats.bsps {
        eprintln!(
            "reach bsp {index} {name}: {} clusters ({} drawn), {} instances ({} drawn), {} meshes ({} with geometry), {} shaders",
            s.clusters,
            s.cluster_placements,
            s.instances,
            s.instance_placements,
            s.meshes,
            s.meshes_with_geometry,
            s.shaders
        );
    }
    let mut kinds: Vec<String> = stats.objects.placed.iter().map(|(k, n)| format!("{n} {k:?}")).collect();
    kinds.sort();
    eprintln!(
        "reach: {} collision triangles; objects {} ({} skipped, {} models failed); {} spawn points, {} starting locations",
        stats.collision_triangles,
        kinds.join(", "),
        stats.objects.skipped,
        stats.objects.models_failed.len(),
        scene.spawns.len(),
        scene.starts.len()
    );
    for f in &stats.objects.models_failed {
        eprintln!("  model failed: {f}");
    }
    Ok(())
}

/// The rest of a Reach map's load, once its materials are resolved.
pub fn finish(
    cache: &'static blam_cache::CacheFile,
    scene: blam_geometry::Scene,
    textures: textures::MaterialTextures,
) -> LoadedMap {
    // The rider is Halo 3's Spartan, borrowed from Valhalla when Halo 3 is
    // installed (Reach's Spartans are not decoded).
    let chief = blam_cache::CacheFile::open(resolve_map("riverworld"))
        .ok()
        .and_then(|c| rider::load_chief(&c));
    let sky = world::sky::load(cache, &scene);
    let water = world::water::load(cache, &scene);
    LoadedMap {
        scene,
        textures,
        chief,
        sky,
        water,
        map_file: Some(cache),
        arsenal: None,
        foot_tuning: None,
        plain_fog: None,
        plain_sound: None,
    }
}
