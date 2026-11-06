//! Our own dirt bike, drawn when the player has no MX vs ATV Reflex: a
//! low-poly 450-class motocrosser built here from boxes, tubes and lathed
//! wheels, nothing read from any game. It has the same six bones the
//! Reflex 450F has (`model.rs` moves them: bars and forks about the
//! steering axis, the fork legs up it, the swingarm, both wheels) and the
//! same axles and tyre sizes (`reflex::DAKUMOTO_450F`), so it sits on the
//! physics the same way.
//!
//! Colours come from a small palette sheet: every vertex points at the
//! middle of its colour's cell.

use std::collections::HashMap;

use bevy::math::{Mat3, Mat4, Vec3};
use reflex_assets::bike::{BikeModel, Bone, Part};
use reflex_assets::texture::Rgba;

use super::physics::RAKE;
use super::reflex::DAKUMOTO_450F;

/// The palette sheet's key in `BikeModel::textures`.
pub const PALETTE: &str = "stand-in palette";

#[derive(Clone, Copy)]
enum Paint {
    Plastic,
    White,
    Seat,
    Rubber,
    Frame,
    Engine,
    Chrome,
    Gold,
    Rim,
    Grip,
}

const PAINTS: [(Paint, [u8; 3]); 10] = [
    (Paint::Plastic, [38, 104, 206]),
    (Paint::White, [232, 234, 236]),
    (Paint::Seat, [28, 30, 34]),
    (Paint::Rubber, [22, 22, 23]),
    (Paint::Frame, [168, 172, 180]),
    (Paint::Engine, [66, 68, 74]),
    (Paint::Chrome, [196, 198, 204]),
    (Paint::Gold, [205, 160, 58]),
    (Paint::Rim, [40, 42, 48]),
    (Paint::Grip, [12, 12, 12]),
];
const CELL: u32 = 32;
const CELLS: u32 = 16;

fn uv(p: Paint) -> [f32; 2] {
    let i = PAINTS.iter().position(|(q, _)| *q as u8 == p as u8).unwrap_or(0);
    [(i as f32 + 0.5) / CELLS as f32, 0.5]
}

fn palette() -> Rgba {
    let (w, h) = (CELL * CELLS, CELL);
    let pixels = (0..w * h)
        .map(|i| {
            let cell = ((i % w) / CELL) as usize;
            let [r, g, b] = PAINTS.get(cell).map_or([128; 3], |(_, c)| *c);
            [r, g, b, 255]
        })
        .collect();
    Rgba { width: w, height: h, pixels }
}

/// One bone's geometry, authored in the bike frame.
#[derive(Default)]
struct Geo {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    uvs: Vec<[f32; 2]>,
    indices: Vec<u32>,
}

impl Geo {
    fn vertex(&mut self, p: Vec3, n: Vec3, paint: Paint) -> u32 {
        self.positions.push(p.into());
        self.normals.push(n.normalize_or_zero().into());
        self.uvs.push(uv(paint));
        self.positions.len() as u32 - 1
    }

    /// A flat quad, wound so it faces away from `inside`.
    fn quad(&mut self, q: [Vec3; 4], inside: Vec3, paint: Paint) {
        let centre = (q[0] + q[1] + q[2] + q[3]) / 4.0;
        let mut n = (q[1] - q[0]).cross(q[2] - q[0]) + (q[2] - q[0]).cross(q[3] - q[0]);
        let q = if n.dot(centre - inside) < 0.0 {
            n = -n;
            [q[0], q[3], q[2], q[1]]
        } else {
            q
        };
        let i: Vec<u32> = q.iter().map(|&p| self.vertex(p, n, paint)).collect();
        self.indices.extend([i[0], i[1], i[2], i[0], i[2], i[3]]);
    }

    /// A convex six-sided block from its corners, indexed `x + 2y + 4z`
    /// (each bit low or high side).
    fn block(&mut self, c: [Vec3; 8], paint: Paint) {
        let inside = c.iter().copied().sum::<Vec3>() / 8.0;
        for face in [[0, 2, 6, 4], [1, 3, 7, 5], [0, 1, 5, 4], [2, 3, 7, 6], [0, 1, 3, 2], [4, 5, 7, 6]] {
            self.quad(face.map(|i| c[i]), inside, paint);
        }
    }

