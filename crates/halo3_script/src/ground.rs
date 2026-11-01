//! What a crate scripts let go comes down on (`crate::crate_physics`).
//! 120_halo's tunnels hang `tunnel_girder` crates (`bloc`, with a `phmo`)
//! across their ceilings; their `fall` damage section loosens the hinge
//! holding each up, `destroy` breaks it.
//!
//! The floor plates are scenery, not crates: they keep tumbling away into
//! the chasm. Only the collision around the crates is kept here: the
//! triangles under and round each one's footprint, down to [`LAND_REACH`]
//! under its lowest point, from the structure BSPs and the other objects.

use std::collections::{HashMap, HashSet};

use blam_geometry::Collision;
use blam_geometry::collision::surface_flags;
use glam::{Vec2, Vec3};

use crate::objects::ObjectInfo;

/// A crate lands on what lies at most this far under it, world units
/// (122 m; the tunnels' girders fall 8 to 25, the chasm floor lies 75 and
/// more under the run). Anything further down is out of sight below the
/// Warthog run, so a crate over the chasm tumbles away and is gone after
/// [`crate::objects::FALL_SECONDS`], as the floor plates do.
pub const LAND_REACH: f32 = 40.0;

/// How far round a crate's footprint its floor is kept, world units: room
/// for it to tumble and slide (our choice).
const REACH_ROUND: f32 = 8.0;

/// Halo's object type for crates (`bloc`), as scripts number them.
pub const CRATE: i16 = 10;

/// Grid cell size, world units.
const CELL: f32 = 2.0;

#[derive(Clone, Copy, Debug)]
struct Tri {
    a: Vec3,
    b: Vec3,
    c: Vec3,
    /// Scene object index, `u32::MAX` for the structure BSP.
    owner: u32,
}

