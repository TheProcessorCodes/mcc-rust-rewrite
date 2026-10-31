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

/// Lockout's weapons (`docs/format/halo2.md` 11, `probe_h2_weapons.py`):
/// no scenario weapon placements; 32 netgame equipment entries, 14 of them
/// weapons (9 kinds), 16 grenades and 2 empty; the SMG to start with.
#[test]
fn lockout_weapons() {
    let Some(path) = map("lockout") else { return };
    let m = blam_h2::Map::open(&path).expect("opens");
    let items = blam_h2::weapons::netgame_items(&m).expect("netgame equipment");
    assert_eq!(items.len(), 32);
    let class = |c: &[u8; 4]| items.iter().filter(|i| i.item_class.as_ref() == Some(c)).count();
    assert_eq!((class(b"weap"), class(b"eqip"), items.iter().filter(|i| i.item.is_none()).count()), (14, 16, 2));
    let placed = blam_h2::weapons::weapon_placements(&items);
    assert_eq!(placed.len(), 14);
    let mut kinds: Vec<&str> = placed.iter().map(|p| p.tag_path.rsplit('\\').next().unwrap()).collect();
    kinds.sort_unstable();
    kinds.dedup();
    assert_eq!(
        kinds,
        ["battle_rifle", "energy_blade", "magnum", "needler", "plasma_pistol", "plasma_rifle", "shotgun", "smg", "sniper_rifle"]
    );
    let start = blam_h2::weapons::starting_weapons(&m).expect("starting equipment");
    assert_eq!(start.len(), 1);
    assert_eq!(m.tag_name(start[0]), Some(r"objects\weapons\rifle\smg\smg"));

    // The battle rifle's numbers and markers.
    let br = placed.iter().find(|p| p.tag_path.ends_with("battle_rifle")).unwrap();
    assert_eq!(br.respawn_seconds, 45);
    let w = blam_h2::weapons::read_weapon(&m, br.tag.unwrap()).expect("battle rifle");
    let mag = &w.magazines[0];
    assert_eq!((mag.rounds_total_initial, mag.rounds_total_maximum, mag.rounds_loaded_maximum), (108, 144, 36));
    assert_eq!(w.triggers[0].behavior, 4);
    assert_eq!(w.barrels[0].shots_per_fire, [3, 3]);
    assert_eq!(w.barrels[0].rounds_per_second, [15.0, 15.0]);
    assert_eq!((w.zoom_levels, w.zoom_range), (1, [2.0, 2.0]));
    let p = w.barrels[0].projectile.as_ref().expect("bullet");
    assert_eq!((p.maximum_range, p.initial_velocity), (40.0, 400.0));
    let muzzle = w.muzzle().expect("primary_trigger").model.w_axis;
    assert!((muzzle.x - 0.192).abs() < 1e-3 && (muzzle.z - 0.046).abs() < 1e-3, "{muzzle}");
    assert!(w.marker("left_hand").is_some());

    // The plasma rifle's empty magazine makes it a battery weapon; the
    // sniper zooms twice.
    let read = |name: &str| {
        let t = placed.iter().find(|p| p.tag_path.ends_with(name)).unwrap().tag.unwrap();
        blam_h2::weapons::read_weapon(&m, t).unwrap()
    };
    let pr = read("plasma_rifle");
    assert!(pr.magazines.is_empty() && pr.barrels[0].magazine.is_none());
    let sniper = read("sniper_rifle");
    assert_eq!((sniper.zoom_levels, sniper.magazines[0].rounds_loaded_maximum), (2, 4));

    // Every kind's third-person model decodes, with its textures.
    let weapons: Vec<_> = placed.iter().filter_map(|p| p.tag).collect();
    let modes: Vec<u32> = weapons.iter().map(|&t| blam_h2::weapons::read_weapon(&m, t).unwrap().render_model.unwrap()).collect();
    let models = blam_h2::decode_models(&m, &modes);
    assert!(models.failed.is_empty(), "{:?}", models.failed);
    assert!(models.meshes.iter().all(|v| !v.is_empty()));
    let br_tris: usize = models.meshes[0].iter().map(|&k| models.scene.meshes[k as usize].triangle_count()).sum();
    assert!(br_tris > 100, "{br_tris}");
    // 29 materials, every one with a base map that decodes; the battle
    // rifle is 0.30 wu long (compression bounds applied).
    assert_eq!(models.looks.len(), 29);
    assert!(models.looks.iter().all(|l| l.as_ref().and_then(|l| l.base).is_some_and(|(b, _)| models.bitmaps.contains_key(&b))));
    let (lo, hi) = models.meshes[0].iter().flat_map(|&k| &models.scene.meshes[k as usize].positions).fold(
        (glam::Vec3::splat(f32::MAX), glam::Vec3::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(*p), hi.max(*p)),
    );
    assert!((hi.x - lo.x - 0.295).abs() < 0.01, "{lo} {hi}");
}

/// The gravity lift (`docs/format/halo2.md` 12): the crate
/// `lockout_lift_tube` at (-1.5, 12.53, -12.2), tilted 7 degrees towards -Y by its
/// roll, carries two phantom boxes from its physics model. The lower one
/// (4.5 wu tall over the pad) lifts at 10 wu/s^2 to 4 wu/s with gravity off
/// and pulls to the axis; the upper one (2 wu over that) pushes out along
/// `upper_lift_direction`, 37 degrees below level towards -Y (30 in its own frame, plus
/// the lean; angles composed about the world axes, `scenario::rotation`).
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
    assert!(near(centre, [-1.497, 12.253, -9.967]), "{centre}");
    assert!(near(half, [0.605, 0.605, 2.25]), "{half}");
    assert!(near(axes[2], [0.0, -0.1217, 0.9926]), "{:?}", axes[2]);
    assert!(near(lower.direction, [0.0, -0.1217, 0.9926]), "{}", lower.direction);
    assert!(near(lower.origin, [-1.5, 12.52, -12.2]), "{}", lower.origin);
    assert_eq!((lower.push.direction, lower.push.axis), ((10.0, 4.0), (10.0, 10.0)));
    assert!(lower.push.negates_gravity && !lower.push.ignores_vehicles);

    let (centre, _, half) = boxed(upper);
    assert!(near(centre, [-1.497, 11.858, -6.741]), "{centre}");
    assert!(near(half, [0.605, 0.605, 1.0]), "{half}");
    assert!(near(upper.direction, [0.0, -0.7987, -0.6017]), "{}", upper.direction);
    assert_eq!((upper.push.direction, upper.push.axis), ((10.0, 4.0), (0.0, 0.0)));
    assert!(!upper.push.negates_gravity);

    // Standing on the pad is inside the lower box; the top floor is not.
    assert!(lower.touches(glam::Vec3::new(-1.5, 12.53, -12.0), 0.0));
    assert!(!lifts.iter().any(|l| l.touches(glam::Vec3::new(-1.5, 8.0, -12.0), 0.0)));
}
