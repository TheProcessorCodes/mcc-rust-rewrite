//! The dirt bike's engine, two ways, to compare (H, or the left stick click
//! while riding, goes Reflex → synth → off):
//!
//! - **Reflex**: the Dakumoto 450F's own recordings, read from the user's
//!   MX vs ATV Reflex install (`reflex_assets::engine_audio`). Reflex plays
//!   them through FMOD events (idle, sustained throttle at four loads, decel,
//!   limiter, landing, wreck) that we cannot read yet, so this does the same
//!   job by hand: each recording's pitch was measured once (the table
//!   below, rpm = the firing frequency × 120), and a layer plays the
//!   recording nearest the bike's rpm and load, crossfading to a closer one
//!   as the rpm moves. Most takes were recorded accelerating, so their pitch
//!   climbs inside the take; at load each one's pitch is followed through it
//!   (`contour`, held near the table's figure so it cannot jump an octave)
//!   and the playback rate undoes that, so the pitch heard is the bike's.
//! - **Synth**: our own single-cylinder four-stroke, made from nothing: a
//!   blow-down pulse per firing (every second turn of the crank), each one a
//!   little different, into two pipe resonators (head pipe, silencer), the
//!   tone opening up with throttle, intake snarl, valve ticks, a stuttering
//!   rev limiter and pops off the throttle.
//!
//! Both have wind rising with speed. Without Reflex the Reflex model falls
//! back to the synth.

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;

use halo3_core::bike::BikeMotion;

use crate::mixer::{Generator, Mixer, RATE, VoiceId};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Model {
    #[default]
    Reflex,
    Synth,
    Off,
}

impl Model {
    fn next(self) -> Model {
        match self {
            Model::Reflex => Model::Synth,
            Model::Synth => Model::Off,
            Model::Off => Model::Reflex,
        }
    }
}

/// What the engine is doing, handed to the audio thread each frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub rpm: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub throttle: f32,
    /// m/s.
    pub speed: f32,
    pub airborne: bool,
    /// Counts up once per landing and per crash; `land_force` 0 to 1.
    pub landings: u32,
    pub land_force: f32,
    pub wrecks: u32,
}

/// Which part a recording plays.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Idle,
    /// Under throttle; the load it was recorded at, 0 to 1.
    Drive(f32),
    Decel,
    Limiter,
    Land,
    Wreck,
    Unused,
}

/// The 450F's recordings: name, the rpm they hold (median of 0.2 s windows,
/// firing frequency × 120), and what they are for.
const CLIPS_450F: &[(&str, f32, Kind)] = &[
    ("MXB_450F_IDLE", 3690.0, Kind::Idle),
    ("MXB_450F_HALF_ACCEL", 3420.0, Kind::Drive(0.3)),
    ("MXB_450F_HALF_SUSTAINED", 4260.0, Kind::Drive(0.2)),
    ("MXB_450F_THROTTLE_LOW_01", 9465.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_LOW_02", 7050.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_LOW_03", 10035.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_LOW_04", 8070.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_LOW_05", 7140.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_LOW_06", 8940.0, Kind::Drive(0.35)),
    ("MXB_450F_THROTTLE_MID_01", 8850.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_02", 9870.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_03", 10140.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_04", 8520.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_05", 9930.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_06", 8085.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_MID_07", 8340.0, Kind::Drive(0.55)),
    ("MXB_450F_THROTTLE_HIGH_01", 11010.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_02", 9870.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_03", 8940.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_04", 8640.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_05", 9030.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_06", 9285.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_07", 9360.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_08", 9360.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_09", 10110.0, Kind::Drive(0.75)),
    ("MXB_450F_THROTTLE_HIGH_10", 8820.0, Kind::Drive(0.75)),
    // The POWER takes wander most (wheelspin, the bike bucking); the
    // steadier ones only.
    ("MXB_450F_THROTTLE_POWER_01", 8850.0, Kind::Drive(0.95)),
    ("MXB_450F_THROTTLE_POWER_06", 6870.0, Kind::Drive(0.95)),
    ("MXB_450F_THROTTLE_POWER_07", 8850.0, Kind::Drive(0.95)),
    ("MXB_450F_THROTTLE_POWER_02", 8640.0, Kind::Unused),
    ("MXB_450F_THROTTLE_POWER_03", 7980.0, Kind::Unused),
    ("MXB_450F_THROTTLE_POWER_04", 5745.0, Kind::Unused),
    ("MXB_450F_THROTTLE_POWER_05", 6570.0, Kind::Unused),
    ("MXB_450F_THROTTLE_POWER_08", 6600.0, Kind::Unused),
    ("MXB_450F_THROTTLE_POWER_09", 6645.0, Kind::Unused),
    // Falling sweeps, about 9900 down to 4300 in a second.
    ("MXB_450F_DECEL_HIGH", 9900.0, Kind::Decel),
    ("MXB_450F_DECEL_LOW", 9780.0, Kind::Decel),
    ("MXB_450F_PANIC_LOOP", 11250.0, Kind::Limiter),
    ("MXB_450F_THROTTLE_LAND_01", 7620.0, Kind::Land),
    ("MXB_450F_THROTTLE_LAND_02", 7815.0, Kind::Land),
    ("MXB_450F_THROTTLE_LAND_03", 6360.0, Kind::Land),
    ("MXB_450F_THROTTLE_LAND_05", 7950.0, Kind::Land),
    ("MXB_450F_THROTTLE_LAND_06", 6870.0, Kind::Land),
    ("MXB_450F_WRECK", 2025.0, Kind::Wreck),
];

struct Recording {
    /// Normalised to the same loudness.
    samples: Vec<f32>,
    /// Source frames per output frame at the recorded pitch.
    step: f64,
    /// The take's typical rpm (the table's), and its rpm every
    /// [`CONTOUR_HOP`] samples.
    rpm: f32,
    contour: Vec<f32>,
    kind: Kind,
}

impl Recording {
    /// The rpm the take holds at sample `pos`.
    fn rpm_at(&self, pos: f64) -> f32 {
        let x = pos / CONTOUR_HOP as f64;
        let i = x as usize;
        match (self.contour.get(i), self.contour.get(i + 1)) {
            (Some(&a), Some(&b)) => a + (b - a) * (x - i as f64) as f32,
            (Some(&a), None) => a,
            _ => self.rpm,
        }
    }
}

/// Recorded samples between two points of a contour.
const CONTOUR_HOP: usize = 4096;
/// Measured at a quarter of the recorded rate.
const DECIMATE: usize = 4;
const FFT: usize = 8192;
const WINDOW: usize = 2048;

/// In-place radix-2 FFT (`re.len()` a power of two).
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let a = -std::f32::consts::TAU / len as f32;
        let (wr, wi) = (a.cos(), a.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (p, q) = (start + k, start + k + len / 2);
                let tr = re[q] * cr - im[q] * ci;
                let ti = re[q] * ci + im[q] * cr;
                re[q] = re[p] - tr;
                im[q] = im[p] - ti;
                re[p] += tr;
                im[p] += ti;
                let c = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = c;
            }
        }
        len <<= 1;
    }
}

