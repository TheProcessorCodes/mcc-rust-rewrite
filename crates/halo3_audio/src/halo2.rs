//! Ambience for maps with no Halo 3 cache file behind them (Halo 2's): the
//! caller hands over already-decoded sounds as plain data
//! ([`PlainAmbience`]), built on a thread, and this plays them through the
//! same mixer as Halo 3's sound.
//!
//! - Backgrounds loop all the time, not panned: each track plays its first
//!   permutation, then the others at random, skipping those whose skip
//!   fraction is 1 (Halo's "play first only"). Their details fire at random
//!   in their period, mono ones placed around the listener.
//! - Sound scenery sits at its position: full volume within the looping
//!   sound's near distance, silent past the far one, panned; its details
//!   play at the scenery.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

use crate::mixer::{Bus, Clip, Mixer, Play, RATE, VoiceId};
use crate::{Listener, attenuation};

/// Decoded samples of one permutation, at the sound's own rate.
pub struct PlainClip {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
    /// 0..1: a random pick skips it this often (1: only ever played first).
    pub skip: f32,
}

pub struct PlainTrack {
    pub name: String,
    /// Linear gain (track and sound gains together).
    pub gain: f32,
    pub clips: Vec<PlainClip>,
}

pub struct PlainDetail {
    pub name: String,
    pub gain: f32,
    pub clips: Vec<PlainClip>,
    /// Seconds between plays.
    pub period: (f32, f32),
    /// 0..1 of plays skipped.
    pub skip: f32,
    /// Radians, and world units from the listener (backgrounds).
    pub yaw: (f32, f32),
    pub pitch: (f32, f32),
    pub distance: (f32, f32),
}

pub struct PlainLoop {
    pub name: String,
    pub tracks: Vec<PlainTrack>,
    pub details: Vec<PlainDetail>,
}

pub struct PlainEmitter {
    /// Halo world units (Z up).
    pub position: Vec3,
    /// Full volume within, silent beyond (world units).
    pub distance: (f32, f32),
    pub sound: PlainLoop,
}

#[derive(Default)]
pub struct PlainAmbience {
    pub backgrounds: Vec<PlainLoop>,
    pub emitters: Vec<PlainEmitter>,
}

/// Builds the ambience (decoding takes a moment); run on a thread.
pub type PlainLoader = Box<dyn FnOnce() -> Result<PlainAmbience, String> + Send>;

/// A clip ready for the mixer, and the playback rate that brings it to the
/// mixer's 48 kHz.
#[derive(Clone)]
struct Ready {
    clip: Arc<Clip>,
    rate: f32,
    skip: f32,
}

fn ready(clips: Vec<PlainClip>) -> Vec<Ready> {
    clips
        .into_iter()
        .filter(|c| !c.samples.is_empty() && c.rate > 0)
        .map(|c| Ready {
            rate: c.rate as f32 / RATE as f32,
            skip: c.skip,
            clip: Arc::new(Clip {
                channels: c.channels,
                samples: c.samples.into(),
            }),
        })
        .collect()
}

struct Track {
    gain: f32,
    clips: Vec<Ready>,
    voice: Option<VoiceId>,
}

struct Detail {
    gain: f32,
    clips: Vec<Ready>,
    period: (f32, f32),
    skip: f32,
    yaw: (f32, f32),
    pitch: (f32, f32),
    distance: (f32, f32),
    timer: f32,
}

struct Layer {
    name: String,
    /// Where it sits; `None`: everywhere (a background).
    at: Option<(Vec3, (f32, f32))>,
    tracks: Vec<Track>,
    details: Vec<Detail>,
}

impl Layer {
    fn new(l: PlainLoop, at: Option<(Vec3, (f32, f32))>) -> Layer {
        Layer {
            name: l.name,
            at,
            tracks: l
                .tracks
                .into_iter()
                .map(|t| Track {
                    gain: t.gain,
                    clips: ready(t.clips),
                    voice: None,
                })
                .collect(),
            details: l
                .details
                .into_iter()
                .map(|d| Detail {
                    gain: d.gain,
                    clips: ready(d.clips),
                    period: d.period,
                    skip: d.skip,
                    yaw: d.yaw,
                    pitch: d.pitch,
                    distance: d.distance,
                    timer: -1.0,
                })
                .collect(),
        }
    }
}

struct OneShot {
    id: VoiceId,
    at: Option<Vec3>,
    gain: f32,
    /// The sound scenery it belongs to, for its distance fade.
    emitter: Option<(Vec3, (f32, f32))>,
}

#[derive(Resource)]
pub(crate) struct PlainState {
    rx: Mutex<Option<Receiver<Result<PlainAmbience, String>>>>,
    layers: Vec<Layer>,
    shots: Vec<OneShot>,
    /// 0..1, ramps up over two seconds once loaded.
    fade: f32,
    rng: u32,
}

impl PlainState {
    pub(crate) fn start(loader: PlainLoader) -> PlainState {
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name("halo2-sound-load".into())
            .spawn(move || {
                let _ = tx.send(loader());
            });
        if let Err(e) = spawned {
            warn!("sound: no loader thread ({e})");
        }
        PlainState {
            rx: Mutex::new(Some(rx)),
            layers: Vec::new(),
            shots: Vec::new(),
            fade: 0.0,
            rng: 0x2545_F491,
        }
    }

    fn random(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        (self.rng >> 8) as f32 / (1u32 << 24) as f32
    }

    fn between(&mut self, (a, b): (f32, f32)) -> f32 {
        a + (b - a) * self.random()
    }
}

