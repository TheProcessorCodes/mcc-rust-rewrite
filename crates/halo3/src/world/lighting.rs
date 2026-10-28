//! Halo 3's baked lighting in the viewer, in place of bevy's lights.
//!
//! - Per-pixel lightmapped placements (every lit BSP cluster, most
//!   instances) keep their lightmap texcoord as `UV_1`; the fragment shader
//!   `world/shaders/lightmap.wesl` decodes the BSP's two DXT5 arrays there with
//!   the surface normal.
//! - Everything else gets its irradiance per vertex on the CPU, as the
//!   vertex colour: per-vertex lit instances from their own coefficients,
//!   probe-lit instances from their probe, and objects (scenery, crates,
//!   machines, which have no lighting of their own here) from the lightmap
//!   at the nearest lit BSP vertex.
//! - With no lightmap at all (`--test-scene`), a fixed sunny-day light.
//!
//! Formula and formats: `docs/format/lightmaps.md`.

use std::collections::HashMap;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;

use blam_geometry::lightmap::ShLight;
use blam_geometry::{Kind, Lighting, Scene};
use blam_texture::{Format, Texture, rgba};

use crate::textures::LightmapTextures;

/// Multiplies Halo's irradiance before bevy's tonemapping. Halo's own
/// exposure comes from tags not read yet; this was picked by comparing
/// screenshots with the game (`docs/viewer.md`). `HALO3_EXPOSURE` overrides.
pub const EXPOSURE: f32 = 4.0;

pub fn exposure() -> f32 {
    std::env::var("HALO3_EXPOSURE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(EXPOSURE)
}

pub type HaloMaterial = ExtendedMaterial<StandardMaterial, HaloLightmap>;

const SHADER: &str = "embedded://halo3/world/shaders/lightmap.wesl";

/// The Halo half of [`HaloMaterial`]: the BSP's two lightmap arrays
/// (absent for vertex-lit batches) and the ranges that scale them, and for
/// terrain (`rmtr`) the blend map and the four layers' base and detail maps
/// that replace the StandardMaterial's base colour texture.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct HaloLightmap {
    /// Ranges of c0..c3.
    #[uniform(100)]
    pub scales: Vec4,
    /// x: dominant light intensity range, y: exposure.
    #[uniform(100)]
    pub extra: Vec4,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub coefficients: Option<Handle<Image>>,
    #[texture(103, dimension = "2d_array")]
    pub intensity: Option<Handle<Image>>,
    /// Terrain: 1 for each layer that is drawn, 0 for one that is off. All
    /// zero on anything that is not terrain.
    #[uniform(100)]
    pub terrain_layers: Vec4,
    /// Terrain: x: the detail multiplier (`DETAIL_MULTIPLIER`), y: 1 when
    /// the blend weights are normalised.
    #[uniform(100)]
    pub terrain_extra: Vec4,
    /// Terrain: the blend map's xform (scale xy, offset zw).
    #[uniform(100)]
    pub blend_xform: Vec4,
    /// Terrain: each layer's base map xform.
    #[uniform(100)]
    pub base_xforms: [Vec4; 4],
    /// Terrain: each layer's detail map xform; zero scale means no detail.
    #[uniform(100)]
    pub detail_xforms: [Vec4; 4],
    /// Terrain blend map, linear (its texels are weights).
    #[texture(104)]
    #[sampler(105)]
    pub blend_map: Option<Handle<Image>>,
    /// Terrain base maps of layers 0..3. A layer that is off gets another
    /// layer's image so every slot binds a real texture; the sampler (repeat,
    /// trilinear) is layer 0's.
    #[texture(106)]
    #[sampler(107)]
    pub base_0: Option<Handle<Image>>,
    #[texture(108)]
    pub base_1: Option<Handle<Image>>,
    #[texture(109)]
    pub base_2: Option<Handle<Image>>,
    #[texture(110)]
    pub base_3: Option<Handle<Image>>,
    #[texture(111)]
    pub detail_0: Option<Handle<Image>>,
    #[texture(112)]
    pub detail_1: Option<Handle<Image>>,
    #[texture(113)]
    pub detail_2: Option<Handle<Image>>,
    #[texture(114)]
    pub detail_3: Option<Handle<Image>>,
}

