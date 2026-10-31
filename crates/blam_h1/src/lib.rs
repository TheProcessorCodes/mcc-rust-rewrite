//! MCC Halo: Combat Evolved (classic graphics) maps read into the same
//! [`blam_geometry::Scene`] the Halo 3 readers produce, so the viewer, the
//! skating and the dirt bike run on them unchanged.
//!
//! Written from `docs/format/halo1.md`, whose facts were checked against the
//! user's maps with the probes in `docs/format/scripts/` (`h1_common.py`,
//! `probe_h1_*.py`). No code from other projects is copied.
//!
//! ```no_run
//! let path = std::path::Path::new("bloodgulch.map");
//! let map = blam_h1::Map::open(path)?;
//! let loaded = blam_h1::decode(&map, &blam_h1::Options::default())?;
//! println!("{} triangles", loaded.scene.triangle_count());
//! # Ok::<(), blam_h1::Error>(())
//! ```

pub mod bitmap;
pub mod cache;
pub mod collision;
pub mod model;
pub mod shader;

use std::collections::HashMap;
use std::path::PathBuf;

use blam_geometry::{
    BspInfo, CollisionMaterial, CollisionSource, Kind, Lighting, Lightmap, Material, Mesh, Part, Placement, Scene,
    Shading, Spawn, SpawnKind, StartingLocation, TriggerVolume,
};
use blam_h2::Fog;
use blam_h2::lightmap::Atlas;
use blam_texture::Texture;
use glam::{Mat4, Vec3};

pub use cache::{Map, Reader, is_halo1};
pub use shader::Shading as ShaderLook;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot read {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Unsupported(String),
}

impl From<blam_h2::Error> for Error {
    fn from(e: blam_h2::Error) -> Error {
        Error::Invalid(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// What to decode.
#[derive(Clone, Debug)]
pub struct Options {
    /// Scenery, machines, controls, light fixtures and the sky.
    pub objects: bool,
    pub collision: bool,
    /// Resolve shaders and decode their bitmaps and the lightmaps.
    pub textures: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            objects: true,
            collision: true,
            textures: true,
        }
    }
}

/// Counts worth printing and pinning in tests.
#[derive(Clone, Debug, Default)]
pub struct Stats {
    /// Lightmaps of the BSPs (one mesh each) and their triangles.
    pub lightmaps: usize,
    pub bsp_triangles: usize,
    pub collision_triangles: usize,
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
    pub looks: Vec<Option<ShaderLook>>,
    /// The bitmaps the looks use, by tag index, each once.
    pub bitmaps: HashMap<u32, Texture>,
    /// The first sky's outdoor fog, when it has one.
    pub fog: Option<Fog>,
    /// Each `scene.lightmaps` entry's atlas of light colours (A8R8G8B8);
    /// its `Lighting::PerPixel` texcoords point into it.
    pub lightmaps: Vec<Texture>,
    pub stats: Stats,
}

/// Scene materials by shader datum.
#[derive(Default)]
struct Materials {
    by_shader: HashMap<u32, u32>,
    looks: Vec<Option<ShaderLook>>,
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
        let shading = match look.as_ref().map(|l| &l.look) {
            Some(l) if l.hidden => Shading::Invisible,
            Some(l) if l.blend != blam_texture::Blend::Opaque || l.water => Shading::Translucent,
            Some(l) if l.alpha_test => Shading::AlphaTest,
            _ => Shading::Opaque,
        };
        let i = scene.materials.len() as u32;
        scene.materials.push(Material {
            tag_path: map.tag_name(shader).unwrap_or("").to_string(),
            shader_class: map.class_of(shader).map_or(String::new(), |c| String::from_utf8_lossy(&c).into_owned()),
            shading,
            two_sided: look.as_ref().is_some_and(|l| l.look.two_sided),
            shader_tag: (shader != u32::MAX).then_some(shader & 0xFFFF),
        });
        self.looks.push(look);
        self.by_shader.insert(shader, i);
        i
    }
}

/// How much the sky model is grown, as for Halo 2 (`blam_h2::SKY_SCALE`).
pub const SKY_SCALE: f32 = blam_h2::SKY_SCALE;

/// The yaw, pitch, roll Blam composes (`blam_geometry::scenario::rotation`).
fn object_transform(position: Vec3, angles: Vec3) -> Mat4 {
    let q = blam_geometry::scenario::rotation(angles.x, angles.y, angles.z);
    Mat4::from_rotation_translation(q, position)
}

