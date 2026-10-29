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
