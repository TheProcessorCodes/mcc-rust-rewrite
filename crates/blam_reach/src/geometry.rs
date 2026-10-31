//! Reach's render geometry: the mesh list shared by lightmap BSP data,
//! structure BSPs and render models, its compression boxes, the resource
//! holding the vertex and index buffers, and Reach's vertex layouts.
//! Follows `docs/format/reach.md` 4 and 5.
//!
//! The shape is Halo 3's (`blam_geometry::render_geometry`) with wider
//! fields: meshes are 0x5C bytes with eight vertex buffer slots, parts have
//! 32-bit index ranges, index buffer descriptions are 28 bytes, and every
//! vertex is 36 bytes (44 skinned) with float positions.

use glam::Vec3;

use blam_cache::{Block, CacheFile, ResourceData, ResourceId, Resources};
use blam_geometry::render_geometry::DecodedSection;
use blam_geometry::vertex::{Bounds, IndexLayout, Vertices};
use blam_geometry::{Error, Result};

pub const MESH_SIZE: usize = 0x5C;
pub const PART_SIZE: usize = 0x18;
pub const SUBPART_SIZE: usize = 0x10;
pub const BOUNDS_SIZE: usize = 0x34;

/// One draw of a mesh: a range of its index buffer with one shader.
#[derive(Clone, Copy, Debug)]
pub struct Part {
    pub shader: i16,
    pub index_start: u32,
    pub index_count: u32,
    pub flags: u16,
}

impl Part {
    pub const IS_WATER_SURFACE: u16 = 1 << 0;
}

/// A range of a mesh's indices; render models' instanced geometry gives
/// each instance one.
#[derive(Clone, Copy, Debug)]
pub struct Subpart {
    pub index_start: u32,
    pub index_count: u32,
    pub part: u16,
}

#[derive(Clone, Debug)]
pub struct Mesh {
    pub parts: Vec<Part>,
    pub subparts: Vec<Subpart>,
    /// Vertex buffer slots: 0 the vertices, then lightmap texcoords and
    /// per-vertex lighting where present.
    pub buffers: [Option<u16>; 8],
    pub index_buffer: Option<u16>,
    pub flags: u16,
    pub node_index: u8,
    pub vertex_type: u8,
    pub index_type: u8,
}

impl Mesh {
    pub const MESH_IS_UNINDEXED: u16 = 1 << 4;
    pub const UNCOMPRESSED_POSITION: u16 = 1 << 8;

    pub fn vertex_buffer(&self) -> Option<u16> {
        self.buffers[0]
    }
    pub fn unindexed(&self) -> bool {
        self.index_buffer.is_none() || self.flags & Self::MESH_IS_UNINDEXED != 0
    }
}

fn opt(v: i16) -> Option<u16> {
    (v >= 0).then_some(v as u16)
}

