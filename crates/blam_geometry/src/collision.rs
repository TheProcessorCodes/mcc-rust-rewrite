//! The real collision BSP of a structure BSP: what the game itself walks
//! into, including invisible walls. It lives in the `structure_bsp_tag_resources`
//! resource the BSP tag names at `0x38C`. Follows
//! `docs/format/collision-bsp-research.md`.
//!
//! Every collision surface is a convex planar polygon stored as a loop of
//! winged edges; the loop is wound counter-clockwise seen from the side you
//! stand on, so a fan from its first vertex gives triangles whose normals
//! face out of the solid.

use glam::{Mat4, Vec3};

use blam_cache::{CacheAddress, CacheFile, ResourceId, Resources, TagEntry};

use crate::render_model::matrix_from_rows;
use crate::scene::{Collision, CollisionMaterial, CollisionSource, CollisionTriangle};
use crate::{Error, Result};

/// Surface flags, as the game stores them.
pub mod surface_flags {
    pub const TWO_SIDED: u8 = 1 << 0;
    pub const INVISIBLE: u8 = 1 << 1;
    pub const CLIMBABLE: u8 = 1 << 2;
    pub const BREAKABLE: u8 = 1 << 3;
    pub const INVALID: u8 = 1 << 4;
    pub const CONVEYOR: u8 = 1 << 5;
    pub const SLIP: u8 = 1 << 6;
    pub const PLANE_NEGATED: u8 = 1 << 7;
}

/// Instance flags from the BSP tag's instance list.
mod instance_flags {
    pub const RENDER_ONLY: u16 = 1 << 1;
}

/// The two buffers a resource definition's addresses point into.
struct Resolver<'a> {
    def: &'a [u8],
    data: &'a [u8],
}

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}

/// A tag block inside a definition: count, then a cache address.
#[derive(Clone, Copy, Debug)]
struct DefBlock {
    count: usize,
    address: CacheAddress,
}

impl<'a> Resolver<'a> {
    fn block(&self, at: usize) -> Result<DefBlock> {
        if at + 12 > self.def.len() {
            return Err(Error::Invalid(format!(
                "collision definition block at {at:#x} runs past {} bytes",
                self.def.len()
            )));
        }
        Ok(DefBlock {
            count: u32_at(self.def, at).min(i32::MAX as u32) as usize,
            address: CacheAddress(u32_at(self.def, at + 4)),
        })
    }

    /// The bytes of a block's elements.
    fn elements(&self, block: DefBlock, stride: usize, what: &str) -> Result<&'a [u8]> {
        if block.count == 0 {
            return Ok(&[]);
        }
        let buffer = match block.address.kind() {
            CacheAddress::DEFINITION => self.def,
            CacheAddress::DATA => self.data,
            k => {
                return Err(Error::Unsupported(format!(
                    "{what}: collision data in address space {k}"
                )));
            }
        };
        let start = block.address.offset();
        let len = block
            .count
            .checked_mul(stride)
            .ok_or_else(|| Error::Invalid(format!("{what}: {} elements overflow", block.count)))?;
        let end = start
            .checked_add(len)
            .filter(|&e| e <= buffer.len())
            .ok_or_else(|| {
                Error::Invalid(format!(
                    "{what}: {} elements at {start:#x} run past {} bytes",
                    block.count,
                    buffer.len()
                ))
            })?;
        Ok(&buffer[start..end])
    }
}

/// Element sizes of a normal (16-bit) and a large (32-bit) collision BSP.
#[derive(Clone, Copy)]
pub(crate) struct Layout {
    surface: usize,
    edge: usize,
    vertex: usize,
    wide: bool,
}

pub(crate) const NORMAL: Layout = Layout {
    surface: 12,
    edge: 12,
    vertex: 16,
    wide: false,
};
const LARGE: Layout = Layout {
    surface: 16,
    edge: 24,
    vertex: 20,
    wide: true,
};

