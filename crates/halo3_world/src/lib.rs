//! Puts a decoded [`Scene`] on screen: one bevy mesh per material, in
//! metres, Y up.
//!
//! Halo is Z up and measures in world units of ten feet. The renderer frame
//! is `(x, y, z) = 3.048 * (hx, hz, -hy)`: a rotation, not a mirror, the
//! same mapping HaloExport uses for its glTF files.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormatSupport, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor,
};
use bevy::light::NotShadowCaster;
use bevy::math::{Affine2, Mat2};
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use blam_geometry::{Kind, Scene, Shading};
use blam_texture::{Blend, Format, Texture, rgba};

use halo3_core::fly_camera::FlyCamera;
use halo3_textures::{Look, MaterialTextures};

pub mod build;
pub mod culling;
pub mod lighting;
pub mod sky;
pub mod stream;
pub mod water;
pub mod zones;

use build::BuildInput;
use lighting::{Atlas, HaloLightmap, HaloMaterial, SurfaceLights};
use zones::{BspMember, LoadedBsps, ShowCollision};

pub use halo3_core::coords::{to_halo, to_halo_dir, to_render, to_render_dir};

/// The scene and its decoded textures, handed from `main` to the startup
/// system.
#[derive(Resource)]
pub struct PendingScene(
    pub Option<Scene>,
    pub Option<MaterialTextures>,
    /// The open map, for decoding a zone set's textures when it loads.
    pub Option<&'static blam_cache::CacheFile>,
);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            lighting::LightingPlugin,
            sky::SkyPlugin,
            water::WaterPlugin,
            zones::ZonesPlugin,
            stream::StreamPlugin,
            culling::CullingPlugin,
        ))
            .insert_resource(ClearColor(Color::srgb(0.53, 0.70, 0.90)))
            .add_systems(Startup, spawn_world)
            .add_systems(
                Update,
                (toggle_collision.run_if(halo3_core::pause::running), spawn_collision_on_demand).chain(),
            );
    }
}

/// The scene material a world batch draws (its index in `Scene::materials`),
/// for systems that swap in a material of their own (`water.rs`).
#[derive(Component, Clone, Copy)]
pub struct BatchMaterial(pub u32);

/// How many triangles a world mesh entity draws, for `--bench`.
#[derive(Component, Clone, Copy)]
pub struct Triangles(pub u32);

/// The game's collision drawn as a translucent shell; V shows and hides it.
#[derive(Component)]
pub struct CollisionOverlay;

fn toggle_collision(keys: Res<ButtonInput<KeyCode>>, mut show: ResMut<ShowCollision>) {
    if keys.just_pressed(KeyCode::KeyV) {
        show.0 = !show.0;
    }
}

fn spawn_collision(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    scene: &Scene,
) {
    let col = &scene.collision;
    if col.triangles.is_empty() {
        return;
    }
    // Two shells per BSP: ordinary surfaces in green, invisible walls in red.
    let mut bsps: Vec<u32> = col.triangles.iter().map(|t| t.source.bsp()).collect();
    bsps.sort_unstable();
    bsps.dedup();
    let shells = bsps.iter().flat_map(|b| {
        [(*b, false, Color::srgba(0.2, 0.9, 0.3, 0.35)), (*b, true, Color::srgba(0.95, 0.25, 0.2, 0.45))]
    });
    let mut counts = [0usize; 2];
    for (bsp, invisible, colour) in shells {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        for t in col
            .triangles
            .iter()
            .filter(|t| t.source.bsp() == bsp && (t.flags & 2 != 0) == invisible)
        {
            let n = to_render_dir(t.normal()).to_array();
            for p in t.points {
                positions.push(to_render(p).to_array());
                normals.push(n);
            }
        }
        if positions.is_empty() {
            continue;
        }
        counts[invisible as usize] += positions.len() / 3;
        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
        commands.spawn((
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: colour,
                alpha_mode: AlphaMode::Blend,
                unlit: true,
                cull_mode: None,
                double_sided: true,
                ..default()
            })),
            Visibility::Hidden,
            CollisionOverlay,
            BspMember {
                bsps: 1u32.checked_shl(bsp).unwrap_or(0),
                sky: None,
            },
            NotShadowCaster,
            Name::new(if invisible { "collision (invisible walls)" } else { "collision" }),
        ));
    }
    info!(
        "collision overlay: {} surface and {} invisible-wall triangles (press V)",
        counts[0], counts[1]
    );
}

/// A steady, distinct colour per material name, so the untextured world is
/// readable.
fn material_colour(name: &str) -> Color {
    let mut h: u32 = 2166136261;
    for b in name.bytes() {
        h = (h ^ b as u32).wrapping_mul(16777619);
    }
    let hue = (h % 360) as f32;
    let sat = 0.25 + ((h >> 9) % 40) as f32 / 100.0;
    let light = 0.45 + ((h >> 17) % 30) as f32 / 100.0;
    Color::hsl(hue, sat, light)
}

