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

/// Guardian has no HaloExport reference: these are our own counts, checked
/// by looking (headless shots) rather than against another tool.
#[test]
fn guardian_counts() {
    let Some(path) = map("guardian") else { return };
    let scene = blam_geometry::load_map(&path).expect("guardian decodes");
    let c = count(&scene);
    assert_eq!(c.clusters_placed, 26);
    assert_eq!(c.instances_placed, 471);
    assert_eq!(c.cluster_triangles, 15491);
    assert_eq!(c.instance_triangles, 99759);
    assert_eq!(c.scenery_triangles, 85848);
    assert_eq!(c.crate_triangles, 33680);
    assert_eq!(c.machine_triangles, 0);
    assert_eq!(c.sky_triangles, 23497);
    assert_eq!(scene.materials.len(), 102);

    // 53 respawn points, every one standing on a collision floor.
    assert_eq!(scene.spawns.len(), 53);
    let col = &scene.collision;
    for sp in &scene.spawns {
        let p = sp.position;
        let floor = col
            .triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.points;
                let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
                if d.abs() < 1e-9 {
                    return None;
                }
                let u = ((p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y)) / d;
                let v = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / d;
                (u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
            })
            .filter(|z| *z <= p.z + 0.3)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((p.z - floor).abs() < 0.05, "spawn {sp:?} is {:.2} wu over its floor", p.z - floor);
        assert!(p.z > 15.5 && p.z < 19.5, "{sp:?}");
    }

    // The real collision: 14,448 world and 97,940 instanced triangles.
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    assert_eq!(bsp, 14448);
    assert_eq!(col.triangles.len() - bsp, 97940);
    assert_eq!(col.materials.len(), 69);

    // The abyss: `kill_drop` lies under the walkways (z 15.8 and up) and
    // over the fog plane; no spawn is in it.
    let kills: Vec<_> = scene.trigger_volumes.iter().filter(|v| v.kills()).collect();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].name, "kill_drop");
    assert!(kills[0].contains(glam::Vec3::new(0.0, -10.0, 10.0)));
    assert!(!kills[0].contains(glam::Vec3::new(0.0, -10.0, 15.0)));
    assert!(!scene.spawns.iter().any(|s| kills[0].contains(s.position)));
}

/// Blackout (`lockout`, a DLC map: tags under `levels\dlc\lockout`) has no
/// HaloExport reference either: our own counts, checked by looking. An oil
/// rig at night over an icy sea; the sea, the ice floes and the moon are
/// sky pieces, and `kill_volume_bottom` catches anyone who drops off.
#[test]
fn blackout_counts() {
    let Some(path) = map("lockout") else { return };
    let scene = blam_geometry::load_map(&path).expect("lockout decodes");
    let c = count(&scene);
    assert_eq!(c.clusters_placed, 21);
    assert_eq!(c.instances_placed, 447);
    assert_eq!(c.cluster_triangles, 22103);
    assert_eq!(c.instance_triangles, 195893);
    assert_eq!(c.scenery_triangles, 163040);
    assert_eq!(c.crate_triangles, 35972);
    assert_eq!(c.machine_triangles, 928);
    assert_eq!(c.sky_triangles, 23956);
    assert_eq!(scene.materials.len(), 128);

    // 80 respawn points, every one standing on a collision floor of the rig
    // (decks from z -12.6 up to -5.7).
    assert_eq!(scene.spawns.len(), 80);
    let col = &scene.collision;
    for sp in &scene.spawns {
        let p = sp.position;
        let floor = col
            .triangles
            .iter()
            .filter_map(|t| {
                let [a, b, c] = t.points;
                let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
                if d.abs() < 1e-9 {
                    return None;
                }
                let u = ((p.x - a.x) * (c.y - a.y) - (c.x - a.x) * (p.y - a.y)) / d;
                let v = ((b.x - a.x) * (p.y - a.y) - (p.x - a.x) * (b.y - a.y)) / d;
                (u >= -1e-4 && v >= -1e-4 && u + v <= 1.0001).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
            })
            .filter(|z| *z <= p.z + 0.3)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((p.z - floor).abs() < 0.06, "spawn {sp:?} is {:.2} wu over its floor", p.z - floor);
        assert!(p.z > -12.7 && p.z < -5.6, "{sp:?}");
    }

    // The real collision: 19,326 world and 166,536 instanced triangles.
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    assert_eq!(bsp, 19326);
    assert_eq!(col.triangles.len() - bsp, 166536);
    assert_eq!(col.materials.len(), 61);

    // The drop into the sea: `kill_volume_bottom` reaches up to z -18.26,
    // under the lowest deck (-12.6) and the rig's legs (-18.0), and down past
    // the sea (-33); no spawn is in it.
    let kills: Vec<_> = scene.trigger_volumes.iter().filter(|v| v.kills()).collect();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].name, "kill_volume_bottom");
    assert!(kills[0].contains(glam::Vec3::new(20.0, 0.0, -18.5)));
    assert!(kills[0].contains(glam::Vec3::new(0.0, 0.0, -33.0)));
    assert!(!kills[0].contains(glam::Vec3::new(0.0, 0.0, -18.0)));
    assert!(!scene.spawns.iter().any(|s| kills[0].contains(s.position)));

    // Grind rails: about 4,194 found (the finder's count moves by one or two
    // from run to run), 47 of them in the kill volume dropped.
    let (rails, census) = blam_geometry::rails::find_rails(col);
    assert!((4185..=4205).contains(&census.rails), "{} rails", census.rails);
    let found = rails.len();
    let rails = blam_geometry::rails::outside_kill_volumes(rails, &scene.trigger_volumes);
    assert!((40..=55).contains(&(found - rails.len())), "{} rails dropped", found - rails.len());
    for p in rails.iter().flatten() {
        assert!(p.x.abs() < 14.0 && p.y.abs() < 14.0 && p.z > -18.26 && p.z < 0.2, "rail point {p}");
    }
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

