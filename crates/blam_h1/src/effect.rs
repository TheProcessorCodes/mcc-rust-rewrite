//! Halo CE's effects as stored: `effe` (events with parts and particles),
//! `part` (a particle), `pctl` (a particle system: particles that pass
//! through states, fire turning to smoke), `ligh` (a light), `pphy` (point
//! physics) and the sprite sequences of their bitmaps.
//!
//! Written from `docs/format/halo1.md` 13, whose layouts were found on the
//! bytes of `d40` (The Maw) with `docs/format/scripts/probe_h1_effects.py`.
//! [`crate::effect_library`] turns what is read here into Halo 3's effect
//! types, so `halo3_effects` plays Halo CE's effects with its own renderer.
//!
//! Tags are named by their index (the datum's low 16 bits) everywhere here.

use crate::{Error, Map, Reader, Result};

/// The low 16 bits of a datum: the tag's index. `None` for a null
/// reference or one whose salt does not match.
fn index(map: &Map, datum: u32) -> Option<u32> {
    map.tag(datum).map(|_| datum & 0xFFFF)
}

/// A tag reference: its class (as stored, e.g. `snd!`) and index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagRef {
    pub class: String,
    pub tag: u32,
}

fn tag_ref(map: &Map, r: &Reader, off: usize) -> Result<Option<TagRef>> {
    let datum = r.tag_ref(off)?;
    Ok(map.tag(datum).map(|t| TagRef {
        class: t.class_str(),
        tag: datum & 0xFFFF,
    }))
}

fn bounds(r: &Reader, off: usize) -> Result<[f32; 2]> {
    Ok([r.f32(off)?, r.f32(off + 4)?])
}

/// An `argb` real colour (four f32, alpha first).
fn argb(r: &Reader, off: usize) -> Result<[f32; 4]> {
    Ok([r.f32(off)?, r.f32(off + 4)?, r.f32(off + 8)?, r.f32(off + 12)?])
}

fn data<'a>(map: &'a Map, tag: u32, class: &[u8; 4]) -> Result<Reader<'a>> {
    let t = map
        .tags
        .get(tag as usize)
        .ok_or_else(|| Error::Invalid(format!("no tag #{tag}")))?;
    if &t.class != class {
        return Err(Error::Invalid(format!(
            "{} is a {} tag, not {}",
            t.name,
            t.class_str(),
            String::from_utf8_lossy(class)
        )));
    }
    map.tag_data(t.id)
        .ok_or_else(|| Error::Invalid(format!("{} has no data", t.name)))
}

// ---- effe ----

/// An effect (`effe`, 0x40 bytes).
#[derive(Clone, Debug)]
pub struct Effect {
    pub tag: u32,
    pub name: String,
    /// `+0x00`.
    pub flags: u32,
    /// `+0x04`, `+0x06`: the events a looping effect loops between (-1:
    /// it does not loop).
    pub loop_start: i16,
    pub loop_stop: i16,
    /// `+0x28`, 0x20 each: marker names (`''` the effect's own origin,
    /// `normal`, `gravity`).
    pub locations: Vec<String>,
    /// `+0x34`, 0x44 each, played one after another.
    pub events: Vec<Event>,
}

/// An event (0x44 bytes).
#[derive(Clone, Debug)]
pub struct Event {
    /// `+0x04`: chance the event is skipped.
    pub skip_fraction: f32,
    /// `+0x08`: seconds before it starts.
    pub delay: [f32; 2],
    /// `+0x10`: seconds it lasts (0 for every explosion: one burst).
    pub duration: [f32; 2],
    /// `+0x2C`, 0x68 each.
    pub parts: Vec<Part>,
    /// `+0x38`, 0xE8 each.
    pub particles: Vec<EffectParticle>,
}

/// A part (0x68 bytes): a tag made when the event starts.
#[derive(Clone, Debug)]
pub struct Part {
    /// `+0x00`, `+0x02`: create in (environment, violence mode).
    pub create_in: [u16; 2],
    /// `+0x04`: index into the locations.
    pub location: i16,
    /// `+0x06`.
    pub flags: u16,
    /// `+0x18`: a sound (`snd!`), light (`ligh`), particle system
    /// (`pctl`), damage (`jpt!`), decal (`deca`), debris (`garb`), another
    /// effect, ...
    pub what: Option<TagRef>,
    /// `+0x40`: world units per second given to objects made.
    pub velocity: [f32; 2],
    /// `+0x48`: radians round the location's forward.
    pub velocity_cone: f32,
    /// `+0x4C`: radians per second.
    pub angular_velocity: [f32; 2],
    /// `+0x54`.
    pub radius_modifier: [f32; 2],
}

