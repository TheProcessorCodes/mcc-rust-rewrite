//! Finds drawn geometry with no collision under it: samples points on every
//! placement's triangles within [`PLAY`] of a spawn point and checks for a
//! collision triangle within [`NEAR`]. Prints the bare share per kind, the
//! bare clusters' spots and materials, and the barest placements.
//! Diagnostic; run with
//! `cargo test -p blam_reach --test collision_coverage -- --ignored --nocapture`
//! (`REACH_MAP=forge_halo` for one map, `CLUSTERS=1` to list only
//! clusters, `REVERSE=1` to check instance collision lies on the instances).
//!
//! On the user's maps (2026-10-31), once large collision BSPs were read,
//! what is left bare is water (`rmw`), foliage, light planes, shield
//! domes and far scenery, and instances whose definition holds no
//! collision at all and which are not flagged collidable (instance flag
//! bit 3), so the game does not collide with them either.

use std::collections::HashMap;
use std::path::PathBuf;

use blam_geometry::{Kind, Scene};
use glam::Vec3;

fn maps_dir() -> PathBuf {
    game_paths::reach_maps().unwrap_or_default()
}

const CELL: f32 = 2.0;
/// About 90 cm; trim and panels sit a little off their collision hull.
const NEAR: f32 = 0.3;
/// Only geometry this close to a spawn point (horizontally and in height) counts.
const PLAY: f32 = 30.0;

struct Grid {
    cells: HashMap<(i32, i32, i32), Vec<u32>>,
}

fn key(p: Vec3) -> (i32, i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32, (p.z / CELL).floor() as i32)
}

impl Grid {
    fn new(scene: &Scene) -> Self {
        let mut cells: HashMap<_, Vec<u32>> = HashMap::new();
        for (i, t) in scene.collision.triangles.iter().enumerate() {
            let lo = t.points[0].min(t.points[1]).min(t.points[2]) - NEAR;
            let hi = t.points[0].max(t.points[1]).max(t.points[2]) + NEAR;
            let (a, b) = (key(lo), key(hi));
            if (b.0 - a.0) as i64 * (b.1 - a.1) as i64 * (b.2 - a.2) as i64 > 200_000 {
                continue;
            }
            for x in a.0..=b.0 {
                for y in a.1..=b.1 {
                    for z in a.2..=b.2 {
                        cells.entry((x, y, z)).or_default().push(i as u32);
                    }
                }
            }
        }
        Self { cells }
    }

    fn near(&self, scene: &Scene, p: Vec3) -> bool {
        self.cells.get(&key(p)).is_some_and(|list| {
            list.iter().any(|&i| distance(p, scene.collision.triangles[i as usize].points) < NEAR)
        })
    }
}

fn distance(p: Vec3, [a, b, c]: [Vec3; 3]) -> f32 {
    // Closest point on triangle (Ericson).
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return p.distance(a);
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return p.distance(b);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return p.distance(a + ab * (d1 / (d1 - d3)));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return p.distance(c);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return p.distance(a + ac * (d2 / (d2 - d6)));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return p.distance(b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6))));
    }
    let denom = 1.0 / (va + vb + vc);
    p.distance(a + ab * (vb * denom) + ac * (vc * denom))
}

