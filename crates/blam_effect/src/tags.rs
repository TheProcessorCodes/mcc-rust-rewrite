//! The effect tags as read: `effe`, `prt3`, `pmov`, `ligh`, `cntl`
//! (`docs/format/effects.md` 3 to 8). Offsets are MCC Halo 3's (U13).

use blam_cache::{CacheFile, FourCc, TagEntry};

use crate::function::Function;
use crate::{Error, Result};

/// Most entries any block here may have before it is treated as garbage.
const MAX_ENTRIES: u32 = 512;

// ---- properties ----

/// One property (32 bytes): what drives its function and the function.
///
/// | offset | field |
/// |---|---|
/// | 0x00 | u8 input variable |
/// | 0x01 | u8 range variable (picks between a ranged function's two outputs) |
/// | 0x02 | u8 output modifier (0: none) |
/// | 0x03 | u8 output modifier input |
/// | 0x04 | data reference to the function blob (`function.rs`) |
/// | 0x18 | f32 the constant value the engine precomputed |
/// | 0x1C | u32 runtime flags |
#[derive(Clone, Debug, PartialEq)]
pub struct Property {
    pub input: u8,
    pub range: u8,
    pub output_modifier: u8,
    pub output_modifier_input: u8,
    pub function: Function,
}

impl Property {
    pub fn constant(v: f32) -> Property {
        Property {
            input: 0,
            range: 0,
            output_modifier: 0,
            output_modifier_input: 0,
            function: Function::constant(v),
        }
    }
}

/// A property with two vectors (56 bytes): the property scales between
/// them (`a` at 0, `b` at 1). Used for offsets, directions, accelerations.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorProperty {
    pub property: Property,
    pub a: [f32; 3],
    pub b: [f32; 3],
}

fn property(cache: &CacheFile, at: usize) -> Result<Property> {
    let mut r = cache.reader_at(at);
    let (input, range, output_modifier, output_modifier_input) = (r.u8()?, r.u8()?, r.u8()?, r.u8()?);
    let data = r.data_ref()?;
    let function = match data.offset {
        Some(o) if data.size >= 0x20 && data.size <= 4096 => Function::parse(cache.reader_at(o).take(data.size as usize)?)?,
        // An empty blob: Halo treats it as zero.
        _ => Function::constant(r.at(at + 0x18).f32()?),
    };
    Ok(Property {
        input,
        range,
        output_modifier,
        output_modifier_input,
        function,
    })
}

fn vector_property(cache: &CacheFile, at: usize) -> Result<VectorProperty> {
    Ok(VectorProperty {
        property: property(cache, at)?,
        a: cache.reader_at(at + 0x20).f32x3()?,
        b: cache.reader_at(at + 0x2C).f32x3()?,
    })
}

fn bounded(cache: &CacheFile, at: usize, what: &str) -> Result<blam_cache::Block> {
    let b = cache.reader_at(at).block()?;
    if b.count > MAX_ENTRIES {
        return Err(Error::Invalid(format!("{what}: {} entries", b.count)));
    }
    Ok(b)
}

fn sid(cache: &CacheFile, at: usize) -> Result<String> {
    Ok(cache.string(cache.reader_at(at).string_id()?).unwrap_or("").to_string())
}

/// A tag reference: its class and tag index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    pub class: FourCc,
    pub tag: u32,
}

fn tag_ref(cache: &CacheFile, at: usize) -> Result<Option<Ref>> {
    let r = cache.reader_at(at).tag_ref()?;
    Ok(r.index.filter(|i| cache.tags.get(*i).is_some()).map(|tag| Ref { class: r.class, tag }))
}

fn tag_of<'c>(cache: &'c CacheFile, tag: u32, want: &str) -> Result<(&'c TagEntry, usize)> {
    let t = cache.tags.get(tag).ok_or_else(|| Error::Invalid(format!("no tag #{tag}")))?;
    let class = cache.tags.class_of(t).code.as_str();
    if class != want {
        return Err(Error::Invalid(format!("{} is a {class} tag, not {want}", t.name())));
    }
    Ok((t, cache.tag_offset(t)?))
}

// ---- effe ----

/// An effect (`effe`, 0x68 bytes).
#[derive(Clone, Debug)]
pub struct Effect {
    pub tag: u32,
    pub name: String,
    pub flags: u32,
    /// The marker names its parts and particle systems are placed at
    /// (`+0x20`, 12 bytes each).
    pub locations: Vec<String>,
    /// Played one after another (`+0x2C`, 0x44 bytes each).
    pub events: Vec<Event>,
    /// Event the effect loops back to, if it loops (`+0x18`).
    pub loop_start: Option<usize>,
    /// `+0x38`.
    pub looping_sound: Option<Ref>,
}

