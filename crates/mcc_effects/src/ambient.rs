//! The effects a map plays by itself, with no script asking: the effects
//! its placed objects carry (Valhalla's waterfall mist and spray, the
//! man cannons' glow) and its weather (Snowbound's snow, 120_halo's snow,
//! 010_jungle's rain, 100_citadel's dust).
//!
//! * **Placed.** A scenery, crate, machine or control object whose tag's
//!   attachments (`obje +0xBC`, 32 bytes: tag reference, marker name
//!   `+0x10`) name an `effe` plays it at that marker of its render model
//!   (none: its origin), looping, from the map's start. Only those within
//!   [`RANGE`] of the camera play; one that drifts out of range is let
//!   finish and starts again when the camera comes back.
//! * **Weather.** An atmosphere setting can name a weather effect
//!   (`skya` setting `+0x8C`, `docs/format/sky-and-atmosphere.md` 3). The
//!   setting of the cluster the camera is in (`mcc_core::zones::CameraAir`)
//!   plays its weather looping round the camera while that cluster sees
//!   the sky (`CameraOutside`; indoors it stops): the effect follows the
//!   camera, what it has already let fall stays where it is.
//!
//! `main` reads both while the map loads ([`Ambient::read`]) and adds
//! their effects to the ones it loads.

use bevy::prelude::*;

use blam_cache::CacheFile;
use blam_geometry::{Kind, Scene};
use mcc_core::coords::to_halo;
use mcc_core::zones::{CameraAir, CameraOutside};

use crate::Effects;

/// How near the camera (m) a placed effect must be to play. Ours: past it
/// the mist and sparks are too small to see, and every one played costs.
pub const RANGE: f32 = 150.0;
/// A playing one is let go only past this (m), so it does not flicker on
/// the edge.
const RANGE_OUT: f32 = RANGE * 1.2;
/// `obje +0xBC`: the object's attachments, 32 bytes each (tag reference
/// `+0x00`, marker name `+0x10`).
const OBJECT_ATTACHMENTS: usize = 0xBC;
const ATTACHMENT_SIZE: usize = 32;

/// An effect a placed object carries.
#[derive(Clone, Debug)]
pub struct Placed {
    /// The `effe` tag.
    pub effect: u32,
    /// Where, Halo world units, and its frame (x forward, z up).
    pub at: Vec3,
    pub frame: Mat3,
    /// The object's tag path, for logs.
    pub object: String,
}

/// What the map plays by itself, and what of it is playing.
#[derive(Resource, Clone, Debug, Default)]
pub struct Ambient {
    pub placed: Vec<Placed>,
    /// Each atmosphere setting's weather effect, by setting index.
    pub weather: Vec<Option<u32>>,
    /// The setting the camera's air falls back to (the map's world one).
    pub default_air: Option<u32>,
    /// The running instance of each placed effect.
    running: Vec<Option<u64>>,
    /// The weather playing: (effect tag, instance).
    raining: Option<(u32, u64)>,
}

impl Ambient {
    /// The placed effects and the weather of a Halo 3 map.
    pub fn read(cache: &CacheFile, scene: &Scene) -> Ambient {
        let placed = placed(cache, scene);
        if crate::trace() {
            for p in &placed {
                eprintln!("ambient: {} carries effect {} at halo {:.2}", p.object, p.effect, p.at);
            }
        }
        let (weather, default_air) = match blam_geometry::atmosphere::read(cache) {
            Ok(Some(a)) => {
                let default = a.world().and_then(|w| a.settings.iter().position(|s| std::ptr::eq(s, w))).map(|i| i as u32);
                (a.settings.iter().map(|s| s.weather).collect(), default)
            }
            Ok(None) => (Vec::new(), None),
            Err(e) => {
                eprintln!("weather not read: {e}");
                (Vec::new(), None)
            }
        };
        Ambient {
            running: vec![None; placed.len()],
            placed,
            weather,
            default_air,
            raining: None,
        }
    }

    /// The effect tags to load for it.
    pub fn roots(&self) -> Vec<u32> {
        let mut r: Vec<u32> = self.placed.iter().map(|p| p.effect).chain(self.weather.iter().flatten().copied()).collect();
        r.sort_unstable();
        r.dedup();
        r
    }
}

