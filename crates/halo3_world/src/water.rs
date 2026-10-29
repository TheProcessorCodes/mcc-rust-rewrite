//! Water (`rmw ` shaders) and flowing waterfalls, drawn by their own
//! material (`shaders/water.wesl`). Layout and evidence:
//! `docs/format/water.md`.
//!
//! - Water: waves from the shader's slope array (16 animated slices of
//!   U8V8 slopes, stepped by `time_warp`, scrolled by the shader's own
//!   functions), a fresnel reflection of the map's sky (the atmosphere's
//!   scattering along the reflected ray, plus a sun glint), murk from the
//!   depth behind the water (the depth prepass) so shallows show the bed and
//!   the banks fade out, the watercolor texture lit by the lightmap, and foam
//!   from the painted foam mask and the wave slopes.
//! - Flowing shaders (an alpha-blended `rmsh` whose maps scroll, Valhalla's
//!   waterfall): `two_detail` albedo with each map's xform moved by its
//!   function, lit by the lightmap.
//!
//! `world.rs` spawns every batch with a `HaloMaterial` and a
//! [`BatchMaterial`](super::BatchMaterial) marker; [`swap_water_materials`]
//! replaces the material of the water and flow batches, keeping the
//! lightmap. The CPU work happens in [`load`] before the window opens.

use std::collections::HashMap;

use bevy::asset::{RenderAssetUsages, embedded_asset};
use bevy::core_pipeline::prepass::DepthPrepass;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, TextureDataOrder, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_geometry::atmosphere::{Atmosphere, AtmosphereSetting};
use blam_texture::shader::{self, AnimationKind, Shader};
use blam_texture::{Blend, Format, Texture, Xform, bitmap};

use super::lighting::HaloMaterial;
use super::BatchMaterial;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterShading>;

const SHADER: &str = "embedded://halo3_world/shaders/water.wesl";

/// Wave texture coordinates per Halo world unit before the shader's own
/// xform: Halo samples the wave arrays in world space (the water mesh's UV
/// spans the whole river and belongs to the painted masks). Picked by eye
/// against the in-game Valhalla river (`docs/format/water.md` 5).
pub const WAVE_UV_PER_WU: f32 = 0.25;

/// Multiplies the decoded slopes (times `wave_height`): with the slopes at
/// face value the river reflects the sky from every ripple and goes milky;
/// in the game the bed shows through calm, lightly rippled water.
pub const WAVE_SLOPE_SCALE: f32 = 0.6;

/// A bitmap and its tiling, plus how fast its offset moves (UV per second).
#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub bitmap: u32,
    pub xform: Xform,
    pub scroll: [f32; 2],
}

/// How one water or flow material is drawn.
#[derive(Clone, Debug)]
pub enum WaterLook {
    Water(Box<WaterParams>),
    /// An alpha-blended `two_detail` shader whose maps scroll: base (the
    /// mask), detail, detail2.
    Flow { maps: [Option<Map>; 3] },
}

/// The parts of a water shader the viewer uses (`docs/format/water.md` 2).
#[derive(Clone, Debug)]
pub struct WaterParams {
    pub slope: Option<Map>,
    pub foam: Option<Map>,
    pub foam_detail: Option<Map>,
    /// `global_shape_texture`: green scales the waves, blue paints foam.
    pub shape: Option<Map>,
    pub colour_map: Option<Map>,
    /// `water_diffuse`, linear.
    pub colour: [f32; 4],
    pub murkiness: f32,
    pub fresnel: f32,
    pub bank_depth: f32,
    pub slope_range: [f32; 2],
    /// `wave_height`: scales the slopes.
    pub wave_height: f32,
    pub detail_scale: f32,
    pub detail_steepness: f32,
    pub damping: f32,
    pub foam_height: f32,
    pub foam_pow: f32,
    pub watercolor_coefficient: f32,
    /// Seconds per cycle of `time_warp` and `time_warp_aux` (0: still).
    pub warp_periods: [f32; 2],
    pub foam_paint: bool,
    pub foam_auto: bool,
    pub bank_alpha: bool,
}

