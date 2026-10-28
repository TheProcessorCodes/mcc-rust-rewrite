//! Decodes the user's own maps and checks the counts against what
//! HaloExport (C#, on Reclaimer) wrote for the same files
//! (`docs/format/reference-numbers.md`). Skips quietly when a map is not
//! installed.

use std::path::PathBuf;

use blam_geometry::{Kind, Options, Scene};

fn map(name: &str) -> Option<PathBuf> {
    let dir = std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(
                r"C:\Program Files (x86)\Steam\steamapps\common\Halo The Master Chief Collection\halo3\maps",
            )
        });
    let p = dir.join(format!("{name}.map"));
    p.exists().then_some(p)
}

#[derive(Default)]
struct Counts {
    clusters_placed: usize,
    instances_placed: usize,
    cluster_triangles: usize,
    instance_triangles: usize,
    /// Triangles per object kind: scenery, crate, machine, control, sky.
    scenery_triangles: usize,
    crate_triangles: usize,
    machine_triangles: usize,
    sky_triangles: usize,
}

fn count(scene: &Scene) -> Counts {
    let mut c = Counts::default();
    for p in &scene.placements {
        let t = scene.meshes[p.mesh as usize].triangle_count();
        match p.kind {
            Kind::Cluster => {
                c.clusters_placed += 1;
                c.cluster_triangles += t;
            }
            Kind::Instance => {
                c.instances_placed += 1;
                c.instance_triangles += t;
            }
            Kind::Scenery => c.scenery_triangles += t,
            Kind::Crate => c.crate_triangles += t,
            Kind::Machine => c.machine_triangles += t,
            Kind::Sky => c.sky_triangles += t,
            Kind::Control => {}
        }
    }
    c
}

#[test]
fn valhalla_matches_haloexport() {
    let Some(path) = map("riverworld") else { return };
    let scene = blam_geometry::load_map(&path).expect("riverworld decodes");
    let c = count(&scene);
    assert_eq!(c.clusters_placed, 23);
    assert_eq!(c.instances_placed, 261);
    assert_eq!(c.cluster_triangles, 37260);
    assert_eq!(c.instance_triangles, 87367);
    // Scenario objects, as HaloExport counts them (first permutation of
    // every region).
    assert_eq!(c.scenery_triangles, 146845);
    assert_eq!(c.crate_triangles, 39625);
    assert!(c.sky_triangles > 0, "the sky model decodes");
    let (lo, hi) = scene.bounds.expect("world bounds");
    assert!((lo.x - -40.85219).abs() < 1e-3 && (hi.x - 206.53394).abs() < 1e-3);
    assert!((lo.y - -285.42664).abs() < 1e-3 && (hi.y - 92.48665).abs() < 1e-3);
    assert!((lo.z - -15.5174055).abs() < 1e-3);

    // Every placed BSP vertex lies inside the stated bounds (plus a little
    // slack); objects and the sky may poke out.
    let mut placed_hi = glam::Vec3::NEG_INFINITY;
    for p in scene
        .placements
        .iter()
        .filter(|p| matches!(p.kind, Kind::Cluster | Kind::Instance))
    {
        for v in &scene.meshes[p.mesh as usize].positions {
            let w = p.transform.transform_point3(*v);
            assert!(w.x >= lo.x - 0.01 && w.x <= hi.x + 0.01, "{} out of x bounds: {w}", p.name);
            assert!(w.y >= lo.y - 0.01 && w.y <= hi.y + 0.01, "{} out of y bounds: {w}", p.name);
            placed_hi = placed_hi.max(w);
        }
    }
    // The map's real geometry tops out near 91 wu even though the stated Z
    // bound reaches 1892.
    assert!((placed_hi.z - 90.875).abs() < 0.01, "top of the world {placed_hi}");

    // Normals come out unit length and the water shader is translucent.
    let m = &scene.meshes[0];
    for n in &m.normals {
        assert!((n.length() - 1.0).abs() < 0.02, "normal {n}");
    }
    let water = scene
        .materials
        .iter()
        .find(|m| m.tag_path.ends_with("riverworld_water_rough"))
        .expect("water material");
    assert_eq!(water.shader_class, "rmw ");
    assert_eq!(water.shading, blam_geometry::Shading::Translucent);
    // shaders.md 8.4: riverworld_water_rough is tag 11609.
    assert_eq!(water.shader_tag, Some(11609));

    // Valhalla's respawn markers, all standing on the ground inside the map.
    assert!(scene.spawns.len() >= 16, "{} spawns", scene.spawns.len());
    for sp in &scene.spawns {
        assert!(sp.position.x > lo.x && sp.position.x < hi.x && sp.position.y > lo.y && sp.position.y < hi.y, "{sp:?}");
        assert!(sp.position.z > -9.3 && sp.position.z < 91.0, "{sp:?}");
    }

    // The real collision BSP: 24,682 convex surfaces fan into 30,560
    // triangles; 76 instance definitions hold 13,902 more before placement.
    let col = &scene.collision;
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    let inst = col.triangles.len() - bsp;
    assert_eq!(bsp, 30560);
    assert!(inst > 13902, "{inst} instanced collision triangles");
    assert_eq!(col.materials.len(), 37);
    for t in col.triangles.iter().filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_))) {
        for p in t.points {
            assert!(p.x >= -38.1 && p.x <= 206.54 && p.y >= -285.43 && p.y <= 89.63, "{p}");
        }
    }
    // Floors face up: most world collision triangles have a non-negative Z
    // normal component (walls are vertical, ceilings rare on Valhalla).
    let up = col
        .triangles
        .iter()
        .filter(|t| t.normal().z > 0.5)
        .count();
    let down = col
        .triangles
        .iter()
        .filter(|t| t.normal().z < -0.5)
        .count();
    assert!(up > down * 2, "{up} up-facing vs {down} down-facing collision triangles");
}