#[derive(Clone, Copy, Debug, Default)]
pub struct CollisionStats {
    pub bsp_triangles: usize,
    pub instance_triangles: usize,
    pub instances_placed: usize,
    pub instances_skipped: usize,
    pub corrupt_loops: usize,
}

/// Where a collision geometry's blocks live: a resource definition (the
/// structure BSP's collision) or ordinary tag data (an object's `coll` tag).
pub(crate) trait GeometrySource<'a> {
    /// The elements of the 12-byte block header at `at`.
    fn block_elements(&self, at: usize, stride: usize, what: &str) -> Result<&'a [u8]>;
}

impl<'a> GeometrySource<'a> for Resolver<'a> {
    fn block_elements(&self, at: usize, stride: usize, what: &str) -> Result<&'a [u8]> {
        self.elements(self.block(at)?, stride, what)
    }
}

/// Tag blocks in the map file (`at` is a file offset).
pub(crate) struct TagBlocks<'a>(pub &'a CacheFile);

impl<'a> GeometrySource<'a> for TagBlocks<'a> {
    fn block_elements(&self, at: usize, stride: usize, what: &str) -> Result<&'a [u8]> {
        let block = self.0.reader_at(at).block()?;
        let Some(start) = block.offset else {
            return Ok(&[]);
        };
        let len = (block.count as usize)
            .checked_mul(stride)
            .ok_or_else(|| Error::Invalid(format!("{what}: {} elements overflow", block.count)))?;
        self.0
            .bytes()
            .get(start..start + len)
            .ok_or_else(|| Error::Invalid(format!("{what}: {} elements at {start:#x} run past the file", block.count)))
    }
}

/// Triangulates one `CollisionGeometry` (96 bytes at `at` in the
/// definition), transforming into world space when a transform is given.
pub(crate) fn triangulate<'a>(
    res: &impl GeometrySource<'a>,
    at: usize,
    layout: Layout,
    transform: Option<(Mat4, bool)>,
    source: CollisionSource,
    out: &mut Vec<CollisionTriangle>,
    stats: &mut CollisionStats,
) -> Result<usize> {
    let surfaces = res.block_elements(at + 0x3C, layout.surface, "surfaces")?;
    let edges = res.block_elements(at + 0x48, layout.edge, "edges")?;
    let vertices = res.block_elements(at + 0x54, layout.vertex, "vertices")?;
    let surface_count = surfaces.len() / layout.surface;
    let edge_count = edges.len() / layout.edge;
    let vertex_count = vertices.len() / layout.vertex;

    let edge_field = |e: usize, field: usize| -> usize {
        let o = e * layout.edge;
        if layout.wide {
            u32_at(edges, o + field * 4) as usize
        } else {
            u16_at(edges, o + field * 2) as usize
        }
    };
    let vertex = |v: usize| -> Vec3 {
        let o = v * layout.vertex;
        Vec3::new(f32_at(vertices, o), f32_at(vertices, o + 4), f32_at(vertices, o + 8))
    };

    let before = out.len();
    let mut loop_points: Vec<Vec3> = Vec::with_capacity(8);
    for s in 0..surface_count {
        let o = s * layout.surface;
        let (first_edge, material, flags) = if layout.wide {
            (
                u32_at(surfaces, o + 4) as usize,
                u16_at(surfaces, o + 8) as i16,
                surfaces[o + 0xE],
            )
        } else {
            (
                u16_at(surfaces, o + 2) as usize,
                u16_at(surfaces, o + 4) as i16,
                surfaces[o + 0xA],
            )
        };
        if flags & surface_flags::INVALID != 0 {
            continue;
        }
        // Walk the winged-edge loop around surface `s`.
        loop_points.clear();
        let mut e = first_edge;
        let mut corrupt = false;
        for _ in 0..edge_count.max(1) {
            if e >= edge_count {
                corrupt = true;
                break;
            }
            let (start, end, forward, reverse, left, right) = (
                edge_field(e, 0),
                edge_field(e, 1),
                edge_field(e, 2),
                edge_field(e, 3),
                edge_field(e, 4),
                edge_field(e, 5),
            );
            let (v, next) = if left == s {
                (start, forward)
            } else if right == s {
                (end, reverse)
            } else {
                corrupt = true;
                break;
            };
            if v >= vertex_count {
                corrupt = true;
                break;
            }
            loop_points.push(vertex(v));
            if next == first_edge {
                break;
            }
            e = next;
        }
        if corrupt || loop_points.len() < 3 {
            stats.corrupt_loops += 1;
            continue;
        }
        let (m, mirrored) = transform.unwrap_or((Mat4::IDENTITY, false));
        let world: Vec<Vec3> = if transform.is_some() {
            loop_points.iter().map(|p| m.transform_point3(*p)).collect()
        } else {
            loop_points.clone()
        };
        for k in 1..world.len() - 1 {
            let (b, c) = if mirrored { (k + 1, k) } else { (k, k + 1) };
            out.push(CollisionTriangle {
                points: [world[0], world[b], world[c]],
                material,
                flags,
                source,
            });
        }
    }
    Ok(out.len() - before)
}

