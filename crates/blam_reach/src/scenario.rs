//! What a Reach scenario places: scenery, crates, machines and controls,
//! the spawn points (respawn-point scenery, as in Halo 3), the sky, the
//! player starting locations, trigger volumes and the zone sets. Follows
//! `docs/format/reach.md` 2.

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use blam_cache::{Block, CacheFile, Resources, TagEntry};
use blam_geometry::materials::MaterialTable;
use blam_geometry::render_model::ModelParts;
use blam_geometry::scenario::{ObjectStats, rotation, spawn_kind};
use blam_geometry::scene::{Kind, Placement, Scene, Spawn, StartingLocation, TriggerVolume, ZoneSet};
use blam_geometry::Result;

use crate::layout::{OBJECT_MODEL, placement, scnr};
use crate::model;
use crate::sbsp::scenario_offset;

struct ObjectKind {
    kind: Kind,
    placements: usize,
    palette: usize,
    stride: usize,
}

const KINDS: [ObjectKind; 4] = [
    ObjectKind {
        kind: Kind::Scenery,
        placements: scnr::SCENERY,
        palette: scnr::SCENERY_PALETTE,
        stride: scnr::SCENERY_SIZE,
    },
    ObjectKind {
        kind: Kind::Machine,
        placements: scnr::MACHINES,
        palette: scnr::MACHINE_PALETTE,
        stride: scnr::MACHINE_SIZE,
    },
    ObjectKind {
        kind: Kind::Control,
        placements: scnr::CONTROLS,
        palette: scnr::CONTROL_PALETTE,
        stride: scnr::CONTROL_SIZE,
    },
    ObjectKind {
        kind: Kind::Crate,
        placements: scnr::CRATES,
        palette: scnr::CRATE_PALETTE,
        stride: scnr::CRATE_SIZE,
    },
];

/// `obje -> hlmt -> mode`: the object's model at `+0x64`, the model's
/// render model at `+0`.
pub fn render_model_of<'a>(cache: &'a CacheFile, object: &TagEntry) -> Result<Option<&'a TagEntry>> {
    let model = cache.reader_at(cache.tag_offset(object)? + OBJECT_MODEL).tag_ref()?;
    let Some(hlmt) = model.index.and_then(|i| cache.tags.get(i)) else {
        return Ok(None);
    };
    if cache.tags.class_of(hlmt).code.as_str() != "hlmt" {
        return Ok(None);
    }
    let render = cache.reader_at(cache.tag_offset(hlmt)?).tag_ref()?;
    Ok(render
        .index
        .and_then(|i| cache.tags.get(i))
        .filter(|t| cache.tags.class_of(t).code.as_str() == "mode"))
}

/// Multiplayer objects that only mark places for game types (hill and
/// territory markers, boundaries, Forge's invisible blockers): never drawn
/// in Reach outside their game type.
fn is_game_type_object(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains(r"objects\multi\spawning")
        || p.contains(r"objects\multi\generic")
        || p.contains(r"objects\multi\named_location")
        || p.contains(r"objects\multi\invisible_cube")
        || p.contains(r"objects\multi\models\mp_")
        || p.contains(r"objects\multi\nut_blockers")
        || p.contains(r"objects\multi\boundary")
        || p.contains(r"objects\levels\shared\damage_sphere")
}

