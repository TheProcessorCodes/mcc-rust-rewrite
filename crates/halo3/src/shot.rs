//! Screenshot mode: render a map without a window, save one PNG per view,
//! exit. For looking at the renderer's output without anyone at the screen.
//!
//! ```text
//! halo3 <map> --shot <dir> [--view x,y,z,yaw,pitch | spawn[N] | overview]... [--size 1280x720]
//! ```
//!
//! A view is a Halo position in world units and a facing in degrees (yaw
//! about +Z from +X, pitch up from level), the numbers P prints in the
//! viewer. Without `--view` the one view is the start spawn. Files are
//! `<dir>/<map>_<n>.png`.
//!
//! With `--skate` the skater is dropped at the first view (or the start
//! spawn) and left to ride with the sticks at rest for two seconds; then the
//! pictures are the engine's chase camera, the rider from the side, from the
//! front, the board close up and the neck close up, followed by any
//! further views.
//!
//! `HALO3_SHOT_EFFECT=path@x,y,z[;path@x,y,z...]` sets effects off (by
//! their tag path, at Halo positions) `HALO3_SHOT_EFFECT_LEAD` seconds
//! (default 0.3) before the first picture; with `HALO3_SHOT_NEXT_WAIT` and
//! several views at one place the pictures follow the explosion.

use std::path::PathBuf;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use halo3_core::coords::{to_halo, to_render, to_render_dir};
use halo3_core::fly_camera::FlyCamera;
use halo3_core::skate::RiderPose;
use halo3_world::Triangles;

/// Seconds to wait before the first picture, for shaders to compile (they
/// build in the background) and meshes and textures to reach the GPU; later
/// views only need a moment. `HALO3_SHOT_WAIT` overrides the first,
/// `HALO3_SHOT_NEXT_WAIT` the rest.
const FIRST_WAIT: f32 = 12.0;
const NEXT_WAIT: f32 = 0.5;
/// Seconds of skating before the first `--skate` picture, and how long to
/// wait for the session at most.
const SKATE_FOR: f32 = 2.0;
const SKATE_TIMEOUT: f32 = 240.0;

/// The pictures taken around the rider in `--skate` mode.
const RIDER_VIEWS: usize = 5;

#[derive(Clone, Copy, Debug)]
pub struct View {
    pub position: Vec3,
    pub yaw_deg: f32,
    pub pitch_deg: f32,
}

impl View {
    /// Spawn point `n` at Spartan eye height, facing its way; spawn 0 is
    /// where the viewer starts.
    pub fn spawn(scene: &blam_geometry::Scene, n: usize) -> Option<View> {
        let s = scene.spawns.get(n)?;
        Some(View {
            position: s.position + Vec3::Z * 0.6,
            yaw_deg: s.yaw.to_degrees(),
            pitch_deg: 0.0,
        })
    }

    /// High over one corner of the spawn points (the playable space; the
    /// BSP's own bounds reach far out to the scenery around it), looking
    /// down at their middle. Without spawns, the BSP's clusters.
    pub fn overview(scene: &blam_geometry::Scene) -> View {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for s in &scene.spawns {
            lo = lo.min(s.position);
            hi = hi.max(s.position);
        }
        if scene.spawns.len() < 2 {
            for p in scene
                .placements
                .iter()
                .filter(|p| p.kind == blam_geometry::Kind::Cluster)
            {
                for v in &scene.meshes[p.mesh as usize].positions {
                    let w = p.transform.transform_point3(*v);
                    lo = lo.min(w);
                    hi = hi.max(w);
                }
            }
        }
        if !lo.x.is_finite() {
            (lo, hi) = (Vec3::splat(-10.0), Vec3::splat(10.0));
        }
        let centre = (lo + hi) * 0.5;
        let size = (hi - lo).max(Vec3::splat(10.0));
        let eye = Vec3::new(
            centre.x - size.x * 0.4,
            centre.y - size.y * 0.4,
            hi.z + size.x.max(size.y) * 0.45,
        );
        let d = centre - eye;
        View {
            position: eye,
            yaw_deg: d.y.atan2(d.x).to_degrees(),
            pitch_deg: d.z.atan2(d.truncate().length()).to_degrees(),
        }
    }
}