/// Halo's detail-map multiplier: a detail map multiplies the base so that
/// its mid grey leaves the base unchanged. The detail texels are authored in
/// gamma space around 0.5; sampled as sRGB, mid grey reads as
/// `0.5^2.2 = 0.2176`, and `1 / 0.2176 = 2^2.2 = 4.595`.
pub const DETAIL_MULTIPLIER: f32 = 4.594_794;

impl MaterialExtension for HaloLightmap {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/lightmap.wesl");
        app.add_plugins(MaterialPlugin::<HaloMaterial>::default());
    }
}

/// One lightmap array as a GPU image: BC3 kept compressed when the device
/// takes it, else expanded to RGBA8. Linear (the texels are not colours),
/// clamped, bilinear, one level.
pub fn array_image(t: &Texture, bc: bool) -> Result<Image, String> {
    let (format, data) = if bc && t.format == Format::Bc3 {
        (TextureFormat::Bc3RgbaUnorm, t.mips[0].clone())
    } else {
        let mut data = Vec::new();
        for l in 0..t.layers {
            let level = t.level(0, l).ok_or_else(|| format!("layer {l} is short"))?;
            data.extend(
                rgba::decode_level(t.format, t.width, t.height, level)
                    .map_err(|e| e.to_string())?,
            );
        }
        (TextureFormat::Rgba8Unorm, data)
    };
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: t.layers,
        },
        TextureDimension::D2,
        format,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.data = Some(data);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    Ok(image)
}

/// Nearest-texel CPU reads of one BSP's lightmap, decoding BC3 a block at
/// a time.
pub struct Atlas<'a> {
    textures: &'a LightmapTextures,
    scales: [f32; 5],
}

