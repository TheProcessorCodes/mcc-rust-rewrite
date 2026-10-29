//! MCC Halo 2 (classic graphics) maps read into the same
//! [`blam_geometry::Scene`] the Halo 3 readers produce, so the viewer, the
//! skating and the dirt bike run on them unchanged.
//!
//! Written from `docs/format/halo2.md`, whose facts were checked against the
//! user's maps with the probes in `docs/format/scripts/` (`h2_*.py`,
//! `probe_h2_*.py`). No code from other projects is copied.
//!
//! ```no_run
//! let path = std::path::Path::new("coagulation.map");
//! let map = blam_h2::Map::open(path)?;
//! let loaded = blam_h2::decode(&map, &blam_h2::Options::default())?;
//! println!("{} triangles", loaded.scene.triangle_count());
//! # Ok::<(), blam_h2::Error>(())
//! ```

pub mod bitmap;
pub mod cache;
pub mod collision;
pub mod lightmap;
pub mod section;
pub mod shader;

use std::collections::HashMap;
use std::path::PathBuf;

use blam_geometry::{
    BspInfo, ClusterInfo, CollisionMaterial, CollisionSource, Kind, Lighting, Lightmap, Material, Mesh, Placement, Scene,
    Shading, Spawn, SpawnKind, StartingLocation, TriggerVolume,
};
use blam_texture::Texture;
use glam::{Mat4, Vec3};

pub use cache::{Map, Reader, is_halo2};
pub use shader::Look;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to decode.
#[derive(Clone, Debug)]
pub struct Options {
    /// Scenery, crates, machines, controls and the sky.
    pub objects: bool,
    pub collision: bool,
    /// Resolve shaders and decode their bitmaps.
    pub textures: bool,
    pub verbose: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            objects: true,
            collision: true,
            textures: true,
            verbose: false,
        }
    }
}

/// Counts worth printing and pinning in tests.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    pub clusters: usize,
    pub instances: usize,
    pub instance_definitions: usize,
    pub bsp_collision_triangles: usize,
    pub instance_collision_triangles: usize,
    pub corrupt_loops: usize,
    pub objects: HashMap<Kind, usize>,
    pub models_failed: Vec<String>,
    pub bitmaps_failed: Vec<String>,
    /// Lightmap pages packed, and placements lit from them.
    pub lightmap_pages: usize,
    pub lightmapped: usize,
}

/// A decoded map.
pub struct Loaded {
    pub scene: Scene,
    /// How each scene material draws, by material index (`None`: the
    /// shader did not resolve).
    pub looks: Vec<Option<Look>>,
    /// The bitmaps the looks use, by tag index, each once.
    pub bitmaps: HashMap<u32, Texture>,
    /// The first sky's fog, when it has one.
    pub fog: Option<Fog>,
    /// Each `scene.lightmaps` entry's atlas of light colours (A8R8G8B8,
    /// `lightmap.rs`); its `Lighting::PerPixel` texcoords point into it.
    pub lightmaps: Vec<Texture>,
    pub stats: Stats,
}

/// A sky's fog (`sky ` `+0x48`, 0x18 each; `docs/format/halo2.md` 7):
/// its colour as stored (gamma space, like the textures), how thick it
/// gets, and where it starts and reaches that thickness, in world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub colour: [f32; 3],
    pub max_density: f32,
    pub start: f32,
    pub opaque: f32,
}

/// The fog of the scenario's first sky.
fn sky_fog(map: &Map, scnr: Reader<'_>) -> Result<Option<Fog>> {
    let Some(s) = scnr.block(0x8, 8)?.into_iter().next() else { return Ok(None) };
    let Some(sky) = map.tag_data(s.tag_ref(0)?) else { return Ok(None) };
    let Some(f) = sky.block(0x48, 0x18)?.into_iter().next() else { return Ok(None) };
    let fog = Fog {
        colour: f.vec3(0)?.to_array(),
        max_density: f.f32(0xC)?,
        start: f.f32(0x10)?,
        opaque: f.f32(0x14)?,
    };
    let ok = fog.colour.iter().all(|c| c.is_finite()) && fog.max_density > 0.0 && fog.opaque > fog.start;
    Ok(ok.then_some(fog))
}

