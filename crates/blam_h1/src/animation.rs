//! Halo CE animation graphs (`antr`): the nodes and the animations, each
//! decoded to a pose per frame (`docs/format/halo1.md` 15), for the
//! actors' skeletons (`skeleton.rs`; the graph's nodes are the model's, in
//! the same order).
//!
//! Each animation keeps the nodes it moves in its frame data, frame after
//! frame, node after node: a rotation (four i16 over 32767, stored
//! conjugated like the model's) where the rotation flag is set, a
//! translation (three f32) where the translation flag is, a scale (f32)
//! where the scale flag is. What a node does not move is in the default
//! data, once, laid out the same way for the flags that are clear. The
//! root's motion over the ground is the frame info: per frame dx, dy (type
//! 1), and dyaw (type 2), or dx, dy, dz, dyaw (type 3), world units and
//! radians. Compressed animations are not read (none on `d40`'s actors).

use glam::{Quat, Vec3};

use crate::cache::{Map, Reader};
use crate::skeleton::stored_rotation;
use crate::{Error, Result};

/// One node's pose: relative to its parent, world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodePose {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: f32,
}

/// One animation, decoded.
#[derive(Clone, Debug)]
pub struct Animation {
    /// `stand pistol move-front`, `alert unarmed idle%1`, ...
    pub name: String,
    /// 0 base, 1 overlay, 2 replacement.
    pub kind: u16,
    pub frame_count: usize,
    pub node_count: usize,
    pub loop_frame: usize,
    /// `frames[f * node_count + n]`.
    pub frames: Vec<NodePose>,
    /// The root's motion per frame: dx, dy, dz (world units), dyaw.
    pub root_motion: Vec<[f32; 4]>,
}

impl Animation {
    pub fn pose(&self, frame: usize) -> &[NodePose] {
        let f = frame.min(self.frame_count.saturating_sub(1));
        &self.frames[f * self.node_count..(f + 1) * self.node_count]
    }

    /// Ground speed of its root motion, world units a second at 30 frames
    /// a second.
    pub fn root_speed(&self) -> f32 {
        if self.root_motion.is_empty() {
            return 0.0;
        }
        let d: f32 = self.root_motion.iter().map(|m| (m[0] * m[0] + m[1] * m[1]).sqrt()).sum();
        d / self.root_motion.len() as f32 * 30.0
    }
}

/// An animation graph.
#[derive(Clone, Debug, Default)]
pub struct Graph {
    pub nodes: Vec<String>,
    /// `None` for an animation not read (compressed, or broken).
    pub animations: Vec<Option<Animation>>,
}

impl Graph {
    /// The first base animation whose name, without its `%N` variant
    /// suffix, is `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.animations.iter().position(|a| {
            a.as_ref()
                .is_some_and(|a| a.name.split('%').next().unwrap_or(&a.name).eq_ignore_ascii_case(name))
        })
    }
}

/// A 20-byte data field's bytes: size (+0), address (+0xC).
fn data_field<'a>(map: &'a Map, r: &Reader<'_>, off: usize) -> Result<&'a [u8]> {
    let size = r.u32(off)? as usize;
    if size == 0 {
        return Ok(&[]);
    }
    let address = r.u32(off + 0xC)?;
    let at = map
        .space
        .offset(address)
        .ok_or_else(|| Error::Invalid(format!("animation data at {address:#x} is outside the tag data")))?;
    map.slice(at, size)
}

fn bits(r: &Reader<'_>, off: usize) -> Result<u64> {
    Ok(u64::from(r.u32(off)?) | (u64::from(r.u32(off + 4)?) << 32))
}

/// Reads an animation graph: nodes (`+0x68`, 0x40 each, the name first)
/// and animations (`+0x74`, 0xB4 each).
pub fn read(map: &Map, tag: u32) -> Result<Graph> {
    let path = map.tag_name(tag).unwrap_or("?").to_string();
    let g = map.tag_data(tag).ok_or_else(|| Error::Invalid(format!("{path}: no data")))?;
    let nodes: Vec<String> = g.block(0x68, 0x40)?.iter().map(|n| n.str(0, 32)).collect();
    let mut animations = Vec::new();
    for a in g.block(0x74, 0xB4)? {
        animations.push(animation(map, &a).ok().flatten());
    }
    Ok(Graph { nodes, animations })
}

