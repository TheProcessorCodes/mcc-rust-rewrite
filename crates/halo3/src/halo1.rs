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
//!
//! Missions (`main.rs` `MISSIONS`, an [`Insertion`]): only the mission's
//! BSPs are decoded, one loaded at a time and switched by the map's own BSP
//! switch volumes as in the game (`blam_h1`'s zone sets); the player starts
//! on the dirt bike where a named object of the map stands; the map's own
//! scripts (`blam_h1::script`) run on Halo 3's runtime (`halo3_script`).

use std::collections::HashMap;
use std::path::Path;

use blam_geometry::{Kind, Scene, Spawn, SpawnKind, StartingLocation};
use blam_texture::Texture;
use bevy::math::Vec3;

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

pub use halo3_script::h1::Insertion;

/// A Halo CE map: its scene and looks, its sky's fog for the camera and
/// its sky model's looks; for a mission, where it starts and its scripts.
pub struct Loaded {
    pub scene: Scene,
    pub textures: MaterialTextures,
    pub fog: Option<PlainFogSetting>,
    pub sky: SkyData,
    /// A mission's start (feet position, facing).
    pub start: Option<(Vec3, f32)>,
    /// A mission's scripts.
    pub scripts: Option<blam_script::ScriptData>,
    /// The effects a mission's scripts name (`halo1_effects.rs`).
    pub effects: Option<halo3_effects::EffectAssets>,
    /// The map, for what is read after the world (the encounters,
    /// `halo1_ai.rs`).
    pub map: blam_h1::Map,
    /// Each scene object's device, for the scripts (`h1_objects.rs`).
    pub devices: Vec<Option<halo3_script::h1_objects::H1Device>>,
}

pub fn load(path: &Path, insertion: Option<&Insertion>) -> anyhow::Result<Loaded> {
    let started = std::time::Instant::now();
    let map = blam_h1::Map::open(path)?;
    let options = blam_h1::Options {
        bsps: insertion.map(|i| i.bsps.to_vec()),
        ..Default::default()
    };
    let loaded = blam_h1::decode(&map, &options)?;
    let s = &loaded.stats;
    eprintln!(
        "halo ce {}: {} lightmap meshes ({} lit from {} pages), {} triangles; collision {} triangles (+{} of objects); {} spawns; {} scenery; {} bitmaps ({} not decoded) in {:.1} s",
        map.map_name,
        s.lightmaps,
        s.lightmapped,
        s.lightmap_pages,
        loaded.scene.triangle_count(),
        s.collision_triangles,
        s.object_collision_triangles,
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
    let (start, scripts) = match insertion {
        Some(ins) => mission_start(&map, &mut scene, ins)?,
        None => (None, None),
    };
    let effects = scripts.as_ref().map(|d| crate::halo1_effects::load(&map, d));
    let devices = crate::h1_objects::devices(&map, &scene);
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
        start,
        scripts,
        effects,
        map,
        devices,
    })
}

/// A mission's start: the named object's spot becomes the scene's only
/// start and spawn, so a fall, a death or a restart comes back there and
/// never to the map's own start in a BSP that is not loaded (on 120_halo
/// that made a fall loop). The scripts are read here too.
fn mission_start(
    map: &blam_h1::Map,
    scene: &mut Scene,
    ins: &Insertion,
) -> anyhow::Result<(Option<(Vec3, f32)>, Option<blam_script::ScriptData>)> {
    let object = blam_h1::object_named(map, ins.start_at)
        .ok_or_else(|| anyhow::anyhow!("{}: no object named {}", map.map_name, ins.start_at))?;
    let (position, yaw) = (object.position, object.angles.x);
    eprintln!(
        "mission: starts at {} ({}) halo [{:.2}, {:.2}, {:.2}] facing {:.0} degrees",
        ins.start_at,
        object.tag,
        position.x,
        position.y,
        position.z,
        yaw.to_degrees()
    );
    scene.starts = vec![StartingLocation { position, yaw, pitch: 0.0 }];
    scene.spawns = vec![Spawn {
        name: ins.start_at.to_string(),
        position,
        yaw,
        kind: SpawnKind::Respawn,
    }];
    let scripts = match blam_h1::script::read(map) {
        Ok(d) if !d.scripts.is_empty() => {
            eprintln!("scripts: {} scripts, {} globals", d.scripts.len(), d.globals.len());
            Some(d)
        }
        Ok(_) => None,
        Err(e) => {
            eprintln!("scripts: not read ({e})");
            None
        }
    };
    Ok((Some((position, yaw)), scripts))
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
        let is_mask = |b: &u32| bitmaps.get(b).is_some_and(|t| t.format == blam_texture::Format::A8);
        let colour = |b: &u32| bitmaps.contains_key(b) && !is_mask(b);
        let pick = l.maps.iter().rev().find(|(b, _)| colour(b)).copied().or(l.look.base.filter(|(b, _)| colour(b)));
        let Some((bitmap, xform)) = pick else { continue };
        // Colour maps are stored in gamma space: sampled as sRGB.
        let mut t = bitmaps[&bitmap].clone();
        t.srgb = true;
        out.bitmaps.insert(bitmap, t);
        // A cloud layer's alpha is its own map (`cloud mask`, A8) under
        // the colour map: it goes in the detail slot, as neutral grey
        // with the mask in alpha, so it cuts the clouds without tinting.
        let mask = l.maps.iter().find(|(b, _)| is_mask(b)).and_then(|&(b, xform)| {
            let key = MASK + b;
            if !out.bitmaps.contains_key(&key) {
                out.bitmaps.insert(key, neutral_mask(&bitmaps[&b], key)?);
            }
            Some(SkyMap { bitmap: key, xform })
        });
        out.looks.insert(
            m,
            SkyLook {
                albedo: Albedo::Default,
                maps: [Some(SkyMap { bitmap, xform }), mask, None, None, None],
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

/// Keys for the masks made here, past any real tag index.
const MASK: u32 = 0x2_0000;

/// The sky shader multiplies its detail map by this (`DETAIL` in
/// `sky.wesl`); a detail of `1 / DETAIL` leaves the colour as it is.
const SKY_DETAIL: f32 = 4.59479;

/// An A8 mask as an A8R8G8B8 detail map: colour `1 / SKY_DETAIL` (stored
/// linear), the mask in alpha, every mip level kept.
fn neutral_mask(mask: &Texture, key: u32) -> Option<Texture> {
    let grey = (255.0 / SKY_DETAIL).round() as u8;
    let mut mips = Vec::with_capacity(mask.mips.len());
    for level in 0..mask.mips.len() as u32 {
        let (w, h) = blam_texture::Format::level_dims(mask.width, mask.height, level);
        let px = blam_texture::rgba::decode_level(mask.format, w, h, mask.level(level, 0)?).ok()?;
        // Bytes B G R A.
        mips.push(px.as_chunks::<4>().0.iter().flat_map(|p| [grey, grey, grey, p[3]]).collect());
    }
    Some(Texture {
        name: format!("{} (sky mask)", mask.name),
        tag_index: key,
        format: blam_texture::Format::A8R8G8B8,
        srgb: false,
        mips,
        ..mask.clone()
    })
}