#[test]
fn the_pit_matches_haloexport() {
    let Some(path) = map("cyberdyne") else { return };
    let scene = blam_geometry::load_map(&path).expect("cyberdyne decodes");
    let c = count(&scene);
    assert_eq!(c.clusters_placed, 44);
    assert_eq!(c.instances_placed, 830);
    assert_eq!(c.cluster_triangles, 12240);
    assert_eq!(c.instance_triangles, 229319);
    assert_eq!(c.scenery_triangles, 139652);
    assert_eq!(c.crate_triangles, 50382);
    assert_eq!(c.machine_triangles, 3200);
}

#[test]
fn campaign_map_selects_one_bsp() {
    let Some(path) = map("120_halo") else { return };
    let options = Options {
        bsps: Some(vec![2]),
        objects: false,
        collision: false,
        verbose: false,
    };
    let scene = blam_geometry::load_map_with(&path, &options).expect("120_halo decodes");
    let c = count(&scene);
    assert_eq!(c.clusters_placed, 29);
    assert_eq!(c.instances_placed, 373);
    assert!(scene.placements.iter().all(|p| p.name.starts_with("bsp2 ") || p.kind == Kind::Instance));
}

#[test]
fn valhalla_has_grind_rails() {
    let Some(path) = map("riverworld") else { return };
    let options = Options {
        objects: false,
        collision: true,
        ..Default::default()
    };
    let scene = blam_geometry::load_map_with(&path, &options).expect("riverworld decodes");
    let col = &scene.collision;
    assert!(!col.triangles.is_empty(), "collision decoded");
    let (rails, census) = blam_geometry::rails::find_rails(col);
    println!(
        "riverworld rails: {} walkable edges, {} lips, {} runs, {} rails",
        census.candidates, census.lips, census.runs, census.rails
    );
    assert!(!rails.is_empty(), "no rails found");
    assert_eq!(rails.len(), census.rails);
    assert!(census.candidates >= census.lips && census.lips >= census.runs);

    // Every rail point sits on the collision: inside the XY box of some
    // collision triangle (a little slack) and within 0.2 wu of its height
    // range. Triangles are binned by XY cell so the lookup stays quick.
    const CELL: f32 = 4.0;
    const XY_SLACK: f32 = 0.1;
    const Z_SLACK: f32 = 0.2;
    let cell = |v: f32| (v / CELL).floor() as i32;
    let boxes: Vec<(glam::Vec3, glam::Vec3)> = col
        .triangles
        .iter()
        .map(|t| (t.points[0].min(t.points[1]).min(t.points[2]), t.points[0].max(t.points[1]).max(t.points[2])))
        .collect();
    let mut grid: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    for (i, (lo, hi)) in boxes.iter().enumerate() {
        for x in cell(lo.x - XY_SLACK)..=cell(hi.x + XY_SLACK) {
            for y in cell(lo.y - XY_SLACK)..=cell(hi.y + XY_SLACK) {
                grid.entry((x, y)).or_default().push(i);
            }
        }
    }
    let on_collision = |p: glam::Vec3| {
        grid.get(&(cell(p.x), cell(p.y))).is_some_and(|tris| {
            tris.iter().any(|&i| {
                let (lo, hi) = boxes[i];
                p.x >= lo.x - XY_SLACK
                    && p.x <= hi.x + XY_SLACK
                    && p.y >= lo.y - XY_SLACK
                    && p.y <= hi.y + XY_SLACK
                    && p.z >= lo.z - Z_SLACK
                    && p.z <= hi.z + Z_SLACK
            })
        })
    };
    for rail in &rails {
        assert!(rail.len() >= 2, "rail with {} points", rail.len());
        for p in rail {
            assert!(on_collision(*p), "rail point {p} is not on any collision triangle");
        }
    }
}

