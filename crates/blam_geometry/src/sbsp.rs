//! Structure BSPs: the fixed world of a level. Clusters are the world
//! itself, drawn in place; instances are rocks, trees and girders placed
//! many times from one mesh. Follows `docs/format/sbsp-render-geometry.md`.

use glam::{Mat4, Vec3};

use blam_cache::{Block, CacheFile, ResourceId, Resources, TagEntry, TagRef};

use crate::lightmap::{self, Lighting};
use crate::materials::MaterialTable;
use crate::render_geometry::{self, GeometryResource};
use crate::render_model::matrix_from_rows;
use crate::scene::{Kind, Mesh, Part, Placement, Scene};
use crate::{Error, Result};

/// The structure BSPs a scenario lists, with the lightmap tag that holds each
/// one's final geometry on MCC.
#[derive(Clone, Debug)]
pub struct BspTags<'a> {
    pub index: u32,
    pub sbsp: &'a TagEntry,
    pub lightmap_bsp: Option<&'a TagEntry>,
}

/// Which BSPs the map's scenario uses.
pub fn structure_bsps(cache: &CacheFile) -> Result<Vec<BspTags<'_>>> {
    let scnr = cache.scenario().ok_or_else(|| {
        Error::Invalid(format!("no scenario tag named {}", cache.header.scenario_path))
    })?;
    let scnr_off = cache.tag_offset(scnr)?;
    let bsps = cache.reader_at(scnr_off + 0x18).block()?;
    let lightmap = cache.reader_at(scnr_off + 0x6AC).tag_ref()?;

    // The lightmap tag lists one lightmap-BSP-data tag per BSP.
    let mut lightmap_bsps: Vec<(i16, &TagEntry)> = Vec::new();
    if let Some(sldt) = lightmap.index.and_then(|i| cache.tags.get(i)) {
        let off = cache.tag_offset(sldt)?;
        let refs = cache.reader_at(off + 0x4).block()?;
        for o in refs.elements(16) {
            let r = cache.reader_at(o).tag_ref()?;
            if let Some(lbsp) = r.index.and_then(|i| cache.tags.get(i)) {
                let bsp_index = cache.reader_at(cache.tag_offset(lbsp)? + 0x2).i16()?;
                lightmap_bsps.push((bsp_index, lbsp));
            }
        }
    }

    let mut out = Vec::new();
    for (i, o) in bsps.elements(108).enumerate() {
        let r: TagRef = cache.reader_at(o).tag_ref()?;
        let Some(sbsp) = r.index.and_then(|i| cache.tags.get(i)) else {
            continue;
        };
        let lightmap_bsp = lightmap_bsps
            .iter()
            .find(|(b, _)| *b == i as i16)
            .map(|(_, t)| *t)
            .or_else(|| cache.tags.find("Lbsp", sbsp.name()));
        out.push(BspTags {
            index: i as u32,
            sbsp,
            lightmap_bsp,
        });
    }
    Ok(out)
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
    let mut r = cache.reader_at(sbsp + 0x40);
    let (xb, yb, zb) = (r.f32x2()?, r.f32x2()?, r.f32x2()?);
    let lo = Vec3::new(xb[0], yb[0], zb[0]);
    let hi = Vec3::new(xb[1], yb[1], zb[1]);
    scene.bounds = Some(match scene.bounds {
        Some((a, b)) => (a.min(lo), b.max(hi)),
        None => (lo, hi),
    });

    let clusters: Block = cache.reader_at(sbsp + 0xC4).block()?;
    let shaders: Block = cache.reader_at(sbsp + 0xD0).block()?;
    let instances: Block = cache.reader_at(sbsp + 0x1C0).block()?;
    let bounds_block: Block = cache.reader_at(sbsp + 0x300).block()?;

    // Final geometry lives with the lightmap on MCC; the BSP's own copy of
    // the render geometry struct has no resource behind it.
    let (sections_block, resource) = match bsp.lightmap_bsp {
        Some(lbsp) => {
            let o = cache.tag_offset(lbsp)?;
            (
                cache.reader_at(o + 0x134).block()?,
                ResourceId(cache.reader_at(o + 0x1AC).u32()?),
            )
        }
        None => (
            cache.reader_at(sbsp + 0x2F4).block()?,
            ResourceId(cache.reader_at(sbsp + 0x36C).u32()?),
        ),
    };

    // Materials: one per shader slot, shared across the scene.
    let slots = materials.slots(cache, scene, shaders)?;

    let sections = render_geometry::read_sections(cache, sections_block)?;
    let bounds = render_geometry::read_bounds(cache, bounds_block)?;
    let geometry = GeometryResource::load(resources, resource)?;

    // Baked lighting records (`lightmaps.md`). A broken record set costs the
    // lighting, never the geometry.
    let records = match bsp.lightmap_bsp {
        Some(lbsp) => {
            let o = cache.tag_offset(lbsp)?;
            match lightmap::read_lightmap(cache, o, bsp.index).and_then(|l| {
                let records = lightmap::Records::read(cache, o, scene.lightmaps.len() as u32)?;
                Ok((l, records))
            }) {
                Ok((l, records)) => {
                    scene.lightmaps.push(l);
                    Some(records)
                }
                Err(e) => {
                    eprintln!("bsp {} lightmap records not read: {e}", bsp.index);
                    None
                }
            }
        }
        None => None,
    };

    // One scene mesh per section that has geometry.
    let mut mesh_of_section: Vec<Option<u32>> = vec![None; sections.len()];
    let mut stats = BspStats::default();
    for (s, section) in sections.iter().enumerate() {
        let Some(decoded) = render_geometry::decode_section(section, &geometry, bounds.get(s))?
        else {
            continue;
        };
        let mut mesh = Mesh {
            name: format!("bsp{} section {s}", bsp.index),
            positions: decoded.vertices.positions.clone(),
            normals: decoded.vertices.normals.clone(),
            uvs: decoded.vertices.uvs.clone(),
            tangents: decoded.vertices.tangents.clone(),
            parts: Vec::new(),
            skin: None,
        };
        for sm in &section.submeshes {
            let indices = decoded.submesh_triangles(sm);
            if indices.is_empty() {
                continue;
            }
            let material = materials.for_slot(cache, scene, &slots, sm.shader);
            match mesh.parts.iter_mut().find(|p| p.material == material) {
                Some(p) => p.indices.extend(indices),
                None => mesh.parts.push(Part { material, indices }),
            }
        }
        if mesh.parts.is_empty() {
            continue;
        }
        mesh_of_section[s] = Some(scene.meshes.len() as u32);
        scene.meshes.push(mesh);
        stats.sections_with_geometry += 1;
    }
    stats.sections = sections.len() as u32;

    for (c, o) in clusters.elements(280).enumerate() {
        stats.clusters += 1;
        let section = cache.reader_at(o + 216).i16()?;
        let Some(mesh) = usize::try_from(section)
            .ok()
            .and_then(|s| mesh_of_section.get(s).copied().flatten())
        else {
            continue;
        };
        let lighting = match &records {
            Some(r) => r
                .cluster(
                    cache,
                    &geometry,
                    c as u32,
                    sections[section as usize].lightmap_buffer,
                    scene.meshes[mesh as usize].positions.len(),
                )
                .unwrap_or_default(),
            None => Lighting::None,
        };
        scene.placements.push(Placement {
            name: format!("bsp{} cluster {c:03}", bsp.index),
            kind: Kind::Cluster,
            mesh,
            transform: Mat4::IDENTITY,
            mirrored: false,
            lighting,
        });
        stats.cluster_placements += 1;
    }

    for (i, o) in instances.elements(120).enumerate() {
        stats.instances += 1;
        let mut r = cache.reader_at(o);
        let scale = r.f32()?;
        let rows = [r.f32x3()?, r.f32x3()?, r.f32x3()?];
        let translation = Vec3::from(r.f32x3()?);
        let section = r.at(o + 0x34).i16()?;
        let texcoord_block = r.at(o + 0x38).i16()?;
        let name = r.at(o + 0x54).string_id()?;
        let Some(mesh) = usize::try_from(section)
            .ok()
            .and_then(|s| mesh_of_section.get(s).copied().flatten())
        else {
            continue;
        };
        let (transform, mirrored) = matrix_from_rows(rows, translation.to_array(), scale);
        let lighting = match &records {
            Some(r) => r
                .instance(
                    cache,
                    &geometry,
                    i as u32,
                    texcoord_block,
                    scene.meshes[mesh as usize].positions.len(),
                )
                .unwrap_or_default(),
            None => Lighting::None,
        };
        scene.placements.push(Placement {
            name: cache
                .string(name)
                .map_or_else(|| format!("bsp{} instance {i}", bsp.index), str::to_string),
            kind: Kind::Instance,
            mesh,
            transform,
            mirrored,
            lighting,
        });
        stats.instance_placements += 1;
    }
    stats.shaders = shaders.count;
    Ok(stats)
}

/// Counts worth printing after a decode.
#[derive(Clone, Copy, Debug, Default)]
pub struct BspStats {
    pub clusters: u32,
    pub cluster_placements: u32,
    pub instances: u32,
    pub instance_placements: u32,
    pub sections: u32,
    pub sections_with_geometry: u32,
    pub shaders: u32,
}