/// A particle of an event (0xE8 bytes): `count` `part` particles made at
/// the event's start.
#[derive(Clone, Debug)]
pub struct EffectParticle {
    /// `+0x00`, `+0x02`.
    pub create_in: [u16; 2],
    /// `+0x04`: camera mode (0 independent of it).
    pub mode: u16,
    /// `+0x08`.
    pub location: i16,
    /// `+0x0C`: yaw and pitch of the emission axis, radians.
    pub direction: [f32; 2],
    /// `+0x14`: offset from the location, world units.
    pub offset: [f32; 3],
    /// `+0x20`: the emission axis as a vector, worked out from `direction`
    /// (`(1, 0, 0)`, the location's forward, for every run effect).
    pub direction_vector: [f32; 3],
    /// `+0x54`: the `part` tag.
    pub particle: Option<u32>,
    /// `+0x64`.
    pub flags: u32,
    /// `+0x68`: when in the event's duration particles are made.
    pub distribution: u16,
    /// `+0x6C`, `+0x6E`.
    pub count: [i16; 2],
    /// `+0x70`: world units from the location.
    pub distribution_radius: [f32; 2],
    /// `+0x84`: world units per second.
    pub velocity: [f32; 2],
    /// `+0x8C`: half-angle, radians (pi: every way).
    pub velocity_cone: f32,
    /// `+0x90`: radians per second.
    pub angular_velocity: [f32; 2],
    /// `+0xA0`: world units.
    pub radius: [f32; 2],
    /// `+0xB0`, `+0xC0`: argb, a particle's tint picked between them.
    pub tint: [[f32; 4]; 2],
}

/// Reads an `effe` tag (index `tag`).
pub fn read_effect(map: &Map, tag: u32) -> Result<Effect> {
    let r = data(map, tag, b"effe")?;
    let mut locations = Vec::new();
    for l in r.block(0x28, 0x20)? {
        locations.push(l.str(0, 32));
    }
    let mut events = Vec::new();
    for e in r.block(0x34, 0x44)? {
        let mut parts = Vec::new();
        for p in e.block(0x2C, 0x68)? {
            parts.push(Part {
                create_in: [p.u16(0)?, p.u16(2)?],
                location: p.i16(4)?,
                flags: p.u16(6)?,
                what: tag_ref(map, &p, 0x18)?,
                velocity: bounds(&p, 0x40)?,
                velocity_cone: p.f32(0x48)?,
                angular_velocity: bounds(&p, 0x4C)?,
                radius_modifier: bounds(&p, 0x54)?,
            });
        }
        let mut particles = Vec::new();
        for q in e.block(0x38, 0xE8)? {
            particles.push(EffectParticle {
                create_in: [q.u16(0)?, q.u16(2)?],
                mode: q.u16(4)?,
                location: q.i16(8)?,
                direction: bounds(&q, 0xC)?,
                offset: q.vec3(0x14)?.to_array(),
                direction_vector: q.vec3(0x20)?.to_array(),
                particle: index(map, q.tag_ref(0x54)?),
                flags: q.u32(0x64)?,
                distribution: q.u16(0x68)?,
                count: [q.i16(0x6C)?, q.i16(0x6E)?],
                distribution_radius: bounds(&q, 0x70)?,
                velocity: bounds(&q, 0x84)?,
                velocity_cone: q.f32(0x8C)?,
                angular_velocity: bounds(&q, 0x90)?,
                radius: bounds(&q, 0xA0)?,
                tint: [argb(&q, 0xB0)?, argb(&q, 0xC0)?],
            });
        }
        events.push(Event {
            skip_fraction: e.f32(4)?,
            delay: bounds(&e, 8)?,
            duration: bounds(&e, 0x10)?,
            parts,
            particles,
        });
    }
    Ok(Effect {
        tag,
        name: map.tags[tag as usize].name.clone(),
        flags: r.u32(0)?,
        loop_start: r.i16(4)?,
        loop_stop: r.i16(6)?,
        locations,
        events,
    })
}

// ---- the particle shader both `part` and `pctl` carry ----

