//! The bike as Reflex draws it: the Dakumoto 450F with the Cortana graphics
//! kit, read from the user's own MX vs ATV Reflex install by
//! `reflex_assets` in the background at start-up. Without Reflex (or if it
//! fails to read) it is our own stand-in bike (`stand_in.rs`), moved the
//! same way; the stick model in `lib.rs` shows only until one is built.
//!
//! The model's bones follow the simulation: the bars and forks turn about
//! the steering axis, the fork legs slide up it and the swingarm swings so
//! the axles sit where the physics has them, and both wheels roll. Like
//! the rider, the bike is unlit and takes Halo's baked light under it
//! (`mcc_rider::object_light`), with a fixed soft shade baked into the
//! vertices so its shape reads.

use std::sync::Mutex;
use std::sync::mpsc::{Receiver, channel};

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, Face, TextureDimension, TextureFormat};

use mcc_core::coords::to_halo;
use mcc_world::lighting::SceneLights;
use reflex_assets::bike::{BikeModel, KIT};
use reflex_assets::texture::Rgba;

use super::Riding;
use super::physics::{self, FRAME_LIFT};

/// The model: loading, built, or absent.
#[derive(Resource, Default)]
pub struct BikeMesh {
    loading: Option<Mutex<Receiver<Result<BikeModel, String>>>>,
    built: Option<Built>,
}

impl BikeMesh {
    /// Whether a model (Reflex's or the stand-in) is drawn (the sticks are not).
    pub fn ready(&self) -> bool {
        self.built.is_some()
    }
}

struct Built {
    root: Entity,
    bones: Vec<Entity>,
    rest: Vec<Transform>,
    upper_forks: usize,
    lower_forks: usize,
    front_tyre: usize,
    swingarm: usize,
    rear_tyre: usize,
    /// The rear axle from the swingarm pivot at rest, bike frame.
    swing_reach: Vec3,
    materials: Vec<(Handle<StandardMaterial>, LinearRgba)>,
    front_turn: f32,
    rear_turn: f32,
    lit_at: Option<Vec3>,
}

/// Starts reading the 450F off the Reflex install, or hands over the
/// stand-in when there is none.
pub(crate) fn start(mut mesh: ResMut<BikeMesh>) {
    let (send, receive) = channel();
    mesh.loading = Some(Mutex::new(receive));
    let Some(root) = reflex_assets::find_reflex() else {
        info!("bike: no MX vs ATV Reflex; drawing our own stand-in bike");
        let _ = send.send(Ok(super::stand_in::model()));
        return;
    };
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let model = reflex_assets::bike::load(&root).map_err(|e| e.to_string());
        if let Ok(m) = &model {
            info!(
                "bike: Dakumoto 450F from {} ({} parts, kit: {}) in {:.2} s",
                root.display(),
                m.parts.len(),
                m.kit,
                started.elapsed().as_secs_f32()
            );
        }
        let _ = send.send(model);
    });
}