impl Effect {
    /// The longest the effect runs: every event's longest delay and
    /// duration, one after another.
    pub fn max_length(&self) -> f32 {
        self.events.iter().map(|e| e.delay[0].max(e.delay[1]) + e.duration[0].max(e.duration[1])).sum()
    }
}

/// One event (0x44 bytes).
#[derive(Clone, Debug)]
pub struct Event {
    pub name: String,
    pub flags: u32,
    /// Chance the event is skipped (`+0x0C`).
    pub skip_fraction: f32,
    /// Seconds before it starts, after the previous event ends (`+0x10`).
    pub delay: [f32; 2],
    /// Seconds it lasts: how long its emitters emit (`+0x18`).
    pub duration: [f32; 2],
    /// `+0x20`, 0x60 bytes each.
    pub parts: Vec<Part>,
    /// `+0x38`, 0x5C bytes each.
    pub systems: Vec<ParticleSystem>,
}

/// One part (0x60 bytes): a tag created when the event starts.
#[derive(Clone, Debug)]
pub struct Part {
    /// `+0x10`: a sound, light, effect, object (debris), damage effect,
    /// screen effect, decal, ...
    pub what: Option<Ref>,
    /// `+0x04`: index into the effect's locations.
    pub location: i16,
    /// `+0x20`: speed given to objects created, world units per second.
    pub velocity: [f32; 2],
    /// `+0x28`: yaw and pitch of the velocity's axis, radians, in the
    /// location's frame (pitch pi/2 is straight up).
    pub velocity_orientation: [f32; 2],
    /// `+0x30`: half-angle of the cone round that axis, radians.
    pub velocity_cone: f32,
    /// `+0x34`: radians per second.
    pub angular_velocity: [f32; 2],
    /// `+0x3C`.
    pub radius_modifier: [f32; 2],
    /// `+0x44`.
    pub origin_offset: [f32; 3],
}

/// A particle system of an event (0x5C bytes).
#[derive(Clone, Debug)]
pub struct ParticleSystem {
    /// `+0x04`: the `prt3` drawn.
    pub particle: Option<u32>,
    /// `+0x14`: index into the effect's locations.
    pub location: i32,
    /// `+0x18`: 1 when the particles move with what made them.
    pub local: bool,
    /// `+0x20`.
    pub sort_bias: i16,
    /// `+0x22`.
    pub flags: u16,
    /// `+0x40`: beyond this distance (world units) it is not drawn.
    pub lod_distance: f32,
    /// `+0x4C`, 0x2F0 bytes each.
    pub emitters: Vec<Emitter>,
}

/// An emitter (0x2F0 bytes). Inputs and ranges are particle property
/// variables (`docs/format/effects.md` 4.2).
#[derive(Clone, Debug)]
pub struct Emitter {
    pub name: String,
    /// `+0x04`: the emission shape.
    pub shape: u8,
    /// `+0x08`: the bounds the engine worked out (world units).
    pub bounding_radius: f32,
    /// `+0x18`: where particles start, relative to the location.
    pub offset: VectorProperty,
    /// `+0x50`: yaw and pitch (radians) turning the emission axis.
    pub direction: VectorProperty,
    /// `+0x88`: world units.
    pub radius: Property,
    /// `+0xA8`: degrees off the emission axis.
    pub angle: Property,
    /// `+0xC8`: degrees (not used here).
    pub axis_angle: Property,
    /// `+0xE8`: particles made when the event starts.
    pub starting_count: Property,
    /// `+0x108`: most alive at once (0: no limit).
    pub max_count: Property,
    /// `+0x128`: particles a second while the event lasts.
    pub rate: Property,
    /// `+0x148`: seconds.
    pub lifespan: Property,
    /// `+0x168` and `+0x17C`: the physics.
    pub movement: Movement,
    /// `+0x188`: world units per second squared.
    pub self_acceleration: VectorProperty,
    /// `+0x1C0`: world units per second along the emission direction.
    pub velocity: Property,
    /// `+0x1E0`: turns.
    pub rotation: Property,
    /// `+0x200`: turns per second.
    pub rotation_rate: Property,
    /// `+0x220`: world units across.
    pub size: Property,
    /// `+0x240`: multiplies the size.
    pub scale: Property,
    /// `+0x260`: colour.
    pub tint: Property,
    /// `+0x280`.
    pub alpha: Property,
    /// `+0x2A0`: alpha under this goes to 0 (`black_point` shaders).
    pub black_point: Property,
}

