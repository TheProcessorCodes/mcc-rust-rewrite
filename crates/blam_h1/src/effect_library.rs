//! Halo CE's effects turned into Halo 3's effect types
//! ([`blam_effect::Library`]), so `mcc_effects` plays them with the
//! renderer it already has: an effect's events, its `part` particles as
//! emitters, its `pctl` particle systems as one emitter per particle state
//! (fire, then smoke), its lights as flashes, its sounds handed on by tag.
//!
//! What maps to what, and which numbers are our reading, is in
//! `docs/format/halo1.md` 13.6. Halo CE's bitmaps become one atlas per tag
//! ([`AtlasLayout`]): every image stacked, every sequence a run of sprites
//! on it, so the renderer's one-texture-per-bitmap sprite frames work for
//! sprite sheets and for image sequences (lens flares) alike.

use std::collections::{BTreeMap, HashMap};
use std::f32::consts::{PI, TAU};

use blam_cache::FourCc;
use blam_effect::function::{Segment, Shape, srgb_to_linear};
use blam_effect::{
    Attachment, Billboard, Effect as H3Effect, Emitter, Event as H3Event, Function, Library, Light as H3Light,
    Movement, MovementKind, Part as H3Part, Particle as H3Particle, ParticleSystem as H3System, Property, Ref,
    VectorProperty,
};
use blam_texture::bitmap::{Sequence as H3Sequence, Sprite as H3Sprite};
use blam_texture::shader::{Blend, Shader, ShaderMap, Xform};

use crate::effect::{self, Orientation, ParticleShader, PointPhysics};
use crate::{Map, Result};

/// A light's intensity as `mcc_effects` reads it (its `LIGHT_SCALE`
/// makes 20 about one unit of irradiance): our choice, by eye, for every
/// Halo CE effect light at the start of its life (20 turned the whole
/// Warthog bay flat orange).
pub const LIGHT_INTENSITY: f32 = 12.0;
/// What particle colours are multiplied by: `mcc_effects` multiplies
/// every particle by its exposure (3), chosen for Halo 3's run; this
/// undoes it, so a Halo CE colour reaches the screen as the tag has it
/// (at x3 the grey smoke plume came out white).
pub const COLOUR_SCALE: f32 = 1.0 / 3.0;
/// `mcc_effects` lights reach this many times a light's size.
const LIGHT_RANGE_PER_SIZE: f32 = 4.0;
/// Halo CE runs at 30 ticks a second.
const TICKS: f32 = 30.0;
/// Most nested effects followed.
const MAX_DEPTH: u32 = 6;

/// Keys of the particles made from `pctl` particle states: past every tag
/// index (`0x8000_0000 | pctl << 12 | type << 6 | state`).
fn state_key(pctl: u32, ty: usize, state: usize) -> u32 {
    0x8000_0000 | (pctl & 0xFFFF) << 12 | ((ty as u32) & 0x3F) << 6 | (state as u32 & 0x3F)
}

// Particle input variables (`docs/format/effects.md` 3.1): random numbers
// per particle, one per property so they do not move together.
const AGE: u8 = 0;
const SYSTEM_AGE: u8 = 1;
const PER_EMITTER: u8 = 3;
const R_LIFE: u8 = 4;
const R_SPEED: u8 = 5;
const R_PLACE: u8 = 6;
const R_ROTATION: u8 = 7;
const R_SIZE: u8 = 15;
const R_SPIN: u8 = 18;
const R_TINT: u8 = 19;
const R_FRAME: u8 = 20;

// ---- functions ----

fn function(kind: u8, ranged: bool, shape: Shape, min: f32, max: f32) -> Function {
    Function {
        kind,
        ranged,
        shape,
        min,
        max,
        colour_stops: 0,
        colours: [[1.0; 3]; 4],
    }
}

fn property(input: u8, range: u8, function: Function) -> Property {
    Property {
        input,
        range,
        output_modifier: 0,
        output_modifier_input: 0,
        function,
    }
}

fn constant(v: f32) -> Property {
    Property::constant(v)
}

/// A value picked between `b[0]` and `b[1]` by random variable `var`.
fn ranged(var: u8, b: [f32; 2]) -> Property {
    if b[0] == b[1] {
        return constant(b[0]);
    }
    property(SYSTEM_AGE, var, function(1, true, Shape::Constant, b[0], b[1]))
}

