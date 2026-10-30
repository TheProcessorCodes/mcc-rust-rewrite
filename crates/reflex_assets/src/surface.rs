//! `.surface`: a model's vertex and index buffers, one group per material
//! slot (the `.model` names the slots).
//!
//! A 16-byte header (`00 CC BB 99`, version, group count, 0), then a
//! 0x160-byte record per group: +8 flags (non-zero when skinned), +12
//! primitive type, vertex count, index bytes, +24 a D3DVERTEXELEMENT9
//! declaration (8 bytes per element, ended by stream 0xFF), +184 stream
//! count and vertex bytes, +252 the size of a u8 bone palette. The data
//! follows, per group: the palette, the u16 indices (a triangle strip, or
//! a list for type 4), the vertices. Skinned groups are in bind pose, in
//! model space; the others are in their bone's space.

use crate::Error;

pub struct Group {
    pub skinned: bool,
    pub positions: Vec<[f32; 3]>,
    /// Empty when the surface has none.
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Triangles, in the file's winding.
    pub triangles: Vec<[u32; 3]>,
}

fn u32_at(b: &[u8], p: usize) -> Result<usize, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
        .ok_or_else(|| Error::Format(format!("surface ends at {p}")))
}

struct Element {
    offset: usize,
    kind: u8,
    usage: u8,
    index: u8,
}

/// One D3DDECLTYPE value as up to four floats.
fn decode(kind: u8, b: &[u8]) -> Option<[f32; 4]> {
    let f32s = |n: usize| -> Option<[f32; 4]> {
        let mut v = [0.0, 0.0, 0.0, 1.0];
        for (i, x) in v.iter_mut().enumerate().take(n) {
            *x = f32::from_le_bytes(b.get(i * 4..i * 4 + 4)?.try_into().ok()?);
        }
        Some(v)
    };
    let i16s = |n: usize, scale: f32| -> Option<[f32; 4]> {
        let mut v = [0.0, 0.0, 0.0, 1.0];
        for (i, x) in v.iter_mut().enumerate().take(n) {
            *x = i16::from_le_bytes(b.get(i * 2..i * 2 + 2)?.try_into().ok()?) as f32 / scale;
        }
        Some(v)
    };
    let u16s = |n: usize, scale: f32| -> Option<[f32; 4]> {
        let mut v = [0.0, 0.0, 0.0, 1.0];
        for (i, x) in v.iter_mut().enumerate().take(n) {
            *x = u16::from_le_bytes(b.get(i * 2..i * 2 + 2)?.try_into().ok()?) as f32 / scale;
        }
        Some(v)
    };
    let f16s = |n: usize| -> Option<[f32; 4]> {
        let mut v = [0.0, 0.0, 0.0, 1.0];
        for (i, x) in v.iter_mut().enumerate().take(n) {
            *x = half(u16::from_le_bytes(b.get(i * 2..i * 2 + 2)?.try_into().ok()?));
        }
        Some(v)
    };
    let u8s = |scale: f32| -> Option<[f32; 4]> {
        let s = b.get(0..4)?;
        Some([s[0] as f32 / scale, s[1] as f32 / scale, s[2] as f32 / scale, s[3] as f32 / scale])
    };
    let packed = |signed: bool| -> Option<[f32; 4]> {
        let w = u32::from_le_bytes(b.get(0..4)?.try_into().ok()?);
        let c = |shift: u32| {
            let v = (w >> shift) & 0x3FF;
            if signed { ((v << 22) as i32 >> 22) as f32 / 511.0 } else { v as f32 }
        };
        Some([c(0), c(10), c(20), 1.0])
    };
    match kind {
        0..=3 => f32s(kind as usize + 1),
        4 => u8s(255.0).map(|[b, g, r, a]| [r, g, b, a]),
        5 => u8s(1.0),
        6 => i16s(2, 1.0),
        7 => i16s(4, 1.0),
        8 => u8s(255.0),
        9 => i16s(2, 32767.0),
        10 => i16s(4, 32767.0),
        11 => u16s(2, 65535.0),
        12 => u16s(4, 65535.0),
        13 => packed(false),
        14 => packed(true),
        15 => f16s(2),
        16 => f16s(4),
        _ => None,
    }
}

