//! Decodes Halo 3 geometry out of a cache file into [`Scene`]s.
//!
//! The decoders are written from the format notes in `docs/format`. Each
//! module says which note it follows.

pub mod atmosphere;
pub mod camera_fx;
pub mod collision;
pub mod lightmap;
pub mod materials;
pub mod rails;
pub mod render_geometry;
pub mod render_model;
pub mod sbsp;
pub mod scenario;
pub mod scene;
pub mod vertex;

use std::path::Path;

pub use rails::RailCensus;
pub use scene::{
    Bone, Collision, CollisionMaterial, CollisionSource, CollisionTriangle, Kind, Lighting,
    Lightmap, Material, Mesh, Part, Placement, Scene, Shading, Skin, Spawn, SpawnKind, WORLD_UNIT_METRES,
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cache(#[from] blam_cache::Error),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to decode from a map.
#[derive(Clone, Debug)]
pub struct Options {
    /// Only these structure BSPs (by position in the scenario), or all.
    pub bsps: Option<Vec<u32>>,
    /// Also place the scenario's scenery, crates, machines, controls and sky.
    pub objects: bool,
    /// Also decode the real collision BSP.
    pub collision: bool,
    /// Report progress and counts here.
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            bsps: None,
            objects: true,
            collision: true,
            verbose: false,
        }
    }
}

/// Opens a map and decodes its structure BSPs into one [`Scene`].
pub fn load_map(path: &Path) -> Result<Scene> {
    load_map_with(path, &Options::default())
}

pub fn load_map_with(path: &Path, options: &Options) -> Result<Scene> {
    let cache = blam_cache::CacheFile::open(path)?;
    let mut scene = Scene {
        name: cache.header.map_name.clone(),
        ..Default::default()
    };
    decode_into(&cache, options, &mut scene)?;
    Ok(scene)
}

/// Decodes the structure BSPs of an open map into `scene`.
pub fn decode_into(
    cache: &blam_cache::CacheFile,
    options: &Options,
    scene: &mut Scene,
) -> Result<Vec<sbsp::BspStats>> {
    let resources = cache.resources()?;
    let bsps = sbsp::structure_bsps(cache)?;
    if bsps.is_empty() {
        return Err(Error::Invalid(format!(
            "{} lists no structure BSPs",
            cache.header.scenario_path
        )));
    }
    let mut materials = materials::MaterialTable::default();
    let mut stats = Vec::new();
    for bsp in &bsps {
        if options
            .bsps
            .as_ref()
            .is_some_and(|wanted| !wanted.contains(&bsp.index))
        {
            continue;
        }
        let started = std::time::Instant::now();
        // One broken BSP (010_jungle's intro BSP has no geometry page) must
        // not lose the rest of the map.
        let s = match sbsp::decode(cache, &resources, &mut materials, bsp, scene) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("bsp {} {} skipped: {e}", bsp.index, bsp.sbsp.name());
                continue;
            }
        };
        if options.verbose {
            eprintln!(
                "bsp {} {}: {} clusters ({} drawn), {} instances ({} drawn), {} sections ({} with geometry), {} shaders, {:.2} s",
                bsp.index,
                bsp.sbsp.name(),
                s.clusters,
                s.cluster_placements,
                s.instances,
                s.instance_placements,
                s.sections,
                s.sections_with_geometry,
                s.shaders,
                started.elapsed().as_secs_f32()
            );
        }
        if options.collision {
            match collision::decode(cache, &resources, bsp.sbsp, bsp.index, &mut scene.collision) {
                Ok(c) => {
                    if options.verbose {
                        eprintln!(
                            "  collision: {} bsp triangles, {} instance triangles from {} instances ({} skipped), {} corrupt loops",
                            c.bsp_triangles, c.instance_triangles, c.instances_placed, c.instances_skipped, c.corrupt_loops
                        );
                    }
                }
                Err(e) => eprintln!("  collision of bsp {} not decoded: {e}", bsp.index),
            }
        }
        stats.push(s);
        // A 90 MB page per BSP adds up on campaign maps.
        resources.clear_page_cache();
    }
    if options.objects {
        let started = std::time::Instant::now();
        scenario::decode(cache, &resources, &mut materials, scene, options.verbose)?;
        if options.verbose {
            eprintln!("objects decoded in {:.2} s", started.elapsed().as_secs_f32());
        }
        resources.clear_page_cache();
    }
    Ok(stats)
}
