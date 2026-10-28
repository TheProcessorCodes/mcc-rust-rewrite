//! The `render_geometry` structure shared by structure BSPs and render
//! models: a list of sections (meshes), each a set of submeshes drawing a
//! range of one index buffer with one shader, with the vertex and index
//! buffers themselves in a resource. Follows
//! `docs/format/sbsp-render-geometry.md` sections 3 and 4.

use blam_cache::{Block, CacheFile, ResourceData, ResourceId, Resources};

use crate::vertex::{self, Bounds, IndexLayout, VertexFormat, Vertices};
use crate::{Error, Result};

pub const SECTION_SIZE: usize = 76;
pub const SUBMESH_SIZE: usize = 16;
pub const BOUNDS_SIZE: usize = 44;

#[derive(Clone, Copy, Debug)]
pub struct Submesh {
    pub shader: i16,
    pub index_start: u16,
    pub index_count: u16,
    pub vertex_count: u16,
    pub flags: u8,
}

impl Submesh {
    pub const IS_WATER_SURFACE: u8 = 1 << 3;
}

#[derive(Clone, Copy, Debug)]
pub struct Subset {
    pub index_start: u16,
    pub index_count: u16,
    pub submesh: u16,
    pub vertex_count: u16,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub submeshes: Vec<Submesh>,
    pub subsets: Vec<Subset>,
    pub vertex_buffer: Option<u16>,
    /// Vertex buffer slot 1: a structure BSP cluster's lightmap texcoords.
    pub lightmap_buffer: Option<u16>,
    pub index_buffer: Option<u16>,
    pub flags: u8,
    pub node_index: u8,
    pub vertex_format: u8,
    /// Layout of the implied index buffer when the section is unindexed.
    pub index_format: u8,
}

impl Section {
    pub const MESH_IS_UNINDEXED: u8 = 1 << 4;
    pub fn unindexed(&self) -> bool {
        self.index_buffer.is_none() || self.flags & Self::MESH_IS_UNINDEXED != 0
    }
}

