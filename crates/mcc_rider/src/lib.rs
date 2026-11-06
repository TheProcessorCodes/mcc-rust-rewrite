//! The rider drawn from the skate engine's pose: the map's own Master Chief
//! (the multiplayer Spartan) standing on the Skate 3 board.
//!
//! The board is the skater's board from the user's converted Skate 3 data
//! (`board.json` and `rig.json`, which IW4L derives from `skater.glb`; see
//! `docs/skate-interface.md` section 7). Its joints follow the engine's
//! board bones exactly as IW4L skins it: joint `j` sits at
//! `pose.bones[j] * Rb`, and bevy applies the GLB's inverse bind.
//!
//! The Spartan is retargeted the way IW4L retargets the MW2 soldier
//! (`rig.rs::pose`, adapted, Apache-2.0): each mapped Halo bone is moved
//! rigidly so its bind joint lands on the skater's bind joint and its bone
//! segment points along the skater's, then follows the skater bone's
//! solved pose. Fingers inherit the hand. Table and model: `docs/rider.md`.
//!
//! Frames: the skate engine works in metres, Y up; the renderer is the same
//! frame divided by `k` (`skate.rs`). The Spartan's bind pose is in Halo
//! model units, Z up; `halo_to_skate` takes it into skate metres, where the
//! Spartan (2.14 m real) comes out the skater's 1.8 m.

use std::collections::HashMap;
use std::path::Path;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::math::{Affine2, Affine3A, Mat2};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use mcc_core::map::MapAppExt;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use serde::Deserialize;

use blam_cache::CacheFile;
use blam_geometry::render_model::ModelParts;
use blam_geometry::{Scene, WORLD_UNIT_METRES};
use blam_texture::{Texture, rgba};

use mcc_core::coords::to_halo;
use mcc_core::skate::{RiderPose, SkateEnabled, assets_root, skating_on};
use mcc_textures::MaterialTextures;
use mcc_world::lighting::{SceneLights, default_light, exposure};

/// Render model tags to try, multiplayer Spartan first.
const CHIEF_TAGS: [&str; 2] = [
    r"objects\characters\masterchief\mp_masterchief\mp_masterchief",
    r"objects\characters\masterchief\masterchief",
];

/// The Blender bone basis the GLB carries (`skate_board.rs`): columns
/// `X, -Z, Y`. A pose bone times this is the GLB joint's world frame.
const RB: Mat4 = Mat4::from_cols(Vec4::X, Vec4::NEG_Z, Vec4::Y, Vec4::W);

/// Halo bone -> skate bone, and the child pair whose segment is aligned.
/// Halo has two spine bones to the skater's four: `spine1` carries the
/// chest and sits where SPINE2 does, proportionally.
/// A Halo bone, the skate bone it follows, and the (Halo, skate) child pair.
type Mapping = (&'static str, &'static str, Option<(&'static str, &'static str)>);

const MAPPING: [Mapping; 21] = [
    ("pelvis", "HIPS", Some(("spine", "SPINE"))),
    ("spine", "SPINE", Some(("spine1", "SPINE2"))),
    ("spine1", "SPINE2", Some(("neck", "NECK"))),
    ("neck", "NECK", Some(("head", "HEAD"))),
    ("head", "HEAD", None),
    ("l_clavicle", "LEFTSHOULDER", Some(("l_upperarm", "LEFTARM"))),
    ("l_upperarm", "LEFTARM", Some(("l_forearm", "LEFTFOREARM"))),
    ("l_forearm", "LEFTFOREARM", Some(("l_hand", "LEFTHAND"))),
    ("l_hand", "LEFTHAND", None),
    ("r_clavicle", "RIGHTSHOULDER", Some(("r_upperarm", "RIGHTARM"))),
    ("r_upperarm", "RIGHTARM", Some(("r_forearm", "RIGHTFOREARM"))),
    ("r_forearm", "RIGHTFOREARM", Some(("r_hand", "RIGHTHAND"))),
    ("r_hand", "RIGHTHAND", None),
    ("l_thigh", "LEFTUPLEG", Some(("l_calf", "LEFTLEG"))),
    ("l_calf", "LEFTLEG", Some(("l_foot", "LEFTFOOT"))),
    ("l_foot", "LEFTFOOT", Some(("l_toe", "LEFTTOEBASE"))),
    ("l_toe", "LEFTTOEBASE", None),
    ("r_thigh", "RIGHTUPLEG", Some(("r_calf", "RIGHTLEG"))),
    ("r_calf", "RIGHTLEG", Some(("r_foot", "RIGHTFOOT"))),
    ("r_foot", "RIGHTFOOT", Some(("r_toe", "RIGHTTOEBASE"))),
    ("r_toe", "RIGHTTOEBASE", None),
];