/// A repeating, trilinear sampler for a world texture.
fn tiling_sampler(levels: u32) -> ImageSampler {
    let mips = levels > 1;
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: if mips { ImageFilterMode::Linear } else { ImageFilterMode::Nearest },
        // Anisotropy needs all three filters linear (wgpu rule).
        anisotropy_clamp: if mips { 8 } else { 1 },
        ..default()
    })
}

/// A colour texture as a bevy image, sampled as sRGB; see [`texture_image_as`].
pub(crate) fn texture_image(t: &Texture, bc: bool) -> Result<Image, String> {
    texture_image_as(t, bc, true)
}

/// A texture as a bevy image: layer 0 of every level, largest
/// first. BC1/2/3 stay compressed when the GPU takes BC and the size is a
/// whole number of blocks; A8R8G8B8 goes up as BGRA; everything else is
/// expanded to RGBA8 on the CPU. Colour maps (`srgb`) are always sampled as
/// sRGB: Halo's colour maps are authored that way whatever their curve byte
/// says (the "linear"-curve `mountains_snow` only looks right read as sRGB).
/// Data that is not a colour (a terrain blend map's weights) goes up linear.
pub(crate) fn texture_image_as(t: &Texture, bc: bool, srgb: bool) -> Result<Image, String> {
    let levels = t.mips.len() as u32;
    if levels == 0 {
        return Err("no levels".into());
    }
    let pick = |s: TextureFormat, l: TextureFormat| if srgb { s } else { l };
    let compressed = match t.format {
        Format::Bc1 => Some(pick(TextureFormat::Bc1RgbaUnormSrgb, TextureFormat::Bc1RgbaUnorm)),
        Format::Bc2 => Some(pick(TextureFormat::Bc2RgbaUnormSrgb, TextureFormat::Bc2RgbaUnorm)),
        Format::Bc3 => Some(pick(TextureFormat::Bc3RgbaUnormSrgb, TextureFormat::Bc3RgbaUnorm)),
        _ => None,
    }
    .filter(|_| bc && t.width.is_multiple_of(4) && t.height.is_multiple_of(4));
    let raw = |format: TextureFormat| -> Result<(TextureFormat, Vec<u8>), String> {
        let mut data = Vec::with_capacity(t.byte_len() / t.layers.max(1) as usize);
        for l in 0..levels {
            data.extend_from_slice(t.level(l, 0).ok_or_else(|| format!("level {l} is short"))?);
        }
        Ok((format, data))
    };
    let (format, data) = match (compressed, t.format) {
        (Some(f), _) => raw(f)?,
        (None, Format::A8R8G8B8) => raw(pick(TextureFormat::Bgra8UnormSrgb, TextureFormat::Bgra8Unorm))?,
        (None, f) => {
            let mut data = Vec::new();
            for l in 0..levels {
                let (w, h) = t.level_dims(l);
                let level = t.level(l, 0).ok_or_else(|| format!("level {l} is short"))?;
                data.extend(rgba::decode_level(f, w, h, level).map_err(|e| e.to_string())?);
            }
            (pick(TextureFormat::Rgba8UnormSrgb, TextureFormat::Rgba8Unorm), data)
        }
    };
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        format,
        // The CPU copy is dropped once the GPU has it.
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = tiling_sampler(levels);
    Ok(image)
}

