//! Halo CE maps in the viewer: `blam_h1` decodes the map into the same
//! `Scene` the other readers make, and its material looks (Halo 2's
//! [`blam_h2::Look`]) become the viewer's [`Look`]s through the Halo 2
//! conversion (`halo2.rs`). Everything is decoded up front, so the world
//! builds with no open cache file.
//!
//! What is drawn: each material's base map, its detail map over it, the
//! blended ground's two details mixed by the base map's alpha (the terrain
//! path), alpha-tested foliage, the transparent shaders' first map by
//! their framebuffer blend, and the sky model through the sky material
//! (each sky shader's last colour map, unlit).
//!
//! Lighting: Halo CE's lightmap pages are plain light colours (R5G6B5),
//! packed by `blam_h1` into one atlas per BSP and written as Halo 3's
//! constant SH term like Halo 2's (`halo2::encode_lightmap`).

use std::collections::HashMap;
use std::path::Path;

use blam_geometry::{Kind, Scene};
use blam_texture::Texture;

use crate::textures::MaterialTextures;
use crate::world::sky::{Albedo, IllumMode, Map as SkyMap, PlainFogSetting, SkyData, SkyLook};

/// What a Halo CE lightmap texel's light (sRGB, as stored) is multiplied by
/// before the viewer's exposure: `HALO1_LIGHTMAP_SCALE`, else
/// [`LIGHTMAP_SCALE`].
fn lightmap_scale() -> f32 {
    std::env::var("HALO1_LIGHTMAP_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f32| s.is_finite() && *s > 0.0)
        .unwrap_or(LIGHTMAP_SCALE)
}

/// Halo CE's lightmap texel to the viewer's irradiance: Halo 2's number
/// (`halo2::LIGHTMAP_SCALE`, the doubled lightmap over the viewer's
/// exposure); checked by eye on Blood Gulch.
pub const LIGHTMAP_SCALE: f32 = 1.15;

/// A Halo CE map: its scene and looks, its sky's fog for the camera and
/// its sky model's looks.
pub struct Loaded {
    pub scene: Scene,
    pub textures: MaterialTextures,
    pub fog: Option<PlainFogSetting>,
    pub sky: SkyData,
}

pub fn load(path: &Path) -> anyhow::Result<Loaded> {
    let started = std::time::Instant::now();
    let map = blam_h1::Map::open(path)?;
    let loaded = blam_h1::decode(&map, &blam_h1::Options::default())?;
    let s = &loaded.stats;
    eprintln!(
        "halo ce {}: {} lightmap meshes ({} lit from {} pages), {} triangles; collision {} triangles; {} spawns; {} scenery; {} bitmaps ({} not decoded) in {:.1} s",
        map.map_name,
        s.lightmaps,
        s.lightmapped,
        s.lightmap_pages,
        loaded.scene.triangle_count(),
        s.collision_triangles,
        loaded.scene.spawns.len(),
        s.objects.get(&Kind::Scenery).copied().unwrap_or(0),
        loaded.bitmaps.len(),
        s.bitmaps_failed.len(),
        started.elapsed().as_secs_f32()
    );
    for f in s.models_failed.iter().chain(&s.bitmaps_failed) {
        eprintln!("  {f}");
    }
    let blam_h1::Loaded {
        mut scene,
        looks,
        mut bitmaps,
        lightmaps,
        fog,
        ..
    } = loaded;
    if let Some(f) = fog {
        eprintln!(
            "sky fog: colour {:.2?}, up to {:.2} from {} to {} wu",
            f.colour, f.max_density, f.start, f.opaque
        );
    }
    let fog = fog.map(|f| PlainFogSetting {
        colour: f.colour,
        max_density: f.max_density,
        start: f.start,
        opaque: f.opaque,
    });
    let lightmaps = lightmaps
        .iter()
        .zip(&mut scene.lightmaps)
        .map(|(atlas, lm)| {
            let (textures, range) = crate::halo2::encode_lightmap(atlas, lightmap_scale());
            lm.scales = [range, 0.0, 0.0, 0.0, 0.0];
            Some(textures)
        })
        .collect();
    let sky = sky_looks(&scene, &looks, &bitmaps);
    let mut out = Vec::with_capacity(looks.len());
    for (i, l) in looks.iter().enumerate() {
        out.push(l.as_ref().map(|l| {
            let mut look = l.look.clone();
            // A sky material with a sky look is drawn by the sky material;
            // one without stays hidden (the clear colour shows).
            if sky.looks.contains_key(&(i as u32)) {
                look.sky = false;
            }
            crate::halo2::convert(&look, i as u32, &mut bitmaps)
        }));
    }
    Ok(Loaded {
        scene,
        textures: MaterialTextures {
            looks: out,
            bitmaps,
            lightmaps,
        },
        fog,
        sky,
    })
}

/// The sky model's materials as sky looks: the last colour map of each
/// shader's stack (the sky gradient under its stars, the clouds over their
/// mask), drawn unlit as stored, blended the way the shader says.
fn sky_looks(scene: &Scene, looks: &[Option<blam_h1::ShaderLook>], bitmaps: &HashMap<u32, Texture>) -> SkyData {
    let mut out = SkyData {
        light_scale: 1.0,
        ..SkyData::default()
    };
    let mut materials: Vec<u32> = scene
        .placements
        .iter()
        .filter(|p| p.kind == Kind::Sky)
        .flat_map(|p| scene.meshes[p.mesh as usize].parts.iter().map(|part| part.material))
        .collect();
    materials.sort_unstable();
    materials.dedup();
    for m in materials {
        let Some(Some(l)) = looks.get(m as usize) else { continue };
        if l.look.hidden {
            continue;
        }
        let colour = |b: &u32| bitmaps.get(b).is_some_and(|t| t.format != blam_texture::Format::A8);
        let pick = l.maps.iter().rev().find(|(b, _)| colour(b)).copied().or(l.look.base.filter(|(b, _)| colour(b)));
        let Some((bitmap, xform)) = pick else { continue };
        out.bitmaps.insert(bitmap, bitmaps[&bitmap].clone());
        out.looks.insert(
            m,
            SkyLook {
                albedo: Albedo::Default,
                maps: [Some(SkyMap { bitmap, xform }), None, None, None, None],
                albedo_colour: [1.0; 4],
                illum: None,
                illum_mode: IllumMode::Off,
                edge_fade: None,
                blend: l.look.blend,
                lit: false,
                diffuse: 1.0,
            },
        );
    }
    out
}
