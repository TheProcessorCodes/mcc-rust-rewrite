//! The dirt bike's engine, two ways, to compare (H, or the left stick click
//! while riding, goes Reflex → synth → off):
//!
//! - **Reflex**: the Dakumoto 450F's own recordings, read from the user's
//!   MX vs ATV Reflex install (`reflex_assets::engine_audio`), played the
//!   way Reflex plays them. Matching a recording of the game against the
//!   takes (2026-10-30) showed that Reflex never pitches them: on the gas
//!   it starts a take from its beginning at each gear change and lets it
//!   run, at the pitch and level it was recorded at, so the climb through
//!   a gear is the take's own; when a take ends the next one in the pool
//!   follows. The pools are the event file's sound definitions (four
//!   throttle pools, idle, half throttle, the limiter, decel, landing,
//!   wreck); which pool plays in which gear is ours to choose.
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
    /// Smoothed (see `bike_engine`).
    pub rpm: f32,
    pub min_rpm: f32,
    pub max_rpm: f32,
    pub throttle: f32,
    /// 0-based.
    pub gear: usize,
    /// m/s.
    pub speed: f32,
    /// In the air for more than a moment.
    pub airborne: bool,
    pub crashed: bool,
    /// Counts up once per landing and per crash; `land_force` 0 to 1.
    pub landings: u32,
    pub land_force: f32,
    pub wrecks: u32,
}

/// The event file's sound definitions the Reflex model plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pool {
    Idle,
    HalfAccel,
    Half,
    Low,
    Mid,
    High,
    Power,
    DecelHigh,
    DecelLow,
    PanicStart,
    PanicLoop,
    EnterAir,
    Land,
    Wreck,
}

const POOLS: [(Pool, &str); 14] = [
    (Pool::Idle, "MXB_450F_IDLE"),
    (Pool::HalfAccel, "MXB_450F_HALF_ACCEL"),
    (Pool::Half, "MXB_450F_HALF_THROTTLE"),
    (Pool::Low, "MXB_450F_THROTTLE_LOW"),
    (Pool::Mid, "MXB_450F_THROTTLE_MID"),
    (Pool::High, "MXB_450F_THROTTLE_HIGH"),
    (Pool::Power, "MXB_450F_THROTTLE_POWER"),
    (Pool::DecelHigh, "MXB_450F_DECEL_HIGH"),
    (Pool::DecelLow, "MXB_450F_DECEL_LOW"),
    (Pool::PanicStart, "MXB_450F_PANIC_START"),
    (Pool::PanicLoop, "MXB_450F_PANIC_LOOP"),
    (Pool::EnterAir, "MXB_450F_ENTER_AIR"),
    (Pool::Land, "MXB_450F_THROTTLE_LAND"),
    (Pool::Wreck, "MXB_450F_WRECK"),
];

impl Pool {
    /// The throttle pool for a gear: pulling hard in first, then up through
    /// the low, mid and high pools.
    fn for_gear(gear: usize) -> Pool {
        match gear {
            0 => Pool::Power,
            1 => Pool::Low,
            2 => Pool::Mid,
            _ => Pool::High,
        }
    }

    /// What follows when a take from this pool ends.
    fn then(self) -> Pool {
        match self {
            Pool::HalfAccel => Pool::Half,
            Pool::PanicStart | Pool::EnterAir => Pool::PanicLoop,
            p => p,
        }
    }
}

struct Recording {
    samples: Vec<f32>,
    /// Source frames per output frame.
    step: f64,
}

/// Reflex's 450F, decoded: the takes at their own level, and the pools.
pub struct Bank {
    clips: Vec<Recording>,
    pools: Vec<(Pool, Vec<usize>)>,
}

impl Bank {
    pub fn from_reflex(engine: reflex_assets::engine_audio::Engine) -> Bank {
        let names: Vec<String> = engine.clips.iter().map(|c| c.name.clone()).collect();
        let clips = engine
            .clips
            .into_iter()
            .map(|c| Recording {
                step: c.rate as f64 / RATE as f64,
                samples: c.samples,
            })
            .collect();
        let pools = POOLS
            .iter()
            .map(|&(pool, def)| {
                let takes = engine.defs.iter().find(|d| d.name == def).map_or(Vec::new(), |d| {
                    d.takes.iter().filter_map(|t| names.iter().position(|n| n == t)).collect()
                });
                (pool, takes)
            })
            .collect();
        Bank { clips, pools }
    }

