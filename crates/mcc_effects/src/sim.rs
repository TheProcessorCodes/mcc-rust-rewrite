//! Effects played on the CPU, in Halo world units: events one after
//! another, their parts (sounds, lights, child effects, contrails), and
//! their particle systems' emitters spawning, moving and ageing particles
//! by the tags' own functions (`docs/format/effects.md` 9 has the rules
//! and which of them are our reading). No bevy ECS here, so it runs in
//! tests; `draw.rs` turns the state into meshes.

use std::sync::Arc;

use bevy::math::{Mat3, Vec3};
use blam_effect::{Emitter, Library, Property, Ref};

/// Gravity, world units per second squared (`blam_geometry::movement`).
pub const GRAVITY: f32 = blam_geometry::movement::GRAVITY;

/// Most particles alive at once over every effect.
const MAX_PARTICLES: usize = 30_000;
/// Most effects playing at once.
const MAX_INSTANCES: usize = 600;
/// Shortest time between two starts of a looping effect's loop, seconds:
/// a loop of events that last no time would otherwise start every frame.
/// Our own number.
const MIN_LOOP: f32 = 0.5;

/// What a particle system's functions read (`effects.md` 4.2).
pub mod input {
    /// The particle's age over its lifespan.
    pub const PARTICLE_AGE: u8 = 0;
    /// The event's age over its duration.
    pub const SYSTEM_AGE: u8 = 1;
    /// The event's age when the particle was made.
    pub const EMISSION_TIME: u8 = 10;
    pub const LOD: u8 = 11;
    pub const GAME_TIME: u8 = 12;
    pub const EFFECT_A: u8 = 13;
    pub const EFFECT_B: u8 = 14;
    /// Random per emitter (system random and correlations).
    pub const SYSTEM_RANDOM: [u8; 3] = [3, 8, 9];
}

/// A deterministic random number in 0..1 for a seed and a channel.
pub fn random(seed: u32, channel: u32) -> f32 {
    let mut x = seed.wrapping_mul(0x9E37_79B9) ^ channel.wrapping_mul(0x85EB_CA6B).rotate_left(13);
    x ^= x >> 16;
    x = x.wrapping_mul(0x7FEB_352D);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846C_A68B);
    x ^= x >> 16;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// One live particle.
#[derive(Clone, Debug)]
pub struct Particle {
    pub id: u64,
    pub position: Vec3,
    pub velocity: Vec3,
    pub age: f32,
    pub life: f32,
    pub seed: u32,
    /// The event's age (0..1) when it was made.
    pub emitted_at: f32,
}

impl Particle {
    pub fn age01(&self) -> f32 {
        (self.age / self.life.max(1e-4)).clamp(0.0, 1.0)
    }
}

/// One emitter of one particle system of a playing effect.
#[derive(Clone, Debug)]
pub struct System {
    pub id: u64,
    pub instance: u64,
    pub effect: u32,
    pub event: usize,
    pub system: usize,
    pub emitter: usize,
    /// The `prt3` drawn.
    pub particle: u32,
    pub origin: Vec3,
    pub frame: Mat3,
    /// Seconds since the event started, and how long it emits.
    pub age: f32,
    pub duration: f32,
    pub seed: u32,
    pub emitted: u32,
    pub started: bool,
    accumulator: f32,
    pub particles: Vec<Particle>,
    pub game_time: f32,
}

impl System {
    pub fn age01(&self) -> f32 {
        if self.duration <= 1e-4 { 1.0 } else { (self.age / self.duration).clamp(0.0, 1.0) }
    }

    /// The value of input variable `v` for particle `p`.
    pub fn var(&self, v: u8, p: &Particle) -> f32 {
        match v {
            input::PARTICLE_AGE => p.age01(),
            input::SYSTEM_AGE => self.age01(),
            input::EMISSION_TIME => p.emitted_at,
            input::LOD | input::EFFECT_A | input::EFFECT_B => 1.0,
            input::GAME_TIME => self.game_time.fract(),
            v if input::SYSTEM_RANDOM.contains(&v) => random(self.seed, v as u32),
            v => random(p.seed, v as u32),
        }
    }

