//! Halo 2 maps in the viewer: `blam_h2` decodes the map into the same
//! `Scene` the Halo 3 readers make, and its material looks become the
//! viewer's [`Look`]s here. Everything is decoded up front (a multiplayer
//! map is small), so the world builds with no open cache file.
//!
//! What is drawn (first pass, `docs/viewer.md` "Halo 2 maps"): each
//! material's base map with its tiling, the detail map over it, alpha-tested
//! foliage, blended glass and additive glows; detail-blend ground mixes its
//! two detail maps by the base map's alpha through the terrain path.
//!
//! Lighting: Halo 2's lightmap pages are plain light colours (`blam_h2`
//! packs each BSP's pages into one atlas). The viewer's lightmap shader
//! reads Halo 3's SH coefficients, so each colour is written as the constant
//! term alone (`encode_lightmap`): it then lights every surface with that
//! colour whatever its normal, as Halo 2's lightmap does.

use std::path::Path;

use blam_geometry::Scene;
use blam_geometry::weapons::Weapon;
use halo3_weapons::assets::Arsenal;
use blam_texture::{Blend, Format, Kind, Texture, Xform, rgba};

use crate::textures::{LightmapTextures, Look, MaterialTextures, SurfaceLook, TerrainLayerLook, TerrainLook};
use crate::world::sky::PlainFogSetting;

/// Tag indices for the blend maps made here, past any real tag index.
const SYNTHETIC: u32 = 0x1_0000;

/// The map's scene and looks, its sky's fog for the camera, and its
/// weapons for walking.
pub fn load(path: &Path) -> anyhow::Result<(Scene, MaterialTextures, Option<PlainFogSetting>, Option<Arsenal>)> {
    let started = std::time::Instant::now();
    let map = blam_h2::Map::open(path)?;
    let loaded = blam_h2::decode(&map, &blam_h2::Options::default())?;
    let s = &loaded.stats;
    eprintln!(
        "halo 2 {}: {} clusters, {} instances ({} lightmapped from {} pages), {} triangles; collision {} + {} triangles; {} spawns; {} bitmaps ({} not decoded) in {:.1} s",
        map.map_name,
        s.clusters,
        s.instances,
        s.lightmapped,
        s.lightmap_pages,
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
            let (textures, range) = encode_lightmap(atlas, lightmap_scale());
            lm.scales = [range, 0.0, 0.0, 0.0, 0.0];
            Some(textures)
        })
        .collect();
    let mut out = Vec::with_capacity(looks.len());
    for (i, l) in looks.iter().enumerate() {
        out.push(l.as_ref().map(|l| convert(l, i as u32, &mut bitmaps)));
    }
    let arsenal = weapons(&map);
    Ok((
        scene,
        MaterialTextures {
            looks: out,
            bitmaps,
            lightmaps,
        },
        fog,
        arsenal,
    ))
}

/// The weapon a player starts with when the scenario names none.
const FALLBACK_LOADOUT: &str = r"objects\weapons\rifle\battle_rifle\battle_rifle";

/// The map's weapons (`docs/format/halo2.md` 11): those the netgame
/// equipment places and the starting weapons, each with its numbers from
/// its `weap` tag and Halo 2's own third-person model. `None` (logged) when
/// the map has none.
fn weapons(map: &blam_h2::Map) -> Option<Arsenal> {
    let items = blam_h2::weapons::netgame_items(map)
        .map_err(|e| eprintln!("weapons: netgame equipment not read: {e}"))
        .ok()?;
    let placements = blam_h2::weapons::weapon_placements(&items);
    let mut loadout = blam_h2::weapons::starting_weapons(map).unwrap_or_else(|e| {
        eprintln!("weapons: starting equipment not read: {e}");
        Vec::new()
    });
    if loadout.is_empty() {
        loadout.extend(map.find(b"weap", FALLBACK_LOADOUT).map(|t| t.id));
    }
    let mut wanted: Vec<u32> = placements.iter().filter_map(|p| p.tag).chain(loadout.iter().copied()).collect();
    wanted.sort_unstable();
    wanted.dedup();
    let read: Vec<Weapon> = wanted
        .into_iter()
        .filter_map(|w| match blam_h2::weapons::read_weapon(map, w) {
            Ok(w) => Some(w),
            Err(e) => {
                eprintln!("weapons: {} not read: {e}", map.tag_name(w).unwrap_or("?"));
                None
            }
        })
        .collect();
    let modes: Vec<u32> = read.iter().map(|w| w.render_model.unwrap_or(u32::MAX)).collect();
    let models = blam_h2::decode_models(map, &modes);
    for f in &models.failed {
        eprintln!("  weapon model: {f}");
    }
    let blam_h2::Models {
        scene,
        looks,
        mut bitmaps,
        meshes,
        ..
    } = models;
    let looks = looks
        .iter()
        .enumerate()
        .map(|(i, l)| l.as_ref().map(|l| convert(l, i as u32, &mut bitmaps)))
        .collect();
    let textures = MaterialTextures {
        looks,
        bitmaps,
        lightmaps: Vec::new(),
    };
    eprintln!(
        "halo 2 weapons: {} netgame items ({} weapons), starting with {}",
        items.len(),
        placements.len(),
        loadout.iter().filter_map(|t| map.tag_name(*t)).collect::<Vec<_>>().join(", ")
    );
    halo3_weapons::assets::from_scene(read.into_iter().zip(meshes).collect(), &scene, placements, &loadout, textures)
}