/// Every effect the scene's placed objects carry, where they carry it.
pub fn placed(cache: &CacheFile, scene: &Scene) -> Vec<Placed> {
    let mut out = Vec::new();
    // Per object tag: (effect, the attachment's frame in the object's).
    let mut tags: std::collections::HashMap<u32, Vec<(u32, Mat4)>> = std::collections::HashMap::new();
    for o in &scene.objects {
        if !matches!(o.kind, Kind::Scenery | Kind::Crate | Kind::Machine | Kind::Control) || o.starts_absent() {
            continue;
        }
        let carried = tags.entry(o.tag).or_insert_with(|| {
            let Some(tag) = cache.tags.get(o.tag) else { return Vec::new() };
            carried(cache, tag).unwrap_or_else(|e| {
                eprintln!("effects of {}: {e}", tag.name());
                Vec::new()
            })
        });
        for (effect, local) in carried.iter() {
            let m = o.transform * *local;
            let frame = Mat3::from_mat4(m);
            out.push(Placed {
                effect: *effect,
                at: m.w_axis.truncate(),
                frame: Mat3::from_cols(
                    frame.x_axis.normalize_or(Vec3::X),
                    frame.y_axis.normalize_or(Vec3::Y),
                    frame.z_axis.normalize_or(Vec3::Z),
                ),
                object: o.tag_path.clone(),
            });
        }
    }
    out
}

/// The effects an object tag carries and the frames they sit in, in the
/// object's own frame.
fn carried(cache: &CacheFile, object: &blam_cache::TagEntry) -> blam_geometry::Result<Vec<(u32, Mat4)>> {
    let base = cache.tag_offset(object)?;
    let mut effects = Vec::new();
    for a in cache.reader_at(base + OBJECT_ATTACHMENTS).block()?.elements(ATTACHMENT_SIZE) {
        let r = cache.reader_at(a).tag_ref()?;
        if r.class.as_str() != "effe" {
            continue;
        }
        let Some(effect) = r.index else { continue };
        let marker = cache.string(cache.reader_at(a + 0x10).string_id()?).unwrap_or("").to_string();
        effects.push((effect, marker));
    }
    if effects.is_empty() {
        return Ok(Vec::new());
    }
    let markers = match blam_geometry::objects::model_of(cache, object)? {
        Some(hlmt) => {
            let mode = cache.reader_at(cache.tag_offset(hlmt)?).tag_ref()?;
            match mode.index.and_then(|i| cache.tags.get(i)) {
                Some(mode) => blam_geometry::weapons::markers(cache, mode).unwrap_or_default(),
                None => Vec::new(),
            }
        }
        None => Vec::new(),
    };
    Ok(effects
        .into_iter()
        .map(|(effect, marker)| {
            // Every marker of that name would carry one; the first does here.
            let frame = markers.iter().find(|m| !marker.is_empty() && m.name == marker).map_or(Mat4::IDENTITY, |m| m.model);
            (effect, frame)
        })
        .collect())
}

/// Starts, stops and moves the map's own effects for the camera.
pub(crate) fn run(
    ambient: Option<ResMut<Ambient>>,
    effects: Option<ResMut<Effects>>,
    (air, outside): (Option<Res<CameraAir>>, Option<Res<CameraOutside>>),
    cameras: Query<(&GlobalTransform, &Camera), With<Camera3d>>,
) {
    let (Some(mut ambient), Some(mut fx)) = (ambient, effects) else { return };
    let Some((camera, _)) = cameras.iter().find(|(_, c)| c.is_active) else { return };
    let eye = camera.translation();
    let ambient = &mut *ambient;
    let sim = &mut fx.sim;
    for (p, running) in ambient.placed.iter().zip(ambient.running.iter_mut()) {
        let distance = mcc_core::coords::to_render(p.at).distance(eye);
        match *running {
            Some(id) if !sim.playing(id) => *running = None,
            Some(id) if distance > RANGE_OUT => {
                sim.stop(id);
                *running = None;
            }
            _ => {}
        }
        if running.is_none() && distance < RANGE {
            *running = sim.spawn_looping(p.effect, p.at, p.frame);
            if crate::trace() {
                info!("ambient: {} on {} at halo {:.2}", sim.lib.effect(p.effect).map_or("?", |e| e.name.as_str()), p.object, p.at);
            }
        }
    }
    // The weather of the camera's air.
    let air = air.and_then(|a| a.0).or(ambient.default_air);
    let want = air
        .and_then(|a| ambient.weather.get(a as usize).copied().flatten())
        .filter(|_| outside.is_none_or(|o| o.0));
    let at = to_halo(eye);
    match (ambient.raining, want) {
        (Some((tag, id)), Some(w)) if tag == w && sim.playing(id) => sim.move_to(id, at),
        (current, want) => {
            if let Some((_, id)) = current {
                sim.stop(id);
            }
            ambient.raining = want.and_then(|w| sim.spawn_looping(w, at, Mat3::IDENTITY).map(|id| (w, id)));
            if crate::trace() && let Some((w, _)) = ambient.raining {
                info!("ambient: weather {}", sim.lib.effect(w).map_or("?", |e| e.name.as_str()));
            }
        }
    }
}