/// The camera fx exposure (`docs/format/sky-and-atmosphere.md` 5) and
/// Halo's own tangents on BSP vertices.
#[test]
fn camera_fx_and_tangents() {
    for (name, stops) in [("riverworld", -4.8365), ("bunkerworld", -2.5146), ("construct", -4.4739)] {
        let Some(path) = map(name) else { continue };
        let cache = blam_cache::CacheFile::open(&path).unwrap();
        let fx = blam_geometry::camera_fx::read(&cache).unwrap().expect("a cfxs tag");
        assert!((fx.exposure - stops).abs() < 1e-3, "{name}: {}", fx.exposure);
    }
    let Some(path) = map("riverworld") else { return };
    let options = Options {
        objects: false,
        collision: false,
        ..Default::default()
    };
    let scene = blam_geometry::load_map_with(&path, &options).unwrap();
    let (mut checked, mut skewed) = (0, 0);
    for m in scene.meshes.iter().filter(|m| !m.tangents.is_empty()) {
        assert_eq!(m.tangents.len(), m.positions.len(), "{}", m.name);
        for (t, n) in m.tangents.iter().zip(&m.normals) {
            let tv = glam::Vec3::new(t[0], t[1], t[2]);
            // Unit length and a +-1 handedness; at right angles to the
            // normal but for a few stored degenerate ones (tangent = normal),
            // which the viewer's shader skips.
            assert!((tv.length() - 1.0).abs() < 0.05, "{tv}");
            assert!(t[3] == 1.0 || t[3] == -1.0);
            skewed += (tv.dot(*n).abs() > 0.1) as usize;
            checked += 1;
        }
    }
    eprintln!("{checked} tangents, {skewed} not at right angles to their normal");
    assert!(checked > 50_000, "{checked} tangents");
    assert!(skewed * 100 < checked, "{skewed} of {checked} tangents not at right angles");
}

/// Where a vertical line through `(x, y)` crosses the triangle, if it does.
fn crossing_height(t: &[glam::Vec3; 3], x: f32, y: f32) -> Option<f32> {
    let [a, b, c] = *t;
    let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if d.abs() < 1e-9 {
        return None;
    }
    let u = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / d;
    let v = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / d;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
}

/// Last Resort (`docs/format/reference-numbers.md` 9).
#[test]
fn last_resort_matches_haloexport() {
    let Some(path) = map("zanzibar") else { return };
    let scene = blam_geometry::load_map(&path).expect("zanzibar decodes");
    let c = count(&scene);
    // 40 clusters with 39 meshes, 1,130 instances with 1,123 placed.
    assert_eq!(c.clusters_placed, 39);
    assert_eq!(c.instances_placed, 1123);
    assert_eq!(c.cluster_triangles, 38991);
    assert_eq!(c.instance_triangles, 316212);
    assert_eq!(c.scenery_triangles, 183060);
    assert_eq!(c.crate_triangles, 82432);
    assert_eq!(c.machine_triangles, 3192);
    let control: usize = scene
        .placements
        .iter()
        .filter(|p| p.kind == Kind::Control)
        .map(|p| scene.meshes[p.mesh as usize].triangle_count())
        .sum();
    assert_eq!(control, 764);
    assert!(c.sky_triangles > 0, "the sky model decodes");
    let (lo, hi) = scene.bounds.expect("world bounds");
    assert!((lo.x - -99.67684).abs() < 1e-3 && (hi.x - 44.225243).abs() < 1e-3);
    assert!((lo.y - -68.49561).abs() < 1e-3 && (hi.y - 160.14279).abs() < 1e-3);
    assert!((lo.z - -8.990257).abs() < 1e-3 && (hi.z - 20.045654).abs() < 1e-3);

    let water: Vec<_> = scene.materials.iter().filter(|m| m.shader_class == "rmw ").collect();
    assert_eq!(water.len(), 1);
    assert!(water[0].tag_path.ends_with("zanzibar_water_awesome"));

    // The collision BSP and the instanced collision.
    let col = &scene.collision;
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    assert_eq!(bsp, 31784);
    assert_eq!(col.triangles.len() - bsp, 86394);

    // 106 spawn points; each stands on collision a little under its feet,
    // and no collision surface cuts through the body above it.
    assert_eq!(scene.spawns.len(), 106);
    check_spawns(&scene);
}