/// `b[0]` at the particle's birth to `b[1]` at its death.
fn over_life(b: [f32; 2]) -> Property {
    if b[0] == b[1] {
        return constant(b[0]);
    }
    property(AGE, 0, function(0, false, Shape::Identity, b[0], b[1]))
}

/// Straight lines through `points` (x rising), flat past both ends.
fn lines(points: &[(f32, f32)]) -> Shape {
    let mut segments = Vec::new();
    for w in points.windows(2) {
        let ((x0, y0), (x1, y1)) = (w[0], w[1]);
        if x1 <= x0 {
            continue;
        }
        let c = (y1 - y0) / (x1 - x0);
        segments.push(Segment {
            end: x1,
            cubic: [0.0, 0.0, c, y0 - c * x0],
        });
    }
    let last = points.last().map_or(0.0, |p| p.1);
    segments.push(Segment {
        end: f32::MAX,
        cubic: [0.0, 0.0, 0.0, last],
    });
    if let Some(&(x0, y0)) = points.first()
        && x0 > 0.0
    {
        segments.insert(0, Segment { end: x0, cubic: [0.0, 0.0, 0.0, y0] });
    }
    Shape::Curve(segments, None)
}

/// `f` over 0..1, sampled at `n` steps.
fn sampled(f: impl Fn(f32) -> f32, n: usize) -> Shape {
    let points: Vec<(f32, f32)> = (0..=n).map(|k| k as f32 / n as f32).map(|x| (x, f(x))).collect();
    lines(&points)
}

/// A real colour (gamma space, as Halo CE stores it) made linear.
fn linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| srgb_to_linear(v.clamp(0.0, 1.0)))
}

/// A particle colour: linear, times [`COLOUR_SCALE`].
fn particle_colour(c: [f32; 3]) -> [f32; 3] {
    linear(c).map(|v| v * COLOUR_SCALE)
}

/// A colour picked between `a` and `b` by random variable `var`.
fn colour_between(var: u8, a: [f32; 3], b: [f32; 3]) -> Property {
    let (a, b) = (particle_colour(a), particle_colour(b));
    let mut f = function(1, a != b, Shape::Constant, 0.0, 1.0);
    f.colour_stops = if a == b { 1 } else { 2 };
    f.colours = [a, a, b, b];
    property(SYSTEM_AGE, var, f)
}

fn rgb(c: [f32; 4]) -> [f32; 3] {
    [c[1], c[2], c[3]]
}

fn mean(b: [f32; 2]) -> f32 {
    0.5 * (b[0] + b[1])
}

fn vector(v: [f32; 3]) -> VectorProperty {
    VectorProperty {
        property: constant(1.0),
        a: v,
        b: v,
    }
}

/// Halo CE's transition functions (0 linear, 1 early, 2 very early, 3
/// late, 4 very late, 5 cosine): how much of a light is left at `t` (0..1
/// of its life). Our reading of the names: an early falloff loses most of
/// its light early.
pub fn falloff(kind: u16, t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    match kind {
        1 => u * u,
        2 => u.powi(4),
        3 => 1.0 - t * t,
        4 => 1.0 - t.powi(4),
        5 => 0.5 + 0.5 * (PI * t).cos(),
        _ => u,
    }
}

// ---- physics ----

/// A `pphy` as Halo 3's physics movement: gravity scale (minus the air
/// buoyancy: smoke rises) and drag (air friction over the worked-out
/// mass; our reading: it slows smoke within a second or so, leaves stones
/// ballistic).
fn movement(p: Option<PointPhysics>) -> Movement {
    let Some(p) = p else { return Movement::default() };
    let drag = if p.mass > 0.0 { (p.air_friction / p.mass).clamp(0.0, 60.0) } else { 0.0 };
    Movement {
        template: None,
        flags: 0,
        movements: vec![MovementKind {
            kind: 0,
            parameters: vec![(0, constant(-p.air_buoyancy)), (1, constant(drag))],
        }],
    }
}

// ---- bitmaps ----

/// Where each image of a bitmap tag sits in its atlas: the images stacked
/// top to bottom at the left edge.
#[derive(Clone, Debug, PartialEq)]
pub struct AtlasLayout {
    pub width: u32,
    pub height: u32,
    /// Per image: `(x, y, width, height)` in pixels; `None` for an image
    /// that is not 2D.
    pub images: Vec<Option<(u32, u32, u32, u32)>>,
    /// The tag's sequences as sprites on the atlas.
    pub sequences: Vec<H3Sequence>,
    /// The mean sprite width of each sequence, in pixels of its image.
    pub sprite_pixels: Vec<f32>,
}

