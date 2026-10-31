//! What is on the GPU follows the zone set.
//!
//! The world is built per [`Unit`] (`build.rs`): one structure BSP with its
//! instances and objects, or the global unit (sky, objects of no BSP).
//! [`stream_units`] keeps the units of the loaded BSPs (`LoadedBsps::mask`)
//! resident: a missing unit is built on a background thread, then uploaded
//! a slice per frame; once every unit of the new zone set is up, the old
//! zone set's units are despawned, which drops their meshes, materials and
//! (when nothing else uses them) textures and lightmaps. Until then the old
//! zone set stays on screen (`LoadedBsps::shown`).
//!
//! [`zone_triggers`] drives this from the scenario's zone-set switch
//! volumes as the camera or the skater moves: entering a
//! `begin_zone_set:` volume builds the next zone set's BSPs ahead
//! (`Residency::prefetch`), entering a `zone_set:` volume switches to it.

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use blam_texture::Texture;

use super::build::{self, BuildInput, BuiltChunk, BuiltUnit, Lit, Unit};
use super::lighting::{self, HaloLightmap, HaloMaterial};
use super::zones::{self, LoadedBsps, Zones};
use super::{BatchMaterial, Triangles, Upload, Uploaded};
use halo3_core::fly_camera::FlyCamera;
use halo3_textures::LightmapTextures;

/// Triangles uploaded per frame while streaming (the startup load takes
/// everything at once).
const UPLOAD_BUDGET: u32 = 400_000;

/// The scene and its decoded textures, kept for building and uploading
/// units on demand.
#[derive(Resource)]
pub struct WorldSource {
    pub input: Arc<BuildInput>,
    pub bitmaps: HashMap<u32, Texture>,
    pub lightmaps: Vec<Option<LightmapTextures>>,
    /// The GPU takes BC textures.
    pub bc: bool,
}

/// An uploaded image, held weakly, and whether it reads -1..1.
type WeakImage = (AssetId<Image>, bool);

/// GPU resources shared between chunks, held weakly: an entry is reused
/// while some chunk still holds the asset, and made again after the last
/// one is gone.
#[derive(Resource, Default)]
pub struct GpuCache {
    images: HashMap<(u32, Upload), Option<WeakImage>>,
    image_bytes: HashMap<AssetId<Image>, usize>,
    lightmaps: HashMap<u32, (AssetId<Image>, AssetId<Image>)>,
    halo: HashMap<(Lit, u32), AssetId<HaloMaterial>>,
    standard: HashMap<(u32, u32), AssetId<StandardMaterial>>,
    sampler: Option<Handle<Image>>,
    pub textured: HashSet<u32>,
    pub terrains: HashSet<u32>,
}

impl GpuCache {
    /// Bytes of the texture data uploaded that is still alive.
    pub fn live_image_bytes(&self, images: &Assets<Image>) -> usize {
        self.image_bytes
            .iter()
            .filter(|(id, _)| images.contains(**id))
            .map(|(_, b)| b)
            .sum()
    }

    fn image(
        &mut self,
        source: &WorldSource,
        decoded: &HashMap<u32, Texture>,
        images: &mut Assets<Image>,
        bitmap: u32,
        how: Upload,
    ) -> Uploaded {
        if let Some(entry) = self.images.get(&(bitmap, how)) {
            match entry {
                None => return None,
                Some((id, signed)) => {
                    if let Some(h) = images.get_strong_handle(*id) {
                        return Some((h, *signed));
                    }
                }
            }
        }
        let t = source.bitmaps.get(&bitmap).or_else(|| decoded.get(&bitmap))?;
        let made = match how {
            Upload::Colour => super::texture_image_as(t, source.bc, true).map(|i| (i, false)),
            Upload::Linear => super::texture_image_as(t, source.bc, false).map(|i| (i, false)),
            Upload::Normal => super::normal_image(t, source.bc),
        };
        match made {
            Ok((image, signed)) => {
                let bytes = image.data.as_ref().map_or(0, Vec::len);
                let h = images.add(image);
                self.image_bytes.insert(h.id(), bytes);
                self.images.insert((bitmap, how), Some((h.id(), signed)));
                Some((h, signed))
            }
            Err(e) => {
                warn!("texture {}: {e}; left out", t.name);
                self.images.insert((bitmap, how), None);
                None
            }
        }
    }