    /// A property's value for particle `p`.
    pub fn eval(&self, prop: &Property, p: &Particle) -> f32 {
        prop.function.scalar(self.var(prop.input, p), self.var(prop.range, p))
    }

    /// A colour property's value for particle `p`.
    pub fn colour(&self, prop: &Property, p: &Particle) -> [f32; 3] {
        prop.function.colour(self.var(prop.input, p), self.var(prop.range, p))
    }
}

/// A contrail left behind a playing effect.
#[derive(Clone, Debug)]
pub struct Contrail {
    pub id: u64,
    pub instance: u64,
    pub tag: u32,
    pub index: usize,
    pub seed: u32,
    /// Points oldest first.
    pub points: Vec<ContrailPoint>,
    pub emitting: bool,
    accumulator: f32,
}

#[derive(Clone, Debug)]
pub struct ContrailPoint {
    pub position: Vec3,
    pub age: f32,
    pub life: f32,
    pub seed: u32,
}

impl ContrailPoint {
    pub fn age01(&self) -> f32 {
        (self.age / self.life.max(1e-4)).clamp(0.0, 1.0)
    }
}

/// A light flash.
#[derive(Clone, Debug)]
pub struct Flash {
    pub id: u64,
    pub tag: u32,
    pub position: Vec3,
    pub age: f32,
}

/// One playing effect.
#[derive(Clone, Debug)]
pub struct Instance {
    pub id: u64,
    pub effect: u32,
    pub position: Vec3,
    pub frame: Mat3,
    pub source: Option<u32>,
    pub age: f32,
    next_event: usize,
    next_start: f32,
    current_end: f32,
    /// The particle it rides on: (system id, particle id).
    pub follows: Option<(u64, u64)>,
    seed: u32,
    /// Plays its events again from the effect's loop start (from the first
    /// when it names none) until stopped ([`Sim::stop`]): the map's
    /// placed effects and the weather (`ambient.rs`).
    looping: bool,
    /// When the loop last started over (its age).
    looped_at: f32,
}

impl Instance {
    /// Every event has started and the last has ended (never, looping).
    pub fn events_done(&self, lib: &Library) -> bool {
        let n = lib.effect(self.effect).map_or(0, |e| e.events.len());
        !self.looping && self.next_event >= n && self.age >= self.current_end
    }
}

/// Everything playing.
pub struct Sim {
    pub lib: Arc<Library>,
    pub instances: Vec<Instance>,
    pub systems: Vec<System>,
    pub contrails: Vec<Contrail>,
    pub flashes: Vec<Flash>,
    /// Sounds started since last taken: (`snd!` tag, where).
    pub sounds: Vec<(u32, Vec3)>,
    /// Damage effects (`jpt!`) started since last taken, for a camera
    /// shake: (tag, where).
    pub shakes: Vec<(u32, Vec3)>,
    /// Parts not played, by class, for the log.
    pub skipped: Vec<String>,
    next_id: u64,
    seed: u32,
    pub time: f32,
}

impl Sim {
    pub fn new(lib: Arc<Library>) -> Sim {
        Sim {
            lib,
            instances: Vec::new(),
            systems: Vec::new(),
            contrails: Vec::new(),
            flashes: Vec::new(),
            sounds: Vec::new(),
            shakes: Vec::new(),
            skipped: Vec::new(),
            next_id: 1,
            seed: 0x1234_5678,
            time: 0.0,
        }
    }

    fn id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn next_seed(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.seed ^ (self.seed >> 13)
    }

