//! An animation's data: found through its resource group, then decoded
//! from the codec blocks into per-node tracks. `docs/format/animation.md`
//! sections 6 to 9.
//!
//! The data of one animation is, in order: the static codec block (values
//! that never change, one per track), the animated codec block, the static
//! node flags, the animated node flags, then the movement ("frame info").
//! Each flags region is three bit arrays (rotation, translation, scale) of
//! `ceil(nodes / 32)` little-endian u32s; the n-th set bit of an array
//! says which node the codec's n-th track of that kind drives.

use glam::{Quat, Vec3};

use blam_cache::{Resources, ResourceData};

use crate::graph::{AnimKind, Graph, MovementKind};
use crate::{Error, Result};

const MEMBER_SIZE: usize = 48;

/// Codec ids seen in MCC Halo 3 (`docs/format/animation.md` section 8).
pub mod codec_id {
    /// One value per track, rotations as four i16.
    pub const STATIC: u8 = 1;
    /// Every frame of every track; rotations as four i16. Never seen, read
    /// like [`QUANTIZED_ROTATIONS`].
    pub const UNCOMPRESSED: u8 = 2;
    /// Every frame of every track; rotations as four i16.
    pub const QUANTIZED_ROTATIONS: u8 = 3;
    /// Keyframes with u8 frame numbers.
    pub const BYTE_KEYFRAMES: u8 = 4;
    /// Keyframes with u16 frame numbers.
    pub const WORD_KEYFRAMES: u8 = 5;
    /// As [`BYTE_KEYFRAMES`], tracks stored last node first.
    pub const REVERSE_BYTE_KEYFRAMES: u8 = 6;
    /// As [`WORD_KEYFRAMES`], tracks stored last node first.
    pub const REVERSE_WORD_KEYFRAMES: u8 = 7;
    /// Every frame of every track; rotations as four f32.
    pub const FULL_ROTATIONS: u8 = 8;
}

/// A value over time.
#[derive(Clone, Debug, Default)]
pub enum Track<T> {
    /// The animation does not drive this channel of the node.
    #[default]
    None,
    Constant(T),
    /// One value per frame.
    Frames(Vec<T>),
    /// Values at some frames, interpolated in between; `frames` ascend.
    Keys { frames: Vec<u16>, values: Vec<T> },
}

pub trait Lerp: Copy {
    fn lerp_to(self, other: Self, t: f32) -> Self;
}
impl Lerp for Quat {
    fn lerp_to(self, other: Self, t: f32) -> Self {
        // Shortest way round, normalised linear blend (what a game does
        // between neighbouring frames; close to slerp for small steps).
        let o = if self.dot(other) < 0.0 { -other } else { other };
        self.lerp(o, t).normalize()
    }
}
impl Lerp for Vec3 {
    fn lerp_to(self, other: Self, t: f32) -> Self {
        self.lerp(other, t)
    }
}
impl Lerp for f32 {
    fn lerp_to(self, other: Self, t: f32) -> Self {
        self + (other - self) * t
    }
}

impl<T: Lerp> Track<T> {
    pub fn is_none(&self) -> bool {
        matches!(self, Track::None)
    }

    /// The value at frame `f` (fractional), clamped to the track's ends.
    pub fn sample(&self, f: f32) -> Option<T> {
        match self {
            Track::None => None,
            Track::Constant(v) => Some(*v),
            Track::Frames(v) => {
                if v.is_empty() {
                    return None;
                }
                let f = f.clamp(0.0, (v.len() - 1) as f32);
                let i = f.floor() as usize;
                let j = (i + 1).min(v.len() - 1);
                Some(v[i].lerp_to(v[j], f - i as f32))
            }
            Track::Keys { frames, values } => {
                if values.is_empty() {
                    return None;
                }
                let k = frames.partition_point(|&x| (x as f32) <= f);
                if k == 0 {
                    return Some(values[0]);
                }
                if k >= frames.len() {
                    return values.last().copied();
                }
                let (f0, f1) = (frames[k - 1] as f32, frames[k] as f32);
                let t = if f1 > f0 { (f - f0) / (f1 - f0) } else { 0.0 };
                Some(values[k - 1].lerp_to(values[k], t))
            }
        }
    }
}

