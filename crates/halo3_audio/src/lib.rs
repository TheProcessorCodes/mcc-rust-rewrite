//! Halo 3's own sound in the viewer: the map's ambience (each cluster's
//! background sound with its random details, sound scenery placed in the
//! world), its music by hand, board sounds while skating, weapon sounds
//! (`weapons.rs`) and the dirt bike's engine (`engine.rs`).
//! `docs/format/sound.md` has the data, `docs/viewer.md` "Sound" the rules.
//!
//! Music (`music.rs`) follows the map's scripts when they run, and the
//! keys: M music on / off, Shift+M next track, N alternate loop, K mute,
//! `-` / `=` master volume, H the bike's engine sound on / off;
//! the pause menu's Sound tab sets the master, music, effects, ambience and
//! dialogue volumes (`levels.rs`, saved next to the exe), its Dirt bike tab
//! the engine's. `HALO3_AUDIO=0` turns sound off; it is off in `--shot`
//! mode, where `HALO3_AUDIO_TRACE=1` runs the logic without output and
//! logs which emitters are active at each view.

pub mod engine;
pub mod halo2;
mod levels;
mod library;
mod mixer;
mod music;
mod weapons;

use std::collections::HashMap;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use bevy::audio::AddAudioSource;
use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use blam_cache::CacheFile;

use halo3_core::fly_camera::FlyCamera;
use halo3_core::zones::{LoadedBsps, Zones};
use library::{Clips, Loaded, SoundData};
use mixer::{Bus, Clip, Mixer, MixerAsset, Play, VoiceId};

pub use levels::{Channel, SoundLevels};

/// The mixer and the sound systems; the map's own sounds come in with
/// [`install`] when the map loads (`halo3_core::map`).
pub struct HaloAudioPlugin {
    /// Play through the sound device (false: `--shot` trace mode).
    pub output: bool,
    /// Log the active emitters as the camera moves.
    pub trace: bool,
    /// `--music NAME|N`: a music track to start with.
    pub music: Option<String>,
}