/// A normal (bump) map as a bevy image, linear. DXN (BC5) goes up still
/// compressed as `Bc5RgSnorm` when the GPU takes BC, and then reads -1..1
/// (`true`); anything else is expanded on the CPU to RGBA8 reading 0..1
/// (`false`; the CPU BC5 decoder maps -1..1 to 0..1).
pub(crate) fn normal_image(t: &Texture, bc: bool) -> Result<(Image, bool), String> {
    if t.format == Format::Bc5 && bc && t.width.is_multiple_of(4) && t.height.is_multiple_of(4) {
        let levels = t.mips.len() as u32;
        let mut data = Vec::with_capacity(t.byte_len());
        for l in 0..levels {
            data.extend_from_slice(t.level(l, 0).ok_or_else(|| format!("level {l} is short"))?);
        }
        let mut image = Image::new_uninit(
            Extent3d {
                width: t.width,
                height: t.height,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            TextureFormat::Bc5RgSnorm,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = levels;
        image.data = Some(data);
        image.sampler = tiling_sampler(levels);
        return Ok((image, true));
    }
    Ok((texture_image_as(t, bc && t.format != Format::Bc5, false)?, false))
}

/// A 1x1 white image whose only job is to carry the repeating, trilinear,
/// anisotropic sampler the surface maps are read with.
pub(crate) fn sampler_carrier() -> Image {
    let mut image = Image::new(
        Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        vec![255; 4],
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = tiling_sampler(2);
    image
}

/// How a bitmap is uploaded: as a colour (sRGB), as plain data (linear), or
/// as a normal map ([`normal_image`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Upload {
    Colour,
    Linear,
    Normal,
}

/// The `StandardMaterial` for one scene material.
pub(crate) fn standard_material(
    name: &str,
    material: Option<&blam_geometry::Material>,
    look: Option<&Look>,
    texture: Option<Handle<Image>>,
) -> StandardMaterial {
    let shading = material.map_or(Shading::Opaque, |m| m.shading);
    let srgb = |c: [f32; 4], alpha: f32| {
        Color::srgba(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0), alpha)
    };
    let (base_color, alpha_mode, two_sided, uv_transform) = match look {
        Some(l) => {
            let base_color = if l.water {
                srgb(l.colour.unwrap_or([0.07, 0.11, 0.14, 1.0]), 0.75)
            } else {
                // Shader colours are stored the way texture bytes are
                // (water_diffuse equals its texture's mean byte values).
                match (texture.is_some(), l.colour) {
                    (_, Some(c)) => srgb(c, 1.0),
                    (true, None) => Color::WHITE,
                    (false, None) => material_colour(name),
                }
            };
            let alpha_mode = if l.water {
                AlphaMode::Blend
            } else if l.alpha_test {
                AlphaMode::Mask(0.5)
            } else {
                match l.blend {
                    Blend::Opaque => AlphaMode::Opaque,
                    Blend::AlphaBlend => AlphaMode::Blend,
                    Blend::PreMultiplied => AlphaMode::Premultiplied,
                    Blend::Additive => AlphaMode::Add,
                    Blend::Multiply => AlphaMode::Multiply,
                }
            };
            // Halo's xform is `uv * scale + offset`; the mesh keeps the raw
            // UVs and the tiling goes here (no V flip: see blam_texture's
            // `riverworld_v_runs_down` test).
            let uv = Affine2 {
                matrix2: Mat2::from_diagonal(Vec2::from(l.xform.scale)),
                translation: Vec2::from(l.xform.offset),
            };
            (base_color, alpha_mode, l.two_sided, uv)
        }
        None => (
            material_colour(name),
            match shading {
                Shading::Translucent => AlphaMode::Blend,
                _ => AlphaMode::Opaque,
            },
            material.is_some_and(|m| m.two_sided),
            Affine2::IDENTITY,
        ),
    };
    StandardMaterial {
        base_color,
        base_color_texture: texture,
        uv_transform,
        perceptual_roughness: 0.92,
        metallic: 0.0,
        reflectance: 0.2,
        cull_mode: if two_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        },
        double_sided: two_sided,
        // A multiply decal scales what is under it, already fogged; fog
        // on the decal itself would tint it twice.
        fog_enabled: alpha_mode != AlphaMode::Multiply,
        alpha_mode,
        ..default()
    }
}

/// A terrain material's GPU images and xforms, ready for [`HaloLightmap`].
pub(crate) struct TerrainImages {
    /// `None`: no blend map, all layers that are on weigh the same.
    blend_map: Option<Handle<Image>>,
    blend_xform: Vec4,
    layers: [Option<TerrainLayerImages>; 4],
}

struct TerrainLayerImages {
    base: Handle<Image>,
    base_xform: Vec4,
    detail: Option<(Handle<Image>, Vec4)>,
    /// The bump map, its xform and whether it reads signed.
    bump: Option<(Handle<Image>, Vec4, bool)>,
    specular: f32,
    power: f32,
    tint: Vec4,
}

/// An uploaded image and whether it reads -1..1 (BC5 normal maps).
pub(crate) type Uploaded = Option<(Handle<Image>, bool)>;

fn xform_vec(x: &blam_texture::Xform) -> Vec4 {
    Vec4::new(x.scale[0], x.scale[1], x.offset[0], x.offset[1])
}

/// Uploads (or reuses) a terrain's images; `None` when the blend map or
/// every layer failed, so the caller falls back to the dominant layer.
pub(crate) fn terrain_textures(
    t: &halo3_textures::TerrainLook,
    image: &mut dyn FnMut(u32, Upload) -> Uploaded,
) -> Option<TerrainImages> {
    let blend_map = match t.blend_map {
        Some(b) => Some(image(b, Upload::Linear)?.0),
        None => None,
    };
    let layers = t.layers.clone().map(|l| {
        let l = l?;
        Some(TerrainLayerImages {
            base: image(l.base, Upload::Colour)?.0,
            base_xform: xform_vec(&l.base_xform),
            detail: l.detail.and_then(|(d, x)| Some((image(d, Upload::Colour)?.0, xform_vec(&x)))),
            bump: l
                .bump
                .and_then(|(b, x)| image(b, Upload::Normal).map(|(h, s)| (h, xform_vec(&x), s))),
            specular: l.specular,
            power: l.power,
            tint: Vec3::from(l.tint).extend(l.analytical),
        })
    });
    layers.iter().any(Option::is_some).then(|| TerrainImages {
        blend_map,
        blend_xform: xform_vec(&t.blend_xform),
        layers,
    })
}