/// The rpm through a take, every [`CONTOUR_HOP`] samples: for each window,
/// the firing frequency whose first ten harmonics hold the most energy,
/// looked for within 0.7 to 1.4 times `anchor`; then a median of five.
fn contour(samples: &[f32], rate: u32, anchor: f32) -> Vec<f32> {
    let low: Vec<f32> = samples
        .chunks(DECIMATE)
        .map(|c| c.iter().sum::<f32>() / c.len() as f32)
        .collect();
    let sr = rate as f32 / DECIMATE as f32;
    let hop = CONTOUR_HOP / DECIMATE;
    let hann: Vec<f32> = (0..WINDOW)
        .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / WINDOW as f32).cos())
        .collect();
    let (mut re, mut im) = (vec![0.0; FFT], vec![0.0; FFT]);
    let base = anchor / 120.0;
    let mut raw = Vec::new();
    let mut at = 0;
    while at < low.len() {
        // Centred on the point.
        let start = at as isize - WINDOW as isize / 2;
        for i in 0..FFT {
            let j = start + i as isize;
            re[i] = if i < WINDOW && j >= 0 && (j as usize) < low.len() {
                low[j as usize] * hann[i]
            } else {
                0.0
            };
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let mag: Vec<f32> = (0..FFT / 2).map(|i| (re[i] * re[i] + im[i] * im[i] + 1e-12).ln()).collect();
        let mut best = (f32::MIN, base);
        let mut f = base * 0.7;
        while f <= base * 1.4 {
            let mut sum = 0.0;
            for h in 1..=10 {
                let bin = (f * h as f32 * FFT as f32 / sr).round() as usize;
                let lo = bin.saturating_sub(1);
                sum += mag[lo..(bin + 2).min(mag.len())].iter().fold(f32::MIN, |m, &v| m.max(v));
            }
            if sum > best.0 {
                best = (sum, f);
            }
            f += 0.25;
        }
        raw.push(best.1 * 120.0);
        at += hop;
    }
    (0..raw.len())
        .map(|i| {
            let mut w: Vec<f32> = raw[i.saturating_sub(2)..(i + 3).min(raw.len())].to_vec();
            w.sort_by(f32::total_cmp);
            w[w.len() / 2]
        })
        .collect()
}

/// Reflex's 450F, decoded and sorted.
pub struct Bank {
    clips: Vec<Recording>,
}