/// Scene materials by shader datum.
#[derive(Default)]
struct Materials {
    by_shader: HashMap<u32, u32>,
    looks: Vec<Option<Look>>,
}

impl Materials {
    fn index(&mut self, map: &Map, scene: &mut Scene, shader: u32) -> u32 {
        if let Some(&i) = self.by_shader.get(&shader) {
            return i;
        }
        let look = match shader::resolve(map, shader) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("shader {}: {e}", map.tag_name(shader).unwrap_or("?"));
                None
            }
        };
        let shading = match &look {
            Some(l) if l.hidden => Shading::Invisible,
            Some(l) if l.blend != blam_texture::Blend::Opaque || l.water || l.sky => Shading::Translucent,
            Some(l) if l.alpha_test => Shading::AlphaTest,
            _ => Shading::Opaque,
        };
        let i = scene.materials.len() as u32;
        scene.materials.push(Material {
            tag_path: map.tag_name(shader).unwrap_or("").to_string(),
            shader_class: "shad".into(),
            shading,
            two_sided: look.as_ref().is_some_and(|l| l.two_sided),
            shader_tag: (shader != u32::MAX).then_some(shader & 0xFFFF),
        });
        self.looks.push(look);
        self.by_shader.insert(shader, i);
        i
    }
}

/// How much the sky model is grown: Coagulation's is about 150 wu in
/// radius; times 50 puts it about 7,500 wu out, where Halo 3's skies are.
pub const SKY_SCALE: f32 = 50.0;

/// The yaw, pitch, roll Blam composes (`blam_geometry::scenario::rotation`).
fn object_transform(position: Vec3, angles: Vec3, scale: f32) -> Mat4 {
    let q = blam_geometry::scenario::rotation(angles.x, angles.y, angles.z);
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), q, position)
}

/// Decodes the scenario's structure BSPs, objects, spawns and textures.
pub fn decode(map: &Map, options: &Options) -> Result<Loaded> {
    let scnr_tag = map
        .find(b"scnr", &map.scenario_path)
        .or_else(|| map.first_of(b"scnr"))
        .ok_or_else(|| Error::Invalid("no scenario tag".into()))?;
    let scnr = map
        .tag_data(scnr_tag.id)
        .ok_or_else(|| Error::Invalid("scenario has no data".into()))?;
    let mut scene = Scene {
        name: map.map_name.clone(),
        ..Default::default()
    };
    let mut materials = Materials::default();
    let mut stats = Stats::default();
    let mut lightmaps = Vec::new();

    // Structure BSPs (scenario +0x210, 0x44 each; the sbsp reference at +0x10).
    for (bi, b) in scnr.block(0x210, 0x44)?.into_iter().enumerate() {
        let sbsp_id = b.tag_ref(0x10)?;
        let Some(sbsp) = map.tag_data(sbsp_id) else {
            eprintln!("bsp {bi}: no structure bsp data");
            continue;
        };
        // The BSP's lightmap; a map without one still draws, by the
        // default sun.
        let pages = if options.textures {
            lightmap::read(map, b).unwrap_or_else(|e| {
                eprintln!("bsp {bi}: lightmap: {e}");
                None
            })
        } else {
            None
        };
        decode_bsp(map, sbsp, bi as u32, pages, options, &mut scene, &mut materials, &mut stats, &mut lightmaps)
            .map_err(|e| Error::Invalid(format!("bsp {bi} {}: {e}", map.tag_name(sbsp_id).unwrap_or("?"))))?;
    }
    if scene.bsps.is_empty() {
        return Err(Error::Invalid(format!("{} decoded no structure BSPs", map.scenario_path)));
    }

    if options.objects {
        decode_objects(map, scnr, &mut scene, &mut materials, &mut stats)?;
    }
    decode_spawns(map, scnr, &mut scene)?;
    let fog = sky_fog(map, scnr).unwrap_or_else(|e| {
        eprintln!("sky fog: {e}");
        None
    });

    let mut bitmaps = HashMap::new();
    if options.textures {
        let mut wanted: Vec<u32> = materials
            .looks
            .iter()
            .flatten()
            .flat_map(|l| [l.base, l.detail, l.secondary_detail].into_iter().flatten().map(|(b, _)| b))
            .collect();
        wanted.sort_unstable();
        wanted.dedup();
        for b in wanted {
            // The tag index is the datum's low half; the salt comes from the index.
            let Some(datum) = map.tags.get(b as usize).map(|t| t.id) else { continue };
            match bitmap::decode(map, datum, 0) {
                Ok(t) => {
                    bitmaps.insert(b, t);
                }
                Err(e) => stats.bitmaps_failed.push(format!("{}: {e}", map.tag_name(datum).unwrap_or("?"))),
            }
        }
        if options.verbose {
            eprintln!("textures: {} bitmaps decoded, {} failed", bitmaps.len(), stats.bitmaps_failed.len());
            for f in &stats.bitmaps_failed {
                eprintln!("  {f}");
            }
        }
    } else {
        for l in materials.looks.iter_mut().flatten() {
            l.base = None;
            l.detail = None;
            l.secondary_detail = None;
        }
    }
    Ok(Loaded {
        scene,
        looks: materials.looks,
        bitmaps,
        fog,
        lightmaps,
        stats,
    })
}