/// Every spawn stands on collision less than 0.5 wu under it, with no
/// collision surface through the body (0.15 to 0.6 wu up).
fn check_spawns(scene: &Scene) {
    let col = &scene.collision;
    let mut cells: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
    for (i, t) in col.triangles.iter().enumerate() {
        let lo = t.points[0].min(t.points[1]).min(t.points[2]);
        let hi = t.points[0].max(t.points[1]).max(t.points[2]);
        for x in lo.x.floor() as i32..=hi.x.floor() as i32 {
            for y in lo.y.floor() as i32..=hi.y.floor() as i32 {
                cells.entry((x, y)).or_default().push(i);
            }
        }
    }
    for sp in &scene.spawns {
        let p = sp.position;
        let heights: Vec<f32> = cells
            .get(&(p.x.floor() as i32, p.y.floor() as i32))
            .into_iter()
            .flatten()
            .filter_map(|&i| crossing_height(&col.triangles[i].points, p.x, p.y))
            .collect();
        let ground = heights.iter().copied().filter(|h| *h <= p.z + 0.1).fold(f32::NEG_INFINITY, f32::max);
        assert!(p.z - ground < 0.5, "{} at {p} has no ground under it (nearest {ground})", sp.name);
        let body = heights.iter().copied().find(|h| *h > p.z + 0.15 && *h < p.z + 0.6);
        assert!(body.is_none(), "{} at {p}: collision through the body at z {body:?}", sp.name);
    }
}

/// High Ground (`docs/format/reference-numbers.md` 10).
#[test]
fn high_ground_matches_haloexport() {
    let Some(path) = map("deadlock") else { return };
    let scene = blam_geometry::load_map(&path).expect("deadlock decodes");
    let c = count(&scene);
    // 43 clusters, all with a mesh; 857 instances with 849 placed.
    assert_eq!(c.clusters_placed, 43);
    assert_eq!(c.instances_placed, 849);
    assert_eq!(c.cluster_triangles, 38754);
    assert_eq!(c.instance_triangles, 313216);
    assert_eq!(c.scenery_triangles, 141099);
    assert_eq!(c.crate_triangles, 68677);
    assert_eq!(c.machine_triangles, 2276);
    let control: usize = scene
        .placements
        .iter()
        .filter(|p| p.kind == Kind::Control)
        .map(|p| scene.meshes[p.mesh as usize].triangle_count())
        .sum();
    assert_eq!(control, 72);
    assert!(c.sky_triangles > 0, "the sky model decodes");
    let (lo, hi) = scene.bounds.expect("world bounds");
    assert!((lo.x - -62.073673).abs() < 1e-3 && (hi.x - 48.44192).abs() < 1e-3);
    assert!((lo.y - -72.087036).abs() < 1e-3 && (hi.y - 83.86857).abs() < 1e-3);
    assert!((lo.z - 2.225637).abs() < 1e-3 && (hi.z - 41.033073).abs() < 1e-3);

    // The gate (`wall_door`, a machine) in the base wall, where HaloExport
    // put it: x -8.58..-4.68, y 3.40..8.68, z 11.60..13.75 together with
    // the switch lever.
    let gate = scene
        .placements
        .iter()
        .find(|p| p.kind == Kind::Machine && p.name == "wall_door")
        .expect("the gate");
    let t = gate.transform.w_axis;
    assert!((t.x - -6.153).abs() < 0.01 && (t.y - 4.273).abs() < 0.01 && (t.z - 11.588).abs() < 0.01);

    let water: Vec<_> = scene.materials.iter().filter(|m| m.shader_class == "rmw ").collect();
    assert_eq!(water.len(), 1);
    assert!(water[0].tag_path.ends_with("deadlock_water_awesome"));

    let col = &scene.collision;
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    assert_eq!(bsp, 35890);
    assert_eq!(col.triangles.len() - bsp, 89094);

    // The sea's kill volume lies out past the beach; no spawn is in it.
    let kills: Vec<_> = scene.trigger_volumes.iter().filter(|v| v.kills()).collect();
    assert_eq!(kills.len(), 1);
    assert_eq!(kills[0].name, "kill_water2");
    assert!(!scene.spawns.iter().any(|s| kills[0].contains(s.position)));

    assert_eq!(scene.spawns.len(), 85);
    check_spawns(&scene);
}

