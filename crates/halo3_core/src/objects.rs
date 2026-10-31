//! Objects scripts move and hide (`halo3_script`), as the other crates see
//! them: the world chunks that draw one object, the marker that hides them,
//! and the objects whose collision has been taken away (a destroyed or
//! falling floor plate), which the bike, the Spartan and the skater stop
//! colliding with.

use std::collections::HashSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, RwLock};
use std::time::Instant;

use bevy::prelude::*;

/// A world chunk that draws one scene object (`Scene::objects` index)
/// on its own: its `Transform` moves the object from where the scenario
/// placed it, renderer frame.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjectChunk(pub u32);

/// A chunk a script has hidden (destroyed, not yet created): the culling
/// leaves it hidden.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct ScriptHidden;

struct Removed {
    set: HashSet<u32>,
    changed: Option<Instant>,
}

static REMOVED: LazyLock<RwLock<Removed>> = LazyLock::new(|| {
    RwLock::new(Removed {
        set: HashSet::new(),
        changed: None,
    })
});
/// Bumped on every change; zero while nothing was ever removed.
static GENERATION: AtomicU64 = AtomicU64::new(0);

/// Takes an object's collision away (`true`) or gives it back.
pub fn set_collision_removed(object: u32, removed: bool) {
    let mut r = REMOVED.write().unwrap();
    let changed = if removed { r.set.insert(object) } else { r.set.remove(&object) };
    if changed {
        r.changed = Some(Instant::now());
        GENERATION.fetch_add(1, Ordering::Release);
    }
}

/// Whether an object's collision is gone.
pub fn collision_removed(object: u32) -> bool {
    GENERATION.load(Ordering::Acquire) != 0 && REMOVED.read().unwrap().set.contains(&object)
}

/// Drops the candidates whose object's collision is gone; `owner` gives a
/// candidate's object (`u32::MAX` for the structure BSP's own).
pub fn retain_present(candidates: &mut Vec<u32>, owner: impl Fn(u32) -> u32) {
    if GENERATION.load(Ordering::Acquire) == 0 {
        return;
    }
    let r = REMOVED.read().unwrap();
    if r.set.is_empty() {
        return;
    }
    candidates.retain(|&c| {
        let o = owner(c);
        o == u32::MAX || !r.set.contains(&o)
    });
}

/// Gives every object its collision back and puts every moved object back
/// where the scenario placed it (the map is unloading).
pub fn reset() {
    {
        let mut r = REMOVED.write().unwrap();
        if !r.set.is_empty() {
            r.set.clear();
            r.changed = Some(Instant::now());
            GENERATION.fetch_add(1, Ordering::Release);
        }
    }
    let mut p = POSES.write().unwrap();
    if !p.map.is_empty() {
        p.map.clear();
        p.changed = Some(Instant::now());
        POSE_GENERATION.fetch_add(1, Ordering::Release);
    }
}

/// The change counter and when it last changed.
pub fn removed_generation() -> (u64, Option<Instant>) {
    let g = GENERATION.load(Ordering::Acquire);
    if g == 0 {
        return (0, None);
    }
    (g, REMOVED.read().unwrap().changed)
}

/// A copy of the objects whose collision is gone.
pub fn removed_objects() -> HashSet<u32> {
    REMOVED.read().unwrap().set.clone()
}

/// Where solid objects a script moves are now: per object, the motion from
/// where the scenario placed it to where it is (Halo frame, `now *
/// placed⁻¹`). A Warthog run island tilting under the player: its
/// collision goes with it.
static POSES: LazyLock<RwLock<Poses>> = LazyLock::new(|| RwLock::new(Poses::default()));
static POSE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Poses {
    map: std::collections::HashMap<u32, Mat4>,
    changed: Option<Instant>,
}

/// Sets an object's motion from its placement, or clears it (`None`: back
/// where the scenario put it). Changes under a hundredth of a world unit
/// are ignored.
pub fn set_object_pose(object: u32, motion: Option<Mat4>) {
    let motion = motion.filter(|m| !m.abs_diff_eq(Mat4::IDENTITY, 1e-5));
    let mut p = POSES.write().unwrap();
    let changed = match (motion, p.map.get(&object)) {
        (None, None) => false,
        (None, Some(_)) => {
            p.map.remove(&object);
            true
        }
        (Some(m), Some(old)) if m.abs_diff_eq(*old, 1e-4) => false,
        (Some(m), _) => {
            p.map.insert(object, m);
            true
        }
    };
    if changed {
        p.changed = Some(Instant::now());
        POSE_GENERATION.fetch_add(1, Ordering::Release);
    }
}

/// The pose change counter (zero while nothing ever moved) and when it
/// last changed.
pub fn pose_generation() -> (u64, Option<Instant>) {
    let g = POSE_GENERATION.load(Ordering::Acquire);
    if g == 0 {
        return (0, None);
    }
    (g, POSES.read().unwrap().changed)
}

/// A copy of the moved objects' motions (Halo frame).
pub fn object_poses() -> Vec<(u32, Mat4)> {
    if POSE_GENERATION.load(Ordering::Acquire) == 0 {
        return Vec::new();
    }
    POSES.read().unwrap().map.iter().map(|(o, m)| (*o, *m)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_change_only_when_moved() {
        let (g0, _) = pose_generation();
        set_object_pose(9_000_001, Some(Mat4::from_translation(Vec3::Z)));
        let (g1, _) = pose_generation();
        assert!(g1 > g0);
        set_object_pose(9_000_001, Some(Mat4::from_translation(Vec3::Z * 1.000_01)));
        assert_eq!(pose_generation().0, g1);
        assert!(object_poses().iter().any(|(o, _)| *o == 9_000_001));
        set_object_pose(9_000_001, None);
        assert!(pose_generation().0 > g1);
        assert!(!object_poses().iter().any(|(o, _)| *o == 9_000_001));
    }
}