/// Decodes every placed object and the sky into `scene`.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    scene: &mut Scene,
) -> Result<ObjectStats> {
    let s = scenario_offset(cache)?;
    let mut stats = ObjectStats::default();
    let mut models: HashMap<u32, Option<ModelParts>> = HashMap::new();
    let mut model_for = |scene: &mut Scene, materials: &mut MaterialTable, object: &TagEntry, stats: &mut ObjectStats| {
        let mode = match render_model_of(cache, object) {
            Ok(Some(m)) => m,
            _ => return None,
        };
        models
            .entry(mode.index)
            .or_insert_with(|| match model::decode(cache, resources, materials, mode, scene) {
                Ok(parts) => Some(parts),
                Err(e) => {
                    stats.models_failed.push(format!("{}: {e}", mode.name()));
                    None
                }
            })
            .clone()
    };

    // Object names (8 bytes: string id first).
    let names: Vec<String> = cache
        .reader_at(s + scnr::OBJECT_NAMES)
        .block()?
        .elements(8)
        .map(|o| {
            cache
                .reader_at(o)
                .string_id()
                .ok()
                .and_then(|id| cache.string(id))
                .unwrap_or("")
                .to_string()
        })
        .collect();

    for k in &KINDS {
        let placements: Block = cache.reader_at(s + k.placements).block()?;
        let palette: Vec<Option<&TagEntry>> = cache
            .reader_at(s + k.palette)
            .block()?
            .elements(16)
            .map(|o| Ok(cache.reader_at(o).tag_ref()?.index.and_then(|i| cache.tags.get(i))))
            .collect::<Result<_>>()?;
        for (i, o) in placements.elements(k.stride).enumerate() {
            let mut r = cache.reader_at(o);
            let palette_index = r.i16()?;
            let name_index = r.i16()?;
            let position = Vec3::from(r.at(o + placement::POSITION).f32x3()?);
            let [yaw, pitch, roll] = r.at(o + placement::ROTATION).f32x3()?;
            let scale = r.at(o + placement::SCALE).f32()?;
            let origin_bsp = r.at(o + placement::ORIGIN_BSP).i16()?;
            let bsps = u32::try_from(origin_bsp).ok().filter(|b| *b < 32).map_or(0, |b| 1 << b);
            let Some(object) = usize::try_from(palette_index).ok().and_then(|p| palette.get(p).copied().flatten())
            else {
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
            if is_game_type_object(object.name()) {
                continue;
            }
            let Some(parts) = model_for(scene, materials, object, &mut stats) else {
                stats.skipped += 1;
                continue;
            };
            let scale = if scale == 0.0 { 1.0 } else { scale };
            let world = Mat4::from_scale_rotation_translation(Vec3::splat(scale), rotation(yaw, pitch, roll), position);
            let name = usize::try_from(name_index)
                .ok()
                .and_then(|n| names.get(n))
                .filter(|n| !n.is_empty())
                .cloned()
                .unwrap_or_else(|| format!("{} {i}", object.name().rsplit('\\').next().unwrap_or("object")));
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

    // The sky: a scenery object (`+0`) drawn round everything at the origin.
    let skies: Block = cache.reader_at(s + scnr::SKIES).block()?;
    let mut seen = Vec::new();
    for (sky_index, o) in skies.elements(scnr::SKY_SIZE).enumerate() {
        let Some(sky) = cache.reader_at(o).tag_ref()?.index.and_then(|i| cache.tags.get(i)) else {
            continue;
        };
        // 45_aftship lists the same sky six times.
        if seen.contains(&sky.index) {
            continue;
        }
        seen.push(sky.index);
        let Some(parts) = model_for(scene, materials, sky, &mut stats) else {
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
                bsps: 0,
                sky: Some(sky_index as u32),
                cluster: None,
                fade_pixels: [0; 2],
            });
        }
        *stats.placed.entry(Kind::Sky).or_default() += 1;
    }
    Ok(stats)
}

/// The zone sets, player starting locations and trigger volumes. Volumes
/// the scenario lists as kill triggers are named `kill ...` when their own
/// name does not say so, which is how the viewer finds kill volumes
/// (`TriggerVolume::kills`).
pub fn decode_layout(cache: &CacheFile, scene: &mut Scene) -> Result<()> {
    let s = scenario_offset(cache)?;
    let name_of = |id| cache.string(id).map_or_else(String::new, str::to_string);
    for o in cache.reader_at(s + scnr::ZONE_SETS).block()?.elements(scnr::ZONE_SET_SIZE) {
        let mut r = cache.reader_at(o);
        let name = name_of(r.string_id()?);
        let bsps = r.at(o + scnr::ZONE_SET_BSPS).u32()?;
        scene.zone_sets.push(ZoneSet { name, bsps });
    }
    for o in cache
        .reader_at(s + scnr::PLAYER_STARTING_LOCATIONS)
        .block()?
        .elements(scnr::PLAYER_STARTING_LOCATION_SIZE)
    {
        let mut r = cache.reader_at(o);
        let position = Vec3::from(r.f32x3()?);
        let [yaw, pitch] = r.f32x2()?;
        scene.starts.push(StartingLocation { position, yaw, pitch });
    }
    let kill: Vec<i16> = cache
        .reader_at(s + scnr::KILL_TRIGGERS)
        .block()?
        .elements(4)
        .map(|o| cache.reader_at(o).i16())
        .collect::<std::result::Result<_, _>>()?;
    for (i, o) in cache.reader_at(s + scnr::TRIGGER_VOLUMES).block()?.elements(scnr::TRIGGER_VOLUME_SIZE).enumerate() {
        let mut r = cache.reader_at(o);
        let mut name = name_of(r.string_id()?);
        let mut r = r.at(o + 0x10);
        let forward = Vec3::from(r.f32x3()?);
        let up = Vec3::from(r.f32x3()?);
        let position = Vec3::from(r.f32x3()?);
        let extents = Vec3::from(r.f32x3()?);
        let mut volume = TriggerVolume {
            name: name.clone(),
            forward,
            up,
            position,
            extents,
        };
        if kill.contains(&(i as i16)) && !volume.kills() {
            name = format!("kill {name}");
            volume.name = name;
        }
        scene.trigger_volumes.push(volume);
    }
    Ok(())
}
