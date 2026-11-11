//! The point-to-cluster lookup of Reach BSPs (`docs/format/sound.md` 12.5):
//! the collision tree and the BSP's leaf clusters, for both tree formats.
//! Skipped when Reach is not installed.

use std::collections::BTreeMap;

use glam::Vec3;

fn scene(name: &str) -> Option<blam_geometry::Scene> {
    let path = game_paths::reach_maps()?.join(format!("{name}.map"));
    if !path.exists() {
        return None;
    }
    let cache = blam_cache::CacheFile::open(&path).unwrap();
    let mut scene = blam_geometry::Scene::default();
    blam_reach::decode_into(&cache, &mut scene).unwrap();
    Some(scene)
}

/// How many spawn points (head height) land in a cluster with each
/// acoustics entry; `None`: in no cluster.
fn spawn_acoustics(scene: &blam_geometry::Scene) -> BTreeMap<Option<u32>, usize> {
    let bsp = &scene.bsps[0];
    let locator = bsp.locator.as_ref().expect("cluster lookup");
    let mut out = BTreeMap::new();
    for s in &scene.spawns {
        let acoustics = locator
            .cluster(s.position + Vec3::Z * 0.6)
            .and_then(|c| bsp.clusters.get(c as usize))
            .and_then(|c| c.acoustics);
        *out.entry(acoustics).or_insert(0) += 1;
    }
    out
}

#[test]
fn sword_base_spawns_are_indoors() {
    let Some(scene) = scene("20_sword_slayer") else { return };
    let bsp = &scene.bsps[0];
    let locator = bsp.locator.as_ref().expect("cluster lookup");
    assert_eq!(locator.leaf_clusters.len(), 3308);
    assert!(locator.large_nodes.is_empty());
    // 16 clusters are `interior` (1), two `interior_tight` (5), one
    // `interior_large` (2).
    let count = |a: u32| bsp.clusters.iter().filter(|c| c.acoustics == Some(a)).count();
    assert_eq!((bsp.clusters.len(), count(1), count(5), count(2)), (19, 16, 2, 1));
    let found = spawn_acoustics(&scene);
    println!("sword base spawn acoustics: {found:?}");
    // Reach's BSPs are not sealed: the tree has no leaf for much of the
    // open space, so only some spawn points land in a cluster.
    let placed: usize = found.iter().filter(|(a, _)| a.is_some()).map(|(_, n)| n).sum();
    assert!(placed >= 40, "{found:?}");
    assert!(found.keys().flatten().all(|a| [1, 2, 5].contains(a)), "{found:?}");
    // Far below the map: no cluster.
    assert_eq!(locator.cluster(Vec3::new(0.0, 0.0, -500.0)), None);
}

#[test]
fn a_large_collision_tree_finds_clusters() {
    // Timberland keeps its structure collision in a large collision BSP.
    let Some(scene) = scene("cex_timberland") else { return };
    let bsp = &scene.bsps[0];
    let locator = bsp.locator.as_ref().expect("cluster lookup");
    assert!(locator.nodes.is_empty());
    assert_eq!(locator.leaf_clusters.len(), 61906);
    let found = spawn_acoustics(&scene);
    println!("timberland spawn acoustics: {found:?}");
    // Every cluster is `exterior` (1).
    assert_eq!(found.get(&Some(1)).copied().unwrap_or(0) * 10 / scene.spawns.len().max(1), 10, "{found:?}");
}