/// The three channels of one node.
#[derive(Clone, Debug, Default)]
pub struct NodeTrack {
    pub rotation: Track<Quat>,
    pub translation: Track<Vec3>,
    pub scale: Track<f32>,
}

/// One frame of root motion: how far the object moves (world units, its
/// own frame: x forward, y left, z up) and turns (radians, left positive)
/// going from this frame to the next.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
    pub dyaw: f32,
}

/// A decoded animation.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub kind: AnimKind,
    pub frame_count: u16,
    pub loop_frame: i16,
    /// Per graph node.
    pub tracks: Vec<NodeTrack>,
    pub movement_kind: MovementKind,
    /// One entry per frame (empty when the animation has none).
    pub movement: Vec<Motion>,
    /// Codec ids of the static and animated blocks.
    pub static_codec: Option<u8>,
    pub animated_codec: Option<u8>,
}

/// The sizes record of a resource group member (48 bytes).
#[derive(Clone, Copy, Debug)]
pub struct Member {
    pub checksum: u32,
    pub frame_count: u16,
    pub node_count: u8,
    pub movement: u8,
    pub static_flags_size: u8,
    pub animated_flags_size: u8,
    pub movement_size: u16,
    pub pill_size: u16,
    pub static_size: u16,
    pub uncompressed_size: u32,
    pub animated_size: u32,
    pub data_size: u32,
    pub address: u32,
}

fn le16(b: &[u8], o: usize) -> Result<u16> {
    b.get(o..o + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| Error::Invalid(format!("animation data ends before byte {}", o + 2)))
}
fn le32(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| Error::Invalid(format!("animation data ends before byte {}", o + 4)))
}
fn f32_at(b: &[u8], o: usize) -> Result<f32> {
    Ok(f32::from_bits(le32(b, o)?))
}
fn vec3_at(b: &[u8], o: usize) -> Result<Vec3> {
    Ok(Vec3::new(f32_at(b, o)?, f32_at(b, o + 4)?, f32_at(b, o + 8)?))
}
/// A rotation stored as four i16 (x, y, z, w) over 32767.
fn quat16_at(b: &[u8], o: usize) -> Result<Quat> {
    let c = |k: usize| -> Result<f32> { Ok(le16(b, o + 2 * k)? as i16 as f32 / 32767.0) };
    Ok(normalize(Quat::from_xyzw(c(0)?, c(1)?, c(2)?, c(3)?)))
}
fn quat32_at(b: &[u8], o: usize) -> Result<Quat> {
    Ok(normalize(Quat::from_xyzw(f32_at(b, o)?, f32_at(b, o + 4)?, f32_at(b, o + 8)?, f32_at(b, o + 12)?)))
}
fn normalize(q: Quat) -> Quat {
    let l = q.length();
    if l > 1e-6 { q / l } else { Quat::IDENTITY }
}

impl Member {
    fn read(def: &[u8], index: usize) -> Result<Member> {
        let o = index * MEMBER_SIZE;
        if o + MEMBER_SIZE > def.len() {
            return Err(Error::Invalid(format!(
                "member {index} is past the {}-byte group definition",
                def.len()
            )));
        }
        Ok(Member {
            checksum: le32(def, o + 4)?,
            frame_count: le16(def, o + 8)?,
            node_count: def[o + 10],
            movement: def[o + 11],
            static_flags_size: def[o + 12],
            animated_flags_size: def[o + 13],
            movement_size: le16(def, o + 14)?,
            pill_size: le16(def, o + 16)?,
            static_size: le16(def, o + 18)?,
            uncompressed_size: le32(def, o + 20)?,
            animated_size: le32(def, o + 24)?,
            data_size: le32(def, o + 28)?,
            address: le32(def, o + 40)?,
        })
    }
}

/// The raw data of one animation, with its sizes record.
pub struct RawAnimation {
    pub member: Member,
    data: ResourceData,
    start: usize,
}

