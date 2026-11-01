//! HaloScript in the game: the map's own scripts run at 30 ticks a second
//! (`vm.rs`) and move the world (`objects.rs`): objects are created,
//! destroyed, dropped and moved; floor plates lose their collision so the
//! player falls; effects are queued for `halo3_effects`; the player is
//! teleported and zone sets switch when a script says so.
//!
//! The AI and animation sides are hooks (`hooks.rs`) other crates fill.
//! `docs/format/halo-script.md` has the format and the runtime notes.
//!
//! Set up by `main`: [`prepare`] marks the objects scripts move (before the
//! world is built), [`PendingScripts`] carries the runtime in.

pub mod ai_bridge;
pub mod crate_physics;
pub mod dialogue;
pub mod ground;
pub mod hooks;
pub mod objects;
pub mod setup;
pub mod value;
pub mod vm;

use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use halo3_core::coords::{to_halo, to_render, to_render_dir};
use halo3_core::effects::{EffectQueue, EffectRequest};
use halo3_core::falls::{FallbackSpot, set_kill_volume_enabled};
use halo3_core::fly_camera::FlyCamera;
use halo3_core::objects::{ObjectChunk, ScriptHidden, set_collision_removed, set_object_pose};
use halo3_core::script_sound::ScriptSounds;
use halo3_core::skate::{Respawn, RiderPose};
use halo3_core::zones::{LoadedBsps, Zones, load_zone_set};

pub use ai_bridge::BridgeAi;
pub use hooks::{AiHooks, DevicePoser, HookContext, NoAi};
pub use value::{ObjectRef, Value};
pub use vm::{Config, Runtime, WorldApi};

/// Reads a map's scripts and marks the scene objects they move, so the
/// world draws those on their own. `None` when the map has no scripts or
/// scripts are off (`HALO3_SCRIPTS=0`).
pub fn prepare(cache: &blam_cache::CacheFile, scene: &mut blam_geometry::Scene) -> Option<std::sync::Arc<blam_script::ScriptData>> {
    if std::env::var("HALO3_SCRIPTS").is_ok_and(|v| v == "0") {
        return None;
    }
    let data = match blam_script::ScriptData::read(cache) {
        Ok(d) if !d.scripts.is_empty() => d,
        Ok(_) => return None,
        Err(e) => {
            eprintln!("scripts: not read ({e})");
            return None;
        }
    };
    let n = setup::mark_dynamic(scene, &data);
    eprintln!(
        "scripts: {} scripts, {} globals; {n} objects drawn on their own for them",
        data.scripts.len(),
        data.globals.len()
    );
    Some(std::sync::Arc::new(data))
}

/// The runtime, handed from `main`, and a static script to run at start
/// as if typed at the console (an insertion point: `ins_trench_run`).
#[derive(Resource)]
pub struct PendingScripts(pub Option<Runtime>, pub Option<String>, pub Option<setup::StartOverride>);

/// The running scripts.
#[derive(Resource)]
pub struct Scripts {
    pub runtime: Runtime,
    accumulator: f64,
    applied: u64,
    /// Per scene object: whether its collision is in (last told to
    /// `halo3_core::objects`).
    solid: Vec<bool>,
    /// Per scene object: whether it is told to collide away from where the
    /// scenario placed it (`halo3_core::objects::set_object_pose`).
    posed: Vec<bool>,
    /// A teleport waiting for the camera to exist.
    teleport: Option<(Vec3, f32)>,
    /// The last tick traced (`HALO3_SCRIPT_TRACE`).
    traced: u64,
}

/// Ticks run before the first frame, so an insertion's zone switch,
/// teleport and kill-volume changes are in place before anyone can press
/// B, J or F (120_halo's run start lies inside `kill_100_01`, which
/// `object_management` switches off once the zone set is in).
const STARTUP_TICKS: usize = 15;