#[allow(clippy::too_many_arguments)]
fn decode_bsp(
    map: &Map,
    sbsp: Reader<'_>,
    index: u32,
    pages: Option<lightmap::Pages>,
    options: &Options,
    scene: &mut Scene,
    materials: &mut Materials,
    stats: &mut Stats,
    lightmaps: &mut Vec<Texture>,
) -> Result<()> {
    let holder = sbsp.map;
    let name = holder.tags.iter().find(|t| t.offset == Some(sbsp.at)).map_or(String::new(), |t| t.name.clone());
    let mask = 1u32 << index.min(31);
    let shaders: Vec<u32> = sbsp.block(0xA4, 0x20)?.into_iter().map(|m| m.tag_ref(0x8)).collect::<Result<_>>()?;
    let mut material = |scene: &mut Scene, k: i16| -> u32 {
        let shader = usize::try_from(k).ok().and_then(|k| shaders.get(k)).copied().unwrap_or(u32::MAX);
        materials.index(map, scene, shader)
    };
    let bounds = |r: Reader<'_>, at: usize| -> Result<(Vec3, Vec3)> {
        Ok((
            Vec3::new(r.f32(at)?, r.f32(at + 8)?, r.f32(at + 16)?),
            Vec3::new(r.f32(at + 4)?, r.f32(at + 12)?, r.f32(at + 20)?),
        ))
    };
    // One atlas for the BSP's pages: `Lighting::PerPixel` into it for every
    // cluster and instance that has a page and lightmap texcoords.
    let atlas = pages.as_ref().map(|p| lightmap::Atlas::pack(&p.pages, &format!("{name} lightmap")));
    let lightmap_index = scene.lightmaps.len() as u32;
    let lit = |page: Option<u32>, uvs: Vec<[f32; 2]>, stats: &mut Stats| -> Lighting {
        match (&atlas, page) {
            (Some(a), Some(p)) if a.has(p) && !uvs.is_empty() => {
                stats.lightmapped += 1;
                Lighting::PerPixel {
                    lightmap: lightmap_index,
                    uvs: uvs.into_iter().map(|uv| a.place(p, uv)).collect(),
                }
            }
            _ => Lighting::None,
        }
    };
    let cluster_page = |k: usize| pages.as_ref().and_then(|p| p.clusters.get(k).copied().flatten());
    let instance_page = |k: usize| pages.as_ref().and_then(|p| p.instances.get(k).copied().flatten());
    let world = bounds(sbsp, 0x34)?;
    scene.bounds = Some(match scene.bounds {
        Some((a, b)) => (a.min(world.0), b.max(world.1)),
        None => world,
    });

    // Clusters (+0x9C, 0xB0 each).
    let mut clusters = Vec::new();
    for (ci, c) in sbsp.block(0x9C, 0xB0)?.into_iter().enumerate() {
        let sky = c.u8(0x6C)? as i8;
        clusters.push(ClusterInfo {
            bounds: Some(bounds(c, 0x54)?),
            sky: u32::try_from(sky).ok(),
            atmosphere: None,
            acoustics: None,
        });
        let mesh_name = format!("{} cluster {ci}", name.rsplit('\\').next().unwrap_or(&name));
        let mesh = section::decode_lit(c, section::BSP, &mesh_name, &mut |k| material(scene, k))?;
        if let Some((mesh, uvs)) = mesh {
            stats.clusters += 1;
            let lighting = lit(cluster_page(ci), uvs, stats);
            let m = scene.meshes.len() as u32;
            scene.meshes.push(mesh);
            scene.placements.push(Placement {
                name: mesh_name,
                kind: Kind::Cluster,
                mesh: m,
                transform: Mat4::IDENTITY,
                mirrored: false,
                lighting,
                bsps: mask,
                sky: None,
                cluster: Some(ci as u32),
                fade_pixels: [0, 0],
            });
        }
    }

    // Instanced geometry: definitions (+0x138, 0xC8) placed by instances
    // (+0x140, 0x58).
    let definitions = sbsp.block(0x138, 0xC8)?;
    stats.instance_definitions += definitions.len();
    // Each definition's mesh and lightmap texcoords (over whichever page an
    // instance of it uses).
    let mut def_mesh: Vec<Option<Option<(u32, Vec<[f32; 2]>)>>> = vec![None; definitions.len()];
    let instances = sbsp.block(0x140, 0x58)?;
    let mut instance_transforms = Vec::new();
    for (ii, inst) in instances.iter().enumerate() {
        let scale = inst.f32(0)?;
        let scale = if scale == 0.0 { 1.0 } else { scale };
        let cols = [inst.vec3(0x4)?, inst.vec3(0x10)?, inst.vec3(0x1C)?].map(|v| v.to_array());
        let (transform, mirrored) = blam_geometry::render_model::matrix_from_rows(cols, inst.vec3(0x28)?.to_array(), scale);
        let def = inst.i16(0x34)?;
        let inst_name = holder.string(inst.u32(0x50)?).to_string();
        instance_transforms.push((def, transform, mirrored));
        let Some(slot) = usize::try_from(def).ok().filter(|&d| d < definitions.len()) else { continue };
        if def_mesh[slot].is_none() {
            let mesh_name = format!("instance {slot}");
            let mesh = section::decode_lit(definitions[slot], section::BSP, &mesh_name, &mut |k| material(scene, k))?;
            def_mesh[slot] = Some(mesh.map(|(m, uvs)| {
                scene.meshes.push(m);
                (scene.meshes.len() as u32 - 1, uvs)
            }));
        }
        if let Some(Some((m, uvs))) = &def_mesh[slot] {
            let m = *m;
            let lighting = lit(instance_page(ii), uvs.clone(), stats);
            stats.instances += 1;
            scene.placements.push(Placement {
                name: inst_name,
                kind: Kind::Instance,
                mesh: m,
                transform,
                mirrored,
                lighting,
                bsps: mask,
                sky: None,
                cluster: None,
                fade_pixels: [0, 0],
            });
        }
    }

    if options.collision {
        // Collision materials (+0xC, 0x14 each: old shader +0, new +0xC).
        let base = scene.collision.materials.len() as i16;
        for m in sbsp.block(0xC, 0x14)? {
            let new = m.tag_ref(0xC)?;
            let shader = if map.tag(new).is_some() { new } else { m.tag_ref(0)? };
            scene.collision.materials.push(CollisionMaterial {
                shader: map.tag_name(shader).unwrap_or("").to_string(),
                shader_class: "shad".into(),
            });
        }
        let mut out = std::mem::take(&mut scene.collision.triangles);
        if let Some(bsp) = sbsp.block(0x14, 0x40)?.into_iter().next() {
            let s = collision::triangulate(bsp, None, CollisionSource::Bsp(index), base, &mut out)?;
            stats.bsp_collision_triangles += s.triangles;
            stats.corrupt_loops += s.corrupt_loops;
        }
        for (def, transform, mirrored) in &instance_transforms {
            let Some(d) = usize::try_from(*def).ok().and_then(|d| definitions.get(d)) else { continue };
            let s = collision::triangulate(d.at(0x70), Some((*transform, *mirrored)), CollisionSource::Instance(index), base, &mut out)?;
            stats.instance_collision_triangles += s.triangles;
            stats.corrupt_loops += s.corrupt_loops;
        }
        scene.collision.triangles = out;
    }

    if let Some(a) = atlas {
        stats.lightmap_pages += a.rects.iter().filter(|r| r[2] > 0).count();
        scene.lightmaps.push(Lightmap {
            bsp: index,
            coefficients_bitmap: None,
            intensity_bitmap: None,
            // The viewer encodes the colours and sets the ranges.
            scales: [0.0; 5],
        });
        lightmaps.push(a.texture);
    }
    scene.bsps.push(BspInfo {
        index,
        name,
        bounds: world,
        clusters,
        portals: Vec::new(),
        locator: None,
    });
    Ok(())
}