/// Whether to normalise the terrain blend weights to sum to one (the
/// default); `HALO3_TERRAIN_RAW=1` uses them as stored, for comparing.
fn normalise_terrain() -> bool {
    std::env::var("HALO3_TERRAIN_RAW").map_or(true, |v| v.is_empty() || v == "0")
}

/// Whether terrain draws its four-layer blend (the default);
/// `HALO3_TERRAIN_BLEND=0` draws the dominant layer alone, as before.
pub(crate) fn terrain_blend() -> bool {
    std::env::var("HALO3_TERRAIN_BLEND").map_or(true, |v| v != "0")
}

impl TerrainImages {
    pub(crate) fn apply(self, e: &mut HaloLightmap) {
        // A layer that is off still binds a real texture (another layer's)
        // and gets zero weight.
        let fill = self.layers.iter().flatten().next().map(|l| l.base.clone());
        let mut bases: [Option<Handle<Image>>; 4] = Default::default();
        let mut details: [Option<Handle<Image>>; 4] = Default::default();
        for (k, l) in self.layers.into_iter().enumerate() {
            let Some(l) = l else {
                bases[k] = fill.clone();
                details[k] = fill.clone();
                e.base_xforms[k] = Vec4::new(1.0, 1.0, 0.0, 0.0);
                continue;
            };
            e.terrain_layers[k] = 1.0;
            e.base_xforms[k] = l.base_xform;
            let on = materials_on();
            e.terrain_spec[k] = if on { l.specular * specular_gain() } else { 0.0 };
            e.terrain_power[k] = l.power;
            e.terrain_tints[k] = l.tint;
            if let Some((b, x, signed)) = l.bump.filter(|_| on) {
                e.terrain_bump[k] = 1.0;
                e.terrain_bump_signed[k] = if signed { 1.0 } else { 0.0 };
                e.terrain_bump_xforms[k] = x;
                match k {
                    0 => e.terrain_bump_0 = Some(b),
                    1 => e.terrain_bump_1 = Some(b),
                    2 => e.terrain_bump_2 = Some(b),
                    _ => e.terrain_bump_3 = Some(b),
                }
            }
            match l.detail {
                Some((d, dx)) => {
                    details[k] = Some(d);
                    e.detail_xforms[k] = dx;
                }
                // Zero detail scale: the shader skips it.
                None => details[k] = Some(l.base.clone()),
            }
            bases[k] = Some(l.base);
        }
        if e.terrain_spec != Vec4::ZERO {
            // One phong lobe per layer (tint and analytical contribution in
            // `terrain_tints`), no area term.
            e.spec.w = 5.0;
        }
        e.terrain_extra = Vec4::new(
            lighting::DETAIL_MULTIPLIER,
            if normalise_terrain() { 1.0 } else { 0.0 },
            0.0,
            0.0,
        );
        e.blend_xform = self.blend_xform;
        // Without a blend map the fallback image weighs the layers alike
        // once normalised (and a zero sum falls back to equal weights).
        if self.blend_map.is_none() {
            e.terrain_extra.y = 1.0;
        }
        e.blend_map = self.blend_map;
        [e.base_0, e.base_1, e.base_2, e.base_3] = bases;
        [e.detail_0, e.detail_1, e.detail_2, e.detail_3] = details;
    }
}

