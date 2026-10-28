//! The sky and the distance haze, from the map's atmosphere
//! (`docs/format/sky-and-atmosphere.md`).
//!
//! - Sky placements: `world.rs` spawns them with a plain StandardMaterial
//!   and a [`SkyBatch`] marker; [`swap_sky_materials`] replaces that with a
//!   [`SkyMaterial`] (fragment shader `world/shaders/sky.wesl`) that combines
//!   the shader's base and detail maps the way its `albedo` option says, adds
//!   self-illumination, and runs Halo 3's light scattering from the camera
//!   to the surface. The near-black sky dome turns blue that way.
//! - The world: a bevy [`DistanceFog`] on the camera, atmospheric falloff,
//!   with the level's extinction and in-scattered colour, recomputed each
//!   frame for the camera's height (the haze thins with height).
//!
//! CPU work (shader resolve, bitmap decode, the atmosphere tag) happens in
//! [`load`] before the window opens, like `textures.rs`.

use std::collections::HashMap;

use bevy::asset::embedded_asset;
use bevy::pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

use blam_cache::CacheFile;
use blam_geometry::atmosphere::{Atmosphere, AtmosphereSetting};
use blam_geometry::{Kind, Scene, WORLD_UNIT_METRES};
use blam_texture::shader::{self, Shader};
use blam_texture::{Blend, Texture, Xform, bitmap};

use super::lighting;

pub type SkyMaterial = ExtendedMaterial<StandardMaterial, SkyShading>;

const SHADER: &str = "embedded://halo3/world/shaders/sky.wesl";

/// How a sky shader's `albedo` option combines its maps (the numbers the
/// fragment shader switches on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Albedo {
    ConstantColor = 0,
    Default = 1,
    DetailBlend = 2,
    ThreeDetailBlend = 3,
    TwoDetailOverlay = 4,
    TwoDetail = 5,
    TwoDetailBlackPoint = 6,
}

impl Albedo {
    fn of(option: Option<&str>) -> Albedo {
        match option.unwrap_or("default") {
            "constant_color" => Albedo::ConstantColor,
            "detail_blend" => Albedo::DetailBlend,
            "three_detail_blend" => Albedo::ThreeDetailBlend,
            "two_detail_overlay" => Albedo::TwoDetailOverlay,
            "two_detail" => Albedo::TwoDetail,
            "two_detail_black_point" => Albedo::TwoDetailBlackPoint,
            _ => Albedo::Default,
        }
    }
}

/// A sky shader's self-illumination, as the fragment shader switches on it
/// (`illum_colour.w`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IllumMode {
    Off = 0,
    /// The self-illumination map's colour.
    Map = 1,
    /// `from_diffuse`: the albedo itself.
    FromAlbedo = 2,
    /// `plasma`: the alpha mask's alpha.
    Plasma = 3,
    /// `plasma` through a multiply overlay (in the detail3 slot).
    PlasmaOverlay = 4,
}

/// A bitmap and its tiling.
#[derive(Clone, Copy, Debug)]
pub struct Map {
    pub bitmap: u32,
    pub xform: Xform,
}

/// How one sky material is drawn.
#[derive(Clone, Debug)]
pub struct SkyLook {
    pub albedo: Albedo,
    /// base, detail, detail2, detail3 (or the overlay), self-illumination.
    pub maps: [Option<Map>; 5],
    /// Linear RGBA multiplied into the albedo.
    pub albedo_colour: [f32; 4],
    /// Self-illumination colour times intensity.
    pub illum: Option<[f32; 3]>,
    /// Where the glow comes from.
    pub illum_mode: IllumMode,
    pub blend: Blend,
    /// Lit by the sky light (`material_model` other than none).
    pub lit: bool,
    /// The shader's `diffuse_coefficient` (1 when absent): scales the light
    /// on lit surfaces (120_halo's ring plates use 0.3).
    pub diffuse: f32,
}

/// Everything the sky needs, worked out before the window opens.
#[derive(Default)]
pub struct SkyData {
    /// By scene material index, for the materials sky placements use.
    pub looks: HashMap<u32, SkyLook>,
    pub bitmaps: HashMap<u32, Texture>,
    pub atmosphere: Option<Atmosphere>,
    /// [`sky_light_scale`] of the map.
    pub light_scale: f32,
}

