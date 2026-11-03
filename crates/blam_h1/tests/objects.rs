//! Halo CE's placed objects on The Maw (`d40`): which BSP each stands in,
//! their collision models, and the Warthog run's line (BSPs 5, 6, 7) with
//! them solid. Skips quietly when the map is not installed.

use std::collections::BTreeMap;
use std::path::PathBuf;

use blam_geometry::{CollisionSource, Kind, Scene};
use glam::Vec3;

fn d40() -> Option<PathBuf> {
    let p = game_paths::halo1_maps()?.join("d40.map");
    p.exists().then_some(p)
}

fn run_scene(map: &blam_h1::Map) -> Scene {
    let options = blam_h1::Options {
        textures: false,
        bsps: Some(vec![5, 6, 7]),
        ..Default::default()
    };
    blam_h1::decode(map, &options).unwrap().scene
}

/// The scenery placements' own BSP mask (`+0x20`, u16) is the point test
/// on every one of d40's scenery.
#[test]
fn object_bsps_are_the_maps_own() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let trees = blam_h1::objects::bsp_trees(&map).unwrap();
    let scnr = map.tag_data(map.scenario).unwrap();
    let scenery = scnr.block(0x210, 0x48).unwrap();
    assert_eq!(scenery.len(), 293);
    for (k, e) in scenery.iter().enumerate() {
        let stored = u32::from(e.u16(0x20).unwrap());
        let computed = blam_h1::objects::bsp_mask(&trees, e.vec3(8).unwrap());
        assert_eq!(stored, computed, "scenery {k}");
    }
}

/// Segment `a`-`b` against a triangle, either side.
fn crosses(a: Vec3, b: Vec3, t: &[Vec3; 3]) -> bool {
    let d = b - a;
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let s = a - t[0];
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = d.dot(q) / det;
    let w = e2.dot(q) / det;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&w)
}