/// Fills in the material model of an ordinary shader: its extra maps
/// (detail, bump, specular mask, self-illumination) and parameters.
pub(crate) fn apply_surface(
    s: &halo3_textures::SurfaceLook,
    e: &mut HaloLightmap,
    image: &mut dyn FnMut(u32, Upload) -> Uploaded,
) {
    use halo3_textures::{Illum, Model, SpecMask};
    let mut get = |m: Option<(u32, blam_texture::Xform)>, how: Upload| {
        m.and_then(|(b, x)| image(b, how).map(|(h, signed)| (h, xform_vec(&x), signed)))
    };
    // The cut-out's own alpha map is not a material feature: always on.
    if let Some((h, x, _)) = get(s.alpha_map, Upload::Linear) {
        e.alpha_map = Some(h);
        e.alpha_xform = x;
        e.alpha_flags.x = 1.0;
    }
    if !materials_on() {
        return;
    }
    if let Some((h, x, _)) = get(s.detail, Upload::Colour) {
        e.detail_map = Some(h);
        e.detail_xform = x;
        e.surface.z = 1.0;
        e.terrain_extra.x = lighting::DETAIL_MULTIPLIER;
    }
    if let Some((h, x, signed)) = get(s.bump, Upload::Normal) {
        e.bump_map = Some(h);
        e.bump_xform = x;
        e.signed.x = if signed { 1.0 } else { 0.0 };
        e.surface.x = 1.0;
        if let Some((h, x, signed)) = get(s.bump_detail.map(|(b, x, _)| (b, x)), Upload::Normal) {
            e.bump_detail_map = Some(h);
            e.bump_detail_xform = x;
            e.signed.y = if signed { 1.0 } else { 0.0 };
            e.surface.y = s.bump_detail.map_or(0.0, |d| d.2);
        }
    }
    e.surface.w = match s.spec_mask {
        SpecMask::None => 0.0,
        SpecMask::BaseAlpha => 1.0,
        SpecMask::Texture => match get(s.spec_mask_map, Upload::Linear) {
            Some((h, x, _)) => {
                e.mask_map = Some(h);
                e.mask_xform = x;
                2.0
            }
            None => 0.0,
        },
    };
    let model = match s.model {
        Model::Diffuse => 0.0,
        Model::Phong => 1.0,
        Model::CookTorrance => 2.0,
        Model::Glass => 3.0,
    };
    if model != 0.0 {
        e.spec = Vec4::new(s.specular * specular_gain(), s.power[0], s.power[1], s.fresnel_steepness);
        e.tint_normal = Vec3::from(s.tint[0]).extend(s.analytical);
        e.tint_glancing = Vec3::from(s.tint[1]).extend(s.area);
        e.f0 = Vec3::from(s.f0).extend(model);
        e.env = (Vec3::from(s.env_tint) * s.env).extend(0.0);
    }
    let mut mode = match s.illum {
        Illum::Off => 0.0,
        Illum::Map => 1.0,
        Illum::MapDetail => 2.0,
        Illum::FromDiffuse => 3.0,
        Illum::TimesDiffuse => 4.0,
        Illum::ThreeChannel => 5.0,
        Illum::Plasma => 6.0,
    };
    if mode == 0.0 {
        return;
    }
    if mode != 3.0 {
        let Some((h, x, _)) = get(s.illum_map, Upload::Colour) else {
            return;
        };
        e.illum_map = Some(h);
        e.illum_xform = x;
    }
    if mode == 2.0 {
        match get(s.illum_detail, Upload::Colour) {
            Some((h, x, _)) => {
                e.illum_detail_map = Some(h);
                e.illum_detail_xform = x;
                e.terrain_extra.x = lighting::DETAIL_MULTIPLIER;
            }
            // No detail map: the plain map.
            None => mode = 1.0,
        }
    }
    e.illum.x = mode;
    let gain = illum_gain();
    [e.illum_a, e.illum_b, e.illum_c] = s.illum_colour.map(|c| (Vec3::from(c) * gain).extend(0.0));
}

/// Multiplayer markers (`respawn_point`, the per-game-type
/// `*_initial_spawn_point`, `*_respawn_zone`, flag and bomb spawn points,
/// goal and return areas, hills and territories): Forge shows them, a game
/// only its own game type's, and then mostly as effects. Unnamed placements
/// are called after their object (`scenario.rs`), e.g. `respawn_point 12`.
pub(crate) fn is_editor_only(placement: &str) -> bool {
    let object = placement.split(' ').next().unwrap_or(placement);
    [
        "spawn_point",
        "spawn_point_invisible",
        "respawn_zone",
        "_goal_area",
        "_return_area",
        "_destination_static",
        "hill_static",
        "territory_static",
        "territory_flag",
    ]
    .iter()
    .any(|end| object.ends_with(end))
}

/// Whether ordinary shaders get their material model and terrain its bump
/// and specular (the default); `HALO3_MATERIALS=0` draws base colour and
/// lightmap only, as before, for comparisons.
fn materials_on() -> bool {
    std::env::var("HALO3_MATERIALS").map_or(true, |v| v != "0")
}

/// Scales every specular term (`HALO3_SPECULAR`, default 1; 0 turns
/// specular off) for comparisons.
fn specular_gain() -> f32 {
    std::env::var("HALO3_SPECULAR").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)
}

/// Scales self-illumination (`HALO3_SELF_ILLUM`, default 1).
fn illum_gain() -> f32 {
    std::env::var("HALO3_SELF_ILLUM").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0)
}

