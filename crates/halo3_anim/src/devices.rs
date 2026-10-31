//! Device machines posed by their own animations: where each stands before
//! anything moves it ([`place_machines`]), and how the scripts move it
//! (`halo3_script::DevicePoser`): the Warthog run's islands
//! (`plates\island_test`: `other_fall` / `fall` under `tilt_up` / `tilt` /
//! `other_tilt`) and falling pillars (`plates\pillar_fall`:
//! `device:position`), doors and anything else with a `jmad`.
//!
//! A machine is posed as one rigid piece by its root node.
//!
//! - **At rest** a machine is not in its render model's rest pose but in
//!   its `device:position` animation at position 0, whose values replace
//!   the rest pose's (the game never shows a machine unanimated). This
//!   matters for 120_halo's Forward Unto Dawn (`trench_frigate`): its model
//!   lies nose along +Y, and frame 0 turns the root a quarter turn so the
//!   nose runs along +X; placed with a yaw of 180 degrees that puts the
//!   open hangar under its stern facing the end of the Warthog run, where
//!   the `end_trilogy` volumes wait. Unposed it hung broadside across the
//!   run. [`place_machines`] bakes this into the scene's objects before the
//!   world and the collision are built from them.
//! - **When scripts move it**, the position track at the device position
//!   and the overlay track on top are measured from their first frames, so
//!   position 0 / overlay 0 leaves the object where it stood, and laid
//!   over the rest-time pose above.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use blam_cache::CacheFile;
use blam_geometry::{Kind, Scene};

use crate::AnimModel;

/// Poses machines from their animation graphs, loaded once per object tag.
#[derive(Default)]
pub struct MachinePoser {
    models: HashMap<u32, Arc<AnimModel>>,
    /// Per tag: the root's pose at rest relative to the rest pose
    /// ([`initial_pose`]), when it is not the rest pose.
    initial: HashMap<u32, Mat4>,
}

impl MachinePoser {
    /// Loads the graphs of the given object tags (machines the scripts
    /// move); tags without a graph or render model are left out (they keep
    /// the scripts' stand-in pose).
    pub fn load(cache: &CacheFile, tags: impl IntoIterator<Item = u32>) -> MachinePoser {
        let mut models = HashMap::new();
        let mut initial = HashMap::new();
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
                    if let Some(c) = initial_pose(&m) {
                        initial.insert(tag, c);
                    }
                    models.insert(tag, m);
                }
                Err(e) => eprintln!("devices: {}: no animation ({e})", entry.name()),
            }
        }
        MachinePoser { models, initial }
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

/// The root node's model-space frame at rest: its `device:position`
/// animation at position 0 laid over the rest pose as the clip says
/// ([`AnimModel::pose_at`]: its overlay adds to the rest pose), relative to
/// the rest pose. Read as whole values instead, the islands' track (zero
/// translation) dropped them 56 wu below where the scenario put them. `None` when that is the rest
/// pose (doors, whose root the animation does not drive) or the graph has
/// no `device:position`.
pub fn initial_pose(model: &AnimModel) -> Option<Mat4> {
    let pose = model.pose_at(&[("device:position", 0.0)])?;
    let now = pose.model(&model.skeleton).first().copied()?;
    let rest = model.rest_model().first().copied()?;
    let c = now * rest.inverse();
    (!c.abs_diff_eq(Mat4::IDENTITY, 1e-5)).then_some(c)
}

/// Puts every machine of `scene` in its pose at rest ([`initial_pose`]):
/// its object transform and its drawn pieces. Call after the scene is
/// decoded and before the collision is added or the world is built from
/// it. Returns how many machines moved.
pub fn place_machines(cache: &CacheFile, scene: &mut Scene) -> usize {
    let mut poses: HashMap<u32, Option<Mat4>> = HashMap::new();
    let mut moved = 0;
    for o in scene.objects.iter_mut().filter(|o| o.kind == Kind::Machine) {
        let c = *poses.entry(o.tag).or_insert_with(|| -> Option<Mat4> {
            let entry = cache.tags.get(o.tag)?;
            let model = AnimModel::load(cache, entry).ok()?;
            initial_pose(&model)
        });
        let Some(c) = c else { continue };
        let old = o.transform;
        o.transform = old * c;
        // Each piece is `old * piece`; it becomes `old * c * piece`.
        let shift = o.transform * old.inverse();
        for p in &mut scene.placements[o.placements.start as usize..o.placements.end as usize] {
            p.transform = shift * p.transform;
        }
        moved += 1;
    }
    moved
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
    /// Relative to the object's transform at rest, which [`place_machines`]
    /// already turned by the initial pose `c`: `c^-1 * motion * c`, so the
    /// motion happens before the initial pose as it does in model space.
    fn pose(&self, _object: u32, tag: u32, track: Option<&str>, position: f32, overlay_track: Option<&str>, overlay: f32) -> Option<Mat4> {
        let m = machine_pose(self.models.get(&tag)?, track, position, overlay_track, overlay)?;
        Some(match self.initial.get(&tag) {
            Some(c) => c.inverse() * m * *c,
            None => m,
        })
    }

    fn anchor(&self, tag: u32) -> Option<Vec3> {
        // The root's rest position: in the object's frame at rest (already
        // turned by the initial pose `c`) that is where the root is.
        let m = self.models.get(&tag)?;
        m.rest_model().first().map(|r| r.w_axis.truncate())
    }
}