    fn pool(&self, pool: Pool) -> &[usize] {
        self.pools.iter().find(|p| p.0 == pool).map_or(&[], |p| &p.1)
    }

    /// A take from `pool`, not `last` when there is another (FMOD's random
    /// without repeats, the throttle pools' play mode).
    fn pick(&self, pool: Pool, last: Option<usize>, rng: &mut Rng) -> Option<usize> {
        let takes = self.pool(pool);
        let others: Vec<usize> = takes.iter().copied().filter(|&t| Some(t) != last).collect();
        let from = if others.is_empty() { takes } else { &others[..] };
        (!from.is_empty()).then(|| from[rng.next() as usize % from.len()])
    }
}

/// A take playing.
struct Voice {
    clip: usize,
    pos: f64,
    /// 0 to 1, equal power.
    fade: f32,
    /// Per output frame; negative while fading out.
    fade_step: f32,
    gain: f32,
    /// What its pool is (to chain the next take), `None` for a one-shot.
    pool: Option<Pool>,
}

fn fade_step(seconds: f32) -> f32 {
    1.0 / (seconds.max(0.001) * RATE as f32)
}

/// Four-point Hermite between `s[i]` and `s[i + 1]` (`s[i + 2]` must exist).
fn cubic(s: &[f32], i: usize, t: f32) -> f32 {
    let (y0, y1, y2, y3) = (s[i.saturating_sub(1)], s[i], s[i + 1], s[i + 2]);
    let c1 = 0.5 * (y2 - y0);
    let c2 = y0 - 2.5 * y1 + 2.0 * y2 - 0.5 * y3;
    let c3 = 0.5 * (y3 - y0) + 1.5 * (y1 - y2);
    ((c3 * t + c2) * t + c1) * t + y1
}

/// What the engine track plays.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum State {
    Idle,
    Half,
    Drive,
    Limiter,
}

/// FMOD's event volume on every 450F event.
const EVENT_VOLUME: f32 = 0.8;
/// The gap between one gear's take and the next.
const SHIFT_GAP: f32 = 0.08;
/// Two takes overlap this long where one runs into the next.
const CHAIN: f32 = 0.012;

/// The Reflex model.
pub struct Sampled {
    bank: Arc<Bank>,
    shared: Arc<Mutex<Input>>,
    follow: Option<Follow>,
    state: State,
    gear: usize,
    /// The engine track's takes (the current one last) and one-shots.
    voices: Vec<Voice>,
    shots: Vec<Voice>,
    /// A take to start after a delay: the pool, seconds, fade-in seconds,
    /// and where in the take (0 to 1).
    pending: Option<(Pool, f32, f32, f32)>,
    last: Option<usize>,
    /// Seconds since the engine track's current take started.
    since: f32,
    wind: Wind,
    rng: Rng,
    mono: Vec<f32>,
}

impl Sampled {
    pub fn new(bank: Arc<Bank>, shared: Arc<Mutex<Input>>) -> Sampled {
        Sampled {
            bank,
            shared,
            follow: None,
            state: State::Idle,
            gear: 0,
            voices: Vec::new(),
            shots: Vec::new(),
            pending: Some((Pool::Idle, 0.0, 0.05, 0.5)),
            last: None,
            since: 0.0,
            wind: Wind { lp: [0.0; 2], rng: Rng(0x2545_f491) },
            rng: Rng(0x9e37_79b9),
            mono: Vec::new(),
        }
    }

    /// Fades out the engine track over `seconds`, and starts `pool` after
    /// `delay`.
    fn switch(&mut self, pool: Pool, seconds: f32, delay: f32, fade_in: f32) {
        for v in &mut self.voices {
            v.fade_step = -fade_step(seconds);
        }
        self.pending = Some((pool, delay, fade_in, 0.0));
    }

