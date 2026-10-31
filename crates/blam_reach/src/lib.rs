//! Reads MCC Halo: Reach maps into the same [`Scene`] the Halo 3 readers
//! produce, so the viewer, the skate, bike and on-foot code and the texture
//! loader treat them alike.
//!
//! Reach is Halo 3's engine a game later: the cache file opens with
//! `blam_cache` (which tells the two apart by their header size,
//! [`blam_cache::Engine`]), the resource system and the shader and bitmap
//! chains are Halo 3's at other offsets (`blam_texture` reads both), and
//! the collision geometry is Halo 3's with one more block. What differs is
//! the tags' layouts and the render geometry, which live here. Every
//! offset is in `docs/format/reach.md`, checked against the user's maps by
//! `docs/format/scripts/probe_reach_*.py`.
//!
//! Not read yet: Reach's lightmaps (dual vMF, not Halo 3's SH; placements
//! carry [`blam_geometry::Lighting::None`]), cluster portals and the
//! cluster lookup, atmosphere and fog.

pub mod geometry;
pub mod layout;
pub mod model;
pub mod sbsp;
pub mod scenario;

use std::path::Path;

use blam_cache::{CacheFile, Engine};
use blam_geometry::materials::MaterialTable;
use blam_geometry::{Error, Result, Scene};

/// Whether the file at `path` is an MCC Reach cache file (header only).
pub fn is_reach(path: impl AsRef<Path>) -> bool {
    blam_cache::engine_of(path) == Some(Engine::Reach)
}

/// What a decode found, for logging.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub bsps: Vec<(u32, String, sbsp::BspStats)>,
    pub collision_triangles: usize,
    pub objects: blam_geometry::scenario::ObjectStats,
}

/// Decodes the BSPs of the map's first zone set (all of them when it has
/// none), their collision, the scenario's objects and sky into `scene`.
pub fn decode_into(cache: &CacheFile, scene: &mut Scene) -> Result<Stats> {
    if cache.engine() != Engine::Reach {
        return Err(Error::Invalid(format!("{} is not a Reach map", cache.path().display())));
    }
    let resources = cache.resources()?;
    if let Err(e) = scenario::decode_layout(cache, scene) {
        eprintln!("reach: zone sets and starting locations not read: {e}");
    }
    let bsps = sbsp::structure_bsps(cache)?;
    if bsps.is_empty() {
        return Err(Error::Invalid(format!("{} lists no structure BSPs", cache.header.scenario_path)));
    }
    // Multiplayer maps have one zone set; the BSPs outside it (`_hidden`)
    // are never loaded by the game.
    let wanted = scene.zone_sets.first().map_or(u32::MAX, |z| z.bsps);
    let mut materials = MaterialTable::default();
    let mut stats = Stats::default();
    for bsp in &bsps {
        if bsp.index < 32 && wanted & (1 << bsp.index) == 0 {
            continue;
        }
        match sbsp::decode(cache, &resources, &mut materials, bsp, scene) {
            Ok(s) => stats.bsps.push((bsp.index, bsp.sbsp.name().to_string(), s)),
            Err(e) => {
                eprintln!("reach: bsp {} {} skipped: {e}", bsp.index, bsp.sbsp.name());
                continue;
            }
        }
        match sbsp::decode_collision(cache, &resources, bsp.sbsp, bsp.index, scene) {
            Ok(c) => {
                if c.corrupt_loops > 0 {
                    eprintln!("reach: bsp {} collision: {} corrupt surface loops", bsp.index, c.corrupt_loops);
                }
            }
            Err(e) => eprintln!("reach: collision of bsp {} not decoded: {e}", bsp.index),
        }
        resources.clear_page_cache();
    }
    if stats.bsps.is_empty() {
        return Err(Error::Invalid(format!("no structure BSP of {} decoded", cache.header.scenario_path)));
    }
    stats.collision_triangles = scene.collision.triangles.len();
    match scenario::decode(cache, &resources, &mut materials, scene) {
        Ok(o) => stats.objects = o,
        Err(e) => eprintln!("reach: objects not decoded: {e}"),
    }
    resources.clear_page_cache();
    Ok(stats)
}
