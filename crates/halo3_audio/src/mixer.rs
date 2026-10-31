//! Our own mixer: one endless 48 kHz stereo source that bevy_audio plays,
//! into which every Halo voice is mixed. The main thread starts, steers
//! and stops voices through [`Mixer`]; the audio thread renders blocks of
//! [`BLOCK`] frames, locking the shared state once per block.
//!
//! A voice plays a queue of clips, then clips picked at random from its
//! loop set forever (Halo loops a looping sound's permutations that way);
//! changing the queue and loop set takes effect when the current clip
//! ends, which is how music moves to its alternate loop or its outro on a
//! loop boundary.
//!
//! A [`Generator`] is a voice that makes its own sound block by block (the
//! bike's engine, `engine.rs`); it is mixed in with the rest.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use bevy::audio::{ChannelCount, Decodable, Source};
use bevy::prelude::*;

pub const RATE: u32 = 48_000;
const BLOCK: usize = 1024;

/// Decoded audio kept for playing: 16-bit, mono or stereo (multichannel
/// sources are folded to stereo when decoded).
pub struct Clip {
    pub channels: u16,
    pub samples: Arc<[i16]>,
}

impl Clip {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / RATE as f32
    }
    fn frame(&self, i: usize) -> (f32, f32) {
        const S: f32 = 1.0 / 32768.0;
        if self.channels == 1 {
            let v = self.samples[i] as f32 * S;
            (v, v)
        } else {
            (
                self.samples[2 * i] as f32 * S,
                self.samples[2 * i + 1] as f32 * S,
            )
        }
    }
}

pub type VoiceId = u64;

/// The group a voice is mixed in: each has its own volume (the pause menu's
/// sliders) on top of the voice's gain, and its own level meter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Bus {
    Music,
    Ambience,
    Effects,
    Dialogue,
}

impl Bus {
    pub const ALL: [Bus; 4] = [Bus::Music, Bus::Ambience, Bus::Effects, Bus::Dialogue];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn name(self) -> &'static str {
        match self {
            Bus::Music => "music",
            Bus::Ambience => "ambience",
            Bus::Effects => "effects",
            Bus::Dialogue => "dialogue",
        }
    }
}

pub const BUSES: usize = Bus::ALL.len();

struct Voice {
    bus: Bus,
    queue: VecDeque<Arc<Clip>>,
    looping: Vec<Arc<Clip>>,
    current: Option<Arc<Clip>>,
    /// Position in `current`, frames (fractional for `rate`).
    pos: f64,
    /// Playback rate (1.0 = as recorded).
    rate: f32,
    target: [f32; 2],
    gain: [f32; 2],
    /// Stop when the gain has ramped to zero.
    stopping: bool,
    rng: u32,
    last: usize,
}

impl Voice {
    fn next_clip(&mut self) -> Option<Arc<Clip>> {
        if let Some(c) = self.queue.pop_front() {
            return Some(c);
        }
        if self.looping.is_empty() {
            return None;
        }
        // xorshift; avoid repeating the last permutation when there is a
        // choice.
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 17;
        self.rng ^= self.rng << 5;
        let n = self.looping.len();
        let mut i = self.rng as usize % n;
        if n > 1 && i == self.last {
            i = (i + 1) % n;
        }
        self.last = i;
        Some(self.looping[i].clone())
    }
}

/// A voice that renders itself: adds `out.len() / 2` stereo frames at
/// [`RATE`] into `out`, on the audio thread. Returns false when it is done.
pub trait Generator: Send {
    fn render(&mut self, out: &mut [f32]) -> bool;
}

#[derive(Default)]
struct State {
    voices: HashMap<VoiceId, Voice>,
    generators: HashMap<VoiceId, Box<dyn Generator>>,
    next_id: VoiceId,
    master: f32,
    /// Each bus's volume, and what it was at the end of the last block (a
    /// change ramps over one block).
    bus_gain: [f32; BUSES],
    bus_was: [f32; BUSES],
    /// Per bus, the sum of squares of its mix (both channels, before the
    /// master volume), and the frames summed, since `take_levels`.
    meter: [f64; BUSES],
    metered: usize,
    /// Per-bus mixing space for one block.
    scratch: Vec<[f32; BUSES * 2]>,
}

/// The handle the main thread uses.
#[derive(Resource, Clone)]
pub struct Mixer(Arc<Mutex<State>>);

impl Default for Mixer {
    fn default() -> Self {
        Mixer(Arc::new(Mutex::new(State {
            master: 1.0,
            next_id: 1,
            bus_gain: [1.0; BUSES],
            bus_was: [1.0; BUSES],
            ..Default::default()
        })))
    }
}

/// What to start a voice with.
pub struct Play {
    pub bus: Bus,
    pub queue: Vec<Arc<Clip>>,
    pub looping: Vec<Arc<Clip>>,
    /// Starting gains (left, right); ramps from zero to them.
    pub gain: [f32; 2],
    pub rate: f32,
}