/// Upward-facing collision in a grid of columns (X, Y), Halo frame.
#[derive(Default)]
pub struct Ground {
    tris: Vec<Tri>,
    cells: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

impl Ground {
    /// `triangles`: corners as wound in the collision (counter-clockwise
    /// seen from the side you stand on), two-sided flag, owner.
    pub fn new(triangles: impl IntoIterator<Item = ([Vec3; 3], bool, u32)>) -> Ground {
        let mut g = Ground::default();
        for ([a, b, c], two_sided, owner) in triangles {
            let n = (b - a).cross(c - a);
            // Walls and ceilings hold nothing up.
            if !n.is_finite() || (n.z <= 1e-6 * n.length() && !two_sided) || n.z.abs() < 1e-9 {
                continue;
            }
            let i = g.tris.len() as u32;
            g.tris.push(Tri { a, b, c, owner });
            let lo = a.min(b).min(c);
            let hi = a.max(b).max(c);
            for x in cell(lo.x)..=cell(hi.x) {
                for y in cell(lo.y)..=cell(hi.y) {
                    g.cells.entry((x, y)).or_default().push(i);
                }
            }
        }
        g
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    /// The highest surface straight under `p` (at most a hundredth of a
    /// unit above it), skipping triangles whose owner `skip` says to.
    pub fn below(&self, p: Vec3, skip: impl Fn(u32) -> bool) -> Option<f32> {
        self.below_hit(p, skip).map(|(z, _, _)| z)
    }

    /// The same, with who owns that surface (a scene object index,
    /// `u32::MAX` for the structure BSP) and its normal (facing up).
    pub fn below_hit(&self, p: Vec3, skip: impl Fn(u32) -> bool) -> Option<(f32, u32, Vec3)> {
        let list = self.cells.get(&(cell(p.x), cell(p.y)))?;
        let q = Vec2::new(p.x, p.y);
        let mut best: Option<(f32, u32, Vec3)> = None;
        for &i in list {
            let t = &self.tris[i as usize];
            let (a, b, c) = (t.a.truncate(), t.b.truncate(), t.c.truncate());
            let d = (b - a).perp_dot(c - a);
            if d.abs() < 1e-9 {
                continue;
            }
            let w1 = (q - a).perp_dot(c - a) / d;
            let w2 = (b - a).perp_dot(q - a) / d;
            let w0 = 1.0 - w1 - w2;
            if w0 < -1e-5 || w1 < -1e-5 || w2 < -1e-5 {
                continue;
            }
            let z = t.a.z * w0 + t.b.z * w1 + t.c.z * w2;
            if z > p.z + 0.01 || best.is_some_and(|(b, _, _)| b >= z) || (t.owner != u32::MAX && skip(t.owner)) {
                continue;
            }
            let n = (t.b - t.a).cross(t.c - t.a).normalize_or(Vec3::Z);
            best = Some((z, t.owner, if n.z < 0.0 { -n } else { n }));
        }
        best
    }
}

/// The crates that can land and what they land on: per object, the corners
/// of its collision where the scenario places it (empty for objects that
/// cannot land), and the ground under them (`None` when no crate scripts
/// can drop has collision).
pub fn build(collision: &Collision, info: &[ObjectInfo]) -> (Vec<Vec<Vec3>>, Option<Ground>) {
    let mut contacts: Vec<Vec<Vec3>> = vec![Vec::new(); info.len()];
    let lands = |o: u32| {
        info.get(o as usize).is_some_and(|i| {
            i.scene == Some(o)
                && i.object_type == CRATE
                && i.sections.iter().any(|s| s.name.contains("fall") || s.constraint.is_some())
        })
    };
    let mut seen: Vec<HashSet<[i32; 3]>> = vec![HashSet::new(); info.len()];
    for t in &collision.triangles {
        let Some(o) = t.source.object().filter(|&o| lands(o)) else { continue };
        for p in t.points {
            if seen[o as usize].insert((p * 1000.0).round().as_ivec3().to_array()) {
                contacts[o as usize].push(p);
            }
        }
    }
    let boxes: Vec<(Vec3, Vec3)> = contacts
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| {
            let lo = c.iter().fold(Vec3::splat(f32::INFINITY), |m, p| m.min(*p));
            let hi = c.iter().fold(Vec3::splat(f32::NEG_INFINITY), |m, p| m.max(*p));
            (lo - Vec3::new(REACH_ROUND, REACH_ROUND, LAND_REACH + 1.0), hi + Vec3::new(REACH_ROUND, REACH_ROUND, 1.0))
        })
        .collect();
    if boxes.is_empty() {
        return (contacts, None);
    }
    let ground = Ground::new(collision.triangles.iter().filter_map(|t| {
        let lo = t.points[0].min(t.points[1]).min(t.points[2]);
        let hi = t.points[0].max(t.points[1]).max(t.points[2]);
        boxes
            .iter()
            .any(|(blo, bhi)| lo.cmple(*bhi).all() && hi.cmpge(*blo).all())
            .then(|| (t.points, t.flags & surface_flags::TWO_SIDED != 0, t.source.object().unwrap_or(u32::MAX)))
    }));
    (contacts, Some(ground))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn highest_surface_under_a_point() {
        let floor = |z: f32, owner: u32| ([Vec3::new(-5.0, -5.0, z), Vec3::new(5.0, -5.0, z), Vec3::new(0.0, 5.0, z)], false, owner);
        // A ceiling (wound facing down) is not ground.
        let ceiling = ([Vec3::new(-5.0, -5.0, 3.0), Vec3::new(0.0, 5.0, 3.0), Vec3::new(5.0, -5.0, 3.0)], false, u32::MAX);
        let g = Ground::new([floor(0.0, u32::MAX), floor(1.0, 7), ceiling]);
        assert_eq!(g.len(), 2);
        assert_eq!(g.below(Vec3::new(0.0, 0.0, 2.0), |_| false), Some(1.0));
        assert_eq!(g.below(Vec3::new(0.0, 0.0, 2.0), |o| o == 7), Some(0.0));
        assert_eq!(g.below(Vec3::new(0.0, 0.0, 0.5), |_| false), Some(0.0));
        assert_eq!(g.below(Vec3::new(0.0, 0.0, -0.5), |_| false), None);
        assert_eq!(g.below(Vec3::new(9.0, 9.0, 2.0), |_| false), None);
    }
}
