//! Halo 3's own effects in the game: what the map's scripts and damaged
//! objects ask for (`halo3_core::effects::EffectQueue`) played from the
//! map's `effe` tags, as the game plays them: events one after another,
//! particle systems with the tags' functions for count, life, speed,
//! physics, size, colour and fade, drawn with the particles' own bitmaps
//! and render method options; contrails behind carried effects; sounds
//! through `halo3_audio`. `docs/format/effects.md` has the format and the
//! rules, and what is not played yet.
//!
//! `main` reads the effects while the map loads ([`load::load`]) and puts
//! them in as [`PendingEffects`].

pub mod draw;
pub mod load;
pub mod sim;

use std::collections::{HashMap, HashSet};

use bevy::asset::embedded_asset;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::pbr::MaterialPlugin;
use bevy::prelude::*;

use halo3_core::coords::{to_halo, to_render};
use halo3_core::effects::EffectQueue;
use halo3_core::map::MapAppExt;
use halo3_core::script_sound::{ScriptSounds, SoundRequest};

pub use draw::{ParticleMaterial, ParticleShading};
pub use load::EffectAssets;

/// The map's effects, read while it loaded.
#[derive(Resource)]
pub struct PendingEffects(pub Option<EffectAssets>);

/// What is playing, and what it is drawn with.
#[derive(Resource)]
pub struct Effects {
    pub sim: sim::Sim,
    gpu: Gpu,
}

/// The images, materials and meshes the effects are drawn with.
struct Gpu {
    textures: HashMap<u32, Handle<Image>>,
    mono: HashSet<u32>,
    sequences: HashMap<u32, Vec<blam_texture::bitmap::Sequence>>,
    materials: HashMap<u64, Handle<ParticleMaterial>>,
    frames: HashMap<u32, Vec<[f32; 4]>>,
    drawn: HashMap<u64, (Entity, Handle<Mesh>)>,
    unknown: HashSet<String>,
    exposure: f32,
}

/// `HALO3_EFFECT_TRACE=1`: every effect started, and what it made.
fn trace() -> bool {
    static T: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("HALO3_EFFECT_TRACE").is_ok_and(|v| v == "1"));
    *T
}

pub struct EffectsPlugin;

impl Plugin for EffectsPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/particle.wesl");
        app.add_plugins(MaterialPlugin::<ParticleMaterial>::default())
            .init_map_resource::<EffectQueue>()
            .remove_on_map_unload::<PendingEffects>()
            .remove_on_map_unload::<Effects>()
            .add_systems(halo3_core::map::MapStartup, start)
            .add_systems(
                PostUpdate,
                (play.run_if(halo3_core::pause::running), draw)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(halo3_core::map::ready),
            );
    }
}

fn start(mut commands: Commands, pending: Option<ResMut<PendingEffects>>, mut images: ResMut<Assets<Image>>) {
    let Some(mut pending) = pending else { return };
    let Some(assets) = pending.0.take() else { return };
    let mut textures = HashMap::new();
    let mut mono = HashSet::new();
    for (tag, t) in &assets.textures {
        textures.insert(*tag, images.add(draw::image(t)));
        if t.mono {
            mono.insert(*tag);
        }
    }
    let sequences = assets.library.sequences.clone();
    commands.insert_resource(Effects {
        sim: sim::Sim::new(assets.library.clone()),
        gpu: Gpu {
            textures,
            mono,
            sequences,
            materials: HashMap::new(),
            frames: HashMap::new(),
            drawn: HashMap::new(),
            unknown: HashSet::new(),
            exposure: halo3_world::lighting::exposure(),
        },
    });
}

/// Starts the queued effects and moves everything on.
fn play(
    effects: Option<ResMut<Effects>>,
    mut queue: ResMut<EffectQueue>,
    sounds: Option<ResMut<ScriptSounds>>,
    time: Res<Time>,
) {
    let Some(mut fx) = effects else {
        queue.0.clear();
        return;
    };
    let fx = &mut *fx;
    for r in queue.0.drain(..) {
        let tag = fx.sim.lib.by_name.get(&r.name.to_ascii_lowercase()).copied();
        match tag.and_then(|t| fx.sim.spawn(t, r.at, r.frame, r.source)) {
            Some(_) => {
                if trace() {
                    info!("effect: {} at halo [{:.2}, {:.2}, {:.2}]", r.name, r.at.x, r.at.y, r.at.z);
                }
            }
            None => {
                if fx.gpu.unknown.insert(r.name.clone()) {
                    warn!("effect: {} is not among the map's effects read at load; not played", r.name);
                }
            }
        }
    }
    fx.sim.step(time.delta_secs().min(0.1));
    let lib = fx.sim.lib.clone();
    if let Some(mut sounds) = sounds {
        for (tag, at) in fx.sim.sounds.drain(..) {
            let name = lib.sound_names.get(&tag).cloned().unwrap_or_default();
            sounds.push(SoundRequest::Impulse {
                tag,
                name,
                at: Some(at),
                scale: 1.0,
            });
        }
    } else {
        fx.sim.sounds.clear();
    }
    fx.sim.shakes.clear();
    if trace() {
        for s in fx.sim.skipped.drain(..) {
            info!("effect: a {s} part is not played");
        }
    } else {
        fx.sim.skipped.clear();
    }
}