/// Particle physics: the template's (or the emitter's own) movements.
#[derive(Clone, Debug, Default)]
pub struct Movement {
    pub template: Option<u32>,
    pub flags: u32,
    pub movements: Vec<MovementKind>,
}

/// One movement (24 bytes: u16 kind, a block of 36-byte parameters at
/// `+0x04`, a u32 mask of the parameter ids present at `+0x10`).
#[derive(Clone, Debug)]
pub struct MovementKind {
    pub kind: u16,
    /// `(id, value)`: for kind 0 (physics), 0 gravity scale, 1 drag, 2
    /// turbulence (not used here).
    pub parameters: Vec<(u32, Property)>,
}

impl Movement {
    /// The physics movement's parameter `id`, if any.
    pub fn physics(&self, id: u32) -> Option<&Property> {
        self.movements
            .iter()
            .filter(|m| m.kind == 0)
            .flat_map(|m| m.parameters.iter())
            .find(|(i, _)| *i == id)
            .map(|(_, p)| p)
    }
}

fn movements(cache: &CacheFile, at: usize) -> Result<Vec<MovementKind>> {
    let mut out = Vec::new();
    for m in bounded(cache, at, "movements")?.elements(0x18) {
        let kind = cache.reader_at(m).u16()?;
        let mut parameters = Vec::new();
        for p in bounded(cache, m + 4, "movement parameters")?.elements(0x24) {
            parameters.push((cache.reader_at(p).u32()?, property(cache, p + 4)?));
        }
        out.push(MovementKind { kind, parameters });
    }
    Ok(out)
}

/// Reads a `pmov` tag: a template (`+0x00`), flags (`+0x10`) and its
/// movements (`+0x14`); a template with no movements of its own uses its
/// template's.
pub fn read_movement(cache: &CacheFile, tag: u32) -> Result<Movement> {
    read_movement_depth(cache, tag, 0)
}

fn read_movement_depth(cache: &CacheFile, tag: u32, depth: u32) -> Result<Movement> {
    let (_, o) = tag_of(cache, tag, "pmov")?;
    let template = tag_ref(cache, o)?.map(|r| r.tag);
    let flags = cache.reader_at(o + 0x10).u32()?;
    let mut list = movements(cache, o + 0x14)?;
    if list.is_empty() && depth < 4 && let Some(t) = template.filter(|t| *t != tag) {
        list = read_movement_depth(cache, t, depth + 1)?.movements;
    }
    Ok(Movement { template, flags, movements: list })
}

fn emitter(cache: &CacheFile, e: usize) -> Result<Emitter> {
    let p = |o: usize| property(cache, e + o);
    let mut movement = Movement {
        template: tag_ref(cache, e + 0x168)?.map(|r| r.tag),
        flags: cache.reader_at(e + 0x178).u32()?,
        movements: movements(cache, e + 0x17C)?,
    };
    if movement.movements.is_empty()
        && let Some(t) = movement.template
        && let Ok(m) = read_movement(cache, t)
    {
        movement.movements = m.movements;
    }
    Ok(Emitter {
        name: sid(cache, e)?,
        shape: cache.reader_at(e + 4).u8()?,
        bounding_radius: cache.reader_at(e + 8).f32()?,
        offset: vector_property(cache, e + 0x18)?,
        direction: vector_property(cache, e + 0x50)?,
        radius: p(0x88)?,
        angle: p(0xA8)?,
        axis_angle: p(0xC8)?,
        starting_count: p(0xE8)?,
        max_count: p(0x108)?,
        rate: p(0x128)?,
        lifespan: p(0x148)?,
        movement,
        self_acceleration: vector_property(cache, e + 0x188)?,
        velocity: p(0x1C0)?,
        rotation: p(0x1E0)?,
        rotation_rate: p(0x200)?,
        size: p(0x220)?,
        scale: p(0x240)?,
        tint: p(0x260)?,
        alpha: p(0x280)?,
        black_point: p(0x2A0)?,
    })
}

