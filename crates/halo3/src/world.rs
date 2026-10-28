//! Puts a decoded [`Scene`] on screen: one bevy mesh per material, in
//! metres, Y up.
//!
//! Halo is Z up and measures in world units of ten feet. The renderer frame
//! is `(x, y, z) = 3.048 * (hx, hz, -hy)`: a rotation, not a mirror, the
//! same mapping HaloExport uses for its glTF files.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::image::{
    CompressedImageFormatSupport, ImageAddressMode, ImageFilterMode, ImageSampler,
    ImageSamplerDescriptor,
};
use bevy::light::NotShadowCaster;
use bevy::math::{Affine2, Mat2};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use blam_geometry::{Kind, Lighting, Scene, Shading, WORLD_UNIT_METRES};
use blam_texture::{Blend, Format, Texture, rgba};

use crate::fly_camera::FlyCamera;
use crate::textures::{Look, MaterialTextures};

pub mod lighting;

use lighting::{Atlas, HaloLightmap, HaloMaterial, SurfaceLights};

/// Halo world units, Z up, to renderer metres, Y up.
pub fn to_render(h: Vec3) -> Vec3 {
    Vec3::new(h.x, h.z, -h.y) * WORLD_UNIT_METRES
}

/// Renderer metres, Y up, to Halo world units, Z up.
pub fn to_halo(r: Vec3) -> Vec3 {
    Vec3::new(r.x, -r.z, r.y) / WORLD_UNIT_METRES
}

pub fn to_halo_dir(r: Vec3) -> Vec3 {
    Vec3::new(r.x, -r.z, r.y)
}

pub fn to_render_dir(h: Vec3) -> Vec3 {
    Vec3::new(h.x, h.z, -h.y)
}

/// The scene and its decoded textures, handed from `main` to the startup
/// system.
#[derive(Resource)]
pub struct PendingScene(pub Option<Scene>, pub Option<MaterialTextures>);

pub struct WorldPlugin;

impl Plugin for WorldPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(lighting::LightingPlugin)
            .insert_resource(ClearColor(Color::srgb(0.53, 0.70, 0.90)))
            .add_systems(Startup, spawn_world)
            .add_systems(Update, toggle_collision);
    }
}

/// The game's collision drawn as a translucent shell; V shows and hides it.
#[derive(Component)]
pub struct CollisionOverlay;

fn toggle_collision(
    keys: Res<ButtonInput<KeyCode>>,
    mut overlays: Query<&mut Visibility, With<CollisionOverlay>>,
) {
    if !keys.just_pressed(KeyCode::KeyV) {
        return;
    }
    for mut v in &mut overlays {
        *v = match *v {
            Visibility::Hidden => Visibility::Visible,
            _ => Visibility::Hidden,
        };
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
    // Two shells: ordinary surfaces in green, invisible walls in red.
    for (invisible, colour) in [(false, Color::srgba(0.2, 0.9, 0.3, 0.35)), (true, Color::srgba(0.95, 0.25, 0.2, 0.45))] {
        let mut positions: Vec<[f32; 3]> = Vec::new();
        let mut normals: Vec<[f32; 3]> = Vec::new();
        for t in col.triangles.iter().filter(|t| (t.flags & 2 != 0) == invisible) {
            let n = to_render_dir(t.normal()).to_array();
            for p in t.points {
                positions.push(to_render(p).to_array());
                normals.push(n);
            }
        }
        if positions.is_empty() {
            continue;
        }
        let count = positions.len() / 3;
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
            NotShadowCaster,
            Name::new(if invisible { "collision (invisible walls)" } else { "collision" }),
        ));
        info!("collision overlay: {count} {} triangles (press V)", if invisible { "invisible-wall" } else { "surface" });
    }
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