impl std::str::FromStr for View {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let v: Vec<f32> = s
            .split(',')
            .map(|x| x.trim().parse::<f32>().map_err(|e| format!("view {s:?}: {e}")))
            .collect::<Result<_, _>>()?;
        match v[..] {
            [x, y, z, yaw, pitch] => Ok(View {
                position: Vec3::new(x, y, z),
                yaw_deg: yaw,
                pitch_deg: pitch,
            }),
            [x, y, z, yaw] => Ok(View {
                position: Vec3::new(x, y, z),
                yaw_deg: yaw,
                pitch_deg: 0.0,
            }),
            _ => Err(format!("view {s:?}: want x,y,z,yaw[,pitch]")),
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct ShotPlan {
    pub dir: PathBuf,
    pub name: String,
    /// Empty: keep the camera where the world put it (the start spawn).
    pub views: Vec<View>,
    pub size: UVec2,
    /// Skate before the pictures (`--skate`).
    pub skate: bool,
    /// Frames to time before each picture (`--bench N`; 0: none).
    pub bench: u32,
    /// Ride the dirt bike before the pictures (`--bike`).
    pub bike: bool,
    /// Stand (or walk) on foot before the pictures (`--foot`).
    pub foot: bool,
}

impl ShotPlan {
    /// Whether a rider is dropped in before the pictures.
    pub fn rides(&self) -> bool {
        self.skate || self.bike || self.foot
    }
}

#[derive(Resource, Default)]
struct Progress {
    target: Option<Handle<Image>>,
    view: usize,
    wait: f32,
    /// A screenshot is in flight.
    pending: bool,
    saved: usize,
    /// Seconds spent waiting for the skater.
    waited: f32,
    /// The bench of the current view: frames left, frame times so far.
    bench_left: Option<u32>,
    frame_ms: Vec<f32>,
    /// `HALO3_SHOT_EFFECT` has been set off.
    fired: bool,
}

/// The effects `HALO3_SHOT_EFFECT` asks for: `path@x,y,z`, `;` between.
fn shot_effects() -> Vec<(String, Vec3)> {
    let Ok(v) = std::env::var("HALO3_SHOT_EFFECT") else { return Vec::new() };
    v.split(';')
        .filter_map(|e| {
            let (path, at) = e.rsplit_once('@')?;
            let p: Vec<f32> = at.split(',').filter_map(|x| x.trim().parse().ok()).collect();
            (p.len() == 3).then(|| (path.trim().to_string(), Vec3::new(p[0], p[1], p[2])))
        })
        .collect()
}

pub struct ShotPlugin(pub ShotPlan);

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .init_resource::<Progress>()
            .add_systems(halo3_core::map::MapPostStartup, aim_at_image)
            .add_systems(Update, step.run_if(halo3_core::map::ready));
        if self.0.bench > 0 {
            // Without a swap chain nothing makes the CPU wait for the GPU, and
            // frame times would only measure the CPU: wait for every frame.
            app.sub_app_mut(bevy::render::RenderApp).add_systems(
                bevy::render::Render,
                wait_for_gpu.in_set(bevy::render::RenderSystems::Cleanup),
            );
        }
    }
}

fn wait_for_gpu(device: Res<bevy::render::renderer::RenderDevice>) {
    let _ = device.poll(bevy::render::render_resource::PollType::wait_indefinitely());
}

fn place(view: &View, transform: &mut Transform) {
    let (yaw, pitch) = (view.yaw_deg.to_radians(), view.pitch_deg.to_radians());
    let facing = Vec3::new(yaw.cos() * pitch.cos(), yaw.sin() * pitch.cos(), pitch.sin());
    let eye = to_render(view.position);
    *transform = Transform::from_translation(eye).looking_to(to_render_dir(facing), Vec3::Y);
}

