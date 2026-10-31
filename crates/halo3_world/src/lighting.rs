//! Halo 3's baked lighting in the viewer, in place of bevy's lights.
//!
//! - Per-pixel lightmapped placements (every lit BSP cluster, most
//!   instances) keep their lightmap texcoord as `UV_1`; the fragment shader
//!   `shaders/lightmap.wesl` decodes the BSP's two DXT5 arrays there with
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

use halo3_textures::LightmapTextures;

/// Multiplies Halo's irradiance before bevy's tonemapping on Valhalla,
/// picked by comparing screenshots with the game (`docs/viewer.md`).
pub const EXPOSURE: f32 = 4.0;

/// Valhalla's camera fx exposure in stops, the one [`EXPOSURE`] was picked
/// on: other maps get `EXPOSURE * 2^(stops - VALHALLA_STOPS)`.
pub const VALHALLA_STOPS: f32 = -4.8365;

/// The map's own exposure in stops (`cfxs +0x18`), set once at startup.
static MAP_STOPS: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

/// Records the map's camera fx exposure (`blam_geometry::camera_fx`).
/// Values outside -8..0 stops are ignored (Chillout stores -16.6, which
/// reads as "auto exposure only"; [`EXPOSURE`] is used there).
pub fn set_map_exposure(stops: f32) {
    if (-8.0..=0.0).contains(&stops) {
        let _ = MAP_STOPS.set(stops);
    }
}

/// How much of the camera fx's difference from Valhalla is applied. The
/// full difference (1.0) over-exposes the maps with the least negative
/// stops (Avalanche x17, Standoff x20): Halo 3 also adapts its exposure to
/// the view, which the viewer does not, so the stored value alone overshoots.
/// Picked by eye over the 27 multiplayer maps; `HALO3_EXPOSURE_SLOPE`
/// overrides.
pub const EXPOSURE_SLOPE: f32 = 0.6;

/// The most the mean lit-surface irradiance (luminance) may come out at
/// after exposure. Valhalla sits at 1.2 (0.30 x 4), the rest of the 27
/// multiplayer maps at 0.2-2.0; only maps whose lightmaps are far brighter
/// than their camera fx allows for are held back: Sandbox (mean 1.23, else
/// x7.5) and Sandtrap (1.15, else x4.3). A stand-in for Halo's auto
/// exposure.
pub const MAX_EXPOSED_MEAN: f32 = 3.0;

/// The map's mean lit-surface irradiance luminance, set once at startup.
static SCENE_MEAN: std::sync::OnceLock<f32> = std::sync::OnceLock::new();

/// Records the map's mean irradiance ([`scene_mean_irradiance`]).
pub fn set_scene_mean(mean: f32) {
    if mean.is_finite() && mean > 0.0 {
        let _ = SCENE_MEAN.set(mean);
    }
}

/// The mean luminance of the diffuse irradiance over every 25th vertex of
/// the per-pixel lit BSP clusters, each with its own normal (what `blam
/// lightmap` prints as "mean diffuse irradiance"), or `None` without a
/// lightmap.
pub fn scene_mean_irradiance(scene: &Scene, lightmaps: &[Option<LightmapTextures>]) -> Option<f32> {
    let w = Vec3::new(0.212656, 0.715158, 0.0721856);
    let (mut sum, mut n) = (0.0f32, 0usize);
    for p in scene.placements.iter().filter(|p| p.kind == Kind::Cluster) {
        let Lighting::PerPixel { lightmap, uvs } = &p.lighting else {
            continue;
        };
        let (Some(Some(tex)), Some(lm)) = (lightmaps.get(*lightmap as usize), scene.lightmaps.get(*lightmap as usize))
        else {
            continue;
        };
        let atlas = Atlas::new(tex, lm.scales);
        let mesh = &scene.meshes[p.mesh as usize];
        let normal_m = Mat3::from_mat4(p.transform).inverse().transpose();
        for (i, uv) in uvs.iter().enumerate().step_by(25) {
            let Some(normal) = mesh.normals.get(i) else { break };
            let e = atlas.light(*uv).irradiance((normal_m * *normal).normalize_or(Vec3::Z));
            sum += e.max(Vec3::ZERO).dot(w);
            n += 1;
        }
    }
    (n > 0).then(|| sum / n as f32)
}

