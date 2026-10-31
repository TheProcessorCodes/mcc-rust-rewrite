//! Poses: a render model's skeleton with its rest (bind) pose, decoded
//! clips sampled onto it, layered and blended, and turned into model-space
//! matrices for skinning (`docs/format/animation.md` section 10).
//!
//! Every animation value is a node's transform relative to its parent, in
//! the same convention as the render model's node table: a unit quaternion
//! (x, y, z, w; glam's), a translation in world units and a uniform scale.
//! A node an animation does not drive keeps whatever is under it (the rest
//! pose for a base animation).

use glam::{Mat4, Quat, Vec3};

use blam_cache::{CacheFile, TagEntry};

use crate::codec::Clip;
use crate::graph::{AnimKind, BlendScreen, Graph};
use crate::{Error, Result};

/// One node's transform relative to its parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xform {
    pub rotation: Quat,
    pub translation: Vec3,
    pub scale: f32,
}

impl Default for Xform {
    fn default() -> Self {
        Xform::IDENTITY
    }
}

impl Xform {
    pub const IDENTITY: Xform = Xform {
        rotation: Quat::IDENTITY,
        translation: Vec3::ZERO,
        scale: 1.0,
    };

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(Vec3::splat(self.scale), self.rotation, self.translation)
    }

    /// `self` to `other` by `t` (shortest-way rotation).
    pub fn lerp(&self, other: &Xform, t: f32) -> Xform {
        let o = if self.rotation.dot(other.rotation) < 0.0 { -other.rotation } else { other.rotation };
        Xform {
            rotation: self.rotation.lerp(o, t).normalize(),
            translation: self.translation.lerp(other.translation, t),
            scale: self.scale + (other.scale - self.scale) * t,
        }
    }
}

/// A render model's node tree and rest pose.
#[derive(Clone, Debug, Default)]
pub struct Skeleton {
    pub names: Vec<String>,
    pub parents: Vec<Option<u16>>,
    pub rest: Vec<Xform>,
}

impl Skeleton {
    /// Builds a skeleton from (name, parent, rotation, translation) in node
    /// order; a parent must come before its children.
    pub fn new(nodes: impl IntoIterator<Item = (String, Option<u16>, Quat, Vec3)>) -> Skeleton {
        let mut s = Skeleton::default();
        for (name, parent, rotation, translation) in nodes {
            s.names.push(name);
            s.parents.push(parent);
            s.rest.push(Xform {
                rotation: rotation.normalize(),
                translation,
                scale: 1.0,
            });
        }
        s
    }