/// A colour texture as a bevy image: layer 0 of every level, largest
/// first. BC1/2/3 stay compressed when the GPU takes BC and the size is a
/// whole number of blocks; A8R8G8B8 goes up as BGRA; everything else is
/// expanded to RGBA8 on the CPU. Always sampled as sRGB: Halo's colour maps
/// are authored that way whatever their curve byte says (the "linear"-curve
/// `mountains_snow` only looks right read as sRGB).
fn texture_image(t: &Texture, bc: bool) -> Result<Image, String> {
    let levels = t.mips.len() as u32;
    if levels == 0 {
        return Err("no levels".into());
    }
    let compressed = match t.format {
        Format::Bc1 => Some(TextureFormat::Bc1RgbaUnormSrgb),
        Format::Bc2 => Some(TextureFormat::Bc2RgbaUnormSrgb),
        Format::Bc3 => Some(TextureFormat::Bc3RgbaUnormSrgb),
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
        (None, Format::A8R8G8B8) => raw(TextureFormat::Bgra8UnormSrgb)?,
        (None, f) => {
            let mut data = Vec::new();
            for l in 0..levels {
                let (w, h) = t.level_dims(l);
                let level = t.level(l, 0).ok_or_else(|| format!("level {l} is short"))?;
                data.extend(rgba::decode_level(f, w, h, level).map_err(|e| e.to_string())?);
            }
            (TextureFormat::Rgba8UnormSrgb, data)
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

/// The `StandardMaterial` for one scene material.
fn standard_material(
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
        alpha_mode,
        ..default()
    }
}

#[derive(Default)]
struct Batch {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    /// Lightmap texcoords (per-pixel lit batches).
    lightmap_uvs: Vec<[f32; 2]>,
    /// Irradiance per vertex (every other batch but the sky).
    colours: Vec<[f32; 4]>,
    indices: Vec<u32>,
    has_normals: bool,
}

/// How a batch is lit: the sky draws unlit; the world by a BSP's lightmap
/// in the shader, or by irradiance baked into its vertex colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum Lit {
    PerPixel(u32),
    PerVertex,
    Sky,
}

/// Where one placement's per-vertex irradiance comes from.
enum VertexLight<'a> {
    PerVertex(&'a [blam_geometry::lightmap::ShLight]),
    Probe(&'a blam_geometry::lightmap::Probe),
    One(blam_geometry::lightmap::ShLight),
}

impl VertexLight<'_> {
    fn irradiance(&self, vertex: usize, n: Vec3) -> Vec3 {
        match self {
            VertexLight::PerVertex(v) => v.get(vertex).map_or(Vec3::ZERO, |l| l.irradiance(n)),
            VertexLight::Probe(p) => p.irradiance(n),
            VertexLight::One(l) => l.irradiance(n),
        }
    }
}

fn spawn_world(
    mut commands: Commands,
    mut pending: ResMut<PendingScene>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut halo_materials: ResMut<Assets<HaloMaterial>>,
    mut images: ResMut<Assets<Image>>,
    compressed: Option<Res<CompressedImageFormatSupport>>,
) {
    let Some(scene) = pending.0.take() else {
        return;
    };
    let textures = pending.1.take().unwrap_or_default();
    // Without the resource we cannot tell; decode on the CPU to be safe.
    let bc = compressed.is_some_and(|c| c.0.supports(TextureFormat::Bc3RgbaUnormSrgb));
    let look_of = |m: u32| textures.looks.get(m as usize).and_then(Option::as_ref);

    // Each BSP's lightmap: two GPU arrays for the shader, and CPU reads for
    // lighting objects by the surface under them.
    let started = std::time::Instant::now();
    let mut lightmap_images: Vec<Option<(Handle<Image>, Handle<Image>)>> = Vec::new();
    let mut atlases: Vec<Option<Atlas>> = Vec::new();
    for (i, lm) in scene.lightmaps.iter().enumerate() {
        let tex = textures.lightmaps.get(i).and_then(Option::as_ref);
        let gpu = tex.and_then(|t| {
            match (
                lighting::array_image(&t.coefficients, bc),
                lighting::array_image(&t.intensity, bc),
            ) {
                (Ok(a), Ok(b)) => Some((images.add(a), images.add(b))),
                (Err(e), _) | (_, Err(e)) => {
                    warn!("lightmap of bsp {}: {e}; vertex lighting there", lm.bsp);
                    None
                }
            }
        });
        atlases.push(tex.filter(|_| gpu.is_some()).map(|t| Atlas::new(t, lm.scales)));
        lightmap_images.push(gpu);
    }
    let surface = SurfaceLights::build(&scene, &atlases);
    info!(
        "lighting: {} of {} lightmaps on the GPU, surface lights sampled in {:.2} s{}",
        lightmap_images.iter().flatten().count(),
        scene.lightmaps.len(),
        started.elapsed().as_secs_f32(),
        if surface.is_empty() { " (none: default sun)" } else { "" }
    );
    let exposure = lighting::exposure();

    // One batch per material, every placement baked into world space.
    // Keyed by (sky, material): the sky draws unlit, so it never shares a
    // batch with the world even when the shader is the same.
    let mut batches: HashMap<(Lit, u32), Batch> = HashMap::new();
    let mut lo = Vec3::splat(f32::INFINITY);
    let mut hi = Vec3::splat(f32::NEG_INFINITY);
    let mut lit_counts: HashMap<&str, usize> = HashMap::new();
    for placement in &scene.placements {
        let sky = placement.kind == Kind::Sky;
        let mesh = &scene.meshes[placement.mesh as usize];
        let m = placement.transform;
        let normal_m = Mat3::from_mat4(m).inverse().transpose();
        let has_normals = mesh.normals.len() == mesh.positions.len();
        // Per-pixel when the lightmap made it to the GPU; otherwise the
        // placement's own per-vertex data or probe; otherwise the lit
        // surface nearest its base.
        let (lit, lightmap_uvs, vertex_light) = match &placement.lighting {
            _ if sky => (Lit::Sky, None, None),
            Lighting::PerPixel { lightmap, uvs }
                if lightmap_images
                    .get(*lightmap as usize)
                    .is_some_and(Option::is_some) =>
            {
                (Lit::PerPixel(*lightmap), Some(uvs), None)
            }
            Lighting::PerVertex(v) => (Lit::PerVertex, None, Some(VertexLight::PerVertex(v))),
            Lighting::Probe(p) => (Lit::PerVertex, None, Some(VertexLight::Probe(p))),
            _ => {
                let mut a = Vec3::splat(f32::INFINITY);
                let mut b = Vec3::splat(f32::NEG_INFINITY);
                for p in &mesh.positions {
                    let w = m.transform_point3(*p);
                    a = a.min(w);
                    b = b.max(w);
                }
                let base = if a.x.is_finite() {
                    Vec3::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5, a.z + 0.1)
                } else {
                    m.w_axis.truncate()
                };
                let light = if surface.is_empty() {
                    lighting::default_light()
                } else {
                    surface.near(base)
                };
                (Lit::PerVertex, None, Some(VertexLight::One(light)))
            }
        };
        *lit_counts
            .entry(match (&lit, &vertex_light) {
                (Lit::Sky, _) => "sky",
                (Lit::PerPixel(_), _) => "per-pixel",
                (_, Some(VertexLight::PerVertex(_))) => "per-vertex",
                (_, Some(VertexLight::Probe(_))) => "probe",
                _ => "nearest surface",
            })
            .or_default() += 1;
        for part in &mesh.parts {
            let shading = scene
                .materials
                .get(part.material as usize)
                .map_or(Shading::Opaque, |mat| mat.shading);
            let hidden = look_of(part.material).is_some_and(|l| l.hidden);
            if shading == Shading::Invisible || hidden || part.indices.is_empty() {
                continue;
            }
            let batch = batches.entry((lit, part.material)).or_insert_with(|| Batch {
                has_normals,
                ..Default::default()
            });
            batch.has_normals &= has_normals;
            // Re-index this part's vertices into the batch, winding mirrored
            // placements the other way round.
            let mut remap: HashMap<u32, u32> = HashMap::new();
            let order: &[usize] = if placement.mirrored { &[0, 2, 1] } else { &[0, 1, 2] };
            for &i in part
                .indices
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|t| order.iter().map(move |&k| &t[k]))
            {
                let j = *remap.entry(i).or_insert_with(|| {
                    let p = to_render(m.transform_point3(mesh.positions[i as usize]));
                    if !sky {
                        lo = lo.min(p);
                        hi = hi.max(p);
                    }
                    batch.positions.push(p.to_array());
                    let halo_n = mesh
                        .normals
                        .get(i as usize)
                        .map(|n| (normal_m * *n).normalize_or(Vec3::Z))
                        .unwrap_or(Vec3::Z);
                    batch.normals.push(to_render_dir(halo_n).to_array());
                    if let Some(uvs) = lightmap_uvs {
                        batch
                            .lightmap_uvs
                            .push(uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                    }
                    if let Some(l) = &vertex_light {
                        let e = l.irradiance(i as usize, halo_n);
                        batch.colours.push([e.x, e.y, e.z, 1.0]);
                    }
                    batch
                        .uvs
                        .push(mesh.uvs.get(i as usize).copied().unwrap_or([0.0, 0.0]));
                    (batch.positions.len() - 1) as u32
                });
                batch.indices.push(j);
            }
        }
    }

    // Each bitmap becomes one image, shared by every material using it.
    let mut handles: HashMap<u32, Option<Handle<Image>>> = HashMap::new();
    let (mut uploaded, mut uploaded_bytes, mut kept_bc, mut textured) = (0usize, 0usize, 0usize, 0usize);
    let mut image_for = |bitmap: u32, images: &mut Assets<Image>| -> Option<Handle<Image>> {
        handles
            .entry(bitmap)
            .or_insert_with(|| {
                let t = textures.bitmaps.get(&bitmap)?;
                match texture_image(t, bc) {
                    Ok(image) => {
                        uploaded += 1;
                        uploaded_bytes += image.data.as_ref().map_or(0, Vec::len);
                        if image.texture_descriptor.format.is_compressed() {
                            kept_bc += 1;
                        }
                        Some(images.add(image))
                    }
                    Err(e) => {
                        warn!("texture {}: {e}; flat colour instead", t.name);
                        None
                    }
                }
            })
            .clone()
    };

    let mut triangles = 0usize;
    let mut keys: Vec<(Lit, u32)> = batches.keys().copied().collect();
    keys.sort_unstable();
    for (lit, key) in keys {
        let sky = lit == Lit::Sky;
        let batch = batches.remove(&(lit, key)).unwrap();
        triangles += batch.indices.len() / 3;
        let material = scene.materials.get(key as usize);
        let name = material.map_or("unknown", |m| m.tag_path.as_str());
        let look = look_of(key);

        let mut mesh = Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        );
        mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, batch.positions);
        mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, batch.uvs);
        if !batch.lightmap_uvs.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, batch.lightmap_uvs);
        }
        if !batch.colours.is_empty() {
            mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, batch.colours);
        }
        mesh.insert_indices(Indices::U32(batch.indices));
        if batch.has_normals {
            mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, batch.normals);
        } else {
            mesh.compute_normals();
        }

        let texture = look
            .and_then(|l| l.texture)
            .and_then(|b| image_for(b, &mut images));
        textured += texture.is_some() as usize;
        let mut std_material = standard_material(name, material, look, texture);
        // Halo's sky carries its own light: draw its textures as they are.
        std_material.unlit = sky;
        let translucent =
            sky || !matches!(std_material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_));
        let mut entity = commands.spawn((Mesh3d(meshes.add(mesh)), Name::new(name.to_string())));
        if sky {
            entity.insert(MeshMaterial3d(materials.add(std_material)));
        } else {
            let (scales, arrays) = match lit {
                Lit::PerPixel(l) => (
                    scene.lightmaps[l as usize].scales,
                    lightmap_images[l as usize].clone(),
                ),
                _ => ([0.0; 5], None),
            };
            let (coefficients, intensity) = arrays.map_or((None, None), |(a, b)| (Some(a), Some(b)));
            entity.insert(MeshMaterial3d(halo_materials.add(HaloMaterial {
                base: std_material,
                extension: HaloLightmap {
                    scales: Vec4::from_slice(&scales[..4]),
                    extra: Vec4::new(scales[4], exposure, 0.0, 0.0),
                    coefficients,
                    intensity,
                },
            })));
        }
        if translucent {
            entity.insert(NotShadowCaster);
        }
    }

    info!(
        "textures: {textured} materials textured, {uploaded} images ({kept_bc} kept BC-compressed, BC support {bc}), {:.1} MiB to the GPU",
        uploaded_bytes as f64 / (1 << 20) as f64
    );

    let (lo, hi) = if lo.x.is_finite() {
        (lo, hi)
    } else {
        (Vec3::splat(-10.0), Vec3::splat(10.0))
    };
    let centre = (lo + hi) * 0.5;
    let size = hi - lo;
    info!(
        "world: {} materials, {triangles} triangles, {:.0} x {:.0} x {:.0} m",
        batches.len().max(scene.materials.len()),
        size.x,
        size.y,
        size.z
    );

    let mut counts: Vec<_> = lit_counts.into_iter().collect();
    counts.sort_unstable();
    info!("lit placements: {counts:?}");

    spawn_collision(&mut commands, &mut meshes, &mut materials, &scene);

    // No bevy lights: the world carries Halo's baked lighting
    // (`world/lighting.rs`), the sky its own, the collision shell is unlit.

    // Start at the first spawn point at Spartan eye height, facing the way
    // it faces; without spawns, above the middle of the world.
    let (cam, transform) = match scene.spawns.first() {
        Some(spawn) => {
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
        None => {
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
        }],
        bounds: Some((Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, 20.0, 3.0))),
        collision: Default::default(),
        spawns: Vec::new(),
        lightmaps: Vec::new(),
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
    fn riverworld_images_have_exact_lengths() {
        let dir = std::env::var_os("HALO3_MAPS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::path::PathBuf::from(crate::DEFAULT_MAPS));
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
        let textures = crate::textures::load(&cache, &scene);
        assert!(textures.bitmaps.len() >= 50, "{} bitmaps", textures.bitmaps.len());
        let textured = textures.looks.iter().flatten().filter(|l| l.texture.is_some()).count();
        assert!(textured >= 60, "{textured} textured materials");
        let mut gpu_bytes = [0usize; 2];
        for t in textures.bitmaps.values() {
            for bc in [true, false] {
                let image = texture_image(t, bc).unwrap();
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