impl Bank {
    pub fn from_reflex(clips: Vec<reflex_assets::engine_audio::EngineClip>) -> Bank {
        let clips = clips
            .into_iter()
            .filter_map(|c| {
                let &(_, rpm, kind) = CLIPS_450F.iter().find(|(n, ..)| *n == c.name)?;
                if kind == Kind::Unused || c.samples.len() < 4096 {
                    return None;
                }
                let rms = (c.samples.iter().map(|v| v * v).sum::<f32>() / c.samples.len() as f32).sqrt();
                let k = 0.25 / rms.max(1e-4);
                // The idle and the sweeps are played as they are.
                let contour = match kind {
                    Kind::Drive(_) | Kind::Limiter => contour(&c.samples, c.rate, rpm),
                    _ => Vec::new(),
                };
                Some(Recording {
                    samples: c.samples.iter().map(|v| v * k).collect(),
                    step: c.rate as f64 / RATE as f64,
                    rpm,
                    contour,
                    kind,
                })
            })
            .collect();
        Bank { clips }
    }

    fn of(&self, f: impl Fn(Kind) -> bool) -> Vec<usize> {
        (0..self.clips.len()).filter(|&i| f(self.clips[i].kind)).collect()
    }
}

/// xorshift.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 17;
        self.0 ^= self.0 << 5;
        self.0
    }
    /// 0 to 1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / (1 << 24) as f32
    }
    /// -1 to 1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One-pole low-pass coefficient for `hz`.
fn one_pole(hz: f32) -> f32 {
    1.0 - (-std::f32::consts::TAU * hz / RATE as f32).exp()
}

/// Smoothed copies of the inputs, and the edges the models react to.
struct Follow {
    input: Input,
    rpm: f32,
    throttle: f32,
    /// The throttle held in the last half second or so.
    recent: f32,
    landings: u32,
    wrecks: u32,
}

impl Follow {
    fn new(input: Input) -> Follow {
        Follow {
            input,
            rpm: input.rpm,
            throttle: input.throttle,
            recent: input.throttle,
            landings: input.landings,
            wrecks: input.wrecks,
        }
    }
    /// Takes this block's input; returns the rpm and throttle at its start.
    fn take(&mut self, shared: &Mutex<Input>, seconds: f32) -> (f32, f32) {
        self.input = *shared.lock().unwrap();
        let before = (self.rpm, self.throttle);
        // About 40 ms either way; the physics already smooths the pedal.
        let k = (seconds / 0.04).min(1.0);
        self.rpm += (self.input.rpm - self.rpm) * k;
        self.throttle += (self.input.throttle - self.throttle) * k;
        self.recent = self.throttle.max(self.recent - seconds * 2.0);
        before
    }
}

/// Air past the rider: low noise, louder and brighter with speed.
struct Wind {
    lp: [f32; 2],
    rng: Rng,
}

impl Wind {
    fn sample(&mut self, speed: f32) -> f32 {
        let level = (speed / 30.0).powi(2).min(1.5) * 0.18;
        if level < 1e-4 {
            return 0.0;
        }
        let k = one_pole(150.0 + speed * 25.0);
        let n = self.rng.signed();
        self.lp[0] += (n - self.lp[0]) * k;
        self.lp[1] += (self.lp[0] - self.lp[1]) * k;
        self.lp[1] * level * 3.0
    }
}

/// A recording playing, with its own fade.
struct Grain {
    clip: usize,
    pos: f64,
    /// 0 to 1, equal power.
    fade: f32,
    /// Per output frame; negative while fading out.
    fade_step: f32,
}

/// A layer: plays recordings from `pool` one after another, the one
/// nearest the rpm and load, crossfaded.
struct Layer {
    pool: Vec<usize>,
    grains: Vec<Grain>,
    since: f32,
    /// Start somewhere into the recording, not always at its start.
    random_start: bool,
}

const XFADE: f32 = 0.09;

impl Layer {
    fn new(pool: Vec<usize>, random_start: bool) -> Layer {
        Layer {
            pool,
            grains: Vec::new(),
            since: 0.0,
            random_start,
        }
    }

    /// How far `clip` is from what is wanted: pitch apart (log) and load apart.
    fn miss(bank: &Bank, clip: usize, rpm: f32, load: f32) -> f32 {
        let c = &bank.clips[clip];
        let pitch = (rpm / c.rpm).ln().abs();
        let l = match c.kind {
            Kind::Drive(l) => (l - load).abs(),
            _ => 0.0,
        };
        pitch + 0.25 * l
    }

    fn pick(&self, bank: &Bank, rpm: f32, load: f32, rng: &mut Rng, not: Option<usize>) -> Option<usize> {
        let mut scored: Vec<(f32, usize)> = self
            .pool
            .iter()
            .filter(|&&c| Some(c) != not || self.pool.len() == 1)
            .map(|&c| (Self::miss(bank, c, rpm, load), c))
            .collect();
        scored.sort_by(|a, b| a.0.total_cmp(&b.0));
        // One of the nearest few, so it does not loop one take.
        let near: Vec<usize> = scored
            .iter()
            .take_while(|(m, _)| *m <= scored[0].0 + 0.06)
            .take(3)
            .map(|&(_, c)| c)
            .collect();
        (!near.is_empty()).then(|| near[rng.next() as usize % near.len()])
    }