    /// A box at `centre`, `half` its half sizes along `axes`' columns.
    fn boxed(&mut self, centre: Vec3, half: Vec3, axes: Mat3, paint: Paint) {
        let c = std::array::from_fn(|i| {
            let s = Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            );
            centre + axes * (s * half)
        });
        self.block(c, paint);
    }

    /// A box between two points: `width` along X, `thick` square to both.
    fn bar(&mut self, a: Vec3, b: Vec3, width: f32, thick: f32, paint: Paint) {
        let along = (b - a).normalize_or(Vec3::Z);
        let side = if along.x.abs() > 0.9 { Vec3::Y } else { Vec3::X };
        let up = along.cross(side).normalize();
        let side = up.cross(along);
        let axes = Mat3::from_cols(side, up, along);
        self.boxed((a + b) / 2.0, Vec3::new(width / 2.0, thick / 2.0, a.distance(b) / 2.0), axes, paint);
    }

    /// A round tube through `points`, smooth, closed at both ends.
    fn tube(&mut self, points: &[Vec3], radius: f32, paint: Paint) {
        const SIDES: usize = 10;
        let n = points.len();
        if n < 2 {
            return;
        }
        let tangent = |i: usize| -> Vec3 {
            let a = points[i.saturating_sub(1)];
            let b = points[(i + 1).min(n - 1)];
            (b - a).normalize_or(Vec3::Z)
        };
        // Rings carried along the path so they do not twist.
        let mut normal = tangent(0).any_orthonormal_vector();
        let mut rings = Vec::new();
        for i in 0..n {
            let t = tangent(i);
            normal = (normal - t * normal.dot(t)).normalize_or(t.any_orthonormal_vector());
            let binormal = t.cross(normal);
            let start = self.positions.len() as u32;
            for k in 0..SIDES {
                let a = k as f32 / SIDES as f32 * std::f32::consts::TAU;
                let d = normal * a.cos() + binormal * a.sin();
                self.vertex(points[i] + d * radius, d, paint);
            }
            rings.push(start);
        }
        for w in rings.windows(2) {
            for k in 0..SIDES as u32 {
                let k1 = (k + 1) % SIDES as u32;
                let (a, b, c, d) = (w[0] + k, w[0] + k1, w[1] + k1, w[1] + k);
                self.indices.extend([a, b, c, a, c, d]);
            }
        }
        for (end, ring, sign) in [(points[0], rings[0], -1.0), (points[n - 1], rings[n - 1], 1.0)] {
            let t = if sign < 0.0 { -tangent(0) } else { tangent(n - 1) };
            let centre = self.vertex(end, t, paint);
            let rim: Vec<u32> = (0..SIDES as u32)
                .map(|k| {
                    let p = Vec3::from(self.positions[(ring + k) as usize]);
                    self.vertex(p, t, paint)
                })
                .collect();
            for k in 0..SIDES {
                let (a, b) = (rim[k], rim[(k + 1) % SIDES]);
                if sign < 0.0 {
                    self.indices.extend([centre, b, a]);
                } else {
                    self.indices.extend([centre, a, b]);
                }
            }
        }
    }

    /// A solid of revolution about the X axis through `centre`: `profile`
    /// is (x, radius) pairs going round the cross-section anticlockwise
    /// seen from +Z (outward side to the right of travel).
    fn lathe(&mut self, centre: Vec3, profile: &[(f32, f32)], segments: usize, paint: Paint) {
        let m = profile.len();
        // Flat-shaded edges: each profile segment gets its own vertices.
        for j in 0..m - 1 {
            let (x0, r0) = profile[j];
            let (x1, r1) = profile[j + 1];
            // The profile's outward normal in (x, r).
            let (nx, nr) = (r1 - r0, -(x1 - x0));
            let start = self.positions.len() as u32;
            for s in 0..=segments {
                let a = s as f32 / segments as f32 * std::f32::consts::TAU;
                let radial = Vec3::new(0.0, a.cos(), a.sin());
                let n = Vec3::X * nx + radial * nr;
                self.vertex(centre + Vec3::X * x0 + radial * r0, n, paint);
                self.vertex(centre + Vec3::X * x1 + radial * r1, n, paint);
            }
            for s in 0..segments as u32 {
                let (a, b, c, d) = (start + 2 * s, start + 2 * s + 1, start + 2 * s + 3, start + 2 * s + 2);
                self.indices.extend([a, b, c, a, c, d]);
            }
        }
    }
}