/// A render model's meshes: per region, the first permutation at its most
/// detailed level of detail.
fn model_meshes(map: &Map, mode: u32, scene: &mut Scene, materials: &mut Materials, cache: &mut HashMap<u32, Vec<u32>>) -> Result<Vec<u32>> {
    if let Some(m) = cache.get(&mode) {
        return Ok(m.clone());
    }
    let model_name = map.tag_name(mode).unwrap_or("?").to_string();
    let r = map
        .tag_data(mode)
        .ok_or_else(|| Error::Invalid(format!("{model_name}: no data")))?;
    let shaders: Vec<u32> = r.block(96, 32)?.into_iter().map(|s| s.tag_ref(8)).collect::<Result<_>>()?;
    let sections = r.block(36, 92)?;
    let mut out = Vec::new();
    for region in r.block(28, 16)? {
        let Some(perm) = region.block(8, 16)?.into_iter().next() else { continue };
        // Levels of detail, lowest first: take the highest one present.
        let mut section = None;
        for k in (0..6).rev() {
            let s = perm.i16(4 + 2 * k)?;
            if s >= 0 {
                section = Some(s as usize);
                break;
            }
        }
        let Some(s) = section.and_then(|s| sections.get(s)) else { continue };
        let name = format!("{} {}", model_name.rsplit('\\').next().unwrap_or(&model_name), r.map.string(region.u32(0)?));
        let mesh: Option<Mesh> = section::decode(*s, section::MODEL, &name, &mut |k| {
            let shader = usize::try_from(k).ok().and_then(|k| shaders.get(k)).copied().unwrap_or(u32::MAX);
            materials.index(map, scene, shader)
        })?;
        if let Some(mesh) = mesh {
            scene.meshes.push(mesh);
            out.push(scene.meshes.len() as u32 - 1);
        }
    }
    cache.insert(mode, out.clone());
    Ok(out)
}