#[derive(Resource, Default)]
pub struct PendingSky(pub Option<SkyData>);

/// A sky batch, by scene material index; its material is swapped for a
/// [`SkyMaterial`] once spawned.
#[derive(Component, Clone, Copy)]
pub struct SkyBatch(pub u32);

/// The atmosphere setting of the cluster the camera is in (`zones.rs`
/// keeps it up to date); `None`: the map's default world setting.
#[derive(Resource, Default)]
pub struct CameraAir(pub Option<u32>);

/// The world's haze for the camera (`None`: no atmosphere tag).
#[derive(Resource, Default)]
struct WorldHaze {
    setting: Option<AtmosphereSetting>,
    /// Every setting of the atmosphere tag, for clusters that name one.
    settings: Vec<AtmosphereSetting>,
    /// The sky's setting, for the clear colour.
    sky: Option<AtmosphereSetting>,
    exposure: f32,
    /// `HALO3_HAZE`: multiplies the world haze's density (0 turns it off).
    density: f32,
}

/// Shader colours are stored the way texture bytes are; to linear.
fn linear(c: [f32; 4]) -> [f32; 4] {
    let l = Color::srgba(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), c[3])
        .to_linear();
    [l.red, l.green, l.blue, c[3]]
}

fn look_of(s: &Shader) -> SkyLook {
    let albedo = Albedo::of(s.option("albedo"));
    let map = |usage: &str| {
        s.map(usage)
            .and_then(|m| Some(Map { bitmap: m.bitmap?, xform: m.xform }))
    };
    let mut overlay = map("detail_map_overlay").or_else(|| map("detail_map3"));
    let k = s.param("self_illum_intensity").map_or(1.0, |v| v[0]);
    let (illum_mode, illum_map, illum) = match s.option("self_illumination").unwrap_or("off") {
        "off" => (IllumMode::Off, None, None),
        "from_diffuse" => {
            let c = linear(s.param("self_illum_color").unwrap_or([1.0; 4]));
            (IllumMode::FromAlbedo, None, Some([c[0] * k, c[1] * k, c[2] * k]))
        }
        // Halogram plasma (Blackout's aurora): the alpha mask's alpha times
        // the mean of the three band colours, through the multiply overlay
        // (the crawling noise bands are not drawn), as the world does.
        "plasma" => {
            let bands = ["color_medium", "color_wide", "color_sharp"].map(|n| linear(s.param(n).unwrap_or([0.0; 4])));
            let mut c = [0, 1, 2].map(|i| bands.iter().map(|b| b[i]).sum::<f32>() / 3.0 * k);
            if s.option("overlay") == Some("multiply") {
                overlay = map("overlay_map");
                let tint = linear(s.param("overlay_tint").unwrap_or([1.0; 4]));
                let strength = s.param("overlay_intensity").map_or(1.0, |v| v[0]);
                c = [0, 1, 2].map(|i| c[i] * tint[i] * strength);
            }
            let mask = map("alpha_mask_map");
            let mode = if overlay.is_some() { IllumMode::PlasmaOverlay } else { IllumMode::Plasma };
            (if mask.is_some() { mode } else { IllumMode::Off }, mask, Some(c))
        }
        // A map-driven glow without its map adds nothing: the empty slot
        // reads white, which lit Blackout's cloud domes up like day.
        _ => match map("self_illum_map") {
            Some(m) => {
                let c = linear(s.param("self_illum_color").unwrap_or([1.0; 4]));
                (IllumMode::Map, Some(m), Some([c[0] * k, c[1] * k, c[2] * k]))
            }
            None => (IllumMode::Off, None, None),
        },
    };
    SkyLook {
        albedo,
        maps: [map("base_map"), map("detail_map"), map("detail_map2"), overlay, illum_map],
        albedo_colour: linear(s.param("albedo_color").unwrap_or([1.0; 4])),
        illum: illum.filter(|_| illum_mode != IllumMode::Off),
        illum_mode,
        blend: s.blend,
        lit: s.option("material_model").is_none_or(|m| m != "none"),
        diffuse: s.param("diffuse_coefficient").map_or(1.0, |v| v[0]),
    }
}