/// How a particle is drawn (the same 0x2C bytes in a `part` at `+0xD4`
/// and a `pctl` particle state at `+0xDC`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticleShader {
    /// `+0x04`: bit 0 sort bias, bit 1 nonlinear tint, bit 2 don't
    /// overdraw the first-person weapon.
    pub flags: u16,
    /// `+0x06`: framebuffer blend function: 0 alpha blend, 1 multiply, 2
    /// double multiply, 3 add, 4 subtract, 5 component min, 6 component
    /// max, 7 alpha-multiply add.
    pub blend: u16,
    /// `+0x08`: 0 none, 1 fade when perpendicular, 2 fade when parallel.
    pub fade: u16,
    /// `+0x0A`.
    pub map_flags: u16,
    /// `+0x28`: a secondary map (a `zmap` on steam); not drawn.
    pub secondary_map: Option<u32>,
}

fn shader(map: &Map, r: &Reader, off: usize) -> Result<ParticleShader> {
    Ok(ParticleShader {
        flags: r.u16(off + 4)?,
        blend: r.u16(off + 6)?,
        fade: r.u16(off + 8)?,
        map_flags: r.u16(off + 0xA)?,
        secondary_map: index(map, r.tag_ref(off + 0x28)?),
    })
}

// ---- part ----

/// How a particle's quad is turned (`part +0xAC`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    ScreenFacing,
    ParallelToDirection,
    PerpendicularToDirection,
}

/// A particle (`part`, 0x164 bytes).
#[derive(Clone, Debug)]
pub struct Particle {
    pub tag: u32,
    pub name: String,
    /// `+0x00`.
    pub flags: u32,
    /// `+0x04`.
    pub bitmap: Option<u32>,
    /// `+0x14`.
    pub physics: Option<u32>,
    /// `+0x38`: seconds.
    pub lifespan: [f32; 2],
    /// `+0x40`, `+0x44`: seconds it takes to fade in after it is made and
    /// out before it dies.
    pub fade_in: f32,
    pub fade_out: f32,
    /// `+0x48`, `+0x58`: effects played when it hits something and when it
    /// dies.
    pub collision_effect: Option<TagRef>,
    pub death_effect: Option<TagRef>,
    /// `+0x74`: its radius is multiplied from the first to the second over
    /// its life.
    pub radius_animation: [f32; 2],
    /// `+0x80`: sprite frames per second (0: one frame, picked at random).
    pub animation_rate: [f32; 2],
    /// `+0x98`: first sequence, then how many it starts in, loops through
    /// and ends with.
    pub first_sequence: i16,
    pub initial_sequences: i16,
    pub looping_sequences: i16,
    pub final_sequences: i16,
    /// `+0xAC`.
    pub orientation: Orientation,
    /// `+0xD4`.
    pub shader: ParticleShader,
}

/// Reads a `part` tag.
pub fn read_particle(map: &Map, tag: u32) -> Result<Particle> {
    let r = data(map, tag, b"part")?;
    Ok(Particle {
        tag,
        name: map.tags[tag as usize].name.clone(),
        flags: r.u32(0)?,
        bitmap: index(map, r.tag_ref(4)?),
        physics: index(map, r.tag_ref(0x14)?),
        lifespan: bounds(&r, 0x38)?,
        fade_in: r.f32(0x40)?,
        fade_out: r.f32(0x44)?,
        collision_effect: tag_ref(map, &r, 0x48)?,
        death_effect: tag_ref(map, &r, 0x58)?,
        radius_animation: bounds(&r, 0x74)?,
        animation_rate: bounds(&r, 0x80)?,
        first_sequence: r.i16(0x98)?,
        initial_sequences: r.i16(0x9A)?,
        looping_sequences: r.i16(0x9C)?,
        final_sequences: r.i16(0x9E)?,
        orientation: match r.u16(0xAC)? {
            1 => Orientation::ParallelToDirection,
            2 => Orientation::PerpendicularToDirection,
            _ => Orientation::ScreenFacing,
        },
        shader: shader(map, &r, 0xD4)?,
    })
}

// ---- pctl ----

/// A particle system (`pctl`, 0x68 bytes).
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    pub tag: u32,
    pub name: String,
    /// `+0x38`: the system's own point physics.
    pub physics: Option<u32>,
    /// `+0x50`, 4 bytes each.
    pub constants: Vec<f32>,
    /// `+0x5C`, 0x80 each.
    pub types: Vec<SystemType>,
}