/// The Spartan decoded from the map, CPU side, handed over by `main`.
pub struct ChiefModel {
    scene: Scene,
    parts: ModelParts,
    textures: MaterialTextures,
    /// Colour maps with the armour colour baked in, by scene material:
    /// width, height, sRGB RGBA8.
    tinted: HashMap<u32, (u32, u32, Vec<u8>)>,
    /// The same in the red team's colours, for the enemy Spartan.
    tinted_red: HashMap<u32, (u32, u32, Vec<u8>)>,
}

/// Master Chief's olive drab, the primary and secondary change colours
/// (sRGB). `MCC_RIDER_COLOUR=r,g,b` overrides the primary.
const PRIMARY: [f32; 3] = [0.34, 0.40, 0.21];
const SECONDARY: [f32; 3] = [0.27, 0.31, 0.17];

/// The red team's primary and secondary colours (sRGB): the enemy Spartan.
const RED: [[f32; 3]; 2] = [[0.62, 0.07, 0.06], [0.36, 0.05, 0.04]];

fn primary_colour() -> [f32; 3] {
    std::env::var("MCC_RIDER_COLOUR")
        .ok()
        .and_then(|s| {
            let v: Vec<f32> = s.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            <[f32; 3]>::try_from(v).ok()
        })
        .unwrap_or(PRIMARY)
}