/// Lays a bitmap tag's images out as one atlas.
pub fn atlas_layout(map: &Map, bitmap: u32) -> Result<AtlasLayout> {
    let datum = map.tags.get(bitmap as usize).map_or(u32::MAX, |t| t.id);
    let images = crate::bitmap::images(map, datum)?;
    let mut placed = Vec::new();
    let (mut width, mut height) = (1u32, 0u32);
    for im in &images {
        if im.kind == 0 && im.width > 0 && im.height > 0 {
            placed.push(Some((0, height, im.width, im.height)));
            width = width.max(im.width);
            height += im.height;
        } else {
            placed.push(None);
        }
    }
    let height = height.max(1);
    let uv = |image: i16, l: f32, r: f32, t: f32, b: f32| -> Option<(H3Sprite, f32)> {
        let (x, y, w, h) = (*placed.get(usize::try_from(image).ok()?)?)?;
        let (fw, fh) = (width as f32, height as f32);
        Some((
            H3Sprite {
                bitmap: 0,
                left: (x as f32 + l * w as f32) / fw,
                right: (x as f32 + r * w as f32) / fw,
                top: (y as f32 + t * h as f32) / fh,
                bottom: (y as f32 + b * h as f32) / fh,
            },
            (r - l).abs() * w as f32,
        ))
    };
    let mut sequences = Vec::new();
    let mut sprite_pixels = Vec::new();
    let mut seqs = effect::sequences(map, bitmap)?;
    if seqs.is_empty() {
        // No sequences: the first image is the particle.
        seqs.push(effect::Sequence {
            name: String::new(),
            first_image: 0,
            image_count: 1,
            sprites: Vec::new(),
        });
    }
    for s in &seqs {
        let cut: Vec<(H3Sprite, f32)> = if s.sprites.is_empty() {
            let n = s.image_count.max(1);
            (s.first_image..s.first_image.saturating_add(n)).filter_map(|i| uv(i, 0.0, 1.0, 0.0, 1.0)).collect()
        } else {
            s.sprites.iter().filter_map(|p| uv(p.image, p.left, p.right, p.top, p.bottom)).collect()
        };
        let px = if cut.is_empty() { 1.0 } else { cut.iter().map(|c| c.1).sum::<f32>() / cut.len() as f32 };
        sprite_pixels.push(px);
        sequences.push(H3Sequence {
            name: s.name.clone(),
            first_bitmap: 0,
            bitmap_count: 1,
            sprites: cut.into_iter().map(|c| c.0).collect(),
        });
    }
    Ok(AtlasLayout {
        width,
        height,
        images: placed,
        sequences,
        sprite_pixels,
    })
}

/// A bitmap tag's atlas as RGBA8 levels, largest first, with every 2D
/// image decoded into its place. `mono`: every image held one channel (A8,
/// Y8, AY8), now in all four.
pub struct AtlasPixels {
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
    pub mono: bool,
}