/// A map's sound source: a Halo 3 cache file, or for a map with none
/// (Halo 2) its ambience, decoded by the caller on a thread (`halo2.rs`).
pub enum MapSound {
    Halo3(&'static CacheFile),
    Plain(halo2::PlainLoader),
}

/// Starts loading a map's sounds; their systems run from then on. Does
/// nothing without [`HaloAudioPlugin`].
pub fn install(world: &mut World, sound: MapSound) {
    if !world.contains_resource::<Mixer>() {
        return;
    }
    match sound {
        MapSound::Halo3(cache) => world.insert_resource(Loader(Mutex::new(Some(library::load(cache))))),
        MapSound::Plain(loader) => world.insert_resource(halo2::PlainState::start(loader)),
    }
}

/// Whether sound is wanted: not in `--shot` mode, not with `HALO3_AUDIO=0`.
pub fn wanted(shot: bool) -> bool {
    !shot && std::env::var("HALO3_AUDIO").map_or(true, |v| v != "0")
}

/// `HALO3_AUDIO_TRACE=1`.
pub fn trace_wanted() -> bool {
    std::env::var("HALO3_AUDIO_TRACE").is_ok_and(|v| v == "1")
}

impl Plugin for HaloAudioPlugin {
    fn build(&self, app: &mut App) {
        let levels = SoundLevels::load();
        info!("sound: volumes {}", levels.describe());
        app.insert_resource(Mixer::default())
            .insert_resource(levels)
            .init_resource::<levels::SaveDelay>()
            .add_systems(Update, levels::apply.after(VolumeKeys));
        // The dirt bike's engine (`engine.rs`).
        app.init_map_resource::<engine::EngineSound>().add_systems(
            Update,
            engine::bike_engine.before(VolumeKeys).run_if(halo3_core::map::ready),
        );
        // A map's sounds go when it is unloaded (`halo3_core::map`).
        app.remove_on_map_unload::<Loader>()
            .remove_on_map_unload::<Sounds>()
            .remove_on_map_unload::<halo2::PlainState>()
            .on_map_unload(unload_sounds);
        // Halo 3: the sound library loading from the cache file.
        app.insert_resource(Trace {
            on: self.trace,
            ..Default::default()
        })
        .insert_resource(music::MusicPlayer::new(self.music.clone()))
        .init_map_resource::<music::ClassGains>()
        .init_map_resource::<halo3_core::script_sound::ScriptSounds>()
        .init_map_resource::<Listener>()
        .init_map_resource::<Ambience>()
        .init_map_resource::<BoardSounds>()
        .init_map_resource::<weapons::WeaponVoices>()
        // Registered by `WeaponsPlugin` too; harmless twice, and the
        // sound still builds without it.
        .add_message::<halo3_core::weapons::WeaponEvent>()
        .add_message::<halo3_core::weapons::WeaponImpact>()
        .add_systems(
            Update,
            (
                receive,
                listener,
                background,
                details,
                scenery,
                music::script_requests,
                music::hand_controls,
                music::update,
                board,
                weapons::weapon_sounds,
                volume_keys.in_set(VolumeKeys),
                trace,
                trace_levels,
            )
                .chain()
                .after(halo3_core::zones::ZoneKeys)
                .run_if(resource_exists::<Loader>)
                .run_if(halo3_core::map::ready),
        );
        // Halo 2: the plain ambience.
        app.add_systems(
            Update,
            (listener, halo2::receive, halo2::play, volume_keys.in_set(VolumeKeys))
                .chain()
                .run_if(resource_exists::<halo2::PlainState>)
                .run_if(halo3_core::map::ready),
        );
        if self.output {
            app.add_audio_source::<MixerAsset>()
                .add_systems(Startup, start_output);
        } else {
            // No device: mix in real time anyway, so voices start and end
            // as they would, and `HALO3_AUDIO_WAV=path` keeps what was
            // heard (first 60 s) as a WAV for checking.
            app.insert_resource(Headless {
                wav: std::env::var_os("HALO3_AUDIO_WAV").map(std::path::PathBuf::from),
                ..Default::default()
            })
            .add_systems(Update, headless_mix.after(levels::apply));
        }
    }
}

/// Silences the map's voices and starts the music and trace state over.
fn unload_sounds(world: &mut World) {
    world.resource::<Mixer>().stop_all();
    if let Some(mut music) = world.get_resource_mut::<music::MusicPlayer>() {
        let fresh = music.fresh();
        *music = fresh;
    }
    if let Some(mut trace) = world.get_resource_mut::<Trace>() {
        let on = trace.on;
        *trace = Trace { on, ..Default::default() };
    }
}

fn start_output(mut commands: Commands, mixer: Res<Mixer>, mut assets: ResMut<Assets<MixerAsset>>) {
    let handle = assets.add(MixerAsset(mixer.clone()));
    commands.spawn((AudioPlayer(handle), PlaybackSettings::ONCE));
}

#[derive(Resource, Default)]
struct Headless {
    wav: Option<std::path::PathBuf>,
    kept: Vec<f32>,
    owed: f64,
    written: usize,
}

fn headless_mix(time: Res<Time>, mixer: Res<Mixer>, mut h: ResMut<Headless>) {
    h.owed += time.delta_secs_f64() * mixer::RATE as f64;
    // At most a quarter second per frame (the first frames take seconds).
    let frames = (h.owed as usize).min(mixer::RATE as usize / 4);
    h.owed = (h.owed - frames as f64).min(mixer::RATE as f64);
    if frames == 0 {
        return;
    }
    let mut buf = vec![0.0; frames * 2];
    mixer.render(&mut buf);
    if h.wav.is_some() && h.kept.len() < mixer::RATE as usize * 2 * 60 {
        h.kept.extend_from_slice(&buf);
        // Rewritten every two seconds of audio, so an exit at any time
        // leaves a file.
        if h.kept.len() >= h.written + mixer::RATE as usize * 4 {
            h.written = h.kept.len();
            let pcm = blam_sound::Pcm {
                rate: mixer::RATE,
                channels: 2,
                samples: h.kept.clone(),
            };
            if let Some(path) = &h.wav
                && let Err(e) = blam_sound::write_wav(path, &pcm)
            {
                warn!("sound: {}: {e}", path.display());
            }
        }
    }
}

#[derive(Resource)]
struct Loader(Mutex<Option<Receiver<Result<Loaded, String>>>>);

/// The map's sounds once loaded.
#[derive(Resource, Clone)]
struct Sounds {
    data: Arc<SoundData>,
    clips: Arc<Clips>,
}

/// Where the ears are, in Halo world units (Z up).
#[derive(Resource, Default, Clone, Copy)]
struct Listener {
    position: Vec3,
    right: Vec3,
    forward: Vec3,
    valid: bool,
}

impl Listener {
    /// Left and right gains for a source at `at` with overall `gain`:
    /// equal-power panning scaled so a source straight ahead plays at
    /// `gain` in both ears, and never fully out of one ear.
    fn pan(&self, at: Vec3, gain: f32) -> [f32; 2] {
        let d = at - self.position;
        let x = if d.length_squared() < 1e-4 {
            0.0
        } else {
            d.normalize().dot(self.right) * 0.8
        };
        let theta = (x + 1.0) * std::f32::consts::FRAC_PI_4;
        let s = std::f32::consts::SQRT_2;
        [
            gain * (theta.cos() * s).min(1.0),
            gain * (theta.sin() * s).min(1.0),
        ]
    }
}

/// Full volume within `near`, falling to silence at `far` (world units),
/// as the square of the remaining fraction.
fn attenuation(distance: f32, (near, far): (f32, f32)) -> f32 {
    if distance <= near {
        1.0
    } else if distance >= far || far <= near {
        0.0
    } else {
        let t = (far - distance) / (far - near);
        t * t
    }
}

fn db(d: f32) -> f32 {
    10f32.powf(d / 20.0)
}

/// The bus a sound of this class plays on: music (32, and 38, cinematic
/// music), dialogue (19 unit dialogue, 46 to 48 mission and cinematic
/// lines), ambience (33, 34: ambient loops, machinery, terminals), else
/// effects (by the sounds that use each class on 120_halo, `sound.md` 9).
/// Backgrounds, their details and sound scenery are on the ambience bus
/// whatever their class.
fn bus_of_class(class: i32) -> Bus {
    match class {
        32 | 38 => Bus::Music,
        19 | 46..=48 => Bus::Dialogue,
        33 | 34 => Bus::Ambience,
        _ => Bus::Effects,
    }
}

fn receive(mut commands: Commands, loader: Res<Loader>, sounds: Option<Res<Sounds>>) {
    if sounds.is_some() {
        return;
    }
    let mut slot = loader.0.lock().unwrap();
    let Some(rx) = slot.as_ref() else {
        return;
    };
    match rx.try_recv() {
        Ok(Ok(loaded)) => {
            commands.insert_resource(Sounds {
                data: loaded.data,
                clips: loaded.clips,
            });
            *slot = None;
        }
        Ok(Err(e)) => {
            warn!("sound: off ({e})");
            *slot = None;
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => {}
        Err(_) => *slot = None,
    }
}

fn listener(cameras: Query<&GlobalTransform, With<FlyCamera>>, mut l: ResMut<Listener>) {
    let Ok(c) = cameras.single() else {
        return;
    };
    l.position = halo3_core::coords::to_halo(c.translation());
    l.right = halo3_core::coords::to_halo_dir(*c.right()).normalize_or_zero();
    l.forward = halo3_core::coords::to_halo_dir(*c.forward()).normalize_or_zero();
    l.valid = true;
}

/// A looping sound being played as an ambience layer.
#[derive(Default)]
struct Layer {
    /// One voice per track.
    voices: Vec<Option<VoiceId>>,
    /// 0..1 fade, towards 1 while wanted.
    fade: f32,
    wanted: bool,
    /// Seconds until each detail plays next.
    timers: Vec<f32>,
}

/// A one-shot detail placed around the listener.
struct DetailVoice {
    id: VoiceId,
    at: Option<Vec3>,
    gain: f32,
    layer: u32,
}

#[derive(Resource, Default)]
struct Ambience {
    layers: HashMap<u32, Layer>,
    /// The camera's (BSP, cluster, palette entry).
    place: Option<(u32, u32, Option<u32>)>,
    details: Vec<DetailVoice>,
    scenery: Vec<Option<VoiceId>>,
    /// (placement, distance, gain) of the scenery playing, for the trace.
    heard: Vec<(usize, f32, f32)>,
    rng: u32,
}

impl Ambience {
    fn random(&mut self) -> f32 {
        if self.rng == 0 {
            self.rng = 0x9E37_79B9;
        }
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }
    fn between(&mut self, (a, b): (f32, f32)) -> f32 {
        a + (b - a) * self.random()
    }
}

/// The BSP, cluster and acoustics palette entry the camera is in.
fn place_of(zones: &Zones, mask: u32, p: Vec3) -> Option<(u32, u32, Option<u32>)> {
    zones
        .layout
        .bsps
        .iter()
        .filter(|b| mask & (1 << b.index) != 0)
        .find_map(|b| {
            let c = b.locator.as_ref()?.cluster(p)?;
            Some((b.index, c, b.clusters.get(c as usize)?.acoustics))
        })
}

/// The track's loop clips and gain, once decoded.
fn track_loop(sounds: &Sounds, track: &blam_sound::tags::Track) -> Option<(Vec<Arc<Clip>>, f32)> {
    let snd = sounds.data.sounds.get(&track.looped?)?;
    let clips = sounds.clips.all(&snd.permutations)?;
    Some((clips, db(track.gain_db + snd.gain_db)))
}

fn background(
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    zones: Res<Zones>,
    loaded: Res<LoadedBsps>,
    l: Res<Listener>,
    mixer: Res<Mixer>,
    gains: Res<music::ClassGains>,
    mut amb: ResMut<Ambience>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    if !l.valid {
        return;
    }
    let mask = if loaded.shown == 0 {
        loaded.mask
    } else {
        loaded.shown
    };
    // A script's `sound_class_set_gain amb` (1 otherwise).
    let class_gain = gains.get("amb");
    let place = place_of(&zones, mask, l.position);
    // Indoors-outdoors changes only when the camera is in some cluster;
    // outside every cluster the last background stays.
    if place.is_some() {
        amb.place = place;
    }
    let mut wanted: Vec<u32> = Vec::new();
    if let Some((bsp, _, Some(entry))) = amb.place
        && let Some(a) = sounds
            .data
            .map
            .bsps
            .iter()
            .find(|b| b.bsp == bsp)
            .and_then(|b| b.palette.get(entry as usize))
    {
        wanted.extend([a.background, a.inside].into_iter().flatten());
    }
    for w in &wanted {
        let details = sounds.data.loops.get(w).map_or(0, |l| l.details.len());
        let layer = amb.layers.entry(*w).or_default();
        if layer.timers.len() != details {
            layer.timers = vec![-1.0; details];
        }
    }
    let dt = time.delta_secs();
    let mut gone = Vec::new();
    for (tag, layer) in amb.layers.iter_mut() {
        layer.wanted = wanted.contains(tag);
        let Some(looping) = sounds.data.loops.get(tag) else {
            continue;
        };
        if layer.voices.len() != looping.tracks.len() {
            layer.voices = vec![None; looping.tracks.len()];
        }
        // About a second to cross-fade, as the game does between clusters.
        let rate = dt / 1.0;
        layer.fade = if layer.wanted {
            (layer.fade + rate).min(1.0)
        } else {
            (layer.fade - rate).max(0.0)
        };
        for (t, track) in looping.tracks.iter().enumerate() {
            if layer.voices[t].is_none()
                && layer.wanted
                && let Some((clips, _)) = track_loop(&sounds, track)
                && !clips.is_empty()
            {
                layer.voices[t] = Some(mixer.play(Play {
                    bus: Bus::Ambience,
                    queue: Vec::new(),
                    looping: clips,
                    gain: [0.0; 2],
                    rate: 1.0,
                }));
            }
            if let Some(id) = layer.voices[t] {
                let g = sounds
                    .data
                    .sounds
                    .get(&track.looped.unwrap_or(u32::MAX))
                    .map_or(1.0, |s| db(track.gain_db + s.gain_db));
                mixer.set_gain(id, [g * layer.fade * class_gain; 2]);
            }
        }
        if !layer.wanted && layer.fade <= 0.0 {
            for id in layer.voices.iter().flatten() {
                mixer.stop(*id);
            }
            gone.push(*tag);
        }
    }
    for g in gone {
        amb.layers.remove(&g);
    }
}

/// The random one-shots of the backgrounds playing: birds, gusts.
fn details(
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    l: Res<Listener>,
    mixer: Res<Mixer>,
    mut amb: ResMut<Ambience>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    let dt = time.delta_secs();
    let tags: Vec<u32> = amb.layers.keys().copied().collect();
    for tag in tags {
        let Some(looping) = sounds.data.loops.get(&tag) else {
            continue;
        };
        for (i, d) in looping.details.iter().enumerate() {
            let (fade, timer) = {
                let layer = amb.layers.get(&tag).unwrap();
                (layer.fade, layer.timers.get(i).copied().unwrap_or(1.0))
            };
            let mut next = timer - dt;
            if timer < 0.0 {
                // First time: somewhere in the period, not all at once.
                next = amb.between((0.0, d.period.1.max(1.0)));
            } else if next <= 0.0 {
                next = amb.between(d.period).max(0.5);
                'fire: {
                    let Some(snd) = d.sound.and_then(|s| sounds.data.sounds.get(&s)) else {
                        break 'fire;
                    };
                    if fade < 0.5 || snd.permutations.is_empty() {
                        break 'fire;
                    }
                    let pick = (amb.random() * snd.permutations.len() as f32) as usize;
                    let Some(clip) = sounds
                        .clips
                        .get(snd.permutations[pick.min(snd.permutations.len() - 1)])
                    else {
                        // Still decoding: try again in a second.
                        next = 1.0;
                        break 'fire;
                    };
                    let gain = db(d.gain_db + snd.gain_db);
                    let yaw = amb.between(d.yaw);
                    let pitch = amb.between(d.pitch) * 0.3;
                    let dist = amb.between(d.distance);
                    let fwd = Vec3::new(l.forward.x, l.forward.y, 0.0).normalize_or(Vec3::X);
                    let side = Vec3::new(-fwd.y, fwd.x, 0.0);
                    let dir =
                        (fwd * yaw.cos() + side * yaw.sin()) * pitch.cos() + Vec3::Z * pitch.sin();
                    let at = (clip.channels == 1).then(|| l.position + dir * dist);
                    let g = at.map_or([gain; 2], |p| l.pan(p, gain));
                    let id = mixer.play(Play {
                        bus: Bus::Ambience,
                        queue: vec![clip],
                        looping: Vec::new(),
                        gain: g,
                        rate: 1.0,
                    });
                    amb.details.push(DetailVoice {
                        id,
                        at,
                        gain,
                        layer: tag,
                    });
                }
            }
            if let Some(t) = amb.layers.get_mut(&tag).and_then(|l| l.timers.get_mut(i)) {
                *t = next;
            }
        }
    }
    let layers: HashMap<u32, f32> = amb.layers.iter().map(|(t, l)| (*t, l.fade)).collect();
    amb.details.retain(|d| mixer.is_playing(d.id));
    for d in &amb.details {
        let fade = layers.get(&d.layer).copied().unwrap_or(0.0);
        let g = d.at.map_or([d.gain; 2], |p| l.pan(p, d.gain));
        mixer.set_gain(d.id, [g[0] * fade, g[1] * fade]);
    }
}

fn scenery(
    sounds: Option<Res<Sounds>>,
    loaded: Res<LoadedBsps>,
    l: Res<Listener>,
    mixer: Res<Mixer>,
    gains: Res<music::ClassGains>,
    mut amb: ResMut<Ambience>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    if !l.valid {
        return;
    }
    let placed = &sounds.data.map.scenery;
    if amb.scenery.len() != placed.len() {
        amb.scenery = vec![None; placed.len()];
    }
    let mask = if loaded.shown == 0 {
        loaded.mask
    } else {
        loaded.shown
    };
    // Halo caps how many copies of one sound play at once (by sound
    // class; not decoded): here the loudest MAX_PER_SOUND placements of
    // each looping sound play and the rest stay silent. Without it
    // 120_halo's eight falling-plate loops summed to -5 dBFS.
    const MAX_PER_SOUND: usize = 2;
    let mut ranked: Vec<(u32, f32, usize)> = Vec::new();
    for (i, s) in placed.iter().enumerate() {
        let Some(tag) = s.looping else { continue };
        let snd = sounds
            .data
            .loops
            .get(&tag)
            .and_then(|lp| lp.tracks.first())
            .and_then(|t| t.looped)
            .and_then(|t| sounds.data.sounds.get(&t));
        let Some(snd) = snd else { continue };
        let bounds = s.distance.unwrap_or(snd.bounds);
        let d = Vec3::from(s.position).distance(l.position);
        let present = s.origin_bsp.is_none_or(|b| b < 32 && mask & (1 << b) != 0);
        if present && d < bounds.1.max(20.0) + 2.0 {
            ranked.push((tag, attenuation(d, bounds), i));
        }
    }
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    let mut allowed = vec![false; placed.len()];
    let mut run = (u32::MAX, 0);
    for (tag, _, i) in ranked {
        run = if run.0 == tag { (tag, run.1 + 1) } else { (tag, 1) };
        allowed[i] = run.1 <= MAX_PER_SOUND;
    }
    let mut heard = Vec::new();
    for (i, s) in placed.iter().enumerate() {
        let looping = s.looping.and_then(|t| sounds.data.loops.get(&t));
        let track = looping.and_then(|lp| lp.tracks.first());
        let snd = track
            .and_then(|t| t.looped)
            .and_then(|t| sounds.data.sounds.get(&t));
        let (Some(track), Some(snd), true) = (track, snd, allowed[i]) else {
            if let Some(id) = amb.scenery[i].take() {
                mixer.stop(id);
            }
            continue;
        };
        let mut bounds = s.distance.unwrap_or(snd.bounds);
        if bounds.1 <= 0.0 {
            bounds = (2.0, 20.0);
        }
        let mut at = Vec3::from(s.position);
        if s.height > 0.0 {
            // A vertical cylinder: as near as the listener's height allows.
            at.z = l.position.z.clamp(at.z, at.z + s.height);
        }
        let distance = at.distance(l.position);
        // Sound scenery 9 dB below its class level (`gain_db` already has
        // the class's -12 dB): 120_halo's falling plates are authored hot
        // (-7 dBFS RMS loops at 0 dB track gain), and at the class level
        // alone two of them sit within 3 dB of the Warthog run's music,
        // which in the game plays clearly over them. Our own choice, with
        // Halo's distance curve and voice limits still unknown.
        const SCENERY_GAIN: f32 = 0.35;
        let gain = SCENERY_GAIN * db(track.gain_db + snd.gain_db) * attenuation(distance, bounds) * gains.get("amb");
        if distance < bounds.1 + 2.0 {
            if amb.scenery[i].is_none()
                && let Some(clips) = sounds.clips.all(&snd.permutations)
                && !clips.is_empty()
            {
                amb.scenery[i] = Some(mixer.play(Play {
                    bus: Bus::Ambience,
                    queue: Vec::new(),
                    looping: clips,
                    gain: [0.0; 2],
                    rate: 1.0,
                }));
            }
            if let Some(id) = amb.scenery[i] {
                mixer.set_gain(id, l.pan(at, gain));
            }
            heard.push((i, distance, gain));
        } else if distance > bounds.1 + 6.0
            && let Some(id) = amb.scenery[i].take()
        {
            mixer.stop(id);
        }
    }
    amb.heard = heard;
}

#[derive(Resource, Default)]
struct BoardSounds {
    roll: Option<VoiceId>,
    grind: Option<VoiceId>,
    last_state: String,
    air: f32,
    rng: u32,
}

/// Rolling, grinding and landing while skating, from Halo's own samples
/// (Skate 3's audio is in the user's dump, `data\audio\*.big`, but nothing
/// reads it yet): a physics tire rolling on hard ground for the roll, a
/// vehicle scrape for grinds, a wooden pallet hit plus a tire bounce for
/// landings.
fn board(
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    motion: Option<Res<halo3_core::skate::SkateMotion>>,
    mixer: Res<Mixer>,
    mut b: ResMut<BoardSounds>,
) {
    let (Some(sounds), Some(motion)) = (sounds, motion) else {
        return;
    };
    let g = &sounds.data.gameplay;
    let state = motion.state.as_str();
    let air = motion.riding && state.contains("Air");
    let grind = motion.riding && state.contains("Grind");
    let rolling = motion.riding
        && !air
        && !grind
        && state.contains("Ground")
        && !state.contains("Biped")
        && !state.contains("Wipeout");
    let speed = motion.speed;

    let roll_gain = if rolling {
        (speed / 6.0).clamp(0.0, 1.0) * 0.5
    } else {
        0.0
    };
    match b.roll {
        None if roll_gain > 0.01 => {
            if let Some(clips) = sounds.clips.all(&g.roll)
                && !clips.is_empty()
            {
                b.roll = Some(mixer.play(Play {
                    bus: Bus::Effects,
                    queue: Vec::new(),
                    looping: clips,
                    gain: [0.0; 2],
                    rate: 1.0,
                }));
            }
        }
        Some(id) if !motion.riding => {
            mixer.stop(id);
            b.roll = None;
        }
        Some(id) => {
            mixer.set_gain(id, [roll_gain; 2]);
            // Smaller, harder wheels than the tire that was recorded, so a
            // little higher, rising with speed.
            mixer.set_rate(id, (0.9 + speed / 14.0).min(1.7));
        }
        None => {}
    }
    let grind_gain = if grind { 0.45 } else { 0.0 };
    match b.grind {
        None if grind => {
            if let Some(clips) = sounds.clips.all(&g.grind)
                && !clips.is_empty()
            {
                b.grind = Some(mixer.play(Play {
                    bus: Bus::Effects,
                    queue: Vec::new(),
                    looping: clips,
                    gain: [grind_gain; 2],
                    rate: 1.0,
                }));
            }
        }
        Some(id) if !grind => {
            mixer.stop(id);
            b.grind = None;
        }
        Some(id) => mixer.set_rate(id, 0.8 + speed / 20.0),
        None => {}
    }
    if air {
        b.air += time.delta_secs();
    } else {
        if b.air > 0.25 && (rolling || grind) {
            // The deck slap, then the wheels under it, each a random
            // permutation at a slightly random pitch; both sources are
            // bigger than a skateboard, so they play a little high.
            let gain = (0.3 + b.air * 0.25).min(0.75);
            for (set, level, pitch) in [(&g.land, 1.0, 1.2), (&g.land_wheels, 0.6, 1.35)] {
                if set.is_empty() {
                    continue;
                }
                b.rng = b.rng.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let pick = set[(b.rng >> 16) as usize % set.len()];
                let jitter = ((b.rng >> 8) & 0xff) as f32 / 255.0 * 0.15 - 0.075;
                if let Some(clip) = sounds.clips.get(pick) {
                    mixer.play(Play {
                        bus: Bus::Effects,
                        queue: vec![clip],
                        looping: Vec::new(),
                        gain: [gain * level; 2],
                        rate: pitch + jitter,
                    });
                }
            }
        }
        b.air = 0.0;
    }
    // Ask early so the first roll and landing are ready.
    if motion.riding && b.last_state.is_empty() {
        let _ = sounds.clips.all(&g.land);
        let _ = sounds.clips.all(&g.land_wheels);
        let _ = sounds.clips.all(&g.roll);
        let _ = sounds.clips.all(&g.grind);
    }
    if b.last_state != state {
        b.last_state = state.to_string();
    }
}

/// [`volume_keys`], which runs in both the Halo 3 and the Halo 2 chain: a
/// system added twice cannot be ordered against by name, so the others
/// order against this set.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
struct VolumeKeys;

/// K mutes, `-` / `=` step the master volume by 10 % (the pause menu's
/// Master slider shows the same value).
fn volume_keys(keys: Res<ButtonInput<KeyCode>>, mut v: ResMut<SoundLevels>) {
    let mut changed = false;
    if keys.just_pressed(KeyCode::KeyK) {
        v.muted = !v.muted;
        changed = true;
    }
    let step = if keys.just_pressed(KeyCode::Minus) || keys.just_pressed(KeyCode::NumpadSubtract) {
        -0.1
    } else if keys.just_pressed(KeyCode::Equal) || keys.just_pressed(KeyCode::NumpadAdd) {
        0.1
    } else {
        0.0
    };
    if step != 0.0 {
        let m = v.master + step;
        v.set(Channel::Master, m);
        changed = true;
    }
    if changed {
        info!(
            "sound: volume {:.0} %{}",
            v.master * 100.0,
            if v.muted { " (muted)" } else { "" }
        );
    }
}

#[derive(Resource, Default)]
struct Trace {
    on: bool,
    last: Option<Vec3>,
    since: f32,
    logged: bool,
    /// Real seconds since the last level report.
    levels_since: f32,
}

/// `HALO3_AUDIO_TRACE=1`: every 5 s, each bus's level over that time (RMS
/// dBFS before the master volume), the volumes, and the scripts' class
/// gains.
fn trace_levels(
    time: Res<Time<Real>>,
    mixer: Res<Mixer>,
    levels: Res<SoundLevels>,
    gains: Res<music::ClassGains>,
    music: Res<music::MusicPlayer>,
    mut t: ResMut<Trace>,
) {
    if !t.on {
        return;
    }
    t.levels_since += time.delta_secs();
    if t.levels_since < 5.0 {
        return;
    }
    t.levels_since = 0.0;
    let measured = mixer.take_levels();
    let buses: Vec<String> = Bus::ALL
        .iter()
        .map(|b| match measured[b.index()] {
            Some(db) => format!("{} {db:.1} dBFS", b.name()),
            None => format!("{} silent", b.name()),
        })
        .collect();
    info!(
        "audio levels: {}; volumes {}; script class gains music {:.2}, amb {:.2}; music [{}]",
        buses.join(", "),
        levels.describe(),
        gains.get("music"),
        gains.get("amb"),
        music.describe()
    );
}

/// `HALO3_AUDIO_TRACE=1`: what is heard, each time the camera has moved.
fn trace(
    time: Res<Time>,
    sounds: Option<Res<Sounds>>,
    l: Res<Listener>,
    amb: Res<Ambience>,
    mixer: Res<Mixer>,
    music: Res<music::MusicPlayer>,
    mut t: ResMut<Trace>,
) {
    let Some(sounds) = sounds else {
        return;
    };
    if !t.on || !l.valid {
        return;
    }
    t.since += time.delta_secs();
    let moved = t.last.is_none_or(|p| p.distance(l.position) > 2.0);
    // Wait a moment at a new place so decodes and fades settle (`--shot`
    // stays half a second on each later view).
    if moved {
        t.last = Some(l.position);
        t.since = 0.0;
        t.logged = false;
        return;
    }
    if t.logged || t.since < 0.3 {
        return;
    }
    t.logged = true;
    let d = &sounds.data;
    let bgs: Vec<String> = amb
        .layers
        .iter()
        .map(|(tag, layer)| {
            format!(
                "{} fade {:.2}{}",
                d.tag_name(*tag),
                layer.fade,
                if layer.voices.iter().any(Option::is_some) {
                    ""
                } else {
                    " (decoding)"
                }
            )
        })
        .collect();
    let mut heard = amb.heard.clone();
    heard.sort_by(|a, b| b.2.total_cmp(&a.2));
    let scen: Vec<String> = heard
        .iter()
        .take(8)
        .map(|(i, dist, g)| {
            let s = &d.map.scenery[*i];
            format!(
                "#{i} {} at {dist:.1} wu gain {g:.3}{}",
                s.looping.map_or("?", |t| d.tag_name(t)),
                if amb.scenery.get(*i).copied().flatten().is_some() {
                    ""
                } else {
                    " (not started)"
                }
            )
        })
        .collect();
    let (n, mib) = sounds.clips.stats();
    info!(
        "audio: at ({:.1}, {:.1}, {:.1}) cluster {:?}: background [{}]; scenery {} in range [{}]; details {}; music [{}]; voices {}; clips {} ({:.1} MiB)",
        l.position.x,
        l.position.y,
        l.position.z,
        amb.place,
        bgs.join(", "),
        heard.len(),
        scen.join("; "),
        amb.details.len(),
        music.describe(),
        mixer.voices(),
        n,
        mib
    );
}
