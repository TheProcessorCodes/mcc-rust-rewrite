//! `--anim-test`: a row of the map's own bipeds (or machines) playing
//! Halo 3's animations, for checking `halo3_anim` in a screenshot.
//!
//! ```text
//! halo3 120_halo --anim-test flood=move_front,flood=melee,sentinel=aim,chief=combat:rifle:ar:move_front
//!        [--anim-at x,y,z,yaw] [--shot DIR --view ...]
//! ```
//!
//! Each item is `WHO[=ACTION]`. WHO is an alias (`flood`, `flood_elite`,
//! `carrier`, `infection`, `sentinel`, `chief`, `marine`, `elite`, `grunt`,
//! `plate`) or an object tag path. ACTION is an action in the default
//! stance (`combat:any:any`), a full `mode:class:type:action`, `aim` (idle
//! with the aim overlay sweeping), or `position` (a device machine's
//! `device:position` going 0 -> 1 -> 0). Everything loops. The row stands
//! `--anim-at` (Halo feet position, facing in degrees), else three world
//! units in front of the first `--view` or the start, facing back.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::math::{Affine2, Mat2};
use bevy::mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes};
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;

use blam_cache::CacheFile;
use blam_geometry::Scene;
use blam_geometry::render_model::ModelParts;
use halo3_anim::{AnimModel, Animator};
use halo3_core::coords::to_render;
use halo3_textures::MaterialTextures;

const ALIASES: [(&str, &str); 10] = [
    ("flood", r"objects\characters\floodcombat_human\floodcombat_human"),
    ("flood_elite", r"objects\characters\floodcombat_elite\floodcombat_elite"),
    ("carrier", r"objects\characters\floodcarrier\floodcarrier"),
    ("infection", r"objects\characters\flood_infection\flood_infection"),
    ("sentinel", r"objects\characters\sentinel_aggressor\sentinel_aggressor"),
    ("chief", r"objects\characters\masterchief\masterchief"),
    ("marine", r"objects\characters\marine\marine"),
    ("elite", r"objects\characters\elite\elite"),
    ("grunt", r"objects\characters\grunt\grunt"),
    ("plate", r"objects\levels\solo\120_halo\plates\island_test\island_test"),
];

/// One object type, decoded once.
struct Loaded {
    model: Arc<AnimModel>,
    parts: ModelParts,
    scene: Scene,
    textures: MaterialTextures,
    /// Scene materials not drawn: holograms and shields (`rmhg`), which
    /// are effects, not surfaces.
    skip: std::collections::HashSet<u32>,
}

struct TestActor {
    loaded: Arc<Loaded>,
    action: String,
    /// Feet above the ground (sentinels hover).
    lift: f32,
}

#[derive(Resource)]
pub struct PendingAnimTest {
    actors: Vec<TestActor>,
    /// Halo position of the row's centre and the way the actors face.
    anchor: Vec3,
    yaw: f32,
}

/// Reads `--anim-test` and `--anim-at`; `fallback` is where the row goes
/// without `--anim-at` (feet position, facing), already worked out by the
/// caller from the first view or the start.
pub fn load(cache: &CacheFile, spec: &str, at: Option<&str>, fallback: (Vec3, f32)) -> anyhow::Result<PendingAnimTest> {
    let (anchor, yaw) = match at {
        Some(s) => {
            let v: Vec<f32> = s.split(',').map(|x| x.trim().parse::<f32>()).collect::<Result<_, _>>()?;
            match v[..] {
                [x, y, z, yaw] => (Vec3::new(x, y, z), yaw.to_radians()),
                [x, y, z] => (Vec3::new(x, y, z), 0.0),
                _ => anyhow::bail!("--anim-at wants x,y,z[,yaw]"),
            }
        }
        None => fallback,
    };
    let mut loaded: HashMap<String, Arc<Loaded>> = HashMap::new();
    let mut actors = Vec::new();
    for item in spec.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (who, action) = item.split_once('=').unwrap_or((item, "idle"));
        let path = ALIASES.iter().find(|(a, _)| *a == who).map_or(who, |(_, p)| *p);
        if !loaded.contains_key(path) {
            let model = AnimModel::load_path(cache, path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
            let object = ["bipd", "mach", "vehi", "scen", "hlmt"]
                .iter()
                .find_map(|c| cache.tags.find(c, path))
                .ok_or_else(|| anyhow::anyhow!("no object {path}"))?;
            let mode = blam_geometry::scenario::render_model_of(cache, object)?
                .ok_or_else(|| anyhow::anyhow!("{path} has no render model"))?;
            let resources = cache.resources()?;
            let mut materials = blam_geometry::materials::MaterialTable::default();
            let mut scene = Scene::default();
            let parts = blam_geometry::render_model::decode(cache, &resources, &mut materials, mode, &mut scene)?;
            drop(resources);
            let textures = halo3_textures::load(cache, &scene);
            eprintln!(
                "anim test: {path}: {} nodes, {} animations ({} not in this map), {} meshes, {} of {} materials textured",
                model.skeleton.len(),
                model.graph().animations.len(),
                model.missing,
                scene.meshes.len(),
                textures.looks.iter().flatten().filter(|l| l.texture.is_some_and(|t| textures.bitmaps.contains_key(&t))).count(),
                textures.looks.len()
            );
            let skip = scene
                .materials
                .iter()
                .enumerate()
                .filter(|(_, m)| {
                    m.shader_tag
                        .and_then(|t| cache.tags.get(t))
                        .is_some_and(|t| cache.tags.class_of(t).code.as_str() == "rmhg")
                })
                .map(|(i, _)| i as u32)
                .collect();
            loaded.insert(path.to_string(), Arc::new(Loaded { model, parts, scene, textures, skip }));
        }
        let lift = if path.contains("sentinel") { 0.5 } else { 0.0 };
        actors.push(TestActor { loaded: loaded[path].clone(), action: action.to_string(), lift });
    }
    Ok(PendingAnimTest { actors, anchor, yaw })
}