impl Mixer {
    pub fn play(&self, p: Play) -> VoiceId {
        let mut s = self.0.lock().unwrap();
        let id = s.next_id;
        s.next_id += 1;
        s.voices.insert(
            id,
            Voice {
                bus: p.bus,
                queue: p.queue.into(),
                looping: p.looping,
                current: None,
                pos: 0.0,
                rate: p.rate,
                target: p.gain,
                gain: [0.0; 2],
                stopping: false,
                rng: (id as u32).wrapping_mul(2_654_435_761) | 1,
                last: usize::MAX,
            },
        );
        id
    }

    pub fn add_generator(&self, g: Box<dyn Generator>) -> VoiceId {
        let mut s = self.0.lock().unwrap();
        let id = s.next_id;
        s.next_id += 1;
        s.generators.insert(id, g);
        id
    }

    /// Sets a voice's gains; it ramps there over one block (21 ms).
    pub fn set_gain(&self, id: VoiceId, gain: [f32; 2]) {
        if let Some(v) = self.0.lock().unwrap().voices.get_mut(&id) {
            v.target = gain;
        }
    }

    pub fn set_rate(&self, id: VoiceId, rate: f32) {
        if let Some(v) = self.0.lock().unwrap().voices.get_mut(&id) {
            v.rate = rate.clamp(0.25, 4.0);
        }
    }

    /// Replaces what plays after the current clip.
    pub fn set_next(&self, id: VoiceId, queue: Vec<Arc<Clip>>, looping: Vec<Arc<Clip>>) {
        if let Some(v) = self.0.lock().unwrap().voices.get_mut(&id) {
            v.queue = queue.into();
            v.looping = looping;
        }
    }

    /// Fades out over one block and removes the voice (a generator goes at
    /// once: it fades itself).
    pub fn stop(&self, id: VoiceId) {
        let mut s = self.0.lock().unwrap();
        if s.generators.remove(&id).is_some() {
            return;
        }
        if let Some(v) = s.voices.get_mut(&id) {
            v.stopping = true;
            v.target = [0.0; 2];
        }
    }

    /// Fades out every voice (the map is unloading).
    pub fn stop_all(&self) {
        for v in self.0.lock().unwrap().voices.values_mut() {
            v.stopping = true;
            v.target = [0.0; 2];
        }
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        let s = self.0.lock().unwrap();
        s.voices.contains_key(&id) || s.generators.contains_key(&id)
    }

    pub fn voices(&self) -> usize {
        let s = self.0.lock().unwrap();
        s.voices.len() + s.generators.len()
    }

    pub fn set_master(&self, gain: f32) {
        self.0.lock().unwrap().master = gain;
    }

    /// Sets a bus's volume; it ramps there over one block.
    pub fn set_bus(&self, bus: Bus, gain: f32) {
        self.0.lock().unwrap().bus_gain[bus.index()] = gain.max(0.0);
    }

    /// Each bus's RMS level in dBFS (both channels, after the bus volume,
    /// before the master volume) since the last call; `None` for a bus
    /// that was silent.
    pub fn take_levels(&self) -> [Option<f32>; BUSES] {
        let mut s = self.0.lock().unwrap();
        let n = (s.metered * 2).max(1) as f64;
        let out = s.meter.map(|sq| (sq > 0.0).then(|| (10.0 * (sq / n).log10()) as f32));
        s.meter = [0.0; BUSES];
        s.metered = 0;
        out
    }

    /// Mixes `out.len() / 2` stereo frames (the audio thread's job; called
    /// directly when there is no sound device, `--shot` trace mode).
    pub fn render(&self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let mut guard = self.0.lock().unwrap();
        let s = &mut *guard;
        let master = s.master;
        s.scratch.clear();
        s.scratch.resize(frames, [0.0; BUSES * 2]);
        let mut done = Vec::new();
        for (id, v) in s.voices.iter_mut() {
            let start = v.gain;
            let step = [
                (v.target[0] - start[0]) / frames as f32,
                (v.target[1] - start[1]) / frames as f32,
            ];
            let b = v.bus.index();
            let mut f = 0;
            while f < frames {
                if v.current.is_none() {
                    v.current = v.next_clip();
                    v.pos = 0.0;
                    if v.current.is_none() {
                        break;
                    }
                }
                let clip = v.current.clone().unwrap();
                let n = clip.frames();
                while f < frames {
                    let i = v.pos as usize;
                    if i + 1 >= n {
                        v.current = None;
                        break;
                    }
                    let t = (v.pos - i as f64) as f32;
                    let (a0, b0) = clip.frame(i);
                    let (a1, b1) = clip.frame(i + 1);
                    let gl = start[0] + step[0] * f as f32;
                    let gr = start[1] + step[1] * f as f32;
                    let cell = &mut s.scratch[f];
                    cell[2 * b] += (a0 + (a1 - a0) * t) * gl;
                    cell[2 * b + 1] += (b0 + (b1 - b0) * t) * gr;
                    v.pos += v.rate as f64;
                    f += 1;
                }
            }
            v.gain = v.target;
            let finished = v.current.is_none() && v.queue.is_empty() && v.looping.is_empty();
            if finished || (v.stopping && v.gain == [0.0; 2]) {
                done.push(*id);
            }
        }
        for id in done {
            s.voices.remove(&id);
        }
        // The buses into the output, each at its volume (ramped from the
        // last block's).
        let (was, now) = (s.bus_was, s.bus_gain);
        for (f, cell) in s.scratch.iter().enumerate() {
            let t = (f + 1) as f32 / frames as f32;
            for b in 0..BUSES {
                let g = was[b] + (now[b] - was[b]) * t;
                let (l, r) = (cell[2 * b] * g, cell[2 * b + 1] * g);
                s.meter[b] += (l * l + r * r) as f64;
                out[2 * f] += l;
                out[2 * f + 1] += r;
            }
        }
        s.bus_was = now;
        s.metered += frames;
        // Generators (the bike engine, with its own volume) after the
        // buses, under the master volume only.
        s.generators.retain(|_, g| g.render(out));
        for x in out.iter_mut() {
            // A soft limit instead of hard clipping when many voices pile up.
            let y = *x * master;
            *x = if y.abs() < 0.8 {
                y
            } else {
                y.signum() * (0.8 + 0.2 * ((y.abs() - 0.8) / 0.2).tanh())
            };
        }
    }
}

