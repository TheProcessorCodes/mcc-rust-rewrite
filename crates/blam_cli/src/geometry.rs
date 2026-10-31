//! `blam geometry <map> [--bsp N] [--obj PATH]`: decode a map's structure
//! BSPs, print what came out, and optionally write a Wavefront OBJ in
//! metres, Y up (the same frame as HaloExport's glTF, so the numbers can be
//! compared).

use std::collections::BTreeMap;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use blam_cache::CacheFile;
use blam_geometry::{Kind, Options, Scene, WORLD_UNIT_METRES};
use glam::Vec3;

pub fn run(
    cache: &CacheFile,
    bsps: Option<Vec<u32>>,
    objects: bool,
    obj: Option<&Path>,
) -> Result<()> {
    let options = Options {
        bsps,
        objects,
        collision: true,
        verbose: true,
    };
    let started = std::time::Instant::now();
    let mut scene = Scene {
        name: cache.header.map_name.clone(),
        ..Default::default()
    };
    blam_geometry::decode_into(cache, &options, &mut scene)?;
    eprintln!("decoded in {:.2} s", started.elapsed().as_secs_f32());
    report(&scene, obj)
}

/// Prints what a decoded scene holds and optionally writes it as OBJ (also
/// used for Halo 2 maps, `halo2.rs`).
pub fn report(scene: &Scene, obj: Option<&Path>) -> Result<()> {
    // Totals by kind, and the bounds of everything placed.
    let mut tris: BTreeMap<String, usize> = BTreeMap::new();
    let mut placed: BTreeMap<String, usize> = BTreeMap::new();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut by_material = vec![0usize; scene.materials.len()];
    for p in &scene.placements {
        let mesh = &scene.meshes[p.mesh as usize];
        let kind = format!("{:?}", p.kind).to_lowercase();
        *placed.entry(kind.clone()).or_default() += 1;
        *tris.entry(kind).or_default() += mesh.triangle_count();
        for part in &mesh.parts {
            by_material[part.material as usize] += part.indices.len() / 3;
        }
        if p.kind == Kind::Sky {
            continue;
        }
        for v in &mesh.positions {
            let w = p.transform.transform_point3(*v);
            lo = lo.min(w);
            hi = hi.max(w);
        }
    }
    println!(
        "{}: {} materials, {} meshes, {} placements, {} triangles",
        scene.name,
        scene.materials.len(),
        scene.meshes.len(),
        scene.placements.len(),
        scene.triangle_count()
    );
    for (kind, n) in &placed {
        println!("  {kind:<9} {n:>6} placed  {:>9} triangles", tris[kind]);
    }
    if let Some((a, b)) = scene.bounds {
        println!(
            "  stated bounds (wu)  x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}",
            a.x, b.x, a.y, b.y, a.z, b.z
        );
    }
    if lo.x.is_finite() {
        println!(
            "  placed bounds (wu, sky left out)  x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}",
            lo.x, hi.x, lo.y, hi.y, lo.z, hi.z
        );
        let (l, h) = ((hi - lo) * WORLD_UNIT_METRES, hi - lo);
        println!(
            "  size {:.1} x {:.1} x {:.1} m ({:.0} x {:.0} x {:.0} ft)",
            l.x,
            l.y,
            l.z,
            h.x * 10.0,
            h.y * 10.0,
            h.z * 10.0
        );
    }
    // The game's own collision.
    let col = &scene.collision;
    if !col.triangles.is_empty() {
        let (mut bsp, mut inst) = (0usize, 0usize);
        let mut clo = Vec3::splat(f32::INFINITY);
        let mut chi = Vec3::splat(f32::NEG_INFINITY);
        let mut flagged: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &col.triangles {
            match t.source {
                blam_geometry::CollisionSource::Bsp(_) => bsp += 1,
                blam_geometry::CollisionSource::Instance(_) | blam_geometry::CollisionSource::Object { .. } => inst += 1,
            }
            for p in t.points {
                clo = clo.min(p);
                chi = chi.max(p);
            }
            for (bit, name) in [
                (1u8, "two-sided"),
                (2, "invisible"),
                (4, "climbable"),
                (8, "breakable"),
                (32, "conveyor"),
                (64, "slip"),
            ] {
                if t.flags & bit != 0 {
                    *flagged.entry(name).or_default() += 1;
                }
            }
        }
        println!(
            "collision: {} triangles ({bsp} world, {inst} instanced), {} materials",
            col.triangles.len(),
            col.materials.len()
        );
        println!(
            "  bounds (wu)  x {:.3}..{:.3}  y {:.3}..{:.3}  z {:.3}..{:.3}",
            clo.x, chi.x, clo.y, chi.y, clo.z, chi.z
        );
        let flags: Vec<String> = flagged.iter().map(|(k, v)| format!("{v} {k}")).collect();
        println!(
            "  flagged: {}",
            if flags.is_empty() {
                "none".to_string()
            } else {
                flags.join(", ")
            }
        );
    } else {
        println!("collision: none decoded");
    }
    if !scene.spawns.is_empty() {
        println!("spawn points: {}", scene.spawns.len());
        for sp in scene.spawns.iter().take(6) {
            println!(
                "  {:<10} {:?} at [{:.2}, {:.2}, {:.2}] wu facing {:.0} deg",
                sp.name,
                sp.kind,
                sp.position.x,
                sp.position.y,
                sp.position.z,
                sp.yaw.to_degrees()
            );
        }
    }
    println!("materials by triangles:");
    let mut order: Vec<usize> = (0..scene.materials.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(by_material[i]));
    for i in order {
        if by_material[i] == 0 {
            continue;
        }
        let m = &scene.materials[i];
        println!(
            "  {:>8} {:<4} {:<11} {}",
            by_material[i],
            m.shader_class,
            format!("{:?}", m.shading).to_lowercase(),
            m.tag_path
        );
    }

    if let Some(path) = obj {
        write_obj(scene, path).with_context(|| format!("writing {}", path.display()))?;
        println!("wrote {}", path.display());
        if !scene.collision.triangles.is_empty() {
            let cpath = path.with_extension("collision.obj");
            write_collision_obj(scene, &cpath)
                .with_context(|| format!("writing {}", cpath.display()))?;
            println!("wrote {}", cpath.display());
        }
    }
    Ok(())
}

