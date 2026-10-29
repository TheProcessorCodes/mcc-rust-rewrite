//! Decodes the user's own Lockout (MCC Halo 2; Halo 3's Blackout shares the
//! file name, so the viewer runs that one as `blackout`) and pins its
//! numbers. HaloExportH2 has no export of Lockout, so these are our own
//! counts, checked by eye in `--shot` pictures. Skips quietly when the map is
//! not installed.

use std::path::PathBuf;

use blam_geometry::{CollisionSource, Kind};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO2_MAPS").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo2\h2_maps_win64_dx11")
    });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

#[test]
fn lockout_is_halo2() {
    let Some(path) = map("lockout") else { return };
    assert!(blam_h2::is_halo2(&path));
    let m = blam_h2::Map::open(&path).expect("opens");
    assert_eq!(m.map_name, "lockout");
    assert_eq!(m.scenario_path, "scenarios\\multi\\lockout\\lockout");
}

#[test]
fn lockout_counts() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let loaded = blam_h2::decode(&m, &blam_h2::Options::default()).expect("decodes");
    let scene = &loaded.scene;
    let tris = |kind: Kind| -> (usize, usize) {
        let ps: Vec<_> = scene.placements.iter().filter(|p| p.kind == kind).collect();
        (ps.len(), ps.iter().map(|p| scene.meshes[p.mesh as usize].triangle_count()).sum())
    };
    assert_eq!(tris(Kind::Cluster), (16, 7049));
    assert_eq!(tris(Kind::Instance), (75, 7414));
    assert_eq!(tris(Kind::Scenery), (5, 620));
    assert_eq!(tris(Kind::Crate), (15, 1296));
    assert_eq!(tris(Kind::Sky), (7, 234));
    assert!(loaded.stats.models_failed.is_empty(), "{:?}", loaded.stats.models_failed);
    assert!(loaded.stats.bitmaps_failed.is_empty(), "{:?}", loaded.stats.bitmaps_failed);
    assert_eq!(loaded.bitmaps.len(), 33);
    assert_eq!(scene.spawns.len(), 66);

    let col = &scene.collision;
    let world = col.triangles.iter().filter(|t| matches!(t.source, CollisionSource::Bsp(_))).count();
    assert_eq!((world, col.triangles.len() - world), (5570, 7312));
}
