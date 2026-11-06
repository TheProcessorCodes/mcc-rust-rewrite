//! A static triangle-soup ground for engine adapters: triangles in Reflex units and frame, bucketed in a
//! uniform grid for the wheel queries (+-1.2 ft boxes) and the lip helper's segment probe. Builders for the
//! primitive shapes engines use (boxes, cylinders, convex point clouds).

use std::collections::HashMap;

use crate::math::{v3, Quat, Vec3};
use crate::sim::{Ground, Triangle};

/// Triangle soup with a uniform grid (cell size in ft).
#[derive(Clone, Debug, Default)]
pub struct MeshGround {
    pub tris: Vec<Triangle>,
    cell: f64,
    grid: HashMap<(i32, i32, i32), Vec<u32>>,
}

/// Builds the triangle normal from the winding (counter-clockwise seen from outside, in the frame the points are
/// given in). Returns None for degenerate triangles.
pub fn triangle(a: Vec3, b: Vec3, c: Vec3) -> Option<Triangle> {
    let n = (b - a).cross(c - a);
    let l = n.length();
    if l < 1e-12 {
        None
    } else {
        Some(Triangle { a, b, c, normal: n / l })
    }
}

impl MeshGround {
    pub fn new(cell: f64) -> Self {
        MeshGround { tris: vec![], cell, grid: HashMap::new() }
    }

    fn key(&self, p: Vec3) -> (i32, i32, i32) {
        ((p.x / self.cell).floor() as i32, (p.y / self.cell).floor() as i32, (p.z / self.cell).floor() as i32)
    }

    pub fn add(&mut self, t: Triangle) {
        let idx = self.tris.len() as u32;
        let lo = v_min(v_min(t.a, t.b), t.c);
        let hi = v_max(v_max(t.a, t.b), t.c);
        let (k0, k1) = (self.key(lo), self.key(hi));
        for x in k0.0..=k1.0 {
            for y in k0.1..=k1.1 {
                for z in k0.2..=k1.2 {
                    self.grid.entry((x, y, z)).or_default().push(idx);
                }
            }
        }
        self.tris.push(t);
    }

    pub fn clear(&mut self) {
        self.tris.clear();
        self.grid.clear();
    }