/// What a test actor does over time beyond its base animation.
#[derive(Component)]
enum Extra {
    None,
    Aim,
    Position,
}

/// The materials of one actor, with their unlit colours, for lighting.
#[derive(Component)]
struct ActorLook(Vec<(Handle<StandardMaterial>, LinearRgba)>, bool);

pub struct AnimTestPlugin;

impl Plugin for AnimTestPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(halo3_anim::AnimPlugin)
            .add_systems(Startup, spawn)
            .add_systems(Update, (drive, light));
    }
}

fn spawn(
    mut commands: Commands,
    pending: Option<ResMut<PendingAnimTest>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut binds: ResMut<Assets<SkinnedMeshInverseBindposes>>,
) {
    let Some(mut pending) = pending else { return };
    let actors = std::mem::take(&mut pending.actors);
    let n = actors.len();
    let (anchor, yaw) = (pending.anchor, pending.yaw);
    let right = Vec3::new(yaw.sin(), -yaw.cos(), 0.0);
    for (i, test) in actors.into_iter().enumerate() {
        let actor = &*test.loaded;
        let side = (i as f32 - (n as f32 - 1.0) / 2.0) * 1.1;
        let feet = anchor + right * side + Vec3::Z * test.lift;
        let mut animator = Animator::new(actor.model.clone()).with_seed(i as u32 * 7919 + 1);
        let extra = match test.action.as_str() {
            "aim" => {
                animator.play("idle");
                Extra::Aim
            }
            "position" => {
                animator.set_position(0.0);
                Extra::Position
            }
            a => {
                let parts: Vec<&str> = a.split(':').collect();
                let ok = match parts[..] {
                    [m, c, t, act] => {
                        animator.set_stance(m, c, t);
                        animator.play(act)
                    }
                    _ => animator.play(a),
                };
                if !ok {
                    eprintln!("anim test: {} has no action {a}; showing the rest pose", actor.model.name);
                }
                Extra::None
            }
        };
        eprintln!(
            "anim test: {} #{i} plays {:?} ({})",
            actor.model.name,
            animator.current(),
            test.action
        );
        let joints = halo3_anim::spawn_joints(&mut commands, &actor.model);
        let inverse = binds.add(SkinnedMeshInverseBindposes::from(actor.model.inverse_bindposes()));
        let mut looks = Vec::new();
        let bones = actor.model.skeleton.len() as u16;
        let mut image_handles: HashMap<u32, Option<Handle<Image>>> = HashMap::new();
        for piece in &actor.parts.pieces {
            let Some(mesh) = actor.scene.meshes.get(piece.mesh as usize) else { continue };
            let count = mesh.positions.len();
            let (joint_ids, weights) = match &mesh.skin {
                Some(s) if s.joints.len() == count && s.weights.len() == count => (s.joints.clone(), s.weights.clone()),
                _ => {
                    let b = piece.bone.unwrap_or(0).min(bones.saturating_sub(1));
                    (vec![[b, 0, 0, 0]; count], vec![[1.0, 0.0, 0.0, 0.0]; count])
                }
            };
            let positions: Vec<[f32; 3]> =
                mesh.positions.iter().map(|p| piece.transform.transform_point3(*p).to_array()).collect();
            let normals: Option<Vec<[f32; 3]>> = (mesh.normals.len() == count).then(|| {
                mesh.normals
                    .iter()
                    .map(|v| piece.transform.transform_vector3(*v).normalize_or_zero().to_array())
                    .collect()
            });
            let uvs = if mesh.uvs.len() == count { mesh.uvs.clone() } else { vec![[0.0; 2]; count] };
            for part in &mesh.parts {
                let look = actor.textures.looks.get(part.material as usize).and_then(Option::as_ref);
                if look.is_some_and(|l| l.hidden) || actor.skip.contains(&part.material) {
                    continue;
                }
                let texture = look.and_then(|l| l.texture).and_then(|b| {
                    image_handles
                        .entry(b)
                        .or_insert_with(|| {
                            actor.textures.bitmaps.get(&b).and_then(halo3_rider::halo_image).map(|i| images.add(i))
                        })
                        .clone()
                });
                let colour = match (texture.is_some(), look.and_then(|l| l.colour)) {
                    (_, Some(c)) => Color::srgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0)),
                    (true, None) => Color::WHITE,
                    (false, None) => Color::srgb(0.5, 0.5, 0.45),
                };
                let uv_transform = look.map_or(Affine2::IDENTITY, |l| Affine2 {
                    matrix2: Mat2::from_diagonal(Vec2::from(l.xform.scale)),
                    translation: Vec2::from(l.xform.offset),
                });
                let alpha_mode = if look.is_some_and(|l| l.alpha_test) { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque };
                let material = materials.add(StandardMaterial {
                    base_color: colour,
                    base_color_texture: texture,
                    uv_transform,
                    alpha_mode,
                    unlit: true,
                    double_sided: look.is_some_and(|l| l.two_sided),
                    ..default()
                });
                looks.push((material.clone(), colour.to_linear()));
                let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
                m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs.clone());
                m.insert_attribute(Mesh::ATTRIBUTE_JOINT_INDEX, VertexAttributeValues::Uint16x4(joint_ids.clone()));
                m.insert_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT, weights.clone());
                m.insert_indices(Indices::U32(part.indices.clone()));
                match &normals {
                    Some(n) => m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, n.clone()),
                    None => m.compute_normals(),
                }
                commands.spawn((
                    Mesh3d(meshes.add(m)),
                    MeshMaterial3d(material),
                    SkinnedMesh { inverse_bindposes: inverse.clone(), joints: joints.0.clone() },
                    NoFrustumCulling,
                    Name::new(format!("{} {}", actor.model.name, mesh.name)),
                ));
            }
        }
        // Halo yaw about +Z is a turn about the renderer's +Y.
        commands.spawn((
            Transform::from_translation(to_render(feet)).with_rotation(Quat::from_rotation_y(yaw)),
            Visibility::default(),
            animator,
            joints,
            extra,
            ActorLook(looks, false),
            Name::new(format!("anim test {}", actor.model.name)),
        ));
    }
}