    fn lightmap(
        &mut self,
        source: &WorldSource,
        decoded: &HashMap<u32, LightmapTextures>,
        images: &mut Assets<Image>,
        l: u32,
    ) -> Option<(Handle<Image>, Handle<Image>)> {
        if let Some((a, b)) = self.lightmaps.get(&l)
            && let (Some(a), Some(b)) = (images.get_strong_handle(*a), images.get_strong_handle(*b))
        {
            return Some((a, b));
        }
        let t = source
            .lightmaps
            .get(l as usize)
            .and_then(Option::as_ref)
            .or_else(|| decoded.get(&l))?;
        match (lighting::array_image(&t.coefficients, source.bc), lighting::array_image(&t.intensity, source.bc)) {
            (Ok(a), Ok(b)) => {
                let bytes = (a.data.as_ref().map_or(0, Vec::len), b.data.as_ref().map_or(0, Vec::len));
                let (a, b) = (images.add(a), images.add(b));
                self.image_bytes.insert(a.id(), bytes.0);
                self.image_bytes.insert(b.id(), bytes.1);
                self.lightmaps.insert(l, (a.id(), b.id()));
                Some((a, b))
            }
            (Err(e), _) | (_, Err(e)) => {
                warn!("lightmap {l}: {e}");
                None
            }
        }
    }
}

/// Which units are built, being built, waiting to upload, or on the GPU.
#[derive(Resource, Default)]
pub struct Residency {
    resident: HashMap<Unit, Vec<Entity>>,
    building: HashMap<Unit, Mutex<Receiver<BuiltUnit>>>,
    ready: HashMap<Unit, BuiltUnit>,
    /// Units partly uploaded: their entities so far.
    uploading: HashMap<Unit, Vec<Entity>>,
    /// BSPs to build ahead of a switch (`begin_zone_set:` volumes).
    pub prefetch: u32,
    /// Log line state: the shown mask last reported, and the frames to
    /// wait before counting what is alive (dropped assets go a frame or two
    /// after their entities).
    reported: Option<u32>,
    report_in: Option<(u32, Vec<Unit>)>,
}

/// Whether the zone-set switch volumes drive the zone set: on in the
/// viewer, off for screenshots (the camera jumps between views) unless
/// `HALO3_ZONE_TRIGGERS=1`; `HALO3_ZONE_TRIGGERS=0` turns it off everywhere.
#[derive(Resource, Clone, Copy)]
pub struct ZoneTriggers(pub bool);

impl ZoneTriggers {
    pub fn new(screenshots: bool) -> ZoneTriggers {
        ZoneTriggers(match std::env::var("HALO3_ZONE_TRIGGERS").as_deref() {
            Ok("0") => false,
            Ok("1") => true,
            _ => !screenshots,
        })
    }
}

impl Residency {
    /// Records a unit uploaded at startup.
    pub fn adopt(&mut self, unit: Unit, entities: Vec<Entity>) {
        self.resident.insert(unit, entities);
    }
}

pub struct StreamPlugin;

impl Plugin for StreamPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<Residency>()
            .init_map_resource::<GpuCache>()
            .add_systems(
                Update,
                (switch_test, zone_triggers, stream_units).chain().after(zones::ZoneKeys).run_if(halo3_core::map::ready),
            );
    }
}

/// `HALO3_ZONE_SWITCH_TEST=N@S` loads zone set `N` once the skater has
/// ridden `S` seconds (or, when not skating, `S` seconds after start): for
/// testing the streaming and the skate collision swap without a keyboard.
fn switch_test(
    zones: Res<Zones>,
    mut loaded: ResMut<LoadedBsps>,
    rider: Option<Res<halo3_core::skate::RiderPose>>,
    time: Res<Time<Real>>,
    mut done: Local<bool>,
) {
    static TEST: std::sync::LazyLock<Option<(usize, f32)>> = std::sync::LazyLock::new(|| {
        let v = std::env::var("HALO3_ZONE_SWITCH_TEST").ok()?;
        let (n, s) = v.split_once('@')?;
        Some((n.parse().ok()?, s.parse().ok()?))
    });
    let Some((n, after)) = *TEST else {
        return;
    };
    let t = match rider.as_deref() {
        Some(r) if r.active => r.skated,
        Some(_) => return,
        None => time.elapsed_secs(),
    };
    if !*done && t >= after {
        *done = true;
        info!("zone switch test: zone set {n} after {t:.1} s");
        zones::load_zone_set(&mut loaded, &zones, n);
    }
}

