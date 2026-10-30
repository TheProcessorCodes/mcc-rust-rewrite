//! Decodes the user's own Lockout (MCC Halo 2; Halo 3's Blackout shares the
//! file name, so the viewer runs that one as `blackout`) and pins its
//! numbers. HaloExportH2 has no export of Lockout, so these are our own
//! counts (`docs/format/scripts/probe_h2_lightmap.py` for the lightmap),
//! checked by eye in `--shot` pictures. Skips quietly when the map is
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
    // Prop sizes: render model positions are stored -1..1 and stretched
    // over the model's compression bounds. Unstretched, every prop was a
    // 2 wu cube's worth (giant rocks on the rooftops).
    let size = |name: &str| {
        let p = scene.placements.iter().find(|p| p.name.starts_with(name)).expect(name);
        let m = &scene.meshes[p.mesh as usize];
        let (lo, hi) = m.positions.iter().fold((glam::Vec3::MAX, glam::Vec3::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        hi - lo
    };
    let power = size("power_module");
    assert!((power.x - 0.256).abs() < 1e-3 && (power.z - 0.8192).abs() < 1e-3, "{power}");
    let flag = size("flag_base");
    assert!((flag.x - 0.44).abs() < 1e-3 && (flag.z - 0.05).abs() < 1e-3, "{flag}");
    // The lift's crate is a 0.2 wu grey marker tucked under the pad.
    assert!(size("lockout_lift_tube").max_element() < 0.21);
    assert!(loaded.stats.models_failed.is_empty(), "{:?}", loaded.stats.models_failed);
    assert!(loaded.stats.bitmaps_failed.is_empty(), "{:?}", loaded.stats.bitmaps_failed);
    assert_eq!(loaded.bitmaps.len(), 33);
    assert_eq!(scene.spawns.len(), 66);

    // Lightmaps: 16 cluster pages and 49 instance pages in one atlas; the
    // 26 light fixtures have none (lit from the nearest surface).
    assert_eq!((loaded.stats.lightmap_pages, loaded.stats.lightmapped), (65, 65));
    assert_eq!(loaded.lightmaps.len(), 1);
    let lit = |kind: Kind| {
        scene
            .placements
            .iter()
            .filter(|p| p.kind == kind && matches!(p.lighting, blam_geometry::Lighting::PerPixel { .. }))
            .count()
    };
    assert_eq!((lit(Kind::Cluster), lit(Kind::Instance)), (16, 49));
    for p in &scene.placements {
        if let blam_geometry::Lighting::PerPixel { uvs, .. } = &p.lighting {
            assert_eq!(uvs.len(), scene.meshes[p.mesh as usize].positions.len(), "{}", p.name);
            assert!(uvs.iter().flatten().all(|c| (0.0..=1.0).contains(c)), "{}", p.name);
        }
    }
    // The sky's fog: Lockout's blue mist.
    let fog = loaded.fog.expect("sky fog");
    assert!((fog.colour[0] - 0.510).abs() < 1e-3 && (fog.colour[2] - 0.800).abs() < 1e-3);
    assert_eq!((fog.max_density, fog.start, fog.opaque), (0.5, 0.0, 40.0));

    let col = &scene.collision;
    let world = col.triangles.iter().filter(|t| matches!(t.source, CollisionSource::Bsp(_))).count();
    assert_eq!((world, col.triangles.len() - world), (5570, 7312));
}
