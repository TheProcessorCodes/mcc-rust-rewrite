//! Objects the scenario places in the world: scenery, crates, machines,
//! device controls and the sky. Follows
//! `docs/format/scenario-and-tag-reading.md` sections 2 and 3.

use std::collections::HashMap;

use glam::{Mat4, Quat, Vec3};

use blam_cache::{Block, CacheFile, Resources, TagEntry};

use crate::materials::MaterialTable;
use crate::render_model::{self, ModelParts};
use crate::scene::{Kind, Placement, Scene, Spawn, SpawnKind};
use crate::{Error, Result};

/// Which placement block and palette a kind of object lives in.
struct ObjectKind {
    kind: Kind,
    placements: usize,
    palette: usize,
    stride: usize,
}

const KINDS: [ObjectKind; 4] = [
    ObjectKind {
        kind: Kind::Scenery,
        placements: 0xB4,
        palette: 0xC0,
        stride: 180,
    },
    ObjectKind {
        kind: Kind::Machine,
        placements: 0x138,
        palette: 0x144,
        stride: 136,
    },
    ObjectKind {
        kind: Kind::Control,
        placements: 0x168,
        palette: 0x174,
        stride: 120,
    },
    ObjectKind {
        kind: Kind::Crate,
        placements: 0x5BC,
        palette: 0x5C8,
        stride: 176,
    },
];

/// Blam's yaw, pitch, roll (radians, about Z, then Y nose-up, then X) as a
/// rotation.
pub fn rotation(yaw: f32, pitch: f32, roll: f32) -> Quat {
    Quat::from_rotation_z(yaw) * Quat::from_rotation_y(-pitch) * Quat::from_rotation_x(roll)
}

/// Counts worth printing after a decode.
#[derive(Clone, Debug, Default)]
pub struct ObjectStats {
    pub placed: HashMap<Kind, u32>,
    pub skipped: u32,
    pub models_failed: Vec<String>,
}

/// Whether an object tag is one of the multiplayer spawn markers.
pub fn spawn_kind(tag_path: &str) -> Option<SpawnKind> {
    let path = tag_path.to_ascii_lowercase();
    let name = path.rsplit('\\').next().unwrap_or(&path);
    if !path.contains("spawning") {
        return None;
    }
    match name {
        "respawn_point" => Some(SpawnKind::Respawn),
        "respawn_point_invisible" => Some(SpawnKind::RespawnInvisible),
        "initial_spawn_point" => Some(SpawnKind::Initial),
        _ => None,
    }
}

/// Resolves `obje -> hlmt -> mode` from an object tag.
pub fn render_model_of<'a>(cache: &'a CacheFile, object: &TagEntry) -> Result<Option<&'a TagEntry>> {
    let model = cache.reader_at(cache.tag_offset(object)? + 0x34).tag_ref()?;
    let Some(hlmt) = model.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(None);
    };
    let render = cache.reader_at(cache.tag_offset(hlmt)?).tag_ref()?;
    Ok(render.index.and_then(|i| cache.tags.get(i)))
}