/// Rebuilds the meshes of everything playing.
#[allow(clippy::too_many_arguments)]
fn draw(
    mut commands: Commands,
    effects: Option<ResMut<Effects>>,
    cameras: Query<(&Transform, &Camera), With<Camera3d>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ParticleMaterial>>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(mut fx) = effects else { return };
    let Some((cam, _)) = cameras.iter().find(|(_, c)| c.is_active) else { return };
    let eye = draw::Eye {
        position: cam.translation,
        right: cam.right().as_vec3(),
        up: cam.up().as_vec3(),
        forward: cam.forward().as_vec3(),
    };
    let Effects { sim, gpu } = &mut *fx;
    let lib = sim.lib.clone();
    let mut live = HashSet::new();
    // Emitters.
    for s in &sim.systems {
        live.insert(s.id);
        let Some(prt) = lib.particles.get(&s.particle) else { continue };
        let origin = to_render(s.origin);
        let frames = match gpu.frames.get(&s.particle) {
            Some(f) => f.clone(),
            None => {
                let f = draw::frames(&lib, &gpu.sequences, s.particle);
                gpu.frames.insert(s.particle, f.clone());
                f
            }
        };
        let quads = draw::system_quads(&lib, s, &frames, &eye, origin);
        let key = (s.particle as u64) << 8;
        let material = material_for(gpu, &mut materials, key, &prt.shader);
        show(&mut commands, gpu, &mut meshes, &mut visibility, s.id, origin, quads, material);
    }
    // Contrails.
    for c in &sim.contrails {
        let id = c.id | 1 << 62;
        live.insert(id);
        let Some(def) = lib.contrails.get(&c.tag).and_then(|t| t.contrails.get(c.index)) else { continue };
        let origin = c.points.first().map_or(Vec3::ZERO, |p| to_render(p.position));
        let quads = draw::contrail_quads(&lib, c, &eye, origin);
        let key = (c.tag as u64) << 8 | (c.index as u64 + 1);
        let material = material_for(gpu, &mut materials, key, &def.shader);
        show(&mut commands, gpu, &mut meshes, &mut visibility, id, origin, quads, material);
    }
    gpu.drawn.retain(|id, (e, _)| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
    let _ = to_halo;
}

fn material_for(
    fx: &mut Gpu,
    materials: &mut Assets<ParticleMaterial>,
    key: u64,
    shader: &blam_texture::Shader,
) -> Handle<ParticleMaterial> {
    if let Some(m) = fx.materials.get(&key) {
        return m.clone();
    }
    let tex = |usage: &str| shader.map(usage).and_then(|m| m.bitmap).and_then(|b| fx.textures.get(&b).cloned());
    let alpha_tag = shader.map("alpha_map").and_then(|m| m.bitmap);
    let mono = alpha_tag.is_some_and(|b| fx.mono.contains(&b));
    let black_point = shader.option("black_point") == Some("on");
    let m = materials.add(ParticleMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            unlit: true,
            alpha_mode: draw::alpha_mode(shader),
            cull_mode: None,
            double_sided: true,
            fog_enabled: false,
            ..default()
        },
        extension: ParticleShading {
            mode: Vec4::new(draw::albedo_mode(shader), mono as u32 as f32, black_point as u32 as f32, fx.exposure),
            base: tex("base_map"),
            alpha: tex("alpha_map"),
            palette: tex("palette"),
        },
    });
    fx.materials.insert(key, m.clone());
    m
}

#[allow(clippy::too_many_arguments)]
fn show(
    commands: &mut Commands,
    fx: &mut Gpu,
    meshes: &mut Assets<Mesh>,
    visibility: &mut Query<&mut Visibility>,
    id: u64,
    origin: Vec3,
    quads: draw::Quads,
    material: Handle<ParticleMaterial>,
) {
    let empty = quads.is_empty();
    match fx.drawn.get(&id) {
        Some((e, mesh)) => {
            if let Ok(mut v) = visibility.get_mut(*e) {
                let want = if empty { Visibility::Hidden } else { Visibility::Inherited };
                if *v != want {
                    *v = want;
                }
            }
            if !empty && let Some(mut m) = meshes.get_mut(mesh) {
                *m = quads.into_mesh();
            }
            commands.entity(*e).insert(Transform::from_translation(origin));
        }
        None => {
            if empty {
                return;
            }
            let mesh = meshes.add(quads.into_mesh());
            let e = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material),
                    Transform::from_translation(origin),
                    NoFrustumCulling,
                    NotShadowCaster,
                    NotShadowReceiver,
                    Name::new("effect particles"),
                ))
                .id();
            fx.drawn.insert(id, (e, mesh));
        }
    }
}