/// Decodes the images of `bitmap` into the atlas `layout` says.
pub fn atlas_pixels(map: &Map, bitmap: u32, layout: &AtlasLayout) -> Result<AtlasPixels> {
    let datum = map.tags.get(bitmap as usize).map_or(u32::MAX, |t| t.id);
    let (w, h) = (layout.width as usize, layout.height as usize);
    let mut px = vec![0u8; w * h * 4];
    let mut mono = true;
    let mut any = false;
    for (i, place) in layout.images.iter().enumerate() {
        let Some((x, y, iw, ih)) = *place else { continue };
        let t = crate::bitmap::decode(map, datum, i)?;
        let Some(data) = t.level(0, 0) else { continue };
        let mut rgba = blam_texture::rgba::decode_level(t.format, iw, ih, data).map_err(|e| crate::Error::Invalid(e.to_string()))?;
        let one = matches!(t.format, blam_texture::Format::A8 | blam_texture::Format::Y8);
        if one {
            for p in rgba.chunks_exact_mut(4) {
                let v = if t.format == blam_texture::Format::A8 { p[3] } else { p[0] };
                p.copy_from_slice(&[v, v, v, v]);
            }
        }
        mono &= one;
        any = true;
        for row in 0..ih as usize {
            let src = &rgba[row * iw as usize * 4..(row + 1) * iw as usize * 4];
            let at = ((y as usize + row) * w + x as usize) * 4;
            px[at..at + src.len()].copy_from_slice(src);
        }
    }
    if !any {
        return Err(crate::Error::Invalid(format!("bitmap #{bitmap}: no 2D image")));
    }
    // Smaller levels by averaging 2 x 2 texels, down to 1 x 1.
    let mut levels = vec![px];
    let (mut lw, mut lh) = (w, h);
    while lw > 1 || lh > 1 {
        let (nw, nh) = ((lw / 2).max(1), (lh / 2).max(1));
        let prev = levels.last().unwrap();
        let mut next = vec![0u8; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((2 * x + dx).min(lw - 1), (2 * y + dy).min(lh - 1));
                        sum += prev[(sy * lw + sx) * 4 + c] as u32;
                    }
                    next[(y * nw + x) * 4 + c] = (sum / 4) as u8;
                }
            }
        }
        levels.push(next);
        (lw, lh) = (nw, nh);
    }
    Ok(AtlasPixels {
        width: layout.width,
        height: layout.height,
        levels,
        mono: mono && any,
    })
}

// ---- shaders ----

fn shader(key: u32, name: &str, bitmap: Option<u32>, bitmap_name: &str, s: &ParticleShader) -> Shader {
    // Halo CE's framebuffer blend functions as Halo 3's blend modes.
    let (mode, blend) = match s.blend {
        1 => ("multiply", Blend::Multiply),
        2 => ("double_multiply", Blend::Multiply),
        3 | 4 | 6 => ("additive", Blend::Additive),
        7 => ("add_src_times_srcalpha", Blend::Additive),
        _ => ("alpha_blend", Blend::AlphaBlend),
    };
    Shader {
        tag_index: key,
        name: name.to_string(),
        class: "part".into(),
        definition: r"shaders\particle".into(),
        options: vec![
            ("albedo".into(), "diffuse_only".into()),
            ("blend_mode".into(), mode.into()),
            ("black_point".into(), "off".into()),
        ],
        maps: bitmap
            .map(|b| ShaderMap {
                usage: "base_map".into(),
                base_usage: "base_map".into(),
                layer: None,
                bitmap: Some(b),
                bitmap_name: bitmap_name.to_string(),
                xform: Xform::IDENTITY,
            })
            .into_iter()
            .collect(),
        params: Vec::new(),
        base: None,
        alpha_map: None,
        albedo_color: None,
        blend,
        alpha_test: false,
        two_sided: true,
        special: false,
        terrain: None,
        animations: Vec::new(),
    }
}

fn is_additive(s: &ParticleShader) -> bool {
    matches!(s.blend, 3 | 4 | 6 | 7)
}

// ---- the conversion ----

/// Builds Halo 3's effect library from Halo CE effect tags (`roots`, tag
/// indices) and everything they reach. Bitmap keys are tag indices; their
/// atlases are in `layouts`.
pub struct Converted {
    pub library: Library,
    pub layouts: BTreeMap<u32, AtlasLayout>,
}

struct Builder<'m> {
    map: &'m Map,
    lib: Library,
    layouts: BTreeMap<u32, AtlasLayout>,
    physics: HashMap<u32, Option<PointPhysics>>,
}

/// Converts `roots` (effe tag indices) and what they reach.
pub fn convert(map: &Map, roots: &[u32]) -> Converted {
    let mut b = Builder {
        map,
        lib: Library::default(),
        layouts: BTreeMap::new(),
        physics: HashMap::new(),
    };
    for &r in roots {
        b.effect(r, 0);
    }
    Converted {
        library: b.lib,
        layouts: b.layouts,
    }
}