/// Starts building `unit` on a background thread.
fn start_build(residency: &mut Residency, source: &WorldSource, unit: Unit) {
    let (send, receive) = mpsc::channel();
    let input = Arc::clone(&source.input);
    let spawned = std::thread::Builder::new()
        .name(format!("halo3-build-{unit:?}"))
        .spawn(move || {
            let _ = send.send(build::build_unit(&input, unit));
        });
    match spawned {
        Ok(_) => {
            residency.building.insert(unit, Mutex::new(receive));
        }
        Err(e) => error!("cannot start building {unit:?}: {e}"),
    }
}

/// Builds the units of `mask` on the calling thread's scope, in parallel.
pub fn build_now(source: &WorldSource, mask: u32) -> Vec<BuiltUnit> {
    let units: Vec<Unit> = Unit::of_mask(mask).collect();
    std::thread::scope(|s| {
        let jobs: Vec<_> = units
            .iter()
            .map(|u| {
                let input = &source.input;
                s.spawn(move || build::build_unit(input, *u))
            })
            .collect();
        jobs.into_iter().filter_map(|j| j.join().ok()).collect()
    })
}

/// The material a chunk draws with, shared by every chunk of the same
/// material and lighting.
#[allow(clippy::too_many_arguments)]
fn chunk_material(
    chunk: &BuiltChunk,
    decoded: (&HashMap<u32, Texture>, &HashMap<u32, LightmapTextures>),
    source: &WorldSource,
    cache: &mut GpuCache,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    halo_materials: &mut Assets<HaloMaterial>,
) -> (Option<Handle<StandardMaterial>>, Option<Handle<HaloMaterial>>, bool) {
    let key = chunk.material;
    let lit = chunk.lit;
    let sky = lit == Lit::Sky;
    let scene = &source.input.scene;
    let material = scene.materials.get(key as usize);
    let name = material.map_or("unknown", |m| m.tag_path.as_str());
    let look = source.input.looks.get(key as usize).and_then(Option::as_ref);

    // Reuse a live one.
    if sky {
        let k = (key, chunk.member.sky.unwrap_or(u32::MAX));
        if let Some(h) = cache.standard.get(&k).and_then(|id| materials.get_strong_handle(*id)) {
            let translucent = true;
            return (Some(h), None, translucent);
        }
    } else if let Some(h) = cache.halo.get(&(lit, key)).and_then(|id| halo_materials.get_strong_handle(*id)) {
        let translucent = halo_materials
            .get(&h)
            .is_some_and(|m| !matches!(m.base.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)));
        return (None, Some(h), translucent);
    }

    let terrain = look
        .and_then(|l| l.terrain.as_ref())
        .filter(|_| !sky && super::terrain_blend())
        .and_then(|t| super::terrain_textures(t, &mut |b, how| cache.image(source, decoded.0, images, b, how)));
    let texture = if terrain.is_some() {
        None
    } else {
        look.and_then(|l| l.texture)
            .and_then(|b| cache.image(source, decoded.0, images, b, Upload::Colour))
            .map(|(h, _)| h)
    };
    if texture.is_some() || terrain.is_some() {
        cache.textured.insert(key);
    }
    if terrain.is_some() {
        cache.terrains.insert(key);
    }
    let mut std_material = super::standard_material(name, material, look, texture);
    if terrain.is_some() {
        // The blend replaces the texture: keep the tint, not the hashed
        // stand-in colour.
        let c = look.and_then(|l| l.colour).unwrap_or([1.0; 4]);
        std_material.base_color = Color::srgb(c[0].clamp(0.0, 1.0), c[1].clamp(0.0, 1.0), c[2].clamp(0.0, 1.0));
    }
    // Halo's sky carries its own light: draw its textures as they are.
    std_material.unlit = sky;
    let translucent = sky || !matches!(std_material.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_));
    if sky {
        // `sky.rs` swaps in the sky shader and its atmosphere.
        let h = materials.add(std_material);
        cache.standard.insert((key, chunk.member.sky.unwrap_or(u32::MAX)), h.id());
        return (Some(h), None, translucent);
    }
    let (scales, arrays) = match lit {
        Lit::PerPixel(l) => (scene.lightmaps[l as usize].scales, cache.lightmap(source, decoded.1, images, l)),
        _ => ([0.0; 5], None),
    };
    let (coefficients, intensity) = arrays.map_or((None, None), |(a, b)| (Some(a), Some(b)));
    let sampler = cache
        .sampler
        .get_or_insert_with(|| images.add(super::sampler_carrier()))
        .clone();
    let mut extension = HaloLightmap {
        scales: Vec4::from_slice(&scales[..4]),
        extra: Vec4::new(scales[4], lighting::exposure(), 0.0, 0.0),
        coefficients,
        intensity,
        surface_dummy: Some(sampler),
        ..default()
    };
    if let Some(t) = terrain {
        t.apply(&mut extension);
    } else if let Some(l) = look {
        super::apply_surface(&l.surface, &mut extension, &mut |b, how| cache.image(source, decoded.0, images, b, how));
    }
    let h = halo_materials.add(HaloMaterial {
        base: std_material,
        extension,
    });
    cache.halo.insert((lit, key), h.id());
    (None, Some(h), translucent)
}

