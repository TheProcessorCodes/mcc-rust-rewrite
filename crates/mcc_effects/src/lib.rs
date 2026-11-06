//! Halo 3's own effects in the game: what the map's scripts and damaged
//! objects ask for (`mcc_core::effects::EffectQueue`) played from the
//! map's `effe` tags, as the game plays them: events one after another,
//! particle systems with the tags' functions for count, life, speed,
//! physics, size, colour and fade, drawn with the particles' own bitmaps
//! and render method options; contrails behind carried effects; sounds
//! through `mcc_audio`. `docs/format/effects.md` has the format and the
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

use mcc_core::coords::{to_halo, to_render};
use mcc_core::effects::EffectQueue;
use mcc_core::map::MapAppExt;
use mcc_core::script_sound::{ScriptSounds, SoundRequest};

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
    /// The decoded bitmaps, uploaded on first use.
    pixels: HashMap<u32, load::Rgba>,
    /// Uploaded: (bitmap, sampled as sRGB colour).
    textures: HashMap<(u32, bool), Handle<Image>>,
    mono: HashSet<u32>,
    sequences: HashMap<u32, Vec<blam_texture::bitmap::Sequence>>,
    materials: HashMap<u64, Handle<ParticleMaterial>>,
    frames: HashMap<u32, Vec<[f32; 4]>>,
    drawn: HashMap<u64, (Entity, Handle<Mesh>)>,
    lights: HashMap<u64, Entity>,
    /// Invisible draws of every material made at the start, so their
    /// pipelines are compiled before the first explosion needs them (bevy
    /// compiles them in the background; the first one would go off unseen),
    /// and when they are taken away (seconds of map time).
    warm: Vec<Entity>,
    warm_until: Option<f32>,
    unknown: HashSet<String>,
    exposure: f32,
}

/// A light's intensity in the lightmap's irradiance units: our choice,
/// so the plate explosion's flash (20 at its start) adds about one unit
/// (the trench's mean irradiance is 0.06) where it is.
const LIGHT_SCALE: f32 = 0.05;
/// How far a light reaches, in multiples of its size (`ligh +0x08`): our
/// reading; the falloff Halo uses is not decoded.
const LIGHT_RANGE_PER_SIZE: f32 = 4.0;

/// `MCC_EFFECT_TRACE=1`: every effect started, and what it made.
fn trace() -> bool {
    static T: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("MCC_EFFECT_TRACE").is_ok_and(|v| v == "1"));
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
            .add_systems(mcc_core::map::MapStartup, start)
            .add_systems(
                PostUpdate,
                (play.run_if(mcc_core::pause::running), draw)
                    .chain()
                    .before(bevy::transform::TransformSystems::Propagate)
                    .run_if(mcc_core::map::ready),
            );
    }
}