pub(crate) fn receive(mut s: ResMut<PlainState>) {
    let got = {
        let mut slot = s.rx.lock().unwrap();
        let Some(rx) = slot.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(r) => {
                *slot = None;
                r
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(_) => {
                *slot = None;
                Err("the loader stopped".into())
            }
        }
    };
    match got {
        Ok(a) => {
            let mut layers: Vec<Layer> = a.backgrounds.into_iter().map(|l| Layer::new(l, None)).collect();
            layers.extend(a.emitters.into_iter().map(|e| Layer::new(e.sound, Some((e.position, e.distance)))));
            for l in &layers {
                let secs: f32 = l
                    .tracks
                    .iter()
                    .flat_map(|t| &t.clips)
                    .chain(l.details.iter().flat_map(|d| &d.clips))
                    .map(|c| c.clip.frames() as f32 / (RATE as f32 * c.rate))
                    .sum();
                info!(
                    "sound: {} ({} tracks, {} details, {:.0} s of samples){}",
                    l.name,
                    l.tracks.len(),
                    l.details.len(),
                    secs,
                    l.at.map_or(String::new(), |(p, d)| format!(" at {:.1?}, heard within {:.0} wu", p.to_array(), d.1))
                );
            }
            s.layers = layers;
        }
        Err(e) => warn!("sound: no ambience ({e})"),
    }
}

/// Gain of a layer for the listener (backgrounds: 1).
fn layer_gain(at: Option<(Vec3, (f32, f32))>, l: &Listener) -> f32 {
    match at {
        None => 1.0,
        Some((p, d)) => attenuation(p.distance(l.position), d),
    }
}

pub(crate) fn play(time: Res<Time>, l: Res<Listener>, mixer: Res<Mixer>, mut s: ResMut<PlainState>) {
    if s.layers.is_empty() || !l.valid {
        return;
    }
    let dt = time.delta_secs();
    s.fade = (s.fade + dt / 2.0).min(1.0);
    let fade = s.fade;
    let mut fire: Vec<(usize, usize)> = Vec::new();
    for li in 0..s.layers.len() {
        let at = s.layers[li].at;
        let g = layer_gain(at, &l) * fade;
        for t in 0..s.layers[li].tracks.len() {
            let track = &mut s.layers[li].tracks[t];
            if track.clips.is_empty() {
                continue;
            }
            let want = track.gain * g;
            match track.voice {
                // Out of earshot: let the voice go, start it again on return.
                Some(id) if want <= 0.0 && at.is_some() => {
                    mixer.stop(id);
                    track.voice = None;
                }
                Some(id) => {
                    let pan = match at {
                        Some((p, _)) => l.pan(p, want),
                        None => [want; 2],
                    };
                    mixer.set_gain(id, pan);
                }
                None if want > 0.0 => {
                    let first = track.clips[0].clone();
                    let mut looping: Vec<Arc<Clip>> =
                        track.clips.iter().filter(|c| c.skip < 0.999).map(|c| c.clip.clone()).collect();
                    if looping.is_empty() {
                        looping.push(first.clip.clone());
                    }
                    track.voice = Some(mixer.play(Play {
                        bus: Bus::Ambience,
                        queue: vec![first.clip],
                        looping,
                        gain: [0.0; 2],
                        rate: first.rate,
                    }));
                }
                None => {}
            }
        }
        for d in 0..s.layers[li].details.len() {
            let (timer, period) = {
                let det = &s.layers[li].details[d];
                (det.timer, det.period)
            };
            let next = if timer < 0.0 {
                // First time: somewhere in the period, not all at once.
                s.between((0.0, period.1.max(1.0)))
            } else if timer - dt <= 0.0 {
                if g > 0.0 {
                    fire.push((li, d));
                }
                s.between(period).max(0.5)
            } else {
                timer - dt
            };
            s.layers[li].details[d].timer = next;
        }
    }
    for (li, d) in fire {
        let skip = s.layers[li].details[d].skip;
        if s.random() < skip || s.layers[li].details[d].clips.is_empty() {
            continue;
        }
        let n = s.layers[li].details[d].clips.len();
        let pick = ((s.random() * n as f32) as usize).min(n - 1);
        let (yaw, pitch, distance) = {
            let det = &s.layers[li].details[d];
            (det.yaw, det.pitch, det.distance)
        };
        let (yaw, pitch, dist) = (s.between(yaw), s.between(pitch) * 0.3, s.between(distance));
        let emitter = s.layers[li].at;
        let (c, base) = {
            let det = &s.layers[li].details[d];
            (det.clips[pick].clone(), det.gain)
        };
        let gain = base * layer_gain(emitter, &l);
        let at = if c.clip.channels != 1 {
            None
        } else if let Some((p, _)) = emitter {
            Some(p)
        } else {
            let fwd = Vec3::new(l.forward.x, l.forward.y, 0.0).normalize_or(Vec3::X);
            let side = Vec3::new(-fwd.y, fwd.x, 0.0);
            let dir = (fwd * yaw.cos() + side * yaw.sin()) * pitch.cos() + Vec3::Z * pitch.sin();
            // Zero bounds: anywhere around; a few metres keeps the pan.
            Some(l.position + dir * if dist > 0.0 { dist } else { 3.0 })
        };
        let g = at.map_or([gain; 2], |p| l.pan(p, gain));
        let id = mixer.play(Play {
            bus: Bus::Ambience,
            queue: vec![c.clip],
            looping: Vec::new(),
            gain: g,
            rate: c.rate,
        });
        s.shots.push(OneShot {
            id,
            at,
            gain: base,
            emitter,
        });
    }
    s.shots.retain(|o| mixer.is_playing(o.id));
    for o in &s.shots {
        let gain = o.gain * layer_gain(o.emitter, &l);
        let g = o.at.map_or([gain; 2], |p| l.pan(p, gain));
        mixer.set_gain(o.id, [g[0] * fade, g[1] * fade]);
    }
}
