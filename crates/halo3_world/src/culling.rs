//! Which chunks are drawn this frame, on top of bevy's frustum culling.
//!
//! - **Zone sets**: a chunk of a BSP that is not shown is hidden
//!   (`LoadedBsps::shows`).
//! - **Portals**: in every shown BSP the camera's cluster is found through
//!   the collision tree (`ClusterLocator`), then a flood fill walks the
//!   cluster portals (`docs/format/sbsp-portals.md`), each step keeping only
//!   the part of the screen the portal covers (a rectangle in normalised
//!   device coordinates). Clusters the flood never reaches cannot be seen and
//!   their chunks are hidden. A camera inside no cluster of a BSP (in a wall,
//!   outside the level) sees all of that BSP.
//! - **Size**: a chunk of instances or objects whose largest placement
//!   would cover less than [`MIN_PIXELS`] on screen at the chunk's nearest
//!   point is hidden, and a chunk of instances whose BSP gives them a
//!   fade-out size (`docs/format/lod.md`) once smaller than that. Clusters
//!   and the sky are never dropped this way.
//!
//! `HALO3_PORTALS=0` and `HALO3_MIN_PIXELS=0` turn the last two off, for
//! comparisons.

use bevy::camera::primitives::Aabb;
use bevy::prelude::*;
use halo3_core::map::MapAppExt;

use blam_geometry::{BspInfo, WORLD_UNIT_METRES};

use super::CollisionOverlay;
use super::build::Cull;
use super::stream::WorldSource;
use super::zones::{BspMember, LoadedBsps, ShowCollision};
use halo3_core::fly_camera::FlyCamera;

/// The smallest a prop may look on screen, in pixels of a 720-line view
/// (its bounding radius), before its chunk is dropped.
pub const MIN_PIXELS: f32 = 1.5;

/// Per shown BSP: the clusters the camera can see, `None` for all.
#[derive(Resource, Default)]
pub struct PortalView {
    pub visible: Vec<(u32, Option<Vec<bool>>)>,
    /// The camera's cluster in each BSP that holds it, for the log.
    pub camera: Vec<(u32, u32)>,
}

impl PortalView {
    fn sees(&self, bsp: u32, clusters: &[u16]) -> bool {
        match self.visible.iter().find(|(b, _)| *b == bsp) {
            Some((_, Some(v))) => clusters.iter().any(|c| v.get(*c as usize).copied().unwrap_or(true)),
            _ => true,
        }
    }
}

pub struct CullingPlugin;

impl Plugin for CullingPlugin {
    fn build(&self, app: &mut App) {
        app.init_map_resource::<PortalView>()
            .add_systems(PostUpdate, (portal_flood, apply_culling).chain().before(TransformSystems::Propagate).run_if(halo3_core::map::ready));
    }
}

/// A rectangle of the screen in normalised device coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect2 {
    lo: Vec2,
    hi: Vec2,
}

impl Rect2 {
    const FULL: Rect2 = Rect2 {
        lo: Vec2::splat(-1.0),
        hi: Vec2::splat(1.0),
    };

    fn intersect(self, o: Rect2) -> Option<Rect2> {
        let r = Rect2 {
            lo: self.lo.max(o.lo),
            hi: self.hi.min(o.hi),
        };
        (r.lo.x < r.hi.x && r.lo.y < r.hi.y).then_some(r)
    }

    fn union(self, o: Rect2) -> Rect2 {
        Rect2 {
            lo: self.lo.min(o.lo),
            hi: self.hi.max(o.hi),
        }
    }

    fn contains(self, o: Rect2) -> bool {
        self.lo.cmple(o.lo).all() && self.hi.cmpge(o.hi).all()
    }
}

/// The screen rectangle a portal polygon covers: the polygon (Halo units)
/// clipped to the space in front of the camera, projected. `None` when it is
/// wholly behind the camera.
fn portal_rect(vertices: &[Vec3], clip_from_halo: &Mat4) -> Option<Rect2> {
    const NEAR_W: f32 = 1e-3;
    let clip: Vec<Vec4> = vertices.iter().map(|v| *clip_from_halo * v.extend(1.0)).collect();
    let mut lo = Vec2::splat(f32::INFINITY);
    let mut hi = Vec2::splat(f32::NEG_INFINITY);
    let mut add = |c: Vec4| {
        let p = c.truncate().truncate() / c.w;
        lo = lo.min(p);
        hi = hi.max(p);
    };
    // Sutherland-Hodgman against w = NEAR_W, projecting as we go.
    for k in 0..clip.len() {
        let a = clip[k];
        let b = clip[(k + 1) % clip.len()];
        let (ia, ib) = (a.w > NEAR_W, b.w > NEAR_W);
        if ia {
            add(a);
        }
        if ia != ib {
            let t = (NEAR_W - a.w) / (b.w - a.w);
            add(a + (b - a) * t);
        }
    }
    lo.x.is_finite().then_some(Rect2 { lo, hi })
}

