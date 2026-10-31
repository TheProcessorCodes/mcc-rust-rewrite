//! Halo CE collision BSPs: the structure BSP's own (sbsp `+0xB0`), walked
//! as winged-edge loops into triangles. Follows `docs/format/halo1.md`
//! section 7.
//!
//! The layout is Halo 2's with 32-bit fields: every loop is wound
//! counter-clockwise seen from the side you stand on (checked against the
//! surfaces' planes on every map), so a fan from its first vertex faces out.

use glam::Vec3;

use blam_geometry::{CollisionSource, CollisionTriangle};

use crate::cache::Reader;
use crate::Result;

/// The surface flags (the same bits as Halo 2's and Halo 3's).
pub mod surface_flags {
    pub const TWO_SIDED: u8 = 1 << 0;
    pub const INVISIBLE: u8 = 1 << 1;
    pub const CLIMBABLE: u8 = 1 << 2;
    pub const BREAKABLE: u8 = 1 << 3;
}

/// Counts from one collision BSP.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub surfaces: usize,
    pub triangles: usize,
    pub corrupt_loops: usize,
}

/// Triangulates the collision BSP at `at` (eight blocks of 12 bytes:
/// 3D nodes, planes, leaves, 2D references, 2D nodes, surfaces `+0x3C`,
/// edges `+0x48`, vertices `+0x54`). `material_base` is added to each
/// surface's material index.
pub fn triangulate(at: Reader<'_>, source: CollisionSource, material_base: i16, out: &mut Vec<CollisionTriangle>) -> Result<Stats> {
    let surfaces = at.block(0x3C, 12)?;
    let edges = at.block(0x48, 24)?;
    let vertices = at.block(0x54, 16)?;
    let mut stats = Stats {
        surfaces: surfaces.len(),
        ..Default::default()
    };
    // Edge: start and end vertex, forward and reverse edge, left and right
    // surface.
    let edge = |e: usize| -> Result<[i32; 6]> {
        let r = &edges[e];
        Ok([r.i32(0)?, r.i32(4)?, r.i32(8)?, r.i32(12)?, r.i32(16)?, r.i32(20)?])
    };
    let mut points: Vec<Vec3> = Vec::with_capacity(8);
    for (s, sr) in surfaces.iter().enumerate() {
        let first = sr.i32(4)?;
        let flags = sr.u8(8)?;
        let material = sr.i16(10)?;
        points.clear();
        let mut e = first;
        let mut corrupt = false;
        for _ in 0..edges.len().max(1) {
            let Some(ei) = usize::try_from(e).ok().filter(|&e| e < edges.len()) else {
                corrupt = true;
                break;
            };
            let [start, end, forward, reverse, left, right] = edge(ei)?;
            let (v, next) = if left == s as i32 {
                (start, forward)
            } else if right == s as i32 {
                (end, reverse)
            } else {
                corrupt = true;
                break;
            };
            let Some(vr) = usize::try_from(v).ok().and_then(|v| vertices.get(v)) else {
                corrupt = true;
                break;
            };
            points.push(vr.vec3(0)?);
            if next == first {
                break;
            }
            e = next;
        }
        if corrupt || points.len() < 3 {
            stats.corrupt_loops += 1;
            continue;
        }
        for k in 1..points.len() - 1 {
            out.push(CollisionTriangle {
                points: [points[0], points[k], points[k + 1]],
                material: if material < 0 { -1 } else { material + material_base },
                flags,
                source,
            });
            stats.triangles += 1;
        }
    }
    Ok(stats)
}
