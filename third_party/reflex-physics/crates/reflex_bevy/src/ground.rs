//! What the bike rides on. The game provides it as the [`ReflexGround`] resource, in one of three ways:
//!
//! - **Triangles** ([`TriangleGround`]): hand over the level's collision triangles (and boxes / convex shapes)
//!   once, in metres in Bevy's frame. The adapter keeps them in a grid.
//! - **Your own collision** ([`GroundQuery`]): implement two queries over the game's existing collision
//!   structure (the triangles near a point, and a segment cast), also in Bevy units.
//! - **Bevy meshes** (feature `mesh`, `ReflexMeshGroundPlugin`): tag entities that have a `Mesh3d` with
//!   `ReflexCollider`; the ground is rebuilt from them when they change.
//!
//! Winding: a triangle's riding side is the side it is counter-clockwise from (Bevy's front face, the side its
//! normal `(b - a) x (c - a)` points to). Wheels touch only that side.

use bevy::math::{Quat, Vec3};
use bevy::prelude::Resource;
use reflex_core::ground::MeshGround;
use reflex_core::{Ground, Triangle};

use crate::convert::{dir_to_reflex, pos_to_bevy, pos_to_reflex, quat_to_reflex};

type RVec3 = reflex_core::Vec3;

/// The game's collision, seen from Bevy (metres, Bevy's frame). Implement this to let the bike ride on a
/// collision structure the game already has.
pub trait GroundQuery: Send + Sync + 'static {
    /// Every triangle touching the axis-aligned box `center +- half_extent` (the bike asks about 0.37 m around
    /// each wheel, several times a frame). Wound counter-clockwise seen from the riding side. Up to a few hundred
    /// triangles are fine; the original gives up above 400.
    fn triangles(&self, center: Vec3, half_extent: f32, out: &mut Vec<[Vec3; 3]>);

    /// First hit of the segment `from -> to`: (point, unit normal facing back toward `from`). Used for the
    /// chassis crash points (a segment from the chassis centre to each of 12 points on the bike) and the jump-lip
    /// probe.
    fn raycast(&self, from: Vec3, to: Vec3) -> Option<(Vec3, Vec3)>;
}

/// The ground the bikes ride on (one for all bikes). Empty by default: bikes fall.
#[derive(Resource)]
pub struct ReflexGround {
    inner: Box<dyn Ground + Send + Sync>,
}

impl Default for ReflexGround {
    fn default() -> Self {
        ReflexGround { inner: Box::new(MeshGround::new(8.0)) }
    }
}

impl ReflexGround {
    /// A ground from triangles and primitive shapes (see [`TriangleGround`]).
    pub fn from_triangles(t: TriangleGround) -> Self {
        ReflexGround { inner: Box::new(t.0) }
    }

    /// A ground answered by the game's own collision.
    pub fn from_query(q: impl GroundQuery) -> Self {
        ReflexGround { inner: Box::new(QueryGround { q, buf: Default::default() }) }
    }

    /// A ground in Reflex units for advanced use (any [`reflex_core::Ground`]).
    pub fn from_reflex(g: impl Ground + Send + Sync + 'static) -> Self {
        ReflexGround { inner: Box::new(g) }
    }

    /// The ground in Reflex units, for [`crate::ReflexBike::step`].
    pub fn as_ground(&self) -> &dyn Ground {
        self.inner.as_ref()
    }
}

/// Static collision as a triangle soup in a uniform grid. Build it once from the level and wrap it with
/// [`ReflexGround::from_triangles`]. All inputs in metres, Bevy's frame.
pub struct TriangleGround(MeshGround);

impl Default for TriangleGround {
    fn default() -> Self {
        TriangleGround(MeshGround::new(8.0))
    }
}

impl TriangleGround {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds one triangle, counter-clockwise seen from the riding side. Degenerate triangles are skipped.
    pub fn add_triangle(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        if let Some(t) = reflex_triangle(a, b, c) {
            self.0.add(t);
        }
    }

    /// Adds an indexed triangle list (`positions` already in world space).
    pub fn add_indexed(&mut self, positions: &[Vec3], indices: impl IntoIterator<Item = [u32; 3]>) {
        for [i, j, k] in indices {
            let p = |n: u32| positions.get(n as usize).copied();
            if let (Some(a), Some(b), Some(c)) = (p(i), p(j), p(k)) {
                self.add_triangle(a, b, c);
            }
        }
    }

    /// Adds a solid box (all six faces facing out): centre, rotation, half extents.
    pub fn add_box(&mut self, center: Vec3, rotation: Quat, half_extents: Vec3) {
        let h = half_extents.as_dvec3() / reflex_core::FT_TO_M;
        self.0.add_box(pos_to_reflex(center), quat_to_reflex(rotation), RVec3 { x: h.x, y: h.y, z: h.z });
    }

    /// Adds the convex hull of a point cloud (faces facing out).
    pub fn add_convex(&mut self, points: &[Vec3]) {
        let pts: Vec<RVec3> = points.iter().map(|p| pos_to_reflex(*p)).collect();
        self.0.add_convex(&pts);
    }

    /// Adds a solid cylinder about the rotation's Y axis.
    pub fn add_cylinder(&mut self, center: Vec3, rotation: Quat, radius: f32, half_height: f32, segments: usize) {
        let ft = 1.0 / reflex_core::FT_TO_M;
        self.0.add_cylinder(
            pos_to_reflex(center),
            quat_to_reflex(rotation),
            radius as f64 * ft,
            half_height as f64 * ft,
            segments.max(3),
        );
    }

    /// Triangles held.
    pub fn len(&self) -> usize {
        self.0.tris.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.tris.is_empty()
    }
}

/// A Bevy-frame triangle as a Reflex triangle facing the same way. The Z mirror reverses the winding, so two
/// corners are swapped to keep the normal on the riding side.
pub fn reflex_triangle(a: Vec3, b: Vec3, c: Vec3) -> Option<Triangle> {
    reflex_core::ground::triangle(pos_to_reflex(a), pos_to_reflex(c), pos_to_reflex(b))
}

struct QueryGround<Q> {
    q: Q,
    buf: std::sync::Mutex<Vec<[Vec3; 3]>>,
}

impl<Q: GroundQuery> Ground for QueryGround<Q> {
    fn triangles(&self, center: RVec3, half_extent: f64, out: &mut Vec<Triangle>) {
        let mut buf = self.buf.lock().unwrap_or_else(|e| e.into_inner());
        buf.clear();
        self.q.triangles(pos_to_bevy(center), (half_extent * reflex_core::FT_TO_M) as f32, &mut buf);
        out.extend(buf.iter().filter_map(|[a, b, c]| reflex_triangle(*a, *b, *c)));
    }

    fn raycast(&self, a: RVec3, b: RVec3) -> Option<(RVec3, RVec3)> {
        self.q.raycast(pos_to_bevy(a), pos_to_bevy(b)).map(|(p, n)| (pos_to_reflex(p), dir_to_reflex(n)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upward_triangle_keeps_an_upward_normal() {
        // counter-clockwise seen from above in Bevy's frame: normal +Y
        let (a, b, c) = (Vec3::ZERO, Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0));
        assert!((b - a).cross(c - a).y > 0.0);
        let t = reflex_triangle(a, b, c).unwrap();
        assert!(t.normal.y > 0.99, "{:?}", t.normal);
    }
}
