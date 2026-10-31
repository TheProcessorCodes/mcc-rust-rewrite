//! Reach's structure BSPs: clusters drawn in place and instances placed
//! from the lightmap BSP data's meshes, and the collision the game walks
//! on. Follows `docs/format/reach.md` 3 to 6.
//!
//! Unlike Halo 3, a Reach BSP keeps its instances' transforms in a
//! resource (`structure_bsp_cache_file_tag_resources`, sbsp `+0x520`), not
//! in the tag; the tag only has their names.

use glam::{Mat4, Vec3};

use blam_cache::{Block, CacheAddress, CacheFile, ResourceId, Resources, TagEntry};
use blam_geometry::collision::{self, CollisionStats};
use blam_geometry::materials::MaterialTable;
use blam_geometry::render_model::matrix_from_rows;
use blam_geometry::scene::{BspInfo, ClusterInfo, Kind, Mesh, Part, Placement, Scene};
use blam_geometry::{CollisionMaterial, CollisionSource, CollisionTriangle, Error, Lighting, Lightmap, Result};

use crate::geometry::{self, GeometryResource};
use crate::layout::{lbsp, sbsp as off, scnr};

/// A structure BSP the scenario lists, with its lightmap BSP data.
#[derive(Clone, Debug)]
pub struct BspTags<'a> {
    pub index: u32,
    pub sbsp: &'a TagEntry,
    pub lightmap_bsp: Option<&'a TagEntry>,
}

/// The scenario's structure BSPs. The lightmap tag's references are in BSP
/// order (entry `n` lights BSP `n`; empty BSPs have an empty entry).
pub fn structure_bsps(cache: &CacheFile) -> Result<Vec<BspTags<'_>>> {
    let s = scenario_offset(cache)?;
    let bsps = cache.reader_at(s + scnr::STRUCTURE_BSPS).block()?;
    let lightmap = cache.reader_at(s + scnr::LIGHTMAP).tag_ref()?;
    let mut lightmap_bsps: Vec<Option<&TagEntry>> = Vec::new();
    if let Some(sldt) = lightmap.index.and_then(|i| cache.tags.get(i)) {
        let refs = cache.reader_at(cache.tag_offset(sldt)? + 4).block()?;
        for o in refs.elements(0x20) {
            let r = cache.reader_at(o).tag_ref()?;
            lightmap_bsps.push(r.index.and_then(|i| cache.tags.get(i)));
        }
    }
    let mut out = Vec::new();
    for (i, o) in bsps.elements(scnr::STRUCTURE_BSP_SIZE).enumerate() {
        let Some(sbsp) = cache.reader_at(o).tag_ref()?.index.and_then(|i| cache.tags.get(i)) else {
            continue;
        };
        out.push(BspTags {
            index: i as u32,
            sbsp,
            lightmap_bsp: lightmap_bsps.get(i).copied().flatten(),
        });
    }
    Ok(out)
}

pub(crate) fn scenario_offset(cache: &CacheFile) -> Result<usize> {
    let scnr = cache
        .scenario()
        .ok_or_else(|| Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path)))?;
    Ok(cache.tag_offset(scnr)?)
}

/// Counts worth printing after a decode.
#[derive(Clone, Copy, Debug, Default)]
pub struct BspStats {
    pub meshes: u32,
    pub meshes_with_geometry: u32,
    pub clusters: u32,
    pub cluster_placements: u32,
    pub instances: u32,
    pub instance_placements: u32,
    pub shaders: u32,
    pub lightmapped: u32,
}

/// One instance as the resource stores it (0x9C bytes).
#[derive(Clone, Copy, Debug)]
pub struct Instance {
    pub transform: Mat4,
    pub mirrored: bool,
    /// Index into the collision resource's instanced geometry definitions.
    pub definition: i16,
    pub flags: u16,
    pub mesh: i16,
    pub compression: i16,
}

impl Instance {
    pub const RENDER_ONLY: u16 = 1 << 1;
}

/// The fixed-up definition and data of a resource, and where its root is.
struct Loaded {
    def: Vec<u8>,
    data: Vec<u8>,
    root: usize,
}

