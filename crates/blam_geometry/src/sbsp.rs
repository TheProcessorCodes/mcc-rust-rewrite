//! Structure BSPs: the fixed world of a level. Clusters are the world
//! itself, drawn in place; instances are rocks, trees and girders placed
//! many times from one mesh. Follows `docs/format/sbsp-render-geometry.md`.

use glam::{Mat4, Vec3};

use blam_cache::{Block, CacheFile, ResourceId, Resources, TagEntry, TagRef};

use crate::lightmap::{self, Lighting};
use crate::materials::MaterialTable;
use crate::render_geometry::{self, GeometryResource};
use crate::render_model::matrix_from_rows;
use crate::scene::{BspInfo, ClusterInfo, CubemapPoint, Kind, Mesh, Part, Placement, Portal, Scene};
use crate::{Error, Result};

/// The structure BSPs a scenario lists, with the lightmap tag that holds each
/// one's final geometry on MCC.
#[derive(Clone, Debug)]
pub struct BspTags<'a> {
    pub index: u32,
    pub sbsp: &'a TagEntry,
    pub lightmap_bsp: Option<&'a TagEntry>,
    /// The BSP's reflection cube maps (`bitm`, scenario structure BSP
    /// `+0x48`; `docs/format/cubemaps.md`).
    pub cubemaps: Option<u32>,
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
        let cubemaps = cache.reader_at(o + 0x48).tag_ref()?;
        out.push(BspTags {
            index: i as u32,
            sbsp,
            lightmap_bsp,
            cubemaps: cubemaps.index.filter(|_| cubemaps.class.as_str() == "bitm"),
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

    // The atmosphere palette (sbsp + 0x94, 8 bytes: string id name, i16
    // index into the skya settings): what a cluster's atmosphere byte
    // indexes (`sbsp-portals.md` 5).
    let palette: Block = cache.reader_at(sbsp + 0x94).block()?;
    let atmospheres: Vec<Option<u32>> = palette
        .elements(8)
        .map(|o| cache.reader_at(o + 4).i16().map(|i| u32::try_from(i).ok()))
        .collect::<std::result::Result<_, _>>()?;
    let mut cluster_info = Vec::new();
    let mut cubemaps = Vec::new();
    for (c, o) in clusters.elements(280).enumerate() {
        stats.clusters += 1;
        // Bounds (x, y, z ranges; FLT_MAX when unset), the sky index at
        // +0x18 and the atmosphere palette index at +0x19 (`zone-sets.md`
        // 3).
        let mut r = cache.reader_at(o);
        let (x, y, z) = (r.f32x2()?, r.f32x2()?, r.f32x2()?);
        let mut r = r.at(o + 0x18);
        let (sky, atmosphere) = (r.i8()?, r.i8()?);
        // +0x1C: i16 index into the BSP's acoustics palette (sbsp + 0x118,
        // `docs/format/sound.md` 5), -1 none.
        let acoustics = r.at(o + 0x1C).i16()?;
        cluster_info.push(ClusterInfo {
            bounds: (x[0] <= x[1] && x[1] < 1e30)
                .then(|| (Vec3::new(x[0], y[0], z[0]), Vec3::new(x[1], y[1], z[1]))),
            sky: u32::try_from(sky).ok(),
            atmosphere: usize::try_from(atmosphere)
                .ok()
                .and_then(|a| atmospheres.get(a).copied().flatten()),
            acoustics: u32::try_from(acoustics).ok(),
        });
        // +0x10C: the cluster's reflection cube maps, 16 bytes each:
        // capture point, the cube's scenario-wide number, its bitmap in
        // this BSP's cube map tag (`docs/format/cubemaps.md` 2).
        if bsp.cubemaps.is_some() {
            let points: Block = cache.reader_at(o + 0x10C).block()?;
            for p in points.elements(16).take(256) {
                let mut r = cache.reader_at(p);
                let position = Vec3::from(r.f32x3()?);
                let (id, cube) = (r.i16()?, r.i16()?);
                let (Ok(id), Ok(cube)) = (u32::try_from(id), u32::try_from(cube)) else { continue };
                if position.is_finite() {
                    cubemaps.push(CubemapPoint { position, cluster: c as u32, id, cube });
                }
            }
        }
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
            bsps: 1 << bsp.index,
            sky: None,
            cluster: Some(c as u32),
            fade_pixels: [0; 2],
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
        let flags = r.at(o + 0x36).u16()?;
        let texcoord_block = r.at(o + 0x38).i16()?;
        let name = r.at(o + 0x54).string_id()?;
        let pathfinding = r.at(o + 0x58).u16()?;
        let fade_pixels = [r.at(o + 0x6C).u16()?, r.at(o + 0x6E).u16()?];
        let Some(mesh) = usize::try_from(section)
            .ok()
            .and_then(|s| mesh_of_section.get(s).copied().flatten())
        else {
            continue;
        };
        if flags & INSTANCE_COLLIDABLE == 0
            && pathfinding == PATHFINDING_STATIC
            && only_pit_fire(scene, mesh)
        {
            stats.dropped_instances.push(i as u32);
            continue;
        }
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
            bsps: 1 << bsp.index,
            sky: None,
            cluster: None,
            fade_pixels,
        });
        stats.instance_placements += 1;
    }
    stats.shaders = shaders.count;
    let portals = read_portals(cache, sbsp, cluster_info.len())?;
    scene.bsps.push(BspInfo {
        index: bsp.index,
        name: bsp.sbsp.name().to_string(),
        bounds: (lo, hi),
        clusters: cluster_info,
        portals,
        locator: None,
        cubemap_bitmap: bsp.cubemaps,
        cubemaps,
    });
    Ok(stats)
}

