//! Particles and contrails on the GPU: one mesh per emitter (and per
//! contrail), rebuilt every frame from the simulation, drawn with the
//! particle's own render method (`shaders/particle.wesl`).
//!
//! Per vertex: the position (renderer metres, relative to the entity), the
//! sprite UV, the colour (tint x colour x intensity, alpha) and in UV 1
//! the corner of the quad (0..1) plus twice the palette row and the black
//! point, each in 255ths: `uv_b = corner + 2 * (palette, black)`. The
//! shader splits them again.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

use blam_effect::{Billboard, Library};
use mcc_core::coords::{to_render, to_render_dir};

use crate::load::Rgba;
use crate::sim::{self, System};

pub type ParticleMaterial = ExtendedMaterial<StandardMaterial, ParticleShading>;

pub const SHADER: &str = "embedded://mcc_effects/shaders/particle.wesl";

/// The particle half of [`ParticleMaterial`].
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct ParticleShading {
    /// x: albedo mode (0 diffuse_only, 1 diffuse_plus_billboard_alpha, 2
    /// palettized, 3 palettized_plus_billboard_alpha, 4
    /// diffuse_plus_sprite_alpha, 5 palettized_plus_sprite_alpha), y: 1
    /// when the alpha map is a single channel, z: 1 when the black point
    /// is on, w: exposure.
    #[uniform(100)]
    pub mode: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub base: Option<Handle<Image>>,
    #[texture(103)]
    pub alpha: Option<Handle<Image>>,
    #[texture(104)]
    pub palette: Option<Handle<Image>>,
}

impl MaterialExtension for ParticleShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

/// The albedo mode number of a particle or contrail shader.
pub fn albedo_mode(s: &blam_texture::Shader) -> f32 {
    match s.option("albedo").unwrap_or("diffuse_only") {
        "diffuse_plus_billboard_alpha" => 1.0,
        "palettized" => 2.0,
        "palettized_plus_billboard_alpha" => 3.0,
        "diffuse_plus_sprite_alpha" => 4.0,
        "palettized_plus_sprite_alpha" | "palettized_plus_alpha" => 5.0,
        _ => 0.0,
    }
}

/// How the particle blends into what is behind it.
pub fn alpha_mode(s: &blam_texture::Shader) -> AlphaMode {
    match s.option("blend_mode").unwrap_or("additive") {
        "alpha_blend" | "inv_alpha_blend" => AlphaMode::Blend,
        "pre_multiplied_alpha" => AlphaMode::Premultiplied,
        "multiply" | "double_multiply" => AlphaMode::Multiply,
        // additive, add_src_times_srcalpha, add_src_times_dstalpha, maximum,
        // multiply_add: added on top.
        _ => AlphaMode::Add,
    }
}

/// A decoded bitmap as an image with every level, clamped at the edges;
/// sampled as sRGB colour or as plain numbers.
pub fn image(t: &Rgba, srgb: bool) -> Image {
    let mut data = Vec::new();
    for l in &t.levels {
        data.extend_from_slice(l);
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = t.levels.len() as u32;
    image.data = Some(data);
    let mips = t.levels.len() > 1;
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: if mips { ImageFilterMode::Linear } else { ImageFilterMode::Nearest },
        ..default()
    });
    image
}

/// Mesh buffers being filled.
#[derive(Default)]
pub struct Quads {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uv0: Vec<[f32; 2]>,
    pub uv1: Vec<[f32; 2]>,
    pub colours: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl Quads {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// One quad: corners `c` in order (0,0) (1,0) (1,1) (0,1) of `uv`.
    #[allow(clippy::too_many_arguments)]
    fn quad(&mut self, c: [Vec3; 4], uv: [f32; 4], colour: [f32; 4], palette: f32, black: f32, normal: Vec3) {
        let base = self.positions.len() as u32;
        let k = Vec2::new((palette.clamp(0.0, 1.0) * 255.0).round(), (black.clamp(0.0, 1.0) * 255.0).round()) * 2.0;
        let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
        for (i, (u, v)) in corners.iter().enumerate() {
            self.positions.push(c[i].to_array());
            self.normals.push(normal.to_array());
            self.uv0.push([uv[0] + (uv[1] - uv[0]) * u, uv[2] + (uv[3] - uv[2]) * v]);
            self.uv1.push([u + k.x, v + k.y]);
            self.colours.push(colour);
        }
        self.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// One quad with no area: draws nothing, but needs the pipeline.
    pub fn degenerate() -> Quads {
        let mut q = Quads::default();
        q.quad([Vec3::ZERO; 4], [0.0, 1.0, 0.0, 1.0], [0.0; 4], 0.0, 0.0, Vec3::Y);
        q
    }

    pub fn into_mesh(self) -> Mesh {
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, self.positions);
        m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, self.uv0);
        m.insert_attribute(Mesh::ATTRIBUTE_UV_1, self.uv1);
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, self.colours);
        m.insert_indices(Indices::U32(self.indices));
        m
    }
}