impl RawAnimation {
    pub fn bytes(&self) -> &[u8] {
        let all = self.data.bytes();
        let end = (self.start + self.member.data_size as usize).min(all.len());
        &all[self.start.min(end)..end]
    }
}

/// Finds an animation's bytes: its resource group's entry, the member's
/// record in the definition, and the member's data address inside the
/// group's segment.
pub fn raw(resources: &Resources<'_>, graph: &Graph, index: usize) -> Result<RawAnimation> {
    let anim = graph
        .animations
        .get(index)
        .ok_or_else(|| Error::Invalid(format!("{} has no animation {index}", graph.name)))?;
    let group = anim
        .group
        .and_then(|g| graph.groups.get(g as usize))
        .ok_or_else(|| Error::Invalid(format!("{} has no resource group", anim.name)))?;
    let entry = resources.entry(group.resource)?;
    if entry.is_empty() || entry.salt != (group.resource.0 >> 16) as u16 {
        return Err(Error::Invalid(format!("{}: resource entry is stale", anim.name)));
    }
    let def = resources.fixed_definition(&entry)?;
    let member = Member::read(&def, anim.member as usize)?;
    let data = resources.data(&entry).map_err(|e| {
        Error::Invalid(format!("{}: data not in this map ({e})", anim.name))
    })?;
    let start = (member.address & 0x1FFF_FFFF) as usize;
    if start + member.data_size as usize > data.len() {
        return Err(Error::Invalid(format!(
            "{}: {} bytes at {start} run past the {}-byte segment",
            anim.name,
            member.data_size,
            data.len()
        )));
    }
    Ok(RawAnimation { member, data, start })
}

struct Decoded {
    codec: u8,
    rotations: Vec<Track<Quat>>,
    translations: Vec<Track<Vec3>>,
    scales: Vec<Track<f32>>,
}