/// What a Halo 2 lightmap texel's light (sRGB, as stored) is multiplied by
/// before the viewer's exposure: `HALO2_LIGHTMAP_SCALE`, else
/// [`LIGHTMAP_SCALE`].
fn lightmap_scale() -> f32 {
    std::env::var("HALO2_LIGHTMAP_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|s: &f32| s.is_finite() && *s > 0.0)
        .unwrap_or(LIGHTMAP_SCALE)
}

/// Halo 2's lightmap texel to the viewer's irradiance, after its sRGB
/// value is made linear. Halo 2 lights in gamma space with the lightmap
/// doubled (overbright), `2^2.2` in linear terms, and the viewer multiplies
/// irradiance by its exposure (`lighting::EXPOSURE`, 4): `2^2.2 / 4`.
/// Checked by eye on Lockout.
pub const LIGHTMAP_SCALE: f32 = 1.15;

/// The SH constant term that gives irradiance `e` on every normal
/// (`blam_geometry::lightmap::irradiance_with_dominant`: `c0 * 0.886227 / pi`).
const C0_PER_IRRADIANCE: f32 = std::f32::consts::PI / 0.886227;

/// The viewer's lightmap arrays for one atlas of Halo 2 light colours, and
/// the range of c0. Layer 0 and 1 of the coefficients are c0's base and
/// residual (`luvw`: `((a + b) * 2 - 2) * a.alpha * b.alpha * range`); the
/// residual's colour is a fixed mid grey, so the base's colour carries the
/// hue and the two alphas the size (split as square roots, for precision in
/// the dark). The other coefficients and the dominant light are zero: their
/// ranges are 0, so their layers are left out (the shader clamps the layer
/// index) and the intensity array is one black texel.
pub(crate) fn encode_lightmap(atlas: &Texture, scale: f32) -> (LightmapTextures, f32) {
    let srgb = |b: u8| {
        let c = b as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let src = atlas.level(0, 0).unwrap_or(&[]);
    // c0 per texel, R G B.
    let c0: Vec<[f32; 3]> = src
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[2], p[1], p[0]].map(|b| srgb(b) * scale * C0_PER_IRRADIANCE))
        .collect();
    let range = c0.iter().flat_map(|c| c.iter().copied()).fold(0.0f32, f32::max).max(1e-6);
    const MID: u8 = 128;
    let mid = MID as f32 / 255.0;
    let mut base = Vec::with_capacity(src.len());
    let mut residual = Vec::with_capacity(src.len());
    for c in &c0 {
        let m = c[0].max(c[1]).max(c[2]) / range;
        let qa = (m.sqrt() * 255.0).round().clamp(0.0, 255.0);
        let qb = if qa > 0.0 { (m * 255.0 * 255.0 / qa).round().clamp(0.0, 255.0) } else { 0.0 };
        let l = qa / 255.0 * qb / 255.0 * range;
        let a = |v: f32| {
            if l > 0.0 { ((v / (2.0 * l) + 1.0 - mid) * 255.0).round().clamp(0.0, 255.0) as u8 } else { MID }
        };
        // Bytes B G R A.
        base.extend_from_slice(&[a(c[2]), a(c[1]), a(c[0]), qa as u8]);
        residual.extend_from_slice(&[MID, MID, MID, qb as u8]);
    }
    base.extend_from_slice(&residual);
    let array = |name: String, width, height, layers, pixels| Texture {
        name,
        tag_index: u32::MAX,
        bitmap_index: 0,
        kind: Kind::Array,
        width,
        height,
        layers,
        format: Format::A8R8G8B8,
        srgb: false,
        curve: 0,
        mips: vec![pixels],
    };
    (
        LightmapTextures {
            coefficients: array(format!("{} (c0)", atlas.name), atlas.width, atlas.height, 2, base),
            intensity: array(format!("{} (no dominant light)", atlas.name), 1, 1, 1, vec![MID, MID, MID, 0]),
        },
        range,
    )
}