    fn start(&mut self, pool: Pool, fade_in: f32, at: f32) {
        let Some(clip) = self.bank.pick(pool, self.last, &mut self.rng) else {
            return;
        };
        self.last = Some(clip);
        self.since = 0.0;
        let len = self.bank.clips[clip].samples.len() as f64;
        self.voices.push(Voice {
            clip,
            pos: at as f64 * len,
            fade: 0.0,
            fade_step: fade_step(fade_in),
            gain: EVENT_VOLUME,
            pool: Some(pool),
        });
    }

    fn shot(&mut self, pool: Pool) {
        if let Some(clip) = self.bank.pick(pool, None, &mut self.rng) {
            self.shots.push(Voice {
                clip,
                pos: 0.0,
                fade: 1.0,
                fade_step: 0.0,
                gain: EVENT_VOLUME,
                pool: None,
            });
        }
    }

    /// Adds the voices into `out`; a take on the track that ends hands on
    /// to the next from its pool.
    fn play(bank: &Bank, voices: &mut Vec<Voice>, out: &mut [f32]) -> Option<Pool> {
        let mut next = None;
        let chain = (CHAIN * RATE as f32) as f64;
        let current = voices.len().wrapping_sub(1);
        for (k, v) in voices.iter_mut().enumerate() {
            let c = &bank.clips[v.clip];
            let n = c.samples.len();
            for o in out.iter_mut() {
                let i = v.pos as usize;
                if i + 2 >= n {
                    v.fade = 0.0;
                    v.fade_step = -1.0;
                    break;
                }
                // Near the end: the next take comes in under this one.
                if k == current && v.fade_step >= 0.0 && v.pool.is_some() && (n as f64 - v.pos) / c.step < chain {
                    v.fade_step = -fade_step(CHAIN);
                    next = v.pool.map(Pool::then);
                }
                let w = (v.fade * std::f32::consts::FRAC_PI_2).sin();
                *o += cubic(&c.samples, i, (v.pos - i as f64) as f32) * w * v.gain;
                v.pos += c.step;
                v.fade = (v.fade + v.fade_step).clamp(0.0, 1.0);
            }
        }
        voices.retain(|v| v.fade > 0.0 || v.fade_step > 0.0);
        next
    }
}