/// Applies what the scripts asked for during some ticks: kill volumes, a
/// zone switch, a new fallback spot; returns the teleport.
fn apply_requests(
    commands: &mut Commands,
    scripts: &mut Scripts,
    bridge: &mut Bridge,
    loaded: &mut LoadedBsps,
    zones: Option<&Zones>,
    fallback: Option<&mut FallbackSpot>,
) -> Option<(Vec3, f32)> {
    for (v, on) in std::mem::take(&mut scripts.runtime.kill_volumes) {
        if let Some(volume) = scripts.runtime.volumes.get(v) {
            set_kill_volume_enabled(&volume.name, on);
        }
    }
    if let (Some(z), Some(zones)) = (bridge.switch.take(), zones)
        && loaded.zone_set != Some(z)
    {
        load_zone_set(loaded, zones, z);
    }
    let (p, yaw) = bridge.teleport.take()?;
    // A fall before the rider ever stood anywhere safe comes back here,
    // not to the map's old start.
    match fallback {
        Some(f) => *f = FallbackSpot(p, yaw),
        None => commands.insert_resource(FallbackSpot(p, yaw)),
    }
    Some((p, yaw))
}

/// The AI behind `ai_*` / `cs_*` / `vs_*` calls; [`NoAi`] until the
/// enemies are in.
#[derive(Resource)]
pub struct ScriptAi(pub Box<dyn AiHooks>);

impl ScriptAi {
    pub fn new(hooks: impl AiHooks) -> ScriptAi {
        ScriptAi(Box::new(hooks))
    }
}

/// Device animation from the object's `jmad`, when someone provides it.
#[derive(Resource, Default)]
pub struct ScriptAnimation(pub Option<Box<dyn DevicePoser>>);

impl ScriptAnimation {
    pub fn new(poser: impl DevicePoser) -> ScriptAnimation {
        ScriptAnimation(Some(Box::new(poser)))
    }
}

pub struct ScriptPlugin;

impl Plugin for ScriptPlugin {
    fn build(&self, app: &mut App) {
        // A map's scripts come in with it (`PendingScripts`, and `ScriptAi`
        // / `ScriptAnimation` when it has AI and machines); everything they
        // changed goes when it is unloaded (`halo3_core::map`).
        app.insert_resource(ScriptAi::new(NoAi))
            .init_map_resource::<ScriptAnimation>()
            .init_map_resource::<EffectQueue>()
            .init_map_resource::<ScriptSounds>()
            .remove_on_map_unload::<PendingScripts>()
            .remove_on_map_unload::<Scripts>()
            .on_map_unload(|world| {
                world.insert_resource(ScriptAi::new(NoAi));
                halo3_core::objects::reset();
                halo3_core::falls::reset_kill_volumes();
            })
            .add_systems(halo3_core::map::MapStartup, start)
            .add_systems(
                Update,
                (
                    (tick.run_if(halo3_core::pause::running), apply_objects).chain(),
                    forward_sounds.after(tick).run_if(halo3_core::pause::running),
                )
                    .run_if(halo3_core::map::ready),
            );
    }
}

/// Moves the scripts' sound calls on to the sound side (`halo3_audio`).
fn forward_sounds(scripts: Option<ResMut<Scripts>>, mut queue: ResMut<ScriptSounds>) {
    let Some(mut scripts) = scripts else { return };
    for r in scripts.runtime.sounds.drain(..) {
        queue.push(r);
    }
}

#[allow(clippy::too_many_arguments)]
fn start(
    mut commands: Commands,
    pending: Option<ResMut<PendingScripts>>,
    mut ai: ResMut<ScriptAi>,
    animation: Res<ScriptAnimation>,
    zones: Option<Res<Zones>>,
    mut loaded: ResMut<LoadedBsps>,
    mut fallback: Option<ResMut<FallbackSpot>>,
) {
    let Some(mut pending) = pending else { return };
    let Some(mut runtime) = pending.0.take() else { return };
    if let Some(script) = pending.1.take() {
        runtime.console(&script);
    }
    let n = runtime.objects.info.iter().filter(|o| o.scene.is_some()).count();
    let mut scripts = Scripts {
        runtime,
        accumulator: 0.0,
        applied: 0,
        solid: vec![true; n],
        posed: vec![false; n],
        teleport: None,
        traced: 0,
    };
    let mut bridge = Bridge {
        player: None,
        zone_set: loaded.zone_set,
        teleport: None,
        switch: None,
        effects: Vec::new(),
        riding: false,
    };
    for _ in 0..STARTUP_TICKS {
        scripts.runtime.step(&mut bridge, ai.0.as_mut(), animation.0.as_deref());
        bridge.zone_set = bridge.switch.or(bridge.zone_set);
    }
    if let Some(start) = pending.2.take() {
        setup::apply_start(&mut scripts.runtime, &mut bridge, ai.0.as_mut(), animation.0.as_deref(), &start, STARTUP_TICKS);
        bridge.zone_set = bridge.switch.or(bridge.zone_set);
    }
    let t = apply_requests(&mut commands, &mut scripts, &mut bridge, &mut loaded, zones.as_deref(), fallback.as_deref_mut());
    scripts.teleport = t.or(scripts.teleport);
    commands.insert_resource(scripts);
}