impl<'a> Atlas<'a> {
    pub fn new(textures: &'a LightmapTextures, scales: [f32; 5]) -> Atlas<'a> {
        Atlas { textures, scales }
    }

    fn texel(t: &Texture, layer: u32, uv: [f32; 2]) -> [f32; 4] {
        let x = ((uv[0] * t.width as f32) as i64).clamp(0, t.width as i64 - 1) as u32;
        let y = ((uv[1] * t.height as f32) as i64).clamp(0, t.height as i64 - 1) as u32;
        let Some(level) = t.level(0, layer) else {
            return [0.0; 4];
        };
        let px = if t.format == Format::Bc3 {
            let blocks_wide = t.width.div_ceil(4);
            let o = 16 * ((y / 4) * blocks_wide + x / 4) as usize;
            let Some(block) = level.get(o..o + 16) else {
                return [0.0; 4];
            };
            let Ok(p) = rgba::decode_level(Format::Bc3, 4, 4, block) else {
                return [0.0; 4];
            };
            let i = 4 * ((y % 4) * 4 + x % 4) as usize;
            [p[i], p[i + 1], p[i + 2], p[i + 3]]
        } else {
            // Not seen on any map; decode the whole layer (slow but right).
            let Ok(p) = rgba::decode_level(t.format, t.width, t.height, level) else {
                return [0.0; 4];
            };
            let i = 4 * (y * t.width + x) as usize;
            [p[i], p[i + 1], p[i + 2], p[i + 3]]
        };
        px.map(|b| b as f32 / 255.0)
    }

    /// The lighting stored at one atlas texcoord.
    pub fn light(&self, uv: [f32; 2]) -> ShLight {
        use blam_geometry::lightmap::luvw;
        let sh = &self.textures.coefficients;
        let i = &self.textures.intensity;
        ShLight {
            c: std::array::from_fn(|k| {
                luvw(
                    Self::texel(sh, 2 * k as u32, uv),
                    Self::texel(sh, 2 * k as u32 + 1, uv),
                    self.scales[k],
                )
            }),
            dominant: luvw(Self::texel(i, 0, uv), Self::texel(i, 1, uv), self.scales[4]),
        }
    }
}

/// A sunlit Valhalla texel (`lightmaps.md` 5.4), for scenes with no
/// lightmap at all.
pub fn default_light() -> ShLight {
    ShLight {
        c: [
            Vec3::new(1.067, 0.998, 0.984),
            Vec3::new(-0.448, -0.424, -0.448),
            Vec3::new(0.929, 0.857, 0.857),
            Vec3::new(-0.894, -0.793, -0.760),
        ],
        dominant: Vec3::new(2.735, 2.493, 2.354),
    }
}

/// The scene's [`SurfaceLights`], kept after the world is built so moving
/// things (the rider) can be lit the way objects are.
#[derive(Resource)]
pub struct SceneLights(pub SurfaceLights);

/// Lighting sampled at every per-pixel lit cluster vertex, in a grid, for
/// lighting objects by the nearest lit surface.
pub struct SurfaceLights {
    cell: f32,
    /// Position, light and the BSP mask of the cluster it was sampled on.
    grid: HashMap<(i32, i32, i32), Vec<(Vec3, ShLight, u32)>>,
    /// The mean over all samples, for things far from any.
    mean: ShLight,
}

impl SurfaceLights {
    pub fn build(scene: &Scene, atlases: &[Option<Atlas<'_>>]) -> SurfaceLights {
        let cell = 8.0;
        let mut grid: HashMap<(i32, i32, i32), Vec<(Vec3, ShLight, u32)>> = HashMap::new();
        let mut sum = ShLight::default();
        let mut n = 0usize;
        for p in scene.placements.iter().filter(|p| p.kind == Kind::Cluster) {
            let Lighting::PerPixel { lightmap, uvs } = &p.lighting else {
                continue;
            };
            let Some(Some(atlas)) = atlases.get(*lightmap as usize) else {
                continue;
            };
            let mesh = &scene.meshes[p.mesh as usize];
            for (pos, uv) in mesh.positions.iter().zip(uvs) {
                let w = p.transform.transform_point3(*pos);
                let light = atlas.light(*uv);
                for k in 0..4 {
                    sum.c[k] += light.c[k];
                }
                sum.dominant += light.dominant;
                n += 1;
                grid.entry(Self::key(w, cell)).or_default().push((w, light, p.bsps));
            }
        }
        let mean = if n == 0 {
            default_light()
        } else {
            let s = 1.0 / n as f32;
            ShLight {
                c: sum.c.map(|c| c * s),
                dominant: sum.dominant * s,
            }
        };
        SurfaceLights { cell, grid, mean }
    }

    fn key(p: Vec3, cell: f32) -> (i32, i32, i32) {
        (
            (p.x / cell).floor() as i32,
            (p.y / cell).floor() as i32,
            (p.z / cell).floor() as i32,
        )
    }

    pub fn is_empty(&self) -> bool {
        self.grid.is_empty()
    }

    /// The lighting of the lit surface vertex nearest `p` (Halo units),
    /// searching a few cells out; the map's mean light past that.
    pub fn near(&self, p: Vec3) -> ShLight {
        self.near_in(p, 0)
    }

    /// [`near`](Self::near) over the surfaces of the BSPs in `mask` only
    /// (zero: any), so an object is lit by its own BSP where BSPs overlap
    /// (campaign maps swap versions of one space between zone sets).
    pub fn near_in(&self, p: Vec3, mask: u32) -> ShLight {
        let (cx, cy, cz) = Self::key(p, self.cell);
        let mut best: Option<(f32, ShLight)> = None;
        for r in 0..4i32 {
            for dx in -r..=r {
                for dy in -r..=r {
                    for dz in -r..=r {
                        if dx.abs().max(dy.abs()).max(dz.abs()) != r {
                            continue;
                        }
                        let Some(v) = self.grid.get(&(cx + dx, cy + dy, cz + dz)) else {
                            continue;
                        };
                        for (q, l, bsps) in v {
                            if mask != 0 && bsps & mask == 0 {
                                continue;
                            }
                            let d = q.distance_squared(p);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, *l));
                            }
                        }
                    }
                }
            }
            // Anything found within this ring is as close as can be found
            // one ring further out, give or take a cell.
            if best.is_some() {
                break;
            }
        }
        match best {
            Some(b) => b.1,
            None if mask != 0 => self.near_in(p, 0),
            None => self.mean,
        }
    }
}