/// One animation (0xB4): name, type (u16 +0x20), frame count (+0x22),
/// frame size (+0x24), frame info type (+0x26), node count (+0x2C), loop
/// frame (+0x2E), frame info (data +0x48), translation, rotation and scale
/// flags (two u32 each at +0x5C, +0x6C, +0x7C), offset to compressed data
/// (+0x88), default data (+0x8C), frame data (+0xA0).
fn animation(map: &Map, a: &Reader<'_>) -> Result<Option<Animation>> {
    let name = a.str(0, 32);
    let kind = a.u16(0x20)?;
    let frame_count = a.u16(0x22)? as usize;
    let frame_size = a.u16(0x24)? as usize;
    let info_type = a.u16(0x26)?;
    let node_count = a.u16(0x2C)? as usize;
    let loop_frame = a.u16(0x2E)? as usize;
    if a.u32(0x88)? != 0 || node_count == 0 || node_count > 64 || frame_count == 0 {
        return Ok(None);
    }
    let (trans, rot, scale) = (bits(a, 0x5C)?, bits(a, 0x6C)?, bits(a, 0x7C)?);
    let has = |m: u64, n: usize| m >> n & 1 != 0;
    let size: usize = (0..node_count)
        .map(|n| 8 * has(rot, n) as usize + 12 * has(trans, n) as usize + 4 * has(scale, n) as usize)
        .sum();
    if size != frame_size {
        return Err(Error::Invalid(format!("{name}: frame size {frame_size}, flags say {size}")));
    }
    let defaults = data_field(map, a, 0x8C)?;
    let data = data_field(map, a, 0xA0)?;
    if data.len() < frame_size * frame_count {
        return Err(Error::Invalid(format!("{name}: frame data short")));
    }
    let f32_at = |b: &[u8], o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
    let q_at = |b: &[u8], o: usize| {
        let c = |k: usize| i16::from_le_bytes([b[o + 2 * k], b[o + 2 * k + 1]]) as f32 / 32767.0;
        stored_rotation([c(0), c(1), c(2), c(3)])
    };
    // The nodes' still parts, from the default data.
    let mut still = vec![
        NodePose {
            rotation: Quat::IDENTITY,
            translation: Vec3::ZERO,
            scale: 1.0
        };
        node_count
    ];
    let mut o = 0;
    for (n, p) in still.iter_mut().enumerate() {
        if !has(rot, n) && o + 8 <= defaults.len() {
            p.rotation = q_at(defaults, o);
            o += 8;
        }
        if !has(trans, n) && o + 12 <= defaults.len() {
            p.translation = Vec3::new(f32_at(defaults, o), f32_at(defaults, o + 4), f32_at(defaults, o + 8));
            o += 12;
        }
        if !has(scale, n) && o + 4 <= defaults.len() {
            p.scale = f32_at(defaults, o);
            o += 4;
        }
    }
    let mut frames = Vec::with_capacity(frame_count * node_count);
    for f in 0..frame_count {
        let mut o = f * frame_size;
        for (n, s) in still.iter().enumerate() {
            let mut p = *s;
            if has(rot, n) {
                p.rotation = q_at(data, o);
                o += 8;
            }
            if has(trans, n) {
                p.translation = Vec3::new(f32_at(data, o), f32_at(data, o + 4), f32_at(data, o + 8));
                o += 12;
            }
            if has(scale, n) {
                p.scale = f32_at(data, o);
                o += 4;
            }
            frames.push(p);
        }
    }
    let info = data_field(map, a, 0x48)?;
    let stride = match info_type {
        1 => 8,
        2 => 12,
        3 => 16,
        _ => 0,
    };
    let mut root_motion = Vec::new();
    if stride > 0 {
        for k in 0..(info.len() / stride).min(frame_count) {
            let v = |i: usize| f32_at(info, k * stride + 4 * i);
            root_motion.push(match info_type {
                1 => [v(0), v(1), 0.0, 0.0],
                2 => [v(0), v(1), 0.0, v(2)],
                _ => [v(0), v(1), v(2), v(3)],
            });
        }
    }
    Ok(Some(Animation {
        name,
        kind,
        frame_count,
        node_count,
        loop_frame,
        frames,
        root_motion,
    }))
}