/// Resolves the sky materials' shaders, decodes their maps and reads the
/// scenario's atmosphere. Failures are logged; those materials keep the
/// plain drawing.
pub fn load(cache: &CacheFile, scene: &Scene) -> SkyData {
    let mut out = SkyData {
        light_scale: sky_light_scale(&scene.name),
        ..SkyData::default()
    };
    match blam_geometry::atmosphere::read(cache) {
        Ok(a) => out.atmosphere = a,
        Err(e) => eprintln!("atmosphere: {e}; no haze"),
    }
    let mut materials: Vec<u32> = scene
        .placements
        .iter()
        .filter(|p| p.kind == Kind::Sky)
        .flat_map(|p| scene.meshes[p.mesh as usize].parts.iter().map(|part| part.material))
        .collect();
    materials.sort_unstable();
    materials.dedup();
    let resources = match cache.resources() {
        Ok(r) => r,
        Err(e) => {
            eprintln!("sky: no resource tables ({e})");
            return out;
        }
    };
    for m in materials {
        let Some(tag) = scene.materials.get(m as usize).and_then(|m| m.shader_tag) else {
            continue;
        };
        let s = match shader::resolve(cache, tag) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("sky shader #{tag}: {e}");
                continue;
            }
        };
        let look = look_of(&s);
        for map in look.maps.iter().flatten() {
            if out.bitmaps.contains_key(&map.bitmap) {
                continue;
            }
            match bitmap::decode(cache, &resources, map.bitmap, 0) {
                Ok(t) => {
                    out.bitmaps.insert(map.bitmap, t);
                }
                Err(e) => eprintln!("sky bitmap #{} (for {}): {e}", map.bitmap, s.name),
            }
        }
        out.looks.insert(m, look);
    }
    resources.clear_page_cache();
    out
}

/// The sky half of [`SkyMaterial`].
#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct SkyShading {
    #[uniform(100)]
    pub xform_base: Vec4,
    #[uniform(100)]
    pub xform_detail: Vec4,
    #[uniform(100)]
    pub xform_detail2: Vec4,
    #[uniform(100)]
    pub xform_detail3: Vec4,
    #[uniform(100)]
    pub xform_illum: Vec4,
    #[uniform(100)]
    pub albedo_colour: Vec4,
    #[uniform(100)]
    pub illum_colour: Vec4,
    /// x: albedo mode, y: additive, z: lit, w: exposure.
    #[uniform(100)]
    pub mode: Vec4,
    #[uniform(100)]
    pub light: Vec4,
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
    /// x: sea level, y: Rayleigh height, z: Mie height, w: distance bias.
    #[uniform(100)]
    pub heights: Vec4,
    /// x: max fog thickness, y: atmosphere on, z: detail maps present
    /// (bits 0-2).
    #[uniform(100)]
    pub extra: Vec4,
    #[texture(101)]
    #[sampler(102)]
    pub base: Option<Handle<Image>>,
    #[texture(103)]
    pub detail: Option<Handle<Image>>,
    #[texture(104)]
    pub detail2: Option<Handle<Image>>,
    #[texture(105)]
    pub detail3: Option<Handle<Image>>,
    #[texture(106)]
    pub illum: Option<Handle<Image>>,
}

impl MaterialExtension for SkyShading {
    fn fragment_shader() -> ShaderRef {
        SHADER.into()
    }
}

fn xform(m: Option<&Map>) -> Vec4 {
    m.map_or(Vec4::new(1.0, 1.0, 0.0, 0.0), |m| {
        Vec4::new(m.xform.scale[0], m.xform.scale[1], m.xform.offset[0], m.xform.offset[1])
    })
}

fn v3(c: [f32; 3], w: f32) -> Vec4 {
    Vec4::new(c[0], c[1], c[2], w)
}