/// The render model behind an object tag: object `+0x34` names its model
/// (`hlmt`), whose first field names the render model (`mode`).
fn render_model_of(map: &Map, object: u32) -> Option<u32> {
    let o = map.tag_data(object)?;
    let hlmt = o.tag_ref(0x34).ok()?;
    let h = map.tag_data(hlmt)?;
    h.tag_ref(0).ok().filter(|&m| map.tag(m).is_some())
}

fn decode_objects(map: &Map, scnr: Reader<'_>, scene: &mut Scene, materials: &mut Materials, stats: &mut Stats) -> Result<()> {
    let mut cache: HashMap<u32, Vec<u32>> = HashMap::new();
    // (kind, placements, element size, palette)
    let lists = [
        (Kind::Scenery, 0x50, 0x5C, 0x58),
        (Kind::Machine, 0xA8, 0x48, 0xB0),
        (Kind::Control, 0xB8, 0x44, 0xC0),
        (Kind::Crate, 0x328, 0x4C, 0x330),
    ];
    for (kind, at, size, palette_at) in lists {
        let palette: Vec<u32> = scnr.block(palette_at, 0x28)?.into_iter().map(|p| p.tag_ref(0)).collect::<Result<_>>()?;
        for (k, e) in scnr.block(at, size)?.into_iter().enumerate() {
            let Some(&object) = usize::try_from(e.i16(0)?).ok().and_then(|p| palette.get(p)) else { continue };
            let object_name = map.tag_name(object).unwrap_or("?").to_string();
            let Some(mode) = render_model_of(map, object) else {
                stats.models_failed.push(object_name);
                continue;
            };
            let meshes = match model_meshes(map, mode, scene, materials, &mut cache) {
                Ok(m) => m,
                Err(e) => {
                    stats.models_failed.push(format!("{object_name}: {e}"));
                    continue;
                }
            };
            let scale = e.f32(0x20)?;
            let transform = object_transform(e.vec3(0x8)?, e.vec3(0x14)?, if scale > 0.0 { scale } else { 1.0 });
            let origin = e.i16(0x2C)?;
            let bsps = if (0..32).contains(&origin) { 1 << origin } else { 1 };
            for m in meshes {
                scene.placements.push(Placement {
                    name: format!("{} {k}", object_name.rsplit('\\').next().unwrap_or(&object_name)),
                    kind,
                    mesh: m,
                    transform,
                    mirrored: false,
                    lighting: Lighting::None,
                    bsps,
                    sky: None,
                    cluster: None,
                    fade_pixels: [0, 0],
                });
            }
            *stats.objects.entry(kind).or_default() += 1;
        }
    }

    // Skies (+0x8, a tag reference each): the sky's render model at +0,
    // drawn around the world.
    for (si, s) in scnr.block(0x8, 8)?.into_iter().enumerate() {
        let sky = s.tag_ref(0)?;
        let Some(sr) = map.tag_data(sky) else { continue };
        let mode = sr.tag_ref(0)?;
        if map.tag(mode).is_none() {
            continue;
        }
        let meshes = match model_meshes(map, mode, scene, materials, &mut cache) {
            Ok(m) => m,
            Err(e) => {
                stats.models_failed.push(format!("sky: {e}"));
                continue;
            }
        };
        // The game draws the sky (about 150 wu across) round the camera at
        // infinity; a still viewer puts it round the middle of the level,
        // grown to Halo 3's sky distance (`SKY_SCALE`).
        let transform = match scene.bounds {
            Some((lo, hi)) => Mat4::from_scale_rotation_translation(
                Vec3::splat(SKY_SCALE),
                glam::Quat::IDENTITY,
                Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z),
            ),
            None => Mat4::from_scale(Vec3::splat(SKY_SCALE)),
        };
        for m in meshes {
            scene.placements.push(Placement {
                name: format!("sky {si}"),
                kind: Kind::Sky,
                mesh: m,
                transform,
                mirrored: false,
                lighting: Lighting::None,
                bsps: 0,
                sky: Some(si as u32),
                cluster: None,
                fade_pixels: [0, 0],
            });
        }
        *stats.objects.entry(Kind::Sky).or_default() += 1;
    }
    Ok(())
}