    /// The node table of a render model (`mode`): 96-byte nodes at
    /// `mode + 48`, name, parent at `+4`, translation at `+12`, rotation at
    /// `+24` (the same fields `blam_geometry::render_model` reads).
    pub fn from_render_model(cache: &CacheFile, mode: &TagEntry) -> Result<Skeleton> {
        if cache.tags.class_of(mode).code.as_str() != "mode" {
            return Err(Error::Invalid(format!("{} is not a render model", mode.name())));
        }
        let base = cache.tag_offset(mode)?;
        let block = cache.reader_at(base + 48).block()?;
        let nodes = block
            .elements(96)
            .map(|o| {
                let mut r = cache.reader_at(o);
                let name = r.string_id()?;
                let parent = r.i16()?;
                let t = r.at(o + 12).f32x3()?;
                let q = r.at(o + 24).f32x4()?;
                Ok((
                    cache.string(name).unwrap_or("").to_string(),
                    u16::try_from(parent).ok(),
                    Quat::from_xyzw(q[0], q[1], q[2], q[3]),
                    Vec3::from(t),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Skeleton::new(nodes))
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
    pub fn index(&self, name: &str) -> Option<usize> {
        self.names.iter().position(|n| n == name)
    }

    /// Node frames in model space for local transforms `local` (one per
    /// node). Bevy-style skinning multiplies these by the inverse bind
    /// matrices (the inverse of [`Skeleton::rest_model`]).
    pub fn model_space(&self, local: &[Xform]) -> Vec<Mat4> {
        let mut out: Vec<Mat4> = Vec::with_capacity(self.len());
        for (i, x) in local.iter().enumerate().take(self.len()) {
            let m = x.matrix();
            let world = match self.parents[i].map(usize::from).filter(|&p| p < i) {
                Some(p) => out[p] * m,
                None => m,
            };
            out.push(world);
        }
        out
    }

    /// The rest pose in model space (matches `blam_geometry`'s `Bone::world`).
    pub fn rest_model(&self) -> Vec<Mat4> {
        self.model_space(&self.rest)
    }
}

/// Which skeleton node each graph node drives (by name; `None` when the
/// render model has no node of that name).
#[derive(Clone, Debug, Default)]
pub struct NodeMap(pub Vec<Option<u16>>);

impl NodeMap {
    pub fn by_name(graph: &Graph, skeleton: &Skeleton) -> NodeMap {
        NodeMap(
            graph
                .nodes
                .iter()
                .map(|n| skeleton.index(&n.name).and_then(|i| u16::try_from(i).ok()))
                .collect(),
        )
    }
    /// How many graph nodes found a skeleton node.
    pub fn matched(&self) -> usize {
        self.0.iter().flatten().count()
    }
}

/// Converts seconds into a (fractional) frame of `clip`. A looping clip's
/// last frame repeats its first, so it wraps every `frames - 1` frames; a
/// one-shot clip holds its last frame.
pub fn frame_at(clip: &Clip, seconds: f32, looping: bool) -> f32 {
    let last = clip.frame_count.saturating_sub(1) as f32;
    let f = seconds.max(0.0) * crate::FPS;
    if last <= 0.0 {
        0.0
    } else if looping {
        f.rem_euclid(last)
    } else {
        f.min(last)
    }
}

/// A device's position (0 = first frame, 1 = last) as a frame.
pub fn frame_at_position(clip: &Clip, position: f32) -> f32 {
    position.clamp(0.0, 1.0) * clip.frame_count.saturating_sub(1) as f32
}

/// A pose under construction: one local transform per skeleton node.
#[derive(Clone, Debug)]
pub struct Pose {
    pub local: Vec<Xform>,
}

impl Pose {
    pub fn rest(skeleton: &Skeleton) -> Pose {
        Pose {
            local: skeleton.rest.clone(),
        }
    }

    /// Lays `clip` at `frame` over the pose with `weight` (0..1). Base and
    /// replacement clips move each node they drive towards their value;
    /// overlay clips add their difference on top (rotation applied in the
    /// node's own frame after the pose's, translation added).
    pub fn apply(&mut self, clip: &Clip, frame: f32, map: &NodeMap, weight: f32) {
        if weight <= 0.0 {
            return;
        }
        let overlay = clip.kind == AnimKind::Overlay;
        for (g, track) in clip.tracks.iter().enumerate() {
            let Some(n) = map.0.get(g).copied().flatten().map(usize::from) else { continue };
            let Some(cur) = self.local.get_mut(n) else { continue };
            let r = track.rotation.sample(frame);
            let t = track.translation.sample(frame);
            let s = track.scale.sample(frame);
            if overlay {
                if let Some(r) = r {
                    let d = Quat::IDENTITY.lerp(if r.w < 0.0 { -r } else { r }, weight).normalize();
                    cur.rotation = (cur.rotation * d).normalize();
                }
                if let Some(t) = t {
                    cur.translation += t * weight;
                }
                if let Some(s) = s {
                    cur.scale *= 1.0 + (s - 1.0) * weight;
                }
            } else {
                let target = Xform {
                    rotation: r.unwrap_or(cur.rotation),
                    translation: t.unwrap_or(cur.translation),
                    scale: s.unwrap_or(cur.scale),
                };
                *cur = cur.lerp(&target, weight.min(1.0));
            }
        }
    }

    /// An aim overlay sampled on its grid: `yaw` (left positive) and
    /// `pitch` (up positive) in radians, clamped to the blend screen. The
    /// grid's frames run with yaw fastest, from right to left, rows from
    /// down to up; the centre frame is the reference (no change). The
    /// four frames around the aim are blended bilinearly.
    pub fn apply_aim(
        &mut self,
        clip: &Clip,
        screen: &BlendScreen,
        yaw: f32,
        pitch: f32,
        map: &NodeMap,
        weight: f32,
    ) {
        let (yl, yr) = (screen.yaw_steps.0.max(0) as f32, screen.yaw_steps.1.max(0) as f32);
        let (pd, pu) = (screen.pitch_steps.1.max(0) as f32, screen.pitch_steps.0.max(0) as f32);
        let cols = (yl + yr + 1.0) as usize;
        let rows = (pd + pu + 1.0) as usize;
        if cols * rows > clip.frame_count as usize || cols == 0 || rows == 0 {
            // Not a grid: play the middle frame.
            self.apply(clip, (clip.frame_count / 2) as f32, map, weight);
            return;
        }
        // Column from the rightmost (yaw = -right) to the leftmost.
        let col = if yaw >= 0.0 {
            yr + (yaw / screen.yaw_left.max(1e-4)).clamp(0.0, 1.0) * yl
        } else {
            yr - (-yaw / screen.yaw_right.max(1e-4)).clamp(0.0, 1.0) * yr
        };
        let row = if pitch >= 0.0 {
            pd + (pitch / screen.pitch_up.max(1e-4)).clamp(0.0, 1.0) * pu
        } else {
            pd - (-pitch / screen.pitch_down.max(1e-4)).clamp(0.0, 1.0) * pd
        };
        let (c0, r0) = (col.floor() as usize, row.floor() as usize);
        let (c1, r1) = ((c0 + 1).min(cols - 1), (r0 + 1).min(rows - 1));
        let (fc, fr) = (col - c0 as f32, row - r0 as f32);
        for (r, c, w) in [
            (r0, c0, (1.0 - fc) * (1.0 - fr)),
            (r0, c1, fc * (1.0 - fr)),
            (r1, c0, (1.0 - fc) * fr),
            (r1, c1, fc * fr),
        ] {
            if w > 1e-4 {
                self.apply(clip, (r * cols + c) as f32, map, weight * w);
            }
        }
    }

    /// `self` towards `other` by `t`, node by node.
    pub fn blend(&mut self, other: &Pose, t: f32) {
        for (a, b) in self.local.iter_mut().zip(&other.local) {
            *a = a.lerp(b, t);
        }
    }

    pub fn model(&self, skeleton: &Skeleton) -> Vec<Mat4> {
        skeleton.model_space(&self.local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::{NodeTrack, Track};
    use crate::graph::MovementKind;

    fn clip(kind: AnimKind, frames: u16, tracks: Vec<NodeTrack>) -> Clip {
        Clip {
            name: "test".into(),
            kind,
            frame_count: frames,
            loop_frame: 0,
            tracks,
            movement_kind: MovementKind::None,
            movement: Vec::new(),
            static_codec: None,
            animated_codec: None,
        }
    }

    #[test]
    fn keys_interpolate_and_clamp() {
        let t = Track::Keys { frames: vec![0, 4], values: vec![Vec3::ZERO, Vec3::X * 4.0] };
        assert_eq!(t.sample(1.0), Some(Vec3::X));
        assert_eq!(t.sample(-3.0), Some(Vec3::ZERO));
        assert_eq!(t.sample(9.0), Some(Vec3::X * 4.0));
        let f = Track::Frames(vec![0.0f32, 2.0, 4.0]);
        assert_eq!(f.sample(1.5), Some(3.0));
    }

    #[test]
    fn looping_wraps_on_the_last_frame() {
        let c = clip(AnimKind::Base, 31, vec![]);
        assert!((frame_at(&c, 1.0, true) - 0.0).abs() < 1e-4);
        assert!((frame_at(&c, 0.5, true) - 15.0).abs() < 1e-4);
        assert_eq!(frame_at(&c, 5.0, false), 30.0);
        assert_eq!(frame_at_position(&c, 0.5), 15.0);
    }

    #[test]
    fn base_replaces_and_overlay_adds() {
        let skel = Skeleton::new([
            ("root".to_string(), None, Quat::IDENTITY, Vec3::ZERO),
            ("child".to_string(), Some(0), Quat::IDENTITY, Vec3::X),
        ]);
        let map = NodeMap(vec![Some(0), Some(1)]);
        let turn = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let base = clip(
            AnimKind::Base,
            1,
            vec![
                NodeTrack { rotation: Track::Constant(turn), ..Default::default() },
                NodeTrack::default(),
            ],
        );
        let mut pose = Pose::rest(&skel);
        pose.apply(&base, 0.0, &map, 1.0);
        let m = pose.model(&skel);
        assert!((m[1].w_axis.truncate() - Vec3::Y).length() < 1e-5);
        let overlay = clip(
            AnimKind::Overlay,
            1,
            vec![NodeTrack { translation: Track::Constant(Vec3::Z), ..Default::default() }, NodeTrack::default()],
        );
        pose.apply(&overlay, 0.0, &map, 0.5);
        assert!((pose.local[0].translation - Vec3::Z * 0.5).length() < 1e-5);
        assert!(pose.local[0].rotation.dot(turn).abs() > 0.9999);
    }
}