/// Everything the water needs, worked out before the window opens.
#[derive(Default)]
pub struct WaterData {
    /// By scene material index.
    pub looks: HashMap<u32, WaterLook>,
    pub bitmaps: HashMap<u32, Texture>,
    pub atmosphere: Option<AtmosphereSetting>,
}

#[derive(Resource, Default)]
pub struct PendingWater(pub Option<WaterData>);

/// Whether the map has any water; the camera then gets a depth prepass.
#[derive(Resource, Default)]
struct HasWater(bool);

fn linear(c: [f32; 4]) -> [f32; 4] {
    let l = Color::srgba(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), c[3])
        .to_linear();
    [l.red, l.green, l.blue, c[3]]
}

fn map_of(s: &Shader, usage: &str) -> Option<Map> {
    let m = s.map(usage)?;
    Some(Map {
        bitmap: m.bitmap?,
        xform: m.xform,
        scroll: s.scroll(usage),
    })
}

fn scalar(s: &Shader, name: &str, default: f32) -> f32 {
    s.param(name).map_or(default, |v| v[0])
}

fn period(s: &Shader, argument: &str) -> f32 {
    s.animation(argument, AnimationKind::Value)
        .map_or(0.0, |a| a.period)
}

/// What a resolved shader looks like as water, or `None` when it is neither
/// water nor a flowing translucent shader.
pub fn look_of(s: &Shader) -> Option<WaterLook> {
    if s.class == "rmw " {
        let foam = s.option("foam").unwrap_or("none");
        let p = WaterParams {
            slope: map_of(s, "wave_slope_array"),
            foam: map_of(s, "foam_texture").filter(|_| foam != "none"),
            foam_detail: map_of(s, "foam_texture_detail").filter(|_| foam != "none"),
            shape: map_of(s, "global_shape_texture"),
            colour_map: map_of(s, "watercolor_texture").filter(|_| s.option("watercolor") == Some("texture")),
            colour: linear(s.param("water_diffuse").unwrap_or([0.07, 0.11, 0.14, 1.0])),
            murkiness: scalar(s, "water_murkiness", 3.0),
            fresnel: scalar(s, "fresnel_coefficient", 0.1),
            bank_depth: scalar(s, "bankalpha_infuence_depth", 0.2),
            slope_range: [scalar(s, "slope_range_x", 0.5), scalar(s, "slope_range_y", 0.5)],
            wave_height: scalar(s, "wave_height", 0.5),
            detail_scale: scalar(s, "detail_slope_scale_y", 3.0),
            detail_steepness: scalar(s, "detail_slope_steepness", 0.5),
            damping: scalar(s, "wave_visual_damping_distance", 8.0),
            foam_height: scalar(s, "foam_height", 0.5),
            foam_pow: scalar(s, "foam_pow", 2.0),
            watercolor_coefficient: scalar(s, "watercolor_coefficient", 1.0),
            warp_periods: [period(s, "time_warp"), period(s, "time_warp_aux")],
            foam_paint: matches!(foam, "paint" | "both"),
            foam_auto: matches!(foam, "auto" | "both"),
            bank_alpha: s.option("bankalpha").is_some_and(|b| b != "none"),
        };
        return Some(WaterLook::Water(Box::new(p)));
    }
    let flows = s.animations.iter().any(|a| {
        matches!(a.kind, AnimationKind::TranslationX | AnimationKind::TranslationY)
            && matches!(a.argument.as_str(), "detail_map" | "detail_map2")
            && a.rate() != 0.0
    });
    if flows && s.blend == Blend::AlphaBlend && s.option("albedo") == Some("two_detail") {
        return Some(WaterLook::Flow {
            maps: [map_of(s, "base_map"), map_of(s, "detail_map"), map_of(s, "detail_map2")],
        });
    }
    None
}