    fn start(&mut self, bank: &Bank, clip: usize, rng: &mut Rng) {
        let fade_step = 1.0 / (XFADE * RATE as f32);
        for g in &mut self.grains {
            g.fade_step = -fade_step;
        }
        let len = bank.clips[clip].samples.len() as f64;
        let pos = if self.random_start { rng.unit() as f64 * len * 0.5 } else { 0.0 };
        self.grains.push(Grain {
            clip,
            pos,
            fade: 0.0,
            fade_step,
        });
        self.since = 0.0;
    }

    /// Adds `frames` mono frames into `out`, the gain ramping `g0` to `g1`,
    /// pitched to `rpm` (`rate` overrides: a fixed playback rate).
    #[allow(clippy::too_many_arguments)]
    fn render(&mut self, bank: &Bank, out: &mut [f32], rpm: f32, load: f32, g0: f32, g1: f32, rate: Option<f32>, rng: &mut Rng) {
        let frames = out.len();
        self.since += frames as f32 / RATE as f32;
        if g0 < 1e-4 && g1 < 1e-4 {
            // Silent: let it go, and start fresh when it is wanted again.
            self.grains.clear();
            return;
        }
        let ratio = |c: &Recording, pos: f64| rate.unwrap_or_else(|| rpm / c.rpm_at(pos)).clamp(0.4, 2.2) as f64 * c.step;
        let current = self.grains.iter().rev().find(|g| g.fade_step > 0.0 || g.fade >= 1.0).map(|g| g.clip);
        let wanted = match current {
            None => true,
            Some(c) => {
                let g = self.grains.iter().rev().find(|g| g.clip == c).unwrap();
                let clip = &bank.clips[c];
                let left = (clip.samples.len() as f64 - g.pos) / ratio(clip, g.pos);
                let ending = left < (XFADE * RATE as f32) as f64 + frames as f64;
                let off = Self::miss(bank, c, rpm, load);
                let better = self.pick(bank, rpm, load, rng, None).map(|b| Self::miss(bank, b, rpm, load));
                ending || (self.since > 0.25 && off > 0.17 && better.is_some_and(|b| b < off - 0.08))
            }
        };
        if wanted && let Some(c) = self.pick(bank, rpm, load, rng, current) {
            self.start(bank, c, rng);
        }
        for g in &mut self.grains {
            let clip = &bank.clips[g.clip];
            let n = clip.samples.len();
            for (f, o) in out.iter_mut().enumerate() {
                let step = ratio(clip, g.pos);
                let i = g.pos as usize;
                if i + 1 >= n {
                    g.fade = 0.0;
                    g.fade_step = -1.0;
                    break;
                }
                let t = (g.pos - i as f64) as f32;
                let v = clip.samples[i] + (clip.samples[i + 1] - clip.samples[i]) * t;
                let w = (g.fade * std::f32::consts::FRAC_PI_2).sin();
                let gain = g0 + (g1 - g0) * f as f32 / frames as f32;
                *o += v * w * gain;
                g.pos += step;
                g.fade = (g.fade + g.fade_step).clamp(0.0, 1.0);
            }
        }
        self.grains.retain(|g| g.fade > 0.0 || g.fade_step > 0.0);
    }
}

/// A recording played once.
struct OneShot {
    clip: usize,
    pos: f64,
    step: f64,
    gain: f32,
}

/// The Reflex model.
pub struct Sampled {
    bank: Arc<Bank>,
    shared: Arc<Mutex<Input>>,
    follow: Option<Follow>,
    idle: Layer,
    drive: Layer,
    coast: Layer,
    limiter: Layer,
    shots: Vec<OneShot>,
    decel_wait: f32,
    /// Seconds the decel sweep still covers the coast layer.
    decel_left: f32,
    coast_lp: f32,
    gains: [f32; 4],
    wind: Wind,
    rng: Rng,
    mono: Vec<f32>,
    coast_buf: Vec<f32>,
}

impl Sampled {
    pub fn new(bank: Arc<Bank>, shared: Arc<Mutex<Input>>) -> Sampled {
        let idle = Layer::new(bank.of(|k| k == Kind::Idle), true);
        let drive = Layer::new(bank.of(|k| matches!(k, Kind::Drive(_))), true);
        // Off the throttle while still turning over: the light takes, dulled.
        let coast = Layer::new(bank.of(|k| matches!(k, Kind::Drive(l) if l <= 0.35)), true);
        let limiter = Layer::new(bank.of(|k| k == Kind::Limiter), true);
        Sampled {
            bank,
            shared,
            follow: None,
            idle,
            drive,
            coast,
            limiter,
            shots: Vec::new(),
            decel_wait: 0.0,
            decel_left: 0.0,
            coast_lp: 0.0,
            gains: [0.0; 4],
            wind: Wind { lp: [0.0; 2], rng: Rng(0x2545_f491) },
            rng: Rng(0x9e37_79b9),
            mono: Vec::new(),
            coast_buf: Vec::new(),
        }
    }