/// The camera, renderer frame.
#[derive(Clone, Copy)]
pub struct Eye {
    pub position: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub forward: Vec3,
}

/// The sprite rectangles a particle may show: `(left, right, top, bottom)`.
pub fn frames(lib: &Library, sequences: &std::collections::HashMap<u32, Vec<blam_texture::bitmap::Sequence>>, particle: u32) -> Vec<[f32; 4]> {
    let Some(p) = lib.particles.get(&particle) else { return vec![[0.0, 1.0, 0.0, 1.0]] };
    let Some(base) = p.shader.map("base_map").and_then(|m| m.bitmap) else { return vec![[0.0, 1.0, 0.0, 1.0]] };
    let Some(seqs) = sequences.get(&base) else { return vec![[0.0, 1.0, 0.0, 1.0]] };
    let first = p.first_sequence.max(0) as usize;
    let count = if p.sequence_count > 0 { p.sequence_count as usize } else { seqs.len() };
    let out: Vec<[f32; 4]> = seqs
        .iter()
        .skip(first)
        .take(count.max(1))
        .flat_map(|s| s.sprites.iter())
        .filter(|s| s.bitmap == 0 || s.bitmap == -1)
        .map(|s| [s.left, s.right, s.top, s.bottom])
        .collect();
    if out.is_empty() { vec![[0.0, 1.0, 0.0, 1.0]] } else { out }
}

/// One emitter's particles as quads relative to `origin` (renderer
/// metres).
pub fn system_quads(lib: &Library, s: &System, frames: &[[f32; 4]], eye: &Eye, origin: Vec3) -> Quads {
    let mut q = Quads::default();
    let (Some(em), Some(prt)) = (sim::emitter(lib, s), lib.particles.get(&s.particle)) else { return q };
    let m = blam_geometry::WORLD_UNIT_METRES;
    let mut order: Vec<usize> = (0..s.particles.len()).collect();
    if alpha_mode(&prt.shader) == AlphaMode::Blend {
        // Far first.
        let d = |i: usize| to_render(s.particles[i].position).distance_squared(eye.position);
        order.sort_by(|a, b| d(*b).total_cmp(&d(*a)));
    }
    for i in order {
        let p = &s.particles[i];
        let size = s.eval(&em.size, p) * s.eval(&em.scale, p);
        let alpha = s.eval(&em.alpha, p) * s.eval(&prt.alpha, p);
        if size <= 0.0 || alpha <= 0.0 {
            continue;
        }
        let tint = s.colour(&em.tint, p);
        let colour = s.colour(&prt.colour, p);
        let intensity = s.eval(&prt.intensity, p);
        let rgba = [
            tint[0] * colour[0] * intensity,
            tint[1] * colour[1] * intensity,
            tint[2] * colour[2] * intensity,
            alpha.min(1.0),
        ];
        let aspect = s.eval(&prt.aspect, p).max(0.01);
        let half_w = 0.5 * size * m;
        let half_h = half_w * aspect;
        let at = to_render(p.position) - origin;
        let world_at = to_render(p.position);
        let to_eye = (eye.position - world_at).normalize_or(-eye.forward);
        // Frame: random start plus the animation rate over the life.
        let f = (s.eval(&prt.frame, p) + s.eval(&prt.animation_rate, p) * p.age01()).fract();
        let uv = frames[((f * frames.len() as f32) as usize).min(frames.len() - 1)];
        let palette = s.eval(&prt.palette, p);
        let black = s.eval(&em.black_point, p);
        let corners = match prt.billboard {
            Billboard::Velocity => {
                let v = to_render_dir(p.velocity);
                let axis = v.normalize_or(eye.up);
                let side = axis.cross(to_eye).normalize_or(eye.right);
                // Stretched along the motion: the aspect is the length.
                let len = half_w * 2.0 * aspect;
                let centre = at - axis * len * (0.5 + prt.center_offset);
                let (a, b) = (axis * (len * 0.5), side * half_w);
                [centre - a - b, centre - a + b, centre + a + b, centre + a - b]
            }
            Billboard::Vertical => {
                let up = Vec3::Y;
                let side = up.cross(to_eye).normalize_or(eye.right);
                let (a, b) = (side * half_w, up * half_h);
                [at - a - b, at + a - b, at + a + b, at - a + b]
            }
            Billboard::Horizontal => {
                let (a, b) = (Vec3::X * half_w, Vec3::Z * half_h);
                [at - a - b, at + a - b, at + a + b, at - a + b]
            }
            Billboard::Perpendicular => {
                let n = to_render_dir(p.velocity).normalize_or(to_eye);
                let (r, u) = n.any_orthonormal_pair();
                let (a, b) = (r * half_w, u * half_h);
                [at - a - b, at + a - b, at + a + b, at - a + b]
            }
            Billboard::Camera => {
                let turn = std::f32::consts::TAU * (s.eval(&em.rotation, p) + s.eval(&em.rotation_rate, p) * p.age);
                let (sn, cs) = turn.sin_cos();
                let r = eye.right * cs + eye.up * sn;
                let u = eye.up * cs - eye.right * sn;
                let (a, b) = (r * half_w, u * half_h);
                [at - a - b, at + a - b, at + a + b, at - a + b]
            }
        };
        q.quad(corners, uv, rgba, palette, black, to_eye);
    }
    q
}

