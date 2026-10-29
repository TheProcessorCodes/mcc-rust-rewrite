//! One geometry section (a BSP cluster, an instanced geometry definition, a
//! render model section) decoded from its raw data into a mesh.
//! Follows `docs/format/halo2.md` section 5.

use blam_geometry::{Mesh, Part};
use glam::Vec3;

use crate::cache::Reader;
use crate::{Error, Result};

/// Where a section block keeps its counts and its raw-data reference.
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub vertices: usize,
    pub faces: usize,
    pub pointer: usize,
    pub size: usize,
    pub header: usize,
    pub resources: usize,
}

/// Structure BSP clusters and instanced geometry definitions.
pub const BSP: Layout = Layout {
    vertices: 0x00,
    faces: 0x02,
    pointer: 0x28,
    size: 0x2C,
    header: 0x30,
    resources: 0x38,
};

/// Render model sections (92 bytes).
pub const MODEL: Layout = Layout {
    vertices: 0x04,
    faces: 0x06,
    pointer: 0x38,
    size: 0x3C,
    header: 0x40,
    resources: 0x48,
};

/// Resource kinds in a section's resource list.
const BLOCK: u8 = 0;
const VERTEX_BUFFER: u8 = 2;
/// Field offsets (the resource's "primary locator") in the section's
/// render data: parts, strip indices, vertex buffers.
const PARTS: i16 = 0x00;
const INDICES: i16 = 0x20;
const BUFFERS: i16 = 0x38;
/// Size of one part.
const PART_SIZE: usize = 0x48;

struct Resource {
    kind: u8,
    primary: i16,
    secondary: i16,
    size: usize,
    offset: usize,
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Decodes the section at `section`; `material` turns a part's material
/// index into a scene material. `None` for a section with no geometry.
pub fn decode(section: Reader<'_>, layout: Layout, name: &str, material: &mut dyn FnMut(i16) -> u32) -> Result<Option<Mesh>> {
    let vertex_count = section.u16(layout.vertices)? as usize;
    let face_count = section.u16(layout.faces)? as usize;
    if vertex_count == 0 {
        return Ok(None);
    }
    let pointer = section.u32(layout.pointer)?;
    let size = section.u32(layout.size)? as usize;
    let header = section.u32(layout.header)? as usize;
    let raw = section.map.raw(pointer, size)?;
    let base = header + 8;
    let resources = section
        .block(layout.resources, 16)?
        .into_iter()
        .map(|r| {
            Ok(Resource {
                kind: r.u8(0)?,
                primary: r.i16(4)?,
                secondary: r.i16(6)?,
                size: r.u32(8)? as usize,
                offset: r.u32(12)? as usize,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let bytes_of = |r: &Resource| -> Result<&[u8]> {
        let start = base + r.offset;
        raw.get(start..start + r.size)
            .ok_or_else(|| Error::Invalid(format!("{name}: resource at {start:#x} past the section data")))
    };
    let find = |kind: u8, primary: i16, secondary: Option<i16>| {
        resources
            .iter()
            .find(|r| r.kind == kind && r.primary == primary && secondary.is_none_or(|s| r.secondary == s))
    };
    let (Some(parts), Some(indices), Some(positions)) = (
        find(BLOCK, PARTS, None),
        find(BLOCK, INDICES, None),
        find(VERTEX_BUFFER, BUFFERS, Some(0)),
    ) else {
        return Err(Error::Invalid(format!("{name}: section without parts, indices or positions")));
    };

    let mut mesh = Mesh {
        name: name.to_string(),
        ..Default::default()
    };
    let pos = bytes_of(positions)?;
    let stride = pos.len() / vertex_count;
    if stride < 12 {
        return Err(Error::Invalid(format!("{name}: {stride}-byte positions")));
    }
    mesh.positions = (0..vertex_count)
        .map(|i| Vec3::new(f32_at(pos, i * stride), f32_at(pos, i * stride + 4), f32_at(pos, i * stride + 8)))
        .collect();
    if let Some(r) = find(VERTEX_BUFFER, BUFFERS, Some(1)) {
        let b = bytes_of(r)?;
        if b.len() >= vertex_count * 8 {
            mesh.uvs = (0..vertex_count).map(|i| [f32_at(b, i * 8), f32_at(b, i * 8 + 4)]).collect();
        }
    }
    // Normal, binormal, tangent: nine floats per vertex.
    if let Some(r) = find(VERTEX_BUFFER, BUFFERS, Some(2)) {
        let b = bytes_of(r)?;
        if b.len() >= vertex_count * 36 {
            let v = |i: usize, k: usize| Vec3::new(f32_at(b, i * 36 + k), f32_at(b, i * 36 + k + 4), f32_at(b, i * 36 + k + 8));
            for i in 0..vertex_count {
                let (n, bn, t) = (v(i, 0), v(i, 12), v(i, 24));
                mesh.normals.push(n.normalize_or(Vec3::Z));
                let w = if n.cross(t).dot(bn) < 0.0 { -1.0 } else { 1.0 };
                mesh.tangents.push([t.x, t.y, t.z, w]);
            }
        }
    }

    let ib = bytes_of(indices)?;
    let index: Vec<u16> = ib.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect();
    // A triangle list when the face count says so and the first triangles
    // are not degenerate; otherwise a strip.
    let list = face_count * 3 == index.len()
        && index
            .as_chunks::<3>()
            .0
            .iter()
            .take(10)
            .all(|t| t[0] != t[1] && t[1] != t[2] && t[0] != t[2]);
    let part_bytes = bytes_of(parts)?;
    let part_size = if parts.secondary > 0 { parts.secondary as usize } else { PART_SIZE };
    for p in part_bytes.chunks_exact(part_size) {
        let mat = i16::from_le_bytes([p[4], p[5]]);
        let start = u16::from_le_bytes([p[6], p[7]]) as usize;
        let len = u16::from_le_bytes([p[8], p[9]]) as usize;
        let Some(run) = index.get(start..start + len) else {
            return Err(Error::Invalid(format!("{name}: part indices {start}+{len} past {}", index.len())));
        };
        let mut tris = Vec::with_capacity(len * 3);
        let ok = |a: u16| (a as usize) < vertex_count;
        if list {
            for t in run.as_chunks::<3>().0 {
                if t.iter().all(|&a| ok(a)) {
                    tris.extend(t.iter().map(|&a| a as u32));
                }
            }
        } else {
            for i in 0..run.len().saturating_sub(2) {
                let (a, b, c) = (run[i], run[i + 1], run[i + 2]);
                if a == b || b == c || a == c || !ok(a) || !ok(b) || !ok(c) {
                    continue;
                }
                if i % 2 == 0 {
                    tris.extend([a as u32, b as u32, c as u32]);
                } else {
                    tris.extend([a as u32, c as u32, b as u32]);
                }
            }
        }
        if !tris.is_empty() {
            mesh.parts.push(Part {
                material: material(mat),
                indices: tris,
            });
        }
    }
    Ok(Some(mesh))
}