/// Bakes a `two_change_color` shader's tint into its colour map:
/// `base * lerp(1, primary, cc.r) * lerp(1, secondary, cc.g)`, the change
/// map sampled nearest at the base map's size.
fn bake_change_colour(
    cache: &CacheFile,
    resources: &blam_cache::Resources,
    shader_tag: u32,
    base: &Texture,
    [primary, secondary]: [[f32; 3]; 2],
) -> Option<(u32, u32, Vec<u8>)> {
    let s = blam_texture::shader::resolve(cache, shader_tag).ok()?;
    let albedo = s.options.iter().find(|(k, _)| k == "albedo")?;
    if !albedo.1.contains("change_color") {
        return None;
    }
    let cc_tag = s.maps.iter().find(|m| m.usage == "change_color_map")?.bitmap?;
    let cc = blam_texture::bitmap::decode(cache, resources, cc_tag, 0).ok()?;
    let cc_px = rgba::decode_level(cc.format, cc.width, cc.height, cc.level(0, 0)?).ok()?;
    let mut px = rgba::decode_level(base.format, base.width, base.height, base.level(0, 0)?).ok()?;
    let (w, h) = (base.width, base.height);
    for y in 0..h {
        for x in 0..w {
            let cx = (x as u64 * cc.width as u64 / w as u64) as usize;
            let cy = (y as u64 * cc.height as u64 / h as u64) as usize;
            let c = (cy * cc.width as usize + cx) * 4;
            let (r, g) = (cc_px[c] as f32 / 255.0, cc_px[c + 1] as f32 / 255.0);
            let p = ((y * w + x) * 4) as usize;
            for ch in 0..3 {
                let tint = (1.0 - r + r * primary[ch]) * (1.0 - g + g * secondary[ch]);
                px[p + ch] = (px[p + ch] as f32 * tint).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    Some((w, h, px))
}

#[derive(Resource)]
pub struct PendingChief(pub Option<ChiefModel>);

/// Decodes the map's Spartan and its colour maps. `None` (logged) when the
/// map has none, e.g. most campaign maps carry only `masterchief`.
pub fn load_chief(cache: &CacheFile) -> Option<ChiefModel> {
    let mode = CHIEF_TAGS.iter().find_map(|name| cache.tags.find("mode", name))?;
    let resources = cache.resources().ok()?;
    let mut materials = blam_geometry::materials::MaterialTable::default();
    let mut scene = Scene::default();
    // The Mark VI look: each region's `base` / Mark VI permutation where
    // there is one (the first is not always it; the helmet's is Air
    // Assault on riverworld).
    let pick = |region: &str, perms: &[String]| -> usize {
        let found = perms
            .iter()
            .position(|p| p == "base")
            .or_else(|| perms.iter().position(|p| p.contains("mark_vi")))
            .unwrap_or(0);
        if std::env::var_os("MCC_RIDER_PERMUTATIONS").is_some() {
            eprintln!("rider: region {region}: {perms:?} -> {:?}", perms.get(found));
        }
        found
    };
    let parts = match blam_geometry::render_model::decode_with(
        cache,
        &resources,
        &mut materials,
        mode,
        &mut scene,
        &pick,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("rider: {} not decoded: {e}", mode.name());
            return None;
        }
    };
    drop(resources);
    // Only the materials the default permutation draws.
    let used: std::collections::HashSet<u32> =
        scene.meshes.iter().flat_map(|m| m.parts.iter().map(|p| p.material)).collect();
    let all = scene.materials.clone();
    for (i, m) in scene.materials.iter_mut().enumerate() {
        if !used.contains(&(i as u32)) {
            m.shader_tag = None;
        }
    }
    let textures = mcc_textures::load(cache, &scene);
    scene.materials = all;
    let mut tinted = HashMap::new();
    let mut tinted_red = HashMap::new();
    if let Ok(resources) = cache.resources() {
        for &i in &used {
            let base = textures.looks.get(i as usize).and_then(Option::as_ref).and_then(|l| l.texture);
            let shader = scene.materials.get(i as usize).and_then(|m| m.shader_tag);
            if let (Some(base), Some(shader)) = (base, shader)
                && let Some(t) = textures.bitmaps.get(&base)
                && let Some(baked) = bake_change_colour(cache, &resources, shader, t, [primary_colour(), SECONDARY])
            {
                tinted.insert(i, baked);
                if let Some(red) = bake_change_colour(cache, &resources, shader, t, RED) {
                    tinted_red.insert(i, red);
                }
            }
        }
        resources.clear_page_cache();
    }
    eprintln!(
        "rider: {} ({} bones, {} pieces, {} colour maps, {} tinted)",
        mode.name(),
        parts.bones.len(),
        parts.pieces.len(),
        textures.bitmaps.len(),
        tinted.len()
    );
    Some(ChiefModel {
        scene,
        parts,
        textures,
        tinted,
        tinted_red,
    })
}

pub struct RiderPlugin;

impl Plugin for RiderPlugin {
    fn build(&self, app: &mut App) {
        app.remove_on_map_unload::<SpartanStatue>()
            .remove_on_map_unload::<Rider>()
            .add_systems(mcc_core::map::MapStartup, spawn_rider)
            .add_systems(
                PostUpdate,
                (pose_rider, light_rider).chain().before(TransformSystems::Propagate).run_if(mcc_core::map::ready),
            );
    }
}

#[derive(Deserialize)]
struct RigBone {
    name: String,
    bind: [f64; 16],
    inverse_bind: [f32; 16],
}

#[derive(Deserialize)]
struct BoardFile {
    joints: Vec<BoardJoint>,
    surfaces: Vec<BoardSurface>,
    textures: Vec<BoardTexture>,
}

#[derive(Deserialize)]
struct BoardJoint {
    target: String,
}

#[derive(Deserialize)]
struct BoardSurface {
    texture: usize,
    vertices: Vec<BoardVertex>,
    indices: Vec<u32>,
}

#[derive(Deserialize)]
struct BoardVertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: u32,
    joints: [u32; 4],
    weights: [f32; 4],
}

#[derive(Deserialize)]
struct BoardTexture {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// One joint entity and how to pose it: world = `S * pose.bones[target] * offset`.
struct Joint {
    entity: Entity,
    target: String,
    offset: Mat4,
    /// Keep the Spartan's own bone length: the joint sits at this offset
    /// (Halo model units) in the frame of an earlier joint, by index into
    /// `Rider::joints`, and takes only its rotation from the skate bone.
    attach: Option<(usize, Vec3)>,
}

#[derive(Resource, Default)]
struct Rider {
    joints: Vec<Joint>,
    meshes: Vec<Entity>,
    /// The skateboard's meshes, hidden while the rider is on the dirt bike.
    board: Vec<Entity>,
    /// Every rider material with its unlit colour, scaled by the light
    /// where the rider is.
    materials: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    /// Where the light was last sampled (Halo units).
    lit_at: Option<Vec3>,
    /// Whether the rider, and the board, are showing.
    shown: (bool, bool),
}

/// Bones that keep the Spartan's own offset from their parent instead of
/// jumping to the skater's joint: the skater's neck is longer and bends
/// with SPINE3, which left the helmet floating above the collar.
const ATTACHED: [&str; 2] = ["neck", "head"];

/// Feet keep the bind pose's flat sole instead of aligning heel to toe:
/// the Spartan's ankle-to-toe line is far steeper than the skater's, so
/// aligning it tipped the toes up.
const FLAT_FEET: [&str; 4] = ["l_foot", "r_foot", "l_toe", "r_toe"];

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Halo model units, Z up, to skate metres, Y up, at the skate scale `k`.
fn halo_to_skate(k: f32) -> Mat4 {
    Mat4::from_scale(Vec3::splat(WORLD_UNIT_METRES * k))
        * Mat4::from_cols(Vec4::X, Vec4::NEG_Z, Vec4::Y, Vec4::W)
}

pub(crate) fn sampler(repeat: bool) -> ImageSampler {
    let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: mode,
        address_mode_v: mode,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}

/// A Halo colour map as an sRGB RGBA8 image with its mip chain.
pub fn halo_image(t: &Texture) -> Option<Image> {
    let levels = t.mips.len() as u32;
    let mut data = Vec::new();
    for l in 0..levels {
        let (w, h) = t.level_dims(l);
        data.extend(rgba::decode_level(t.format, w, h, t.level(l, 0)?).ok()?);
    }
    let mut image = Image::new_uninit(
        Extent3d {
            width: t.width,
            height: t.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.mip_level_count = levels.max(1);
    image.data = Some(data);
    image.sampler = sampler(true);
    Some(image)
}

fn skinned_mesh(
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    uvs: Vec<[f32; 2]>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
    mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    mesh.insert_indices(Indices::U32(indices));
    match normals {
        Some(n) => mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n),
        None => mesh.compute_normals(),
    }
    mesh
}

#[allow(clippy::too_many_arguments)]
fn spawn_rider(
    mut commands: Commands,
    pending: Option<ResMut<PendingChief>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut binds: ResMut<Assets<SkinnedMeshInverseBindposes>>,
    mut pose: ResMut<RiderPose>,
    enabled: Option<Res<SkateEnabled>>,
) {
    let chief = pending.and_then(|mut p| p.0.take());
    if let Some(chief) = &chief {
        let statue = build_statue(chief, &mut meshes, &mut materials, &mut images);
        commands.insert_resource(statue);
    }
    let root = assets_root();
    let rig: Vec<RigBone> = match read_json(&root.join("rig.json")) {
        Ok(r) => r,
        Err(e) => {
            warn!("rider: {e}; the rider stays a stick figure");
            return;
        }
    };
    let k = pose.scale;
    let mut rider = Rider::default();
    let hidden = Visibility::Hidden;

    // The board, unless skating is off in Settings.
    let board = skating_on(enabled.as_deref()).then(|| read_json::<BoardFile>(&root.join("board.json")));
    match board {
        None => {}
        Some(Ok(board)) => {
            let mut inverse = Vec::new();
            let mut entities = Vec::new();
            for j in &board.joints {
                let Some(r) = rig.iter().find(|b| b.name == j.target) else {
                    continue;
                };
                let entity = commands.spawn(GlobalTransform::IDENTITY).id();
                rider.joints.push(Joint {
                    entity,
                    target: j.target.clone(),
                    offset: RB,
                    attach: None,
                });
                entities.push(entity);
                inverse.push(Mat4::from_cols_array(&r.inverse_bind));
            }
            if entities.len() == board.joints.len() {
                let inverse = binds.add(SkinnedMeshInverseBindposes::from(inverse));
                let textures: Vec<Handle<Image>> = board
                    .textures
                    .into_iter()
                    .map(|t| {
                        let mut image = Image::new(
                            Extent3d {
                                width: t.width,
                                height: t.height,
                                depth_or_array_layers: 1,
                            },
                            TextureDimension::D2,
                            t.rgba,
                            TextureFormat::Rgba8UnormSrgb,
                            RenderAssetUsages::RENDER_WORLD,
                        );
                        image.sampler = sampler(false);
                        images.add(image)
                    })
                    .collect();
                for s in board.surfaces {
                    let uv = |packed: u32| {
                        [
                            half::f16::from_bits((packed >> 16) as u16).to_f32(),
                            half::f16::from_bits(packed as u16).to_f32(),
                        ]
                    };
                    let v = &s.vertices;
                    // board.json re-winds for IW4 (a, c, b); the renderer
                    // frame is not mirrored, so put the GLB order back.
                    let indices = s
                        .indices
                        .as_chunks::<3>()
                        .0
                        .iter()
                        .flat_map(|&[a, b, c]| [a, c, b])
                        .collect();
                    let mesh = skinned_mesh(
                        v.iter().map(|v| v.position).collect(),
                        Some(v.iter().map(|v| v.normal).collect()),
                        v.iter().map(|v| uv(v.uv)).collect(),
                        v.iter().map(|v| v.joints.map(|j| j as u16)).collect(),
                        v.iter().map(|v| v.weights).collect(),
                        indices,
                    );
                    let material = materials.add(StandardMaterial {
                        base_color_texture: textures.get(s.texture).cloned(),
                        double_sided: true,
                        cull_mode: None,
                        unlit: true,
                        ..default()
                    });
                    rider.materials.push((material.clone(), LinearRgba::WHITE));
                    let e = commands
                        .spawn((
                            Mesh3d(meshes.add(mesh)),
                            MeshMaterial3d(material),
                            SkinnedMesh {
                                inverse_bindposes: inverse.clone(),
                                joints: entities.clone(),
                            },
                            NoFrustumCulling,
                            hidden,
                            Name::new("skate board"),
                        ))
                        .id();
                    rider.board.push(e);
                }
            }
        }
        Some(Err(e)) => warn!("rider: {e}; no board"),
    }

    // The Spartan.
    if let Some(chief) = chief {
        match spawn_chief(
            &mut commands,
            &chief,
            &rig,
            k,
            (&mut meshes, &mut materials, &mut images, &mut binds),
            &mut rider,
        ) {
            Ok(()) => pose.drawn = true,
            Err(e) => warn!("rider: {e}; drawing the stick figure"),
        }
    }
    commands.insert_resource(rider);
}

type Stores<'a> = (
    &'a mut Assets<Mesh>,
    &'a mut Assets<StandardMaterial>,
    &'a mut Assets<Image>,
    &'a mut Assets<SkinnedMeshInverseBindposes>,
);

fn spawn_chief(
    commands: &mut Commands,
    chief: &ChiefModel,
    rig: &[RigBone],
    k: f32,
    (meshes, materials, images, binds): Stores,
    rider: &mut Rider,
) -> Result<(), String> {
    let bones = &chief.parts.bones;
    if bones.is_empty() || bones.len() > 256 {
        return Err(format!("{} bones", bones.len()));
    }
    let h = halo_to_skate(k);
    let index = |name: &str| bones.iter().position(|b| b.name == name);
    let chief_at = |i: usize| h.transform_point3(bones[i].world.w_axis.truncate());
    let skate_bind = |name: &str| -> Option<Mat4> {
        let r = rig.iter().find(|b| b.name == name)?;
        Some(Mat4::from_cols_array(&r.bind.map(|v| v as f32)))
    };
    let skate_at = |name: &str| skate_bind(name).map(|m| m.w_axis.truncate());
    let flat = |v: Vec3| Vec3::new(v.x, 0.0, v.z).try_normalize();

    // Turn the Spartan to face the way the skater faces (heel to toe).
    let facing = (|| {
        let (foot, toe) = (index("l_foot")?, index("l_toe")?);
        let from = flat(chief_at(toe) - chief_at(foot))?;
        let to = flat(skate_at("LEFTTOEBASE")? - skate_at("LEFTFOOT")?)?;
        Some(Quat::from_rotation_arc(from, to))
    })()
    .unwrap_or(Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2));

    // Per bone: the skate bone it follows and `Bk^-1 * fit * H`, the move
    // from Halo model space to the skater's bind space.
    let mut follow: Vec<Option<(String, Mat4)>> = Vec::with_capacity(bones.len());
    for (i, bone) in bones.iter().enumerate() {
        let parent = bone.parent.map(usize::from).filter(|&p| p < i);
        let mapped = MAPPING.iter().find(|m| m.0 == bone.name).and_then(|&(_, skate, child)| {
            let bind = skate_bind(skate)?;
            if FLAT_FEET.contains(&bone.name.as_str()) {
                // The ground point under the ankle onto the skater's,
                // turned only to face the same way: soles flat on the deck.
                // Toes move with their foot.
                let foot = if bone.name.ends_with("toe") { parent? } else { i };
                let ankle = MAPPING.iter().find(|m| m.0 == bones[foot].name)?.1;
                let ground = Vec3::new(1.0, 0.0, 1.0);
                let from = chief_at(foot) * ground;
                let to = skate_at(ankle)? * ground;
                let fit = Mat4::from_rotation_translation(facing, to - facing * from);
                return Some((skate.to_string(), bind.inverse() * fit * h));
            }
            let (from, to) = (chief_at(i), bind.w_axis.truncate());
            let rotation = child
                .and_then(|(halo_child, skate_child)| {
                    let a = (facing * (chief_at(index(halo_child)?) - from)).try_normalize()?;
                    let b = (skate_at(skate_child)? - to).try_normalize()?;
                    Some(Quat::from_rotation_arc(a, b) * facing)
                })
                .or_else(|| {
                    // Terminal bones keep their parent's alignment, so
                    // wrists, toes and the head stay attached.
                    let p = parent?;
                    let (_, parent_skate, _) =
                        MAPPING.iter().find(|m| m.0 == bones[p].name)?;
                    let a = (facing * (from - chief_at(p))).try_normalize()?;
                    let b = (to - skate_at(parent_skate)?).try_normalize()?;
                    Some(Quat::from_rotation_arc(a, b) * facing)
                })
                .unwrap_or(facing);
            let fit = Mat4::from_rotation_translation(rotation, to - rotation * from);
            Some((skate.to_string(), bind.inverse() * fit * h))
        });
        let entry = mapped.or_else(|| parent.and_then(|p| follow[p].clone()));
        follow.push(entry);
    }

    let mut joints = Vec::with_capacity(bones.len());
    let first = rider.joints.len();
    for (bone, f) in bones.iter().zip(&follow) {
        let (target, delta) = f.clone().unwrap_or(("HIPS".into(), h));
        let entity = commands.spawn(GlobalTransform::IDENTITY).id();
        let attach = bone
            .parent
            .map(usize::from)
            .filter(|_| ATTACHED.contains(&bone.name.as_str()))
            .and_then(|p| {
                let parent = bones.get(p)?;
                let local = parent.world.inverse().transform_point3(bone.world.w_axis.truncate());
                Some((first + p, local))
            });
        rider.joints.push(Joint {
            entity,
            target,
            offset: delta * bone.world,
            attach,
        });
        joints.push(entity);
    }
    let inverse = binds.add(SkinnedMeshInverseBindposes::from(
        bones.iter().map(|b| b.world.inverse()).collect::<Vec<_>>(),
    ));

    let mut image_handles: HashMap<u32, Option<Handle<Image>>> = HashMap::new();
    for piece in &chief.parts.pieces {
        let Some(mesh) = chief.scene.meshes.get(piece.mesh as usize) else {
            continue;
        };
        let n = mesh.positions.len();
        let (joint_ids, weights) = match &mesh.skin {
            Some(s) if s.joints.len() == n && s.weights.len() == n => {
                (s.joints.clone(), s.weights.clone())
            }
            _ => {
                let b = piece.bone.unwrap_or(0).min(bones.len() as u16 - 1);
                (vec![[b, 0, 0, 0]; n], vec![[1.0, 0.0, 0.0, 0.0]; n])
            }
        };
        let positions: Vec<[f32; 3]> =
            mesh.positions.iter().map(|p| piece.transform.transform_point3(*p).to_array()).collect();
        let normals = (mesh.normals.len() == n).then(|| {
            mesh.normals
                .iter()
                .map(|v| piece.transform.transform_vector3(*v).normalize_or_zero().to_array())
                .collect::<Vec<_>>()
        });
        let uvs = if mesh.uvs.len() == n { mesh.uvs.clone() } else { vec![[0.0; 2]; n] };
        for part in &mesh.parts {
            let Some((material, base_color)) =
                chief_material(chief, part.material, &chief.tinted, images, materials, &mut image_handles)
            else {
                continue;
            };
            rider.materials.push((material.clone(), base_color.to_linear()));
            let used: Vec<u32> = part.indices.clone();
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(skinned_mesh(
                        positions.clone(),
                        normals.clone(),
                        uvs.clone(),
                        joint_ids.clone(),
                        weights.clone(),
                        used,
                    ))),
                    MeshMaterial3d(material),
                    SkinnedMesh {
                        inverse_bindposes: inverse.clone(),
                        joints: joints.clone(),
                    },
                    NoFrustumCulling,
                    Visibility::Hidden,
                    Name::new(mesh.name.clone()),
                ))
                .id();
            rider.meshes.push(e);
        }
    }
    Ok(())
}

