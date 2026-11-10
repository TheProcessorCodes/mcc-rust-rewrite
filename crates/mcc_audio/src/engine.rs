//! The dirt bike's engine (H, or the left stick click while riding, turns
//! it off and on): the Dakumoto 450F's own recordings, read from the user's
//! MX vs ATV Reflex install (`reflex_assets::engine_audio`), played the way
//! Reflex plays them. Matching a recording of the game against the takes
//! (2026-10-30) showed that Reflex never pitches them: on the gas it starts
//! a take from its beginning at each gear change and lets it run, at the
//! pitch and level it was recorded at, so the climb through a gear is the
//! take's own; when a take ends the next one in the pool follows. The pools
//! are the event file's sound definitions (four throttle pools, idle, half
//! throttle, the limiter, decel, landing, wreck); which pool plays in which
//! gear is ours to choose. Wind rises with speed.
//!
//! Without Reflex the bike is silent. (A synthesised engine was tried and
//! dropped on 2026-10-31: it did not sound good enough.)

use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

use bevy::input::gamepad::GamepadButton as B;
use bevy::prelude::*;

use mcc_core::bike::{BikeMotion, EngineVolume};

use crate::mixer::{Generator, Mixer, RATE, VoiceId};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Model {
    #[default]
    Reflex,
    Off,
}

impl Model {
    fn next(self) -> Model {
        match self {
            Model::Reflex => Model::Off,
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
    /// The rear wheel free: in the air for more than a moment, or the
    /// clutch held in.
    pub airborne: bool,
    pub crashed: bool,
    /// Counts up once per landing and per crash; `land_force` 0 to 1.
    pub landings: u32,
    pub land_force: f32,
    pub wrecks: u32,
    /// The pause menu's engine slider, 0 to 1.
    pub volume: f32,
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
    /// The engine track's gain: lower on light gas.
    level: f32,
    /// The slider's, as last heard.
    volume: f32,
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
            level: 1.0,
            volume: -1.0,
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
        // runs to the limiter; otherwise idle, or on the gas a gear's pool.
        // The half-throttle takes are low revs (3,400 to 4,300), so they
        // only play creeping on light gas with the engine low; riding along
        // on a little gas plays the gear's takes, quieter (`level`).
        let gas = t > 0.02;
        let low = rpm < 4800.0 || (rpm < 5400.0 && self.state == State::Half);
        let state = if gas && (i.crashed || i.airborne) && t > 0.5 {
            State::Limiter
        } else if !gas || i.crashed {
            State::Idle
        } else if t < 0.35 && low {
            State::Half
        } else {
            State::Drive
        };
        let level = if state == State::Drive { 0.5 + 0.5 * ((t - 0.02) / 0.5).clamp(0.0, 1.0).sqrt() } else { 1.0 };
        let level0 = self.level;
        self.level = level;
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
        for (f, m) in self.mono.iter_mut().enumerate() {
            *m *= level0 + (level - level0) * f as f32 / frames as f32;
        }
        Self::play(&bank, &mut self.shots, &mut self.mono);

        let (g0, g1) = (follow_volume(&mut self.volume, i.volume), self.volume);
        for (f, m) in self.mono.iter().enumerate() {
            let g = g0 + (g1 - g0) * f as f32 / frames as f32;
            let v = (m * LEVEL + self.wind.sample(i.speed)) * 0.9 * g;
            out[2 * f] += v;
            out[2 * f + 1] += v;
        }
        true
    }
}

/// Steps `held` to the slider's `wanted`; returns where it was (the first
/// block starts there).
fn follow_volume(held: &mut f32, wanted: f32) -> f32 {
    let was = if *held < 0.0 { wanted } else { *held };
    *held = wanted;
    was
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


/// The engine's loudness in the mix (the board's roll is 0.5).
const LEVEL: f32 = 0.55;

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
    volume: Option<Res<EngineVolume>>,
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
            Model::Reflex if e.bank.is_none() && e.tried && e.loading.is_none() => "on (no Reflex: silent)",
            Model::Reflex => "Reflex's 450F recordings",
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
            let bank = game_paths::reflex_ready().then(reflex_assets::imported::engine).and_then(|r| match r {
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
                    info!("bike: no Reflex engine sounds; the bike is silent");
                }
                e.bank = bank.map(Arc::new);
                e.loading = None;
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
    // The heard rpm. Pulling away on a slipping clutch the physics holds
    // the engine at idle while the bike moves; a real one is revved over the
    // clutch, so it is heard there. The physics rpm also jumps by thousands
    // in a tenth of a second while the rear tyre spins and grips; a
    // flywheel cannot, so the heard rpm moves at most 30,000 rpm/s and is
    // smoothed over about 0.1 s. (The recordings only use it to pick the
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
        // The clutch held in frees the engine as the air does: on the gas
        // it runs to the limiter instead of pulling through a gear.
        let (rpm, landings, wrecks, air) = (e.rpm, e.landings, e.wrecks, e.air > 0.25 || m.clutch_in);
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
        s.volume = volume.map_or(EngineVolume::default().0, |v| v.0);
    }

    // The recordings, once loaded; silence while they load, without Reflex
    // and when switched off.
    let wanted = (e.model == Model::Reflex && e.bank.is_some()).then_some(Model::Reflex);
    if e.playing.map(|(m, _)| m) != wanted {
        if let Some((_, id)) = e.playing.take() {
            mixer.stop(id);
        }
        if let (Some(model), Some(bank)) = (wanted, e.bank.clone()) {
            let g: Box<dyn Generator> = Box::new(Sampled::new(bank, e.shared.clone()));
            e.playing = Some((model, mixer.add_generator(g)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ride: idle, full gas up through the revs, two
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
                s.volume = 1.0;
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
        if let Some(path) = std::env::var_os("MCC_ENGINE_WAV") {
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