fn drive(time: Res<Time>, mut actors: Query<(&mut Animator, &Extra)>) {
    let t = time.elapsed_secs();
    for (mut a, extra) in &mut actors {
        match extra {
            Extra::None => {}
            Extra::Aim => a.aim((t * 0.9).sin() * 0.9, (t * 0.6).sin() * 0.6, false),
            Extra::Position => {
                let p = 0.5 - 0.5 * (t * std::f32::consts::TAU / 6.0).cos();
                a.set_position(p);
            }
        }
        // The row stands still: root motion is dropped.
        a.take_root_motion();
    }
}

/// Lights each actor once the world's baked lighting is known, the way the
/// rider is lit.
fn light(
    lights: Option<Res<halo3_world::lighting::SceneLights>>,
    loaded: Option<Res<halo3_core::zones::LoadedBsps>>,
    mut actors: Query<(&GlobalTransform, &mut ActorLook)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(lights) = lights.filter(|l| !l.0.is_empty()) else { return };
    for (at, mut look) in &mut actors {
        if look.1 {
            continue;
        }
        look.1 = true;
        let p = halo3_core::coords::to_halo(at.translation()) + Vec3::Z * 0.1;
        let e = halo3_rider::object_light(Some(&lights), p, loaded.as_deref().map_or(0, |b| b.shown));
        for (handle, colour) in &look.0 {
            if let Some(mut m) = materials.get_mut(handle) {
                m.base_color = LinearRgba::rgb(colour.red * e.x, colour.green * e.y, colour.blue * e.z).into();
            }
        }
    }
}