/// Triangulates one collision geometry struct of a fixed-up resource
/// definition (`def`, with `data` its primary bytes) in its own frame,
/// for readers of other games' resources. `at` is where the struct's
/// surfaces, edges and vertices blocks sit less 0x3C / 0x48 / 0x54: the
/// struct itself for Halo 3, the struct + 0xC for Reach, whose collision
/// geometry has a supernodes block after the 3D nodes
/// (`docs/format/reach.md` 6). Element layouts are Halo 3's 16-bit ones.
pub fn triangulate_geometry(
    def: &[u8],
    data: &[u8],
    at: usize,
    source: CollisionSource,
    out: &mut Vec<CollisionTriangle>,
    stats: &mut CollisionStats,
) -> Result<usize> {
    triangulate(&Resolver { def, data }, at, NORMAL, None, source, out, stats)
}

/// [`triangulate_geometry`] for a large collision geometry: the same
/// blocks with 32-bit indices (surfaces 16, edges 24, vertices 20 bytes).
pub fn triangulate_large_geometry(
    def: &[u8],
    data: &[u8],
    at: usize,
    source: CollisionSource,
    out: &mut Vec<CollisionTriangle>,
    stats: &mut CollisionStats,
) -> Result<usize> {
    triangulate(&Resolver { def, data }, at, LARGE, None, source, out, stats)
}

/// The BSP 3D nodes and planes of a structure BSP's first (normal)
/// collision BSP, for finding the leaf a point is in
/// (`docs/format/sbsp-portals.md` 3). Its leaves are the structure BSP's
/// leaves, whose clusters `sbsp::leaf_clusters` reads.
/// `render_method +0x40`: the global material (`matg`) a shader's surface is
/// made of, a string ID; a terrain shader (`rmtr`) has one per layer there,
/// four in a row. Read from riverworld's shaders (`riverworld_ground_snow`:
/// `tough_terrain_dirt`, `tough_terrain_grass`, `tough_terrain_dirt`,
/// `soft_terrain_snow`; `riverworld_metal`: `hard_metal_thick_for`).
const SHADER_GLOBAL_MATERIAL: usize = 0x40;

/// The global materials a collision shader names; only names of the game's
/// material families are kept, so a shader class laid out otherwise gives
/// nothing rather than a wrong name.
fn global_materials(cache: &CacheFile, shader: &TagEntry) -> Vec<String> {
    const FAMILIES: [&str; 6] = ["soft_", "tough_", "hard_", "liquid_", "energy_", "special_"];
    let class = cache.tags.class_of(shader).code.as_str().to_string();
    if !class.starts_with("rm") {
        return Vec::new();
    }
    let Ok(base) = cache.tag_offset(shader) else {
        return Vec::new();
    };
    let count = if class == "rmtr" { 4 } else { 1 };
    let mut out = Vec::new();
    for i in 0..count {
        let Ok(id) = cache.reader_at(base + SHADER_GLOBAL_MATERIAL + 4 * i).string_id() else {
            continue;
        };
        if let Some(name) = cache.string(id).filter(|n| FAMILIES.iter().any(|f| n.starts_with(f))) {
            out.push(name.to_string());
        }
    }
    out
}