/// Puts the camera on rider view `n` (`RIDER_VIEWS`): the engine's camera,
/// then side, front and the board, all framed on the posed bones.
fn place_rider(n: usize, pose: &RiderPose, transform: &mut Transform, projection: &mut Projection) {
    let at = |name: &str| {
        let i = pose.names.iter().position(|b| b == name)?;
        Some(pose.bones.get(i)?.w_axis.truncate() / pose.scale)
    };
    let (Some(front), Some(back), Some(hips)) = (at("TRUCK_FRONT"), at("TRUCK_BACK"), at("HIPS"))
    else {
        return;
    };
    let mut fov = 50f32.to_radians();
    let board = (front + back) * 0.5;
    let forward = Vec3::new(front.x - back.x, 0.0, front.z - back.z).normalize_or(Vec3::NEG_Z);
    let side = forward.cross(Vec3::Y).normalize_or(Vec3::X);
    let chest = hips + Vec3::Y * 0.3;
    let (eye, target) = match n {
        0 => match pose.camera {
            Some((view, engine_fov)) => {
                *transform = view;
                fov = engine_fov;
                (view.translation, view.translation + view.forward().as_vec3())
            }
            None => (chest - forward * 4.0 + Vec3::Y, chest),
        },
        1 => (chest + side * 3.5 + Vec3::Y * 0.3, chest - Vec3::Y * 0.3),
        2 => (chest + forward * 3.5 + Vec3::Y * 0.3, chest - Vec3::Y * 0.3),
        3 => (board + side * 1.3 + Vec3::Y * 0.6 + forward * 0.4, board + Vec3::Y * 0.2),
        // The collar and helmet, close.
        _ => {
            let neck = at("NECK").unwrap_or(chest + Vec3::Y * 0.5);
            (neck + side * 0.9 + forward * 0.5 + Vec3::Y * 0.15, neck)
        }
    };
    if n != 0 || pose.camera.is_none() {
        *transform = Transform::from_translation(eye).looking_at(target, Vec3::Y);
    }
    if let Projection::Perspective(lens) = projection {
        lens.fov = fov;
    }
}

fn aim_at_image(
    mut commands: Commands,
    plan: Res<ShotPlan>,
    pose: Option<ResMut<RiderPose>>,
    mut progress: ResMut<Progress>,
    mut images: ResMut<Assets<Image>>,
    mut cameras: Query<(Entity, &mut Transform), With<FlyCamera>>,
) {
    let mut image = Image::new_target_texture(
        plan.size.x,
        plan.size.y,
        TextureFormat::Rgba8UnormSrgb,
        None,
    );
    image.texture_descriptor.usage |= bevy::render::render_resource::TextureUsages::COPY_SRC;
    let handle = images.add(image);
    if let Some(mut pose) = pose {
        pose.drive_camera = false;
    }
    for (camera, mut transform) in &mut cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(handle.clone().into()));
        if let Some(view) = plan.views.first().filter(|_| !plan.rides()) {
            place(view, &mut transform);
        }
    }
    progress.target = Some(handle);
    progress.wait = std::env::var("HALO3_SHOT_WAIT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(FIRST_WAIT);
    if let Err(e) = std::fs::create_dir_all(&plan.dir) {
        error!("shot: cannot create {}: {e}", plan.dir.display());
    }
}

