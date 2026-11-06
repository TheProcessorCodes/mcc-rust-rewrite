//! Halo 3's reflection cube maps: the scenes the level was captured into at
//! points round every cluster, reflected by shaders whose
//! `environment_mapping` is `dynamic` (`docs/format/cubemaps.md`).
//!
//! - Every BSP's cube map tag (scenario structure BSP `+0x48`) holds its
//!   cubes; each cluster lists the capture points it uses (cluster `+0x10C`).
//!   The cubes of every BSP go up once, at load, as one cube array, the same
//!   capture shared by two BSPs once ([`CubeIndex`]).
//! - Each chunk (`build.rs`) picks up to three capture points: those of its
//!   clusters nearest its middle. They ride in the chunk's [`MeshTag`]; the
//!   fragment shader (`shaders/lightmap.wesl`) blends their cubes by the
//!   pixel's distance to each and looks the reflection up in them.
//! - A cube texel is a colour and a brightness, `rgb` (sRGB) times `alpha`,
//!   but every cube is stored at the same mean brightness (rgb about 0.25,
//!   alpha about 0.06, indoors and out): a cube gives the pattern of the
//!   light round its point, not its level. The level is the lightmap's: the
//!   shader divides the cube by its own mean and multiplies by the mean
//!   light the lightmap holds at the pixel (its SH's constant term), times
//!   [`CUBE_SCALE`].
//!
//! Without cubes (Halo 2, Reach, the test scene, a cluster with no point)
//! the SH stand-in stays.
//!
//! [`MeshTag`]: bevy::mesh::MeshTag

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};

use blam_geometry::Scene;
use blam_texture::rgba;

use super::to_render;

/// Capture points a chunk can name: three 10-bit indices fit a mesh tag
/// beside a 2-bit count; and a cube array has at most 2048 layers on the
/// GPUs we target (6 per cube).
pub const MAX_CUBES: usize = 2048 / 6;

/// The reflection's brightness against the mean light the lightmap holds
/// at the pixel, for a cube texel at its cube's mean: 1, the level the SH
/// stand-in reflected at. Matching the world drawn from a capture point
/// (`HALO3_REFLECTIONS=view`) gave 0.12, about a mean albedo, but that left
/// the metal floors that are mostly reflection near black: the Warthog
/// run's road in the tunnels (about a fifth of its brightness before the
/// cubes). At 1 Valhalla's shots come out as bright as before the cubes
/// (`docs/format/cubemaps.md` 4). `HALO3_CUBE_SCALE` overrides.
pub const CUBE_SCALE: f32 = 1.0;

/// Every capture point of the scene, numbered as the cube array's cubes.
#[derive(Default)]
pub struct CubeIndex {
    /// Halo-frame capture points, by cube.
    points: Vec<Vec3>,
    /// Where each cube's bitmap is: the `bitm` tag and the bitmap in it.
    sources: Vec<(u32, u32)>,
    /// The cubes each (BSP, cluster) lists.
    by_cluster: HashMap<(u32, u32), Vec<u32>>,
    /// Every cube a BSP lists.
    by_bsp: HashMap<u32, Vec<u32>>,
}

impl CubeIndex {
    pub fn new(scene: &Scene) -> CubeIndex {
        let mut index = CubeIndex::default();
        let mut by_id: HashMap<u32, u32> = HashMap::new();
        for bsp in &scene.bsps {
            let Some(tag) = bsp.cubemap_bitmap else { continue };
            for p in &bsp.cubemaps {
                let cube = match by_id.get(&p.id) {
                    Some(c) => *c,
                    None if index.points.len() < MAX_CUBES => {
                        let c = index.points.len() as u32;
                        index.points.push(p.position);
                        index.sources.push((tag, p.cube));
                        by_id.insert(p.id, c);
                        c
                    }
                    None => continue,
                };
                let list = index.by_cluster.entry((bsp.index, p.cluster)).or_default();
                if !list.contains(&cube) {
                    list.push(cube);
                }
                let list = index.by_bsp.entry(bsp.index).or_default();
                if !list.contains(&cube) {
                    list.push(cube);
                }
            }
        }
        index
    }