/// The camera fx's auto-exposure range (`+0x10`, `+0x14`), set once.
static AUTO_RANGE: std::sync::OnceLock<[f32; 2]> = std::sync::OnceLock::new();

/// Records the map's auto-exposure range in stops (`camera_fx`).
pub fn set_auto_range(range: [f32; 2]) {
    if range.iter().all(|r| r.is_finite()) {
        let _ = AUTO_RANGE.set(range);
    }
}

/// The most the auto exposure brightens a dark place, in stops: the width
/// of the camera fx's auto range (Valhalla 0..3; 120_halo 0..2), at most
/// [`AUTO_MAX_STOPS`].
fn auto_max_stops() -> f32 {
    AUTO_RANGE.get().map_or(2.0, |r| (r[1] - r[0]).max(0.0)).min(AUTO_MAX_STOPS)
}

/// Cap on [`auto_max_stops`].
pub const AUTO_MAX_STOPS: f32 = 3.0;

/// How much darker than the map's mean (stops) a place may be before the
/// auto exposure starts brightening: shade outdoors (Valhalla's first spawn,
/// under a base's overhang) is part of the look; only real interiors (120_halo's
/// start, 6.7 stops under the mean; Valhalla's first spawn is 3.6) are brightened, by what lies beyond this.
pub const AUTO_THRESHOLD: f32 = 4.0;

/// A stand-in for Halo's auto exposure: in a place much darker than the
/// map's mean lit surface ([`AUTO_THRESHOLD`]), the whole picture is
/// brightened by up to the camera
/// fx's auto range (through bevy's colour grading, so the world, the sky,
/// the haze and the rider move together). The place's light is the lit
/// surface nearest the camera (`SurfaceLights`, ground first), averaged
/// over six directions. Adapts over about a second; snaps when the camera
/// jumps. Never darkens: [`exposure`] already holds the bright maps back.
/// `HALO3_AUTO_EXPOSURE=0` turns it off.
pub fn auto_exposure(
    mut commands: Commands,
    time: Res<Time>,
    lights: Option<Res<SceneLights>>,
    loaded: Option<Res<super::zones::LoadedBsps>>,
    mut cameras: Query<(Entity, &GlobalTransform, Option<&mut bevy::render::view::ColorGrading>), With<halo3_core::fly_camera::FlyCamera>>,
    mut state: Local<Option<(f32, Vec3)>>,
) {
    static OFF: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("HALO3_AUTO_EXPOSURE").is_ok_and(|v| v == "0"));
    let (Some(lights), Some(mean)) = (lights, SCENE_MEAN.get()) else {
        return;
    };
    if *OFF || lights.0.is_empty() {
        return;
    }
    let Ok((entity, transform, grading)) = cameras.single_mut() else {
        return;
    };
    let eye = super::to_halo(transform.translation());
    let mask = loaded.map_or(0, |l| l.shown);
    let light = lights.0.near_in(eye, mask);
    let w = Vec3::new(0.212656, 0.715158, 0.0721856);
    let local = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z]
        .iter()
        .map(|n| light.irradiance(*n).max(Vec3::ZERO).dot(w))
        .sum::<f32>()
        / 6.0;
    let target = ((mean / local.max(1e-4)).log2() - AUTO_THRESHOLD).clamp(0.0, auto_max_stops());
    let stops = match *state {
        Some((s, at)) if at.distance(eye) < 20.0 => s + (target - s) * (1.0 - (-time.delta_secs() / 0.7).exp()),
        _ => target,
    };
    if state.is_none_or(|(s, _)| (s * 4.0).round() != (stops * 4.0).round()) {
        debug!("auto exposure: local light {local:.3} against mean {mean:.3}: +{stops:.2} stops");
    }
    *state = Some((stops, eye));
    match grading {
        Some(mut g) => {
            if (g.global.exposure - stops).abs() > 1e-3 {
                g.global.exposure = stops;
            }
        }
        None => {
            let mut g = bevy::render::view::ColorGrading::default();
            g.global.exposure = stops;
            commands.entity(entity).insert(g);
        }
    }
}