pub fn tree(cache: &CacheFile, resources: &Resources<'_>, sbsp: &TagEntry) -> Result<(Vec<u64>, Vec<[f32; 4]>, usize)> {
    let base = cache.tag_offset(sbsp)?;
    let datum = ResourceId(cache.reader_at(base + 0x38C).u32()?);
    if datum.is_null() {
        return Err(Error::Invalid(format!("{} has no collision resource", sbsp.name())));
    }
    let entry = resources.entry(datum)?;
    if entry.is_empty() || entry.definition_address.kind() != CacheAddress::DEFINITION {
        return Err(Error::Invalid(format!("{}: no usable collision resource", sbsp.name())));
    }
    let def = resources.fixed_definition(&entry)?;
    let data = resources.data(&entry)?;
    let res = Resolver {
        def: &def,
        data: data.bytes(),
    };
    let root = entry.definition_address.offset();
    let bsps = res.block(root)?;
    if bsps.count == 0 || bsps.address.kind() != CacheAddress::DEFINITION {
        return Err(Error::Invalid(format!("{}: no normal collision bsp", sbsp.name())));
    }
    let at = bsps.address.offset();
    let nodes = res.elements(res.block(at)?, 8, "bsp 3d nodes")?;
    let planes = res.elements(res.block(at + 0x0C)?, 16, "planes")?;
    let leaves = res.block(at + 0x18)?.count;
    let nodes = nodes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| u64::from_le_bytes(*b))
        .collect();
    let planes = planes
        .as_chunks::<16>()
        .0
        .iter()
        .map(|b| [f32_at(b, 0), f32_at(b, 4), f32_at(b, 8), f32_at(b, 12)])
        .collect();
    Ok((nodes, planes, leaves))
}