/// One particle type of a system (0x80 bytes).
#[derive(Clone, Debug)]
pub struct SystemType {
    /// `+0x00`: `fire to smoke`, `space explosion`, ...
    pub name: String,
    /// `+0x20`.
    pub flags: u32,
    /// `+0x24`: particles made when the system starts.
    pub initial_count: i16,
    /// `+0x28`.
    pub render_mode: u16,
    /// `+0x2C`: world units round the origin they are made in.
    pub radius: f32,
    /// `+0x54`: how particles start moving (1 explosion: out from the
    /// origin, every way).
    pub creation_physics: u16,
    /// `+0x58`.
    pub physics_flags: u32,
    /// `+0x5C`, 4 bytes each. For explosion creation physics, the first
    /// two are the speed bounds in world units per tick (our reading).
    pub constants: Vec<f32>,
    /// `+0x68`, 0xC0 each: the system's own states (`birth`, `death`).
    pub states: Vec<SystemState>,
    /// `+0x74`, 0x178 each: the states each particle passes through, the
    /// last (`death`) the one it fades into.
    pub particle_states: Vec<ParticleState>,
}

/// A state of the system as a whole (0xC0 bytes).
#[derive(Clone, Debug)]
pub struct SystemState {
    pub name: String,
    /// `+0x20`: seconds.
    pub duration: [f32; 2],
    /// `+0x28`: seconds it takes to become the next state.
    pub transition: [f32; 2],
    /// `+0x34`: nine multipliers (scale, animation rate, rotation rate,
    /// colour argb, radius, and one more; our reading of their order).
    pub multipliers: [f32; 9],
}

/// A state of each particle (0x178 bytes).
#[derive(Clone, Debug)]
pub struct ParticleState {
    pub name: String,
    /// `+0x20`: seconds in this state.
    pub duration: [f32; 2],
    /// `+0x28`: seconds it takes to become the next state.
    pub transition: [f32; 2],
    /// `+0x30`.
    pub bitmap: Option<u32>,
    /// `+0x40`: the bitmap's sequence drawn.
    pub sequence: i16,
    /// `+0x48`: world units per pixel of the sprite.
    pub scale: [f32; 2],
    /// `+0x50`: sprite frames per second.
    pub animation_rate: [f32; 2],
    /// `+0x58`: radians per second.
    pub rotation_rate: [f32; 2],
    /// `+0x60`, `+0x70`: argb, picked between.
    pub colour: [[f32; 4]; 2],
    /// `+0x80`.
    pub radius_multiplier: f32,
    /// `+0x84`.
    pub physics: Option<u32>,
    /// `+0xDC`.
    pub shader: ParticleShader,
}

fn constants(r: &Reader, off: usize) -> Result<Vec<f32>> {
    r.block(off, 4)?.iter().map(|k| k.f32(0)).collect()
}

/// Reads a `pctl` tag.
pub fn read_system(map: &Map, tag: u32) -> Result<ParticleSystem> {
    let r = data(map, tag, b"pctl")?;
    let mut types = Vec::new();
    for t in r.block(0x5C, 0x80)? {
        let mut states = Vec::new();
        for s in t.block(0x68, 0xC0)? {
            let mut multipliers = [0.0; 9];
            for (k, m) in multipliers.iter_mut().enumerate() {
                *m = s.f32(0x34 + 4 * k)?;
            }
            states.push(SystemState {
                name: s.str(0, 32),
                duration: bounds(&s, 0x20)?,
                transition: bounds(&s, 0x28)?,
                multipliers,
            });
        }
        let mut particle_states = Vec::new();
        for s in t.block(0x74, 0x178)? {
            particle_states.push(ParticleState {
                name: s.str(0, 32),
                duration: bounds(&s, 0x20)?,
                transition: bounds(&s, 0x28)?,
                bitmap: index(map, s.tag_ref(0x30)?),
                sequence: s.i16(0x40)?,
                scale: bounds(&s, 0x48)?,
                animation_rate: bounds(&s, 0x50)?,
                rotation_rate: bounds(&s, 0x58)?,
                colour: [argb(&s, 0x60)?, argb(&s, 0x70)?],
                radius_multiplier: s.f32(0x80)?,
                physics: index(map, s.tag_ref(0x84)?),
                shader: shader(map, &s, 0xDC)?,
            });
        }
        types.push(SystemType {
            name: t.str(0, 32),
            flags: t.u32(0x20)?,
            initial_count: t.i16(0x24)?,
            render_mode: t.u16(0x28)?,
            radius: t.f32(0x2C)?,
            creation_physics: t.u16(0x54)?,
            physics_flags: t.u32(0x58)?,
            constants: constants(&t, 0x5C)?,
            states,
            particle_states,
        });
    }
    Ok(ParticleSystem {
        tag,
        name: map.tags[tag as usize].name.clone(),
        physics: index(map, r.tag_ref(0x38)?),
        constants: constants(&r, 0x50)?,
        types,
    })
}

