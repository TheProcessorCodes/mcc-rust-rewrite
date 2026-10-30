//! Device machines posed by their own animations for the scripts
//! (`halo3_script::DevicePoser`): the Warthog run's islands
//! (`plates\island_test`: `other_fall` / `fall` under `tilt_up` / `tilt` /
//! `other_tilt`) and falling pillars (`plates\pillar_fall`:
//! `device:position`), doors and anything else with a `jmad`.
//!
//! A machine is posed as one rigid piece by its root node: the scripts'
//! position track at the device position, the overlay track on top, both
//! measured from their first frames (what the designers saw when they placed
//! the object), so position 0 / overlay 0 leaves it exactly where the
//! scenario put it.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use blam_cache::CacheFile;

use crate::AnimModel;

/// Poses machines from their animation graphs, loaded once per object tag.
#[derive(Default)]
pub struct MachinePoser {
    models: HashMap<u32, Arc<AnimModel>>,
}

impl MachinePoser {
    /// Loads the graphs of the given object tags (machines the scripts
    /// move); tags without a graph or render model are left out (they keep
    /// the scripts' stand-in pose).
    pub fn load(cache: &CacheFile, tags: impl IntoIterator<Item = u32>) -> MachinePoser {
        let mut models = HashMap::new();
        for tag in tags {
            if models.contains_key(&tag) {
                continue;
            }
            let Some(entry) = cache.tags.get(tag) else { continue };
            if cache.tags.class_of(entry).code.as_str() != "mach" {
                continue;
            }
            match AnimModel::load(cache, entry) {
                Ok(m) => {
                    models.insert(tag, m);
                }
                Err(e) => eprintln!("devices: {}: no animation ({e})", entry.name()),
            }
        }
        MachinePoser { models }
    }

    pub fn len(&self) -> usize {
        self.models.len()
    }

    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    pub fn model(&self, tag: u32) -> Option<&Arc<AnimModel>> {
        self.models.get(&tag)
    }
}

/// The animation a position track names: the track itself when the graph
/// has it, else `device:position`.
fn position_track<'a>(model: &AnimModel, track: Option<&'a str>) -> &'a str {
    match track {
        Some(t) if model.named(t).is_some() => t,
        _ => "device:position",
    }
}

/// The root node's model-space frame for a position and an overlay.
fn root_frame(model: &AnimModel, layers: &[(&str, f32)]) -> Option<Mat4> {
    let pose = model.pose_at(layers)?;
    pose.model(&model.skeleton).first().copied()
}

/// The object's motion (object frame, Halo world units) for a machine's
/// tracks: root node now, relative to the same tracks at their first frame.
pub fn machine_pose(model: &AnimModel, track: Option<&str>, position: f32, overlay_track: Option<&str>, overlay: f32) -> Option<Mat4> {
    let base = position_track(model, track);
    let overlay_track = overlay_track.filter(|o| model.named(o).is_some());
    let mut now = vec![(base, position)];
    let mut start = vec![(base, 0.0)];
    if let Some(o) = overlay_track {
        now.push((o, overlay));
        start.push((o, 0.0));
    }
    let a = root_frame(model, &now)?;
    let b = root_frame(model, &start)?;
    Some(a * b.inverse())
}

impl halo3_script::DevicePoser for MachinePoser {
    fn pose(&self, _object: u32, tag: u32, track: Option<&str>, position: f32, overlay_track: Option<&str>, overlay: f32) -> Option<Mat4> {
        machine_pose(self.models.get(&tag)?, track, position, overlay_track, overlay)
    }

    fn anchor(&self, tag: u32) -> Option<Vec3> {
        let m = self.models.get(&tag)?;
        m.rest_model().first().map(|r| r.w_axis.truncate())
    }
}