/// A wheel about its axle at `centre`: tyre to `radius` at the knobs on a
/// rim of `rim`, `width` wide, with a disc on the left.
fn wheel(g: &mut Geo, centre: Vec3, radius: f32, rim: f32, width: f32) {
    let carcass = radius - 0.014;
    let w = width / 2.0;
    // The tyre's section: bead, sidewall, shoulder, tread, and back.
    let tyre = [
        (w * 0.55, rim),
        (w * 0.95, rim + 0.25 * (carcass - rim)),
        (w, rim + 0.6 * (carcass - rim)),
        (w * 0.8, carcass - 0.005),
        (0.0, carcass),
        (-w * 0.8, carcass - 0.005),
        (-w, rim + 0.6 * (carcass - rim)),
        (-w * 0.95, rim + 0.25 * (carcass - rim)),
        (-w * 0.55, rim),
    ];
    g.lathe(centre, &tyre, 36, Paint::Rubber);
    // Knobs: three staggered rows round the tread.
    let knobs = 34;
    for k in 0..knobs {
        let a = k as f32 / knobs as f32 * std::f32::consts::TAU;
        let radial = Vec3::new(0.0, a.cos(), a.sin());
        let along = Vec3::new(0.0, -a.sin(), a.cos());
        let rows: &[f32] = if k % 2 == 0 { &[-0.62, 0.62] } else { &[0.0] };
        for &x in rows {
            let axes = Mat3::from_cols(Vec3::X, radial, along);
            g.boxed(
                centre + Vec3::X * (x * w) + radial * (carcass + 0.006),
                Vec3::new(w * 0.2, 0.008, 0.016),
                axes,
                Paint::Rubber,
            );
        }
    }
    // The rim: a channel facing out, and the hub.
    let rim_w = w * 0.6;
    g.lathe(
        centre,
        &[(rim_w, rim - 0.022), (rim_w, rim + 0.004), (-rim_w, rim + 0.004), (-rim_w, rim - 0.022), (rim_w, rim - 0.022)],
        36,
        Paint::Rim,
    );
    g.lathe(
        centre,
        &[(0.07, 0.0), (0.07, 0.022), (0.05, 0.045), (-0.05, 0.045), (-0.07, 0.022), (-0.07, 0.0)],
        16,
        Paint::Chrome,
    );
    // Spokes, crossed, alternate sides.
    let spokes = 24;
    for k in 0..spokes {
        let a = k as f32 / spokes as f32 * std::f32::consts::TAU;
        let skew = if k % 2 == 0 { 0.35 } else { -0.35 };
        let hub_at = Vec3::new(0.0, a.cos(), a.sin()) * 0.04;
        let rim_at = Vec3::new(0.0, (a + skew).cos(), (a + skew).sin()) * (rim - 0.02);
        let side = if k % 2 == 0 { 0.045 } else { -0.045 };
        g.bar(
            centre + hub_at + Vec3::X * side,
            centre + rim_at + Vec3::X * side * 0.2,
            0.004,
            0.004,
            Paint::Chrome,
        );
    }
    // The brake disc, on the left.
    g.lathe(
        centre + Vec3::X * 0.06,
        &[(0.003, 0.05), (0.003, 0.125), (-0.003, 0.125), (-0.003, 0.05), (0.003, 0.05)],
        24,
        Paint::Chrome,
    );
}

/// Mirrors `f`'s geometry across the bike's centre plane by calling it with
/// `side` = 1 then -1.
fn both(mut f: impl FnMut(f32)) {
    f(1.0);
    f(-1.0);
}

