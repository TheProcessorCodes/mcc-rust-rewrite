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
//!   finish and starts again when the camera comes back. An effect at a
//!   man cannon or lift that blows some way is turned about the vertical
//!   to blow the way the lift pushes ([`aim_along_lift`]).
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
use blam_geometry::{Kind, LiftVolume, Scene};
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

/// How near a lift's marker or volume (wu) a placed effect must be to be
/// that lift's effect. Valhalla's sit 0.6 to 0.9 wu from their cannon's
/// marker, and 4 wu from the next cannon's.
const LIFT_NEAR: f32 = 1.5;

/// The frame of a lift's effect, turned about the vertical so the stream
/// its emitters blow (`direction`, the first emitter direction that is not
/// zero, in the effect's frame) goes the way the lift pushes. `None` when
/// the effect is no lift's, blows straight up, or already blows that way.
///
/// Valhalla's man cannon effect is carried by a null object beside the
/// cannon, with no marker; its emitters blow 50 degrees up along the
/// effect's x, the cannon's own angle, but in the object's frame that
/// points across the pad. The effect names the location `up`, which is no
/// marker of the object but a frame Halo makes itself; how Halo turns that
/// frame about the vertical is not decoded. Along the cannon's push is
/// our reading: it is the only way the stream follows the player's flight
/// on all four cannons (two tags, four headings).
pub fn aim_along_lift(at: Vec3, frame: Mat3, direction: Vec3, lifts: &[LiftVolume]) -> Option<Mat3> {
    let flat = |v: Vec3| Vec3::new(v.x, v.y, 0.0);
    let blows = flat(frame * direction);
    let lift = lifts
        .iter()
        .filter(|l| l.push.direction.0 != 0.0)
        .map(|l| (l, l.origin.distance(at).min(l.shape.distance(at))))
        .filter(|(_, d)| *d < LIFT_NEAR)
        .min_by(|a, b| a.1.total_cmp(&b.1))?
        .0;
    let pushes = flat(lift.direction * lift.push.direction.0.signum());
    if blows.length() < 0.1 || pushes.length() < 0.1 {
        return None;
    }
    let turn = blows.truncate().angle_to(pushes.truncate());
    (turn.abs() > 5f32.to_radians()).then(|| Mat3::from_rotation_z(turn) * frame)
}

/// The first emitter direction of `effect` that is not zero, in the
/// effect's frame.
fn stream(cache: &CacheFile, effect: u32) -> Option<Vec3> {
    let e = blam_effect::read_effect(cache, effect).ok()?;
    e.events
        .iter()
        .flat_map(|ev| &ev.systems)
        .flat_map(|s| &s.emitters)
        .map(|em| Vec3::from(em.direction.a))
        .find(|d| d.length_squared() > 1e-6)
}

/// Every effect the scene's placed objects carry, where they carry it.
pub fn placed(cache: &CacheFile, scene: &Scene) -> Vec<Placed> {
    let mut out = Vec::new();
    // The lifts: `main` has read them into the scene by now; a test's
    // scene may not have them.
    let read;
    let lifts: &[LiftVolume] = if scene.lifts.is_empty() {
        read = blam_geometry::objects::lifts(cache, scene);
        &read
    } else {
        &scene.lifts
    };
    let mut streams: std::collections::HashMap<u32, Option<Vec3>> = std::collections::HashMap::new();
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
            let frame = Mat3::from_cols(
                frame.x_axis.normalize_or(Vec3::X),
                frame.y_axis.normalize_or(Vec3::Y),
                frame.z_axis.normalize_or(Vec3::Z),
            );
            let at = m.w_axis.truncate();
            let aimed = if lifts.is_empty() {
                None
            } else {
                streams
                    .entry(*effect)
                    .or_insert_with(|| stream(cache, *effect))
                    .and_then(|d| aim_along_lift(at, frame, d, lifts))
            };
            out.push(Placed {
                effect: *effect,
                at,
                frame: aimed.unwrap_or(frame),
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

#[cfg(test)]
mod tests {
    use super::*;
    use blam_geometry::{LiftPush, LiftShape};

    fn lift(origin: Vec3, direction: Vec3) -> LiftVolume {
        LiftVolume {
            name: "cannon".into(),
            shape: LiftShape::Box {
                centre: origin,
                axes: [Vec3::X, Vec3::Y, Vec3::Z],
                half: Vec3::splat(0.3),
            },
            origin,
            direction: direction.normalize(),
            push: LiftPush {
                direction: (60.0, 11.0),
                ..Default::default()
            },
        }
    }

    /// Valhalla's layout: the effect blows 50 degrees up along its x, the
    /// cannon beside it throws along +y.
    #[test]
    fn a_cannons_effect_is_turned_to_blow_along_its_push() {
        let blows = Vec3::new(0.6427876, 0.0, 0.7660445);
        let pushes = Vec3::new(0.0, 0.6427876, 0.7660445);
        let lifts = [lift(Vec3::new(0.0, 0.6, 0.6), pushes), lift(Vec3::new(4.0, 0.0, 0.0), Vec3::new(-1.0, 0.0, 1.0))];
        let frame = aim_along_lift(Vec3::ZERO, Mat3::IDENTITY, blows, &lifts).expect("turned");
        assert!((frame * blows - pushes).length() < 1e-4, "{}", frame * blows);
        // Still upright.
        assert!((frame.z_axis - Vec3::Z).length() < 1e-5);
        // Already along it: left alone.
        assert!(aim_along_lift(Vec3::ZERO, frame, blows, &lifts).is_none());
        // No lift near, a stream straight up, or a lift straight up: left alone.
        assert!(aim_along_lift(Vec3::new(0.0, 20.0, 0.0), Mat3::IDENTITY, blows, &lifts).is_none());
        assert!(aim_along_lift(Vec3::ZERO, Mat3::IDENTITY, Vec3::Z, &lifts).is_none());
        assert!(aim_along_lift(Vec3::ZERO, Mat3::IDENTITY, blows, &[lift(Vec3::ZERO, Vec3::Z)]).is_none());
    }
}