fn load_resource(resources: &Resources<'_>, id: ResourceId, what: &str, with_data: bool) -> Result<Loaded> {
    if id.is_null() {
        return Err(Error::Invalid(format!("no {what} resource")));
    }
    let entry = resources.entry(id)?;
    if entry.is_empty() || entry.salt != (id.0 >> 16) as u16 {
        return Err(Error::Invalid(format!("{what} resource {:#010x} is stale or wiped", id.0)));
    }
    if entry.definition_address.kind() != CacheAddress::DEFINITION {
        return Err(Error::Invalid(format!("{what} resource root outside its definition")));
    }
    let def = resources.fixed_definition(&entry)?;
    let data = match entry.segment {
        Some(_) if with_data => resources.data(&entry)?.bytes().to_vec(),
        _ => Vec::new(),
    };
    Ok(Loaded {
        root: entry.definition_address.offset(),
        def,
        data,
    })
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

/// A block inside a definition: (count, offset in the definition).
fn def_block(l: &Loaded, at: usize) -> Result<(usize, usize)> {
    let (Some(count), Some(address)) = (u32_at(&l.def, at), u32_at(&l.def, at + 4)) else {
        return Err(Error::Invalid(format!("definition block at {at:#x} runs past the end")));
    };
    if count == 0 {
        return Ok((0, 0));
    }
    let a = CacheAddress(address);
    if a.kind() != CacheAddress::DEFINITION {
        return Err(Error::Unsupported(format!("definition block in address space {}", a.kind())));
    }
    Ok((count as usize, a.offset()))
}

/// The instances of a BSP, from its cache-file resource (root `+0x48`).
pub fn instances(cache: &CacheFile, resources: &Resources<'_>, sbsp: &TagEntry) -> Result<Vec<Instance>> {
    let base = cache.tag_offset(sbsp)?;
    let id = ResourceId(cache.reader_at(base + off::CACHE_FILE_RESOURCE).u32()?);
    let l = load_resource(resources, id, "instance", false)?;
    let (count, at) = def_block(&l, l.root + 0x48)?;
    if at + count * 0x9C > l.def.len() {
        return Err(Error::Invalid(format!("{count} instances run past the definition")));
    }
    let f = |o: usize| f32::from_bits(u32_at(&l.def, o).unwrap_or(0));
    let i16_at = |o: usize| i16::from_le_bytes([l.def[o], l.def[o + 1]]);
    Ok((0..count)
        .map(|i| {
            let o = at + i * 0x9C;
            let scale = f(o);
            let row = |k: usize| [f(o + 4 + 12 * k), f(o + 8 + 12 * k), f(o + 12 + 12 * k)];
            let (transform, mirrored) = matrix_from_rows([row(0), row(1), row(2)], row(3), scale);
            Instance {
                transform,
                mirrored,
                definition: i16_at(o + 0x34),
                flags: i16_at(o + 0x36) as u16,
                mesh: i16_at(o + 0x38),
                compression: i16_at(o + 0x3A),
            }
        })
        .collect())
}

/// Decodes one BSP's clusters and instances into `scene`.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    materials: &mut MaterialTable,
    bsp: &BspTags<'_>,
    scene: &mut Scene,
) -> Result<BspStats> {
    let sbsp = cache.tag_offset(bsp.sbsp)?;
    let mut r = cache.reader_at(sbsp + off::WORLD_BOUNDS);
    let (xb, yb, zb) = (r.f32x2()?, r.f32x2()?, r.f32x2()?);
    let lo = Vec3::new(xb[0], yb[0], zb[0]);
    let hi = Vec3::new(xb[1], yb[1], zb[1]);

    let clusters: Block = cache.reader_at(sbsp + off::CLUSTERS).block()?;
    let shaders: Block = cache.reader_at(sbsp + off::MATERIALS).block()?;
    let names: Block = cache.reader_at(sbsp + off::INSTANCE_NAMES).block()?;

    // The final geometry is the lightmap BSP data's; a BSP without one
    // keeps its own copy in its third render geometry.
    let lbsp_off = bsp.lightmap_bsp.map(|t| cache.tag_offset(t)).transpose()?;
    let from_lightmap = match lbsp_off {
        Some(o) => cache.reader_at(o + lbsp::MESHES).block()?.count > 0,
        None => false,
    };
    let (meshes_block, bounds_block, resource) = match (from_lightmap, lbsp_off) {
        (true, Some(o)) => (
            cache.reader_at(o + lbsp::MESHES).block()?,
            cache.reader_at(o + lbsp::COMPRESSION).block()?,
            ResourceId(cache.reader_at(o + lbsp::RESOURCE).u32()?),
        ),
        _ => (
            cache.reader_at(sbsp + off::OWN_MESHES).block()?,
            cache.reader_at(sbsp + off::OWN_COMPRESSION).block()?,
            ResourceId(cache.reader_at(sbsp + off::OWN_RESOURCE).u32()?),
        ),
    };
    if meshes_block.count == 0 {
        return Err(Error::Invalid(format!("{} has no render geometry", bsp.sbsp.name())));
    }
    scene.bounds = Some(match scene.bounds {
        Some((a, b)) => (a.min(lo), b.max(hi)),
        None => (lo, hi),
    });

    // Materials: one per shader slot (0x2C each, the reference first).
    let slots: Vec<u32> = shaders
        .elements(0x2C)
        .map(|o| Ok(materials.index(cache, scene, cache.reader_at(o).tag_ref()?)))
        .collect::<Result<_>>()?;
    let meshes = geometry::read_meshes(cache, meshes_block)?;
    let bounds = geometry::read_bounds(cache, bounds_block)?;
    let resource = GeometryResource::load(resources, resource)?;

    // The lightmap: its bitmaps (the viewer decodes them, `halo3_textures`
    // `reach.rs`) and, per instance, whether it is lit per pixel and the
    // vertex buffer of its texcoords.
    let mut lightmap: Option<(u32, Block, Block)> = None;
    if let (true, Some(o)) = (from_lightmap, lbsp_off) {
        let primary = cache.reader_at(o + lbsp::PRIMARY_MAP).tag_ref()?;
        let intensity = cache.reader_at(o + lbsp::INTENSITY_MAP).tag_ref()?;
        if intensity.index.is_some() {
            lightmap = Some((
                scene.lightmaps.len() as u32,
                cache.reader_at(o + lbsp::INSTANCE_LIGHTING).block()?,
                cache.reader_at(o + lbsp::INSTANCE_TEXCOORDS).block()?,
            ));
            scene.lightmaps.push(Lightmap {
                bsp: bsp.index,
                coefficients_bitmap: primary.index,
                intensity_bitmap: intensity.index,
                scales: [0.0; 5],
            });
        }
    }
    let per_pixel = |buffer: Option<u16>, count: usize| -> Lighting {
        let (Some((l, _, _)), Some(b)) = (&lightmap, buffer) else {
            return Lighting::None;
        };
        match resource.texcoords(b, count) {
            Ok(Some(uvs)) => Lighting::PerPixel { lightmap: *l, uvs },
            _ => Lighting::None,
        }
    };

    let mut stats = BspStats {
        meshes: meshes.len() as u32,
        shaders: shaders.count,
        ..Default::default()
    };
    // One scene mesh per (mesh, compression box) pair used.
    let mut decoded: std::collections::HashMap<(usize, usize), Option<u32>> = std::collections::HashMap::new();
    let mut mesh_for = |m: usize, b: usize, scene: &mut Scene, materials: &mut MaterialTable| -> Result<Option<u32>> {
        if let Some(&d) = decoded.get(&(m, b)) {
            return Ok(d);
        }
        let Some(mesh) = meshes.get(m) else {
            return Ok(None);
        };
        let result = match geometry::decode_mesh(mesh, &resource, bounds.get(b))? {
            None => None,
            Some(d) => {
                let mut out = Mesh {
                    name: format!("bsp{} mesh {m}", bsp.index),
                    positions: d.vertices.positions.clone(),
                    normals: d.vertices.normals.clone(),
                    uvs: d.vertices.uvs.clone(),
                    tangents: d.vertices.tangents.clone(),
                    parts: Vec::new(),
                    skin: None,
                };
                for p in &mesh.parts {
                    let indices = geometry::part_triangles(&d, p.index_start, p.index_count);
                    if indices.is_empty() {
                        continue;
                    }
                    let material = materials.for_slot(cache, scene, &slots, p.shader);
                    match out.parts.iter_mut().find(|q| q.material == material) {
                        Some(q) => q.indices.extend(indices),
                        None => out.parts.push(Part { material, indices }),
                    }
                }
                (!out.parts.is_empty()).then(|| {
                    scene.meshes.push(out);
                    (scene.meshes.len() - 1) as u32
                })
            }
        };
        decoded.insert((m, b), result);
        Ok(result)
    };

    // Clusters: the mesh at +0x40, its compression box the same index.
    let mut cluster_info = Vec::new();
    for (c, o) in clusters.elements(off::CLUSTER_SIZE).enumerate() {
        stats.clusters += 1;
        let mut r = cache.reader_at(o);
        let (x, y, z) = (r.f32x2()?, r.f32x2()?, r.f32x2()?);
        let sky = r.at(o + 0x18).i8()?;
        cluster_info.push(ClusterInfo {
            bounds: (x[0] <= x[1] && x[1] < 1e30).then(|| (Vec3::new(x[0], y[0], z[0]), Vec3::new(x[1], y[1], z[1]))),
            sky: u32::try_from(sky).ok(),
            atmosphere: None,
            acoustics: None,
        });
        let m = cache.reader_at(o + off::CLUSTER_MESH).i16()?;
        let Ok(m) = usize::try_from(m) else { continue };
        let Some(mesh) = mesh_for(m, m, scene, materials)? else {
            continue;
        };
        // A cluster's lightmap texcoords are its mesh's own second buffer.
        let count = scene.meshes[mesh as usize].positions.len();
        let lighting = per_pixel(meshes[m].buffers[1], count);
        if !matches!(lighting, Lighting::None) {
            stats.lightmapped += 1;
        }
        scene.placements.push(Placement {
            name: format!("bsp{} cluster {c:03}", bsp.index),
            kind: Kind::Cluster,
            mesh,
            transform: Mat4::IDENTITY,
            mirrored: false,
            lighting,
            bsps: 1 << bsp.index,
            sky: None,
            cluster: Some(c as u32),
            fade_pixels: [0; 2],
        });
        stats.cluster_placements += 1;
    }

    // Instances, from the resource; their names from the tag.
    let list = match instances(cache, resources, bsp.sbsp) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("bsp {} instances not read: {e}", bsp.index);
            Vec::new()
        }
    };
    for (i, inst) in list.iter().enumerate() {
        stats.instances += 1;
        let (Ok(m), Ok(b)) = (usize::try_from(inst.mesh), usize::try_from(inst.compression)) else {
            continue;
        };
        let Some(mesh) = mesh_for(m, b, scene, materials)? else {
            continue;
        };
        let name = names
            .element(i as u32, 4)
            .and_then(|o| cache.reader_at(o).string_id().ok())
            .and_then(|id| cache.string(id))
            .map_or_else(|| format!("bsp{} instance {i}", bsp.index), str::to_string);
        let mut lighting = Lighting::None;
        if let Some((_, lit, texcoords)) = &lightmap
            && let (Some(a), Some(b)) = (lit.element(i as u32, 0xC), texcoords.element(i as u32, 2))
            && cache.reader_at(a).i16()? >= 0
        {
            let vb = cache.reader_at(b).i16()?;
            lighting = per_pixel(u16::try_from(vb).ok(), scene.meshes[mesh as usize].positions.len());
        }
        if !matches!(lighting, Lighting::None) {
            stats.lightmapped += 1;
        }
        scene.placements.push(Placement {
            name,
            kind: Kind::Instance,
            mesh,
            transform: inst.transform,
            mirrored: inst.mirrored,
            lighting,
            bsps: 1 << bsp.index,
            sky: None,
            cluster: None,
            fade_pixels: [0; 2],
        });
        stats.instance_placements += 1;
    }
    stats.meshes_with_geometry = decoded.values().filter(|d| d.is_some()).count() as u32;
    scene.bsps.push(BspInfo {
        index: bsp.index,
        name: bsp.sbsp.name().to_string(),
        bounds: (lo, hi),
        clusters: cluster_info,
        portals: Vec::new(),
        locator: None,
    });
    Ok(stats)
}