/// Resolves the water and flowing materials' shaders and decodes their
/// maps. Failures are logged; those materials keep the plain drawing.
pub fn load(cache: &CacheFile, scene: &Scene) -> WaterData {
    let mut out = WaterData::default();
    let resources = match cache.resources() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("water: no resource tables ({e})");
            return out;
        }
    };
    for (i, m) in scene.materials.iter().enumerate() {
        let Some(tag) = m.shader_tag else { continue };
        let Ok(s) = shader::resolve(cache, tag) else { continue };
        let Some(look) = look_of(&s) else { continue };
        let maps: Vec<Map> = match &look {
            WaterLook::Water(p) => [p.slope, p.foam, p.foam_detail, p.shape, p.colour_map]
                .into_iter()
                .flatten()
                .collect(),
            WaterLook::Flow { maps } => maps.iter().flatten().copied().collect(),
        };
        for map in maps {
            if out.bitmaps.contains_key(&map.bitmap) {
                continue;
            }
            match bitmap::decode(cache, &resources, map.bitmap, 0) {
                Ok(t) => {
                    out.bitmaps.insert(map.bitmap, t);
                }
                Err(e) => eprintln!("water bitmap #{} (for {}): {e}", map.bitmap, s.name),
            }
        }
        let (mut lo, mut hi, mut count) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY), 0);
        for (_, t) in scene.world_triangles().filter(|(m, _)| *m == i as u32) {
            for p in t {
                lo = lo.min(p);
                hi = hi.max(p);
            }
            count += 1;
        }
        eprintln!(
            "water: {} drawn as {}: {count} triangles, halo bounds {lo:.1} .. {hi:.1}",
            s.name,
            if matches!(look, WaterLook::Water(_)) { "water" } else { "flowing" }
        );
        out.looks.insert(i as u32, look);
    }
    if !out.looks.is_empty() {
        match blam_geometry::atmosphere::read(cache) {
            Ok(a) => out.atmosphere = a.as_ref().and_then(Atmosphere::sky).cloned(),
            Err(e) => eprintln!("water: atmosphere: {e}; plain sky reflection"),
        }
    }
    resources.clear_page_cache();
    out
}

/// The water half of [`WaterMaterial`]. In flow mode `map_a`, `map_b`,
/// `map_c` are the base, detail and detail2 maps; in water mode the foam,
/// the shape mask and the watercolor texture.
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct WaterShading {
    /// x: murkiness, y: fresnel f0, z: bank depth (wu, 0 = off), w: exposure.
    #[uniform(100)]
    pub params0: Vec4,
    /// x, y: slope range, z: detail steepness, w: damping distance (wu).
    #[uniform(100)]
    pub params1: Vec4,
    /// x: foam height, y: foam pow, z: watercolor coefficient, w: mode
    /// (0 water, 1 flow).
    #[uniform(100)]
    pub params2: Vec4,
    /// x, y: time_warp periods, z: slices in the slope array, w: flags
    /// (1 slope, 2 foam, 4 shape, 8 colour map, 16 foam paint, 32 foam auto,
    /// 64 lightmapped, 128 atmosphere).
    #[uniform(100)]
    pub params3: Vec4,
    /// `water_diffuse`, linear.
    #[uniform(100)]
    pub colour: Vec4,
    /// Wave slope xform (world units already folded in) and its scroll (xy)
    /// plus the detail scale (z).
    #[uniform(100)]
    pub slope_xform: Vec4,
    #[uniform(100)]
    pub slope_scroll: Vec4,
    /// map_a, map_b, map_c xforms (scale xy, offset zw) and scrolls (xy).
    #[uniform(100)]
    pub xform_a: Vec4,
    #[uniform(100)]
    pub scroll_a: Vec4,
    #[uniform(100)]
    pub xform_b: Vec4,
    #[uniform(100)]
    pub scroll_b: Vec4,
    #[uniform(100)]
    pub xform_c: Vec4,
    #[uniform(100)]
    pub scroll_c: Vec4,
    /// Foam detail xform and scroll (water mode; sampled from map_a).
    #[uniform(100)]
    pub xform_d: Vec4,
    #[uniform(100)]
    pub scroll_d: Vec4,
    /// The sky's atmosphere, as in `sky.rs`.
    #[uniform(100)]
    pub sun_dir: Vec4,
    #[uniform(100)]
    pub sun_colour: Vec4,
    #[uniform(100)]
    pub beta_m: Vec4,
    #[uniform(100)]
    pub beta_p: Vec4,
    #[uniform(100)]
    pub beta_m_theta: Vec4,
    #[uniform(100)]
    pub beta_p_theta: Vec4,
    #[uniform(100)]
    pub heights: Vec4,
    /// x: max fog thickness.
    #[uniform(100)]
    pub atmosphere_extra: Vec4,
    /// The lightmap's c0..c3 ranges and (x) the dominant light's range.
    #[uniform(100)]
    pub lightmap_scales: Vec4,
    #[uniform(100)]
    pub lightmap_extra: Vec4,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub slope: Option<Handle<Image>>,
    #[texture(103)]
    #[sampler(104)]
    pub map_a: Option<Handle<Image>>,
    #[texture(105)]
    pub map_b: Option<Handle<Image>>,
    #[texture(106)]
    pub map_c: Option<Handle<Image>>,
    #[texture(107, dimension = "2d_array")]
    #[sampler(108)]
    pub coefficients: Option<Handle<Image>>,
    #[texture(109, dimension = "2d_array")]
    pub intensity: Option<Handle<Image>>,
}