/// The collision triangles as one OBJ object per source, same frame as
/// [`write_obj`].
fn write_collision_obj(scene: &Scene, path: &Path) -> std::io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    writeln!(w, "# {} collision from blam_cli; metres, Y up", scene.name)?;
    let mut n = 1u32;
    let mut last: Option<(blam_geometry::CollisionSource, u8)> = None;
    for t in &scene.collision.triangles {
        let key = (t.source, t.flags);
        if last != Some(key) {
            writeln!(w, "o {:?}_flags{}", t.source, t.flags)?;
            last = Some(key);
        }
        for p in t.points {
            let v = to_metres_y_up(p);
            writeln!(w, "v {} {} {}", v.x, v.y, v.z)?;
        }
        writeln!(w, "f {} {} {}", n, n + 1, n + 2)?;
        n += 3;
    }
    w.flush()
}

fn to_metres_y_up(h: Vec3) -> Vec3 {
    Vec3::new(h.x, h.z, -h.y) * WORLD_UNIT_METRES
}

/// Everything placed, baked into one OBJ: one object per placement, one
/// material per shader, vertices in metres with Y up.
fn write_obj(scene: &Scene, path: &Path) -> std::io::Result<()> {
    let mut w = BufWriter::new(std::fs::File::create(path)?);
    writeln!(
        w,
        "# {} from blam_cli; metres, Y up: (x, y, z) = 3.048 * halo (x, z, -y)",
        scene.name
    )?;
    let mtl_name = |i: usize| -> String {
        let m = &scene.materials[i];
        let base = m.tag_path.rsplit('\\').next().unwrap_or("mat");
        format!(
            "{i}_{}",
            base.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        )
    };
    let mut next_vertex = 1u32;
    for p in &scene.placements {
        if p.kind == Kind::Sky {
            continue;
        }
        let mesh = &scene.meshes[p.mesh as usize];
        writeln!(w, "o {}", p.name.replace(' ', "_"))?;
        let normal_m = glam::Mat3::from_mat4(p.transform).inverse().transpose();
        for (v, n) in mesh
            .positions
            .iter()
            .zip(mesh.normals.iter().chain(std::iter::repeat(&Vec3::Z)))
        {
            let wv = to_metres_y_up(p.transform.transform_point3(*v));
            let wn = {
                let n = normal_m * *n;
                Vec3::new(n.x, n.z, -n.y).normalize_or(Vec3::Y)
            };
            writeln!(w, "v {} {} {}", wv.x, wv.y, wv.z)?;
            writeln!(w, "vn {} {} {}", wn.x, wn.y, wn.z)?;
        }
        for uv in &mesh.uvs {
            writeln!(w, "vt {} {}", uv[0], uv[1])?;
        }
        let (b, c) = if p.mirrored { (2, 1) } else { (1, 2) };
        for part in &mesh.parts {
            writeln!(w, "usemtl {}", mtl_name(part.material as usize))?;
            for t in part.indices.as_chunks::<3>().0 {
                let i = |k: usize| next_vertex + t[k];
                writeln!(w, "f {0}/{0}/{0} {1}/{1}/{1} {2}/{2}/{2}", i(0), i(b), i(c))?;
            }
        }
        next_vertex += mesh.positions.len() as u32;
    }
    w.flush()
}
