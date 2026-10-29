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

struct Voice {
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

#[derive(Default)]
struct State {
    voices: HashMap<VoiceId, Voice>,
    next_id: VoiceId,
    master: f32,
}

/// The handle the main thread uses.
#[derive(Resource, Clone)]
pub struct Mixer(Arc<Mutex<State>>);

impl Default for Mixer {
    fn default() -> Self {
        Mixer(Arc::new(Mutex::new(State {
            master: 1.0,
            next_id: 1,
            ..Default::default()
        })))
    }
}

/// What to start a voice with.
pub struct Play {
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

    /// Fades out over one block and removes the voice.
    pub fn stop(&self, id: VoiceId) {
        if let Some(v) = self.0.lock().unwrap().voices.get_mut(&id) {
            v.stopping = true;
            v.target = [0.0; 2];
        }
    }

    pub fn is_playing(&self, id: VoiceId) -> bool {
        self.0.lock().unwrap().voices.contains_key(&id)
    }

    pub fn voices(&self) -> usize {
        self.0.lock().unwrap().voices.len()
    }

    pub fn set_master(&self, gain: f32) {
        self.0.lock().unwrap().master = gain;
    }

    /// Mixes `out.len() / 2` stereo frames (the audio thread's job; called
    /// directly when there is no sound device, `--shot` trace mode).
    pub fn render(&self, out: &mut [f32]) {
        out.fill(0.0);
        let frames = out.len() / 2;
        let mut s = self.0.lock().unwrap();
        let master = s.master;
        let mut done = Vec::new();
        for (id, v) in s.voices.iter_mut() {
            let start = v.gain;
            let step = [
                (v.target[0] - start[0]) / frames as f32,
                (v.target[1] - start[1]) / frames as f32,
            ];
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
                    out[2 * f] += (a0 + (a1 - a0) * t) * gl;
                    out[2 * f + 1] += (b0 + (b1 - b0) * t) * gr;
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
            queue: vec![clip(10, 1000)],
            looping: Vec::new(),
            gain: [1.0; 2],
            rate: 1.0,
        });
        let mut out = vec![0.0; BLOCK * 2];
        m.render(&mut out);
        assert!(!m.is_playing(id));
    }
}