    /// Starts effect `tag` at `position` (Halo world units) in `frame`.
    /// Returns its id, or `None` when the library does not have it.
    pub fn spawn(&mut self, tag: u32, position: Vec3, frame: Mat3, source: Option<u32>) -> Option<u64> {
        let lib = self.lib.clone();
        let effect = lib.effect(tag)?;
        if self.instances.len() >= MAX_INSTANCES {
            return None;
        }
        let seed = self.next_seed();
        let first_delay = effect.events.first().map_or(0.0, |e| bounds(e.delay, random(seed, 1000)));
        let id = self.id();
        self.instances.push(Instance {
            id,
            effect: tag,
            position,
            frame,
            source,
            age: 0.0,
            next_event: 0,
            next_start: first_delay,
            current_end: 0.0,
            follows: None,
            seed,
            looping: false,
            looped_at: 0.0,
        });
        Some(id)
    }

    /// Starts effect `tag` looping (see [`Instance`]) until [`Sim::stop`].
    pub fn spawn_looping(&mut self, tag: u32, position: Vec3, frame: Mat3) -> Option<u64> {
        let id = self.spawn(tag, position, frame, None)?;
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == id) {
            i.looping = true;
        }
        Some(id)
    }

    /// Lets looping effect `id` end: it plays out what it has started.
    pub fn stop(&mut self, id: u64) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == id) {
            i.looping = false;
        }
    }

    /// Whether effect `id` is still playing.
    pub fn playing(&self, id: u64) -> bool {
        self.instances.iter().any(|i| i.id == id)
    }

    /// Moves effect `id` and the emitters it has running to `position`
    /// (Halo world units): what they emit from now on comes from there;
    /// what they have emitted stays where it is (the weather round the
    /// camera).
    pub fn move_to(&mut self, id: u64, position: Vec3) {
        if let Some(i) = self.instances.iter_mut().find(|i| i.id == id) {
            i.position = position;
        }
        for s in self.systems.iter_mut().filter(|s| s.instance == id) {
            s.origin = position;
        }
    }

    /// Advances everything by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        // Effects riding on particles move with them.
        for k in 0..self.instances.len() {
            if let Some((sys, pid)) = self.instances[k].follows {
                let p = self
                    .systems
                    .iter()
                    .find(|s| s.id == sys)
                    .and_then(|s| s.particles.iter().find(|p| p.id == pid))
                    .map(|p| p.position);
                match p {
                    Some(p) => self.instances[k].position = p,
                    None => self.instances[k].follows = Some((0, 0)),
                }
            }
        }
        // Events.
        let mut k = 0;
        while k < self.instances.len() {
            self.instances[k].age += dt;
            loop {
                let lib = self.lib.clone();
                let inst = &self.instances[k];
                let Some(effect) = lib.effect(inst.effect) else { break };
                if inst.looping && inst.next_event >= effect.events.len() && inst.age >= inst.next_start {
                    // Round again, no sooner than `MIN_LOOP` after the last time.
                    let first = effect.loop_start.filter(|&l| l < effect.events.len()).unwrap_or(0);
                    let inst = &mut self.instances[k];
                    let again = inst.next_start.max(inst.looped_at + MIN_LOOP);
                    inst.next_event = first;
                    inst.looped_at = again;
                    inst.next_start = again + effect.events.get(first).map_or(0.0, |n| bounds(n.delay, random(inst.seed, 1000 + first as u32)));
                    continue;
                }
                if inst.next_event >= effect.events.len() || inst.age < inst.next_start {
                    break;
                }
                let e = inst.next_event;
                let event = &effect.events[e];
                let seed = inst.seed;
                let start = inst.next_start;
                let duration = bounds(event.duration, random(seed, 2000 + e as u32));
                let skip = random(seed, 3000 + e as u32) < event.skip_fraction;
                {
                    let inst = &mut self.instances[k];
                    inst.current_end = start + duration;
                    inst.next_event += 1;
                    inst.next_start = inst.current_end
                        + effect.events.get(e + 1).map_or(0.0, |n| bounds(n.delay, random(seed, 1000 + e as u32 + 1)));
                }
                if !skip {
                    self.start_event(k, e, duration, inst_age_past(&self.instances[k], start));
                }
            }
            k += 1;
        }
        // Particles.
        let lib = self.lib.clone();
        let mut deaths = Vec::new();
        let mut births = Vec::new();
        let mut total = 0;
        for s in &mut self.systems {
            let Some(em) = emitter(&lib, s) else { continue };
            s.game_time = self.time;
            let was = s.age;
            let before = s.emitted;
            s.age += dt;
            if !s.started {
                s.started = true;
                let probe = probe(s.seed);
                let n = s.eval(&em.starting_count, &probe).round().max(0.0) as u32;
                for _ in 0..n.min(2000) {
                    emit(s, em, &lib);
                }
            }
            if was < s.duration {
                let probe = probe(s.seed ^ 0xABCD);
                let rate = s.eval(&em.rate, &probe).max(0.0);
                let max = s.eval(&em.max_count, &probe).round() as usize;
                s.accumulator += rate * dt.min(s.duration - was);
                while s.accumulator >= 1.0 {
                    s.accumulator -= 1.0;
                    if max == 0 || s.particles.len() < max {
                        emit(s, em, &lib);
                    }
                }
            }
            // Effects a particle carries start with it and ride on it.
            if s.emitted > before && lib.particles.get(&s.particle).is_some_and(|p| p.attachments.iter().any(|a| a.trigger == 0)) {
                for p in s.particles.iter().filter(|p| (p.id & 0xF_FFFF) as u32 > before) {
                    births.push((s.id, s.particle, p.id, p.position));
                }
            }
            let gravity = em.movement.physics(0);
            let drag = em.movement.physics(1);
            let accel_vec = em.self_acceleration.a;
            let frame = s.frame;
            let mut i = 0;
            while i < s.particles.len() {
                let p = &s.particles[i];
                let g = gravity.map_or(0.0, |g| s.eval(g, p));
                let d = drag.map_or(0.0, |d| s.eval(d, p)).max(0.0);
                let a = s.eval(&em.self_acceleration.property, p);
                let accel = Vec3::new(0.0, 0.0, -GRAVITY * g) + frame * (Vec3::from(accel_vec) * a);
                let p = &mut s.particles[i];
                p.velocity += accel * dt;
                if d > 0.0 {
                    p.velocity *= (-d * dt).exp();
                }
                p.position += p.velocity * dt;
                p.age += dt;
                if p.age >= p.life {
                    deaths.push((s.id, s.particle, p.position));
                    s.particles.swap_remove(i);
                } else {
                    i += 1;
                }
            }
            total += s.particles.len();
        }
        if total > MAX_PARTICLES {
            // Oldest systems give up their particles first.
            let mut over = total - MAX_PARTICLES;
            for s in &mut self.systems {
                let n = over.min(s.particles.len());
                s.particles.drain(..n);
                over -= n;
                if over == 0 {
                    break;
                }
            }
        }
        for (sys, particle, pid, at) in births {
            let Some(prt) = lib.particles.get(&particle) else { continue };
            for a in prt.attachments.iter().filter(|a| a.trigger == 0 && a.what.class.as_str() == "effe") {
                if let Some(id) = self.spawn(a.what.tag, at, Mat3::IDENTITY, None)
                    && let Some(child) = self.instances.iter_mut().find(|i| i.id == id)
                {
                    child.follows = Some((sys, pid));
                }
            }
        }
        // Attachments made when a particle dies.
        for (_, particle, at) in deaths {
            let Some(prt) = lib.particles.get(&particle) else { continue };
            for a in prt.attachments.iter().filter(|a| a.trigger == 2 && a.what.class.as_str() == "effe") {
                self.spawn(a.what.tag, at, Mat3::IDENTITY, None);
            }
        }
        // Contrails.
        for c in &mut self.contrails {
            let inst = self.instances.iter().find(|i| i.id == c.instance);
            let riding = inst.is_some_and(|i| !matches!(i.follows, Some((0, 0))));
            let alive = inst.is_some_and(|i| !i.events_done(&lib)) && riding;
            c.emitting = c.emitting && alive;
            for p in &mut c.points {
                p.age += dt;
            }
            c.points.retain(|p| p.age < p.life);
            let Some(def) = lib.contrails.get(&c.tag).and_then(|t| t.contrails.get(c.index)) else {
                c.emitting = false;
                continue;
            };
            if c.emitting && let Some(inst) = inst {
                let rate = def.rate.function.scalar(0.0, random(c.seed, 1)).max(0.0);
                c.accumulator += rate * dt;
                let mut made = false;
                while c.accumulator >= 1.0 {
                    c.accumulator -= 1.0;
                    made = true;
                }
                // The head always follows; a new point is kept at the rate.
                if made || c.points.is_empty() {
                    let seed = c.seed.wrapping_add(c.points.len() as u32 * 7919);
                    let life = def.lifespan.function.scalar(0.0, random(seed, 2)).max(0.05);
                    c.points.push(ContrailPoint { position: inst.position, age: 0.0, life, seed });
                } else if let Some(last) = c.points.last_mut() {
                    last.position = inst.position;
                }
            }
        }
        self.contrails.retain(|c| c.emitting || !c.points.is_empty());
        // Lights.
        self.flashes.retain_mut(|f| {
            f.age += dt;
            lib.lights.get(&f.tag).is_some_and(|l| f.age < l.duration.max(0.05))
        });
        // Systems end when their event is over and their particles are gone.
        self.systems.retain(|s| s.age < s.duration || !s.particles.is_empty() || !s.started);
        // Effects end when their events are done and nothing of theirs lives.
        let systems = &self.systems;
        let contrails = &self.contrails;
        self.instances.retain(|i| {
            !i.events_done(&lib)
                || systems.iter().any(|s| s.instance == i.id)
                || contrails.iter().any(|c| c.instance == i.id)
                || (i.follows.is_some() && !matches!(i.follows, Some((0, 0))))
        });
    }

    fn start_event(&mut self, k: usize, e: usize, duration: f32, _late: f32) {
        let lib = self.lib.clone();
        let inst = self.instances[k].clone();
        let Some(effect) = lib.effect(inst.effect) else { return };
        let event = &effect.events[e];
        for part in &event.parts {
            let Some(Ref { class, tag }) = part.what else { continue };
            let at = inst.position + inst.frame * Vec3::from(part.origin_offset);
            match class.as_str() {
                "snd!" => self.sounds.push((tag, at)),
                "ligh" => {
                    let id = self.id();
                    self.flashes.push(Flash { id, tag, position: at, age: 0.0 });
                }
                "effe" => {
                    if let Some(id) = self.spawn(tag, at, inst.frame, inst.source) {
                        let follows = inst.follows;
                        if let Some(child) = self.instances.iter_mut().find(|i| i.id == id) {
                            child.follows = follows;
                        }
                    }
                }
                "cntl" => {
                    let n = lib.contrails.get(&tag).map_or(0, |c| c.contrails.len());
                    for index in 0..n {
                        let id = self.id();
                        let seed = self.next_seed();
                        self.contrails.push(Contrail {
                            id,
                            instance: inst.id,
                            tag,
                            index,
                            seed,
                            points: Vec::new(),
                            emitting: true,
                            accumulator: 0.0,
                        });
                    }
                }
                "jpt!" => self.shakes.push((tag, at)),
                other => self.skipped.push(other.to_string()),
            }
        }
        for (si, system) in event.systems.iter().enumerate() {
            let Some(particle) = system.particle.filter(|p| lib.particles.contains_key(p)) else { continue };
            for ei in 0..system.emitters.len() {
                let id = self.id();
                let seed = self.next_seed();
                self.systems.push(System {
                    id,
                    instance: inst.id,
                    effect: inst.effect,
                    event: e,
                    system: si,
                    emitter: ei,
                    particle,
                    origin: inst.position,
                    frame: inst.frame,
                    age: 0.0,
                    duration,
                    seed,
                    emitted: 0,
                    started: false,
                    accumulator: 0.0,
                    particles: Vec::new(),
                    game_time: self.time,
                });
            }
        }
    }
}