/// Isolation (`docs/format/reference-numbers.md` 11): a Flood-overgrown
/// Forerunner yard inside a ring of tall walls, with tunnels and a cave
/// under it. No water and no kill volume: the walls keep everyone in.
#[test]
fn isolation_matches_haloexport() {
    let Some(path) = map("isolation") else { return };
    let scene = blam_geometry::load_map(&path).expect("isolation decodes");
    let c = count(&scene);
    // 49 clusters with 47 meshes, 391 instances all placed.
    assert_eq!(c.clusters_placed, 47);
    assert_eq!(c.instances_placed, 391);
    assert_eq!(c.cluster_triangles, 18987);
    assert_eq!(c.instance_triangles, 191321);
    assert_eq!(c.scenery_triangles, 148112);
    assert_eq!(c.crate_triangles, 19108);
    assert_eq!(c.machine_triangles, 3134);
    assert!(!scene.placements.iter().any(|p| p.kind == Kind::Control), "no controls");
    // HaloExport left the sky out; ours: 6 placements, 5,748 triangles.
    assert_eq!(c.sky_triangles, 5748);
    assert_eq!(scene.materials.len(), 61);
    let (lo, hi) = scene.bounds.expect("world bounds");
    assert!((lo.x - -112.74447).abs() < 1e-3 && (hi.x - 112.74448).abs() < 1e-3);
    assert!((lo.y - -39.311527).abs() < 1e-3 && (hi.y - 43.637577).abs() < 1e-3);
    assert!((lo.z - -14.824824).abs() < 1e-3 && (hi.z - 47.826878).abs() < 1e-3);

    // The one machine, the Flood door `flood_door_fast` on the lower level
    // north of the middle, tilted 15 degrees: its three pieces (door, hole,
    // wound) span what they span in HaloExport's glTF.
    let mut door = (glam::Vec3::splat(f32::INFINITY), glam::Vec3::splat(f32::NEG_INFINITY));
    for p in scene.placements.iter().filter(|p| p.kind == Kind::Machine) {
        let mesh = &scene.meshes[p.mesh as usize];
        for &i in mesh.parts.iter().flat_map(|part| &part.indices) {
            let w = p.transform.transform_point3(mesh.positions[i as usize]);
            door = (door.0.min(w), door.1.max(w));
        }
    }
    let near = |a: glam::Vec3, b: [f32; 3]| (a - glam::Vec3::from(b)).abs().max_element() < 0.02;
    assert!(near(door.0, [-2.6713, 5.8743, -10.932]), "{door:?}");
    assert!(near(door.1, [2.6989, 11.2887, -5.72]), "{door:?}");

    // No water anywhere: the green in the cave is the `flood_fog` haze.
    assert!(!scene.materials.iter().any(|m| m.shader_class == "rmw "));

    let col = &scene.collision;
    let bsp = col
        .triangles
        .iter()
        .filter(|t| matches!(t.source, blam_geometry::CollisionSource::Bsp(_)))
        .count();
    assert_eq!(bsp, 16280);
    assert_eq!(col.triangles.len() - bsp, 79954);
    assert_eq!(col.materials.len(), 27);

    // Four trigger volumes and none of them kills: `safe_zone` (the whole
    // yard, every spawn inside it) and three bird flocks beyond the walls.
    // Falling is caught by the floor under the world (`falls.rs`).
    let names: Vec<&str> = scene.trigger_volumes.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names, ["safe_zone", "flock0", "flock2", "flock3"]);
    assert!(!scene.trigger_volumes.iter().any(|v| v.kills()));
    let safe = &scene.trigger_volumes[0];
    assert!(scene.spawns.iter().all(|s| safe.contains(s.position)));

    // 91 spawns (84 plain, 7 invisible), all on a floor with room above.
    assert_eq!(scene.spawns.len(), 91);
    let invisible = scene.spawns.iter().filter(|s| s.kind == blam_geometry::SpawnKind::RespawnInvisible).count();
    assert_eq!(invisible, 7);
    let first = scene.spawns[0].position;
    assert!((first - glam::Vec3::new(18.818, 1.178, -7.455)).length() < 0.01, "{first}");
    check_spawns(&scene);

    // Grind rails: about 3,904 (the finder's count moves by one or two from
    // run to run); none in a kill volume or under water to drop.
    let (rails, census) = blam_geometry::rails::find_rails(col);
    assert!((3890..=3915).contains(&census.rails), "{} rails", census.rails);
    let found = rails.len();
    let rails = blam_geometry::rails::outside_kill_volumes(rails, &scene.trigger_volumes);
    assert_eq!(rails.len(), found);
}