    pub fn len(&self) -> usize {
        self.points.len()
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The mesh tag of a chunk of BSP `bsp` lying in `clusters` (empty: not
    /// known) around `centre` (Halo frame): the count in the top two bits,
    /// then up to three cube numbers of 10 bits, nearest first. The
    /// clusters' own cubes; failing those the BSP's; failing those (things
    /// of no BSP) any. 0: no cube.
    pub fn tag(&self, bsp: Option<u32>, clusters: &[u16], centre: Vec3) -> u32 {
        if self.points.is_empty() {
            return 0;
        }
        let mut candidates: Vec<u32> = Vec::new();
        if let Some(b) = bsp {
            for c in clusters {
                for cube in self.by_cluster.get(&(b, u32::from(*c))).into_iter().flatten() {
                    if !candidates.contains(cube) {
                        candidates.push(*cube);
                    }
                }
            }
            if candidates.is_empty() {
                candidates = self.by_bsp.get(&b).cloned().unwrap_or_default();
            }
        }
        if candidates.is_empty() {
            candidates = (0..self.points.len() as u32).collect();
        }
        candidates.sort_by(|a, b| {
            let da = self.points[*a as usize].distance_squared(centre);
            let db = self.points[*b as usize].distance_squared(centre);
            da.total_cmp(&db)
        });
        candidates.truncate(3);
        let mut tag = (candidates.len() as u32) << 30;
        for (k, c) in candidates.iter().enumerate() {
            tag |= (c & 0x3FF) << (10 * k);
        }
        tag
    }

    /// The capture points as a texture of one row: render-frame position,
    /// and one over the cube's mean brightness (`CubeStats::brightness`)
    /// in w. Read with `textureLoad`.
    pub fn points_image(&self, brightness: &[f32]) -> Image {
        let n = self.points.len().max(1) as u32;
        let mut data = Vec::with_capacity(16 * n as usize);
        for (k, p) in self.points.iter().enumerate() {
            let r = to_render(*p);
            let inverse = brightness.get(k).filter(|b| **b > 1e-5).map_or(0.0, |b| 1.0 / b);
            for v in [r.x, r.y, r.z, inverse] {
                data.extend_from_slice(&v.to_le_bytes());
            }
        }
        data.resize(16 * n as usize, 0);
        Image::new(
            Extent3d {
                width: n,
                height: 1,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            data,
            TextureFormat::Rgba32Float,
            RenderAssetUsages::RENDER_WORLD,
        )
    }

    /// Decodes every cube (in parallel) into one cube array: RGBA8, colour
    /// as sRGB and brightness in alpha, all at the largest cube's size
    /// (smaller ones scaled up), with a full mip chain for rough surfaces.
    /// `None` when there are no cubes or none decoded.
    pub fn cube_array(&self, cache: &blam_cache::CacheFile) -> Option<(Image, CubeStats)> {
        if self.sources.is_empty() {
            return None;
        }
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
        let faces: Vec<Option<Face6>> = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..threads)
                .map(|k| {
                    let sources = &self.sources;
                    s.spawn(move || {
                        let Ok(resources) = cache.resources() else {
                            return Vec::new();
                        };
                        let mut tags: HashMap<u32, blam_texture::bitmap::BitmapTag> = HashMap::new();
                        sources
                            .iter()
                            .enumerate()
                            .skip(k)
                            .step_by(threads)
                            .map(|(i, (tag, bitmap))| {
                                let header = match tags.entry(*tag) {
                                    std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                                    std::collections::hash_map::Entry::Vacant(e) => {
                                        e.insert(blam_texture::bitmap::read_tag(cache, *tag).ok()?)
                                    }
                                };
                                let t = blam_texture::bitmap::decode_from(&resources, header, *bitmap).ok()?;
                                Some((i, decode_faces(&t)))
                            })
                            .collect::<Vec<Option<(usize, Option<Face6>)>>>()
                    })
                })
                .collect();
            let mut out: Vec<Option<Face6>> = vec![None; self.sources.len()];
            for j in jobs {
                for (i, f) in j.join().unwrap_or_default().into_iter().flatten() {
                    out[i] = f;
                }
            }
            out
        });
        let size = faces.iter().flatten().map(|f| f.size).max()?;
        let levels = size.ilog2() + 1;
        let mut stats = CubeStats {
            decoded: faces.iter().flatten().count(),
            size,
            ..Default::default()
        };
        // Level-major per layer: wgpu wants layer 0's whole chain, then
        // layer 1's ... (bevy's default `TextureDataOrder::LayerMajor`).
        let mut data = Vec::new();
        for f in &faces {
            let f = match f {
                Some(f) => f.scaled(size),
                None => Face6::grey(size),
            };
            stats.brightness.push(f.brightness());
            for face in &f.faces {
                let mut level = face.clone();
                let mut s = size;
                for _ in 0..levels {
                    data.extend_from_slice(&level);
                    if s > 1 {
                        level = halve(&level, s);
                        s /= 2;
                    }
                }
            }
        }
        let mut image = Image::new_uninit(
            Extent3d {
                width: size,
                height: size,
                depth_or_array_layers: 6 * faces.len() as u32,
            },
            TextureDimension::D2,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = levels;
        image.data = Some(data);
        image.texture_view_descriptor = Some(TextureViewDescriptor {
            dimension: Some(TextureViewDimension::CubeArray),
            ..default()
        });
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::ClampToEdge,
            address_mode_v: ImageAddressMode::ClampToEdge,
            address_mode_w: ImageAddressMode::ClampToEdge,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            ..default()
        });
        Some((image, stats))
    }

    /// The capture points, Halo frame, by cube.
    pub fn points(&self) -> &[Vec3] {
        &self.points
    }
}