/// Fills in the atmosphere part of a [`SkyShading`].
fn set_atmosphere(s: &mut SkyShading, a: Option<&AtmosphereSetting>) {
    let Some(a) = a else {
        s.extra = Vec4::new(0.0, 0.0, 0.0, 0.0);
        return;
    };
    s.sun_dir = a.sun_direction().extend(a.mie_g);
    s.sun_colour = v3(a.sun_colour, 0.0) * Vec4::new(a.sun_intensity, a.sun_intensity, a.sun_intensity, 0.0);
    s.beta_m = v3(a.beta_m, 0.0);
    s.beta_p = v3(a.beta_p, 0.0);
    s.beta_m_theta = v3(a.beta_m_theta, 0.0);
    s.beta_p_theta = v3(a.beta_p_theta, 0.0);
    s.heights = Vec4::new(a.sea_level, a.rayleigh_height, a.mie_height, a.distance_bias);
    s.extra = Vec4::new(a.max_fog_thickness.max(1.0), 1.0, 0.0, 0.0);
}

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/sky.wesl");
        app.add_plugins(MaterialPlugin::<SkyMaterial>::default())
            .init_resource::<PendingSky>()
            .init_resource::<WorldHaze>()
            .init_resource::<CameraAir>()
            .add_systems(Update, (swap_sky_materials, update_sky_air, add_haze, update_haze).chain());
    }
}

/// How far a level ray runs for the clear colour: the riverworld sky dome's
/// radius, 7,560 world units.
const HORIZON_METRES: f32 = 23_000.0;

/// Maps whose lit sky surfaces take a fraction of the daylight below
/// (`sky-and-atmosphere.md` section 4). Halo's own source of sky light is not
/// decoded, and nothing found in the tags (atmosphere sun, camera fx,
/// lightmap) tells a night sky from a day one on every map, so the night
/// maps are listed. Blackout (`lockout`): its sky dome is a constant white
/// lit surface and its sea a lit plane; in daylight both came out white.
const NIGHT_SKIES: &[(&str, f32)] = &[("lockout", 0.1)];

/// The sky light scale of a map ([`NIGHT_SKIES`], else 1).
pub fn sky_light_scale(map: &str) -> f32 {
    NIGHT_SKIES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(map))
        .map_or(1.0, |(_, k)| *k)
}

/// Light on lit sky surfaces (clouds, distant mountains): the sunlit ground
/// irradiance of a clear Valhalla texel, exposed like the world, times the
/// map's scale. `HALO3_SKY_LIGHT` overrides the scale.
fn sky_light(exposure: f32, scale: f32) -> Vec3 {
    let scale = std::env::var("HALO3_SKY_LIGHT")
        .ok()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(scale);
    lighting::default_light().irradiance(Vec3::Z) * exposure * scale
}

