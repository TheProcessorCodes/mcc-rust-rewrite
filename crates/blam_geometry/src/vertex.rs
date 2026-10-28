//! Vertex formats of MCC Halo 3 render geometry and how to unpack them.
//! Follows `docs/format/sbsp-render-geometry.md` sections 5 and 6.

use glam::Vec3;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VertexFormat {
    /// Float positions and texcoords: BSP clusters.
    World,
    /// Positions and texcoords packed into 16 bits per axis and expanded with
    /// a bounding box: BSP instances and rigid model parts.
    Rigid,
    /// Rigid plus bone indices and weights.
    Skinned,
    Decorator,
    TinyPosition,
}

impl VertexFormat {
    pub fn from_id(id: u8) -> Option<VertexFormat> {
        Some(match id {
            0x00 => VertexFormat::World,
            0x01 => VertexFormat::Rigid,
            0x02 => VertexFormat::Skinned,
            0x0F => VertexFormat::Decorator,
            0x10 => VertexFormat::TinyPosition,
            _ => return None,
        })
    }

    /// Bytes per vertex in stream 0.
    pub fn stride(self) -> usize {
        match self {
            VertexFormat::World => 44,
            VertexFormat::Rigid => 36,
            VertexFormat::Skinned => 44,
            VertexFormat::Decorator => 20,
            VertexFormat::TinyPosition => 8,
        }
    }

    /// Whether positions and texcoords need a bounding box to expand.
    pub fn compressed(self) -> bool {
        self != VertexFormat::World
    }
}

/// The box that packed positions and texcoords are relative to.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Bounds {
    pub x: [f32; 2],
    pub y: [f32; 2],
    pub z: [f32; 2],
    pub u: [f32; 2],
    pub v: [f32; 2],
}

impl Bounds {
    pub fn is_empty(&self) -> bool {
        self.x[0] == self.x[1] && self.y[0] == self.y[1] && self.z[0] == self.z[1]
    }
    fn expand(&self, p: Vec3) -> Vec3 {
        Vec3::new(
            self.x[0] + p.x * (self.x[1] - self.x[0]),
            self.y[0] + p.y * (self.y[1] - self.y[0]),
            self.z[0] + p.z * (self.z[1] - self.z[0]),
        )
    }
    fn expand_uv(&self, t: [f32; 2]) -> [f32; 2] {
        [
            self.u[0] + t[0] * (self.u[1] - self.u[0]),
            self.v[0] + t[1] * (self.v[1] - self.v[0]),
        ]
    }
}

#[derive(Clone, Debug, Default)]
pub struct Vertices {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<[f32; 2]>,
    /// Bone indices per vertex, only for the skinned format.
    pub joints: Vec<[u16; 4]>,
    /// Bone weights per vertex, summing to one, only for the skinned format.
    pub weights: Vec<[f32; 4]>,
}

impl Vertices {
    pub fn is_skinned(&self) -> bool {
        !self.joints.is_empty()
    }
}

