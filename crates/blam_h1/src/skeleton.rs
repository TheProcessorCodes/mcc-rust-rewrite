//! A gbxmodel's (`mod2`) nodes and markers: the skeleton actors are drawn
//! on and the frames weapons hang from (`docs/format/halo1.md` 9).
//!
//! Halo CE stores its rotations as the inverse of the usual quaternion:
//! read as stored, the pelvis's child `bip01 spine` (+0.038 along its
//! parent's X) would sit beside the pelvis; conjugated it sits above it,
//! and the thighs to either side. So every rotation here is conjugated
//! (checked on the Flood combat forms, the grunt and the sentinel).

use glam::{Mat4, Quat, Vec3};

use crate::cache::Map;
use crate::{Error, Result};

/// One node, relative to its parent at the default pose.
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub parent: Option<u16>,
    pub translation: Vec3,
    pub rotation: Quat,
}

/// One marker instance: a frame on a node.
#[derive(Clone, Debug)]
pub struct Marker {
    /// As stored, with spaces (`right hand`, `primary trigger`).
    pub name: String,
    pub node: u16,
    pub region: i8,
    pub permutation: i8,
    pub translation: Vec3,
    pub rotation: Quat,
}

/// A model's nodes in order (mod2 `+0xB8`, 0x9C each: name, next sibling
/// i16 +0x20, first child +0x22, parent +0x24, translation +0x28, rotation
/// +0x34 as i, j, k, w).
pub fn nodes(map: &Map, mode: u32) -> Result<Vec<Node>> {
    let r = map
        .tag_data(mode)
        .ok_or_else(|| Error::Invalid(format!("{}: no data", map.tag_name(mode).unwrap_or("?"))))?;
    let mut out = Vec::new();
    for n in r.block(0xB8, 0x9C)? {
        let q = [n.f32(0x34)?, n.f32(0x38)?, n.f32(0x3C)?, n.f32(0x40)?];
        out.push(Node {
            name: n.str(0, 32),
            parent: u16::try_from(n.i16(0x24)?).ok(),
            translation: n.vec3(0x28)?,
            rotation: stored_rotation(q),
        });
    }
    Ok(out)
}

/// A model's markers (mod2 `+0xAC`, 0x40 each: name, instances block
/// +0x34 of 0x20: region i8, permutation i8, node i8, translation +4,
/// rotation +0x10).
pub fn markers(map: &Map, mode: u32) -> Result<Vec<Marker>> {
    let r = map
        .tag_data(mode)
        .ok_or_else(|| Error::Invalid(format!("{}: no data", map.tag_name(mode).unwrap_or("?"))))?;
    let mut out = Vec::new();
    for m in r.block(0xAC, 0x40)? {
        let name = m.str(0, 32);
        for i in m.block(0x34, 0x20)? {
            let q = [i.f32(0x10)?, i.f32(0x14)?, i.f32(0x18)?, i.f32(0x1C)?];
            out.push(Marker {
                name: name.clone(),
                region: i.i8(0)?,
                permutation: i.i8(1)?,
                node: u16::from(i.u8(2)?),
                translation: i.vec3(4)?,
                rotation: stored_rotation(q),
            });
        }
    }
    Ok(out)
}

/// A stored Halo CE rotation (i, j, k, w) as a usual quaternion: its
/// conjugate, normalised (identity when degenerate).
pub fn stored_rotation(q: [f32; 4]) -> Quat {
    let q = Quat::from_xyzw(-q[0], -q[1], -q[2], q[3]);
    if q.length_squared() > 1e-6 { q.normalize() } else { Quat::IDENTITY }
}

/// Each node's frame in model space at the default pose.
pub fn model_frames(nodes: &[Node]) -> Vec<Mat4> {
    let mut out: Vec<Mat4> = Vec::with_capacity(nodes.len());
    for (i, n) in nodes.iter().enumerate() {
        let local = Mat4::from_rotation_translation(n.rotation, n.translation);
        let parent = n.parent.map(usize::from).filter(|p| *p < i).map_or(Mat4::IDENTITY, |p| out[p]);
        out.push(parent * local);
    }
    out
}