    fn shot(&mut self, kind: Kind, rpm: Option<f32>, gain: f32) {
        let pool = self.bank.of(|k| k == kind);
        if pool.is_empty() {
            return;
        }
        let clip = pool[self.rng.next() as usize % pool.len()];
        let c = &self.bank.clips[clip];
        let rate = rpm.map_or(1.0, |r| (r / c.rpm).clamp(0.6, 1.5)) as f64;
        self.shots.push(OneShot {
            clip,
            pos: 0.0,
            step: rate * c.step,
            gain,
        });
    }
}

impl Generator for Sampled {
    fn render(&mut self, out: &mut [f32]) -> bool {
        let frames = out.len() / 2;
        let seconds = frames as f32 / RATE as f32;
        let follow = self.follow.get_or_insert_with(|| Follow::new(*self.shared.lock().unwrap()));
        let (_, t0) = follow.take(&self.shared, seconds);
        let (i, rpm, t, recent) = (follow.input, follow.rpm, follow.throttle, follow.recent);
        let landed = i.landings != follow.landings;
        let wrecked = i.wrecks != follow.wrecks;
        follow.landings = i.landings;
        follow.wrecks = i.wrecks;

        // Letting go from high up: the falling sweep, once.
        self.decel_wait -= seconds;
        self.decel_left -= seconds;
        if t < 0.1 && t0 >= 0.1 && recent > 0.5 && rpm > 6500.0 && self.decel_wait <= 0.0 {
            self.shot(Kind::Decel, Some(rpm * 0.98), 0.75);
            self.decel_wait = 1.5;
            self.decel_left = 0.8;
        }
        if landed && i.land_force > 0.1 {
            self.shot(Kind::Land, Some(rpm), 0.35 + 0.45 * i.land_force);
        }
        if wrecked {
            self.shot(Kind::Wreck, None, 0.9);
        }

        let (lo, hi) = (i.min_rpm.max(500.0), i.max_rpm.max(i.min_rpm + 1000.0));
        let on = smoothstep(0.02, 0.2, t);
        let limit = smoothstep(hi * 0.95, hi * 0.99, rpm) * smoothstep(0.5, 0.8, t);
        let duck = if self.decel_left > 0.0 { 0.3 } else { 1.0 };
        let target = [
            (1.0 - on) * (1.0 - smoothstep(lo + 800.0, lo + 2600.0, rpm)) * 0.5,
            on * (0.5 + 0.7 * t) * (1.0 - 0.7 * limit),
            (1.0 - on) * smoothstep(lo + 400.0, lo + 2200.0, rpm) * 0.5 * duck,
            limit * 0.9,
        ];
        let g0 = self.gains;
        self.gains = target;

        self.mono.clear();
        self.mono.resize(frames, 0.0);
        self.coast_buf.clear();
        self.coast_buf.resize(frames, 0.0);
        let bank = self.bank.clone();
        // The idle recording runs a little fast of our idle: as recorded at
        // idle, rising with the rpm.
        let idle_rate = (rpm / lo).clamp(1.0, 1.8).powf(0.85);
        self.idle.render(&bank, &mut self.mono, rpm, 0.0, g0[0], target[0], Some(idle_rate), &mut self.rng);
        self.drive.render(&bank, &mut self.mono, rpm, t, g0[1], target[1], None, &mut self.rng);
        self.coast.render(&bank, &mut self.coast_buf, rpm, 0.2, g0[2], target[2], None, &mut self.rng);
        self.limiter.render(&bank, &mut self.mono, rpm, 1.0, g0[3], target[3], None, &mut self.rng);
        let k = one_pole(1400.0);
        for (m, c) in self.mono.iter_mut().zip(&self.coast_buf) {
            self.coast_lp += (c - self.coast_lp) * k;
            *m += self.coast_lp;
        }
        for s in &mut self.shots {
            let c = &bank.clips[s.clip];
            for m in self.mono.iter_mut() {
                let i = s.pos as usize;
                if i + 1 >= c.samples.len() {
                    break;
                }
                let f = (s.pos - i as f64) as f32;
                *m += (c.samples[i] + (c.samples[i + 1] - c.samples[i]) * f) * s.gain;
                s.pos += s.step;
            }
        }
        self.shots.retain(|s| (s.pos as usize) + 1 < bank.clips[s.clip].samples.len());

        for (f, m) in self.mono.iter().enumerate() {
            let v = (m * LEVEL + self.wind.sample(i.speed)) * 0.9;
            out[2 * f] += v;
            out[2 * f + 1] += v;
        }
        true
    }
}

/// Both models' loudness in the mix (the board's roll is 0.5).
const LEVEL: f32 = 0.55;