pub(crate) fn convert(l: &blam_h2::Look, material: u32, bitmaps: &mut std::collections::HashMap<u32, Texture>) -> Look {
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

/// The map's ambience for the sound plugin, decoded when the returned loader
/// runs (on the plugin's thread): the BSPs' background loops and the
/// scenario's sound scenery, each permutation decoded from the
/// `sounds_*.dat` files beside the map (`blam_h2::sound`). A sound that will
/// not decode is left out with a note.
pub fn ambience_loader(path: &Path) -> crate::audio::halo2::PlainLoader {
    let path = path.to_path_buf();
    Box::new(move || {
        use crate::audio::halo2::{PlainAmbience, PlainClip, PlainDetail, PlainEmitter, PlainLoop, PlainTrack};
        use blam_h2::sound;
        let started = std::time::Instant::now();
        let map = blam_h2::Map::open(&path).map_err(|e| e.to_string())?;
        let found = sound::ambience(&map).map_err(|e| e.to_string())?;
        let files = sound::SoundFiles::beside(&path);
        let db = |d: f32| 10f32.powf(d / 20.0);
        // A sound's permutations, decoded, and its own gain and skip.
        let clips = |id: Option<u32>| -> Option<(Vec<PlainClip>, f32, f32, (f32, f32))> {
            let s = match sound::sound(&map, id?) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("halo 2 sound {}: {e}", map.tag_name(id?).unwrap_or("?"));
                    return None;
                }
            };
            let mut out = Vec::new();
            for (k, p) in s.permutations.iter().enumerate() {
                match files.decode(&s, k) {
                    Ok(pcm) => out.push(PlainClip {
                        rate: pcm.rate,
                        channels: pcm.channels,
                        samples: pcm.samples,
                        skip: p.skip,
                    }),
                    Err(e) => eprintln!("halo 2 sound {} {}: {e}", s.name, p.name),
                }
            }
            (!out.is_empty()).then_some((out, s.gain_db, s.skip_fraction, s.distance))
        };
        let looping = |id: u32| -> Option<(PlainLoop, (f32, f32))> {
            let l = match sound::looping(&map, id) {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("halo 2 looping sound {id:#x}: {e}");
                    return None;
                }
            };
            let tracks = l
                .tracks
                .iter()
                .filter_map(|t| {
                    let (clips, gain_db, _, _) = clips(t.looped)?;
                    Some(PlainTrack {
                        name: t.name.clone(),
                        gain: db(t.gain_db + gain_db),
                        clips,
                    })
                })
                .collect();
            let details = l
                .details
                .iter()
                .filter_map(|d| {
                    let (clips, gain_db, skip, _) = clips(d.sound)?;
                    Some(PlainDetail {
                        name: d.name.clone(),
                        gain: db(d.gain_db + gain_db),
                        clips,
                        period: d.period,
                        skip,
                        yaw: d.yaw,
                        pitch: d.pitch,
                        distance: d.distance,
                    })
                })
                .collect();
            Some((
                PlainLoop {
                    name: l.name,
                    tracks,
                    details,
                },
                l.distance,
            ))
        };
        let backgrounds = sound::background_loops(&found).into_iter().filter_map(|id| Some(looping(id)?.0)).collect();
        let emitters = found
            .emitters
            .iter()
            .filter_map(|e| {
                let (sound, distance) = looping(e.looping)?;
                Some(PlainEmitter {
                    position: e.position,
                    distance,
                    sound,
                })
            })
            .collect();
        eprintln!("halo 2 sound: decoded in {:.2} s", started.elapsed().as_secs_f32());
        Ok(PlainAmbience { backgrounds, emitters })
    })
}