/// Builds and uploads the start zone set (every unit at once, in
/// parallel), places the camera, and hands the scene to the streaming
/// (`stream.rs`) for later zone sets.
#[allow(clippy::too_many_arguments)]
fn spawn_world(
    mut commands: Commands,
    mut pending: ResMut<PendingScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut halo_materials: ResMut<Assets<HaloMaterial>>,
    mut images: ResMut<Assets<Image>>,
    compressed: Option<Res<CompressedImageFormatSupport>>,
    mut loaded: ResMut<LoadedBsps>,
    mut residency: ResMut<stream::Residency>,
    mut cache: ResMut<stream::GpuCache>,
) {
    let Some(scene) = pending.0.take() else {
        return;
    };
    let textures = pending.1.take().unwrap_or_default();
    // Without the resource we cannot tell; decode on the CPU to be safe.
    let bc = compressed.is_some_and(|c| c.0.supports(TextureFormat::Bc3RgbaUnormSrgb));

    // CPU reads of every BSP's lightmap, for lighting objects by the
    // surface under them.
    let started = std::time::Instant::now();
    let atlases: Vec<Option<Atlas>> = scene
        .lightmaps
        .iter()
        .enumerate()
        .map(|(i, lm)| textures.lightmaps.get(i).and_then(Option::as_ref).map(|t| Atlas::new(t, lm.scales)))
        .collect();
    let surface = Arc::new(SurfaceLights::build(&scene, &atlases));
    drop(atlases);
    info!(
        "lighting: {} of {} lightmaps decoded, surface lights sampled in {:.2} s{}",
        textures.lightmaps.iter().flatten().count(),
        scene.lightmaps.len(),
        started.elapsed().as_secs_f32(),
        if surface.is_empty() { " (none: default sun)" } else { "" }
    );
    let lightmap_ok = textures.lightmaps.iter().map(Option::is_some).collect();
    let halo3_textures::MaterialTextures {
        looks,
        bitmaps,
        lightmaps,
    } = textures;
    let source = stream::WorldSource {
        input: Arc::new(BuildInput {
            scene,
            looks,
            lightmap_ok,
            surface: Arc::clone(&surface),
            cache: pending.2,
        }),
        bitmaps,
        lightmaps,
        bc,
    };
    let scene = &source.input.scene;

    // The start zone set, built in parallel and uploaded at once.
    let started = std::time::Instant::now();
    let mut built = stream::build_now(&source, loaded.mask & scene.all_bsps());
    let build_seconds = started.elapsed().as_secs_f32();
    let (mut triangles, mut chunks) = (0usize, 0usize);
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut lit_counts: HashMap<&str, usize> = HashMap::new();
    let mut editor_only = 0;
    for b in &mut built {
        triangles += b.chunks.iter().map(|c| c.triangles as usize).sum::<usize>();
        chunks += b.chunks.len();
        if let Some((a, c)) = b.extent {
            lo = lo.min(a);
            hi = hi.max(c);
        }
        for (k, n) in &b.lit_counts {
            *lit_counts.entry(k).or_default() += n;
        }
        editor_only += b.editor_only;
        let mut entities = Vec::new();
        stream::upload(
            b,
            u32::MAX,
            &mut entities,
            &mut commands,
            &source,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut halo_materials,
        );
        residency.adopt(b.unit, entities);
    }
    loaded.shown = loaded.mask;
    info!(
        "world: {chunks} chunks, {triangles} triangles built in {build_seconds:.2} s; {} materials textured ({} terrain blends), {:.1} MiB of textures and lightmaps to the GPU (BC support {bc})",
        cache.textured.len(),
        cache.terrains.len(),
        cache.live_image_bytes(&images) as f64 / (1 << 20) as f64
    );
    let mut counts: Vec<_> = lit_counts.into_iter().collect();
    counts.sort_unstable();
    info!("lit placements: {counts:?}; {editor_only} spawn-marker pieces not drawn");

    let (lo, hi) = if lo.x.is_finite() {
        (lo, hi)
    } else {
        (Vec3::splat(-10.0), Vec3::splat(10.0))
    };
    let centre = (lo + hi) * 0.5;
    let size = hi - lo;

    // No bevy lights: the world carries Halo's baked lighting
    // (`world/lighting.rs`), the sky its own, the collision shell is unlit.

    // Start at the first spawn point at Spartan eye height, facing the way
    // it faces; campaign maps have no spawn points but the scenario's
    // player starting locations; without either, above the middle of the
    // world.
    let start = scene.starts.first().filter(|_| scene.spawns.is_empty());
    let (cam, transform) = match (scene.spawns.first(), start) {
        (None, Some(s)) => {
            let eye = to_render(s.position + Vec3::Z * 0.6);
            let facing = Vec3::new(s.yaw.cos() * s.pitch.cos(), s.yaw.sin() * s.pitch.cos(), s.pitch.sin());
            info!(
                "start at the player starting location: halo [{:.2}, {:.2}, {:.2}] yaw {:.0} deg",
                s.position.x,
                s.position.y,
                s.position.z,
                s.yaw.to_degrees()
            );
            FlyCamera::looking_at(eye, eye + to_render_dir(facing))
        }
        (Some(spawn), _) => {
            let eye = to_render(spawn.position + Vec3::Z * 0.6);
            let target = eye + to_render_dir(spawn.facing());
            info!(
                "start at {}: halo [{:.2}, {:.2}, {:.2}] yaw {:.0} deg ({} spawn points)",
                spawn.name,
                spawn.position.x,
                spawn.position.y,
                spawn.position.z,
                spawn.yaw.to_degrees(),
                scene.spawns.len()
            );
            FlyCamera::looking_at(eye, target)
        }
        (None, None) => {
            let eye = centre + Vec3::new(0.0, size.y * 0.6 + 20.0, size.z * 0.35 + 20.0);
            FlyCamera::looking_at(eye, centre)
        }
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: 75f32.to_radians(),
            near: 0.1,
            // Skies reach tens of kilometres out (the Ark on 100_the_ark).
            far: 200_000.0,
            ..default()
        }),
        transform,
        cam,
    ));
    commands.insert_resource(lighting::SceneLights(surface));
    // Later zone sets decode their own lightmaps and bitmaps with their
    // units: the start's CPU copies can go.
    let mut source = source;
    if source.input.cache.is_some() {
        source.lightmaps.clear();
        source.bitmaps.clear();
    }
    commands.insert_resource(source);
}

