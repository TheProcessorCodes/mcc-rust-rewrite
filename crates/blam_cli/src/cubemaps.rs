//! `blam cubemaps <map>`: each structure BSP's reflection cube maps (the
//! bitmap tag, its cubes, the capture points per cluster) and how the map's
//! shaders use environment maps and parallax. See `docs/format/cubemaps.md`.

use std::collections::BTreeMap;

use anyhow::Result;
use blam_cache::CacheFile;
use blam_geometry::Options;
use blam_texture::bitmap;
use blam_texture::shader;

pub fn run(cache: &CacheFile) -> Result<()> {
    let mut scene = blam_geometry::Scene::default();
    blam_geometry::decode_into(
        cache,
        &Options {
            objects: true,
            collision: false,
            ..Options::default()
        },
        &mut scene,
    )?;
    for bsp in &scene.bsps {
        println!("bsp {} {}", bsp.index, bsp.name);
        let Some(tag) = bsp.cubemap_bitmap else {
            println!("  no cube map tag");
            continue;
        };
        let header = bitmap::read_tag(cache, tag)?;
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for b in &header.bitmaps {
            *kinds
                .entry(format!("{}x{} type {} format {} mips {} curve {}", b.width, b.height, b.kind, b.format, b.mipmap_count, b.curve))
                .or_default() += 1;
        }
        println!("  #{tag} {}: {} bitmaps", header.name, header.bitmaps.len());
        for (k, n) in kinds {
            println!("    {n} x {k}");
        }
        let used: std::collections::BTreeSet<u32> = bsp.cubemaps.iter().map(|p| p.cube).collect();
        let clusters: std::collections::BTreeSet<u32> = bsp.cubemaps.iter().map(|p| p.cluster).collect();
        println!(
            "  {} capture points in {} of {} clusters, {} distinct cubes (highest {:?})",
            bsp.cubemaps.len(),
            clusters.len(),
            bsp.clusters.len(),
            used.len(),
            used.last()
        );
        for p in bsp.cubemaps.iter().take(12) {
            println!(
                "    cluster {:3} cube {:3} (scenario {:3}) at ({:.2}, {:.2}, {:.2})",
                p.cluster, p.cube, p.id, p.position.x, p.position.y, p.position.z
            );
        }
    }

    // How the materials use environment maps and parallax.
    let mut env: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut parallax: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (mi, m) in scene.materials.iter().enumerate() {
        let Some(tag) = m.shader_tag else { continue };
        let Ok(s) = shader::resolve(cache, tag) else { continue };
        let e = s
            .option("environment_mapping")
            .or_else(|| s.option("environment_map"))
            .unwrap_or("-")
            .to_string();
        *env.entry((s.class.clone(), e)).or_default() += 1;
        if let Some(p) = s.option("parallax").filter(|p| *p != "off") {
            // Where it is drawn first: a vertex of the first placement using it.
            let at = scene.placements.iter().find_map(|pl| {
                let mesh = &scene.meshes[pl.mesh as usize];
                let part = mesh.parts.iter().find(|x| x.material == mi as u32)?;
                let v = mesh.positions.get(*part.indices.first()? as usize)?;
                Some(pl.transform.transform_point3(*v))
            });
            parallax.entry(p.to_string()).or_default().push(format!(
                "{} (height_scale {:?}, height_map {:?}) at {:?}",
                s.name,
                s.param("height_scale").map(|v| v[0]),
                s.map("height_map").map(|m| m.bitmap_name.clone()),
                at.map(|a| [a.x, a.y, a.z])
            ));
        }
    }
    println!("environment mapping by shader class:");
    for ((class, e), n) in env {
        println!("  {class} {e:<26} {n}");
    }
    println!("parallax:");
    for (p, names) in parallax {
        println!("  {p}: {}", names.len());
        for n in names {
            println!("    {n}");
        }
    }
    Ok(())
}