fn f32_at(b: &[u8], o: usize) -> f32 {
    f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
fn u16n(b: &[u8], o: usize) -> f32 {
    u16::from_le_bytes([b[o], b[o + 1]]) as f32 / 65535.0
}
fn i16n(b: &[u8], o: usize) -> f32 {
    (i16::from_le_bytes([b[o], b[o + 1]]) as f32 / 32767.0).clamp(-1.0, 1.0)
}

/// Unpacks `count` vertices of `format` from `bytes`. `bounds` is required
/// for the compressed formats; without it their positions stay in 0..1.
pub fn decode(
    format: VertexFormat,
    bytes: &[u8],
    count: usize,
    bounds: Option<&Bounds>,
) -> Result<Vertices> {
    let stride = format.stride();
    if bytes.len() < count * stride {
        return Err(Error::Invalid(format!(
            "vertex buffer holds {} bytes, {count} vertices of {format:?} need {}",
            bytes.len(),
            count * stride
        )));
    }
    let bounds = bounds.filter(|b| format.compressed() && !b.is_empty());
    let skinned = format == VertexFormat::Skinned;
    let mut out = Vertices {
        positions: Vec::with_capacity(count),
        normals: Vec::with_capacity(count),
        uvs: Vec::with_capacity(count),
        joints: Vec::with_capacity(if skinned { count } else { 0 }),
        weights: Vec::with_capacity(if skinned { count } else { 0 }),
    };
    for i in 0..count {
        let v = &bytes[i * stride..(i + 1) * stride];
        match format {
            VertexFormat::World => {
                out.positions
                    .push(Vec3::new(f32_at(v, 0), f32_at(v, 4), f32_at(v, 8)));
                out.uvs.push([f32_at(v, 12), f32_at(v, 16)]);
                out.normals
                    .push(Vec3::new(i16n(v, 20), i16n(v, 22), i16n(v, 24)));
            }
            VertexFormat::Rigid | VertexFormat::Skinned | VertexFormat::Decorator => {
                let p = Vec3::new(u16n(v, 0), u16n(v, 2), u16n(v, 4));
                let t = [u16n(v, 8), u16n(v, 10)];
                let n = Vec3::new(i16n(v, 12), i16n(v, 14), i16n(v, 16));
                match bounds {
                    Some(b) => {
                        out.positions.push(b.expand(p));
                        out.uvs.push(b.expand_uv(t));
                    }
                    None => {
                        out.positions.push(p);
                        out.uvs.push(t);
                    }
                }
                out.normals.push(n);
                if skinned {
                    out.joints
                        .push([v[0x24] as u16, v[0x25] as u16, v[0x26] as u16, v[0x27] as u16]);
                    let w = [
                        v[0x28] as f32 / 255.0,
                        v[0x29] as f32 / 255.0,
                        v[0x2A] as f32 / 255.0,
                        v[0x2B] as f32 / 255.0,
                    ];
                    let sum: f32 = w.iter().sum();
                    out.weights.push(if sum > 0.0 { w.map(|x| x / sum) } else { [1.0, 0.0, 0.0, 0.0] });
                }
            }
            VertexFormat::TinyPosition => {
                let p = Vec3::new(u16n(v, 0), u16n(v, 2), u16n(v, 4));
                out.positions
                    .push(bounds.map_or(p, |b| b.expand(p)));
                out.uvs.push([0.0, 0.0]);
                out.normals.push(Vec3::Z);
            }
        }
    }
    Ok(out)
}

/// Triangle list or strip: how an index buffer's indices make triangles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexLayout {
    List,
    Strip,
}

impl IndexLayout {
    /// From the resource's index-format number: 3 is a list, 5 (and 0) a strip.
    pub fn from_id(id: i32) -> Option<IndexLayout> {
        match id {
            3 => Some(IndexLayout::List),
            0 | 5 => Some(IndexLayout::Strip),
            _ => None,
        }
    }

    /// Turns a run of indices into a flat triangle list.
    pub fn triangles(self, indices: &[u32]) -> Vec<u32> {
        match self {
            IndexLayout::List => indices[..indices.len() - indices.len() % 3].to_vec(),
            IndexLayout::Strip => unstrip(indices),
        }
    }
}

/// Unrolls a triangle strip, dropping the degenerate stitch triangles and
/// flipping every other one so the winding stays consistent.
pub fn unstrip(indices: &[u32]) -> Vec<u32> {
    let mut out = Vec::with_capacity(indices.len().saturating_sub(2) * 3);
    for (n, w) in indices.windows(3).enumerate() {
        let (a, b, c) = (w[0], w[1], w[2]);
        if a == b || a == c || b == c {
            continue;
        }
        if n % 2 == 0 {
            out.extend([a, b, c]);
        } else {
            out.extend([a, c, b]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_unrolls_with_alternating_winding() {
        assert_eq!(unstrip(&[0, 1, 2, 3, 4]), vec![0, 1, 2, 1, 3, 2, 2, 3, 4]);
        // A repeated index is a stitch and makes no triangle.
        assert_eq!(unstrip(&[0, 1, 2, 2, 3]), vec![0, 1, 2]);
    }

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
        v[0..2].copy_from_slice(&65535u16.to_le_bytes()); // x = 1
        v[2..4].copy_from_slice(&0u16.to_le_bytes()); // y = 0
        v[8..10].copy_from_slice(&32768u16.to_le_bytes()); // u ~ 0.5
        v[12..14].copy_from_slice(&32767i16.to_le_bytes()); // nx = 1
        let got = decode(VertexFormat::Rigid, &v, 1, Some(&bounds)).unwrap();
        assert_eq!(got.positions[0].x, 1.0);
        assert_eq!(got.positions[0].y, 0.0);
        assert!((got.uvs[0][0] - 1.0).abs() < 1e-3);
        assert_eq!(got.normals[0], Vec3::X);
    }
}