/// What [`CubeIndex::cube_array`] made.
#[derive(Default)]
pub struct CubeStats {
    pub decoded: usize,
    pub size: u32,
    /// Per cube, the mean over its six faces of `luminance(rgb) * alpha`,
    /// rgb read as sRGB.
    pub brightness: Vec<f32>,
}

/// Six square RGBA8 faces, `+X -X +Y -Y +Z -Z`.
#[derive(Clone)]
struct Face6 {
    size: u32,
    faces: Vec<Vec<u8>>,
}

impl Face6 {
    fn grey(size: u32) -> Face6 {
        // A dim grey: alpha small, as the real ones are.
        let px = [128u8, 128, 128, 4];
        Face6 {
            size,
            faces: vec![px.repeat((size * size) as usize); 6],
        }
    }

    /// Bilinear resize of every face to `size`.
    fn scaled(&self, size: u32) -> Face6 {
        if size == self.size {
            return self.clone();
        }
        let (src, n) = (self.size as f32, self.size as i32);
        let faces = self
            .faces
            .iter()
            .map(|f| {
                let mut out = Vec::with_capacity((4 * size * size) as usize);
                for y in 0..size {
                    for x in 0..size {
                        let u = (x as f32 + 0.5) * src / size as f32 - 0.5;
                        let v = (y as f32 + 0.5) * src / size as f32 - 0.5;
                        let (x0, y0) = (u.floor() as i32, v.floor() as i32);
                        let (fu, fv) = (u - x0 as f32, v - y0 as f32);
                        let at = |x: i32, y: i32, c: usize| {
                            let (x, y) = (x.clamp(0, n - 1) as usize, y.clamp(0, n - 1) as usize);
                            f[4 * (y * n as usize + x) + c] as f32
                        };
                        for c in 0..4 {
                            let top = at(x0, y0, c) * (1.0 - fu) + at(x0 + 1, y0, c) * fu;
                            let bottom = at(x0, y0 + 1, c) * (1.0 - fu) + at(x0 + 1, y0 + 1, c) * fu;
                            out.push((top * (1.0 - fv) + bottom * fv).round() as u8);
                        }
                    }
                }
                out
            })
            .collect();
        Face6 { size, faces }
    }