impl Builder<'_> {
    fn name(&self, tag: u32) -> String {
        self.map.tags.get(tag as usize).map_or_else(String::new, |t| t.name.clone())
    }

    fn physics(&mut self, tag: Option<u32>) -> Option<PointPhysics> {
        let tag = tag?;
        if let Some(p) = self.physics.get(&tag) {
            return *p;
        }
        let p = match effect::read_physics(self.map, tag) {
            Ok(p) => Some(p),
            Err(e) => {
                self.lib.errors.push(format!("pphy {}: {e}", self.name(tag)));
                None
            }
        };
        self.physics.insert(tag, p);
        p
    }

    /// The atlas of a bitmap, laid out once; `None` if it cannot be read.
    fn bitmap(&mut self, tag: Option<u32>) -> Option<u32> {
        let tag = tag?;
        if self.layouts.contains_key(&tag) {
            return Some(tag);
        }
        match atlas_layout(self.map, tag) {
            Ok(l) => {
                self.lib.sequences.insert(tag, l.sequences.clone());
                self.lib.bitmaps.insert(tag);
                self.layouts.insert(tag, l);
                Some(tag)
            }
            Err(e) => {
                self.lib.errors.push(format!("bitmap {}: {e}", self.name(tag)));
                None
            }
        }
    }

    fn effect(&mut self, tag: u32, depth: u32) {
        if self.lib.effects.contains_key(&tag) || depth > MAX_DEPTH {
            return;
        }
        let e = match effect::read_effect(self.map, tag) {
            Ok(e) => e,
            Err(err) => {
                self.lib.errors.push(format!("effe {}: {err}", self.name(tag)));
                return;
            }
        };
        // Put a placeholder in first, so an effect reaching itself stops.
        self.lib.by_name.insert(e.name.to_ascii_lowercase(), tag);
        self.lib.effects.insert(
            tag,
            H3Effect {
                tag,
                name: e.name.clone(),
                flags: e.flags,
                locations: Vec::new(),
                events: Vec::new(),
                loop_start: None,
                looping_sound: None,
            },
        );
        let mut events = Vec::new();
        for (i, ev) in e.events.iter().enumerate() {
            let mut parts = Vec::new();
            let mut systems = Vec::new();
            for p in &ev.parts {
                let Some(what) = &p.what else { continue };
                match what.class.as_str() {
                    "pctl" => {
                        systems.extend(self.system(what.tag, p.location));
                        continue;
                    }
                    "ligh" => {
                        if !self.light(what.tag) {
                            continue;
                        }
                    }
                    "snd!" => {
                        self.lib.sound_names.insert(what.tag, self.name(what.tag));
                    }
                    "effe" => self.effect(what.tag, depth + 1),
                    _ => {}
                }
                parts.push(H3Part {
                    what: Some(Ref {
                        class: FourCc::from_code(&what.class),
                        tag: what.tag,
                    }),
                    location: p.location,
                    velocity: p.velocity,
                    velocity_orientation: [0.0, 0.0],
                    velocity_cone: p.velocity_cone,
                    angular_velocity: p.angular_velocity,
                    radius_modifier: p.radius_modifier,
                    origin_offset: [0.0; 3],
                });
            }
            for q in &ev.particles {
                if let Some(s) = self.effect_particle(q, depth) {
                    systems.push(s);
                }
            }
            events.push(H3Event {
                name: format!("event {i}"),
                flags: 0,
                skip_fraction: ev.skip_fraction,
                delay: ev.delay,
                duration: ev.duration,
                parts,
                systems,
            });
        }
        let out = self.lib.effects.get_mut(&tag).unwrap();
        out.locations = e.locations;
        out.events = events;
    }

    /// A light, read once; false if it cannot be.
    fn light(&mut self, tag: u32) -> bool {
        if self.lib.lights.contains_key(&tag) {
            return true;
        }
        let l = match effect::read_light(self.map, tag) {
            Ok(l) => l,
            Err(e) => {
                self.lib.errors.push(format!("ligh {}: {e}", self.name(tag)));
                return false;
            }
        };
        // From the upper colour to the lower as the falloff runs out.
        let mut colour = function(8, false, sampled(|t| falloff(l.falloff, t), 12), 0.0, 1.0);
        colour.colour_stops = 2;
        let (lo, hi) = (linear(rgb(l.colour_lower)), linear(rgb(l.colour_upper)));
        colour.colours = [lo, lo, hi, hi];
        self.lib.lights.insert(
            tag,
            H3Light {
                tag,
                name: l.name.clone(),
                flags: l.flags,
                kind: 0,
                size: l.radius.max(0.1) / LIGHT_RANGE_PER_SIZE,
                colour: property(AGE, 0, colour),
                intensity: constant(LIGHT_INTENSITY),
                duration: l.duration_seconds().max(1.0 / TICKS),
            },
        );
        true
    }

    /// An event's `part` particle as one emitter of its own system.
    fn effect_particle(&mut self, q: &effect::EffectParticle, depth: u32) -> Option<H3System> {
        let tag = q.particle?;
        let part = match effect::read_particle(self.map, tag) {
            Ok(p) => p,
            Err(e) => {
                self.lib.errors.push(format!("part {}: {e}", self.name(tag)));
                return None;
            }
        };
        let bitmap = self.bitmap(part.bitmap);
        let physics = self.physics(part.physics);
        let life = mean(part.lifespan).max(0.01);
        if !self.lib.particles.contains_key(&tag) {
            let (first, count) = sequences_of(&part);
            let frames = bitmap
                .and_then(|b| self.layouts.get(&b))
                .map_or(1, |l| l.sequences.iter().skip(first.max(0) as usize).take(count.max(1) as usize).map(|s| s.sprites.len()).sum::<usize>())
                .max(1);
            let rate = mean(part.animation_rate);
            let (frame, animation_rate) = if rate > 0.0 {
                (constant(0.0), constant(rate * life / frames as f32))
            } else {
                (ranged(R_FRAME, [0.0, 0.9999]), constant(0.0))
            };
            let mut attachments = Vec::new();
            if let Some(d) = part.death_effect.as_ref().filter(|d| d.class == "effe") {
                self.effect(d.tag, depth + 1);
                attachments.push(Attachment {
                    what: Ref {
                        class: FourCc::from_code("effe"),
                        tag: d.tag,
                    },
                    trigger: 2,
                });
            }
            let bitmap_name = bitmap.map(|b| self.name(b)).unwrap_or_default();
            self.lib.particles.insert(
                tag,
                H3Particle {
                    tag,
                    name: part.name.clone(),
                    flags: part.flags,
                    attachments,
                    appearance_flags: 0,
                    billboard: match part.orientation {
                        Orientation::ScreenFacing => Billboard::Camera,
                        Orientation::ParallelToDirection => Billboard::Velocity,
                        Orientation::PerpendicularToDirection => Billboard::Perpendicular,
                    },
                    first_sequence: first,
                    sequence_count: count,
                    center_offset: 0.0,
                    shader: shader(tag, &part.name, bitmap, &bitmap_name, &part.shader),
                    aspect: constant(1.0),
                    colour: constant(1.0),
                    intensity: constant(1.0),
                    alpha: constant(1.0),
                    animation_flags: 0,
                    frame,
                    animation_rate,
                    palette: constant(0.0),
                },
            );
        }
        // Fades in and out over its seconds, as a share of a mean life.
        let (fade_in, fade_out) = ((part.fade_in / life).clamp(0.0, 1.0), (part.fade_out / life).clamp(0.0, 1.0));
        let alpha_scale = 0.5 * (q.tint[0][0] + q.tint[1][0]);
        let alpha = sampled(
            |x| {
                let a = if fade_in > 0.0 { (x / fade_in).min(1.0) } else { 1.0 };
                let b = if fade_out > 0.0 { ((1.0 - x) / fade_out).min(1.0) } else { 1.0 };
                (a * b).clamp(0.0, 1.0) * alpha_scale
            },
            16,
        );
        let emitter = Emitter {
            name: part.name.clone(),
            shape: 0,
            bounding_radius: 0.0,
            offset: vector(q.offset),
            direction: vector(if q.velocity_cone >= PI - 1e-3 { [0.0; 3] } else { q.direction_vector }),
            radius: ranged(R_PLACE, q.distribution_radius),
            angle: constant(q.velocity_cone.to_degrees()),
            axis_angle: constant(0.0),
            starting_count: ranged(PER_EMITTER, q.count.map(|c| c.max(0) as f32)),
            max_count: constant(0.0),
            rate: constant(0.0),
            lifespan: ranged(R_LIFE, part.lifespan),
            movement: movement(physics),
            self_acceleration: vector([0.0; 3]),
            velocity: ranged(R_SPEED, q.velocity),
            rotation: ranged(R_ROTATION, [0.0, 1.0]),
            rotation_rate: ranged(R_SPIN, q.angular_velocity.map(|v| v / TAU)),
            size: ranged(R_SIZE, q.radius.map(|r| 2.0 * r)),
            scale: over_life(part.radius_animation),
            tint: colour_between(R_TINT, rgb(q.tint[0]), rgb(q.tint[1])),
            alpha: property(AGE, 0, function(8, false, alpha, 0.0, 1.0)),
            black_point: constant(0.0),
        };
        Some(H3System {
            particle: Some(tag),
            location: q.location as i32,
            local: false,
            sort_bias: 0,
            flags: 0,
            lod_distance: 0.0,
            emitters: vec![emitter],
        })
    }

    /// A `pctl` as systems: one per particle state a particle is seen in.
    fn system(&mut self, tag: u32, location: i16) -> Vec<H3System> {
        let sys = match effect::read_system(self.map, tag) {
            Ok(s) => s,
            Err(e) => {
                self.lib.errors.push(format!("pctl {}: {e}", self.name(tag)));
                return Vec::new();
            }
        };
        let system_physics = self.physics(sys.physics);
        let mut out = Vec::new();
        for (ti, ty) in sys.types.iter().enumerate() {
            let states = &ty.particle_states;
            if states.is_empty() {
                continue;
            }
            // States seen: all but the last (`death`, what the last one
            // fades into), unless there is only one.
            let seen = if states.len() > 1 { states.len() - 1 } else { 1 };
            // The timeline at the mean durations: state k is fully there
            // from start[k] for its duration, then turns into k + 1 over
            // its transition.
            let mut start = vec![0.0f32; seen + 1];
            for k in 0..seen {
                start[k + 1] = start[k] + mean(states[k].duration) + mean(states[k].transition);
            }
            let total = start[seen].max(0.05);
            let life = [
                states[..seen].iter().map(|s| s.duration[0] + s.transition[0]).sum::<f32>().max(0.05),
                states[..seen].iter().map(|s| s.duration[1] + s.transition[1]).sum::<f32>().max(0.05),
            ];
            let speed = match (ty.creation_physics, ty.constants.as_slice()) {
                (1, [a, b, ..]) => [a * TICKS, b * TICKS],
                _ => [0.0, 0.0],
            };
            for k in 0..seen {
                let s = &states[k];
                let key = state_key(tag, ti, k);
                let bitmap = self.bitmap(s.bitmap);
                let pixels = bitmap
                    .and_then(|b| self.layouts.get(&b))
                    .and_then(|l| l.sprite_pixels.get(s.sequence.max(0) as usize).copied())
                    .unwrap_or(32.0);
                let size_of = |st: &effect::ParticleState| mean(st.scale) * pixels;
                let own = size_of(s).max(1e-4);
                let frames = bitmap
                    .and_then(|b| self.layouts.get(&b))
                    .and_then(|l| l.sequences.get(s.sequence.max(0) as usize))
                    .map_or(1, |q| q.sprites.len())
                    .max(1);
                let rate = mean(s.animation_rate);
                let (frame, animation_rate) = if rate > 0.0 {
                    (constant(0.0), constant(rate * total / frames as f32))
                } else {
                    (ranged(R_FRAME, [0.0, 0.9999]), constant(0.0))
                };
                if !self.lib.particles.contains_key(&key) {
                    let name = format!("{} {} {}", sys.name, ty.name, s.name);
                    let bitmap_name = bitmap.map(|b| self.name(b)).unwrap_or_default();
                    self.lib.particles.insert(
                        key,
                        H3Particle {
                            tag: key,
                            name: name.clone(),
                            flags: 0,
                            attachments: Vec::new(),
                            appearance_flags: 0,
                            billboard: Billboard::Camera,
                            first_sequence: s.sequence.max(0),
                            sequence_count: 1,
                            center_offset: 0.0,
                            shader: shader(key, &name, bitmap, &bitmap_name, &s.shader),
                            aspect: constant(1.0),
                            colour: constant(1.0),
                            intensity: constant(1.0),
                            alpha: constant(1.0),
                            animation_flags: 0,
                            frame,
                            animation_rate,
                            palette: constant(0.0),
                        },
                    );
                }
                // Seen from the start of the previous state's transition to
                // the end of its own; the last state seen fades towards
                // `death` (alpha 0 on every run effect).
                let additive = is_additive(&s.shader);
                let strength = |st: &effect::ParticleState| -> f32 {
                    if additive {
                        1.0
                    } else {
                        0.5 * (st.colour[0][0] + st.colour[1][0])
                    }
                };
                let here = strength(s).max(1e-3);
                let rise = if k > 0 { start[k] - mean(states[k - 1].transition) } else { 0.0 };
                let full = start[k];
                let hold = start[k] + mean(s.duration);
                let end = start[k + 1];
                let next = &states[(k + 1).min(states.len() - 1)];
                let end_alpha = if k + 1 == seen && states.len() > 1 {
                    if additive {
                        0.0
                    } else {
                        (strength(next) / here).clamp(0.0, 1.0)
                    }
                } else {
                    0.0
                };
                let prev_size = if k > 0 { size_of(&states[k - 1]) / own } else { 1.0 };
                let next_size = size_of(next) / own;
                let x = |t: f32| t / total;
                let alpha = lines(&[(x(rise), if k == 0 { 1.0 } else { 0.0 }), (x(full), 1.0), (x(hold), 1.0), (x(end), end_alpha)]);
                let scale = lines(&[(x(rise), prev_size), (x(full), 1.0), (x(hold), 1.0), (x(end), next_size)]);
                let alpha_f = function(8, false, alpha, 0.0, here);
                let emitter = Emitter {
                    name: format!("{} {}", ty.name, s.name),
                    shape: 0,
                    bounding_radius: ty.radius,
                    offset: vector([0.0; 3]),
                    direction: vector([0.0; 3]),
                    radius: ranged(R_PLACE, [0.0, ty.radius]),
                    angle: constant(180.0),
                    axis_angle: constant(0.0),
                    starting_count: constant(ty.initial_count.max(0) as f32),
                    max_count: constant(0.0),
                    rate: constant(0.0),
                    lifespan: ranged(R_LIFE, life),
                    movement: movement(self.physics(s.physics).or(system_physics)),
                    self_acceleration: vector([0.0; 3]),
                    velocity: ranged(R_SPEED, speed),
                    rotation: ranged(R_ROTATION, [0.0, 1.0]),
                    rotation_rate: ranged(R_SPIN, s.rotation_rate.map(|v| v / TAU)),
                    size: ranged(R_SIZE, s.scale.map(|v| v * pixels)),
                    scale: property(AGE, 0, function(8, false, scale, 0.0, 1.0)),
                    tint: colour_between(R_TINT, rgb(s.colour[0]), rgb(s.colour[1])),
                    alpha: property(AGE, 0, alpha_f),
                    black_point: constant(0.0),
                };
                out.push(H3System {
                    particle: Some(key),
                    location: location as i32,
                    local: false,
                    sort_bias: 0,
                    flags: 0,
                    lod_distance: 0.0,
                    emitters: vec![emitter],
                });
            }
        }
        out
    }
}