/// The whole stand-in bike, in the bike frame like `reflex_assets::bike::load`.
pub fn model() -> BikeModel {
    let (front_axle, rear_axle, front_radius, rear_radius) = DAKUMOTO_450F;
    let axis = Vec3::new(0.0, RAKE.cos(), -RAKE.sin());
    let head = front_axle + axis * 0.70;
    let pivot = Vec3::new(0.0, -0.16, -0.19);
    let at = |s: f32| front_axle + axis * s;

    let bones = vec![
        Bone { name: "OBJ_Frame".into(), parent: None, local: Mat4::IDENTITY.to_cols_array_2d() },
        Bone {
            name: "OBJ_UPR_Forks".into(),
            parent: None,
            local: (Mat4::from_translation(head) * Mat4::from_rotation_x(-RAKE)).to_cols_array_2d(),
        },
        Bone { name: "OBJ_LWR_Forks".into(), parent: Some(1), local: Mat4::IDENTITY.to_cols_array_2d() },
        Bone {
            name: "OBJ_FRT_Tire".into(),
            parent: Some(2),
            local: Mat4::from_translation(Vec3::NEG_Y * 0.70).to_cols_array_2d(),
        },
        Bone { name: "OBJ_Swingarm".into(), parent: None, local: Mat4::from_translation(pivot).to_cols_array_2d() },
        Bone {
            name: "OBJ_BCK_Tire".into(),
            parent: Some(4),
            local: Mat4::from_translation(rear_axle - pivot).to_cols_array_2d(),
        },
    ];
    let mut geo: Vec<Geo> = (0..bones.len()).map(|_| Geo::default()).collect();

    // The frame: twin spars from the head to the pivot, a cradle under the
    // engine, the subframe under the seat.
    {
        let g = &mut geo[0];
        let neck = at(0.64);
        g.tube(&[at(0.58), at(0.72)], 0.032, Paint::Frame);
        both(|s| {
            g.tube(
                &[
                    neck + Vec3::new(0.03 * s, -0.02, -0.02),
                    Vec3::new(0.09 * s, 0.17, 0.12),
                    Vec3::new(0.11 * s, 0.06, -0.10),
                    Vec3::new(0.10 * s, -0.10, -0.17),
                    Vec3::new(0.09 * s, -0.22, -0.16),
                ],
                0.026,
                Paint::Frame,
            );
            g.tube(
                &[
                    Vec3::new(0.09 * s, -0.22, -0.16),
                    Vec3::new(0.06 * s, -0.36, -0.10),
                    Vec3::new(0.05 * s, -0.37, 0.12),
                    Vec3::new(0.02 * s, -0.20, 0.27),
                    at(0.60) + Vec3::new(0.0, -0.03, -0.03),
                ],
                0.017,
                Paint::Frame,
            );
            g.tube(
                &[Vec3::new(0.10 * s, 0.10, -0.12), Vec3::new(0.085 * s, 0.25, -0.40), Vec3::new(0.07 * s, 0.30, -0.62)],
                0.014,
                Paint::Frame,
            );
            g.tube(&[Vec3::new(0.09 * s, -0.08, -0.17), Vec3::new(0.075 * s, 0.24, -0.45)], 0.012, Paint::Frame);
            // Footpegs.
            g.boxed(Vec3::new(0.19 * s, -0.27, -0.13), Vec3::new(0.05, 0.012, 0.03), Mat3::IDENTITY, Paint::Engine);
        });

        // The engine: crankcase, cylinder leaning forward, head.
        g.boxed(Vec3::new(0.0, -0.22, 0.03), Vec3::new(0.11, 0.12, 0.17), Mat3::IDENTITY, Paint::Engine);
        let lean = Mat3::from_rotation_x(-0.25);
        g.boxed(Vec3::new(0.0, -0.04, 0.10), Vec3::new(0.075, 0.09, 0.075), lean, Paint::Chrome);
        g.boxed(Vec3::new(0.0, 0.06, 0.12), Vec3::new(0.085, 0.04, 0.085), lean, Paint::Engine);
        // Clutch and stator covers; the profile runs the other way on the
        // right so the faces still point out.
        both(|s| {
            let mut cover = vec![(0.012 * s, 0.0), (0.012 * s, 0.075), (0.0, 0.085)];
            if s < 0.0 {
                cover.reverse();
            }
            g.lathe(Vec3::new(0.11 * s, -0.22, 0.0), &cover, 18, Paint::Engine);
        });
        // The shock, upright behind the engine.
        g.tube(&[Vec3::new(0.0, 0.14, -0.13), Vec3::new(0.0, -0.20, -0.24)], 0.024, Paint::Gold);

        // Tank, radiator shrouds, seat, side plates, rear fender.
        g.block(
            [
                Vec3::new(-0.11, 0.18, 0.28),
                Vec3::new(0.11, 0.18, 0.28),
                Vec3::new(-0.10, 0.32, 0.26),
                Vec3::new(0.10, 0.32, 0.26),
                Vec3::new(-0.13, 0.20, 0.0),
                Vec3::new(0.13, 0.20, 0.0),
                Vec3::new(-0.11, 0.34, 0.02),
                Vec3::new(0.11, 0.34, 0.02),
            ],
            Paint::Plastic,
        );
        both(|s| {
            // Radiator behind the shroud.
            g.boxed(
                Vec3::new(0.12 * s, 0.08, 0.20),
                Vec3::new(0.025, 0.10, 0.07),
                Mat3::from_rotation_x(-0.35),
                Paint::Engine,
            );
            g.block(
                [
                    Vec3::new(0.13 * s, 0.30, 0.33),
                    Vec3::new(0.18 * s, 0.26, 0.31),
                    Vec3::new(0.12 * s, 0.33, 0.30),
                    Vec3::new(0.15 * s, 0.31, 0.28),
                    Vec3::new(0.15 * s, -0.02, 0.10),
                    Vec3::new(0.20 * s, 0.02, 0.06),
                    Vec3::new(0.14 * s, 0.25, 0.0),
                    Vec3::new(0.17 * s, 0.23, -0.02),
                ],
                Paint::Plastic,
            );
            // Side plate, white, over the airbox.
            g.block(
                [
                    Vec3::new(0.11 * s, 0.10, -0.25),
                    Vec3::new(0.135 * s, 0.10, -0.25),
                    Vec3::new(0.10 * s, 0.32, -0.30),
                    Vec3::new(0.125 * s, 0.32, -0.30),
                    Vec3::new(0.09 * s, 0.16, -0.55),
                    Vec3::new(0.115 * s, 0.16, -0.55),
                    Vec3::new(0.085 * s, 0.31, -0.58),
                    Vec3::new(0.11 * s, 0.31, -0.58),
                ],
                Paint::White,
            );
        });
        g.block(
            [
                Vec3::new(-0.08, 0.33, 0.06),
                Vec3::new(0.08, 0.33, 0.06),
                Vec3::new(-0.07, 0.38, 0.05),
                Vec3::new(0.07, 0.38, 0.05),
                Vec3::new(-0.10, 0.31, -0.64),
                Vec3::new(0.10, 0.31, -0.64),
                Vec3::new(-0.09, 0.375, -0.62),
                Vec3::new(0.09, 0.375, -0.62),
            ],
            Paint::Seat,
        );
        g.block(
            [
                Vec3::new(-0.11, 0.29, -0.52),
                Vec3::new(0.11, 0.29, -0.52),
                Vec3::new(-0.11, 0.31, -0.52),
                Vec3::new(0.11, 0.31, -0.52),
                Vec3::new(-0.06, 0.37, -0.98),
                Vec3::new(0.06, 0.37, -0.98),
                Vec3::new(-0.06, 0.39, -0.98),
                Vec3::new(0.06, 0.39, -0.98),
            ],
            Paint::White,
        );

        // The pipe: header round the front of the engine on the right
        // (-X), then up into the silencer under the seat.
        let right = -1.0;
        g.tube(
            &[
                Vec3::new(0.02 * right, 0.04, 0.20),
                Vec3::new(0.06 * right, -0.02, 0.27),
                Vec3::new(0.11 * right, -0.12, 0.22),
                Vec3::new(0.13 * right, -0.14, 0.04),
                Vec3::new(0.14 * right, -0.06, -0.16),
                Vec3::new(0.15 * right, 0.08, -0.30),
            ],
            0.024,
            Paint::Chrome,
        );
        g.tube(
            &[Vec3::new(0.15 * right, 0.08, -0.30), Vec3::new(0.155 * right, 0.17, -0.48), Vec3::new(0.16 * right, 0.25, -0.70)],
            0.05,
            Paint::Frame,
        );
        g.tube(&[Vec3::new(0.16 * right, 0.25, -0.70), Vec3::new(0.162 * right, 0.27, -0.76)], 0.022, Paint::Engine);
    }

    // The front end that turns with the bars: clamps, bars, fork tubes,
    // the high fender, the number plate.
    {
        let g = &mut geo[1];
        let up = axis;
        let fwd = Vec3::new(0.0, RAKE.sin(), RAKE.cos());
        g.bar(at(0.60) + Vec3::X * 0.12 + fwd * 0.02, at(0.60) - Vec3::X * 0.12 + fwd * 0.02, 0.05, 0.045, Paint::Engine);
        g.bar(at(0.72) + Vec3::X * 0.12 + fwd * 0.02, at(0.72) - Vec3::X * 0.12 + fwd * 0.02, 0.05, 0.03, Paint::Engine);
        both(|s| {
            g.tube(&[at(0.18) + Vec3::X * 0.09 * s + fwd * 0.02, at(0.74) + Vec3::X * 0.09 * s + fwd * 0.02], 0.027, Paint::Gold);
        });
        // The bars: up from the clamp, out and back.
        let bar_root = at(0.78) - fwd * 0.01;
        g.tube(
            &[
                bar_root + Vec3::new(0.40, 0.06, -0.06),
                bar_root + Vec3::new(0.24, 0.05, -0.03),
                bar_root + Vec3::new(0.12, 0.0, 0.0),
                bar_root + Vec3::new(-0.12, 0.0, 0.0),
                bar_root + Vec3::new(-0.24, 0.05, -0.03),
                bar_root + Vec3::new(-0.40, 0.06, -0.06),
            ],
            0.012,
            Paint::Chrome,
        );
        both(|s| {
            g.tube(
                &[bar_root + Vec3::new(0.30 * s, 0.055, -0.045), bar_root + Vec3::new(0.42 * s, 0.062, -0.063)],
                0.018,
                Paint::Grip,
            );
            g.bar(
                bar_root + Vec3::new(0.22 * s, 0.06, -0.03),
                bar_root + Vec3::new(0.36 * s, 0.05, 0.05),
                0.01,
                0.012,
                Paint::Engine,
            );
        });
        g.bar(bar_root + Vec3::new(0.06, -0.01, 0.0), bar_root + Vec3::new(-0.06, -0.01, 0.0), 0.03, 0.035, Paint::Engine);
        // Fender: a curved strip high over the tyre, off the lower clamp.
        let mid = at(0.56) + fwd * 0.04;
        let fender: Vec<Vec3> = (0..=6)
            .map(|i| {
                let t = i as f32 / 6.0 - 0.45;
                mid + fwd * (t * 0.62) - up * (t * t * 0.32)
            })
            .collect();
        for w in fender.windows(2) {
            let d = (w[1] - w[0]).normalize();
            let n = d.cross(Vec3::X).normalize() * if d.cross(Vec3::X).dot(up) < 0.0 { -1.0 } else { 1.0 };
            let half = 0.065;
            g.block(
                [
                    w[0] - Vec3::X * half,
                    w[0] + Vec3::X * half,
                    w[0] - Vec3::X * half + n * 0.008,
                    w[0] + Vec3::X * half + n * 0.008,
                    w[1] - Vec3::X * half,
                    w[1] + Vec3::X * half,
                    w[1] - Vec3::X * half + n * 0.008,
                    w[1] + Vec3::X * half + n * 0.008,
                ],
                Paint::White,
            );
        }
        // Number plate, ahead of the clamps along the axis.
        g.boxed(at(0.70) + fwd * 0.07, Vec3::new(0.11, 0.12, 0.008), Mat3::from_cols(Vec3::X, up, fwd), Paint::White);
        g.boxed(at(0.70) + fwd * 0.076, Vec3::new(0.06, 0.07, 0.004), Mat3::from_cols(Vec3::X, up, fwd), Paint::Plastic);
    }

    // The fork legs that slide up the tubes, down to the axle.
    {
        let g = &mut geo[2];
        let fwd = Vec3::new(0.0, RAKE.sin(), RAKE.cos());
        both(|s| {
            g.tube(&[at(0.0) + Vec3::X * 0.09 * s, at(0.30) + Vec3::X * 0.09 * s + fwd * 0.02], 0.022, Paint::Chrome);
            g.boxed(at(0.02) + Vec3::X * 0.09 * s, Vec3::new(0.025, 0.035, 0.03), Mat3::IDENTITY, Paint::Engine);
        });
        g.tube(&[front_axle + Vec3::X * 0.11, front_axle - Vec3::X * 0.11], 0.010, Paint::Chrome);
    }

    wheel(&mut geo[3], front_axle, front_radius, 0.272, 0.09);

    // The swingarm: two tapered arms from the pivot to past the axle.
    {
        let g = &mut geo[4];
        both(|s| {
            g.bar(
                pivot + Vec3::new(0.085 * s, 0.0, 0.03),
                rear_axle + Vec3::new(0.075 * s, 0.0, -0.05),
                0.03,
                0.05,
                Paint::Frame,
            );
        });
        g.bar(pivot + Vec3::new(0.09, 0.0, -0.10), pivot + Vec3::new(-0.09, 0.0, -0.10), 0.05, 0.05, Paint::Frame);
        g.tube(&[pivot + Vec3::X * 0.11, pivot - Vec3::X * 0.11], 0.016, Paint::Chrome);
        // Chain guard on the left.
        g.bar(
            pivot + Vec3::new(0.11, 0.035, -0.08),
            rear_axle + Vec3::new(0.11, 0.035, 0.1),
            0.01,
            0.02,
            Paint::Engine,
        );
    }

    wheel(&mut geo[5], rear_axle, rear_radius, 0.245, 0.11);

    // Into each bone's own frame.
    let rests: Vec<Mat4> = (0..bones.len()).map(|i| Mat4::from_cols_array_2d(&rest(&bones, i))).collect();
    let names = ["frame", "upper forks", "lower forks", "front wheel", "swingarm", "rear wheel"];
    let parts = geo
        .into_iter()
        .enumerate()
        .map(|(bone, g)| {
            let to_local = rests[bone].inverse();
            let turn = Mat3::from_mat4(to_local);
            Part {
                bone,
                positions: g.positions.iter().map(|&p| to_local.transform_point3(Vec3::from(p)).into()).collect(),
                normals: g.normals.iter().map(|&n| (turn * Vec3::from(n)).normalize_or_zero().into()).collect(),
                uvs: g.uvs,
                indices: g.indices,
                texture: Some(PALETTE.into()),
                cutout: false,
                slot: format!("stand-in {}", names[bone]),
            }
        })
        .collect();
    BikeModel {
        bones,
        parts,
        textures: HashMap::from([(PALETTE.to_owned(), palette())]),
        kit: "our own stand-in bike",
    }
}