/// Decodes a BSP's collision: its own collision BSP from the
/// `structure_bsp_tag_resources` resource (sbsp `+0x518`), and every
/// collidable instance's definition placed by the instance.
pub fn decode_collision(
    cache: &CacheFile,
    resources: &Resources<'_>,
    sbsp: &TagEntry,
    bsp_index: u32,
    scene: &mut Scene,
) -> Result<CollisionStats> {
    let base = cache.tag_offset(sbsp)?;
    let mut stats = CollisionStats::default();
    let material_base = scene.collision.materials.len() as i16;
    for o in cache.reader_at(base + off::COLLISION_MATERIALS).block()?.elements(0x18) {
        let shader = cache.reader_at(o).tag_ref()?;
        let tag = shader.index.and_then(|i| cache.tags.get(i));
        scene.collision.materials.push(CollisionMaterial {
            shader: tag.map_or_else(|| "(none)".to_string(), |t| t.name().to_string()),
            shader_class: tag.map(|t| cache.tags.class_of(t).code.as_str().to_string()).unwrap_or_default(),
        });
    }
    let id = ResourceId(cache.reader_at(base + off::COLLISION_RESOURCE).u32()?);
    let l = load_resource(resources, id, "collision", true)?;
    let fix = |t: &mut CollisionTriangle| {
        if t.material >= 0 {
            t.material += material_base;
        }
    };
    let mut triangles = Vec::new();
    // The BSP's own collision geometries (0x6C each).
    let (count, at) = def_block(&l, l.root)?;
    for k in 0..count {
        let start = triangles.len();
        let n = collision::triangulate_geometry(
            &l.def,
            &l.data,
            at + k * GEOMETRY_SIZE + SURFACES_SHIFT,
            CollisionSource::Bsp(bsp_index),
            &mut triangles,
            &mut stats,
        )?;
        triangles[start..].iter_mut().for_each(fix);
        stats.bsp_triangles += n;
    }
    // Instanced geometry definitions (0x144 each, collision at +8).
    let (defs, defs_at) = def_block(&l, l.root + 0x18)?;
    let mut def_triangles: Vec<Option<Vec<CollisionTriangle>>> = vec![None; defs];
    let list = instances(cache, resources, sbsp)?;
    for inst in &list {
        let Ok(d) = usize::try_from(inst.definition) else {
            stats.instances_skipped += 1;
            continue;
        };
        if d >= defs || inst.flags & Instance::RENDER_ONLY != 0 {
            stats.instances_skipped += 1;
            continue;
        }
        if def_triangles[d].is_none() {
            let mut local = Vec::new();
            collision::triangulate_geometry(
                &l.def,
                &l.data,
                defs_at + d * DEFINITION_SIZE + 8 + SURFACES_SHIFT,
                CollisionSource::Instance(bsp_index),
                &mut local,
                &mut stats,
            )?;
            local.iter_mut().for_each(fix);
            def_triangles[d] = Some(local);
        }
        let local = def_triangles[d].as_ref().unwrap();
        for t in local {
            let p = t.points.map(|v| inst.transform.transform_point3(v));
            triangles.push(CollisionTriangle {
                points: if inst.mirrored { [p[0], p[2], p[1]] } else { p },
                ..*t
            });
        }
        stats.instance_triangles += local.len();
        stats.instances_placed += 1;
    }
    scene.collision.triangles.append(&mut triangles);
    Ok(stats)
}

/// Reach's collision geometry struct, and how far its surfaces block is
/// past Halo 3's (the supernodes block is new).
const GEOMETRY_SIZE: usize = 0x6C;
const SURFACES_SHIFT: usize = 0xC;
/// An instanced geometry definition of the collision resource.
const DEFINITION_SIZE: usize = 0x144;