    /// Indices of triangles whose cells overlap the box lo..hi (deduplicated).
    fn candidates(&self, lo: Vec3, hi: Vec3, out: &mut Vec<u32>) {
        out.clear();
        let (k0, k1) = (self.key(lo), self.key(hi));
        for x in k0.0..=k1.0 {
            for y in k0.1..=k1.1 {
                for z in k0.2..=k1.2 {
                    if let Some(v) = self.grid.get(&(x, y, z)) {
                        out.extend_from_slice(v);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
    }

    /// Adds a box of half extents `h` (local axes) at `center` with orientation `q`.
    pub fn add_box(&mut self, center: Vec3, q: Quat, h: Vec3) {
        let p = |x: f64, y: f64, z: f64| center + q.rotate(v3(x * h.x, y * h.y, z * h.z));
        let c = [
            p(-1., -1., -1.),
            p(1., -1., -1.),
            p(1., 1., -1.),
            p(-1., 1., -1.),
            p(-1., -1., 1.),
            p(1., -1., 1.),
            p(1., 1., 1.),
            p(-1., 1., 1.),
        ];
        let faces = [[0, 3, 2, 1], [4, 5, 6, 7], [0, 1, 5, 4], [3, 7, 6, 2], [0, 4, 7, 3], [1, 2, 6, 5]];
        for f in faces {
            self.add_quad(c[f[0]], c[f[1]], c[f[2]], c[f[3]], center);
        }
    }

    /// Adds a cylinder along local Y (radius, half height) with `segments` sides.
    pub fn add_cylinder(&mut self, center: Vec3, q: Quat, radius: f64, half_height: f64, segments: usize) {
        let ring = |i: usize, y: f64| {
            let a = i as f64 / segments as f64 * std::f64::consts::TAU;
            center + q.rotate(v3(radius * a.cos(), y, radius * a.sin()))
        };
        let (top, bot) = (center + q.rotate(v3(0.0, half_height, 0.0)), center + q.rotate(v3(0.0, -half_height, 0.0)));
        for i in 0..segments {
            let j = (i + 1) % segments;
            let (a0, a1, b0, b1) = (ring(i, half_height), ring(j, half_height), ring(i, -half_height), ring(j, -half_height));
            self.add_quad(b0, b1, a1, a0, center);
            self.add_oriented(top, a0, a1, center);
            self.add_oriented(bot, b1, b0, center);
        }
    }

    /// Adds the convex hull of `points` (small point counts; O(n^4) worst case).
    pub fn add_convex(&mut self, points: &[Vec3]) {
        for f in convex_hull_faces(points) {
            for i in 1..f.len() - 1 {
                if let Some(t) = triangle(f[0], f[i], f[i + 1]) {
                    self.add(t);
                }
            }
        }
    }

    /// Adds a quad, flipping it if needed so the normal points away from `inside`.
    fn add_quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, inside: Vec3) {
        self.add_oriented(a, b, c, inside);
        self.add_oriented(a, c, d, inside);
    }

    fn add_oriented(&mut self, a: Vec3, b: Vec3, c: Vec3, inside: Vec3) {
        if let Some(t) = triangle(a, b, c) {
            if t.normal.dot(a - inside) < 0.0 {
                self.add(Triangle { a, b: c, c: b, normal: -t.normal });
            } else {
                self.add(t);
            }
        }
    }
}

impl Ground for MeshGround {
    fn triangles(&self, center: Vec3, half_extent: f64, out: &mut Vec<Triangle>) {
        let h = Vec3 { x: half_extent, y: half_extent, z: half_extent };
        let (lo, hi) = (center - h, center + h);
        let mut idx = vec![];
        self.candidates(lo, hi, &mut idx);
        for i in idx {
            let t = &self.tris[i as usize];
            let tlo = v_min(v_min(t.a, t.b), t.c);
            let thi = v_max(v_max(t.a, t.b), t.c);
            if (0..3).all(|k| thi[k] >= lo[k] && tlo[k] <= hi[k]) {
                out.push(*t);
            }
        }
    }

    fn raycast(&self, a: Vec3, b: Vec3) -> Option<(Vec3, Vec3)> {
        let mut idx = vec![];
        self.candidates(v_min(a, b), v_max(a, b), &mut idx);
        let d = b - a;
        let mut best: Option<(f64, Vec3)> = None;
        for &i in &idx {
            let t = &self.tris[i as usize];
            // Moller-Trumbore, both sides; keep the nearest hit along a -> b
            let (e1, e2) = (t.b - t.a, t.c - t.a);
            let p = d.cross(e2);
            let det = e1.dot(p);
            if det.abs() < 1e-12 {
                continue;
            }
            let s = a - t.a;
            let u = s.dot(p) / det;
            let q = s.cross(e1);
            let v = d.dot(q) / det;
            let w = e2.dot(q) / det;
            if (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && (0.0..=1.0).contains(&w) && best.is_none_or(|(bw, _)| w < bw) {
                best = Some((w, t.normal));
            }
        }
        best.map(|(w, n)| (a + d * w, n))
    }
}

/// Faces of the convex hull of a small point cloud, each as a polygon (counter-clockwise from outside).
pub fn convex_hull_faces(points: &[Vec3]) -> Vec<Vec<Vec3>> {
    let n = points.len();
    if n < 4 {
        return vec![];
    }
    let scale = points.iter().fold(0.0f64, |m, p| m.max(p.length())).max(1.0);
    let eps = 1e-9 * scale;
    let centroid = points.iter().fold(Vec3::ZERO, |s, p| s + *p) / n as f64;
    let mut planes: Vec<(Vec3, f64)> = vec![];
    for i in 0..n {
        for j in i + 1..n {
            for k in j + 1..n {
                let nrm = (points[j] - points[i]).cross(points[k] - points[i]);
                let l = nrm.length();
                if l < eps {
                    continue;
                }
                let mut nrm = nrm / l;
                let mut d = nrm.dot(points[i]);
                if nrm.dot(centroid) > d {
                    nrm = -nrm;
                    d = -d;
                }
                if points.iter().all(|p| nrm.dot(*p) <= d + eps)
                    && !planes.iter().any(|(m, e)| (*m - nrm).length() < 1e-6 && (e - d).abs() < 1e-6 * scale)
                {
                    planes.push((nrm, d));
                }
            }
        }
    }
    planes
        .into_iter()
        .map(|(nrm, d)| {
            let mut on: Vec<Vec3> = points.iter().copied().filter(|p| (nrm.dot(*p) - d).abs() <= eps * 10.0).collect();
            on.dedup_by(|a, b| (*a - *b).length() < eps);
            let c = on.iter().fold(Vec3::ZERO, |s, p| s + *p) / on.len() as f64;
            let (t, b) = nrm.any_orthonormal_pair();
            on.sort_by(|p, q| {
                let ap = (*p - c).dot(b).atan2((*p - c).dot(t));
                let aq = (*q - c).dot(b).atan2((*q - c).dot(t));
                ap.total_cmp(&aq)
            });
            // ensure counter-clockwise seen from outside (normal = nrm)
            if on.len() >= 3 && (on[1] - on[0]).cross(on[2] - on[0]).dot(nrm) < 0.0 {
                on.reverse();
            }
            on
        })
        .filter(|f| f.len() >= 3)
        .collect()
}

fn v_min(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x.min(b.x), a.y.min(b.y), a.z.min(b.z))
}

fn v_max(a: Vec3, b: Vec3) -> Vec3 {
    v3(a.x.max(b.x), a.y.max(b.y), a.z.max(b.z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::query;

    #[test]
    fn box_ground_matches_plane() {
        let mut g = MeshGround::new(4.0);
        g.add_box(v3(0.0, -1.0, 0.0), Quat::IDENTITY, v3(20.0, 1.0, 20.0));
        assert_eq!(g.tris.len(), 12);
        let mut buf = vec![];
        let (p, n) = query(&g, v3(1.0, 1.1, 2.0), Quat::IDENTITY, 1.2, &mut buf).unwrap();
        assert!((p - v3(1.0, 0.0, 2.0)).length() < 1e-9, "{p:?}");
        assert!((n - Vec3::Y).length() < 1e-9);
        assert!(g.segment_hits(v3(0.0, 0.5, 0.0), v3(0.0, -0.5, 0.0)));
        assert!(!g.segment_hits(v3(0.0, 0.5, 0.0), v3(5.0, 0.5, 0.0)));
    }

    #[test]
    fn convex_hull_of_a_cube() {
        let pts: Vec<Vec3> = (0..8).map(|i| v3((i & 1) as f64, ((i >> 1) & 1) as f64, ((i >> 2) & 1) as f64)).collect();
        let faces = convex_hull_faces(&pts);
        assert_eq!(faces.len(), 6);
        assert!(faces.iter().all(|f| f.len() == 4));
        let mut g = MeshGround::new(2.0);
        g.add_convex(&pts);
        assert_eq!(g.tris.len(), 12);
        for t in &g.tris {
            let c = (t.a + t.b + t.c) / 3.0;
            assert!(t.normal.dot(c - v3(0.5, 0.5, 0.5)) > 0.0, "normal points inward");
        }
    }

    #[test]
    fn cylinder_normals_point_out() {
        let mut g = MeshGround::new(2.0);
        g.add_cylinder(v3(0.0, 0.0, 0.0), Quat::IDENTITY, 1.0, 0.5, 16);
        for t in &g.tris {
            let c = (t.a + t.b + t.c) / 3.0;
            assert!(t.normal.dot(c) > 0.0);
        }
    }
}
