//! Actors' bodies on screen: each unit's render model decoded once at
//! start (CPU, `decode`), turned into skinned bevy meshes once (`upload`),
//! and copied per actor (`spawn_body`) as a root entity, one joint entity
//! per node and the skinned meshes bound to those joints.
//!
//! The joints start at the bind pose and stay there until the animation
//! code poses them (`halo3_core::ai::ActorSkeleton`): every joint's local
//! `Transform` is its node relative to its parent node, in Halo model units
//! (world units, +Z up), so a `jmad` frame drops straight in.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::{Affine2, Mat2};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_geometry::render_model::ModelParts;
use blam_geometry::weapons::Marker;

use halo3_textures::MaterialTextures;

/// One unit's model, CPU side.
pub struct ModelData {
    pub mode: u32,
    pub scene: Scene,
    pub parts: ModelParts,
    pub textures: MaterialTextures,
    pub markers: Vec<Marker>,
    /// Bind-pose bounds in model space (world units).
    pub min: Vec3,
    pub max: Vec3,
}

impl ModelData {
    pub fn marker(&self, name: &str) -> Option<&Marker> {
        self.markers.iter().find(|m| m.name == name)
    }

    /// The node named `name`.
    pub fn node(&self, name: &str) -> Option<usize> {
        self.parts.bones.iter().position(|b| b.name == name)
    }
}

/// Decodes a render model with its default look: each region's `base` or
/// `default` permutation where it has one, else its first.
pub fn decode(cache: &CacheFile, mode_index: u32) -> Option<ModelData> {
    let mode = cache.tags.get(mode_index)?;
    let resources = cache.resources().ok()?;
    let mut materials = blam_geometry::materials::MaterialTable::default();
    let mut scene = Scene::default();
    let pick = |_region: &str, perms: &[String]| -> usize {
        perms
            .iter()
            .position(|p| p == "base" || p == "default")
            .unwrap_or(0)
    };
    let parts = match blam_geometry::render_model::decode_with(cache, &resources, &mut materials, mode, &mut scene, &pick) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("ai: {} not decoded: {e}", mode.name());
            return None;
        }
    };
    drop(resources);
    let markers = blam_geometry::weapons::markers(cache, mode).unwrap_or_default();
    // Only the materials this look draws.
    let used: std::collections::HashSet<u32> =
        scene.meshes.iter().flat_map(|m| m.parts.iter().map(|p| p.material)).collect();
    let all = scene.materials.clone();
    for (i, m) in scene.materials.iter_mut().enumerate() {
        if !used.contains(&(i as u32)) {
            m.shader_tag = None;
        }
    }
    let textures = halo3_textures::load(cache, &scene);
    scene.materials = all;
    let (mut min, mut max) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for piece in &parts.pieces {
        if let Some(mesh) = scene.meshes.get(piece.mesh as usize) {
            for p in &mesh.positions {
                let q = piece.transform.transform_point3(*p);
                min = min.min(q);
                max = max.max(q);
            }
        }
    }
    if min.x > max.x {
        (min, max) = (Vec3::ZERO, Vec3::splat(0.2));
    }
    Some(ModelData {
        mode: mode_index,
        scene,
        parts,
        textures,
        markers,
        min,
        max,
    })
}

/// One unit's model on the GPU, shared by every actor wearing it.
pub struct ModelLook {
    pub pieces: Vec<LookPiece>,
    pub inverse_bindposes: Option<Handle<SkinnedMeshInverseBindposes>>,
    /// Node names and bind-pose local frames (parent-relative), and parents.
    pub names: Vec<String>,
    pub bind: Vec<Transform>,
    pub parents: Vec<Option<usize>>,
}

pub struct LookPiece {
    pub mesh: Handle<Mesh>,
    pub texture: Option<Handle<Image>>,
    pub colour: LinearRgba,
    pub uv_transform: Affine2,
    pub alpha_mode: AlphaMode,
}

impl LookPiece {
    /// This piece's material for one actor, lit by `light`.
    pub fn material(&self, light: Vec3) -> StandardMaterial {
        let c = self.colour;
        StandardMaterial {
            base_color: LinearRgba::rgb(c.red * light.x, c.green * light.y, c.blue * light.z).into(),
            base_color_texture: self.texture.clone(),
            uv_transform: self.uv_transform,
            alpha_mode: self.alpha_mode,
            unlit: true,
            double_sided: true,
            cull_mode: None,
            ..default()
        }
    }
}

fn skinned_mesh(
    positions: Vec<[f32; 3]>,
    normals: Option<Vec<[f32; 3]>>,
    uvs: Vec<[f32; 2]>,
    skin: Option<(Vec<[u16; 4]>, Vec<[f32; 4]>)>,
    indices: Vec<u32>,
) -> Mesh {
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    if let Some((joints, weights)) = skin {
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joints));
        mesh.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights);
    }
    mesh.insert_indices(Indices::U32(indices));
    match normals {
        Some(n) => mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n),
        None => mesh.compute_normals(),
    }
    mesh
}