/// Baked lighting links (`docs/format/lightmaps.md` 3, 4): every lit
/// cluster finds its texcoords, instances split by their lighting policy.
#[test]
fn valhalla_lighting() {
    use blam_geometry::Lighting;
    let Some(path) = map("riverworld") else { return };
    let options = Options {
        objects: false,
        collision: false,
        ..Default::default()
    };
    let scene = blam_geometry::load_map_with(&path, &options).expect("riverworld decodes");
    assert_eq!(scene.lightmaps.len(), 1);
    let lm = &scene.lightmaps[0];
    assert!(lm.coefficients_bitmap.is_some() && lm.intensity_bitmap.is_some());
    assert!((lm.scales[0] - 4.6922183).abs() < 1e-5 && (lm.scales[4] - 16.105705).abs() < 1e-5);
    let mut counts = std::collections::HashMap::<(Kind, &str), usize>::new();
    for p in &scene.placements {
        let mode = match &p.lighting {
            Lighting::None => "none",
            Lighting::PerPixel { uvs, .. } => {
                assert_eq!(uvs.len(), scene.meshes[p.mesh as usize].positions.len());
                "pixel"
            }
            Lighting::PerVertex(v) => {
                assert_eq!(v.len(), scene.meshes[p.mesh as usize].positions.len());
                "vertex"
            }
            Lighting::Probe(_) => "probe",
        };
        *counts.entry((p.kind, mode)).or_default() += 1;
    }
    eprintln!("{counts:?}");
    assert_eq!(counts.get(&(Kind::Cluster, "pixel")), Some(&23));
    assert_eq!(counts.get(&(Kind::Instance, "pixel")), Some(&184));
    // 59 per-vertex records; one belongs to the instance without a mesh.
    assert_eq!(counts.get(&(Kind::Instance, "vertex")), Some(&58));
    // The per-pixel instance whose record has image -1.
    assert_eq!(counts.get(&(Kind::Instance, "none")), Some(&1));
    assert_eq!(counts.get(&(Kind::Instance, "probe")), Some(&18));
}