/// What the scripts asked of the world during the ticks of one frame.
struct Bridge {
    player: Option<Vec3>,
    zone_set: Option<usize>,
    teleport: Option<(Vec3, f32)>,
    switch: Option<usize>,
    effects: Vec<EffectRequest>,
    /// The player rides the dirt bike.
    riding: bool,
}

impl WorldApi for Bridge {
    fn player_position(&self) -> Option<Vec3> {
        self.player
    }
    fn teleport_player(&mut self, position: Vec3, yaw: f32) {
        self.teleport = Some((position, yaw));
        self.player = Some(position);
    }
    fn current_zone_set(&self) -> Option<usize> {
        self.switch.or(self.zone_set)
    }
    fn switch_zone_set(&mut self, zone_set: usize) {
        self.switch = Some(zone_set);
    }
    fn effect(&mut self, tag: &str, at: Vec3) {
        if self.effects.len() < 64 {
            self.effects.push(EffectRequest::at(tag, at));
        }
    }
    fn effect_placed(&mut self, tag: &str, at: Vec3, frame: Mat3, source: Option<u32>) {
        if self.effects.len() < 64 {
            self.effects.push(EffectRequest {
                name: tag.to_string(),
                at,
                frame,
                source,
            });
        }
    }
    fn player_in_vehicle(&self) -> bool {
        self.riding
    }
}

#[allow(clippy::too_many_arguments)]
fn tick(
    mut commands: Commands,
    scripts: Option<ResMut<Scripts>>,
    mut ai: ResMut<ScriptAi>,
    animation: Res<ScriptAnimation>,
    time: Res<Time>,
    rider: Option<Res<RiderPose>>,
    mut respawn: Option<ResMut<Respawn>>,
    mut fallback: Option<ResMut<FallbackSpot>>,
    zones: Option<Res<Zones>>,
    mut loaded: ResMut<LoadedBsps>,
    mut cameras: Query<(&mut Transform, &mut FlyCamera)>,
    mut effects: ResMut<EffectQueue>,
) {
    let Some(mut scripts) = scripts else { return };
    let Ok((mut camera, mut fly)) = cameras.single_mut() else { return };
    // The skater's (or rider's) hips while riding, else the camera.
    let at = rider
        .as_deref()
        .filter(|r| r.active)
        .and_then(|r| {
            let i = r.names.iter().position(|n| n == "HIPS")?;
            Some(r.bones.get(i)?.w_axis.truncate() / r.scale)
        })
        .unwrap_or(camera.translation);
    let mut bridge = Bridge {
        player: Some(to_halo(at)),
        zone_set: loaded.zone_set,
        teleport: None,
        switch: None,
        effects: Vec::new(),
        riding: rider.as_deref().is_some_and(|r| r.active && r.bike),
    };
    // `HALO3_SCRIPT_TRACE=1`: where the player is, once a second.
    static TRACE: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("HALO3_SCRIPT_TRACE").is_ok_and(|v| v == "1"));
    if *TRACE && scripts.runtime.tick >= scripts.traced + 30 {
        scripts.traced = scripts.runtime.tick;
        let p = to_halo(at);
        let mode = match rider.as_deref() {
            Some(r) if r.active && r.bike => "bike",
            Some(r) if r.active && r.foot => "foot",
            Some(r) if r.active => "skate",
            _ => "camera",
        };
        info!(
            "script trace: tick {} {mode} at halo [{:.2}, {:.2}, {:.2}], zone set {:?}",
            scripts.runtime.tick, p.x, p.y, p.z, loaded.zone_set
        );
    }
    scripts.accumulator = (scripts.accumulator + time.delta_secs_f64()).min(0.25);
    let dt = 1.0 / vm::TICKS_PER_SECOND;
    let poser = animation.0.as_deref();
    while scripts.accumulator >= dt && scripts.runtime.mission_over.is_none() {
        scripts.accumulator -= dt;
        scripts.runtime.step(&mut bridge, ai.0.as_mut(), poser);
        if scripts.runtime.mission_over.is_some() {
            // Won: the scripts stop here. The outro goes on to teleport the
            // player to its cutscene flags (120_halo's `cin_halo_outro`);
            // the game's completion screen takes over instead.
            bridge.teleport = None;
            scripts.accumulator = 0.0;
        }
    }

    if let Some(t) = apply_requests(&mut commands, &mut *scripts, &mut bridge, &mut loaded, zones.as_deref(), fallback.as_deref_mut()) {
        scripts.teleport = Some(t);
    }
    if let Some((p, yaw)) = scripts.teleport.take() {
        let skating = rider.as_deref().is_some_and(|r| r.active && !r.bike && !r.foot);
        if skating && let Some(respawn) = respawn.as_deref_mut() {
            respawn.0 = Some((p, yaw));
        } else if rider.as_deref().is_some_and(|r| r.active) {
            info!("script: teleport skipped while riding the bike or on foot");
        } else {
            // Eye height above the flag.
            camera.translation = to_render(p + Vec3::Z * 0.7);
            let f = to_render_dir(Vec3::new(yaw.cos(), yaw.sin(), 0.0));
            fly.yaw = (-f.x).atan2(-f.z);
            fly.pitch = 0.0;
            camera.rotation = fly.rotation();
        }
    }
    for r in bridge.effects {
        effects.push(r);
    }
}