/// Player starting locations (+0x100, 0x34 each) are the multiplayer
/// respawn points in Halo 2; trigger volumes (+0x108) and the kill list
/// (+0x230) give the kill volumes.
fn decode_spawns(map: &Map, scnr: Reader<'_>, scene: &mut Scene) -> Result<()> {
    for (k, e) in scnr.block(0x100, 0x34)?.into_iter().enumerate() {
        let position = e.vec3(0)?;
        let yaw = e.f32(0xC)?;
        if k == 0 {
            scene.starts.push(StartingLocation { position, yaw, pitch: 0.0 });
        }
        scene.spawns.push(Spawn {
            name: format!("player starting location {k}"),
            position,
            yaw,
            kind: SpawnKind::Respawn,
        });
    }
    let kills: Vec<i16> = scnr.block(0x230, 2)?.into_iter().map(|e| e.i16(0)).collect::<Result<_>>()?;
    for (k, e) in scnr.block(0x108, 0x44)?.into_iter().enumerate() {
        let mut name = scnr.map.string(e.u32(0)?).to_string();
        // A volume on the kill list kills whatever its name says.
        if kills.contains(&(k as i16)) && !name.to_ascii_lowercase().starts_with("kill") {
            name = format!("kill {name}");
        }
        scene.trigger_volumes.push(TriggerVolume {
            name,
            forward: e.vec3(0xC)?,
            up: e.vec3(0x18)?,
            position: e.vec3(0x24)?,
            extents: e.vec3(0x30)?,
        });
    }
    let _ = map;
    Ok(())
}