#[allow(clippy::too_many_arguments)]
fn swap_sky_materials(
    mut commands: Commands,
    mut pending: ResMut<PendingSky>,
    mut haze: ResMut<WorldHaze>,
    batches: Query<(Entity, &SkyBatch, &MeshMaterial3d<StandardMaterial>), Added<SkyBatch>>,
    standard: Res<Assets<StandardMaterial>>,
    mut sky_materials: ResMut<Assets<SkyMaterial>>,
    mut images: ResMut<Assets<Image>>,
    compressed: Option<Res<bevy::image::CompressedImageFormatSupport>>,
    mut handles: Local<HashMap<u32, Option<Handle<Image>>>>,
) {
    if batches.is_empty() {
        return;
    }
    let Some(data) = pending.0.take() else {
        return;
    };
    let bc = compressed.is_some_and(|c| c.0.supports(bevy::render::render_resource::TextureFormat::Bc3RgbaUnormSrgb));
    let exposure = lighting::exposure();
    let atmosphere = data.atmosphere.as_ref();
    haze.setting = atmosphere.and_then(Atmosphere::world).cloned();
    haze.sky = atmosphere.and_then(Atmosphere::sky).cloned();
    haze.settings = atmosphere.map(|a| a.settings.clone()).unwrap_or_default();
    haze.exposure = exposure;
    haze.density = std::env::var("HALO3_HAZE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);
    if let Some(a) = atmosphere {
        let w = a.world().map_or("-", |s| s.name.as_str());
        let s = a.sky().map_or("-", |s| s.name.as_str());
        info!("atmosphere {}: world haze {w:?}, sky {s:?}", a.tag_name);
    }
    let light = sky_light(exposure, data.light_scale);
    let mut swapped = 0;
    for (entity, batch, standard_handle) in &batches {
        let Some(look) = data.looks.get(&batch.0) else {
            continue;
        };
        let Some(base) = standard.get(&standard_handle.0).cloned() else {
            continue;
        };
        let mut image = |m: Option<&Map>| -> Option<Handle<Image>> {
            let m = m?;
            handles
                .entry(m.bitmap)
                .or_insert_with(|| {
                    let t = data.bitmaps.get(&m.bitmap)?;
                    match super::texture_image(t, bc) {
                        Ok(mut i) => {
                            // Sampled the way the bitmap's DXGI format says:
                            // masks are linear, colour maps mostly sRGB.
                            if !t.srgb {
                                let f = &mut i.texture_descriptor.format;
                                *f = f.remove_srgb_suffix();
                            }
                            Some(images.add(i))
                        }
                        Err(e) => {
                            warn!("sky texture {}: {e}", t.name);
                            None
                        }
                    }
                })
                .clone()
        };
        let maps = &look.maps;
        let mut shading = SkyShading {
            xform_base: xform(maps[0].as_ref()),
            xform_detail: xform(maps[1].as_ref()),
            xform_detail2: xform(maps[2].as_ref()),
            xform_detail3: xform(maps[3].as_ref()),
            xform_illum: xform(maps[4].as_ref()),
            albedo_colour: Vec4::from_array(look.albedo_colour),
            illum_colour: look.illum.map_or(Vec4::ZERO, |c| v3(c, look.illum_mode as u32 as f32)),
            mode: Vec4::new(
                look.albedo as u32 as f32,
                (look.blend == Blend::Additive) as u32 as f32,
                look.lit as u32 as f32,
                exposure,
            ),
            light: (light * look.diffuse).extend(0.0),
            base: image(maps[0].as_ref()),
            detail: image(maps[1].as_ref()),
            detail2: image(maps[2].as_ref()),
            detail3: image(maps[3].as_ref()),
            illum: image(maps[4].as_ref()),
            ..default()
        };
        set_atmosphere(&mut shading, atmosphere.and_then(Atmosphere::sky));
        // Empty detail slots read as neutral grey in the shader (a missing
        // texture binds white, which a detail would scale by 4.6).
        shading.extra.z = (shading.detail.is_some() as u32
            | (shading.detail2.is_some() as u32) << 1
            | (shading.detail3.is_some() as u32) << 2) as f32;
        let material = SkyMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                base_color_texture: None,
                unlit: true,
                fog_enabled: false,
                ..base
            },
            extension: shading,
        };
        commands
            .entity(entity)
            .remove::<MeshMaterial3d<StandardMaterial>>()
            .insert(MeshMaterial3d(sky_materials.add(material)));
        swapped += 1;
    }
    info!("sky: {swapped} batches drawn with their atmosphere");
}

/// The setting the sky is seen through: the camera's cluster's when it names
/// one (campaign maps: 120_halo's Warthog run has thin air, its snowfields
/// thick), else the map's sky setting (`sky-and-atmosphere.md` 3.3).
fn sky_setting<'a>(haze: &'a WorldHaze, air: &CameraAir) -> Option<&'a AtmosphereSetting> {
    air.0
        .and_then(|i| haze.settings.get(i as usize))
        .or(haze.sky.as_ref())
}

/// Re-aims the sky's scattering when the camera moves into other air.
fn update_sky_air(
    air: Res<CameraAir>,
    haze: Res<WorldHaze>,
    batches: Query<&MeshMaterial3d<SkyMaterial>>,
    added: Query<(), Added<MeshMaterial3d<SkyMaterial>>>,
    mut materials: ResMut<Assets<SkyMaterial>>,
) {
    if !air.is_changed() && added.is_empty() {
        return;
    }
    let setting = sky_setting(&haze, &air);
    for handle in &batches {
        if let Some(mut m) = materials.get_mut(handle) {
            let detail_bits = m.extension.extra.z;
            set_atmosphere(&mut m.extension, setting);
            m.extension.extra.z = detail_bits;
        }
    }
}

/// Gives the camera its haze once the world is up.
fn add_haze(mut commands: Commands, cameras: Query<Entity, (With<Camera3d>, Without<DistanceFog>)>, haze: Res<WorldHaze>) {
    if haze.setting.is_none() {
        return;
    }
    for camera in &cameras {
        commands.entity(camera).insert(DistanceFog::default());
    }
}

