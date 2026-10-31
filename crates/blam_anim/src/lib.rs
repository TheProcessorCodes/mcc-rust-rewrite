//! Halo 3 animation: the `jmad` tag (model_animation_graph) and the
//! animation data it keeps in resources, read from the user's own MCC maps.
//!
//! - [`Graph`]: the tag: skeleton names, the animation list, the
//!   mode / weapon class / weapon type / action tree, inheritance and the
//!   resource groups.
//! - [`Clip`]: one animation decoded from its codec blocks into per-node
//!   tracks, plus its root motion.
//! - [`Skeleton`], [`NodeMap`], [`Pose`]: a render model's rest pose, the
//!   graph-to-model node match, and a pose clips are laid onto, ending in
//!   model-space matrices for skinning.
//! - [`Library`]: loads graphs (and the graphs they inherit from) once and
//!   follows inherited action references.
//!
//! Layouts are in `docs/format/animation.md`, checked against the bytes by
//! `docs/format/scripts/probe_animation.py`. No code from other projects is
//! copied.

pub mod codec;
pub mod graph;
pub mod pose;

use std::collections::HashMap;

pub use blam_cache::{Error, Result};
pub use codec::{Clip, Motion, NodeTrack, Track};
pub use graph::{AnimKind, AnimRef, Animation, BlendScreen, Graph, MovementKind};
pub use pose::{NodeMap, Pose, Skeleton, Xform, frame_at, frame_at_position};

use blam_cache::CacheFile;

/// Halo animates at 30 frames a second.
pub const FPS: f32 = 30.0;

/// An animation somewhere in a graph family: the graph's tag index and the
/// animation's index in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AnimId {
    pub graph: u32,
    pub index: u16,
}

/// Graphs by tag index, with the graphs they inherit from.
#[derive(Default)]
pub struct Library {
    pub graphs: HashMap<u32, Graph>,
}

impl Library {
    /// Reads graph `tag` (a `jmad` tag index) and everything it inherits.
    pub fn load(&mut self, cache: &CacheFile, tag: u32) -> Result<&Graph> {
        let mut todo = vec![tag];
        while let Some(t) = todo.pop() {
            if self.graphs.contains_key(&t) {
                continue;
            }
            let entry = cache
                .tags
                .get(t)
                .ok_or_else(|| Error::Invalid(format!("no tag #{t}")))?;
            let g = Graph::read(cache, entry)?;
            todo.extend(g.inheritance.iter().filter_map(|i| i.graph));
            todo.extend(g.parent);
            self.graphs.insert(t, g);
        }
        Ok(&self.graphs[&tag])
    }

    pub fn graph(&self, tag: u32) -> Option<&Graph> {
        self.graphs.get(&tag)
    }

    /// Where a reference found in graph `from` points.
    pub fn resolve(&self, from: u32, r: &AnimRef) -> Option<AnimId> {
        let g = self.graphs.get(&from)?;
        let graph = match r.graph {
            None => from,
            Some(i) => g.inheritance.get(i as usize)?.graph?,
        };
        let target = self.graphs.get(&graph)?;
        ((r.animation as usize) < target.animations.len()).then_some(AnimId {
            graph,
            index: r.animation,
        })
    }

    /// `mode:class:type:action` in graph `from`, with fallbacks
    /// ([`Graph::action`]), followed through inheritance.
    pub fn action(&self, from: u32, mode: &str, class: &str, kind: &str, action: &str) -> Option<AnimId> {
        let r = self.graphs.get(&from)?.action(mode, class, kind, action)?;
        self.resolve(from, r)
    }

    /// Like [`Library::action`], for overlays.
    pub fn overlay(&self, from: u32, mode: &str, class: &str, kind: &str, overlay: &str) -> Option<AnimId> {
        let r = self.graphs.get(&from)?.overlay(mode, class, kind, overlay)?;
        self.resolve(from, r)
    }

    /// An animation by its full name (`combat:rifle:move_front`,
    /// `device:position`) in graph `from`, else in the graphs it inherits.
    pub fn named(&self, from: u32, name: &str) -> Option<AnimId> {
        let g = self.graphs.get(&from)?;
        if let Some(i) = g.find(name) {
            return Some(AnimId { graph: from, index: i as u16 });
        }
        g.inheritance.iter().filter_map(|i| i.graph).find_map(|t| {
            let i = self.graphs.get(&t)?.find(name)?;
            Some(AnimId { graph: t, index: i as u16 })
        })
    }
}