// ---- ligh ----

/// A light (`ligh`), as far as an effect's flash needs it.
#[derive(Clone, Debug)]
pub struct Light {
    pub tag: u32,
    pub name: String,
    /// `+0x00`.
    pub flags: u32,
    /// `+0x04`: world units it reaches.
    pub radius: f32,
    /// `+0x38`, `+0x48`: argb. The light goes from the upper colour to the
    /// lower over its duration (`docs/format/halo1.md` 13.5).
    pub colour_lower: [f32; 4],
    pub colour_upper: [f32; 4],
    /// `+0xF4`: in ticks (30 a second): 30 on an explosion's light.
    pub duration_ticks: f32,
    /// `+0xFA`: 0 linear, 1 early, 2 very early, 3 late, 4 very late, 5
    /// cosine.
    pub falloff: u16,
}

impl Light {
    pub fn duration_seconds(&self) -> f32 {
        self.duration_ticks / 30.0
    }
}

/// Reads a `ligh` tag.
pub fn read_light(map: &Map, tag: u32) -> Result<Light> {
    let r = data(map, tag, b"ligh")?;
    Ok(Light {
        tag,
        name: map.tags[tag as usize].name.clone(),
        flags: r.u32(0)?,
        radius: r.f32(4)?,
        colour_lower: argb(&r, 0x38)?,
        colour_upper: argb(&r, 0x48)?,
        duration_ticks: r.f32(0xF4)?,
        falloff: r.u16(0xFA)?,
    })
}

// ---- pphy ----

/// Point physics (`pphy`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointPhysics {
    /// `+0x00`.
    pub flags: u32,
    /// `+0x20`: grams per millilitre (air is 0.0011).
    pub density: f32,
    /// `+0x24`, `+0x28`, `+0x2C`, `+0x30`.
    pub air_friction: f32,
    pub water_friction: f32,
    pub surface_friction: f32,
    pub elasticity: f32,
    /// `+0x04`: worked out by the tools: the density times 118,625 (our
    /// name: mass).
    pub mass: f32,
    /// `+0x0C`: worked out by the tools: `(air - density) / (air +
    /// density)`, the share of gravity that lifts it (smoke 0.16 to 0.47,
    /// stones -0.999).
    pub air_buoyancy: f32,
}

/// Reads a `pphy` tag.
pub fn read_physics(map: &Map, tag: u32) -> Result<PointPhysics> {
    let r = data(map, tag, b"pphy")?;
    Ok(PointPhysics {
        flags: r.u32(0)?,
        density: r.f32(0x20)?,
        air_friction: r.f32(0x24)?,
        water_friction: r.f32(0x28)?,
        surface_friction: r.f32(0x2C)?,
        elasticity: r.f32(0x30)?,
        mass: r.f32(4)?,
        air_buoyancy: r.f32(0xC)?,
    })
}

// ---- bitmap sequences ----

/// A sprite of a sequence (0x20 bytes).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sprite {
    /// `+0x00`: which image of the tag it is cut from.
    pub image: i16,
    /// `+0x08`: UV fractions of that image.
    pub left: f32,
    pub right: f32,
    pub top: f32,
    pub bottom: f32,
}

/// A sequence of a bitmap tag (`+0x54`, 0x40 bytes each).
#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub name: String,
    /// `+0x20`, `+0x22`: whole images, for tags that are not sprite sheets.
    pub first_image: i16,
    pub image_count: i16,
    /// `+0x34`, 0x20 each: sprites, for sprite sheets (bitmap type 3).
    pub sprites: Vec<Sprite>,
}

/// The sequences of a bitmap tag.
pub fn sequences(map: &Map, bitmap: u32) -> Result<Vec<Sequence>> {
    let r = data(map, bitmap, b"bitm")?;
    let mut out = Vec::new();
    for s in r.block(0x54, 0x40)? {
        let mut sprites = Vec::new();
        for p in s.block(0x34, 0x20)? {
            sprites.push(Sprite {
                image: p.i16(0)?,
                left: p.f32(8)?,
                right: p.f32(0xC)?,
                top: p.f32(0x10)?,
                bottom: p.f32(0x14)?,
            });
        }
        out.push(Sequence {
            name: s.str(0, 32),
            first_image: s.i16(0x20)?,
            image_count: s.i16(0x22)?,
            sprites,
        });
    }
    Ok(out)
}