/// Decodes a BSP's collision into `collision`, returning what was found.
pub fn decode(
    cache: &CacheFile,
    resources: &Resources<'_>,
    sbsp: &TagEntry,
    bsp_index: u32,
    dropped_instances: &[u32],
    collision: &mut Collision,
) -> Result<CollisionStats> {
    let base = cache.tag_offset(sbsp)?;
    let mut stats = CollisionStats::default();

    // Collision materials: the shader each surface's material index names.
    let material_base = collision.materials.len() as i16;
    let materials = cache.reader_at(base + 0x28).block()?;
    for o in materials.elements(24) {
        let shader = cache.reader_at(o).tag_ref()?;
        let tag = shader.index.and_then(|i| cache.tags.get(i));
        collision.materials.push(CollisionMaterial {
            shader: tag.map_or_else(|| "(none)".to_string(), |t| t.name().to_string()),
            shader_class: tag
                .map(|t| cache.tags.class_of(t).code.as_str().to_string())
                .unwrap_or_default(),
            global_materials: tag.map(|t| global_materials(cache, t)).unwrap_or_default(),
        });
    }

    let datum = ResourceId(cache.reader_at(base + 0x38C).u32()?);
    if datum.is_null() {
        return Err(Error::Invalid(format!("{} has no collision resource", sbsp.name())));
    }
    let entry = resources.entry(datum)?;
    if entry.is_empty() {
        return Err(Error::Invalid(format!(
            "{}: collision resource {} is empty",
            sbsp.name(),
            entry.index
        )));
    }
    let def = resources.fixed_definition(&entry)?;
    let data = resources.data(&entry)?;
    let res = Resolver {
        def: &def,
        data: data.bytes(),
    };
    if entry.definition_address.kind() != CacheAddress::DEFINITION {
        return Err(Error::Invalid(format!(
            "{}: collision root is in address space {}",
            sbsp.name(),
            entry.definition_address.kind()
        )));
    }
    let root = entry.definition_address.offset();
    if root + 0x30 > def.len() {
        return Err(Error::Invalid(format!(
            "{}: collision root at {root:#x} runs past the {}-byte definition",
            sbsp.name(),
            def.len()
        )));
    }

    // Built locally and appended only on success, so an error here cannot
    // lose the triangles of BSPs decoded before.
    let mut triangles = Vec::new();
    let fix_material = |t: &mut CollisionTriangle| {
        if t.material >= 0 {
            t.material += material_base;
        }
    };

    // The world's own BSP(s): normal and large.
    for (block_at, layout) in [(root, NORMAL), (root + 0xC, LARGE)] {
        let bsps = res.block(block_at)?;
        let bytes = res.elements(bsps, 0x60, "collision bsps")?;
        for i in 0..bsps.count {
            let geometry_at = match bsps.address.kind() {
                CacheAddress::DEFINITION => bsps.address.offset() + i * 0x60,
                _ => {
                    return Err(Error::Unsupported(
                        "collision bsp struct outside the definition".into(),
                    ));
                }
            };
            let _ = bytes;
            let start = triangles.len();
            let n = triangulate(
                &res,
                geometry_at,
                layout,
                None,
                CollisionSource::Bsp(bsp_index),
                &mut triangles,
                &mut stats,
            )?;
            triangles[start..].iter_mut().for_each(fix_material);
            stats.bsp_triangles += n;
        }
    }

    // Instanced geometry: each definition's collision placed by every
    // instance that uses it, unless the instance is render-only or was
    // left out of the render (`sbsp::only_pit_fire`).
    let defs = res.block(root + 0x18)?;
    let def_stride = 0xC4;
    res.elements(defs, def_stride, "instanced geometry definitions")?;
    let mut def_triangles: Vec<Option<Vec<CollisionTriangle>>> = vec![None; defs.count];
    let instances = cache.reader_at(base + 0x1C0).block()?;
    for (i, o) in instances.elements(120).enumerate() {
        if dropped_instances.contains(&(i as u32)) {
            stats.instances_skipped += 1;
            continue;
        }
        let mut r = cache.reader_at(o);
        let scale = r.f32()?;
        let rows = [r.f32x3()?, r.f32x3()?, r.f32x3()?];
        let translation = r.f32x3()?;
        let def_index = r.at(o + 0x34).i16()?;
        let flags = r.at(o + 0x36).u16()?;
        let Ok(d) = usize::try_from(def_index) else {
            stats.instances_skipped += 1;
            continue;
        };
        if d >= defs.count || flags & instance_flags::RENDER_ONLY != 0 {
            stats.instances_skipped += 1;
            continue;
        }
        if def_triangles[d].is_none() {
            // The definition's collision geometry sits inline at +0x14.
            let geometry_at = match defs.address.kind() {
                CacheAddress::DEFINITION => defs.address.offset() + d * def_stride + 0x14,
                _ => {
                    return Err(Error::Unsupported(
                        "instanced geometry definitions outside the definition".into(),
                    ));
                }
            };
            let mut local = Vec::new();
            triangulate(
                &res,
                geometry_at,
                NORMAL,
                None,
                CollisionSource::Instance(bsp_index),
                &mut local,
                &mut stats,
            )?;
            local.iter_mut().for_each(fix_material);
            def_triangles[d] = Some(local);
        }
        let (m, mirrored) = matrix_from_rows(rows, translation, scale);
        let local = def_triangles[d].as_ref().unwrap();
        for t in local {
            let p = t.points.map(|v| m.transform_point3(v));
            triangles.push(CollisionTriangle {
                points: if mirrored { [p[0], p[2], p[1]] } else { p },
                ..*t
            });
        }
        stats.instance_triangles += local.len();
        stats.instances_placed += 1;
    }

    collision.triangles.append(&mut triangles);
    Ok(stats)
}
