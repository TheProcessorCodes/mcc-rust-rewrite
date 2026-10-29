//! Halo 3's own animations on the game's objects: bipeds (enemies, the
//! Spartan), device machines (doors, the 120_halo plates) and anything else
//! with a `jmad`.
//!
//! Two layers:
//!
//! - [`AnimModel`]: what one object type shares, loaded once from the map:
//!   its animation graph (and the graphs it inherits), the render model's
//!   skeleton and rest pose, and every animation decoded. `Arc` it and hand
//!   it to each actor of that type.
//! - [`Animator`]: one actor's playback state, a bevy component. Pick
//!   animations by Halo's names (`stance` + `play("move_front")`), one-shots
//!   (`play_once("melee")`), overlays (`overlay("fire_1")`), aiming
//!   (`aim(yaw, pitch)`) or a device position (`set_position(0.4)`).
//!   [`Animator::model_matrices`] returns one model-space matrix per render
//!   model node, ready for skinning.
//!
//! With [`AnimPlugin`] added, an entity carrying an [`Animator`] and
//! [`AnimJoints`] (its skinned mesh's joint entities, in render model node
//! order) is advanced every frame and its joints' `GlobalTransform`s are
//! written as `owner GlobalTransform * HALO_TO_RENDER * node matrix`. The
//! owner's transform places the object in the world (renderer frame, Y up);
//! the joints need no `Transform` of their own. The skinned mesh's inverse
//! bind matrices are [`AnimModel::inverse_bindposes`].
//!
//! Format notes: `docs/format/animation.md`.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::math::Affine3A;
use bevy::prelude::*;

use blam_anim::{AnimId, Clip, Graph, Library, Motion, NodeMap, Pose, Skeleton};
use blam_cache::{CacheFile, TagEntry};
use blam_geometry::WORLD_UNIT_METRES;

pub use blam_anim::{AnimKind, FPS};

/// Halo model space (world units, Z up) to the renderer's frame (metres, Y
/// up), the same turn the world and the rider use.
pub fn halo_to_render() -> Mat4 {
    Mat4::from_scale(Vec3::splat(WORLD_UNIT_METRES)) * Mat4::from_cols(Vec4::X, Vec4::NEG_Z, Vec4::Y, Vec4::W)
}

/// What every object of one type shares.
pub struct AnimModel {
    pub name: String,
    /// The object's own graph (tag index).
    pub graph: u32,
    pub library: Library,
    pub skeleton: Skeleton,
    /// Graph node -> skeleton node, per graph of the family.
    maps: HashMap<u32, NodeMap>,
    clips: HashMap<AnimId, Arc<Clip>>,
    /// Animations that did not decode (data not in this map).
    pub missing: usize,
}

impl AnimModel {
    /// Loads the graph and render model of an object tag (`bipd`, `mach`,
    /// `vehi`, `scen`, ...) or a model (`hlmt`), decoding all of its
    /// animations. The render model comes from the object's model; for a
    /// bare `jmad` it is the `mode` of the same name.
    pub fn load(cache: &CacheFile, object: &TagEntry) -> blam_cache::Result<Arc<AnimModel>> {
        let jmad = Graph::tag_of_object(cache, object)?
            .ok_or_else(|| blam_cache::Error::Invalid(format!("{} has no animation graph", object.name())))?;
        let mode = match cache.tags.class_of(object).code.as_str() {
            "jmad" => cache.tags.find("mode", object.name()),
            "hlmt" => cache.reader_at(cache.tag_offset(object)?).tag_ref()?.index.and_then(|i| cache.tags.get(i)),
            _ => blam_geometry::scenario::render_model_of(cache, object)
                .map_err(|e| blam_cache::Error::Invalid(e.to_string()))?,
        }
        .ok_or_else(|| blam_cache::Error::Invalid(format!("{} has no render model", object.name())))?;
        let skeleton = Skeleton::from_render_model(cache, mode)?;
        let mut library = Library::default();
        library.load(cache, jmad.index)?;
        let resources = cache.resources()?;
        let mut maps = HashMap::new();
        let mut clips = HashMap::new();
        let mut missing = 0;
        for (&tag, graph) in &library.graphs {
            maps.insert(tag, NodeMap::by_name(graph, &skeleton));
            for i in 0..graph.animations.len() {
                match Clip::load(&resources, graph, i) {
                    Ok(c) => {
                        clips.insert(AnimId { graph: tag, index: i as u16 }, Arc::new(c));
                    }
                    Err(_) => missing += 1,
                }
            }
        }
        resources.clear_page_cache();
        Ok(Arc::new(AnimModel {
            name: object.name().to_string(),
            graph: jmad.index,
            library,
            skeleton,
            maps,
            clips,
            missing,
        }))
    }