fn inst_age_past(inst: &Instance, start: f32) -> f32 {
    inst.age - start
}

/// A particle that is not one yet, for the functions of the emitter as a
/// whole (starting count, rate).
fn probe(seed: u32) -> Particle {
    Particle {
        id: 0,
        position: Vec3::ZERO,
        velocity: Vec3::ZERO,
        age: 0.0,
        life: 1.0,
        seed,
        emitted_at: 0.0,
    }
}

fn bounds(b: [f32; 2], t: f32) -> f32 {
    (b[0] + (b[1] - b[0]) * t).max(0.0)
}

/// The emitter a system plays.
pub fn emitter<'l>(lib: &'l Library, s: &System) -> Option<&'l Emitter> {
    lib.effect(s.effect)?.events.get(s.event)?.systems.get(s.system)?.emitters.get(s.emitter)
}

/// A unit vector in the cone of half-angle `angle` (radians) round `axis`.
fn in_cone(axis: Vec3, angle: f32, u: f32, v: f32) -> Vec3 {
    let cos_a = 1.0 - (1.0 - angle.cos()) * u;
    let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt();
    let phi = std::f32::consts::TAU * v;
    let (b1, b2) = axis.any_orthonormal_pair();
    (axis * cos_a + (b1 * phi.cos() + b2 * phi.sin()) * sin_a).normalize_or(axis)
}