/// Builds the collision shell the first time V asks for it (120_halo's is
/// a million triangles: not worth the GPU memory until wanted).
fn spawn_collision_on_demand(
    mut commands: Commands,
    show: Res<ShowCollision>,
    source: Option<Res<stream::WorldSource>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut done: Local<bool>,
) {
    if *done || !show.0 {
        return;
    }
    let Some(source) = source else {
        return;
    };
    *done = true;
    spawn_collision(&mut commands, &mut meshes, &mut materials, &source.input.scene);
}

/// A small made-up scene for checking the camera before any map decodes: a
/// floor, a ramp and a few boxes, in Halo units.
pub fn test_scene() -> Scene {
    use blam_geometry::{Material, Mesh as GMesh, Part, Placement};

    fn quad(mesh: &mut GMesh, a: Vec3, b: Vec3, c: Vec3, d: Vec3, material: u32) {
        let base = mesh.positions.len() as u32;
        let n = (b - a).cross(c - a).normalize_or(Vec3::Z);
        mesh.positions.extend([a, b, c, d]);
        mesh.normals.extend([n; 4]);
        mesh.uvs.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        let part = match mesh.parts.iter_mut().find(|p| p.material == material) {
            Some(p) => p,
            None => {
                mesh.parts.push(Part {
                    material,
                    indices: Vec::new(),
                });
                mesh.parts.last_mut().unwrap()
            }
        };
        part.indices
            .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    fn cube(mesh: &mut GMesh, lo: Vec3, hi: Vec3, material: u32) {
        let v = |x: bool, y: bool, z: bool| {
            Vec3::new(
                if x { hi.x } else { lo.x },
                if y { hi.y } else { lo.y },
                if z { hi.z } else { lo.z },
            )
        };
        let (f, t) = (false, true);
        quad(mesh, v(f, f, t), v(t, f, t), v(t, t, t), v(f, t, t), material); // top
        quad(mesh, v(f, t, f), v(t, t, f), v(t, f, f), v(f, f, f), material); // bottom
        quad(mesh, v(f, f, f), v(t, f, f), v(t, f, t), v(f, f, t), material); // -y
        quad(mesh, v(t, t, f), v(f, t, f), v(f, t, t), v(t, t, t), material); // +y
        quad(mesh, v(f, t, f), v(f, f, f), v(f, f, t), v(f, t, t), material); // -x
        quad(mesh, v(t, f, f), v(t, t, f), v(t, t, t), v(t, f, t), material); // +x
    }

    let mut floor = GMesh {
        name: "floor".into(),
        ..default()
    };
    quad(
        &mut floor,
        Vec3::new(-20.0, -20.0, 0.0),
        Vec3::new(20.0, -20.0, 0.0),
        Vec3::new(20.0, 20.0, 0.0),
        Vec3::new(-20.0, 20.0, 0.0),
        0,
    );
    // A ramp rising toward +X.
    quad(
        &mut floor,
        Vec3::new(4.0, -3.0, 0.0),
        Vec3::new(10.0, -3.0, 2.0),
        Vec3::new(10.0, 3.0, 2.0),
        Vec3::new(4.0, 3.0, 0.0),
        1,
    );
    let mut box_mesh = GMesh {
        name: "box".into(),
        ..default()
    };
    cube(&mut box_mesh, Vec3::new(-0.5, -0.5, 0.0), Vec3::new(0.5, 0.5, 1.0), 2);

    let mut scene = Scene {
        name: "test scene".into(),
        materials: vec![
            Material {
                tag_path: "test\\ground".into(),
                shader_class: "rmtr".into(),
                shading: Shading::Terrain,
                two_sided: false,
                shader_tag: None,
            },
            Material {
                tag_path: "test\\ramp".into(),
                shader_class: "rmsh".into(),
                shading: Shading::Opaque,
                two_sided: false,
                shader_tag: None,
            },
            Material {
                tag_path: "test\\crate".into(),
                shader_class: "rmsh".into(),
                shading: Shading::Opaque,
                two_sided: false,
                shader_tag: None,
            },
        ],
        meshes: vec![floor, box_mesh],
        placements: vec![Placement {
            name: "floor".into(),
            kind: Kind::Cluster,
            mesh: 0,
            transform: Mat4::IDENTITY,
            mirrored: false,
            lighting: Default::default(),
            bsps: 0,
            sky: None,
            cluster: None,
            fade_pixels: [0; 2],
        }],
        bounds: Some((Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 3.0))),
        collision: Default::default(),
        ..Default::default()
    };
    for i in 0..12 {
        let a = i as f32 * 0.52;
        let r = 6.0 + (i % 3) as f32 * 3.0;
        let s = 0.7 + (i % 4) as f32 * 0.4;
        scene.placements.push(Placement {
            name: format!("box {i}"),
            kind: Kind::Instance,
            mesh: 1,
            transform: Mat4::from_scale_rotation_translation(
                Vec3::splat(s),
                Quat::from_rotation_z(a),
                Vec3::new(a.cos() * r, a.sin() * r, 0.0),
            ),
            mirrored: false,
            lighting: Default::default(),
            bsps: 0,
            sky: None,
            cluster: None,
            fade_pixels: [0; 2],
        });
    }
    scene
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bytes wgpu will slice out of `Image::data` for a full chain: it
    /// panics on the render thread when the data is shorter.
    fn wgpu_chain_len(image: &Image) -> usize {
        let d = &image.texture_descriptor;
        let (bw, bh) = d.format.block_dimensions();
        let bytes = d.format.block_copy_size(None).unwrap() as usize;
        (0..d.mip_level_count)
            .map(|l| {
                let w = (d.size.width >> l).max(1).div_ceil(bw) as usize;
                let h = (d.size.height >> l).max(1).div_ceil(bh) as usize;
                w * h * bytes
            })
            .sum()
    }

    #[test]
    fn spawn_markers_are_not_drawn() {
        for hidden in ["respawn_point 12", "slayer_initial_spawn_point 3", "ctf_flag_at_home_respawn_zone 0", "koth_hill_static 1"] {
            assert!(is_editor_only(hidden), "{hidden}");
        }
        for drawn in ["fusion_coil 2", "box_l 0", "warthog", "spawn point lamp"] {
            assert!(!is_editor_only(drawn), "{drawn}");
        }
    }

    #[test]
    fn riverworld_images_have_exact_lengths() {
        let dir = std::env::var_os("HALO3_MAPS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(halo3_core::DEFAULT_MAPS));
        let path = dir.join("riverworld.map");
        if !path.exists() {
            return;
        }
        let cache = blam_cache::CacheFile::open(path).unwrap();
        let mut scene = Scene::default();
        let options = blam_geometry::Options {
            collision: false,
            ..Default::default()
        };
        blam_geometry::decode_into(&cache, &options, &mut scene).unwrap();
        let textures = halo3_textures::load(&cache, &scene);
        assert!(textures.bitmaps.len() >= 50, "{} bitmaps", textures.bitmaps.len());
        let textured = textures.looks.iter().flatten().filter(|l| l.texture.is_some()).count();
        assert!(textured >= 60, "{textured} textured materials");
        // Valhalla's main ground blends all four layers, each with a detail
        // map; the snow ground has layer 0 off.
        let terrain_of = |path: &str| {
            let i = scene.materials.iter().position(|m| m.tag_path.ends_with(path)).unwrap();
            textures.looks[i].as_ref().unwrap().terrain.clone().unwrap()
        };
        let ground = terrain_of("shaders\\riverworld_ground");
        assert!(ground.layers.iter().all(|l| l.as_ref().is_some_and(|l| l.detail.is_some())));
        assert_eq!(ground.layers[0].as_ref().unwrap().base_xform.scale, [40.0, 80.0]);
        let snow = terrain_of("shaders\\riverworld_ground_snow");
        assert!(snow.layers[0].is_none() && snow.layers[1..].iter().all(Option::is_some));
        let mut gpu_bytes = [0usize; 2];
        for t in textures.bitmaps.values() {
            for bc in [true, false] {
                let image = texture_image_as(t, bc, true).unwrap();
                let len = image.data.as_ref().unwrap().len();
                gpu_bytes[bc as usize] += len;
                assert_eq!(len, wgpu_chain_len(&image), "{} (bc {bc})", t.name);
                assert!(image.texture_descriptor.format.is_srgb());
                if !bc {
                    assert!(!image.texture_descriptor.format.is_compressed());
                }
            }
        }
        eprintln!(
            "riverworld: {} images for {textured} materials; {:.1} MiB with BC, {:.1} MiB as RGBA8",
            textures.bitmaps.len(),
            gpu_bytes[1] as f64 / (1 << 20) as f64,
            gpu_bytes[0] as f64 / (1 << 20) as f64
        );
    }
}