    /// Loads by tag path, trying the classes an object can be.
    pub fn load_path(cache: &CacheFile, path: &str) -> blam_cache::Result<Arc<AnimModel>> {
        let tag = ["bipd", "mach", "vehi", "scen", "ctrl", "weap", "hlmt", "jmad"]
            .iter()
            .find_map(|c| cache.tags.find(c, path))
            .ok_or_else(|| blam_cache::Error::Invalid(format!("no object tag named {path}")))?;
        AnimModel::load(cache, tag)
    }

    pub fn graph(&self) -> &Graph {
        &self.library.graphs[&self.graph]
    }

    /// `mode:class:type:action` with Halo's fallbacks (exact, then `any`
    /// type and class, then `combat` / `any` mode), through inheritance.
    pub fn action(&self, mode: &str, class: &str, kind: &str, action: &str) -> Option<AnimId> {
        self.library.action(self.graph, mode, class, kind, action)
    }

    pub fn overlay(&self, mode: &str, class: &str, kind: &str, name: &str) -> Option<AnimId> {
        self.library.overlay(self.graph, mode, class, kind, name)
    }

    /// An animation by its full name, e.g. `device:position`.
    pub fn named(&self, name: &str) -> Option<AnimId> {
        self.library.named(self.graph, name)
    }

    pub fn clip(&self, id: AnimId) -> Option<&Arc<Clip>> {
        self.clips.get(&id)
    }

    pub fn animation(&self, id: AnimId) -> Option<&blam_anim::Animation> {
        self.library.graph(id.graph)?.animations.get(id.index as usize)
    }

    /// One of the variants of `id` (`var1`, `var2`, ...), by weight.
    pub fn variant(&self, id: AnimId, roll: f32) -> AnimId {
        match self.library.graph(id.graph) {
            Some(g) => AnimId { graph: id.graph, index: g.pick_variant(id.index as usize, roll) as u16 },
            None => id,
        }
    }

    /// The rest pose in model space; invert for the skinned mesh.
    pub fn rest_model(&self) -> Vec<Mat4> {
        self.skeleton.rest_model()
    }

    /// Inverse bind matrices for bevy's `SkinnedMeshInverseBindposes`, in
    /// render model node order (vertices are in model space).
    pub fn inverse_bindposes(&self) -> Vec<Mat4> {
        self.rest_model().iter().map(Mat4::inverse).collect()
    }

    fn map(&self, graph: u32) -> Option<&NodeMap> {
        self.maps.get(&graph)
    }
}

/// The weapon stance an action is looked up under: Halo's mode, weapon
/// class and weapon type (`combat`, `rifle`, `ar`). Unknown parts fall back
/// to `any`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stance {
    pub mode: String,
    pub class: String,
    pub kind: String,
}

impl Default for Stance {
    fn default() -> Self {
        Stance { mode: "combat".into(), class: "any".into(), kind: "any".into() }
    }
}

#[derive(Clone, Debug)]
struct Playing {
    id: AnimId,
    /// The label it was asked for (`move_front`), to skip restarts.
    label: String,
    time: f32,
    speed: f32,
    looping: bool,
    weight: f32,
}