/// Decodes every placed object and the sky into `scene`.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    scene: &mut Scene,
    verbose: bool,
) -> Result<ObjectStats> {
    let scnr = cache.scenario().ok_or_else(|| {
        Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path))
    })?;
    let scnr_off = cache.tag_offset(scnr)?;
    let mut stats = ObjectStats::default();
    let mut models: HashMap<u32, Option<ModelParts>> = HashMap::new();

    // Object names, for placements that have one.
    let names_block: Block = cache.reader_at(scnr_off + 0xA8).block()?;
    let names: Vec<String> = names_block
        .elements(36)
        .map(|o| blam_cache::header_cstr(cache.bytes(), o, 32))
        .collect();

    let mut model_for = |cache: &CacheFile,
                         scene: &mut Scene,
                         materials: &mut MaterialTable,
                         object: &TagEntry,
                         stats: &mut ObjectStats|
     -> Option<ModelParts> {
        let mode = match render_model_of(cache, object) {
            Ok(Some(m)) => m,
            _ => return None,
        };
        models
            .entry(mode.index)
            .or_insert_with(|| match render_model::decode(cache, resources, materials, mode, scene) {
                Ok(parts) => Some(parts),
                Err(e) => {
                    stats.models_failed.push(format!("{}: {e}", mode.name()));
                    None
                }
            })
            .clone()
    };

    for k in &KINDS {
        let placements: Block = cache.reader_at(scnr_off + k.placements).block()?;
        let palette: Block = cache.reader_at(scnr_off + k.palette).block()?;
        let palette_tags: Vec<Option<&TagEntry>> = palette
            .elements(16)
            .map(|o| Ok(cache.reader_at(o).tag_ref()?.index.and_then(|i| cache.tags.get(i))))
            .collect::<Result<Vec<_>>>()?;
        for (i, o) in placements.elements(k.stride).enumerate() {
            let mut r = cache.reader_at(o);
            let palette_index = r.i16()?;
            let name_index = r.i16()?;
            let position = Vec3::from(r.at(o + 0x08).f32x3()?);
            let [yaw, pitch, roll] = r.at(o + 0x14).f32x3()?;
            let scale = r.at(o + 0x20).f32()?;
            // The object id's origin BSP (`zone-sets.md` 2); -1 for none.
            let origin_bsp = r.at(o + 0x3C).i16()?;
            let bsps = u32::try_from(origin_bsp)
                .ok()
                .filter(|b| *b < 32)
                .map_or(0, |b| 1 << b);
            let Some(object) = usize::try_from(palette_index)
                .ok()
                .and_then(|p| palette_tags.get(p).copied().flatten())
            else {
                stats.skipped += 1;
                continue;
            };
            let Some(parts) = model_for(cache, scene, materials, object, &mut stats) else {
                stats.skipped += 1;
                continue;
            };
            if k.kind == Kind::Scenery
                && let Some(kind) = spawn_kind(object.name())
            {
                scene.spawns.push(Spawn {
                    name: format!("spawn {}", scene.spawns.len()),
                    position,
                    yaw,
                    kind,
                });
            }
            let scale = if scale == 0.0 { 1.0 } else { scale };
            let world = Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                rotation(yaw, pitch, roll),
                position,
            );
            let name = usize::try_from(name_index)
                .ok()
                .and_then(|n| names.get(n))
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| {
                    format!(
                        "{} {i}",
                        object.name().rsplit('\\').next().unwrap_or("object")
                    )
                });
            for piece in &parts.pieces {
                scene.placements.push(Placement {
                    name: name.clone(),
                    kind: k.kind,
                    mesh: piece.mesh,
                    transform: world * piece.transform,
                    mirrored: piece.mirrored,
                    lighting: Default::default(),
                    bsps,
                    sky: None,
                    cluster: None,
                    fade_pixels: [0; 2],
                });
            }
            *stats.placed.entry(k.kind).or_default() += 1;
        }
    }

    // The sky: scenery objects drawn around everything, at the origin.
    let skies: Block = cache.reader_at(scnr_off + 0x34).block()?;
    for (sky_index, o) in skies.elements(20).enumerate() {
        // The BSPs the sky is active in (`zone-sets.md` 3). A sky active
        // nowhere (120_halo's `sky_02`) is not drawn when the map has others.
        let bsps = u32::from(cache.reader_at(o + 0x12).u16()?);
        if bsps == 0 && skies.count > 1 {
            continue;
        }
        let Some(sky) = cache.reader_at(o).tag_ref()?.index.and_then(|i| cache.tags.get(i)) else {
            continue;
        };
        let Some(parts) = model_for(cache, scene, materials, sky, &mut stats) else {
            continue;
        };
        for piece in &parts.pieces {
            scene.placements.push(Placement {
                name: format!("sky {}", parts.name),
                kind: Kind::Sky,
                mesh: piece.mesh,
                transform: piece.transform,
                mirrored: piece.mirrored,
                lighting: Default::default(),
                bsps,
                sky: Some(sky_index as u32),
                cluster: None,
                fade_pixels: [0; 2],
            });
        }
        *stats.placed.entry(Kind::Sky).or_default() += 1;
    }

    if verbose {
        let mut kinds: Vec<_> = stats.placed.iter().collect();
        kinds.sort_by_key(|(k, _)| format!("{k:?}"));
        let summary: Vec<String> = kinds.iter().map(|(k, n)| format!("{n} {k:?}")).collect();
        eprintln!(
            "objects: {} placed, {} skipped, {} models failed, {} spawn points",
            summary.join(", "),
            stats.skipped,
            stats.models_failed.len(),
            scene.spawns.len()
        );
        for f in &stats.models_failed {
            eprintln!("  model failed: {f}");
        }
    }
    Ok(stats)
}