/// Renderer frame from Halo frame: `(x, y, z) = 3.048 * (hx, hz, -hy)`.
fn render_from_halo() -> Mat4 {
    let s = blam_geometry::WORLD_UNIT_METRES;
    Mat4::from_cols(
        Vec4::new(s, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -s, 0.0),
        Vec4::new(0.0, s, 0.0, 0.0),
        Vec4::W,
    )
}

/// Shows, hides and moves the chunks of scripted objects, and tells the
/// collision users which objects no longer collide.
fn apply_objects(
    mut commands: Commands,
    scripts: Option<ResMut<Scripts>>,
    mut chunks: Query<(Entity, &ObjectChunk, &mut Transform, Has<ScriptHidden>)>,
    added: Query<(), Added<ObjectChunk>>,
) {
    let Some(mut scripts) = scripts else { return };
    let generation = scripts.runtime.objects.generation;
    if generation == scripts.applied && added.is_empty() {
        return;
    }
    scripts.applied = generation;
    let c = render_from_halo();
    let c_inv = c.inverse();
    let objects = &scripts.runtime.objects;
    // Scene objects are the first entries, by scene index.
    for (entity, chunk, mut transform, hidden) in &mut chunks {
        let Some(info) = objects.info.get(chunk.0 as usize).filter(|i| i.scene == Some(chunk.0)) else {
            continue;
        };
        let s = &objects.state[chunk.0 as usize];
        let hide = !s.exists || s.hidden;
        if hide != hidden {
            if hide {
                commands.entity(entity).insert(ScriptHidden);
            } else {
                commands.entity(entity).remove::<ScriptHidden>();
            }
        }
        let delta = c * s.transform * info.base.inverse() * c_inv;
        let t = Transform::from_matrix(delta);
        if *transform != t {
            *transform = t;
        }
    }
    let scripts = &mut *scripts;
    publish_collision(&scripts.runtime.objects, &mut scripts.solid, &mut scripts.posed);
}

/// Tells the collision users (`halo3_core::objects`) which scene objects
/// stopped or started colliding, and where solid objects a script moved
/// are now (an island tilting under the player: its collision goes where
/// it is drawn). `solid` and `posed` hold, per scene object, what was told
/// last; start with all `true` and all `false`.
pub fn publish_collision(objects: &crate::objects::Objects, solid: &mut [bool], posed: &mut [bool]) {
    for (i, was) in solid.iter_mut().enumerate() {
        let s = &objects.state[i];
        let now = s.exists && s.solid;
        if now != *was {
            *was = now;
            set_collision_removed(i as u32, !now);
        }
    }
    for (i, was) in posed.iter_mut().enumerate() {
        let (info, s) = (&objects.info[i], &objects.state[i]);
        let moved = s.exists && s.solid && s.transform != info.base;
        if moved {
            set_object_pose(i as u32, Some(s.transform * info.base.inverse()));
        } else if *was {
            set_object_pose(i as u32, None);
        }
        *was = moved;
    }
}