/// One object's playback: a base animation (cross-faded when it changes),
/// overlays on top, an aim overlay, or a device position.
#[derive(Component)]
pub struct Animator {
    pub model: Arc<AnimModel>,
    pub stance: Stance,
    base: Option<Playing>,
    /// The previous base, fading out: (animation, seconds left, fade length).
    fading: Option<(Playing, f32, f32)>,
    overlays: Vec<Playing>,
    aim: Option<(AnimId, f32, f32)>,
    position: Option<(AnimId, f32)>,
    /// Cross-fade length for base changes, seconds.
    pub fade: f32,
    root: Motion,
    seed: u32,
}

impl Animator {
    pub fn new(model: Arc<AnimModel>) -> Animator {
        let seed = model.graph.wrapping_mul(2654435761) ^ 0x9E37_79B9;
        Animator {
            model,
            stance: Stance::default(),
            base: None,
            fading: None,
            overlays: Vec::new(),
            aim: None,
            position: None,
            fade: 0.2,
            root: Motion::default(),
            seed,
        }
    }

    /// A different random sequence per actor (variant picks).
    pub fn with_seed(mut self, seed: u32) -> Animator {
        self.seed = seed | 1;
        self
    }

    fn roll(&mut self) -> f32 {
        // xorshift32
        let mut x = self.seed.max(1);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.seed = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn set_stance(&mut self, mode: &str, class: &str, kind: &str) {
        self.stance = Stance { mode: mode.into(), class: class.into(), kind: kind.into() };
    }

    fn lookup(&self, action: &str) -> Option<AnimId> {
        let s = &self.stance;
        self.model
            .action(&s.mode, &s.class, &s.kind, action)
            .or_else(|| self.model.named(action))
    }

    /// Loops the stance's `action` (`idle`, `move_front`, `turn_left`),
    /// cross-fading from what played before. Asking again for the action
    /// already playing does nothing. False when the graph has no such
    /// action under any fallback.
    pub fn play(&mut self, action: &str) -> bool {
        self.start(action, true)
    }

    /// Plays the stance's `action` once (`melee`, `evade_left`, `land_hard`)
    /// and holds its last frame; [`Animator::finished`] says when it is done.
    pub fn play_once(&mut self, action: &str) -> bool {
        self.start(action, false)
    }

    fn start(&mut self, action: &str, looping: bool) -> bool {
        if self.base.as_ref().is_some_and(|b| b.label == action && b.looping == looping && (looping || !self.finished())) {
            return true;
        }
        let Some(id) = self.lookup(action) else { return false };
        let roll = self.roll();
        let id = self.model.variant(id, roll);
        self.play_id(id, action, looping);
        true
    }

    /// Plays a specific animation as the base.
    pub fn play_id(&mut self, id: AnimId, label: &str, looping: bool) {
        let next = Playing { id, label: label.into(), time: 0.0, speed: 1.0, looping, weight: 1.0 };
        if let Some(old) = self.base.take()
            && self.fade > 0.0
        {
            self.fading = Some((old, self.fade, self.fade));
        }
        self.base = Some(next);
        self.position = None;
    }

    /// Plays an animation by its full name (`fall`, `device:power`,
    /// `combat:rifle:melee`) as the base, from the start.
    pub fn play_named(&mut self, name: &str, looping: bool) -> bool {
        match self.model.named(name) {
            Some(id) => {
                self.play_id(id, name, looping);
                true
            }
            None => false,
        }
    }

    /// Playback speed of the base animation (1 = Halo's 30 frames a second).
    pub fn set_speed(&mut self, speed: f32) {
        if let Some(b) = &mut self.base {
            b.speed = speed;
        }
    }

    /// Whether the base is a one-shot that has reached its last frame (or
    /// nothing plays).
    pub fn finished(&self) -> bool {
        match &self.base {
            None => true,
            Some(b) => {
                !b.looping
                    && self.model.clip(b.id).is_none_or(|c| b.time >= c.duration())
            }
        }
    }

    /// The label of the base animation playing.
    pub fn current(&self) -> Option<&str> {
        self.base.as_ref().map(|b| b.label.as_str())
    }

    /// Adds the stance's overlay `name` (`fire_1`, `melee`, `look`) on top.
    /// One-shot overlays drop off when they end.
    pub fn overlay(&mut self, name: &str, looping: bool) -> bool {
        let s = &self.stance;
        let Some(id) = self.model.overlay(&s.mode, &s.class, &s.kind, name).or_else(|| self.model.named(name)) else {
            return false;
        };
        self.overlays.retain(|o| o.label != name);
        self.overlays.push(Playing { id, label: name.into(), time: 0.0, speed: 1.0, looping, weight: 1.0 });
        true
    }

    pub fn clear_overlays(&mut self) {
        self.overlays.clear();
    }

    /// Aims with the stance's aim overlay (`aim_still_up`, or `aim_move_up`
    /// when `moving`): yaw left and pitch up positive, radians, relative to
    /// the body's facing.
    pub fn aim(&mut self, yaw: f32, pitch: f32, moving: bool) {
        let s = &self.stance;
        let name = if moving { "aim_move_up" } else { "aim_still_up" };
        let id = self
            .model
            .overlay(&s.mode, &s.class, &s.kind, name)
            .or_else(|| self.model.overlay(&s.mode, &s.class, &s.kind, "aim_still_up"));
        self.aim = id.map(|id| (id, yaw, pitch));
    }

    pub fn stop_aiming(&mut self) {
        self.aim = None;
    }

    /// Device machines: shows `device:position` at `position` (0 closed /
    /// start, 1 open / end). False when the graph has no such animation.
    pub fn set_position(&mut self, position: f32) -> bool {
        let id = match self.position {
            Some((id, _)) => Some(id),
            None => self.model.named("device:position"),
        };
        match id {
            Some(id) => {
                self.position = Some((id, position.clamp(0.0, 1.0)));
                true
            }
            None => false,
        }
    }

    /// Moves time on by `dt` seconds, collecting the base's root motion.
    pub fn advance(&mut self, dt: f32) {
        if let Some(b) = &mut self.base
            && let Some(clip) = self.model.clips.get(&b.id)
        {
            let before = blam_anim::frame_at(clip, b.time, b.looping);
            b.time += dt * b.speed;
            let after = blam_anim::frame_at(clip, b.time, b.looping);
            if !clip.movement.is_empty() {
                let (m0, m1) = (clip.motion_until(before), clip.motion_until(after));
                let mut d = Motion {
                    dx: m1.dx - m0.dx,
                    dy: m1.dy - m0.dy,
                    dz: m1.dz - m0.dz,
                    dyaw: m1.dyaw - m0.dyaw,
                };
                if after < before {
                    // Wrapped: the rest of the loop plus the start of the next.
                    let end = clip.motion_until(clip.frame_count.saturating_sub(1) as f32);
                    d = Motion {
                        dx: end.dx - m0.dx + m1.dx,
                        dy: end.dy - m0.dy + m1.dy,
                        dz: end.dz - m0.dz + m1.dz,
                        dyaw: end.dyaw - m0.dyaw + m1.dyaw,
                    };
                }
                self.root.dx += d.dx;
                self.root.dy += d.dy;
                self.root.dz += d.dz;
                self.root.dyaw += d.dyaw;
            }
        }
        if let Some((old, left, _)) = &mut self.fading {
            old.time += dt * old.speed;
            *left -= dt;
            if *left <= 0.0 {
                self.fading = None;
            }
        }
        let model = &self.model;
        self.overlays.retain_mut(|o| {
            o.time += dt * o.speed;
            o.looping || model.clips.get(&o.id).is_some_and(|c| o.time < c.duration())
        });
    }

    /// The root motion of the base animation since the last call, in the
    /// object's own frame (world units forward / left / up, radians left).
    /// Move the actor by it to keep feet from sliding.
    pub fn take_root_motion(&mut self) -> Motion {
        std::mem::take(&mut self.root)
    }

    /// The current local pose.
    pub fn pose(&self) -> Pose {
        let m = &self.model;
        let mut pose = Pose::rest(&m.skeleton);
        let layer = |pose: &mut Pose, p: &Playing, weight: f32| {
            if let (Some(clip), Some(map)) = (m.clips.get(&p.id), m.map(p.id.graph)) {
                pose.apply(clip, blam_anim::frame_at(clip, p.time, p.looping), map, weight * p.weight);
            }
        };
        if let Some((id, position)) = self.position {
            if let (Some(clip), Some(map)) = (m.clips.get(&id), m.map(id.graph)) {
                pose.apply(clip, blam_anim::frame_at_position(clip, position), map, 1.0);
            }
            return pose;
        }
        if let Some((old, left, total)) = &self.fading {
            layer(&mut pose, old, 1.0);
            if let Some(b) = &self.base {
                layer(&mut pose, b, 1.0 - (left / total.max(1e-3)).clamp(0.0, 1.0));
            }
        } else if let Some(b) = &self.base {
            layer(&mut pose, b, 1.0);
        }
        if let Some((id, yaw, pitch)) = self.aim
            && let (Some(clip), Some(map), Some(graph)) = (m.clips.get(&id), m.map(id.graph), m.library.graph(id.graph))
        {
            let screen = graph
                .animations
                .get(id.index as usize)
                .and_then(|a| a.blend_screen)
                .and_then(|s| graph.blend_screens.get(s as usize));
            match screen {
                Some(screen) => pose.apply_aim(clip, screen, yaw, pitch, map, 1.0),
                None => pose.apply(clip, (clip.frame_count / 2) as f32, map, 1.0),
            }
        }
        for o in &self.overlays {
            layer(&mut pose, o, 1.0);
        }
        pose
    }

    /// One model-space matrix per render model node (Halo model frame).
    pub fn model_matrices(&self) -> Vec<Mat4> {
        self.pose().model(&self.model.skeleton)
    }
}

/// The joint entities of an animated skinned mesh, in render model node
/// order (the `SkinnedMesh::joints` list).
#[derive(Component, Clone, Debug, Default)]
pub struct AnimJoints(pub Vec<Entity>);

/// Advances every [`Animator`] and poses its [`AnimJoints`].
pub struct AnimPlugin;

/// The systems that pose animated joints (after transforms propagate, so
/// the owner's `GlobalTransform` is this frame's).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct AnimSystems;

impl Plugin for AnimPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            pose_joints.in_set(AnimSystems).after(bevy::transform::TransformSystems::Propagate),
        );
    }
}

fn pose_joints(
    time: Res<Time>,
    paused: Option<Res<halo3_core::pause::Paused>>,
    mut animators: Query<(&mut Animator, &AnimJoints, &GlobalTransform)>,
    mut joints: Query<&mut GlobalTransform, Without<Animator>>,
) {
    let dt = if paused.is_some_and(|p| p.0) { 0.0 } else { time.delta_secs() };
    let h = halo_to_render();
    for (mut animator, joint_list, owner) in &mut animators {
        animator.advance(dt);
        let owner = owner.to_matrix() * h;
        for (m, &e) in animator.model_matrices().iter().zip(&joint_list.0) {
            if let Ok(mut g) = joints.get_mut(e) {
                *g = GlobalTransform::from(Affine3A::from_mat4(owner * *m));
            }
        }
    }
}

/// Spawns one bare joint entity per render model node (a `GlobalTransform`
/// only), for a `SkinnedMesh`'s `joints`.
pub fn spawn_joints(commands: &mut Commands, model: &AnimModel) -> AnimJoints {
    AnimJoints(
        (0..model.skeleton.len())
            .map(|_| commands.spawn(GlobalTransform::IDENTITY).id())
            .collect(),
    )
}
