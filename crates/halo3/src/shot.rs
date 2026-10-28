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

use std::path::PathBuf;

use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};

use crate::fly_camera::FlyCamera;
use crate::skate::RiderPose;
use crate::world::{to_render, to_render_dir};

/// Seconds to wait before the first picture, for shaders to compile (they
/// build in the background) and meshes and textures to reach the GPU; later
/// views only need a moment. `HALO3_SHOT_WAIT` overrides the first.
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
}

pub struct ShotPlugin(pub ShotPlan);

impl Plugin for ShotPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.0.clone())
            .init_resource::<Progress>()
            .add_systems(PostStartup, aim_at_image)
            .add_systems(Update, step);
    }
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
        if let Some(view) = plan.views.first().filter(|_| !plan.skate) {
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

fn step(
    mut commands: Commands,
    plan: Res<ShotPlan>,
    mut progress: ResMut<Progress>,
    mut cameras: Query<(&mut Transform, &mut Projection), With<FlyCamera>>,
    pose: Option<Res<RiderPose>>,
    mut exit: MessageWriter<AppExit>,
    time: Res<Time<Real>>,
) {
    let Some(target) = progress.target.clone() else {
        return;
    };
    // In skate mode views 0..RIDER_VIEWS follow the rider; the first
    // `--view` was the drop point, the rest come after.
    let skate = plan.skate.then_some(pose.as_deref()).flatten();
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
            progress.wait = NEXT_WAIT;
        }
        return;
    }
    if progress.wait > 0.0 {
        progress.wait -= time.delta_secs();
        return;
    }
    let path = plan.dir.join(format!("{}_{}.png", plan.name, progress.view));
    eprintln!("shot: {}", path.display());
    if let Some(pose) = skate
        && let Some(i) = pose.names.iter().position(|n| n == "HIPS")
    {
        let h = crate::world::to_halo(pose.bones[i].w_axis.truncate() / pose.scale);
        let c = pose.camera.map(|(v, _)| crate::world::to_halo(v.translation));
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