/// A Spartan material, its colour map tinted from `tinted` where it has a
/// change colour. `None` for the parts not drawn (emblem overlays, hidden).
fn chief_material(
    chief: &ChiefModel,
    material: u32,
    tinted: &HashMap<u32, (u32, u32, Vec<u8>)>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    image_handles: &mut HashMap<u32, Option<Handle<Image>>>,
) -> Option<(Handle<StandardMaterial>, Color)> {
    let look = chief.textures.looks.get(material as usize).and_then(Option::as_ref);
    // The personal decals and emblem are overlays coloured by the
    // player's emblem choice; drawn plain they are bright blue
    // patches (their colour map is a change-colour mask).
    let overlay = chief.scene.materials.get(material as usize).is_some_and(|m| {
        m.tag_path.ends_with("mp_personal_decals") || m.tag_path.ends_with("mc_emblem")
    });
    if overlay || look.is_some_and(|l| l.hidden) {
        return None;
    }
    let baked = tinted.get(&material).map(|(w, h, px)| {
        let mut image = Image::new(
            Extent3d {
                width: *w,
                height: *h,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            px.clone(),
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.sampler = sampler(true);
        images.add(image)
    });
    let texture = baked.or_else(|| look.and_then(|l| l.texture).and_then(|b| {
        image_handles
            .entry(b)
            .or_insert_with(|| {
                chief.textures.bitmaps.get(&b).and_then(halo_image).map(|i| images.add(i))
            })
            .clone()
    }));
    let base_color = match (texture.is_some(), look.and_then(|l| l.colour)) {
        (_, Some(c)) => Color::srgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)),
        (true, None) => Color::WHITE,
        // Mark VI olive when the shader gave nothing usable.
        (false, None) => Color::srgb(0.33, 0.37, 0.22),
    };
    let uv_transform = look.map_or(Affine2::IDENTITY, |l| Affine2 {
        matrix2: Mat2::from_diagonal(Vec2::from(l.xform.scale)),
        translation: Vec2::from(l.xform.offset),
    });
    let alpha_mode = if look.is_some_and(|l| l.alpha_test) {
        AlphaMode::Mask(0.5)
    } else {
        AlphaMode::Opaque
    };
    // Unlit: the world has no bevy lights, only baked ones;
    // `light_rider` scales the colour by the light at the rider.
    let handle = materials.add(StandardMaterial {
        base_color,
        base_color_texture: texture,
        uv_transform,
        alpha_mode,
        unlit: true,
        ..default()
    });
    Some((handle, base_color))
}