impl Generator for Sampled {
    fn render(&mut self, out: &mut [f32]) -> bool {
        let frames = out.len() / 2;
        let seconds = frames as f32 / RATE as f32;
        let follow = self.follow.get_or_insert_with(|| Follow::new(*self.shared.lock().unwrap()));
        follow.take(&self.shared, seconds);
        let (i, rpm, t) = (follow.input, follow.rpm, follow.throttle);
        let landed = i.landings != follow.landings;
        let wrecked = i.wrecks != follow.wrecks;
        follow.landings = i.landings;
        follow.wrecks = i.wrecks;
        self.since += seconds;

        if wrecked {
            self.shot(Pool::Wreck);
        }
        // Gas held with the rear wheel free (in the air, or the bike down)
        // runs to the limiter; otherwise idle, half throttle or a gear's
        // pool, with a little hysteresis between half and full.
        let gas = t > 0.05;
        let state = if gas && (i.crashed || i.airborne) && t > 0.5 {
            State::Limiter
        } else if !gas || i.crashed {
            State::Idle
        } else if t < 0.3 || (t < 0.4 && self.state == State::Half) {
            State::Half
        } else {
            State::Drive
        };
        let pool = Pool::for_gear(i.gear);
        let shifted = i.gear != self.gear;
        self.gear = i.gear;
        if state != self.state {
            match state {
                State::Idle => {
                    // Letting go from high up: the falling sweep, and the
                    // idle comes in under its tail.
                    if self.state == State::Drive && rpm > 6000.0 && self.since > 0.3 {
                        self.shot(if rpm > 8500.0 { Pool::DecelHigh } else { Pool::DecelLow });
                        self.switch(Pool::Idle, 0.06, 0.5, 0.4);
                    } else {
                        self.switch(Pool::Idle, 0.08, 0.0, 0.25);
                    }
                    if let Some(p) = self.pending.as_mut() {
                        p.3 = self.rng.unit() * 0.8;
                    }
                }
                State::Half => self.switch(Pool::HalfAccel, 0.05, 0.0, 0.01),
                State::Drive => self.switch(pool, 0.06, 0.0, 0.01),
                State::Limiter => {
                    let first = if i.crashed { Pool::PanicStart } else { Pool::EnterAir };
                    self.switch(first, 0.05, 0.0, 0.01);
                }
            }
            self.state = state;
        } else if state == State::Drive && landed && i.land_force > 0.1 {
            // Back on the ground on the gas: a landing take, then the gear's.
            self.switch(Pool::Land, 0.04, 0.0, 0.01);
        } else if state == State::Drive && shifted && self.since > 0.25 {
            // A gear change: this take stops, the next gear's starts from
            // its beginning.
            self.switch(pool, 0.05, SHIFT_GAP, 0.01);
        }

        self.mono.clear();
        self.mono.resize(frames, 0.0);
        let bank = self.bank.clone();
        // A pending start partway into this block.
        let mut split = 0;
        if let Some((p, delay, fade_in, at)) = self.pending {
            let wait = (delay * RATE as f32) as usize;
            if wait < frames {
                split = wait;
                self.pending = None;
                Self::play(&bank, &mut self.voices, &mut self.mono[..split]);
                self.start(p, fade_in, at);
            } else {
                self.pending = Some((p, delay - seconds, fade_in, at));
            }
        }
        let next = Self::play(&bank, &mut self.voices, &mut self.mono[split..]);
        if let Some(p) = next {
            // The land take hands over to the gear's pool.
            let p = if p == Pool::Land { pool } else { p };
            if self.pending.is_none() {
                self.start(p, CHAIN, 0.0);
            }
        }
        Self::play(&bank, &mut self.shots, &mut self.mono);

        for (f, m) in self.mono.iter().enumerate() {
            let v = (m * LEVEL + self.wind.sample(i.speed)) * 0.9;
            out[2 * f] += v;
            out[2 * f + 1] += v;
        }
        true
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
    /// The engine's rpm, smoothed (see `bike_engine`).
    rpm: f32,
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
        e.rpm = 0.0;
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
    // The synth's rpm. Pulling away on a slipping clutch the physics holds
    // the engine at idle while the bike moves; a real one is revved over the
    // clutch, so it is heard there. The physics rpm also jumps by thousands
    // in a tenth of a second while the rear tyre spins and grips; a
    // flywheel cannot, so the heard rpm moves at most 30,000 rpm/s and is
    // smoothed over about 0.1 s. (The Reflex model only uses it to pick the
    // decel take.)
    let mut target = m.rpm;
    if m.clutch_slipping && m.throttle > 0.05 && !m.crashed {
        target = target.max(m.min_rpm + 1200.0 + 5500.0 * m.throttle);
    }
    if e.rpm <= 0.0 {
        e.rpm = target;
    }
    let smoothed = e.rpm + (target - e.rpm) * (dt / 0.1).min(1.0);
    e.rpm += (smoothed - e.rpm).clamp(-30_000.0 * dt, 30_000.0 * dt);
    {
        let (rpm, landings, wrecks, air) = (e.rpm, e.landings, e.wrecks, e.air > 0.25);
        let mut s = e.shared.lock().unwrap();
        s.rpm = rpm;
        s.min_rpm = m.min_rpm;
        s.max_rpm = m.max_rpm;
        s.throttle = m.throttle;
        s.gear = m.gear;
        s.speed = m.speed;
        s.airborne = air;
        s.crashed = m.crashed;
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

    /// A ride for either model: idle, full gas up through the revs, two
    /// shifts, off the gas, the limiter in the air, a landing, half gas.
    fn ride(g: &mut dyn Generator, shared: &Mutex<Input>) -> Vec<f32> {
        let mut out = Vec::new();
        let block = 1024;
        let mut buf = vec![0.0; block * 2];
        let steps = (9.0 * RATE as f32 / block as f32) as usize;
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
                    t if t < 8.0 => (8000.0, 0.6),
                    _ => (5000.0, 0.2),
                };
                s.rpm = rpm;
                s.gear = usize::from(t >= 2.2) + usize::from(t >= 3.0);
                s.airborne = (5.5..7.0).contains(&t);
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
        for (pool, def) in POOLS {
            assert!(!bank.pool(pool).is_empty(), "{def}");
        }
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