/// Mean of `exp(-h / scale)` between heights `h0` and `h1`.
fn mean_density(h0: f32, h1: f32, scale: f32) -> f32 {
    let s = scale.max(1e-3);
    let dh = h1 - h0;
    if dh.abs() < 1e-2 {
        (-h0 / s).exp()
    } else {
        s * ((-h0 / s).exp() - (-h1 / s).exp()) / dh
    }
}

/// The haze for an eye at height `z` (Halo units): extinction per metre and
/// the in-scattered colour it fades toward (exposed).
pub fn haze_at(a: &AtmosphereSetting, z: f32, exposure: f32) -> (Vec3, Vec3) {
    // Bevy's fog has one density along the whole ray: take the one at the
    // eye, right for level rays (most of them), thin for rays down into a
    // valley.
    // Below the setting's sea level the density stays at the sea level's:
    // Sandtrap's play space is 20 wu below it, where the falloff would make
    // the air e^10 times thicker and the view black.
    let h = (z - a.sea_level).max(0.0);
    let dm = mean_density(h, h, a.rayleigh_height);
    let dp = mean_density(h, h, a.mie_height);
    let bm = Vec3::from_array(a.beta_m);
    let bp = Vec3::from_array(a.beta_p);
    let tau = bm * dm + bp * dp;
    // Phase functions averaged over all directions: Rayleigh's (1 + mu^2)
    // to 4/3, the Henyey-Greenstein prefix (1 - g)^2 / (...)^1.5 to
    // (1 - g) / (1 + g).
    let g = a.mie_g.clamp(-0.95, 0.95);
    let rayleigh = Vec3::from_array(a.beta_m_theta) * (4.0 / 3.0);
    let mie = Vec3::from_array(a.beta_p_theta) * ((1.0 - g) / (1.0 + g));
    let ratio = (rayleigh * dm + mie * dp) / tau.max(Vec3::splat(1e-9));
    let colour = ratio * Vec3::from_array(a.sun_colour) * a.sun_intensity * exposure;
    (tau / WORLD_UNIT_METRES, colour)
}

fn update_haze(
    haze: Res<WorldHaze>,
    air: Res<CameraAir>,
    mut clear: ResMut<ClearColor>,
    mut cameras: Query<(&GlobalTransform, &mut DistanceFog)>,
) {
    let Some(default) = &haze.setting else {
        return;
    };
    // Campaign clusters name their own setting (`zone-sets.md` 3).
    let a = air.0.and_then(|i| haze.settings.get(i as usize)).unwrap_or(default);
    if air.is_changed() {
        info!("haze: {:?} ({})", air.0, a.name);
    }
    for (transform, mut fog) in &mut cameras {
        let z = super::to_halo(transform.translation()).z;
        if let Some(sky) = sky_setting(&haze, &air) {
            // Where no sky geometry covers the view (below the dome's rim),
            // the colour of a long level ray through the sky's air.
            let (extinction, colour) = haze_at(sky, z, haze.exposure);
            let c = colour * (1.0 - (-extinction * HORIZON_METRES).exp());
            clear.0 = Color::linear_rgb(c.x, c.y, c.z);
        }
        let (extinction, colour) = haze_at(a, z, haze.exposure);
        let extinction = extinction * haze.density;
        fog.color = Color::linear_rgb(colour.x, colour.y, colour.z);
        fog.directional_light_color = Color::NONE;
        // Where the fog starts, for the world shaders (`fog_position` in
        // `lightmap.wesl`): unused by bevy while the light colour is none.
        fog.directional_light_exponent = (-a.distance_bias).max(0.0) * WORLD_UNIT_METRES;
        fog.falloff = FogFalloff::Atmospheric {
            extinction,
            inscattering: extinction,
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn night_skies() {
        assert_eq!(sky_light_scale("lockout"), 0.1);
        assert_eq!(sky_light_scale("riverworld"), 1.0);
    }

    #[test]
    fn densities() {
        assert!((mean_density(0.0, 0.0, 10.0) - 1.0).abs() < 1e-6);
        // Mean of exp(-h) over 0..1 is 1 - 1/e.
        assert!((mean_density(0.0, 1.0, 1.0) - (1.0 - (-1.0f32).exp())).abs() < 1e-5);
        assert!((mean_density(1.0, 0.0, 1.0) - mean_density(0.0, 1.0, 1.0)).abs() < 1e-5);
    }
}