/// The run's solid objects: the oxygen tanks and field generators of BSP 7
/// have their collision models where they are drawn; the Longsword and the
/// flames have none, as in the game.
#[test]
fn run_objects_collide_where_drawn() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let scene = run_scene(&map);
    let mut by_tag: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for (i, o) in scene.objects.iter().enumerate() {
        let n = scene.collision.triangles.iter().filter(|t| t.source.object() == Some(i as u32)).count();
        let e = by_tag.entry(o.tag_path.rsplit('\\').next().unwrap().to_string()).or_default();
        e.0 += 1;
        e.1 += n;
    }
    eprintln!("objects in BSPs 5-7 (count, collision triangles): {by_tag:?}");
    assert_eq!(by_tag["h oxy tank"].0, 60);
    assert!(by_tag["h oxy tank"].1 > 60 * 200);
    assert_eq!(by_tag["fighterbomber_scenery"].1, 0);
    assert_eq!(by_tag["burning_flame"].1, 0);
    // Nothing from the ship's inside (BSPs 1 to 4) inside BSP 5's box.
    assert!(!by_tag.contains_key("door small"), "{by_tag:?}");

    let tank = scene.objects.iter().position(|o| o.name.as_deref() == Some("bsp_8_22")).unwrap();
    let o = &scene.objects[tank];
    assert_eq!(o.bsps, 1 << 7);
    let (mut lo, mut hi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for t in scene.collision.triangles.iter().filter(|t| t.source.object() == Some(tank as u32)) {
        for p in t.points {
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    let (mut dlo, mut dhi) = (Vec3::INFINITY, Vec3::NEG_INFINITY);
    for pl in &scene.placements[o.placements.start as usize..o.placements.end as usize] {
        for p in &scene.meshes[pl.mesh as usize].positions {
            let w = pl.transform.transform_point3(*p);
            dlo = dlo.min(w);
            dhi = dhi.max(w);
        }
    }
    eprintln!("bsp_8_22 collision {lo:.2} .. {hi:.2}, drawn {dlo:.2} .. {dhi:.2}");
    assert!(lo.distance(dlo) < 0.15 && hi.distance(dhi) < 0.15);
    // A segment through its middle meets it.
    let mid = (lo + hi) * 0.5;
    let solid = scene
        .collision
        .triangles
        .iter()
        .filter(|t| t.source.object() == Some(tank as u32))
        .any(|t| crosses(mid - Vec3::X * 2.0, mid + Vec3::X * 2.0, &t.points));
    assert!(solid);
}

/// The floor at (x, y) nearest in height to `z` (within 6 wu), else the
/// highest below it (a drop): the structure BSP's upward faces.
fn floor(bsp: &[[Vec3; 3]], x: f32, y: f32, z: f32) -> Option<f32> {
    let p = glam::Vec2::new(x, y);
    let mut best: Option<f32> = None;
    let mut below: Option<f32> = None;
    for t in bsp {
        let (a, b, c) = (t[0].truncate(), t[1].truncate(), t[2].truncate());
        let d = (b - a).perp_dot(c - a);
        if d.abs() < 1e-9 || (t[1] - t[0]).cross(t[2] - t[0]).z <= 0.0 {
            continue;
        }
        let w1 = (p - a).perp_dot(c - a) / d;
        let w2 = (b - a).perp_dot(p - a) / d;
        let w0 = 1.0 - w1 - w2;
        if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
            continue;
        }
        let h = t[0].z * w0 + t[1].z * w1 + t[2].z * w2;
        if (h - z).abs() < 6.0 && best.is_none_or(|q| (h - z).abs() < (q - z).abs()) {
            best = Some(h);
        }
        if h < z && below.is_none_or(|q| h > q) {
            below = Some(h);
        }
    }
    best.or(below)
}

/// Along the run's line (y = -4, from the Warthogs at x -115 to the
/// Longsword's hangar at x 1052), bike-high over the floor: what solid
/// object is met, and that the road goes round each one.
#[test]
fn run_line_is_not_shut_by_objects() {
    let Some(path) = d40() else { return };
    let map = blam_h1::Map::open(&path).unwrap();
    let scene = run_scene(&map);
    let bsp: Vec<[Vec3; 3]> = scene
        .collision
        .triangles
        .iter()
        .filter(|t| matches!(t.source, CollisionSource::Bsp(_)))
        .map(|t| t.points)
        .collect();
    let objects: Vec<(u32, [Vec3; 3])> = scene
        .collision
        .triangles
        .iter()
        .filter_map(|t| Some((t.source.object()?, t.points)))
        .collect();
    let hits = |a: Vec3, b: Vec3| -> Vec<u32> {
        let mut out: Vec<u32> = objects.iter().filter(|(_, t)| crosses(a, b, t)).map(|(o, _)| *o).collect();
        out.dedup();
        out
    };
    let y = -4.0;
    let mut z = 107.7;
    let mut x = -115.0f32;
    let mut met: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    let mut gaps = 0;
    let mut checked = 0;
    while x < 1052.0 {
        let nx = x + 0.5;
        let (Some(f0), Some(f1)) = (floor(&bsp, x, y, z), floor(&bsp, nx, y, z)) else {
            gaps += 1;
            if std::env::var("H1_LINE_TRACE").is_ok() {
                eprintln!("x {x:.1} no floor below {z:.2}");
            }
            x = nx;
            continue;
        };
        z = f1;
        checked += 1;
        if std::env::var("H1_LINE_TRACE").is_ok() && (x * 2.0) as i32 % 50 == 0 {
            eprintln!("x {x:.1} floor {f1:.2}");
        }
        for h in [0.3, 0.8] {
            let found = hits(Vec3::new(x, y, f0 + h), Vec3::new(nx, y, f1 + h));
            for o in found {
                let so = &scene.objects[o as usize];
                assert!(
                    so.kind == Kind::Scenery,
                    "{} ({}) shuts the road at x {x}",
                    so.name.as_deref().unwrap_or("-"),
                    so.tag_path
                );
                met.entry(format!("{} {}", so.name.as_deref().unwrap_or("-"), so.tag_path.rsplit('\\').next().unwrap())).or_default().push(x);
                // The way round: a parallel line within 6 wu with floor and
                // no object.
                let round = (1..=12).flat_map(|k| [k as f32 * 0.5, -(k as f32) * 0.5]).any(|dy| {
                    let (Some(g0), Some(g1)) = (floor(&bsp, x, y + dy, f0), floor(&bsp, nx, y + dy, f1)) else { return false };
                    (g0 - f0).abs() < 1.0
                        && (g1 - f1).abs() < 1.0
                        && [0.3, 0.8].iter().all(|h| hits(Vec3::new(x - 1.0, y + dy, g0 + h), Vec3::new(nx + 1.0, y + dy, g1 + h)).is_empty())
                });
                assert!(round, "no way round {} at x {x}", so.name.as_deref().unwrap_or("-"));
            }
        }
        x = nx;
    }
    for (name, xs) in &met {
        eprintln!("on the line: {name} at x {:.1}..{:.1}", xs[0], xs[xs.len() - 1]);
    }
    eprintln!("{checked} steps checked, {gaps} without floor");
    assert!(checked > 2000, "{checked}");
}