fn start(mut commands: Commands, pending: Option<ResMut<PendingEffects>>) {
    let Some(mut pending) = pending else { return };
    let Some(assets) = pending.0.take() else { return };
    let mono = assets.textures.iter().filter(|(_, t)| t.mono).map(|(k, _)| *k).collect();
    let sequences = assets.library.sequences.clone();
    commands.insert_resource(Effects {
        sim: sim::Sim::new(assets.library.clone()),
        gpu: Gpu {
            pixels: assets.textures,
            textures: HashMap::new(),
            mono,
            sequences,
            materials: HashMap::new(),
            frames: HashMap::new(),
            drawn: HashMap::new(),
            lights: HashMap::new(),
            warm: Vec::new(),
            warm_until: None,
            unknown: HashSet::new(),
            // Our choice, by eye: the particles' colour times 3. The
            // world's exposure (x8 on 120_halo) makes up for our lightmap
            // scale; on the particles it turned every fireball white, and
            // at 1 (the authored colour) they were faint. How Halo scales
            // its particles against its lightmaps is not decoded.
            // `MCC_EFFECT_EXPOSURE` overrides.
            exposure: std::env::var("MCC_EFFECT_EXPOSURE").ok().and_then(|v| v.parse().ok()).unwrap_or(3.0),
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
    mut images: ResMut<Assets<Image>>,
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
    match gpu.warm_until {
        None => {
            gpu.warm_until = Some(sim.time + 20.0);
            let at = eye.position + eye.forward * 10.0;
            let mut shaders: Vec<(u64, &blam_texture::Shader)> =
                lib.particles.iter().map(|(t, p)| ((*t as u64) << 8, &p.shader)).collect();
            for (t, c) in &lib.contrails {
                shaders.extend(c.contrails.iter().enumerate().map(|(i, c)| ((*t as u64) << 8 | (i as u64 + 1), &c.shader)));
            }
            for (key, shader) in shaders {
                let material = material_for(gpu, &mut materials, &mut images, key, shader);
                let mesh = meshes.add(draw::Quads::degenerate().into_mesh());
                let e = commands
                    .spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(at), NoFrustumCulling, NotShadowCaster, NotShadowReceiver))
                    .id();
                gpu.warm.push(e);
            }
        }
        Some(t) if sim.time > t && !gpu.warm.is_empty() => {
            for e in gpu.warm.drain(..) {
                commands.entity(e).despawn();
            }
        }
        _ => {}
    }
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
        let material = material_for(gpu, &mut materials, &mut images, key, &prt.shader);
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
        let material = material_for(gpu, &mut materials, &mut images, key, &def.shader);
        show(&mut commands, gpu, &mut meshes, &mut visibility, id, origin, quads, material);
    }
    // Light flashes: bevy point lights the world shader reads
    // (`lightmap.wesl` `effect_irradiance`).
    for f in &sim.flashes {
        let id = f.id | 1 << 61;
        live.insert(id);
        let Some(light) = lib.lights.get(&f.tag) else { continue };
        let t = (f.age / light.duration.max(0.05)).clamp(0.0, 1.0);
        let c = light.colour.function.colour(t, 0.0);
        let intensity = light.intensity.function.scalar(t, 0.0).max(0.0) * LIGHT_SCALE;
        let point = PointLight {
            color: Color::linear_rgb(c[0], c[1], c[2]),
            intensity: 4.0 * std::f32::consts::PI * intensity,
            range: light.size.max(0.5) * LIGHT_RANGE_PER_SIZE * blam_geometry::WORLD_UNIT_METRES,
            radius: 0.0,
            shadow_maps_enabled: false,
            ..default()
        };
        let at = Transform::from_translation(to_render(f.position));
        match gpu.lights.get(&id) {
            Some(e) => {
                commands.entity(*e).insert((point, at));
            }
            None => {
                let e = commands.spawn((point, at, Name::new("effect light"))).id();
                gpu.lights.insert(id, e);
            }
        }
    }
    gpu.lights.retain(|id, e| {
        let keep = live.contains(id);
        if !keep {
            commands.entity(*e).despawn();
        }
        keep
    });
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
    images: &mut Assets<Image>,
    key: u64,
    shader: &blam_texture::Shader,
) -> Handle<ParticleMaterial> {
    if let Some(m) = fx.materials.get(&key) {
        return m.clone();
    }
    // A palettized shader's base map holds palette coordinates, not
    // colour: it is sampled linear. Colour maps and palettes are sRGB.
    let palettized = matches!(draw::albedo_mode(shader) as u32, 2 | 3 | 5);
    let mut tex = |usage: &str, srgb: bool| -> Option<Handle<Image>> {
        let b = shader.map(usage).and_then(|m| m.bitmap)?;
        if let Some(h) = fx.textures.get(&(b, srgb)) {
            return Some(h.clone());
        }
        let h = images.add(draw::image(fx.pixels.get(&b)?, srgb));
        fx.textures.insert((b, srgb), h.clone());
        Some(h)
    };
    let (base, alpha, palette) = (tex("base_map", !palettized), tex("alpha_map", false), tex("palette", true));
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
            base,
            alpha,
            palette,
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