/// Reads an `effe` tag.
pub fn read_effect(cache: &CacheFile, tag: u32) -> Result<Effect> {
    let (t, o) = tag_of(cache, tag, "effe")?;
    let mut locations = Vec::new();
    for l in bounded(cache, o + 0x20, "locations")?.elements(0xC) {
        locations.push(sid(cache, l)?);
    }
    let mut events = Vec::new();
    for e in bounded(cache, o + 0x2C, "events")?.elements(0x44) {
        let mut r = cache.reader_at(e + 0x0C);
        let skip_fraction = r.f32()?;
        let delay = r.f32x2()?;
        let duration = r.f32x2()?;
        let mut parts = Vec::new();
        for p in bounded(cache, e + 0x20, "parts")?.elements(0x60) {
            let mut r = cache.reader_at(p + 0x20);
            parts.push(Part {
                what: tag_ref(cache, p + 0x10)?,
                location: cache.reader_at(p + 4).i16()?,
                velocity: r.f32x2()?,
                velocity_orientation: r.f32x2()?,
                velocity_cone: r.f32()?,
                angular_velocity: r.f32x2()?,
                radius_modifier: r.f32x2()?,
                origin_offset: r.f32x3()?,
            });
        }
        let mut systems = Vec::new();
        for s in bounded(cache, e + 0x38, "particle systems")?.elements(0x5C) {
            let mut emitters = Vec::new();
            for em in bounded(cache, s + 0x4C, "emitters")?.elements(0x2F0) {
                emitters.push(emitter(cache, em)?);
            }
            let mut r = cache.reader_at(s + 0x20);
            systems.push(ParticleSystem {
                particle: tag_ref(cache, s + 4)?.filter(|r| r.class.as_str() == "prt3").map(|r| r.tag),
                location: cache.reader_at(s + 0x14).i32()?,
                local: cache.reader_at(s + 0x18).u32()? == 1,
                sort_bias: r.i16()?,
                flags: r.u16()?,
                lod_distance: cache.reader_at(s + 0x40).f32()?,
                emitters,
            });
        }
        events.push(Event {
            name: sid(cache, e)?,
            flags: cache.reader_at(e + 4).u32()?,
            skip_fraction,
            delay,
            duration,
            parts,
            systems,
        });
    }
    let loop_start = cache.reader_at(o + 0x18).i16()?;
    Ok(Effect {
        tag,
        name: t.name().to_string(),
        flags: cache.reader_at(o).u32()?,
        locations,
        events,
        loop_start: usize::try_from(loop_start).ok(),
        looping_sound: tag_ref(cache, o + 0x38)?,
    })
}

// ---- prt3 ----

/// How a particle's quad is turned (`prt3 +0x14`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Billboard {
    /// Faces the camera (0 screen facing, 1 camera facing).
    Camera,
    /// Stretched along its velocity, turned to the camera round it (2).
    Velocity,
    /// Faces along its velocity (3).
    Perpendicular,
    /// Upright, turned to the camera round the vertical (4).
    Vertical,
    /// Flat on the ground (5).
    Horizontal,
}

/// An effect a particle carries (`prt3 +0x04`, 20 bytes each).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attachment {
    pub what: Ref,
    /// `+0x10`: 0 while the particle lives, 1 when it hits something, 2
    /// when it dies (our reading of the values seen).
    pub trigger: u8,
}

/// A particle (`prt3`).
#[derive(Clone, Debug)]
pub struct Particle {
    pub tag: u32,
    pub name: String,
    pub flags: u32,
    pub attachments: Vec<Attachment>,
    /// `+0x10`.
    pub appearance_flags: u32,
    pub billboard: Billboard,
    /// `+0x18`, `+0x1A`: which sequences of the base map's sprite sheet.
    pub first_sequence: i16,
    pub sequence_count: i16,
    /// `+0x20`: where along a stretched quad the particle sits (-0.5:
    /// the head leads).
    pub center_offset: f32,
    /// The render method at `+0x3C` (`rmdf shaders\particle`).
    pub shader: blam_texture::Shader,
    /// `+0x7C`: height over width.
    pub aspect: Property,
    /// `+0x9C`.
    pub colour: Property,
    /// `+0xBC`.
    pub intensity: Property,
    /// `+0xDC`.
    pub alpha: Property,
    /// `+0xFC`.
    pub animation_flags: u32,
    /// `+0x100`: 0..1 across the sprite frames.
    pub frame: Property,
    /// `+0x120`: frames a lifetime, as a fraction of the sequence.
    pub animation_rate: Property,
    /// `+0x140`: the row of the palette (`palettized` shaders).
    pub palette: Property,
}