/// A contrail's ribbon relative to `origin` (renderer metres).
pub fn contrail_quads(lib: &Library, c: &sim::Contrail, eye: &Eye, origin: Vec3) -> Quads {
    let mut q = Quads::default();
    let Some(def) = lib.contrails.get(&c.tag).and_then(|t| t.contrails.get(c.index)) else { return q };
    let m = blam_geometry::WORLD_UNIT_METRES;
    let n = c.points.len();
    if n < 2 {
        return q;
    }
    // Input 7 reads as random per contrail (our reading, effects.md 7).
    let r = sim::random(c.seed, 3);
    let colour = def.colour.function.colour(r, r);
    let intensity = def.intensity.function.scalar(r, r);
    let black = def.black_point.function.scalar(r, r);
    let kb = 2.0 * (black.clamp(0.0, 1.0) * 255.0).round();
    let mut prev_side: Option<(Vec3, Vec3, f32, f32)> = None;
    for k in 0..n {
        let p = &c.points[k];
        let at = to_render(p.position);
        let next = to_render(c.points[(k + 1).min(n - 1)].position);
        let before = to_render(c.points[k.saturating_sub(1)].position);
        let dir = (next - before).normalize_or(eye.up);
        let side = dir.cross((eye.position - at).normalize_or(-eye.forward)).normalize_or(eye.right);
        let a = p.age01();
        let w = def.size.function.scalar(a, sim::random(p.seed, 5)) * m * 0.5;
        let alpha = def.alpha.function.scalar(a, sim::random(p.seed, 6));
        let u = k as f32 / (n - 1) as f32;
        let here = (at - origin, side * w, u, alpha);
        if let Some((p0, s0, u0, a0)) = prev_side {
            let (p1, s1, u1, a1) = here;
            let rgba0 = [colour[0] * intensity, colour[1] * intensity, colour[2] * intensity, a0.clamp(0.0, 1.0)];
            let rgba1 = [colour[0] * intensity, colour[1] * intensity, colour[2] * intensity, a1.clamp(0.0, 1.0)];
            let base = q.positions.len() as u32;
            for (pos, uv, rgba, cu) in [
                (p0 - s0, [u0 * def.repeat[0].max(0.01), 0.0], rgba0, [0.0, kb]),
                (p0 + s0, [u0 * def.repeat[0].max(0.01), 1.0], rgba0, [0.0, 1.0 + kb]),
                (p1 + s1, [u1 * def.repeat[0].max(0.01), 1.0], rgba1, [1.0, 1.0 + kb]),
                (p1 - s1, [u1 * def.repeat[0].max(0.01), 0.0], rgba1, [1.0, kb]),
            ] {
                q.positions.push(pos.to_array());
                q.normals.push((eye.position - at).normalize_or(Vec3::Y).to_array());
                q.uv0.push(uv);
                q.uv1.push(cu);
                q.colours.push(rgba);
            }
            q.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        prev_side = Some(here);
    }
    q
}