/// The scale from Halo's light to bevy's tonemapper input:
/// `HALO3_EXPOSURE` when set, else `EXPOSURE * 2^(slope * (stops -
/// VALHALLA_STOPS))` from the map's camera fx (else [`EXPOSURE`]), held so
/// that the map's mean irradiance stays under [`MAX_EXPOSED_MEAN`].
pub fn exposure() -> f32 {
    let env = |name: &str| std::env::var(name).ok().and_then(|s| s.parse::<f32>().ok());
    env("HALO3_EXPOSURE").unwrap_or_else(|| {
        let e = match MAP_STOPS.get() {
            Some(stops) => {
                let slope = env("HALO3_EXPOSURE_SLOPE").unwrap_or(EXPOSURE_SLOPE);
                EXPOSURE * (slope * (stops - VALHALLA_STOPS)).exp2()
            }
            None => EXPOSURE,
        };
        match SCENE_MEAN.get() {
            Some(mean) => e.min(MAX_EXPOSED_MEAN / mean),
            None => e,
        }
    })
}

pub type HaloMaterial = ExtendedMaterial<StandardMaterial, HaloLightmap>;

const SHADER: &str = "embedded://halo3_world/shaders/lightmap.wesl";

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

    // The material model (`docs/format/shaders.md` section 11); the
    // uniform fields mirror `HaloLightmapParams` in the shader.
    /// x: bump map on, y: bump detail coefficient (0: none), z: albedo
    /// detail map on, w: specular mask source (0 none, 1 base alpha, 2 mask
    /// texture).
    #[uniform(100)]
    pub surface: Vec4,
    /// 1 where a normal map is BC5 signed: x bump, y bump detail.
    #[uniform(100)]
    pub signed: Vec4,
    #[uniform(100)]
    pub bump_xform: Vec4,
    #[uniform(100)]
    pub bump_detail_xform: Vec4,
    #[uniform(100)]
    pub detail_xform: Vec4,
    #[uniform(100)]
    pub mask_xform: Vec4,
    #[uniform(100)]
    pub illum_xform: Vec4,
    #[uniform(100)]
    pub illum_detail_xform: Vec4,
    /// Specular coefficient, normal power, glancing power, fresnel
    /// steepness.
    #[uniform(100)]
    pub spec: Vec4,
    /// Normal specular tint; w: analytical contribution.
    #[uniform(100)]
    pub tint_normal: Vec4,
    /// Glancing specular tint; w: area contribution.
    #[uniform(100)]
    pub tint_glancing: Vec4,
    /// F0; w: model (0 diffuse, 1 phong, 2 cook-torrance, 3 glass).
    #[uniform(100)]
    pub f0: Vec4,
    /// Reflection colour: environment tint times contribution.
    #[uniform(100)]
    pub env: Vec4,
    /// x: self-illumination mode (0 off, 1 map, 2 map x detail, 3 from
    /// diffuse, 4 map x diffuse, 5 three channels, 6 the map's alpha: plasma);
    /// y: plasma only, 1 when the mask is read from red instead of alpha.
    #[uniform(100)]
    pub illum: Vec4,
    #[uniform(100)]
    pub illum_a: Vec4,
    #[uniform(100)]
    pub illum_b: Vec4,
    #[uniform(100)]
    pub illum_c: Vec4,
    /// Terrain: 1 per layer with a bump map, and whether it is signed.
    #[uniform(100)]
    pub terrain_bump: Vec4,
    #[uniform(100)]
    pub terrain_bump_signed: Vec4,
    #[uniform(100)]
    pub terrain_bump_xforms: [Vec4; 4],
    /// Terrain: specular coefficient (0 for diffuse-only layers) and power
    /// per layer.
    #[uniform(100)]
    pub terrain_spec: Vec4,
    #[uniform(100)]
    pub terrain_power: Vec4,
    /// Terrain: per layer specular tint, analytical contribution in w.
    #[uniform(100)]
    pub terrain_tints: [Vec4; 4],
    /// x: 1 when an `alpha_test_map` gives the alpha (not the base map);
    /// the map's xform after it.
    #[uniform(100)]
    pub alpha_flags: Vec4,
    #[uniform(100)]
    pub alpha_xform: Vec4,

    /// A 1x1 image that only carries the repeating, anisotropic sampler
    /// every map below is read with.
    #[texture(115)]
    #[sampler(116)]
    pub surface_dummy: Option<Handle<Image>>,
    #[texture(117)]
    pub bump_map: Option<Handle<Image>>,
    #[texture(118)]
    pub bump_detail_map: Option<Handle<Image>>,
    #[texture(119)]
    pub detail_map: Option<Handle<Image>>,
    #[texture(120)]
    pub mask_map: Option<Handle<Image>>,
    #[texture(121)]
    pub illum_map: Option<Handle<Image>>,
    #[texture(122)]
    pub illum_detail_map: Option<Handle<Image>>,
    #[texture(123)]
    pub terrain_bump_0: Option<Handle<Image>>,
    #[texture(124)]
    pub terrain_bump_1: Option<Handle<Image>>,
    #[texture(125)]
    pub terrain_bump_2: Option<Handle<Image>>,
    #[texture(126)]
    pub terrain_bump_3: Option<Handle<Image>>,
    #[texture(127)]
    pub alpha_map: Option<Handle<Image>>,
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

    /// The depth prepass (maps with water) must cut `alpha_test_map`
    /// cut-outs too, or their whole cards hide what is behind them (Last
    /// Resort's palm fronds drew as white haze-coloured squares).
    fn prepass_fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub struct LightingPlugin;