    /// The mean of `luminance(rgb) * alpha`, rgb read as sRGB.
    fn brightness(&self) -> f32 {
        let lin = |c: u8| {
            let c = c as f32 / 255.0;
            if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
        };
        let (mut sum, mut n) = (0f64, 0f64);
        for f in &self.faces {
            for px in f.chunks_exact(4) {
                let lum = 0.212656 * lin(px[0]) + 0.715158 * lin(px[1]) + 0.0721856 * lin(px[2]);
                sum += (lum * px[3] as f32 / 255.0) as f64;
                n += 1.0;
            }
        }
        (sum / n.max(1.0)) as f32
    }
}

/// Level 0 of the six faces of a cube bitmap, decoded to RGBA8.
fn decode_faces(t: &blam_texture::Texture) -> Option<Face6> {
    if t.layers != 6 || t.width != t.height || t.width == 0 {
        return None;
    }
    let faces = (0..6)
        .map(|l| rgba::decode_level(t.format, t.width, t.height, t.level(0, l)?).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(Face6 { size: t.width, faces })
}

/// One mip level down: the mean of each 2x2 block.
fn halve(level: &[u8], size: u32) -> Vec<u8> {
    let (s, h) = (size as usize, (size / 2).max(1) as usize);
    let mut out = Vec::with_capacity(4 * h * h);
    for y in 0..h {
        for x in 0..h {
            for c in 0..4 {
                let at = |dx: usize, dy: usize| level[4 * ((2 * y + dy).min(s - 1) * s + (2 * x + dx).min(s - 1)) + c] as u32;
                out.push(((at(0, 0) + at(1, 0) + at(0, 1) + at(1, 1) + 2) / 4) as u8);
            }
        }
    }
    out
}

/// How the shader reflects (`HALO3_REFLECTIONS`): 0 the SH stand-in, 1 the
/// cube maps (the default), 2 every surface a mirror of the cubes alone
/// (for checking the cubes line up with the world), 3 the cubes seen along
/// the view ray (`view`: from a capture point it should look like the
/// world).
pub fn mode() -> f32 {
    match std::env::var("HALO3_REFLECTIONS").as_deref() {
        Ok("0") => 0.0,
        Ok("mirror") => 2.0,
        Ok("view") => 3.0,
        _ => 1.0,
    }
}

/// [`CUBE_SCALE`], or `HALO3_CUBE_SCALE`.
pub fn cube_scale() -> f32 {
    std::env::var("HALO3_CUBE_SCALE").ok().and_then(|v| v.parse().ok()).unwrap_or(CUBE_SCALE)
}

/// The matrix (rows) from a Halo-frame direction to a cube lookup
/// direction. `HALO3_CUBE_AXES=x,z,-y` (each row a signed axis) overrides.
pub fn cube_axes() -> [Vec4; 3] {
    let parse = |s: &str| -> Option<Vec4> {
        let (sign, axis) = match s.trim().strip_prefix('-') {
            Some(a) => (-1.0, a),
            None => (1.0, s.trim()),
        };
        let v = match axis {
            "x" => Vec3::X,
            "y" => Vec3::Y,
            "z" => Vec3::Z,
            _ => return None,
        };
        Some((v * sign).extend(0.0))
    };
    if let Ok(v) = std::env::var("HALO3_CUBE_AXES") {
        let rows: Vec<Vec4> = v.split(',').filter_map(parse).collect();
        if let [a, b, c] = rows[..] {
            return [a, b, c];
        }
    }
    [Vec4::X, Vec4::Y, Vec4::Z]
}