/// Builds the entities once the model has arrived.
pub(crate) fn build(
    mut commands: Commands,
    mut mesh: ResMut<BikeMesh>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(loading) = mesh.loading.as_ref() else {
        return;
    };
    let received = loading.lock().unwrap_or_else(|e| e.into_inner()).try_recv();
    let model = match received {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => {
            warn!("bike: the Reflex 450F did not load ({e}); drawing our own stand-in bike");
            super::stand_in::model()
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => return,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => super::stand_in::model(),
    };
    mesh.loading = None;
    let skeleton = |m: &BikeModel| {
        Some((
            m.bone("OBJ_UPR_Forks")?,
            m.bone("OBJ_LWR_Forks")?,
            m.bone("OBJ_FRT_Tire")?,
            m.bone("OBJ_Swingarm")?,
            m.bone("OBJ_BCK_Tire")?,
        ))
    };
    let (model, (upper_forks, lower_forks, front_tyre, swingarm, rear_tyre)) = match skeleton(&model) {
        Some(bones) => (model, bones),
        None => {
            warn!("bike: the 450F's skeleton is not as expected; drawing our own stand-in bike");
            let m = super::stand_in::model();
            let Some(bones) = skeleton(&m) else {
                return;
            };
            (m, bones)
        }
    };


    let root = commands
        .spawn((Transform::default(), Visibility::Hidden, Name::new(if model.textures.contains_key(super::stand_in::PALETTE) {
            "bike: stand-in"
        } else {
            "bike: Dakumoto 450F"
        })))
        .id();
    let rest: Vec<Transform> = model
        .bones
        .iter()
        .map(|b| Transform::from_matrix(Mat4::from_cols_array_2d(&b.local)))
        .collect();
    let bones: Vec<Entity> = model
        .bones
        .iter()
        .zip(&rest)
        .map(|(b, t)| commands.spawn((*t, Visibility::Inherited, Name::new(b.name.clone()))).id())
        .collect();
    for (i, b) in model.bones.iter().enumerate() {
        let parent = b.parent.and_then(|p| bones.get(p).copied()).unwrap_or(root);
        commands.entity(parent).add_child(bones[i]);
    }

    let textures: std::collections::HashMap<&str, Handle<Image>> = model
        .textures
        .iter()
        .map(|(k, t)| (k.as_str(), images.add(image(t))))
        .collect();
    // A soft light from above, ahead and to the side, fixed to the bike.
    let light = Vec3::new(0.35, 1.0, 0.45).normalize();
    let mut made = Vec::new();
    for part in &model.parts {
        let rotation = Mat3::from_mat4(Mat4::from_cols_array_2d(&model.rest(part.bone)));
        let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
        m.insert_attribute(Mesh::ATTRIBUTE_POSITION, part.positions.clone());
        m.insert_attribute(Mesh::ATTRIBUTE_UV_0, part.uvs.clone());
        m.insert_indices(Indices::U32(part.indices.clone()));
        if part.normals.len() == part.positions.len() {
            m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, part.normals.clone());
        } else {
            m.compute_normals();
        }
        let shade: Vec<[f32; 4]> = match m.attribute(Mesh::ATTRIBUTE_NORMAL).and_then(|a| a.as_float3()) {
            Some(normals) => normals
                .iter()
                .map(|&n| {
                    let n = (rotation * Vec3::from(n)).normalize_or_zero();
                    let s = 0.62 + 0.48 * n.dot(light).max(0.0) - 0.12 * (-n.y).max(0.0);
                    [s, s, s, 1.0]
                })
                .collect(),
            None => vec![[1.0; 4]; part.positions.len()],
        };
        m.insert_attribute(Mesh::ATTRIBUTE_COLOR, shade);
        let texture = part.texture.as_deref().and_then(|t| textures.get(t)).cloned();
        let colour = if texture.is_some() {
            LinearRgba::WHITE
        } else if part.slot.contains("Tire") {
            // A tyre without its sheet: black rubber.
            LinearRgba::rgb(0.03, 0.03, 0.03)
        } else {
            LinearRgba::rgb(0.5, 0.5, 0.52)
        };
        let material = materials.add(StandardMaterial {
            base_color: colour.into(),
            base_color_texture: texture,
            unlit: true,
            alpha_mode: if part.cutout { AlphaMode::Mask(0.5) } else { AlphaMode::Opaque },
            cull_mode: if part.cutout { None } else { Some(Face::Back) },
            double_sided: part.cutout,
            ..default()
        });
        made.push((material.clone(), colour));
        let e = commands
            .spawn((
                Mesh3d(meshes.add(m)),
                MeshMaterial3d(material),
                Transform::default(),
                // The bones move it about every frame.
                NoFrustumCulling,
                Name::new(format!("bike part {}", part.slot)),
            ))
            .id();
        commands.entity(bones[part.bone]).add_child(e);
    }
    let pivot = Vec3::from(model.rest(swingarm)[3][..3].try_into().unwrap_or([0.0; 3]));
    let axle = Vec3::from(model.rest(rear_tyre)[3][..3].try_into().unwrap_or([0.0; 3]));
    if textures.contains_key(super::stand_in::PALETTE) {
        info!("bike: {}", model.kit);
    } else {
        info!("bike: kit sheet {}", if textures.contains_key(KIT) { "loaded" } else { "missing" });
    }
    mesh.built = Some(Built {
        root,
        bones,
        rest,
        upper_forks,
        lower_forks,
        front_tyre,
        swingarm,
        rear_tyre,
        swing_reach: axle - pivot,
        materials: made,
        front_turn: 0.0,
        rear_turn: 0.0,
        lit_at: None,
    });
}