/// Decodes one codec block starting at `b[0]`.
fn decode_block(b: &[u8], frames: u16) -> Result<Decoded> {
    let codec = *b.first().ok_or_else(|| Error::Invalid("empty codec block".into()))?;
    let (nr, nt, ns) = (
        *b.get(1).unwrap_or(&0) as usize,
        *b.get(2).unwrap_or(&0) as usize,
        *b.get(3).unwrap_or(&0) as usize,
    );
    let u = |o: usize| le32(b, o).map(|v| v as usize);
    let mut out = Decoded {
        codec,
        rotations: Vec::with_capacity(nr),
        translations: Vec::with_capacity(nt),
        scales: Vec::with_capacity(ns),
    };
    match codec {
        codec_id::STATIC => {
            let (t_off, s_off) = (u(12)?, u(16)?);
            for i in 0..nr {
                out.rotations.push(Track::Constant(quat16_at(b, 32 + 8 * i)?));
            }
            for i in 0..nt {
                out.translations.push(Track::Constant(vec3_at(b, t_off + 12 * i)?));
            }
            for i in 0..ns {
                out.scales.push(Track::Constant(f32_at(b, s_off + 4 * i)?));
            }
        }
        codec_id::UNCOMPRESSED | codec_id::QUANTIZED_ROTATIONS | codec_id::FULL_ROTATIONS => {
            let (t_off, s_off) = (u(12)?, u(16)?);
            let (r_stride, t_stride, s_stride) = (u(20)?, u(24)?, u(28)?);
            let r_size = if codec == codec_id::FULL_ROTATIONS { 16 } else { 8 };
            let n = |stride: usize, size: usize| (stride / size).min(frames.max(1) as usize);
            for i in 0..nr {
                let base = 32 + r_stride * i;
                let v = (0..n(r_stride, r_size))
                    .map(|f| {
                        let o = base + r_size * f;
                        if r_size == 16 { quat32_at(b, o) } else { quat16_at(b, o) }
                    })
                    .collect::<Result<Vec<_>>>()?;
                out.rotations.push(Track::Frames(v));
            }
            for i in 0..nt {
                let base = t_off + t_stride * i;
                let v = (0..n(t_stride, 12)).map(|f| vec3_at(b, base + 12 * f)).collect::<Result<Vec<_>>>()?;
                out.translations.push(Track::Frames(v));
            }
            for i in 0..ns {
                let base = s_off + s_stride * i;
                let v = (0..n(s_stride, 4)).map(|f| f32_at(b, base + 4 * f)).collect::<Result<Vec<_>>>()?;
                out.scales.push(Track::Frames(v));
            }
        }
        codec_id::BYTE_KEYFRAMES
        | codec_id::WORD_KEYFRAMES
        | codec_id::REVERSE_BYTE_KEYFRAMES
        | codec_id::REVERSE_WORD_KEYFRAMES => {
            let word = matches!(codec, codec_id::WORD_KEYFRAMES | codec_id::REVERSE_WORD_KEYFRAMES);
            // Offsets from the block start: translation and scale track
            // headers, then the key frame numbers and the values of each
            // kind. Rotation headers start right after the 48-byte header.
            let (t_head, s_head) = (u(12)?, u(16)?);
            let (r_keys, t_keys, s_keys) = (u(20)?, u(24)?, u(28)?);
            let (r_vals, t_vals, s_vals) = (u(32)?, u(36)?, u(40)?);
            // Each track header is a u32: low 12 bits key count, high 20
            // bits the first key's index into that kind's keys and values.
            let keys = |head: usize, i: usize, key_base: usize| -> Result<(usize, Vec<u16>)> {
                let h = u(head + 4 * i)?;
                let (count, first) = (h & 0xFFF, h >> 12);
                let frames = (0..count)
                    .map(|k| {
                        if word {
                            le16(b, key_base + 2 * (first + k))
                        } else {
                            b.get(key_base + first + k)
                                .map(|&x| x as u16)
                                .ok_or_else(|| Error::Invalid("key frames run past the data".into()))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?;
                Ok((first, frames))
            };
            for i in 0..nr {
                let (first, frames) = keys(48, i, r_keys)?;
                let values = (0..frames.len()).map(|k| quat16_at(b, r_vals + 8 * (first + k))).collect::<Result<Vec<_>>>()?;
                out.rotations.push(Track::Keys { frames, values });
            }
            for i in 0..nt {
                let (first, frames) = keys(t_head, i, t_keys)?;
                let values = (0..frames.len()).map(|k| vec3_at(b, t_vals + 12 * (first + k))).collect::<Result<Vec<_>>>()?;
                out.translations.push(Track::Keys { frames, values });
            }
            for i in 0..ns {
                let (first, frames) = keys(s_head, i, s_keys)?;
                let values = (0..frames.len()).map(|k| f32_at(b, s_vals + 4 * (first + k))).collect::<Result<Vec<_>>>()?;
                out.scales.push(Track::Keys { frames, values });
            }
        }
        other => return Err(Error::Invalid(format!("unknown animation codec {other}"))),
    }
    Ok(out)
}

/// Bit `n` of a flags array of `words` u32s at `o`.
fn flag(b: &[u8], o: usize, n: usize) -> bool {
    le32(b, o + 4 * (n / 32)).map(|w| w >> (n % 32) & 1 == 1).unwrap_or(false)
}

impl Clip {
    /// Reads and decodes animation `index` of `graph`.
    pub fn load(resources: &Resources<'_>, graph: &Graph, index: usize) -> Result<Clip> {
        let raw = raw(resources, graph, index)?;
        Clip::decode(graph, index, &raw.member, raw.bytes())
    }

    /// Decodes an animation's bytes (see [`raw`]).
    pub fn decode(graph: &Graph, index: usize, member: &Member, data: &[u8]) -> Result<Clip> {
        let anim = &graph.animations[index];
        // The data is laid out for the member's node count; the graph may
        // have more nodes (they get no tracks).
        let nodes = member.node_count as usize;
        let frames = member.frame_count;
        let (ss, asz) = (member.static_size as usize, member.animated_size as usize);
        if member.uncompressed_size != 0 {
            return Err(Error::Invalid(format!(
                "{}: has {} bytes of uncompressed data, a layout not seen yet",
                anim.name, member.uncompressed_size
            )));
        }
        let mut tracks = vec![NodeTrack::default(); nodes.max(graph.nodes.len())];
        let mut flags_at = ss + asz;
        let mut codecs = [None, None];
        for (k, (start, size, flags_size)) in [
            (0, ss, member.static_flags_size as usize),
            (ss, asz, member.animated_flags_size as usize),
        ]
        .into_iter()
        .enumerate()
        {
            let here = flags_at;
            flags_at += flags_size;
            if size == 0 {
                continue;
            }
            // Three bit arrays of `words` u32s each. Usually
            // `words = ceil(nodes / 32)`, but some cinematic animations of
            // big skeletons carry fewer words: they drive only the first
            // `32 * words` nodes.
            if flags_size == 0 || flags_size % 12 != 0 {
                return Err(Error::Invalid(format!(
                    "{}: {flags_size} bytes of node flags for {nodes} nodes",
                    anim.name
                )));
            }
            let words = flags_size / 12;
            let block = data.get(start..start + size).ok_or_else(|| {
                Error::Invalid(format!("{}: codec block runs past the data", anim.name))
            })?;
            let d = decode_block(block, frames)?;
            codecs[k] = Some(d.codec);
            let which = |kind: usize| -> Vec<usize> {
                (0..nodes.min(32 * words)).filter(|&n| flag(data, here + 4 * words * kind, n)).collect()
            };
            let (rn, tn, sn) = (which(0), which(1), which(2));
            if rn.len() != d.rotations.len() || tn.len() != d.translations.len() || sn.len() != d.scales.len() {
                return Err(Error::Invalid(format!(
                    "{}: codec {} has {}/{}/{} tracks but the flags name {}/{}/{} nodes",
                    anim.name,
                    d.codec,
                    d.rotations.len(),
                    d.translations.len(),
                    d.scales.len(),
                    rn.len(),
                    tn.len(),
                    sn.len()
                )));
            }
            for (n, t) in rn.into_iter().zip(d.rotations) {
                tracks[n].rotation = t;
            }
            for (n, t) in tn.into_iter().zip(d.translations) {
                tracks[n].translation = t;
            }
            for (n, t) in sn.into_iter().zip(d.scales) {
                tracks[n].scale = t;
            }
        }
        let movement_kind = MovementKind::from_byte(member.movement);
        let per = movement_kind.floats();
        let mut movement = Vec::new();
        if per > 0 {
            let count = member.movement_size as usize / (4 * per);
            for f in 0..count {
                let o = flags_at + 4 * per * f;
                let v = |k: usize| f32_at(data, o + 4 * k);
                movement.push(match movement_kind {
                    MovementKind::DxDy => Motion { dx: v(0)?, dy: v(1)?, ..Default::default() },
                    MovementKind::DxDyDyaw => Motion { dx: v(0)?, dy: v(1)?, dz: 0.0, dyaw: v(2)? },
                    _ => Motion { dx: v(0)?, dy: v(1)?, dz: v(2)?, dyaw: v(3)? },
                });
            }
        }
        Ok(Clip {
            name: anim.name.clone(),
            kind: anim.kind,
            frame_count: frames.max(1),
            loop_frame: anim.loop_frame,
            tracks,
            movement_kind,
            movement,
            static_codec: codecs[0],
            animated_codec: codecs[1],
        })
    }

    /// Length in seconds at Halo's 30 frames a second, for a clip that
    /// plays once (frame 0 to the last frame).
    pub fn duration(&self) -> f32 {
        (self.frame_count.saturating_sub(1)) as f32 / crate::FPS
    }

    /// Root motion summed from frame 0 up to (fractional) frame `f`.
    pub fn motion_until(&self, f: f32) -> Motion {
        let mut m = Motion::default();
        let whole = f.max(0.0).floor() as usize;
        for (i, step) in self.movement.iter().enumerate() {
            let w = if i < whole {
                1.0
            } else if i == whole {
                f - whole as f32
            } else {
                break;
            };
            m.dx += step.dx * w;
            m.dy += step.dy * w;
            m.dz += step.dz * w;
            m.dyaw += step.dyaw * w;
        }
        m
    }
}