impl MaterialExtension for WaterShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

pub struct WaterPlugin;

impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default())
            .init_resource::<PendingWater>()
            .init_resource::<HasWater>()
            .add_systems(Update, (swap_water_materials, add_depth_prepass).chain());
    }
}

/// A U8V8 slope array as a GPU `Rg8Snorm` 2D array with every level.
fn slope_image(t: &Texture) -> Result<Image, String> {
    if t.format != Format::U8V8 {
        return Err(format!("{}: {:?}, not U8V8", t.name, t.format));
    }
    let mut data = Vec::with_capacity(t.byte_len());
    for m in &t.mips {
        data.extend_from_slice(m);
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: t.layers.max(1),
        },
        TextureDimension::D2,
        TextureFormat::Rg8Snorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = t.mips.len() as u32;
    // Our levels hold every layer one after another.
    image.data_order = TextureDataOrder::MipMajor;
    image.data = Some(data);
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    Ok(image)
}

fn xform_vec(m: Option<&Map>) -> Vec4 {
    m.map_or(Vec4::new(1.0, 1.0, 0.0, 0.0), |m| {
        Vec4::new(m.xform.scale[0], m.xform.scale[1], m.xform.offset[0], m.xform.offset[1])
    })
}

fn scroll_vec(m: Option<&Map>) -> Vec4 {
    m.map_or(Vec4::ZERO, |m| Vec4::new(m.scroll[0], m.scroll[1], 0.0, 0.0))
}

fn v3(c: [f32; 3]) -> Vec4 {
    Vec4::new(c[0], c[1], c[2], 0.0)
}

fn set_atmosphere(s: &mut WaterShading, a: &AtmosphereSetting) {
    s.sun_dir = a.sun_direction().extend(a.mie_g);
    s.sun_colour = v3(a.sun_colour) * a.sun_intensity;
    s.beta_m = v3(a.beta_m);
    s.beta_p = v3(a.beta_p);
    s.beta_m_theta = v3(a.beta_m_theta);
    s.beta_p_theta = v3(a.beta_p_theta);
    s.heights = Vec4::new(a.sea_level, a.rayleigh_height, a.mie_height, a.distance_bias);
    s.atmosphere_extra = Vec4::new(a.max_fog_thickness.max(1.0), 0.0, 0.0, 0.0);
}

#[allow(clippy::too_many_arguments)]
/// Water materials already made, by the world material they replace: the
/// world's chunks of one material share it, and so do their water
/// materials. Held weakly, so a zone set's water goes with its BSPs.
#[derive(Default)]
struct WaterCache {
    materials: HashMap<AssetId<HaloMaterial>, AssetId<WaterMaterial>>,
    images: HashMap<(u32, bool), Option<AssetId<Image>>>,
}