/// The synth model: a single-cylinder four-stroke from first principles.
pub struct Synth {
    shared: Arc<Mutex<Input>>,
    follow: Option<Follow>,
    /// Through the four-stroke cycle (two turns of the crank), 0 to 1.
    phase: f32,
    /// Since this cycle fired, seconds.
    since: f32,
    /// This firing's strength and the stretch of its pulse.
    amp: f32,
    width: f32,
    /// Seconds of popping left after letting go of the throttle.
    pops: f32,
    head: Vec<f32>,
    silencer: Vec<f32>,
    at: [usize; 2],
    loop_lp: [f32; 2],
    tone: [f32; 2],
    intake: [f32; 2],
    tick: f32,
    dc: [f32; 2],
    wind: Wind,
    rng: Rng,
}

/// Head pipe and silencer lengths as delays: a round trip of about 1.1 m
/// and 0.3 m of pipe at 343 m/s.
const HEAD: usize = 300;
const SILENCER: usize = 90;

impl Synth {
    pub fn new(shared: Arc<Mutex<Input>>) -> Synth {
        Synth {
            shared,
            follow: None,
            phase: 0.0,
            since: 1.0,
            amp: 0.0,
            width: 0.004,
            pops: 0.0,
            head: vec![0.0; HEAD],
            silencer: vec![0.0; SILENCER],
            at: [0; 2],
            loop_lp: [0.0; 2],
            tone: [0.0; 2],
            intake: [0.0; 2],
            tick: 0.0,
            dc: [0.0; 2],
            wind: Wind { lp: [0.0; 2], rng: Rng(0x6a09_e667) },
            rng: Rng(0xbb67_ae85),
        }
    }

    /// A new firing: how hard, from the load, with cycle-to-cycle spread;
    /// the limiter and a closed throttle cut some.
    fn fire(&mut self, rpm: f32, load: f32, i: &Input) {
        let spread = 0.12 + 0.15 * (1.0 - load);
        let mut amp = (0.5 + 0.5 * load.powf(0.8)) * (1.0 + spread * self.rng.signed());
        let hi = i.max_rpm.max(i.min_rpm + 1000.0);
        if rpm >= hi * 0.985 && load > 0.4 && self.rng.unit() < 0.5 {
            // The limiter cuts the spark.
            amp *= 0.05;
        }
        if load < 0.05 && rpm > i.min_rpm + 1500.0 {
            // Engine braking: weak, lean firings, and now and then the
            // unburnt mix going off in the pipe.
            amp = 0.07 * (1.0 + 0.5 * self.rng.signed());
            if self.pops > 0.0 && self.rng.unit() < 0.12 {
                amp = 0.9 + 0.5 * self.rng.unit();
            }
        }
        self.amp = amp;
        // The exhaust valve open for about 80° of crank.
        let crank = rpm / 60.0;
        self.width = (0.22 / crank).clamp(0.0012, 0.009);
        self.since = 0.0;
    }
}

impl Generator for Synth {
    fn render(&mut self, out: &mut [f32]) -> bool {
        let frames = out.len() / 2;
        let dt = 1.0 / RATE as f32;
        let follow = self.follow.get_or_insert_with(|| Follow::new(*self.shared.lock().unwrap()));
        let (rpm0, t0) = follow.take(&self.shared, frames as f32 * dt);
        let (i, rpm1, t1, recent) = (follow.input, follow.rpm, follow.throttle, follow.recent);
        follow.landings = i.landings;
        follow.wrecks = i.wrecks;
        if t1 < 0.1 && t0 >= 0.1 && recent > 0.4 && rpm1 > 5000.0 {
            self.pops = 1.2;
        }
        self.pops -= frames as f32 * dt;
        for f in 0..frames {
            let a = f as f32 / frames as f32;
            let rpm = rpm0 + (rpm1 - rpm0) * a;
            let load = t0 + (t1 - t0) * a;
            let cycle = rpm / 120.0;
            let before = self.phase;
            self.phase += cycle * dt * (1.0 + 0.01 * self.rng.signed());
            if self.phase >= 1.0 {
                self.phase -= 1.0;
                self.fire(rpm, load, &i);
            }
            self.since += dt;

            // The blow-down pulse, with turbulence riding on it.
            let x = self.since / (self.width * 0.35);
            let env = if x < 10.0 { x * (1.0 - x).exp() } else { 0.0 };
            let noise = self.rng.signed();
            let src = self.amp * env * (1.0 + noise * (0.3 + 0.5 * load));

            // Head pipe: the open end sends the pulse back inverted.
            let d = self.head[self.at[0]];
            self.loop_lp[0] += (d - self.loop_lp[0]) * 0.35;
            let y1 = src - 0.62 * self.loop_lp[0];
            self.head[self.at[0]] = y1;
            self.at[0] = (self.at[0] + 1) % HEAD;
            // Silencer chamber.
            let d = self.silencer[self.at[1]];
            self.loop_lp[1] += (d - self.loop_lp[1]) * 0.4;
            let y2 = 0.5 * y1 + 0.45 * self.loop_lp[1];
            self.silencer[self.at[1]] = y2;
            self.at[1] = (self.at[1] + 1) % SILENCER;
            let body = 0.55 * y1 + 0.45 * y2;

            // The tone opens with the throttle and the revs.
            let k = one_pole((900.0 + 5200.0 * load.powf(0.8) + rpm * 0.25).min(9000.0));
            self.tone[0] += (body - self.tone[0]) * k;
            self.tone[1] += (self.tone[0] - self.tone[1]) * k;
            let mut v = self.tone[1];

            // Intake snarl while the intake valve is open, with the throttle.
            if (0.45..0.7).contains(&self.phase) {
                let e = ((self.phase - 0.45) / 0.25 * std::f32::consts::PI).sin();
                let kc = one_pole(900.0 + rpm * 0.1);
                self.intake[0] += (noise - self.intake[0]) * kc;
                self.intake[1] += (self.intake[0] - self.intake[1]) * kc;
                v += (self.intake[0] - self.intake[1]) * e * load * 0.5;
            }
            // Valve gear: a small click at each valve event, heard most off
            // the throttle.
            if [0.25f32, 0.5].iter().any(|&p| before < p && self.phase >= p) {
                self.tick = 1.0;
            }
            if self.tick > 1e-3 {
                v += self.rng.signed() * self.tick * (0.02 + 0.03 * (1.0 - load));
                self.tick *= 0.85;
            }

            // A little saturation, harder with load; the DC taken out.
            let drive = 1.3 + 1.7 * load;
            v = (v * drive).tanh() / drive.tanh();
            self.dc[1] = v - self.dc[0] + 0.995 * self.dc[1];
            self.dc[0] = v;
            let s = (self.dc[1] * LEVEL * 0.6 + self.wind.sample(i.speed)) * 0.9;
            out[2 * f] += s;
            out[2 * f + 1] += s;
        }
        true
    }
}

