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

/// The gravity lift (`docs/format/halo2.md` 11): the crate
/// `lockout_lift_tube` at (-1.5, 12.53, -12.2), tilted 7 degrees by its
/// roll, carries two phantom boxes from its physics model. The lower one
/// (4.5 wu tall over the pad) lifts at 10 wu/s^2 to 4 wu/s with gravity off
/// and pulls to the axis; the upper one (2 wu over that) pushes out along
/// `upper_lift_direction`, 30 degrees below level towards -Y.
#[test]
fn lockout_lift() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let options = blam_h2::Options {
        textures: false,
        ..Default::default()
    };
    let loaded = blam_h2::decode(&m, &options).expect("decodes");
    let lifts = &loaded.scene.lifts;
    let names: Vec<&str> = lifts.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["lockout_lift_tube 0 lower_lift", "lockout_lift_tube 0 upper_lift"]);
    let near = |a: glam::Vec3, b: [f32; 3]| (a - glam::Vec3::from(b)).length() < 0.01;
    let boxed = |l: &blam_geometry::LiftVolume| match l.shape {
        blam_geometry::LiftShape::Box { centre, axes, half } => (centre, axes, half),
        other => panic!("{other:?}"),
    };

    let (lower, upper) = (&lifts[0], &lifts[1]);
    let (centre, axes, half) = boxed(lower);
    assert!(near(centre, [-1.774, 12.53, -9.967]), "{centre}");
    assert!(near(half, [0.605, 0.605, 2.25]), "{half}");
    assert!(near(axes[2], [-0.1217, 0.0, 0.9926]), "{:?}", axes[2]);
    assert!(near(lower.direction, [-0.1217, 0.0, 0.9926]), "{}", lower.direction);
    assert!(near(lower.origin, [-1.5, 12.52, -12.2]), "{}", lower.origin);
    assert_eq!((lower.push.direction, lower.push.axis), ((10.0, 4.0), (10.0, 10.0)));
    assert!(lower.push.negates_gravity && !lower.push.ignores_vehicles);

    let (centre, _, half) = boxed(upper);
    assert!(near(centre, [-2.166, 12.527, -6.741]), "{centre}");
    assert!(near(half, [0.605, 0.605, 1.0]), "{half}");
    assert!(near(upper.direction, [0.0608, -0.866, -0.4963]), "{}", upper.direction);
    assert_eq!((upper.push.direction, upper.push.axis), ((10.0, 4.0), (0.0, 0.0)));
    assert!(!upper.push.negates_gravity);

    // Standing on the pad is inside the lower box; the top floor is not.
    assert!(lower.touches(glam::Vec3::new(-1.5, 12.53, -12.0), 0.0));
    assert!(!lifts.iter().any(|l| l.touches(glam::Vec3::new(-1.5, 8.0, -12.0), 0.0)));
}