/// The endless source bevy_audio plays.
pub struct MixerSource {
    mixer: Mixer,
    buf: Vec<f32>,
    at: usize,
}

impl Iterator for MixerSource {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        if self.at >= self.buf.len() {
            self.mixer.render(&mut self.buf);
            self.at = 0;
        }
        let v = self.buf[self.at];
        self.at += 1;
        Some(v)
    }
}

impl Source for MixerSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> ChannelCount {
        ChannelCount::new(2).unwrap()
    }
    fn sample_rate(&self) -> bevy::audio::SampleRate {
        bevy::audio::SampleRate::new(RATE).unwrap()
    }
    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

/// The asset that hands bevy_audio our source.
#[derive(Asset, TypePath)]
pub struct MixerAsset(pub Mixer);

impl Decodable for MixerAsset {
    type Decoder = MixerSource;
    fn decoder(&self) -> MixerSource {
        MixerSource {
            mixer: self.0.clone(),
            buf: vec![0.0; BLOCK * 2],
            at: BLOCK * 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clip(frames: usize, value: i16) -> Arc<Clip> {
        Arc::new(Clip {
            channels: 1,
            samples: vec![value; frames].into(),
        })
    }

    #[test]
    fn queue_then_loop_then_stop() {
        let m = Mixer::default();
        let id = m.play(Play {
            bus: Bus::Effects,
            queue: vec![clip(100, 16384)],
            looping: vec![clip(50, 8192)],
            gain: [1.0, 0.5],
            rate: 1.0,
        });
        let mut out = vec![0.0; BLOCK * 2];
        m.render(&mut out);
        // Ramps up over the first block, then holds; right is half the left.
        assert_eq!(out[0], 0.0);
        m.render(&mut out);
        assert!((out[0] - 0.25).abs() < 1e-3, "{}", out[0]);
        assert!((out[1] - 0.125).abs() < 1e-3, "{}", out[1]);
        assert!(m.is_playing(id));
        m.stop(id);
        m.render(&mut out);
        assert!(!m.is_playing(id));
    }

    #[test]
    fn one_shot_ends() {
        let m = Mixer::default();
        let id = m.play(Play {
            bus: Bus::Effects,
            queue: vec![clip(10, 1000)],
            looping: Vec::new(),
            gain: [1.0; 2],
            rate: 1.0,
        });
        let mut out = vec![0.0; BLOCK * 2];
        m.render(&mut out);
        assert!(!m.is_playing(id));
    }

    #[test]
    fn bus_volume_and_meter() {
        let m = Mixer::default();
        m.play(Play {
            bus: Bus::Music,
            queue: Vec::new(),
            looping: vec![clip(4096, 16384)],
            gain: [1.0; 2],
            rate: 1.0,
        });
        let mut out = vec![0.0; BLOCK * 2];
        m.render(&mut out);
        let _ = m.take_levels();
        m.render(&mut out);
        assert!((out[0] - 0.5).abs() < 1e-3, "{}", out[0]);
        let levels = m.take_levels();
        // Half scale is -6 dBFS.
        let music = levels[Bus::Music.index()].unwrap();
        assert!((music + 6.02).abs() < 0.05, "{music}");
        assert_eq!(levels[Bus::Effects.index()], None);
        // Half the bus volume: -6 dB more, after a one-block ramp.
        m.set_bus(Bus::Music, 0.5);
        m.render(&mut out);
        m.render(&mut out);
        assert!((out[0] - 0.25).abs() < 1e-3, "{}", out[0]);
    }
}