/// Makes one particle (`effects.md` 9.2).
fn emit(s: &mut System, em: &Emitter, _lib: &Library) {
    s.emitted += 1;
    let seed = s.seed.wrapping_mul(31).wrapping_add(s.emitted.wrapping_mul(2_654_435_761));
    let mut p = Particle {
        id: (s.id << 20) | s.emitted as u64,
        position: s.origin,
        velocity: Vec3::ZERO,
        age: 0.0,
        life: 1.0,
        seed,
        emitted_at: s.age01(),
    };
    p.life = s.eval(&em.lifespan, &p).max(0.02);
    // The emission axis: the location's z, or the emitter's direction.
    let dt = s.eval(&em.direction.property, &p);
    let d = Vec3::from(em.direction.a).lerp(Vec3::from(em.direction.b), dt);
    let axis = s.frame * if d.length_squared() > 1e-6 { d.normalize() } else { Vec3::Z };
    let axis = axis.normalize_or(Vec3::Z);
    let angle = s.eval(&em.angle, &p).to_radians();
    let dir = in_cone(axis, angle, random(seed, 101), random(seed, 102));
    let radius = s.eval(&em.radius, &p).max(0.0);
    let ot = s.eval(&em.offset.property, &p);
    let offset = s.frame * Vec3::from(em.offset.a).lerp(Vec3::from(em.offset.b), ot);
    let place = match em.shape {
        // Disc and ring: round the axis.
        1 | 5 => {
            let (b1, b2) = axis.any_orthonormal_pair();
            let phi = std::f32::consts::TAU * random(seed, 103);
            let r = if em.shape == 1 { radius * random(seed, 104).sqrt() } else { radius };
            (b1 * phi.cos() + b2 * phi.sin()) * r
        }
        // Everything else: out along the particle's own direction.
        _ => dir * radius,
    };
    p.position = s.origin + offset + place;
    p.velocity = dir * s.eval(&em.velocity, &p);
    s.particles.push(p);
}