/// The Spartan in red, standing still in his bind pose (arms forward, ready):
/// static meshes in renderer metres, Y up, feet at the origin, facing +X.
/// Spawned and lit by `mcc_weapons::enemy`.
#[derive(Resource)]
pub struct SpartanStatue {
    /// Each mesh with its material and the material's unlit colour.
    pub parts: Vec<(Handle<Mesh>, Handle<StandardMaterial>, LinearRgba)>,
    /// Top of the helmet over the soles, metres.
    pub height: f32,
}

fn build_statue(
    chief: &ChiefModel,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    images: &mut Assets<Image>,
) -> SpartanStatue {
    // Halo model units, Z up, to renderer metres, Y up (facing stays +X).
    let to_render = Mat4::from_scale(Vec3::splat(WORLD_UNIT_METRES))
        * Mat4::from_cols(Vec4::X, Vec4::NEG_Z, Vec4::Y, Vec4::W);
    let mut image_handles = HashMap::new();
    let mut parts = Vec::new();
    let mut top = 0.0f32;
    for piece in &chief.parts.pieces {
        let Some(mesh) = chief.scene.meshes.get(piece.mesh as usize) else {
            continue;
        };
        let m = to_render * piece.transform;
        let n = mesh.positions.len();
        let positions: Vec<[f32; 3]> = mesh.positions.iter().map(|p| m.transform_point3(*p).to_array()).collect();
        top = positions.iter().fold(top, |t, p| t.max(p[1]));
        let normals = (mesh.normals.len() == n).then(|| {
            mesh.normals
                .iter()
                .map(|v| m.transform_vector3(*v).normalize_or_zero().to_array())
                .collect::<Vec<_>>()
        });
        let uvs = if mesh.uvs.len() == n { mesh.uvs.clone() } else { vec![[0.0; 2]; n] };
        for part in &mesh.parts {
            let Some((material, base_color)) =
                chief_material(chief, part.material, &chief.tinted_red, images, materials, &mut image_handles)
            else {
                continue;
            };
            let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
            out.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
            out.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
            out.insert_indices(Indices::U32(part.indices.clone()));
            match &normals {
                Some(v) => out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, v.clone()),
                None => out.compute_normals(),
            }
            parts.push((meshes.add(out), material, base_color.to_linear()));
        }
    }
    SpartanStatue {
        parts,
        height: if top > 0.5 { top } else { 0.70 * WORLD_UNIT_METRES },
    }
}

