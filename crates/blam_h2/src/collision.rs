//! Halo 2 collision BSPs: the structure BSP's own (sbsp `+0x14`) and each
//! instanced geometry definition's (`+0x70`), walked as winged-edge loops
//! into triangles. Follows `docs/format/halo2.md` section 6.
//!
//! The layout is Halo 3's normal collision BSP with 16-bit fields and an
//! 8-byte surface; every loop is wound counter-clockwise seen from the side
//! you stand on, so a fan from its first vertex faces out of the solid.

use glam::{Mat4, Vec3};

use blam_geometry::{CollisionSource, CollisionTriangle};

use crate::cache::Reader;
use crate::Result;

/// The surface flags (the same bits as Halo 3's).
pub mod surface_flags {
    pub const TWO_SIDED: u8 = 1 << 0;
    pub const INVISIBLE: u8 = 1 << 1;
    pub const CLIMBABLE: u8 = 1 << 2;
    pub const BREAKABLE: u8 = 1 << 3;
    pub const INVALID: u8 = 1 << 4;
}

/// Counts from one collision BSP.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stats {
    pub surfaces: usize,
    pub triangles: usize,
    pub corrupt_loops: usize,
}

/// Triangulates the collision BSP whose nine blocks start at `at`
/// (3D nodes `+0x00` ... surfaces `+0x28`, edges `+0x30`, vertices `+0x38`).
/// `material_base` is added to each surface's material index.
pub fn triangulate(
    at: Reader<'_>,
    transform: Option<(Mat4, bool)>,
    source: CollisionSource,
    material_base: i16,
    out: &mut Vec<CollisionTriangle>,
) -> Result<Stats> {
    let surfaces = at.block(0x28, 8)?;
    let edges = at.block(0x30, 12)?;
    let vertices = at.block(0x38, 16)?;
    let mut stats = Stats {
        surfaces: surfaces.len(),
        ..Default::default()
    };
    let edge = |e: usize| -> Result<[usize; 6]> {
        let r = &edges[e];
        Ok([r.u16(0)?, r.u16(2)?, r.u16(4)?, r.u16(6)?, r.u16(8)?, r.u16(10)?].map(|v| v as usize))
    };
    let mut points: Vec<Vec3> = Vec::with_capacity(8);
    for (s, sr) in surfaces.iter().enumerate() {
        let first = sr.u16(2)? as usize;
        let flags = sr.u8(4)?;
        let material = sr.i16(6)?;
        if flags & surface_flags::INVALID != 0 {
            continue;
        }
        points.clear();
        let mut e = first;
        let mut corrupt = false;
        for _ in 0..edges.len().max(1) {
            if e >= edges.len() {
                corrupt = true;
                break;
            }
            let [start, end, forward, reverse, left, right] = edge(e)?;
            let (v, next) = if left == s {
                (start, forward)
            } else if right == s {
                (end, reverse)
            } else {
                corrupt = true;
                break;
            };
            let Some(vr) = vertices.get(v) else {
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
        let (m, mirrored) = transform.unwrap_or((Mat4::IDENTITY, false));
        if transform.is_some() {
            for p in &mut points {
                *p = m.transform_point3(*p);
            }
        }
        for k in 1..points.len() - 1 {
            let (b, c) = if mirrored { (k + 1, k) } else { (k, k + 1) };
            out.push(CollisionTriangle {
                points: [points[0], points[b], points[c]],
                material: if material < 0 { -1 } else { material + material_base },
                flags,
                source,
            });
            stats.triangles += 1;
        }
    }
    Ok(stats)
}