/// Uploads up to `budget` triangles of a built unit, spawning its chunks
/// hidden (the culling shows them once their BSP is shown). Returns the
/// triangles uploaded.
#[allow(clippy::too_many_arguments)]
pub fn upload(
    built: &mut BuiltUnit,
    budget: u32,
    entities: &mut Vec<Entity>,
    commands: &mut Commands,
    source: &WorldSource,
    cache: &mut GpuCache,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
    halo_materials: &mut Assets<HaloMaterial>,
) -> u32 {
    let mut done = 0u32;
    while done < budget {
        let Some(chunk) = built.chunks.pop() else {
            break;
        };
        done += chunk.triangles;
        let (standard, halo, translucent) = chunk_material(&chunk, (&built.bitmaps, &built.lightmaps), source, cache, images, materials, halo_materials);
        let name = source
            .input
            .scene
            .materials
            .get(chunk.material as usize)
            .map_or("unknown", |m| m.tag_path.as_str());
        let mut entity = commands.spawn((
            Mesh3d(meshes.add(chunk.mesh)),
            Name::new(name.to_string()),
            chunk.member,
            Triangles(chunk.triangles),
            chunk.cull,
            built.unit,
            Visibility::Hidden,
        ));
        match (standard, halo) {
            (Some(s), _) => {
                entity.insert((MeshMaterial3d(s), super::sky::SkyBatch(chunk.material)));
            }
            (None, Some(h)) => {
                entity.insert((MeshMaterial3d(h), BatchMaterial(chunk.material)));
            }
            (None, None) => {}
        }
        if translucent {
            entity.insert(NotShadowCaster);
        }
        entities.push(entity.id());
    }
    done
}

/// Keeps the loaded BSPs' units on the GPU and the others off it.
#[allow(clippy::too_many_arguments)]
fn stream_units(
    mut commands: Commands,
    source: Option<Res<WorldSource>>,
    mut residency: ResMut<Residency>,
    mut loaded: ResMut<LoadedBsps>,
    mut cache: ResMut<GpuCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut halo_materials: ResMut<Assets<HaloMaterial>>,
) {
    let Some(source) = source else {
        return;
    };
    let residency = &mut *residency;
    let all = source.input.scene.all_bsps();
    let want: Vec<Unit> = Unit::of_mask(loaded.mask & all).collect();
    let ahead: Vec<Unit> = Unit::of_mask(residency.prefetch & all).collect();

    // Collect finished builds.
    let finished: Vec<Unit> = residency
        .building
        .iter()
        .filter_map(|(u, r)| r.lock().ok()?.try_recv().ok().map(|b| (*u, b)))
        .map(|(u, b)| {
            info!(
                "stream: built {u:?} in {:.2} s: {} chunks, {} triangles",
                b.seconds,
                b.chunks.len(),
                b.chunks.iter().map(|c| c.triangles as usize).sum::<usize>()
            );
            residency.ready.insert(u, b);
            u
        })
        .collect();
    for u in finished {
        residency.building.remove(&u);
    }

    // Start what is wanted and not there.
    for u in want.iter().chain(&ahead) {
        if !residency.resident.contains_key(u)
            && !residency.ready.contains_key(u)
            && !residency.building.contains_key(u)
            && !residency.uploading.contains_key(u)
        {
            start_build(residency, &source, *u);
        }
    }
    // Drop prepared units nobody wants any more.
    residency.ready.retain(|u, _| want.contains(u) || ahead.contains(u));

    // Upload the wanted units that are ready, a slice per frame.
    let mut budget = UPLOAD_BUDGET;
    for u in &want {
        if budget == 0 {
            break;
        }
        let Some(mut built) = residency.ready.remove(u) else {
            continue;
        };
        let mut entities = residency.uploading.remove(u).unwrap_or_default();
        budget = budget.saturating_sub(upload(
            &mut built,
            budget,
            &mut entities,
            &mut commands,
            &source,
            &mut cache,
            &mut meshes,
            &mut images,
            &mut materials,
            &mut halo_materials,
        ));
        if built.chunks.is_empty() {
            residency.resident.insert(*u, entities);
        } else {
            residency.uploading.insert(*u, entities);
            residency.ready.insert(*u, built);
        }
    }

    // Everything wanted is up: show it and let the rest go.
    if want.iter().all(|u| residency.resident.contains_key(u)) {
        let gone: Vec<Unit> = residency.resident.keys().filter(|u| !want.contains(u)).copied().collect();
        for u in &gone {
            for e in residency.resident.remove(u).unwrap_or_default() {
                commands.entity(e).despawn();
            }
        }
        // Partial uploads of units no longer wanted.
        let stale: Vec<Unit> = residency.uploading.keys().filter(|u| !want.contains(u)).copied().collect();
        for u in stale {
            for e in residency.uploading.remove(&u).unwrap_or_default() {
                commands.entity(e).despawn();
            }
        }
        if loaded.shown != loaded.mask {
            loaded.shown = loaded.mask;
        }
        if residency.reported != Some(loaded.shown) {
            residency.reported = Some(loaded.shown);
            residency.report_in = Some((3, gone));
        }
    }
    if let Some((frames, gone)) = residency.report_in.take() {
        if frames > 0 {
            residency.report_in = Some((frames - 1, gone));
        } else {
            info!(
                "stream: showing BSP mask {:#x}; dropped {gone:?}; {:.1} MiB of textures and lightmaps alive, {} meshes",
                loaded.shown,
                cache.live_image_bytes(&images) as f64 / (1 << 20) as f64,
                meshes.len()
            );
        }
    }
}