/// Reads a block of sections.
pub fn read_sections(cache: &CacheFile, block: Block) -> Result<Vec<Section>> {
    block
        .elements(SECTION_SIZE)
        .map(|o| {
            let mut r = cache.reader_at(o);
            let submeshes = r.block()?;
            let submeshes = submeshes
                .elements(SUBMESH_SIZE)
                .map(|so| {
                    let mut r = cache.reader_at(so);
                    Ok(Submesh {
                        shader: r.i16()?,
                        index_start: r.at(so + 4).u16()?,
                        index_count: r.at(so + 6).u16()?,
                        flags: r.at(so + 0xD).u8()?,
                        vertex_count: r.at(so + 0xE).u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let subsets = cache.reader_at(o + 0xC).block()?;
            let subsets = subsets
                .elements(8)
                .map(|so| {
                    let mut r = cache.reader_at(so);
                    Ok(Subset {
                        index_start: r.u16()?,
                        index_count: r.u16()?,
                        submesh: r.u16()?,
                        vertex_count: r.u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let vertex_buffer = r.at(o + 0x18).i16()?;
            let lightmap_buffer = r.at(o + 0x1A).i16()?;
            let index_buffer = r.at(o + 0x28).i16()?;
            let mut r = r.at(o + 0x2C);
            Ok(Section {
                submeshes,
                subsets,
                vertex_buffer: (vertex_buffer >= 0).then_some(vertex_buffer as u16),
                lightmap_buffer: (lightmap_buffer >= 0).then_some(lightmap_buffer as u16),
                index_buffer: (index_buffer >= 0).then_some(index_buffer as u16),
                flags: r.u8()?,
                node_index: r.u8()?,
                vertex_format: r.u8()?,
                index_format: r.at(o + 0x30).u8()?,
            })
        })
        .collect()
}

/// Reads a block of compression bounds.
pub fn read_bounds(cache: &CacheFile, block: Block) -> Result<Vec<Bounds>> {
    block
        .elements(BOUNDS_SIZE)
        .map(|o| {
            let mut r = cache.reader_at(o + 4);
            Ok(Bounds {
                x: r.f32x2()?,
                y: r.f32x2()?,
                z: r.f32x2()?,
                u: r.f32x2()?,
                v: r.f32x2()?,
            })
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
pub struct VertexBufferInfo {
    pub count: u32,
    pub length: u32,
    /// Where the buffer starts in the resource data.
    pub offset: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct IndexBufferInfo {
    pub format: i32,
    pub length: u32,
    pub offset: u32,
}

/// A render geometry resource: its buffer tables and its bytes.
pub struct GeometryResource {
    pub vertex_buffers: Vec<VertexBufferInfo>,
    pub index_buffers: Vec<IndexBufferInfo>,
    data: ResourceData,
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl GeometryResource {
    pub fn load(resources: &Resources<'_>, id: ResourceId) -> Result<GeometryResource> {
        let entry = resources.entry(id)?;
        let def = resources.definition(&entry)?;
        if def.len() < 48 {
            return Err(Error::Invalid(format!(
                "render geometry resource {} has a {}-byte definition",
                entry.index,
                def.len()
            )));
        }
        let vb_count = i32_at(def, def.len() - 24).max(0) as usize;
        let ib_count = i32_at(def, def.len() - 12).max(0) as usize;
        let ib_base = 40 * vb_count;
        if ib_base + 24 * ib_count + 12 * ib_count + 48 > def.len() {
            return Err(Error::Invalid(format!(
                "render geometry resource {}: {vb_count} vertex and {ib_count} index buffers do not fit a {}-byte definition",
                entry.index,
                def.len()
            )));
        }
        let fixups = resources.fixup_offsets(&entry)?;
        if fixups.len() < 2 * vb_count + ib_count {
            return Err(Error::Invalid(format!(
                "render geometry resource {}: {} fixups for {vb_count} vertex and {ib_count} index buffers",
                entry.index,
                fixups.len()
            )));
        }
        let vertex_buffers = (0..vb_count)
            .map(|i| VertexBufferInfo {
                count: i32_at(def, 28 * i).max(0) as u32,
                length: i32_at(def, 28 * i + 8).max(0) as u32,
                offset: fixups[i],
            })
            .collect();
        let index_buffers = (0..ib_count)
            .map(|j| IndexBufferInfo {
                format: i32_at(def, ib_base + 24 * j),
                length: i32_at(def, ib_base + 24 * j + 4).max(0) as u32,
                offset: fixups[2 * vb_count + j],
            })
            .collect();
        let data = resources.data(&entry)?;
        Ok(GeometryResource {
            vertex_buffers,
            index_buffers,
            data,
        })
    }

    fn slice(&self, offset: u32, length: u32, what: &str) -> Result<&[u8]> {
        let bytes = self.data.bytes();
        let start = offset as usize;
        let end = start + length as usize;
        if end > bytes.len() {
            return Err(Error::Invalid(format!(
                "{what} at {start:#x}+{length:#x} runs past the {}-byte resource",
                bytes.len()
            )));
        }
        Ok(&bytes[start..end])
    }

    pub fn vertex_bytes(&self, i: u16) -> Result<(&VertexBufferInfo, &[u8])> {
        let info = self
            .vertex_buffers
            .get(i as usize)
            .ok_or_else(|| Error::Invalid(format!("vertex buffer {i} of {}", self.vertex_buffers.len())))?;
        Ok((info, self.slice(info.offset, info.length, "vertex buffer")?))
    }

    pub fn index_bytes(&self, j: u16) -> Result<(&IndexBufferInfo, &[u8])> {
        let info = self
            .index_buffers
            .get(j as usize)
            .ok_or_else(|| Error::Invalid(format!("index buffer {j} of {}", self.index_buffers.len())))?;
        Ok((info, self.slice(info.offset, info.length, "index buffer")?))
    }
}

/// A section's geometry, unpacked but still in its own frame: vertices, the
/// raw index buffer and how its indices make triangles.
#[derive(Clone, Debug)]
pub struct DecodedSection {
    pub vertices: Vertices,
    pub indices: Vec<u32>,
    pub layout: IndexLayout,
}

impl DecodedSection {
    /// The triangles of one submesh as a flat index list.
    pub fn submesh_triangles(&self, s: &Submesh) -> Vec<u32> {
        let start = (s.index_start as usize).min(self.indices.len());
        let end = (start + s.index_count as usize).min(self.indices.len());
        let mut tris = self.layout.triangles(&self.indices[start..end]);
        // Drop triangles naming vertices the buffer does not have, so a
        // damaged map cannot make later code index out of bounds.
        let n = self.vertices.positions.len() as u32;
        if tris.iter().any(|&i| i >= n) {
            tris = tris
                .as_chunks::<3>()
                .0
                .iter()
                .filter(|t| t.iter().all(|&i| i < n))
                .flatten()
                .copied()
                .collect();
        }
        tris
    }
}

/// Unpacks one section, or `None` when it has no vertex buffer.
pub fn decode_section(
    section: &Section,
    resource: &GeometryResource,
    bounds: Option<&Bounds>,
) -> Result<Option<DecodedSection>> {
    let Some(vbi) = section.vertex_buffer else {
        return Ok(None);
    };
    if vbi as usize >= resource.vertex_buffers.len() {
        return Ok(None);
    }
    let (vinfo, vbytes) = resource.vertex_bytes(vbi)?;
    if vinfo.count == 0 {
        return Ok(None);
    }
    let format = VertexFormat::from_id(section.vertex_format).ok_or_else(|| {
        Error::Unsupported(format!("vertex format {:#x}", section.vertex_format))
    })?;
    let vertices = vertex::decode(format, vbytes, vinfo.count as usize, bounds)?;

    let (indices, layout) = if section.unindexed() {
        let layout = IndexLayout::from_id(section.index_format as i32).ok_or_else(|| {
            Error::Unsupported(format!("index format {}", section.index_format))
        })?;
        ((0..vinfo.count).collect(), layout)
    } else {
        let (iinfo, ibytes) = resource.index_bytes(section.index_buffer.unwrap())?;
        let layout = IndexLayout::from_id(iinfo.format)
            .ok_or_else(|| Error::Unsupported(format!("index format {}", iinfo.format)))?;
        let indices = if vinfo.count > u16::MAX as u32 {
            ibytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect()
        } else {
            ibytes
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b) as u32)
                .collect()
        };
        (indices, layout)
    };
    Ok(Some(DecodedSection {
        vertices,
        indices,
        layout,
    }))
}
