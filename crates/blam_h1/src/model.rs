//! Halo CE gbxmodels (`mod2`): per region, the first permutation at its
//! most detailed level, each part a triangle strip over its own vertices.
//! Follows `docs/format/halo1.md` section 9.
//!
//! Vertices are in model space at the default pose (a wheel's vertices sit
//! where the wheel is), so no node transforms are applied. Each mesh also
//! carries its vertices' two nodes and weights (`Mesh::skin`, model node
//! indices) for the actors' skeletons (`skeleton.rs`). Strips are wound
//! clockwise, like the BSP's triangles, and are flipped here.

use glam::Vec3;

use blam_geometry::{Mesh, Part};

use crate::cache::Map;
use crate::{Error, Result};

/// Bytes per uncompressed model vertex: position, normal, binormal,
/// tangent, texcoord, two node indices and two weights.
const VERTEX: usize = 68;

/// A model's meshes, one per region; `material` maps the model's shader
/// index (its shader block `+0xDC`, 0x20 each) to the caller's material.
pub fn meshes(map: &Map, mode: u32, material: &mut dyn FnMut(u32) -> u32) -> Result<Vec<Mesh>> {
    let name = map.tag_name(mode).unwrap_or("?").to_string();
    let r = map.tag_data(mode).ok_or_else(|| Error::Invalid(format!("{name}: no data")))?;
    // The base map scale (`+0x30`, `+0x34`) stretches every texcoord.
    let us = r.f32(0x30)?;
    let vs = r.f32(0x34)?;
    let (us, vs) = (if us == 0.0 { 1.0 } else { us }, if vs == 0.0 { 1.0 } else { vs });
    let shaders: Vec<u32> = r.block(0xDC, 0x20)?.into_iter().map(|s| s.tag_ref(0)).collect::<Result<_>>()?;
    let geometries = r.block(0xD0, 0x30)?;
    let (vertex_area, vertex_size) = map.model_vertices;
    let index_area = vertex_area + vertex_size;
    let short = name.rsplit('\\').next().unwrap_or(&name).to_string();
    let mut out = Vec::new();
    for region in r.block(0xC4, 0x4C)? {
        let Some(perm) = region.block(0x40, 0x58)?.into_iter().next() else { continue };
        // Levels of detail, super low first (`+0x40` .. `+0x48`): the most
        // detailed one present.
        let mut geometry = None;
        for k in (0..5).rev() {
            let g = perm.i16(0x40 + 2 * k)?;
            if g >= 0 {
                geometry = Some(g as usize);
                break;
            }
        }
        let Some(g) = geometry.and_then(|g| geometries.get(g)) else { continue };
        let mut mesh = Mesh {
            name: format!("{short} {}", region.str(0, 32)),
            ..Default::default()
        };
        for p in g.block(0x24, 0x84)? {
            let shader = usize::try_from(p.i16(4)?).ok().and_then(|k| shaders.get(k)).copied().unwrap_or(u32::MAX);
            let index_count = p.u32(0x48)? as usize + 2;
            let index_at = index_area + p.u32(0x4C)? as usize;
            let vertex_count = p.u32(0x58)? as usize;
            let vertex_at = vertex_area + p.u32(0x64)? as usize;
            if vertex_count == 0 || index_count < 3 || vertex_count > 65536 {
                continue;
            }
            let base = mesh.positions.len() as u32;
            let vb = map.slice(vertex_at, vertex_count * VERTEX)?;
            let f = |o: usize| f32::from_le_bytes(vb[o..o + 4].try_into().unwrap());
            // Parts flagged 0x2 index their nodes through a table of their
            // own (`+0x6C`, up to 22 bytes); the others index the model's.
            let local = p.u32(0)? & 2 != 0;
            let table: Vec<u8> = (0..22).map(|k| p.u8(0x6C + k)).collect::<Result<_>>()?;
            let node = |i: i16| -> u16 {
                match (local, usize::try_from(i).ok()) {
                    (_, None) => 0,
                    (true, Some(k)) => table.get(k).copied().map_or(0, u16::from),
                    (false, Some(k)) => k as u16,
                }
            };
            let skin = mesh.skin.get_or_insert_with(Default::default);
            for v in 0..vertex_count {
                let o = v * VERTEX;
                mesh.positions.push(Vec3::new(f(o), f(o + 4), f(o + 8)));
                mesh.normals.push(Vec3::new(f(o + 12), f(o + 16), f(o + 20)));
                mesh.uvs.push([f(o + 48) * us, f(o + 52) * vs]);
                // Two nodes and their weights (+56, +58, +60, +64).
                let n0 = i16::from_le_bytes([vb[o + 56], vb[o + 57]]);
                let n1 = i16::from_le_bytes([vb[o + 58], vb[o + 59]]);
                let (w0, w1) = (f(o + 60), if n1 >= 0 { f(o + 64) } else { 0.0 });
                let sum = w0 + w1;
                let (w0, w1) = if sum > 1e-4 { (w0 / sum, w1 / sum) } else { (1.0, 0.0) };
                skin.joints.push([node(n0), node(n1), 0, 0]);
                skin.weights.push([w0, w1, 0.0, 0.0]);
            }
            let ib = map.slice(index_at, index_count * 2)?;
            let strip: Vec<u32> = ib
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b))
                .filter(|&i| i != 0xFFFF)
                .map(u32::from)
                .collect();
            let mut indices = Vec::with_capacity(strip.len() * 3);
            for k in 0..strip.len().saturating_sub(2) {
                let [a, b, c] = [strip[k], strip[k + 1], strip[k + 2]];
                if a == b || b == c || a == c || a as usize >= vertex_count || b as usize >= vertex_count || c as usize >= vertex_count {
                    continue;
                }
                // Clockwise as stored: (a, c, b) on even triangles, the
                // strip's own flip on odd ones.
                let t = if k % 2 == 0 { [a, c, b] } else { [a, b, c] };
                indices.extend(t.map(|i| i + base));
            }
            if !indices.is_empty() {
                mesh.parts.push(Part {
                    material: material(shader),
                    indices,
                });
            }
        }
        if !mesh.parts.is_empty() {
            out.push(mesh);
        }
    }
    Ok(out)
}