/// Fires the scenario's zone-set switch volumes (`zone-sets.md` 5) when
/// the skater, or the camera when not skating, enters one: `begin` builds
/// the next zone set ahead, `commit` switches to it. Volumes the view
/// starts in are ignored until left. See [`ZoneTriggers`].
#[allow(clippy::too_many_arguments)]
fn zone_triggers(
    source: Option<Res<WorldSource>>,
    zones: Res<Zones>,
    mut loaded: ResMut<LoadedBsps>,
    mut residency: ResMut<Residency>,
    cameras: Query<&GlobalTransform, With<FlyCamera>>,
    rider: Option<Res<halo3_core::skate::RiderPose>>,
    on: Option<Res<ZoneTriggers>>,
    mut inside: Local<Option<Vec<bool>>>,
) {
    let Some(source) = source else {
        return;
    };
    let scene = &source.input.scene;
    // Only while a zone set is loaded: `--bsps`, `--all-bsps` and Home
    // choose BSPs by hand.
    if !on.is_some_and(|o| o.0) || scene.zone_switches.is_empty() || loaded.zone_set.is_none() {
        *inside = None;
        return;
    }
    let Ok(camera) = cameras.single() else {
        return;
    };
    // The skater's hips while riding, else the camera.
    let at = rider
        .as_deref()
        .filter(|r| r.active)
        .and_then(|r| {
            let i = r.names.iter().position(|n| n == "HIPS")?;
            Some(r.bones.get(i)?.w_axis.truncate() / r.scale)
        })
        .unwrap_or_else(|| camera.translation());
    let at = super::to_halo(at);
    let now: Vec<bool> = scene
        .zone_switches
        .iter()
        .map(|z| scene.trigger_volumes.get(z.volume as usize).is_some_and(|v| v.contains(at)))
        .collect();
    let Some(before) = inside.replace(now.clone()) else {
        return;
    };
    for (k, z) in scene.zone_switches.iter().enumerate() {
        if !now[k] || before.get(k).copied().unwrap_or(true) {
            continue;
        }
        let volume = scene.trigger_volumes.get(z.volume as usize).map_or("?", |v| v.name.as_str());
        if let Some(b) = z.begin.and_then(|b| zones.layout.zone_sets.get(b as usize)) {
            residency.prefetch |= b.bsps;
            info!("trigger {volume}: preparing zone set {}", b.name);
        }
        if let Some(c) = z.commit
            && loaded.zone_set != Some(c as usize)
        {
            info!("trigger {volume}: switching");
            zones::load_zone_set(&mut loaded, &zones, c as usize);
            residency.prefetch = 0;
        }
    }
}