/// The engine sound's state on the main thread.
#[derive(Resource, Default)]
pub struct EngineSound {
    model: Model,
    playing: Option<(Model, VoiceId)>,
    shared: Arc<Mutex<Input>>,
    bank: Option<Arc<Bank>>,
    loading: Option<Mutex<Receiver<Option<Bank>>>>,
    tried: bool,
    air: f32,
    crashed: bool,
    landings: u32,
    wrecks: u32,
}

/// Follows `BikeMotion` into the voice; H / the left stick click switches
/// the model.
pub fn bike_engine(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
    motion: Option<Res<BikeMotion>>,
    mixer: Res<Mixer>,
    mut e: ResMut<EngineSound>,
) {
    let Some(m) = motion else {
        return;
    };
    let pad = gamepads.iter().next();
    let click = pad.is_some_and(|p| p.just_pressed(B::LeftThumb) && !p.pressed(B::RightThumb));
    if keys.just_pressed(KeyCode::KeyH) || (m.riding && click) {
        e.model = e.model.next();
        let name = match e.model {
            Model::Reflex if e.bank.is_none() && e.tried && e.loading.is_none() => "Reflex (not found: synth)",
            Model::Reflex => "Reflex's 450F recordings",
            Model::Synth => "synth",
            Model::Off => "off",
        };
        info!("bike: engine sound {name}");
    }
    if !m.riding {
        if let Some((_, id)) = e.playing.take() {
            mixer.stop(id);
        }
        e.air = 0.0;
        return;
    }
    if !e.tried {
        e.tried = true;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let bank = reflex_assets::find_reflex().and_then(|root| match reflex_assets::engine_audio::load_450f(&root) {
                Ok(clips) => Some(Bank::from_reflex(clips)),
                Err(err) => {
                    warn!("bike: Reflex engine sounds: {err}");
                    None
                }
            });
            let _ = tx.send(bank);
        });
        e.loading = Some(Mutex::new(rx));
    }
    if let Some(rx) = e.loading.as_ref() {
        let got = rx.lock().unwrap().try_recv();
        match got {
            Ok(bank) => {
                if bank.is_none() {
                    info!("bike: no Reflex engine sounds; the synth plays instead");
                }
                e.bank = bank.map(Arc::new);
                e.loading = None;
                // Restart on the recordings if the synth stood in.
                if let Some((_, id)) = e.playing.take() {
                    mixer.stop(id);
                }
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => e.loading = None,
        }
    }

    // Landings and crashes, as counters the audio thread sees change.
    let dt = time.delta_secs();
    if m.grounded == 0 && !m.crashed {
        e.air += dt;
    } else {
        if e.air > 0.35 && m.grounded > 0 {
            e.landings += 1;
            let force = ((e.air - 0.35) / 1.2).clamp(0.0, 1.0) * (0.4 + 0.6 * m.throttle);
            let mut s = e.shared.lock().unwrap();
            s.land_force = force;
        }
        e.air = 0.0;
    }
    if m.crashed && !e.crashed {
        e.wrecks += 1;
    }
    e.crashed = m.crashed;
    {
        let (landings, wrecks, air) = (e.landings, e.wrecks, e.air > 0.0);
        let mut s = e.shared.lock().unwrap();
        s.rpm = m.rpm;
        s.min_rpm = m.min_rpm;
        s.max_rpm = m.max_rpm;
        s.throttle = if m.crashed { 0.0 } else { m.throttle };
        s.speed = m.speed;
        s.airborne = air;
        s.landings = landings;
        s.wrecks = wrecks;
    }

    // The voice for the model (the synth while Reflex loads or is missing).
    let wanted = match e.model {
        Model::Reflex if e.bank.is_some() => Some(Model::Reflex),
        Model::Off => None,
        _ => Some(Model::Synth),
    };
    if e.playing.map(|(m, _)| m) != wanted {
        if let Some((_, id)) = e.playing.take() {
            mixer.stop(id);
        }
        if let Some(model) = wanted {
            let g: Box<dyn Generator> = match model {
                Model::Reflex => Box::new(Sampled::new(e.bank.clone().unwrap(), e.shared.clone())),
                _ => Box::new(Synth::new(e.shared.clone())),
            };
            e.playing = Some((model, mixer.add_generator(g)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ride for either model: idle, full gas up through the revs, a
    /// shift, off the gas, the limiter in the air, a landing.
    fn ride(g: &mut dyn Generator, shared: &Mutex<Input>) -> Vec<f32> {
        let mut out = Vec::new();
        let block = 1024;
        let mut buf = vec![0.0; block * 2];
        let steps = (8.0 * RATE as f32 / block as f32) as usize;
        for n in 0..steps {
            let t = n as f32 * block as f32 / RATE as f32;
            {
                let mut s = shared.lock().unwrap();
                s.min_rpm = 1500.0;
                s.max_rpm = 11500.0;
                let (rpm, gas) = match t {
                    t if t < 1.5 => (1500.0, 0.0),
                    t if t < 3.0 => (1500.0 + (t - 1.5) / 1.5 * 8500.0, 1.0),
                    t if t < 3.2 => (10000.0 - (t - 3.0) * 15000.0, 1.0),
                    t if t < 4.5 => (7000.0 + (t - 3.2) * 2000.0, 0.7),
                    t if t < 5.5 => (9600.0 - (t - 4.5) * 5000.0, 0.0),
                    t if t < 7.0 => (11500.0, 1.0),
                    _ => (8000.0, 0.6),
                };
                s.rpm = rpm;
                s.throttle = gas;
                s.speed = (t * 4.0).min(20.0);
                if (7.0..7.03).contains(&t) {
                    s.landings += 1;
                    s.land_force = 0.8;
                }
            }
            buf.fill(0.0);
            assert!(g.render(&mut buf));
            out.extend_from_slice(&buf);
        }
        out
    }

    #[test]
    fn synth_is_heard_and_stays_in_range() {
        let shared = Arc::new(Mutex::new(Input::default()));
        let mut s = Synth::new(shared.clone());
        let out = ride(&mut s, &shared);
        let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
        let rms = |a: usize, b: usize| {
            let a = a * RATE as usize * 2;
            let b = b * RATE as usize * 2;
            (out[a..b].iter().map(|v| v * v).sum::<f32>() / (b - a) as f32).sqrt()
        };
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(peak < 1.0, "peak {peak}");
        // Idle is quieter than full gas.
        assert!(rms(0, 1) > 0.005 && rms(2, 3) > rms(0, 1) * 1.5, "idle {} gas {}", rms(0, 1), rms(2, 3));
        if let Some(path) = std::env::var_os("HALO3_ENGINE_WAV") {
            write_wav(&std::path::PathBuf::from(path).join("engine-synth.wav"), &out);
        }
    }

    /// The Reflex model on the user's own recordings; skipped without Reflex.
    #[test]
    fn reflex_model_plays() {
        let Some(root) = reflex_assets::find_reflex() else {
            eprintln!("no Reflex install; skipped");
            return;
        };
        let bank = Arc::new(Bank::from_reflex(reflex_assets::engine_audio::load_450f(&root).unwrap()));
        assert_eq!(bank.clips.len(), CLIPS_450F.iter().filter(|c| c.2 != Kind::Unused).count());
        let shared = Arc::new(Mutex::new(Input::default()));
        let mut s = Sampled::new(bank, shared.clone());
        let out = ride(&mut s, &shared);
        let peak = out.iter().fold(0f32, |m, v| m.max(v.abs()));
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(peak > 0.05 && peak < 1.5, "peak {peak}");
        if let Some(path) = std::env::var_os("HALO3_ENGINE_WAV") {
            write_wav(&std::path::PathBuf::from(path).join("engine-reflex.wav"), &out);
        }
    }

    fn write_wav(path: &std::path::Path, stereo: &[f32]) {
        let mut b = Vec::new();
        let data = (stereo.len() * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        for v in [16u32] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [1u16, 2] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [RATE, RATE * 4] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        for v in [4u16, 16] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for v in stereo {
            b.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(path, b).unwrap();
    }
}