fn size_of(kind: u8) -> usize {
    match kind {
        0 => 4,
        1 => 8,
        2 => 12,
        3 => 16,
        4 | 5 | 6 | 8 | 9 | 11 | 13 | 14 | 15 => 4,
        7 | 10 | 12 | 16 => 8,
        _ => 0,
    }
}

/// An IEEE half as f32.
fn half(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (h >> 10) & 0x1F;
    let mant = (h & 0x3FF) as f32;
    sign * match exp {
        0 => mant * 2f32.powi(-24),
        31 => f32::INFINITY,
        e => (1.0 + mant / 1024.0) * 2f32.powi(e as i32 - 15),
    }
}

pub fn read(data: &[u8]) -> Result<Vec<Group>, Error> {
    if u32_at(data, 0)? != 0x99BB_CC00 {
        return Err(Error::Format("not a .surface".into()));
    }
    let count = u32_at(data, 8)?;
    struct Record {
        skinned: bool,
        prim: usize,
        vertices: usize,
        index_bytes: usize,
        vertex_bytes: usize,
        palette: usize,
        elements: Vec<Element>,
    }
    let mut records = Vec::with_capacity(count);
    for g in 0..count {
        let r = 0x10 + g * 0x160;
        let mut elements = Vec::new();
        for e in 0..20 {
            let p = r + 24 + e * 8;
            let s = data.get(p..p + 8).ok_or_else(|| Error::Format("surface declaration".into()))?;
            if u16::from_le_bytes([s[0], s[1]]) == 0xFF {
                break;
            }
            elements.push(Element {
                offset: u16::from_le_bytes([s[2], s[3]]) as usize,
                kind: s[4],
                usage: s[6],
                index: s[7],
            });
        }
        records.push(Record {
            skinned: u32_at(data, r + 8)? != 0,
            prim: u32_at(data, r + 12)?,
            vertices: u32_at(data, r + 16)?,
            index_bytes: u32_at(data, r + 20)?,
            vertex_bytes: u32_at(data, r + 188)?,
            palette: u32_at(data, r + 252)?,
            elements,
        });
    }
    let mut p = 0x10 + count * 0x160;
    let mut groups = Vec::with_capacity(count);
    for r in records {
        p += r.palette;
        let indices: Vec<u32> = data
            .get(p..p + r.index_bytes)
            .ok_or_else(|| Error::Format("surface indices".into()))?
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]) as u32)
            .collect();
        p += r.index_bytes;
        let vb = data
            .get(p..p + r.vertex_bytes)
            .ok_or_else(|| Error::Format("surface vertices".into()))?;
        p += r.vertex_bytes;
        let stride = r.vertex_bytes / r.vertices.max(1);
        let read = |usage: u8, index: u8| -> Vec<[f32; 4]> {
            let Some(e) = r.elements.iter().find(|e| e.usage == usage && e.index == index) else {
                return Vec::new();
            };
            (0..r.vertices)
                .map_while(|v| {
                    let at = v * stride + e.offset;
                    decode(e.kind, vb.get(at..at + size_of(e.kind))?)
                })
                .collect()
        };
        let positions: Vec<[f32; 3]> = read(0, 0).iter().map(|v| [v[0], v[1], v[2]]).collect();
        if positions.len() != r.vertices {
            return Err(Error::Format("surface positions".into()));
        }
        let normals = read(3, 0).iter().map(|v| [v[0], v[1], v[2]]).collect();
        let mut uvs: Vec<[f32; 2]> = read(5, 0).iter().map(|v| [v[0], v[1]]).collect();
        uvs.resize(r.vertices, [0.0, 0.0]);
        let n = r.vertices as u32;
        let triangles = if r.prim == 4 {
            indices.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect()
        } else {
            let mut t = Vec::with_capacity(indices.len());
            for (k, w) in indices.windows(3).enumerate() {
                let (a, b, c) = (w[0], w[1], w[2]);
                if a == b || b == c || a == c {
                    continue;
                }
                t.push(if k % 2 == 0 { [a, b, c] } else { [a, c, b] });
            }
            t
        };
        let triangles = triangles.into_iter().filter(|t| t.iter().all(|&i| i < n)).collect();
        groups.push(Group {
            skinned: r.skinned,
            positions,
            normals,
            uvs,
            triangles,
        });
    }
    Ok(groups)
}