#[test]
#[ignore]
fn coverage() {
    let dir = maps_dir();
    let only = std::env::var("REACH_MAP").ok();
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.strip_suffix(".map").map(str::to_string))
        .filter(|n| only.as_deref().is_none_or(|o| o == n))
        .collect();
    names.sort();
    for name in names {
        let path = dir.join(format!("{name}.map"));
        if !blam_reach::is_reach(&path) {
            continue;
        }
        let Ok(cache) = blam_cache::CacheFile::open(&path) else { continue };
        let mut scene = Scene::default();
        if blam_reach::decode_into(&cache, &mut scene).is_err() {
            continue;
        }
        let grid = Grid::new(&scene);
        if std::env::var_os("REVERSE").is_some() {
            // Is instance collision where instance render geometry is?
            let mut render = Scene::default();
            for p in scene.placements.iter().filter(|p| p.kind == Kind::Instance) {
                for t in scene.meshes[p.mesh as usize].triangles() {
                    render.collision.triangles.push(blam_geometry::CollisionTriangle {
                        points: t.map(|v| p.transform.transform_point3(v)),
                        material: -1,
                        flags: 0,
                        source: blam_geometry::CollisionSource::Bsp(0),
                    });
                }
            }
            let rgrid = Grid::new(&render);
            let (mut n, mut near) = (0, 0);
            let mut far = Vec::new();
            for (i, t) in scene.collision.triangles.iter().enumerate() {
                if !matches!(t.source, blam_geometry::CollisionSource::Instance(_)) || i % 7 != 0 {
                    continue;
                }
                n += 1;
                let c = (t.points[0] + t.points[1] + t.points[2]) / 3.0;
                if rgrid.near(&render, c) { near += 1 } else if far.len() < 8 { far.push(c) }
            }
            println!("   reverse: {near} of {n} instance collision samples lie on instance render geometry; far ones {far:?}");
        }
        // (placement, kind, sampled area, uncovered area)
        let mut misses = Vec::new();
        let mut by_kind: HashMap<Kind, (f32, f32)> = HashMap::new();
        let mut bare_materials: HashMap<(bool, u32), f32> = HashMap::new();
        let mut spots: HashMap<(i32, i32, i32), (f32, f32, Vec3)> = HashMap::new();
        for p in &scene.placements {
            if matches!(p.kind, Kind::Sky) {
                continue;
            }
            let mesh = &scene.meshes[p.mesh as usize];
            let (mut area, mut bare) = (0.0f32, 0.0f32);
            let tris = mesh.parts.iter().flat_map(|part| {
                part.indices.as_chunks::<3>().0.iter().map(move |t| (part.material, t.map(|i| mesh.positions[i as usize])))
            });
            for (k, (material, t)) in tris.enumerate() {
                if k % 3 != 0 {
                    continue;
                }
                let w = t.map(|v| p.transform.transform_point3(v));
                let a = (w[1] - w[0]).cross(w[2] - w[0]).length() * 0.5;
                if a < 1e-4 {
                    continue;
                }
                let c = (w[0] + w[1] + w[2]) / 3.0;
                if !scene.spawns.iter().any(|s| s.position.truncate().distance(c.truncate()) < PLAY && (s.position.z - c.z).abs() < PLAY) {
                    continue;
                }
                area += a;
                let spot = spots.entry(((c.x / 10.0).floor() as i32, (c.y / 10.0).floor() as i32, (c.z / 10.0).floor() as i32)).or_insert((0.0, 0.0, c));
                spot.0 += a;
                if !grid.near(&scene, c) {
                    bare += a;
                    if matches!(p.kind, Kind::Cluster | Kind::Instance) {
                        *bare_materials.entry((p.kind == Kind::Instance, material)).or_insert(0.0) += a;
                    }
                    if p.kind == Kind::Cluster {
                        spot.1 += a;
                        spot.2 = c;
                    }
                }
            }
            let e = by_kind.entry(p.kind).or_default();
            e.0 += area;
            e.1 += bare;
            if area > 1.0 && bare / area > 0.5 {
                misses.push((p.name.clone(), p.kind, area, bare, p.transform.w_axis.truncate()));
            }
        }
        let mut spots: Vec<_> = spots.into_values().filter(|s| s.1 > 0.0).collect();
        spots.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (all, bare, at) in spots.iter().take(12) {
            println!("   bare cluster spot {bare:.0} of {all:.0} near {at:.1}");
        }
        let mut mats: Vec<_> = bare_materials.into_iter().collect();
        mats.sort_by(|a, b| b.1.total_cmp(&a.1));
        for ((inst, m), a) in mats.iter().take(12) {
            let m = &scene.materials[*m as usize];
            let what = if *inst { "instance" } else { "cluster" };
            println!("   bare {what} material {a:.0}: {} ({})", m.tag_path, m.shader_class);
        }
        misses.sort_by(|a, b| b.3.total_cmp(&a.3));
        let mut kinds: Vec<_> = by_kind.into_iter().collect();
        kinds.sort_by_key(|k| format!("{:?}", k.0));
        println!("== {name}: {} collision triangles", scene.collision.triangles.len());
        for (k, (a, b)) in kinds {
            println!("   {k:?}: {:.0} of {:.0} sampled area bare ({:.1}%)", b, a, 100.0 * b / a.max(1e-6));
        }
        for (n, k, a, b, at) in misses.iter().filter(|m| std::env::var_os("CLUSTERS").is_none() || m.1 == Kind::Cluster).take(25) {
            println!("   bare {k:?} {n}: {b:.1}/{a:.1} at {at:.1}");
        }
    }
}