/// The effects a map's scripts name (their tag references, `halo1.md`
/// 12): what [`convert`] needs for a mission.
pub fn script_effects(scripts: &blam_script::ScriptData) -> Vec<u32> {
    let mut out: Vec<u32> = scripts.references.iter().filter(|(c, _)| c == "effe").filter_map(|(_, t)| *t).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The sequences a `part` is drawn from: its first, through the ones it
/// starts in and loops through (the final ones are left out).
fn sequences_of(p: &effect::Particle) -> (i16, i16) {
    let count = p.initial_sequences.max(0) + p.looping_sequences.max(0);
    (p.first_sequence.max(0), count.max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_meet_their_points() {
        let s = lines(&[(0.2, 0.0), (0.4, 1.0), (0.8, 1.0), (1.0, 0.5)]);
        let f = function(8, false, s, 0.0, 1.0);
        assert_eq!(f.scalar(0.1, 0.0), 0.0);
        assert!((f.scalar(0.3, 0.0) - 0.5).abs() < 1e-5);
        assert!((f.scalar(0.6, 0.0) - 1.0).abs() < 1e-5);
        assert!((f.scalar(0.9, 0.0) - 0.75).abs() < 1e-5);
        assert!((f.scalar(1.0, 0.0) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn ranged_picks_by_its_variable() {
        let p = ranged(R_SIZE, [2.0, 4.0]);
        assert_eq!(p.range, R_SIZE);
        assert_eq!(p.function.scalar(0.0, 0.5), 3.0);
    }

    #[test]
    fn early_falloff_loses_light_early() {
        assert!(falloff(1, 0.5) < falloff(0, 0.5));
        assert_eq!(falloff(0, 0.0), 1.0);
        assert!(falloff(5, 1.0).abs() < 1e-6);
    }
}