/// An sRGB RGBA8 image with a box-filtered mip chain.
fn image(t: &Rgba) -> Image {
    let (mut w, mut h) = (t.width as usize, t.height as usize);
    let mut level: Vec<[u8; 4]> = t.pixels.clone();
    let mut data: Vec<u8> = level.iter().flatten().copied().collect();
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let next: Vec<[u8; 4]> = (0..nw * nh)
            .map(|i| {
                let (x, y) = (i % nw, i / nw);
                let mut sum = [0u32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = level[(y * 2 + dy).min(h - 1) * w + (x * 2 + dx).min(w - 1)];
                    for c in 0..4 {
                        sum[c] += p[c] as u32;
                    }
                }
                sum.map(|s| ((s + 2) / 4) as u8)
            })
            .collect();
        data.extend(next.iter().flatten());
        level = next;
        (w, h) = (nw, nh);
        levels += 1;
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
    image.texture_descriptor.mip_level_count = levels;
    image.data = Some(data);
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    image
}

/// Puts the model where the bike is and moves its parts with the physics.
pub(crate) fn place(
    riding: Res<Riding>,
    mut mesh: ResMut<BikeMesh>,
    time: Res<Time>,
    mut transforms: Query<(&mut Transform, &mut Visibility)>,
) {
    let Some(built) = mesh.built.as_mut() else {
        return;
    };
    let Some(bike) = riding.bike.as_ref() else {
        if let Ok((_, mut v)) = transforms.get_mut(built.root) {
            *v = Visibility::Hidden;
        }
        return;
    };
    let dt = time.delta_secs();
    if let Ok((mut t, mut v)) = transforms.get_mut(built.root) {
        // The model's origin is the centre of gravity, under the frame's.
        *t = Transform::from_translation(bike.to_world(Vec3::NEG_Y * FRAME_LIFT)).with_rotation(bike.rotation);
        *v = Visibility::Visible;
    }
    let mut set = |bone: usize, t: Transform| {
        if let Ok((mut at, _)) = transforms.get_mut(built.bones[bone]) {
            *at = t;
        }
    };
    let compression = |i: usize, leg: physics::Leg| bike.wheels[i].compression.clamp(0.0, leg.travel);

    // Bars and forks about the steering axis (the triple clamp's own Y).
    let upper = built.rest[built.upper_forks];
    set(built.upper_forks, upper.with_rotation(upper.rotation * Quat::from_rotation_y(bike.steer)));
    // The fork legs slide up the axis as far as the axle has risen.
    let rise = compression(1, physics::front_leg()) / physics::steering_axis().y.max(0.3);
    let lower = built.rest[built.lower_forks];
    set(built.lower_forks, lower.with_translation(lower.translation + Vec3::Y * rise));
    // The swingarm swings until the rear axle has risen as far.
    let reach = built.swing_reach;
    let r = reach.length().max(1e-3);
    let lift = compression(0, physics::rear_leg());
    let swing = ((reach.y + lift) / r).clamp(-1.0, 1.0).asin() - (reach.y / r).clamp(-1.0, 1.0).asin();
    let arm = built.rest[built.swingarm];
    set(built.swingarm, arm.with_rotation(Quat::from_rotation_x(swing) * arm.rotation));

    // Wheels roll: the rear at the drive's speed, the front with the ground.
    built.rear_turn = (built.rear_turn + bike.wheel_spin * dt) % std::f32::consts::TAU;
    let forward = bike.velocity.dot(bike.forward());
    built.front_turn = (built.front_turn + forward / physics::front_leg().radius * dt) % std::f32::consts::TAU;
    for (bone, turn) in [(built.front_tyre, built.front_turn), (built.rear_tyre, built.rear_turn)] {
        let rest = built.rest[bone];
        set(bone, rest.with_rotation(rest.rotation * Quat::from_rotation_x(turn)));
    }
}

/// Halo's baked light where the bike is, as the rider gets it.
pub(crate) fn light(
    riding: Res<Riding>,
    mut mesh: ResMut<BikeMesh>,
    lights: Option<Res<SceneLights>>,
    loaded: Option<Res<mcc_core::zones::LoadedBsps>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let (Some(built), Some(bike)) = (mesh.built.as_mut(), riding.bike.as_ref()) else {
        return;
    };
    let at = to_halo(bike.position);
    if built.lit_at.is_some_and(|p| p.distance(at) < 0.2) {
        return;
    }
    built.lit_at = Some(at);
    let e = mcc_rider::object_light(lights.as_deref(), at, loaded.as_deref().map_or(0, |b| b.shown));
    for (handle, colour) in &built.materials {
        if let Some(mut m) = materials.get_mut(handle) {
            m.base_color = LinearRgba::rgb(colour.red * e.x, colour.green * e.y, colour.blue * e.z).into();
        }
    }
}