/// Instance flag (+0x36) bit 3: the engine collides with this instance
/// (`collision-bsp-research.md` 10).
const INSTANCE_COLLIDABLE: u16 = 0x8;
/// Instance pathfinding policy (+0x58) "static": AI walk round it as a solid.
const PATHFINDING_STATIC: u16 = 1;

/// Whether every part of a mesh is drawn with 120_halo's `firey_pit`, the
/// glow of the pit floor far below the Warthog run. In `120_bsp_110` the
/// trench's 68 boulders (`rock_small` ... `rock_huge_capped`) carry that
/// shader and no collision, though their pathfinding says solid, while the
/// same rocks in 100 and 120 are `rock_basic_trench` and collidable; Halo 3
/// shows no glowing boulders there, so these instances are not placed
/// (issue #5). The pit floor's own quads (`_firey_pit_01` ... `_04`,
/// pathfinding "none") and the cluster geometry with the shader stay.
fn only_pit_fire(scene: &Scene, mesh: u32) -> bool {
    let parts = &scene.meshes[mesh as usize].parts;
    !parts.is_empty()
        && parts.iter().all(|p| {
            scene.materials[p.material as usize]
                .tag_path
                .eq_ignore_ascii_case(r"levels\solo\120_halo\shaders\firey_pit")
        })
}

/// The cluster portals at sbsp + 0x7C, 40 bytes each
/// (`docs/format/sbsp-portals.md` 1). Portals naming a cluster the BSP does
/// not have are dropped.
fn read_portals(cache: &CacheFile, sbsp: usize, clusters: usize) -> Result<Vec<Portal>> {
    let block: Block = cache.reader_at(sbsp + 0x7C).block()?;
    let mut out = Vec::with_capacity(block.count as usize);
    for o in block.elements(40) {
        let mut r = cache.reader_at(o);
        let (back, front) = (r.i16()?, r.i16()?);
        let centroid = Vec3::from(r.at(o + 0x08).f32x3()?);
        let radius = r.at(o + 0x14).f32()?;
        let vertices: Block = r.at(o + 0x1C).block()?;
        let vertices = vertices
            .elements(12)
            .map(|v| cache.reader_at(v).f32x3().map(Vec3::from))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let (Ok(back), Ok(front)) = (u32::try_from(back), u32::try_from(front)) else {
            continue;
        };
        if back as usize >= clusters || front as usize >= clusters || vertices.len() < 3 {
            continue;
        }
        out.push(Portal {
            back,
            front,
            vertices,
            centroid,
            radius,
        });
    }
    Ok(out)
}

/// The cluster of each collision BSP leaf: one signed byte per leaf at
/// sbsp + 0x34 (`docs/format/sbsp-portals.md` 3).
pub fn leaf_clusters(cache: &CacheFile, sbsp: &TagEntry) -> Result<Vec<i8>> {
    let base = cache.tag_offset(sbsp)?;
    let block: Block = cache.reader_at(base + 0x34).block()?;
    block.elements(1).map(|o| Ok(cache.reader_at(o).i8()?)).collect()
}

/// Counts worth printing after a decode.
#[derive(Clone, Debug, Default)]
pub struct BspStats {
    pub clusters: u32,
    pub cluster_placements: u32,
    pub instances: u32,
    pub instance_placements: u32,
    pub sections: u32,
    pub sections_with_geometry: u32,
    pub shaders: u32,
    /// Instances left out on purpose ([`only_pit_fire`]); collision skips
    /// them too.
    pub dropped_instances: Vec<u32>,
}