/// Makes a model's meshes and textures. Skinned when the model has nodes:
/// rigid pieces are bound wholly to their node.
pub fn upload(
    model: &ModelData,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    binds: &mut Assets<SkinnedMeshInverseBindposes>,
) -> ModelLook {
    let bones = &model.parts.bones;
    let skinned = !bones.is_empty() && bones.len() <= 256;
    let inverse_bindposes = skinned.then(|| {
        binds.add(SkinnedMeshInverseBindposes::from(
            bones.iter().map(|b| b.world.inverse()).collect::<Vec<_>>(),
        ))
    });
    let mut image_handles: HashMap<u32, Option<Handle<Image>>> = HashMap::new();
    let mut pieces = Vec::new();
    for piece in &model.parts.pieces {
        let Some(mesh) = model.scene.meshes.get(piece.mesh as usize) else {
            continue;
        };
        let n = mesh.positions.len();
        let skin = skinned.then(|| match &mesh.skin {
            Some(s) if s.joints.len() == n && s.weights.len() == n => (s.joints.clone(), s.weights.clone()),
            _ => {
                let b = piece.bone.unwrap_or(0).min(bones.len() as u16 - 1);
                (vec![[b, 0, 0, 0]; n], vec![[1.0, 0.0, 0.0, 0.0]; n])
            }
        });
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
            let look = model.textures.looks.get(part.material as usize).and_then(Option::as_ref);
            // Energy-shield shells (the sentinel's `rmhg ..._shield` sphere)
            // only flash when hit; drawn plain they hide the body.
            let shield = model
                .scene
                .materials
                .get(part.material as usize)
                .is_some_and(|m| m.tag_path.to_ascii_lowercase().contains("shield"));
            if shield || look.is_some_and(|l| l.hidden) {
                continue;
            }
            let texture = look.and_then(|l| l.texture).and_then(|b| {
                image_handles
                    .entry(b)
                    .or_insert_with(|| {
                        model
                            .textures
                            .bitmaps
                            .get(&b)
                            .and_then(halo3_rider::halo_image)
                            .map(|i| images.add(i))
                    })
                    .clone()
            });
            let colour = match (texture.is_some(), look.and_then(|l| l.colour)) {
                (_, Some(c)) => Color::srgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)),
                (true, None) => Color::WHITE,
                (false, None) => Color::srgb(0.45, 0.42, 0.36),
            };
            let mut indices = part.indices.clone();
            if piece.mirrored {
                for t in indices.chunks_exact_mut(3) {
                    t.swap(1, 2);
                }
            }
            pieces.push(LookPiece {
                mesh: meshes.add(skinned_mesh(positions.clone(), normals.clone(), uvs.clone(), skin.clone(), indices)),
                texture,
                colour: colour.to_linear(),
                uv_transform: look.map_or(Affine2::IDENTITY, |l| Affine2 {
                    matrix2: Mat2::from_diagonal(Vec2::from(l.xform.scale)),
                    translation: Vec2::from(l.xform.offset),
                }),
                alpha_mode: if look.is_some_and(|l| l.alpha_test) {
                    AlphaMode::Mask(0.5)
                } else {
                    AlphaMode::Opaque
                },
            });
        }
    }
    ModelLook {
        pieces,
        inverse_bindposes,
        names: bones.iter().map(|b| b.name.clone()).collect(),
        bind: bones
            .iter()
            .map(|b| Transform::from_translation(b.position).with_rotation(b.rotation))
            .collect(),
        parents: bones
            .iter()
            .enumerate()
            .map(|(i, b)| b.parent.map(usize::from).filter(|p| *p < i))
            .collect(),
    }
}

/// An actor's body: its joints and its materials (for lighting).
pub struct Body {
    pub joints: Vec<Entity>,
    pub materials: Vec<(Handle<StandardMaterial>, usize)>,
}

/// Spawns `look` under `root`: the joints at the bind pose, then the
/// meshes, skinned to them.
pub fn spawn_body(
    commands: &mut Commands,
    look: &ModelLook,
    materials: &mut Assets<StandardMaterial>,
    root: Entity,
    light: Vec3,
) -> Body {
    let mut joints: Vec<Entity> = Vec::with_capacity(look.bind.len());
    for (i, bind) in look.bind.iter().enumerate() {
        let parent = look.parents[i].map_or(root, |p| joints[p]);
        joints.push(commands.spawn((*bind, Visibility::Inherited, ChildOf(parent))).id());
    }
    let mut mats = Vec::with_capacity(look.pieces.len());
    for (i, piece) in look.pieces.iter().enumerate() {
        let m = materials.add(piece.material(light));
        mats.push((m.clone(), i));
        let mut e = commands.spawn((
            Mesh3d(piece.mesh.clone()),
            MeshMaterial3d(m),
            Transform::IDENTITY,
            Visibility::Inherited,
            ChildOf(root),
        ));
        if let Some(inverse) = &look.inverse_bindposes {
            e.insert((
                SkinnedMesh {
                    inverse_bindposes: inverse.clone(),
                    joints: joints.clone(),
                },
                NoFrustumCulling,
            ));
        }
    }
    Body { joints, materials: mats }
}