/// Reads a `prt3` tag.
pub fn read_particle(cache: &CacheFile, tag: u32) -> Result<Particle> {
    let (t, o) = tag_of(cache, tag, "prt3")?;
    let mut attachments = Vec::new();
    for a in bounded(cache, o + 4, "attachments")?.elements(0x14) {
        if let Some(what) = tag_ref(cache, a)? {
            attachments.push(Attachment {
                what,
                trigger: cache.reader_at(a + 0x10).u8()?,
            });
        }
    }
    let mut r = cache.reader_at(o + 0x10);
    let appearance_flags = r.u32()?;
    let billboard = match r.u16()? {
        2 => Billboard::Velocity,
        3 => Billboard::Perpendicular,
        4 => Billboard::Vertical,
        5 => Billboard::Horizontal,
        _ => Billboard::Camera,
    };
    let mut r = cache.reader_at(o + 0x18);
    let (first_sequence, sequence_count) = (r.i16()?, r.i16()?);
    let p = |off: usize| property(cache, o + off);
    Ok(Particle {
        tag,
        name: t.name().to_string(),
        flags: cache.reader_at(o).u32()?,
        attachments,
        appearance_flags,
        billboard,
        first_sequence,
        sequence_count,
        center_offset: cache.reader_at(o + 0x20).f32()?,
        shader: blam_texture::shader::resolve_render_method(cache, tag, o + 0x3C)?,
        aspect: p(0x7C)?,
        colour: p(0x9C)?,
        intensity: p(0xBC)?,
        alpha: p(0xDC)?,
        animation_flags: cache.reader_at(o + 0xFC).u32()?,
        frame: p(0x100)?,
        animation_rate: p(0x120)?,
        palette: p(0x140)?,
    })
}

// ---- ligh ----

/// A light (`ligh`), as far as a flash needs it.
#[derive(Clone, Debug)]
pub struct Light {
    pub tag: u32,
    pub name: String,
    pub flags: u32,
    /// `+0x04`: 0 a sphere, 1 a spot.
    pub kind: u16,
    /// `+0x08`: world units.
    pub size: f32,
    /// `+0x24`: colour over the light's life.
    pub colour: Property,
    /// `+0x48`: intensity over the light's life.
    pub intensity: Property,
    /// `+0x70`: seconds.
    pub duration: f32,
}

/// Reads a `ligh` tag.
pub fn read_light(cache: &CacheFile, tag: u32) -> Result<Light> {
    let (t, o) = tag_of(cache, tag, "ligh")?;
    Ok(Light {
        tag,
        name: t.name().to_string(),
        flags: cache.reader_at(o).u32()?,
        kind: cache.reader_at(o + 4).u16()?,
        size: cache.reader_at(o + 8).f32()?,
        colour: property(cache, o + 0x24)?,
        intensity: property(cache, o + 0x48)?,
        duration: cache.reader_at(o + 0x70).f32()?,
    })
}

// ---- cntl ----

/// One contrail of a `cntl` tag (0x26C bytes): a ribbon of points left
/// behind whatever carries it.
#[derive(Clone, Debug)]
pub struct Contrail {
    pub name: String,
    /// `+0x24`: points a second.
    pub rate: Property,
    /// `+0x44`: seconds each point lives.
    pub lifespan: Property,
    /// `+0x9C`: width (world units) over a point's life.
    pub size: Property,
    /// `+0x130`: the render method (`rmdf shaders\contrail`).
    pub shader: blam_texture::Shader,
    /// `+0x170`, `+0x174`: texture repeats along and across.
    pub repeat: [f32; 2],
    /// `+0x180`.
    pub colour: Property,
    /// `+0x1A0`: over a point's life.
    pub alpha: Property,
    /// `+0x1C0`.
    pub intensity: Property,
}

/// A contrail system (`cntl`): its contrails (`+0x00`).
#[derive(Clone, Debug)]
pub struct ContrailSystem {
    pub tag: u32,
    pub name: String,
    pub contrails: Vec<Contrail>,
}

/// Reads a `cntl` tag.
pub fn read_contrail(cache: &CacheFile, tag: u32) -> Result<ContrailSystem> {
    let (t, o) = tag_of(cache, tag, "cntl")?;
    let mut contrails = Vec::new();
    for c in bounded(cache, o, "contrails")?.elements(0x26C) {
        let p = |off: usize| property(cache, c + off);
        let shader = blam_texture::shader::resolve_render_method(cache, tag, c + 0x130)?;
        contrails.push(Contrail {
            name: sid(cache, c)?,
            rate: p(0x24)?,
            lifespan: p(0x44)?,
            size: p(0x9C)?,
            shader,
            repeat: cache.reader_at(c + 0x170).f32x2()?,
            colour: p(0x180)?,
            alpha: p(0x1A0)?,
            intensity: p(0x1C0)?,
        });
    }
    Ok(ContrailSystem {
        tag,
        name: t.name().to_string(),
        contrails,
    })
}
