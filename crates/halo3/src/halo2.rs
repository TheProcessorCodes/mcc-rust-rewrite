//! Halo 2 maps in the viewer: `blam_h2` decodes the map into the same
//! `Scene` the Halo 3 readers make, and its material looks become the
//! viewer's [`Look`]s here. Everything is decoded up front (a multiplayer
//! map is small), so the world builds with no open cache file.
//!
//! What is drawn (first pass, `docs/viewer.md` "Halo 2 maps"): each
//! material's base map with its tiling, the detail map over it, alpha-tested
//! foliage, blended glass and additive glows; detail-blend ground mixes its
//! two detail maps by the base map's alpha through the terrain path. No
//! lightmaps yet: the world is lit by the default sun and sky.

use std::path::Path;

use blam_geometry::Scene;
use blam_texture::{Blend, Format, Kind, Texture, Xform, rgba};

use crate::textures::{Look, MaterialTextures, SurfaceLook, TerrainLayerLook, TerrainLook};

/// Tag indices for the blend maps made here, past any real tag index.
const SYNTHETIC: u32 = 0x1_0000;

pub fn load(path: &Path) -> anyhow::Result<(Scene, MaterialTextures)> {
    let started = std::time::Instant::now();
    let map = blam_h2::Map::open(path)?;
    let loaded = blam_h2::decode(&map, &blam_h2::Options::default())?;
    let s = &loaded.stats;
    eprintln!(
        "halo 2 {}: {} clusters, {} instances, {} triangles; collision {} + {} triangles; {} spawns; {} bitmaps ({} not decoded) in {:.1} s",
        map.map_name,
        s.clusters,
        s.instances,
        loaded.scene.triangle_count(),
        s.bsp_collision_triangles,
        s.instance_collision_triangles,
        loaded.scene.spawns.len(),
        loaded.bitmaps.len(),
        s.bitmaps_failed.len(),
        started.elapsed().as_secs_f32()
    );
    for f in s.models_failed.iter().chain(&s.bitmaps_failed) {
        eprintln!("  {f}");
    }
    let blam_h2::Loaded {
        scene, looks, mut bitmaps, ..
    } = loaded;
    let mut out = Vec::with_capacity(looks.len());
    for (i, l) in looks.iter().enumerate() {
        out.push(l.as_ref().map(|l| convert(l, i as u32, &mut bitmaps)));
    }
    Ok((
        scene,
        MaterialTextures {
            looks: out,
            bitmaps,
            lightmaps: Vec::new(),
        },
    ))
}

fn convert(l: &blam_h2::Look, material: u32, bitmaps: &mut std::collections::HashMap<u32, Texture>) -> Look {
    let have = |m: Option<(u32, Xform)>, bitmaps: &std::collections::HashMap<u32, Texture>| m.filter(|(b, _)| bitmaps.contains_key(b));
    let base = have(l.base, bitmaps);
    let detail = have(l.detail, bitmaps);
    let secondary = have(l.secondary_detail, bitmaps);
    let terrain = match (l.detail_blend, base, detail, secondary) {
        (true, Some((b, bx)), Some(d), Some(s2)) => blend_map(&bitmaps[&b], SYNTHETIC + material).map(|blend| {
            bitmaps.insert(SYNTHETIC + material, blend);
            TerrainLook {
                blend_map: Some(SYNTHETIC + material),
                blend_xform: bx,
                layers: [
                    Some(layer(b, bx, d)),
                    Some(layer(b, bx, s2)),
                    None,
                    None,
                ],
            }
        }),
        _ => None,
    };
    Look {
        texture: base.map(|(b, _)| b),
        xform: base.map_or(Xform::IDENTITY, |(_, x)| x),
        colour: None,
        blend: l.blend,
        alpha_test: l.alpha_test,
        two_sided: l.two_sided || l.blend != Blend::Opaque,
        // The sky's layers need Halo 2's sky blending (stars, glows and
        // cloud masks over a gradient); drawn as plain translucent meshes
        // they wall the canyon in flat blue, so the clear colour stands in.
        hidden: l.hidden || l.sky,
        water: l.water,
        terrain,
        surface: SurfaceLook {
            detail: if l.blend == Blend::Opaque { detail } else { None },
            ..Default::default()
        },
    }
}

fn layer(base: u32, base_xform: Xform, detail: (u32, Xform)) -> TerrainLayerLook {
    TerrainLayerLook {
        base,
        base_xform,
        detail: Some(detail),
        bump: None,
        specular: 0.0,
        power: 1.0,
        tint: [1.0; 3],
        analytical: 0.0,
    }
}

/// The weights of a detail-blend ground (`docs/format/halo2.md` 8.4): the
/// base map's alpha picks the secondary detail. Red = 1 - alpha weights
/// layer 0 (base x detail), green = alpha layer 1 (base x secondary).
fn blend_map(base: &Texture, tag_index: u32) -> Option<Texture> {
    let px = rgba::decode_level(base.format, base.width, base.height, base.level(0, 0)?).ok()?;
    let mut bgra = Vec::with_capacity(px.len());
    for p in px.as_chunks::<4>().0 {
        let a = p[3];
        // Bytes B G R A.
        bgra.extend_from_slice(&[0, a, 255 - a, 255]);
    }
    Some(Texture {
        name: format!("{} (detail blend weights)", base.name),
        tag_index,
        bitmap_index: 0,
        kind: Kind::D2,
        width: base.width,
        height: base.height,
        layers: 1,
        format: Format::A8R8G8B8,
        srgb: false,
        curve: 0,
        mips: vec![bgra],
    })
}