/// Logs the frame times of a bench and what the last frame drew: mesh
/// entities that passed visibility (hidden, frustum and portal culling) and
/// their triangles.
fn bench_report(view: usize, frame_ms: &[f32], drawn: &Query<(&ViewVisibility, Option<&Triangles>), With<Mesh3d>>) {
    let mut sorted = frame_ms.to_vec();
    sorted.sort_unstable_by(f32::total_cmp);
    let n = sorted.len().max(1) as f32;
    let mean = sorted.iter().sum::<f32>() / n;
    let pct = |p: f32| sorted.get(((sorted.len() as f32 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    let (mut entities, mut visible, mut triangles) = (0usize, 0usize, 0usize);
    for (v, m) in drawn {
        entities += 1;
        if v.get() {
            visible += 1;
            triangles += m.map_or(0, |t| t.0 as usize);
        }
    }
    eprintln!(
        "bench: view {view}: {} frames, mean {mean:.2} ms, median {:.2} ms, p95 {:.2} ms; {visible} of {entities} meshes drawn, {triangles} triangles",
        sorted.len(),
        pct(0.5),
        pct(0.95)
    );
}

#[allow(clippy::too_many_arguments)]
fn step(
    mut commands: Commands,
    plan: Res<ShotPlan>,
    mut progress: ResMut<Progress>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<FlyCamera>>,
    pose: Option<Res<RiderPose>>,
    mut exit: MessageWriter<AppExit>,
    time: Res<Time<Real>>,
    drawn: Query<(&ViewVisibility, Option<&Triangles>), With<Mesh3d>>,
    portals: Option<Res<halo3_world::culling::PortalView>>,
    effects: Option<ResMut<halo3_core::effects::EffectQueue>>,
) {
    let Some(target) = progress.target.clone() else {
        return;
    };
    // In skate mode views 0..RIDER_VIEWS follow the rider; the first
    // `--view` was the drop point, the rest come after.
    let skate = plan.rides().then_some(pose.as_deref()).flatten();
    let total = match skate {
        Some(_) => RIDER_VIEWS + plan.views.len().saturating_sub(1),
        None => plan.views.len().max(1),
    };
    let halo_view = |n: usize| match skate {
        Some(_) => n.checked_sub(RIDER_VIEWS).and_then(|i| plan.views.get(i + 1)),
        None => plan.views.get(n),
    };
    if let Some(pose) = skate {
        // `HALO3_SKATE_FOR` rides longer (or shorter) before the pictures.
        let skate_for = std::env::var("HALO3_SKATE_FOR")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(SKATE_FOR);
        if pose.skated < skate_for {
            progress.waited += time.delta_secs();
            if progress.waited > SKATE_TIMEOUT {
                error!("shot: the skater never got going");
                exit.write(AppExit::error());
            }
            return;
        }
        if progress.view < RIDER_VIEWS && !progress.pending {
            for (mut t, mut p) in &mut cameras {
                place_rider(progress.view, pose, &mut t, &mut p);
            }
        }
    }
    if progress.pending {
        // The observer below counts finished saves.
        if progress.saved > progress.view {
            progress.pending = false;
            progress.view += 1;
            if progress.view >= total {
                exit.write(AppExit::Success);
                return;
            }
            if let Some(view) = halo_view(progress.view) {
                for (mut t, _) in &mut cameras {
                    place(view, &mut t);
                }
            }
            // `HALO3_SHOT_NEXT_WAIT` spaces the later views (seconds): a
            // run of views at one place makes a time series (an effect
            // going off).
            progress.wait = std::env::var("HALO3_SHOT_NEXT_WAIT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(NEXT_WAIT);
        }
        return;
    }
    if !progress.fired && progress.view == 0 {
        let lead = std::env::var("HALO3_SHOT_EFFECT_LEAD").ok().and_then(|s| s.parse().ok()).unwrap_or(0.3);
        if progress.wait <= lead {
            progress.fired = true;
            if let Some(mut queue) = effects {
                for (path, at) in shot_effects() {
                    eprintln!("shot: effect {path} at halo [{:.2}, {:.2}, {:.2}]", at.x, at.y, at.z);
                    queue.push(halo3_core::effects::EffectRequest::at(path, at));
                }
            }
        }
    }
    if progress.wait > 0.0 {
        progress.wait -= time.delta_secs();
        return;
    }
    if plan.bench > 0 {
        let left = *progress.bench_left.get_or_insert(plan.bench);
        if left > 0 {
            // The first frame's delta still includes the wait.
            if left < plan.bench {
                progress.frame_ms.push(time.delta_secs() * 1000.0);
            }
            progress.bench_left = Some(left - 1);
            return;
        }
        bench_report(progress.view, &progress.frame_ms, &drawn);
        if let Some(v) = portals.as_deref() {
            let seen: Vec<String> = v
                .visible
                .iter()
                .map(|(b, s)| match s {
                    Some(s) => format!("bsp {b}: {} of {} clusters", s.iter().filter(|x| **x).count(), s.len()),
                    None => format!("bsp {b}: all"),
                })
                .collect();
            eprintln!("bench: view {}: camera in clusters {:?}; {}", progress.view, v.camera, seen.join(", "));
        }
        progress.frame_ms.clear();
        progress.bench_left = None;
    }
    let path = plan.dir.join(format!("{}_{}.png", plan.name, progress.view));
    eprintln!("shot: {}", path.display());
    if let Some(pose) = skate
        && let Some(i) = pose.names.iter().position(|n| n == "HIPS")
    {
        let h = to_halo(pose.bones[i].w_axis.truncate() / pose.scale);
        let c = pose.camera.map(|(v, _)| to_halo(v.translation));
        eprintln!("shot: rider hips at halo [{:.2}, {:.2}, {:.2}], engine camera {c:.2?}", h.x, h.y, h.z);
    }
    progress.pending = true;
    commands
        .spawn(Screenshot::image(target))
        .observe(save_to_disk(path))
        .observe(|_: On<ScreenshotCaptured>, mut progress: ResMut<Progress>| {
            progress.saved += 1;
        });
}