#[allow(clippy::too_many_arguments)]
fn swap_water_materials(
    mut commands: Commands,
    pending: Res<PendingWater>,
    mut has_water: ResMut<HasWater>,
    batches: Query<(Entity, &BatchMaterial, &MeshMaterial3d<HaloMaterial>), Added<BatchMaterial>>,
    halo: Res<Assets<HaloMaterial>>,
    mut water_materials: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    compressed: Option<Res<bevy::image::CompressedImageFormatSupport>>,
    mut cache: Local<WaterCache>,
) {
    if batches.is_empty() {
        return;
    }
    // Kept for the zone sets streamed in later.
    let Some(data) = pending.0.as_ref() else {
        return;
    };
    if data.looks.is_empty() {
        return;
    }
    let bc = compressed.is_some_and(|c| c.0.supports(TextureFormat::Bc3RgbaUnormSrgb));
    let exposure = super::lighting::exposure();
    let cache = &mut *cache;
    let handles = &mut cache.images;
    let mut image = |m: Option<&Map>, srgb: bool, array: bool| -> Option<Handle<Image>> {
        let m = m?;
        match handles.get(&(m.bitmap, srgb)) {
            Some(None) => return None,
            Some(Some(id)) => {
                if let Some(h) = images.get_strong_handle(*id) {
                    return Some(h);
                }
            }
            None => {}
        }
        let t = data.bitmaps.get(&m.bitmap)?;
        let made = if array { slope_image(t) } else { super::texture_image_as(t, bc, srgb) };
        match made {
            Ok(i) => {
                let h = images.add(i);
                handles.insert((m.bitmap, srgb), Some(h.id()));
                Some(h)
            }
            Err(e) => {
                warn!("water texture {}: {e}", t.name);
                handles.insert((m.bitmap, srgb), None);
                None
            }
        }
    };
    let mut swapped = (0, 0);
    for (entity, batch, handle) in &batches {
        let Some(look) = data.looks.get(&batch.0) else {
            continue;
        };
        if let Some(h) = cache
            .materials
            .get(&handle.0.id())
            .and_then(|id| water_materials.get_strong_handle(*id))
        {
            commands
                .entity(entity)
                .remove::<MeshMaterial3d<HaloMaterial>>()
                .insert(MeshMaterial3d(h));
            continue;
        }
        let Some(old) = halo.get(&handle.0) else {
            continue;
        };
        let lit = old.extension.coefficients.is_some() && old.extension.intensity.is_some();
        let mut s = WaterShading {
            lightmap_scales: old.extension.scales,
            lightmap_extra: old.extension.extra,
            coefficients: old.extension.coefficients.clone(),
            intensity: old.extension.intensity.clone(),
            ..default()
        };
        let mut flags = if lit { 64 } else { 0 };
        match look {
            WaterLook::Water(p) => {
                s.slope = image(p.slope.as_ref(), false, true);
                s.map_a = image(p.foam.as_ref(), true, false);
                s.map_b = image(p.shape.as_ref(), false, false);
                s.map_c = image(p.colour_map.as_ref(), true, false);
                let layers = p
                    .slope
                    .and_then(|m| data.bitmaps.get(&m.bitmap))
                    .map_or(1, |t| t.layers.max(1));
                flags |= (s.slope.is_some() as u32)
                    | (s.map_a.is_some() as u32) << 1
                    | (s.map_b.is_some() as u32) << 2
                    | (s.map_c.is_some() as u32) << 3
                    | (p.foam_paint as u32) << 4
                    | (p.foam_auto as u32) << 5;
                s.params0 = Vec4::new(
                    p.murkiness,
                    p.fresnel,
                    if p.bank_alpha { p.bank_depth } else { 0.0 },
                    exposure,
                );
                let k = p.wave_height * WAVE_SLOPE_SCALE;
                s.params1 = Vec4::new(p.slope_range[0] * k, p.slope_range[1] * k, p.detail_steepness, p.damping);
                s.params2 = Vec4::new(p.foam_height, p.foam_pow, p.watercolor_coefficient, 0.0);
                s.colour = Vec4::from_array(p.colour);
                // World units to wave UV, then the shader's own tiling.
                let sx = xform_vec(p.slope.as_ref());
                s.slope_xform = Vec4::new(sx.x * WAVE_UV_PER_WU, sx.y * WAVE_UV_PER_WU, sx.z, sx.w);
                s.slope_scroll = scroll_vec(p.slope.as_ref()) + Vec4::new(0.0, 0.0, p.detail_scale, 0.0);
                let wave = |m: Option<&Map>| {
                    let x = xform_vec(m);
                    Vec4::new(x.x * WAVE_UV_PER_WU, x.y * WAVE_UV_PER_WU, x.z, x.w)
                };
                s.xform_a = wave(p.foam.as_ref());
                s.scroll_a = scroll_vec(p.foam.as_ref());
                s.xform_d = wave(p.foam_detail.as_ref());
                s.scroll_d = scroll_vec(p.foam_detail.as_ref());
                // The shape and colour maps follow the mesh's own UV.
                s.xform_b = xform_vec(p.shape.as_ref());
                s.xform_c = xform_vec(p.colour_map.as_ref());
                s.params3 = Vec4::new(p.warp_periods[0], p.warp_periods[1], layers as f32, 0.0);
                if let Some(a) = &data.atmosphere {
                    set_atmosphere(&mut s, a);
                    flags |= 128;
                }
                swapped.0 += 1;
            }
            WaterLook::Flow { maps } => {
                s.map_a = image(maps[0].as_ref(), true, false);
                s.map_b = image(maps[1].as_ref(), true, false);
                s.map_c = image(maps[2].as_ref(), true, false);
                s.xform_a = xform_vec(maps[0].as_ref());
                s.scroll_a = scroll_vec(maps[0].as_ref());
                s.xform_b = xform_vec(maps[1].as_ref());
                s.scroll_b = scroll_vec(maps[1].as_ref());
                s.xform_c = xform_vec(maps[2].as_ref());
                s.scroll_c = scroll_vec(maps[2].as_ref());
                s.params0.w = exposure;
                s.params2.w = 1.0;
                flags |= (s.map_a.is_some() as u32) << 1
                    | (s.map_b.is_some() as u32) << 2
                    | (s.map_c.is_some() as u32) << 3;
                swapped.1 += 1;
            }
        }
        // `HALO3_WATER_OFF=<bits>` turns features off for comparisons (the
        // flag bits of `params3.w`).
        let mut off: u32 = std::env::var("HALO3_WATER_OFF")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        // The painted masks (shape, watercolor, foam paint) are laid out on
        // a texcoord the water mesh's UV is not (water.md 4): sampled at the
        // mesh UV they come out in triangle-shaped patches. Off unless
        // `HALO3_WATER_MASKS=1`.
        if std::env::var("HALO3_WATER_MASKS").map_or(true, |v| v != "1") && matches!(look, WaterLook::Water(_)) {
            off |= 4 | 8 | 16;
        }
        s.params3.w = (flags & !off) as f32;
        let material = WaterMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: None,
                alpha_mode: AlphaMode::Blend,
                ..old.base.clone()
            },
            extension: s,
        };
        let h = water_materials.add(material);
        cache.materials.insert(handle.0.id(), h.id());
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<HaloMaterial>>()
            .insert(MeshMaterial3d(h));
    }
    if swapped.0 > 0 && !has_water.0 {
        has_water.0 = true;
    }
    if swapped != (0, 0) {
        info!("water: {} water and {} flowing materials", swapped.0, swapped.1);
    }
}

/// The water reads the depth behind it for murk and shore fade.
fn add_depth_prepass(
    mut commands: Commands,
    has_water: Res<HasWater>,
    cameras: Query<Entity, (With<Camera3d>, Without<DepthPrepass>)>,
) {
    if !has_water.0 {
        return;
    }
    for camera in &cameras {
        commands.entity(camera).insert(DepthPrepass);
    }
}