/// The clusters of `bsp` seen from `cluster` at `eye` (Halo units).
fn flood(bsp: &BspInfo, cluster: u32, eye: Vec3, clip_from_halo: &Mat4) -> Vec<bool> {
    let n = bsp.clusters.len();
    let mut adjacent: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, p) in bsp.portals.iter().enumerate() {
        adjacent[p.back as usize].push(i);
        if p.front != p.back {
            adjacent[p.front as usize].push(i);
        }
    }
    let mut seen: Vec<Option<Rect2>> = vec![None; n];
    let mut visible = vec![false; n];
    visible[cluster as usize] = true;
    seen[cluster as usize] = Some(Rect2::FULL);
    let mut stack = vec![(cluster, Rect2::FULL)];
    let mut steps = 0;
    while let Some((c, rect)) = stack.pop() {
        steps += 1;
        if steps > 20_000 {
            // Should not happen; never hide what might be seen.
            return vec![true; n];
        }
        for &i in &adjacent[c as usize] {
            let p = &bsp.portals[i];
            let other = p.other(c);
            // Standing in the portal: it covers whatever is in view.
            let plane_n = (p.vertices[1] - p.vertices[0]).cross(p.vertices[2] - p.vertices[0]).normalize_or_zero();
            let near = plane_n.dot(eye - p.centroid).abs() < 0.5 && eye.distance(p.centroid) < p.radius + 0.5;
            let r = if near {
                Some(rect)
            } else {
                portal_rect(&p.vertices, clip_from_halo).and_then(|r| r.intersect(rect))
            };
            let Some(r) = r else { continue };
            if seen[other as usize].is_some_and(|s| s.contains(r)) {
                continue;
            }
            seen[other as usize] = Some(seen[other as usize].map_or(r, |s| s.union(r)));
            visible[other as usize] = true;
            stack.push((other, r));
        }
    }
    visible
}

fn portals_on() -> bool {
    static ON: std::sync::LazyLock<bool> =
        std::sync::LazyLock::new(|| std::env::var("HALO3_PORTALS").map_or(true, |v| v != "0"));
    *ON
}

fn min_pixels() -> f32 {
    static PX: std::sync::LazyLock<f32> = std::sync::LazyLock::new(|| {
        std::env::var("HALO3_MIN_PIXELS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(MIN_PIXELS)
    });
    *PX
}

/// The renderer frame from Halo units: `render = 3.048 * (x, z, -y)`.
fn render_from_halo() -> Mat4 {
    Mat4::from_cols(
        Vec4::new(1.0, 0.0, 0.0, 0.0),
        Vec4::new(0.0, 0.0, -1.0, 0.0),
        Vec4::new(0.0, 1.0, 0.0, 0.0),
        Vec4::W,
    ) * Mat4::from_scale(Vec3::splat(WORLD_UNIT_METRES))
}

fn portal_flood(
    source: Option<Res<WorldSource>>,
    loaded: Res<LoadedBsps>,
    cameras: Query<(&GlobalTransform, &Camera), With<FlyCamera>>,
    mut view: ResMut<PortalView>,
) {
    let Some(source) = source else {
        return;
    };
    let Ok((transform, camera)) = cameras.single() else {
        return;
    };
    let eye = super::to_halo(transform.translation());
    let clip_from_halo = camera.clip_from_view() * transform.to_matrix().inverse() * render_from_halo();
    let mut visible = Vec::new();
    let mut at = Vec::new();
    if portals_on() {
        for bsp in source.input.scene.bsps.iter().filter(|b| loaded.shown & (1 << b.index) != 0) {
            let Some(c) = bsp.locator.as_ref().and_then(|l| l.cluster(eye)) else {
                continue;
            };
            if c as usize >= bsp.clusters.len() {
                continue;
            }
            at.push((bsp.index, c));
            visible.push((bsp.index, Some(flood(bsp, c, eye, &clip_from_halo))));
        }
    }
    if view.camera != at {
        debug!("camera in clusters {at:?}");
    }
    view.visible = visible;
    view.camera = at;
}

#[allow(clippy::type_complexity)]
fn apply_culling(
    loaded: Res<LoadedBsps>,
    show: Res<ShowCollision>,
    view: Res<PortalView>,
    cameras: Query<(&GlobalTransform, &Projection), With<FlyCamera>>,
    mut chunks: Query<(
        &BspMember,
        Option<&Cull>,
        Option<&Aabb>,
        &mut Visibility,
        Has<CollisionOverlay>,
        Has<halo3_core::objects::ScriptHidden>,
    )>,
) {
    let camera = cameras.single().ok();
    let eye = camera.map(|(t, _)| t.translation());
    // Pixels per unit of size at unit distance, on a 720-line view.
    let pixels = match camera.map(|(_, p)| p) {
        Some(Projection::Perspective(p)) => 360.0 / (p.fov * 0.5).tan(),
        _ => 469.0,
    };
    let min_px = min_pixels();
    for (member, cull, aabb, mut visibility, overlay, hidden) in &mut chunks {
        let mut shown = loaded.shows(member) && (!overlay || show.0) && !hidden;
        if shown && let Some(cull) = cull {
            if let Some(b) = cull.bsp
                && !view.sees(b, &cull.clusters)
            {
                shown = false;
            }
            if shown
                && cull.detail.is_finite()
                && let (Some(eye), Some(aabb)) = (eye, aabb)
            {
                let centre = Vec3::from(aabb.center);
                let half = Vec3::from(aabb.half_extents);
                let d = ((eye - centre).abs() - half).max(Vec3::ZERO).length();
                // The largest placement's radius and diameter on screen.
                let radius_px = cull.detail * pixels;
                if (min_px > 0.0 && radius_px < min_px * d) || 2.0 * radius_px < cull.fade_pixels * d {
                    shown = false;
                }
            }
        }
        let want = if shown { Visibility::Inherited } else { Visibility::Hidden };
        if *visibility != want {
            *visibility = want;
        }
    }
}