/// Moves the joints to the latest pose and shows or hides the rider.
fn pose_rider(
    rider: Option<ResMut<Rider>>,
    pose: Res<RiderPose>,
    mut joints: Query<&mut GlobalTransform>,
    mut visibility: Query<&mut Visibility>,
) {
    let Some(mut rider) = rider else {
        return;
    };
    let show = pose.active && !pose.bones.is_empty();
    let shown = (show && !pose.first_person, show && !pose.bike && !pose.foot);
    if shown != rider.shown {
        rider.shown = shown;
        let rider = &*rider;
        let all = rider.meshes.iter().map(|e| (e, shown.0)).chain(rider.board.iter().map(|e| (e, shown.1)));
        for (&e, on) in all {
            if let Ok(mut v) = visibility.get_mut(e) {
                *v = if on { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
    if !show || !pose.is_changed() {
        return;
    }
    let by_name: HashMap<&str, &Mat4> =
        pose.names.iter().map(String::as_str).zip(pose.bones.iter()).collect();
    let s = Mat4::from_scale(Vec3::splat(1.0 / pose.scale));
    let mut world: Vec<Mat4> = Vec::with_capacity(rider.joints.len());
    for j in &rider.joints {
        let Some(bone) = by_name.get(j.target.as_str()) else {
            world.push(Mat4::IDENTITY);
            continue;
        };
        let mut m = s * **bone * j.offset;
        if let Some((parent, offset)) = j.attach
            && let Some(p) = world.get(parent)
        {
            m.w_axis = p.transform_point3(offset).extend(1.0);
        }
        world.push(m);
        if let Ok(mut g) = joints.get_mut(j.entity) {
            *g = GlobalTransform::from(Affine3A::from_mat4(m));
        }
    }
}

/// What an unlit object's colour is multiplied by at a Halo point: the
/// baked lighting of the nearest lit surface (of the shown BSPs) for an
/// up-weighted spread of normals, times the viewer's exposure.
pub fn object_light(lights: Option<&SceneLights>, at: Vec3, shown: u32) -> Vec3 {
    let light = match lights {
        // The shown BSPs' surfaces only: overlapping versions of a space
        // (120_halo's 010 and 070) light differently.
        Some(l) if !l.0.is_empty() => l.0.near_in(at, shown),
        _ => default_light(),
    };
    let side: Vec3 = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y]
        .iter()
        .map(|n| light.irradiance(*n).max(Vec3::ZERO))
        .sum();
    (light.irradiance(Vec3::Z).max(Vec3::ZERO) * 0.4 + side * 0.15) * exposure()
}

/// Lights the rider the way the world lights objects: Halo's baked
/// lighting at the nearest lit surface under the board, evaluated for an
/// up-weighted spread of normals, times the viewer's exposure. Resampled
/// when the rider has moved a little.
fn light_rider(
    rider: Option<ResMut<Rider>>,
    pose: Res<RiderPose>,
    lights: Option<Res<SceneLights>>,
    loaded: Option<Res<mcc_core::zones::LoadedBsps>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(mut rider) = rider else {
        return;
    };
    if !rider.shown.0 {
        return;
    }
    let Some(i) = pose.names.iter().position(|n| n == "SKATEBOARD_ROOT") else {
        return;
    };
    let at = to_halo(pose.bones[i].w_axis.truncate() / pose.scale) + Vec3::Z * 0.1;
    if rider.lit_at.is_some_and(|p| p.distance(at) < 0.2) {
        return;
    }
    rider.lit_at = Some(at);
    let e = object_light(lights.as_deref(), at, loaded.as_deref().map_or(0, |b| b.shown));
    for (handle, colour) in &rider.materials {
        if let Some(mut m) = materials.get_mut(handle) {
            m.base_color = LinearRgba::rgb(colour.red * e.x, colour.green * e.y, colour.blue * e.z).into();
        }
    }
}