fn rest(bones: &[Bone], i: usize) -> [[f32; 4]; 4] {
    let local = Mat4::from_cols_array_2d(&bones[i].local);
    match bones[i].parent {
        Some(p) => (Mat4::from_cols_array_2d(&rest(bones, p)) * local).to_cols_array_2d(),
        None => local.to_cols_array_2d(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every triangle faces the way its normals say (so back-face culling
    /// keeps the outside), and the bones put the wheels on the axles.
    #[test]
    fn stand_in_is_whole_and_faces_out() {
        let m = model();
        assert_eq!(m.parts.len(), 6);
        for part in &m.parts {
            assert!(!part.indices.is_empty(), "{} is empty", part.slot);
            for t in part.indices.chunks(3) {
                let p: [Vec3; 3] = std::array::from_fn(|k| Vec3::from(part.positions[t[k] as usize]));
                let face = (p[1] - p[0]).cross(p[2] - p[0]);
                if face.length() < 1e-9 {
                    continue;
                }
                let n: Vec3 = t.iter().map(|&i| Vec3::from(part.normals[i as usize])).sum();
                assert!(face.dot(n) > 0.0, "{}: a triangle faces in", part.slot);
            }
        }
        let (front, rear, _, _) = DAKUMOTO_450F;
        let at = |name: &str| Vec3::from_slice(&m.rest(m.bone(name).unwrap())[3][..3]);
        assert!(at("OBJ_FRT_Tire").distance(front) < 1e-4);
        assert!(at("OBJ_BCK_Tire").distance(rear) < 1e-4);
        // The upper forks' Y is the steering axis.
        let fork = Mat4::from_cols_array_2d(&m.rest(m.bone("OBJ_UPR_Forks").unwrap()));
        assert!(fork.transform_vector3(Vec3::Y).distance(crate::physics::steering_axis()) < 1e-4);
        // Nothing hangs below the ground (0.60 m under the centre of gravity)
        // but the tyres' contact.
        for part in &m.parts {
            let rest = Mat4::from_cols_array_2d(&m.rest(part.bone));
            for &p in &part.positions {
                assert!(rest.transform_point3(Vec3::from(p)).y > -0.62, "{} below the ground", part.slot);
            }
        }
    }
}