/// Reads a block of meshes.
pub fn read_meshes(cache: &CacheFile, block: Block) -> Result<Vec<Mesh>> {
    block
        .elements(MESH_SIZE)
        .map(|o| {
            let parts = cache
                .reader_at(o)
                .block()?
                .elements(PART_SIZE)
                .map(|p| {
                    let mut r = cache.reader_at(p);
                    Ok(Part {
                        shader: r.i16()?,
                        index_start: r.at(p + 4).u32()?,
                        index_count: r.at(p + 8).u32()?,
                        flags: r.at(p + 0x12).u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let subparts = cache
                .reader_at(o + 0xC)
                .block()?
                .elements(SUBPART_SIZE)
                .map(|p| {
                    let mut r = cache.reader_at(p);
                    Ok(Subpart {
                        index_start: r.u32()?,
                        index_count: r.u32()?,
                        part: r.u16()?,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let mut r = cache.reader_at(o + 0x18);
            let mut buffers = [None; 8];
            for b in &mut buffers {
                *b = opt(r.i16()?);
            }
            let index_buffer = opt(r.i16()?);
            let mut r = r.at(o + 0x2C);
            Ok(Mesh {
                parts,
                subparts,
                buffers,
                index_buffer,
                flags: r.u16()?,
                node_index: r.u8()?,
                vertex_type: r.u8()?,
                index_type: r.at(o + 0x32).u8()?,
            })
        })
        .collect()
}

/// Reads a block of compression boxes (0x34 bytes, the ranges at +4).
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
pub struct VertexBuffer {
    pub count: u32,
    /// The buffer's own type code (1 world, 2 rigid, 9 lightmap texcoords ...).
    pub kind: u16,
    pub stride: u16,
    pub length: u32,
    pub offset: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct IndexBuffer {
    pub format: i32,
    pub length: u32,
    pub offset: u32,
}

/// A render geometry resource: its buffer tables and its bytes.
pub struct GeometryResource {
    pub vertex_buffers: Vec<VertexBuffer>,
    pub index_buffers: Vec<IndexBuffer>,
    data: ResourceData,
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_bits(u32_at(b, o))
}
fn f16_at(b: &[u8], o: usize) -> f32 {
    blam_geometry::lightmap::f16(u16_at(b, o))
}
fn u16n(b: &[u8], o: usize) -> f32 {
    u16_at(b, o) as f32 / 65535.0
}
fn i16n(b: &[u8], o: usize) -> f32 {
    (u16_at(b, o) as i16 as f32 / 32767.0).clamp(-1.0, 1.0)
}
fn i16n3(b: &[u8], o: usize) -> Vec3 {
    Vec3::new(i16n(b, o), i16n(b, o + 2), i16n(b, o + 4))
}

impl GeometryResource {
    /// The definition is laid out as every vertex buffer description (28
    /// bytes: count +0, type +4, stride +6, length +8), one 12-byte
    /// pointer per vertex buffer, every index buffer description (28
    /// bytes: format +0, length +8), one pointer per index buffer, then the
    /// two blocks (count, address) 24 and 12 bytes from the end. Fixup `i`
    /// is vertex buffer `i`'s place in the data, fixup `2 * vertex buffers
    /// + j` index buffer `j`'s.
    pub fn load(resources: &Resources<'_>, id: ResourceId) -> Result<GeometryResource> {
        let entry = resources.entry(id)?;
        if entry.is_empty() || entry.salt != (id.0 >> 16) as u16 {
            return Err(Error::Invalid(format!("geometry resource {:#010x} is stale or wiped", id.0)));
        }
        let def = resources.definition(&entry)?;
        if def.len() < 24 {
            return Err(Error::Invalid(format!(
                "render geometry resource {} has a {}-byte definition",
                entry.index,
                def.len()
            )));
        }
        let vb_count = u32_at(def, def.len() - 24) as usize;
        let ib_count = u32_at(def, def.len() - 12) as usize;
        let ib_base = 40 * vb_count;
        if ib_base + 40 * ib_count + 24 > def.len() {
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
            .map(|i| VertexBuffer {
                count: u32_at(def, 28 * i),
                kind: u16_at(def, 28 * i + 4),
                stride: u16_at(def, 28 * i + 6),
                length: u32_at(def, 28 * i + 8),
                offset: fixups[i],
            })
            .collect();
        let index_buffers = (0..ib_count)
            .map(|j| IndexBuffer {
                format: u32_at(def, ib_base + 28 * j) as i32,
                length: u32_at(def, ib_base + 28 * j + 8),
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

    pub fn vertex_bytes(&self, i: u16) -> Result<(&VertexBuffer, &[u8])> {
        let info = self
            .vertex_buffers
            .get(i as usize)
            .ok_or_else(|| Error::Invalid(format!("vertex buffer {i} of {}", self.vertex_buffers.len())))?;
        Ok((info, self.slice(info.offset, info.length, "vertex buffer")?))
    }

    pub fn index_bytes(&self, j: u16) -> Result<(&IndexBuffer, &[u8])> {
        let info = self
            .index_buffers
            .get(j as usize)
            .ok_or_else(|| Error::Invalid(format!("index buffer {j} of {}", self.index_buffers.len())))?;
        Ok((info, self.slice(info.offset, info.length, "index buffer")?))
    }

    /// Lightmap texcoords (2 x u16 / 65535) of one vertex buffer, if it has
    /// exactly `count` 4-byte vertices.
    pub fn texcoords(&self, buffer: u16, count: usize) -> Result<Option<Vec<[f32; 2]>>> {
        let (info, bytes) = self.vertex_bytes(buffer)?;
        if info.count as usize != count || info.stride != 4 || bytes.len() < 4 * count {
            return Ok(None);
        }
        Ok(Some(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .take(count)
                .map(|c| [u16n(c, 0), u16n(c, 2)])
                .collect(),
        ))
    }
}

/// Reach's vertex types (mesh `+0x2F`).
pub mod vertex_type {
    /// Float positions and half-float texcoords: BSP clusters.
    pub const WORLD: u8 = 0;
    /// Positions 0..1 in the mesh's compression box, 16-bit texcoords.
    pub const RIGID: u8 = 1;
    /// Rigid plus four bone indices and weights.
    pub const SKINNED: u8 = 2;
}

/// A tangent with the handedness of its fourth component (zero counts as
/// right-handed).
fn tangent(t: Vec3, w: f32) -> [f32; 4] {
    [t.x, t.y, t.z, if w < 0.0 { -1.0 } else { 1.0 }]
}

/// Unpacks `count` vertices of `vertex_type` laid out every `stride`
/// bytes. Rigid and skinned positions and texcoords are expanded with
/// `bounds` unless `uncompressed`.
pub fn decode_vertices(
    vertex_type: u8,
    bytes: &[u8],
    count: usize,
    stride: usize,
    bounds: Option<&Bounds>,
    uncompressed: bool,
) -> Result<Vertices> {
    let need = match vertex_type {
        vertex_type::WORLD | vertex_type::RIGID => 36,
        vertex_type::SKINNED => 44,
        t => return Err(Error::Unsupported(format!("Reach vertex type {t}"))),
    };
    if stride < need || bytes.len() < count.saturating_sub(1) * stride + need {
        return Err(Error::Invalid(format!(
            "vertex buffer of {} bytes, stride {stride}, cannot hold {count} type {vertex_type} vertices",
            bytes.len()
        )));
    }
    let skinned = vertex_type == vertex_type::SKINNED;
    let expand = vertex_type != vertex_type::WORLD && !uncompressed;
    let bounds = bounds.filter(|b| expand && !b.is_empty());
    let mut out = Vertices {
        positions: Vec::with_capacity(count),
        normals: Vec::with_capacity(count),
        uvs: Vec::with_capacity(count),
        tangents: Vec::with_capacity(count),
        joints: Vec::with_capacity(if skinned { count } else { 0 }),
        weights: Vec::with_capacity(if skinned { count } else { 0 }),
    };
    for i in 0..count {
        let v = &bytes[i * stride..i * stride + need];
        let p = Vec3::new(f32_at(v, 0), f32_at(v, 4), f32_at(v, 8));
        let t = if vertex_type == vertex_type::WORLD {
            [f16_at(v, 16), f16_at(v, 18)]
        } else {
            [u16n(v, 16), u16n(v, 18)]
        };
        match bounds {
            Some(b) => {
                out.positions.push(Vec3::new(
                    b.x[0] + p.x * (b.x[1] - b.x[0]),
                    b.y[0] + p.y * (b.y[1] - b.y[0]),
                    b.z[0] + p.z * (b.z[1] - b.z[0]),
                ));
                out.uvs.push([b.u[0] + t[0] * (b.u[1] - b.u[0]), b.v[0] + t[1] * (b.v[1] - b.v[0])]);
            }
            None => {
                out.positions.push(p);
                out.uvs.push(t);
            }
        }
        out.normals.push(i16n3(v, 20).normalize_or(Vec3::Z));
        out.tangents.push(tangent(i16n3(v, 28), i16n(v, 34)));
        if skinned {
            out.joints.push([v[36] as u16, v[37] as u16, v[38] as u16, v[39] as u16]);
            let w = [v[40], v[41], v[42], v[43]].map(|x| x as f32 / 255.0);
            let sum: f32 = w.iter().sum();
            out.weights.push(if sum > 0.0 { w.map(|x| x / sum) } else { [1.0, 0.0, 0.0, 0.0] });
        }
    }
    Ok(out)
}

/// Unpacks one mesh, or `None` when it has no vertices.
pub fn decode_mesh(mesh: &Mesh, resource: &GeometryResource, bounds: Option<&Bounds>) -> Result<Option<DecodedSection>> {
    let Some(vbi) = mesh.vertex_buffer() else {
        return Ok(None);
    };
    if vbi as usize >= resource.vertex_buffers.len() {
        return Ok(None);
    }
    let (vinfo, vbytes) = resource.vertex_bytes(vbi)?;
    if vinfo.count == 0 {
        return Ok(None);
    }
    let vertices = decode_vertices(
        mesh.vertex_type,
        vbytes,
        vinfo.count as usize,
        vinfo.stride as usize,
        bounds,
        mesh.flags & Mesh::UNCOMPRESSED_POSITION != 0,
    )?;
    let (indices, layout) = if mesh.unindexed() {
        let layout = IndexLayout::from_id(mesh.index_type as i32)
            .ok_or_else(|| Error::Unsupported(format!("index type {}", mesh.index_type)))?;
        ((0..vinfo.count).collect(), layout)
    } else {
        let (iinfo, ibytes) = resource.index_bytes(mesh.index_buffer.unwrap())?;
        let layout = IndexLayout::from_id(iinfo.format)
            .ok_or_else(|| Error::Unsupported(format!("index format {}", iinfo.format)))?;
        let indices = if vinfo.count > u16::MAX as u32 {
            ibytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect()
        } else {
            ibytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b) as u32).collect()
        };
        (indices, layout)
    };
    Ok(Some(DecodedSection {
        vertices,
        indices,
        layout,
    }))
}

/// The triangles of one part as a flat index list, dropping any that name
/// vertices the buffer does not have.
pub fn part_triangles(decoded: &DecodedSection, start: u32, count: u32) -> Vec<u32> {
    let start = (start as usize).min(decoded.indices.len());
    let end = (start + count as usize).min(decoded.indices.len());
    let mut tris = decoded.layout.triangles(&decoded.indices[start..end]);
    let n = decoded.vertices.positions.len() as u32;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rigid_vertex_expands_with_bounds() {
        let bounds = Bounds {
            x: [-1.0, 1.0],
            y: [0.0, 10.0],
            z: [5.0, 5.0],
            u: [0.0, 2.0],
            v: [0.0, 1.0],
        };
        let mut v = vec![0u8; 36];
        v[0..4].copy_from_slice(&1.0f32.to_le_bytes());
        v[4..8].copy_from_slice(&0.5f32.to_le_bytes());
        v[16..18].copy_from_slice(&32768u16.to_le_bytes());
        v[20..22].copy_from_slice(&32767i16.to_le_bytes());
        let got = decode_vertices(vertex_type::RIGID, &v, 1, 36, Some(&bounds), false).unwrap();
        assert_eq!(got.positions[0], Vec3::new(1.0, 5.0, 5.0));
        assert!((got.uvs[0][0] - 1.0).abs() < 1e-3);
        assert_eq!(got.normals[0], Vec3::X);
        // World vertices are never expanded.
        let got = decode_vertices(vertex_type::WORLD, &v, 1, 36, Some(&bounds), false).unwrap();
        assert_eq!(got.positions[0], Vec3::new(1.0, 0.5, 0.0));
    }
}