/// Decodes the scenario's structure BSPs, objects, spawns and textures.
pub fn decode(map: &Map, options: &Options) -> Result<Loaded> {
    let scnr = map
        .tag_data(map.scenario)
        .ok_or_else(|| Error::Invalid("scenario has no data".into()))?;
    let mut scene = Scene {
        name: map.map_name.clone(),
        ..Default::default()
    };
    let mut materials = Materials::default();
    let mut stats = Stats::default();
    let mut lightmaps = Vec::new();

    for (bi, chunk) in map.bsps()?.iter().enumerate() {
        decode_bsp(map, chunk, bi as u32, options, &mut scene, &mut materials, &mut stats, &mut lightmaps)
            .map_err(|e| Error::Invalid(format!("bsp {bi} {}: {e}", map.tag_name(chunk.tag).unwrap_or("?"))))?;
    }
    if scene.bsps.is_empty() {
        return Err(Error::Invalid(format!("{} decoded no structure BSPs", map.map_name)));
    }
    if options.objects {
        decode_objects(map, scnr, &mut scene, &mut materials, &mut stats)?;
    }
    decode_spawns(scnr, &mut scene)?;
    let fog = sky_fog(map, scnr).unwrap_or_else(|e| {
        eprintln!("sky fog: {e}");
        None
    });
    let mut bitmaps = HashMap::new();
    if options.textures {
        bitmaps = decode_bitmaps(map, &materials.looks, &mut stats.bitmaps_failed);
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

/// The bitmaps `looks` use (base, details and every stacked map), by tag
/// index, each decoded once; failures are listed in `failed`.
fn decode_bitmaps(map: &Map, looks: &[Option<ShaderLook>], failed: &mut Vec<String>) -> HashMap<u32, Texture> {
    let mut wanted: Vec<u32> = looks
        .iter()
        .flatten()
        .flat_map(|l| {
            [l.look.base, l.look.detail, l.look.secondary_detail]
                .into_iter()
                .flatten()
                .chain(l.maps.iter().copied())
                .map(|(b, _)| b)
        })
        .collect();
    wanted.sort_unstable();
    wanted.dedup();
    let mut bitmaps = HashMap::new();
    for b in wanted {
        let Some(datum) = map.tags.get(b as usize).map(|t| t.id) else { continue };
        match bitmap::decode(map, datum, 0) {
            Ok(t) => {
                bitmaps.insert(b, t);
            }
            Err(e) => failed.push(format!("{}: {e}", map.tag_name(datum).unwrap_or("?"))),
        }
    }
    bitmaps
}

/// The lightmap bitmap's pages (sbsp `+0`), widened to A8R8G8B8 for the
/// atlas. A page that does not decode stays empty (its rect is left out).
fn lightmap_pages(map: &Map, bitmap: u32) -> Result<Vec<Texture>> {
    let count = bitmap::images(map, bitmap)?.len();
    let mut pages = Vec::with_capacity(count);
    for k in 0..count {
        pages.push(bitmap::decode(map, bitmap, k).unwrap_or_else(|e| {
            eprintln!("lightmap page {k}: {e}");
            Texture {
                name: String::new(),
                tag_index: u32::MAX,
                bitmap_index: k as u32,
                kind: blam_texture::Kind::D2,
                width: 0,
                height: 0,
                layers: 1,
                format: blam_texture::Format::A8,
                srgb: false,
                curve: 0,
                mips: Vec::new(),
            }
        }));
    }
    Ok(pages)
}

/// Bytes per rendered BSP vertex (position, normal, binormal, tangent,
/// texcoord) and per lightmap vertex (normal, texcoord).
const RENDERED: usize = 56;
const LIGHTMAP_VERTEX: usize = 20;

#[allow(clippy::too_many_arguments)]
fn decode_bsp(
    map: &Map,
    chunk: &cache::BspChunk,
    index: u32,
    options: &Options,
    scene: &mut Scene,
    materials: &mut Materials,
    stats: &mut Stats,
    lightmaps: &mut Vec<Texture>,
) -> Result<()> {
    let sbsp = map.sbsp(chunk);
    let name = map.tag_name(chunk.tag).unwrap_or("").to_string();
    let short = name.rsplit('\\').next().unwrap_or(&name).to_string();
    let mask = 1u32 << index.min(31);
    let b = |k: usize| sbsp.f32(0xC8 + 4 * k);
    let world = (Vec3::new(b(0)?, b(2)?, b(4)?), Vec3::new(b(1)?, b(3)?, b(5)?));
    scene.bounds = Some(match scene.bounds {
        Some((lo, hi)) => (lo.min(world.0), hi.max(world.1)),
        None => world,
    });

    // One atlas for the BSP's lightmap pages.
    let lightmap_bitmap = sbsp.tag_ref(0)?;
    let atlas = if options.textures && map.tag(lightmap_bitmap).is_some() {
        Some(Atlas::pack(&lightmap_pages(map, lightmap_bitmap)?, &format!("{short} lightmap")))
    } else {
        None
    };
    let lightmap_index = scene.lightmaps.len() as u32;

    // Surfaces (`+0xF8`): three u16 vertex indices each, local to the
    // lightmap material that lists them.
    let surfaces_n = sbsp.block_count(0xF8)?;
    let surfaces_at = match sbsp.block(0xF8, 6)?.first() {
        Some(r) => r.at,
        None => 0,
    };
    let surfaces = map.slice(surfaces_at, surfaces_n * 6)?;
    let (vertex_area, vertex_size) = chunk.vertices;
    let vertices = map.slice(vertex_area, vertex_size)?;
    let f = |o: usize| -> Result<f32> {
        vertices
            .get(o..o + 4)
            .map(|b| f32::from_le_bytes(b.try_into().unwrap()))
            .ok_or_else(|| Error::Invalid(format!("vertex read at {o:#x} past the BSP vertex buffer")))
    };

    // Lightmaps (`+0x104`, 0x20 each): a page (i16 at +0, -1: none) and
    // its materials (`+0x14`, 0x100 each). Each lightmap becomes one mesh.
    for (li, lm) in sbsp.block(0x104, 0x20)?.into_iter().enumerate() {
        let page = u32::try_from(lm.i16(0)?).ok();
        let mut mesh = Mesh {
            name: format!("{short} lightmap {li}"),
            ..Default::default()
        };
        let mut lm_uvs: Vec<[f32; 2]> = Vec::new();
        for m in lm.block(0x14, 0x100)? {
            let shader = m.tag_ref(0)?;
            let first = m.i32(0x14)?.max(0) as usize;
            let count = m.i32(0x18)?.max(0) as usize;
            let nv = m.u32(0xB4)? as usize;
            let voff = m.u32(0xB8)? as usize;
            let nl = m.u32(0xC8)? as usize;
            let loff = m.u32(0xCC)? as usize;
            if nv == 0 || count == 0 {
                continue;
            }
            let base = mesh.positions.len() as u32;
            for k in 0..nv {
                let o = voff + RENDERED * k;
                mesh.positions.push(Vec3::new(f(o)?, f(o + 4)?, f(o + 8)?));
                mesh.normals.push(Vec3::new(f(o + 12)?, f(o + 16)?, f(o + 20)?));
                mesh.uvs.push([f(o + 48)?, f(o + 52)?]);
                lm_uvs.push(if nl == nv {
                    let o = loff + LIGHTMAP_VERTEX * k;
                    [f(o + 12)?, f(o + 16)?]
                } else {
                    [0.0, 0.0]
                });
            }
            let tris = surfaces
                .get(first * 6..(first + count) * 6)
                .ok_or_else(|| Error::Invalid(format!("lightmap {li}: surfaces {first}+{count} past the block")))?;
            let mut indices = Vec::with_capacity(count * 3);
            for t in tris.as_chunks::<6>().0 {
                let [a, b, c] = [0, 2, 4].map(|o| u16::from_le_bytes([t[o], t[o + 1]]) as u32);
                if a as usize >= nv || b as usize >= nv || c as usize >= nv {
                    continue;
                }
                // Stored clockwise: (a, c, b) faces along the normals.
                indices.extend([a + base, c + base, b + base]);
            }
            stats.bsp_triangles += indices.len() / 3;
            let material = materials.index(map, scene, shader);
            mesh.parts.push(Part { material, indices });
        }
        if mesh.parts.is_empty() {
            continue;
        }
        stats.lightmaps += 1;
        let lighting = match (&atlas, page) {
            (Some(a), Some(p)) if a.has(p) => {
                stats.lightmapped += 1;
                Lighting::PerPixel {
                    lightmap: lightmap_index,
                    uvs: lm_uvs.iter().map(|uv| a.place(p, *uv)).collect(),
                }
            }
            _ => Lighting::None,
        };
        let m = scene.meshes.len() as u32;
        scene.meshes.push(mesh);
        scene.placements.push(Placement {
            name: format!("{short} lightmap {li}"),
            kind: Kind::Cluster,
            mesh: m,
            transform: Mat4::IDENTITY,
            mirrored: false,
            lighting,
            bsps: mask,
            sky: None,
            cluster: None,
            fade_pixels: [0, 0],
        });
    }

    if options.collision {
        // Collision materials (`+0xA4`, 0x14 each: the shader at +0).
        let base = scene.collision.materials.len() as i16;
        for m in sbsp.block(0xA4, 0x14)? {
            let shader = m.tag_ref(0)?;
            scene.collision.materials.push(CollisionMaterial {
                shader: map.tag_name(shader).unwrap_or("").to_string(),
                shader_class: map.class_of(shader).map_or(String::new(), |c| String::from_utf8_lossy(&c).into_owned()),
            });
        }
        if let Some(bsp) = sbsp.block(0xB0, 0x60)?.into_iter().next() {
            let s = collision::triangulate(bsp, CollisionSource::Bsp(index), base, &mut scene.collision.triangles)?;
            stats.collision_triangles += s.triangles;
            stats.corrupt_loops += s.corrupt_loops;
        }
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
        clusters: Vec::new(),
        portals: Vec::new(),
        locator: None,
    });
    Ok(())
}

/// The model behind an object tag (object `+0x28`, a `mod2` reference).
fn model_of(map: &Map, object: u32) -> Option<u32> {
    let o = map.tag_data(object)?;
    o.tag_ref(0x28).ok().filter(|&m| map.tag(m).is_some_and(|t| t.is(b"mod2")))
}

/// A model's meshes in the scene, decoded once.
fn model_meshes(map: &Map, mode: u32, scene: &mut Scene, materials: &mut Materials, cache: &mut HashMap<u32, Vec<u32>>) -> Result<Vec<u32>> {
    if let Some(m) = cache.get(&mode) {
        return Ok(m.clone());
    }
    let meshes = model::meshes(map, mode, &mut |shader| materials.index(map, scene, shader))?;
    let mut out = Vec::with_capacity(meshes.len());
    for mesh in meshes {
        out.push(scene.meshes.len() as u32);
        scene.meshes.push(mesh);
    }
    cache.insert(mode, out.clone());
    Ok(out)
}

/// Render models decoded on their own (weapons): a scene of their meshes
/// and its materials' looks and bitmaps, like [`Loaded`].
pub struct Models {
    pub scene: Scene,
    pub looks: Vec<Option<ShaderLook>>,
    pub bitmaps: HashMap<u32, Texture>,
    /// Per requested model, its meshes (indices into `scene.meshes`), in
    /// model space; empty when it failed (listed in `failed`).
    pub meshes: Vec<Vec<u32>>,
    pub failed: Vec<String>,
}

/// Decodes gbxmodels (`mod2` datums) the way scenery is.
pub fn decode_models(map: &Map, modes: &[u32]) -> Models {
    let mut scene = Scene::default();
    let mut materials = Materials::default();
    let mut cache = HashMap::new();
    let mut failed = Vec::new();
    let meshes = modes
        .iter()
        .map(|&m| {
            model_meshes(map, m, &mut scene, &mut materials, &mut cache).unwrap_or_else(|e| {
                failed.push(format!("{}: {e}", map.tag_name(m).unwrap_or("?")));
                Vec::new()
            })
        })
        .collect();
    let bitmaps = decode_bitmaps(map, &materials.looks, &mut failed);
    Models {
        scene,
        looks: materials.looks,
        bitmaps,
        meshes,
        failed,
    }
}

fn decode_objects(map: &Map, scnr: Reader<'_>, scene: &mut Scene, materials: &mut Materials, stats: &mut Stats) -> Result<()> {
    let mut cache: HashMap<u32, Vec<u32>> = HashMap::new();
    // (kind, placements, element size, palette): scenery, machines,
    // controls, light fixtures. Each placement starts with its palette
    // index (i16), position (+0x8) and yaw, pitch, roll (+0x14). Vehicles
    // and equipment belong to the game type and are not placed.
    let lists = [
        (Kind::Scenery, 0x210, 0x48, 0x21C),
        (Kind::Machine, 0x294, 0x40, 0x2A0),
        (Kind::Control, 0x2AC, 0x40, 0x2B8),
        (Kind::Machine, 0x2C4, 0x58, 0x2D0),
    ];
    for (kind, at, size, palette_at) in lists {
        let palette: Vec<u32> = scnr.block(palette_at, 0x30)?.into_iter().map(|p| p.tag_ref(0)).collect::<Result<_>>()?;
        for (k, e) in scnr.block(at, size)?.into_iter().enumerate() {
            let Some(&object) = usize::try_from(e.i16(0)?).ok().and_then(|p| palette.get(p)) else { continue };
            let object_name = map.tag_name(object).unwrap_or("?").to_string();
            let Some(mode) = model_of(map, object) else {
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
            let transform = object_transform(e.vec3(0x8)?, e.vec3(0x14)?);
            for m in meshes {
                scene.placements.push(Placement {
                    name: format!("{} {k}", object_name.rsplit('\\').next().unwrap_or(&object_name)),
                    kind,
                    mesh: m,
                    transform,
                    mirrored: false,
                    lighting: Lighting::None,
                    bsps: 1,
                    sky: None,
                    cluster: None,
                    fade_pixels: [0, 0],
                });
            }
            *stats.objects.entry(kind).or_default() += 1;
        }
    }

    // Skies (`+0x30`, a 16-byte reference each): the sky's model at +0,
    // drawn round the middle of the level, grown like Halo 2's.
    for (si, s) in scnr.block(0x30, 0x10)?.into_iter().enumerate() {
        let Some(sky) = map.tag_data(s.tag_ref(0)?) else { continue };
        let mode = sky.tag_ref(0)?;
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
        let transform = match scene.bounds {
            Some((lo, hi)) => Mat4::from_scale_rotation_translation(
                Vec3::splat(SKY_SCALE),
                glam::Quat::IDENTITY,
                Vec3::new((lo.x + hi.x) * 0.5, (lo.y + hi.y) * 0.5, lo.z),
            ),
            None => Mat4::from_scale(Vec3::splat(SKY_SCALE)),
        };
        for m in meshes {
            // Every material of a sky model is the sky's.
            for part in &scene.meshes[m as usize].parts {
                if let Some(Some(l)) = materials.looks.get_mut(part.material as usize) {
                    l.look.sky = true;
                }
            }
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

/// Player starting locations (`+0x354`, 0x34 each: position, facing in
/// radians at +0xC) are the multiplayer spawn points; trigger volumes
/// (`+0x360`, 0x60 each) are kept for the kill-volume logic.
fn decode_spawns(scnr: Reader<'_>, scene: &mut Scene) -> Result<()> {
    for (k, e) in scnr.block(0x354, 0x34)?.into_iter().enumerate() {
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
    for e in scnr.block(0x360, 0x60)? {
        scene.trigger_volumes.push(TriggerVolume {
            name: e.str(4, 32),
            forward: e.vec3(0x30)?,
            up: e.vec3(0x3C)?,
            position: e.vec3(0x48)?,
            extents: e.vec3(0x54)?,
        });
    }
    Ok(())
}

/// The first sky's outdoor fog (`sky ` `+0x58` colour, `+0x6C` maximum
/// density, `+0x70` start and `+0x74` opaque distance).
fn sky_fog(map: &Map, scnr: Reader<'_>) -> Result<Option<Fog>> {
    let Some(s) = scnr.block(0x30, 0x10)?.into_iter().next() else { return Ok(None) };
    let Some(sky) = map.tag_data(s.tag_ref(0)?) else { return Ok(None) };
    let fog = Fog {
        colour: sky.vec3(0x58)?.to_array(),
        max_density: sky.f32(0x6C)?,
        start: sky.f32(0x70)?,
        opaque: sky.f32(0x74)?,
    };
    let ok = fog.colour.iter().all(|c| c.is_finite()) && fog.max_density > 0.0 && fog.opaque > fog.start;
    Ok(ok.then_some(fog))
}