impl Plugin for LightingPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/lightmap.wesl");
        app.add_plugins(MaterialPlugin::<HaloMaterial>::default())
            .add_systems(Update, auto_exposure.run_if(halo3_core::map::ready));
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
        } else if t.format == Format::A8R8G8B8 {
            // Halo 2's lightmaps (`halo3::halo2`): bytes B G R A.
            let i = 4 * (y * t.width + x) as usize;
            let Some(p) = level.get(i..i + 4) else {
                return [0.0; 4];
            };
            [p[2], p[1], p[0], p[3]]
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
pub struct SceneLights(pub std::sync::Arc<SurfaceLights>);

/// A lit surface point: position, light, its BSP mask, and the height of
/// its normal (how much it faces up).
type Sample = (Vec3, ShLight, u32, f32);

/// Lighting sampled at the vertices of every per-pixel lit cluster and
/// instance, in a grid, for lighting objects and the rider by the lit
/// surface under them.
pub struct SurfaceLights {
    cell: f32,
    grid: HashMap<(i32, i32, i32), Vec<Sample>>,
    /// The mean over all samples, for things far from any.
    mean: ShLight,
}

/// Whether a lightmap texel holds any light: texels outside every chart
/// (and the unbaked backs of things) decode to zero, which drew the rider
/// black on 120_halo's Warthog run.
fn holds_light(l: &ShLight) -> bool {
    l.c[0].max_element() > 1e-4 || l.dominant.max_element() > 1e-4
}

/// Spacing of the light samples on big triangles, world units.
const SAMPLE_SPACING: f32 = 3.0;

/// The samples of one placement's per-pixel lit triangles: inside each
/// triangle, never on its corners (a vertex's texcoord sits on the edge of
/// its lightmap chart, where the texel is often the dark padding around the
/// chart: the black rider), a grid about [`SAMPLE_SPACING`] apart on big
/// triangles so the nearest sample is near the surface under a point.
fn sample_placement(scene: &Scene, p: &blam_geometry::Placement, atlas: &Atlas<'_>, uvs: &[[f32; 2]], out: &mut Vec<Sample>) {
    let mesh = &scene.meshes[p.mesh as usize];
    // Instances repeat one mesh many times: every other triangle is plenty.
    let step = if p.kind == Kind::Instance { 2 } else { 1 };
    let triangles = mesh.parts.iter().flat_map(|part| part.indices.as_chunks::<3>().0.iter());
    for t in triangles.step_by(step) {
        let [a, b, c] = t.map(|i| i as usize);
        let (Some(ua), Some(ub), Some(uc)) = (uvs.get(a), uvs.get(b), uvs.get(c)) else {
            continue;
        };
        let [pa, pb, pc] = [a, b, c].map(|i| p.transform.transform_point3(mesh.positions[i]));
        let mut normal = (pb - pa).cross(pc - pa).normalize_or_zero();
        if p.mirrored {
            normal = -normal;
        }
        let longest = pa.distance(pb).max(pb.distance(pc)).max(pc.distance(pa));
        let k = (longest / SAMPLE_SPACING).ceil().clamp(1.0, 12.0) as u32;
        // The centres of the k x k sub-triangles, in barycentric
        // coordinates (u, v), w = 1 - u - v: the upright ones, then the
        // upside-down ones between them.
        for i in 0..k {
            for j in 0..k - i {
                for (du, dv) in [(1.0 / 3.0, 1.0 / 3.0), (2.0 / 3.0, 2.0 / 3.0)] {
                    if du > 0.5 && i + j + 1 >= k {
                        continue;
                    }
                    let u = (i as f32 + du) / k as f32;
                    let v = (j as f32 + dv) / k as f32;
                    let w0 = 1.0 - u - v;
                    let uv = [ua[0] * w0 + ub[0] * u + uc[0] * v, ua[1] * w0 + ub[1] * u + uc[1] * v];
                    let light = atlas.light(uv);
                    if holds_light(&light) {
                        out.push((pa * w0 + pb * u + pc * v, light, p.bsps, normal.z));
                    }
                }
            }
        }
    }
}

impl SurfaceLights {
    pub fn build(scene: &Scene, atlases: &[Option<Atlas<'_>>]) -> SurfaceLights {
        let cell = 8.0;
        let lit: Vec<(&blam_geometry::Placement, &Atlas<'_>, &[[f32; 2]])> = scene
            .placements
            .iter()
            .filter(|p| matches!(p.kind, Kind::Cluster | Kind::Instance))
            .filter_map(|p| {
                let Lighting::PerPixel { lightmap, uvs } = &p.lighting else {
                    return None;
                };
                Some((p, atlases.get(*lightmap as usize)?.as_ref()?, uvs.as_slice()))
            })
            .collect();
        // Spread over the cores: every sample decodes ten BC3 blocks.
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
        let samples: Vec<Sample> = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..threads)
                .map(|k| {
                    let lit = &lit;
                    s.spawn(move || {
                        let mut out = Vec::new();
                        for (p, atlas, uvs) in lit.iter().skip(k).step_by(threads) {
                            sample_placement(scene, p, atlas, uvs, &mut out);
                        }
                        out
                    })
                })
                .collect();
            jobs.into_iter().flat_map(|j| j.join().unwrap_or_default()).collect()
        });
        let mut grid: HashMap<(i32, i32, i32), Vec<Sample>> = HashMap::new();
        let mut sum = ShLight::default();
        for sample in &samples {
            for k in 0..4 {
                sum.c[k] += sample.1.c[k];
            }
            sum.dominant += sample.1.dominant;
            grid.entry(Self::key(sample.0, cell)).or_default().push(*sample);
        }
        let mean = if samples.is_empty() {
            default_light()
        } else {
            let s = 1.0 / samples.len() as f32;
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

    /// The lighting of the lit surface nearest `p` (Halo units), searching
    /// a few cells out (the map's mean light past that), over the surfaces
    /// of the BSPs in `mask` only (zero: any), so an object is lit by its own BSP where BSPs overlap
    /// (campaign maps swap versions of one space between zone sets).
    /// Surfaces under `p` facing up count as nearer than walls and
    /// ceilings: an object stands on the ground it is lit like.
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
                        for (q, l, bsps, up) in v {
                            if mask != 0 && bsps & mask == 0 {
                                continue;
                            }
                            // Ground under the point: as is. A wall, a
                            // ceiling or the underside of a ledge: as if 3
                            // world units further.
                            let ground = *up > 0.5 && q.z <= p.z + 0.5;
                            let d = q.distance_squared(p) + if ground { 0.0 } else { 9.0 };
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
