//! Grind rails found in the map's collision: the lips a skater can grind.
//!
//! Ported from the MW2 Skate 3 mashup (IW4L), licensed Apache-2.0:
//! `crates/render_anim/src/skate/rails.rs` in
//! https://github.com/chasmlol/2010-rust-rewrite-mashup.
//! The detector ([`find`]) is unchanged apart from using `glam` directly
//! instead of bevy's re-export and dropping the log lines; it still works in
//! IW4 inches, Z up, with its constants in inches, so it behaves exactly as
//! it does on MW2 maps. [`find_rails`] scales Halo's collision into that
//! frame and the rails back out.
//!
//! MW2 collision comes from brushes, the clip mesh and static model collision,
//! so the top of a ledge and its wall rarely share an edge vertex for vertex.
//! A lip is therefore found by probing the geometry around each edge of a
//! walkable face rather than by matching triangles: the ground must fall away
//! just past the edge and nothing may rise there. Surviving edges are merged
//! along their lines across seams and T-junctions, then chained into
//! polylines where they meet at a gentle turn, as the authoring tool does for
//! edges marked sharp.

use std::collections::HashMap;

use glam::Vec3;

use crate::{Collision, TriggerVolume};
use crate::collision::surface_flags;
use crate::column_grid::ColumnGrid;

/// Inches per Halo world unit: one unit is ten feet.
pub const INCHES_PER_WORLD_UNIT: f32 = 120.0;

/// A face this upward is walkable; its edges are rail candidates.
const UPWARD_Z: f32 = 0.65;
/// XY grid cell for probing, in map units (inches).
const CELL: f32 = 64.0;
/// How far past the lip the probes look.
const PROBE_OUT: f32 = 2.0;
/// Open space the ground must drop below the lip, just past it.
const MIN_DROP: f32 = 3.0;
/// Height above the lip the wall probe runs at.
const WALL_PROBE_UP: f32 = 3.0;
/// Shortest rail kept, after merging and chaining.
const MIN_RAIL: f32 = 24.0;
/// Steepest rail, as rise over length: stair handrails and ramp edges pass.
const MAX_SLOPE: f32 = 0.7;
/// Chains continue through a joint turning less than about 35 degrees.
const MIN_TURN_COS: f32 = 0.82;
/// Water deeper than this (world units, about 1.4 m) cannot be ridden: the
/// viewer sinks the skater there, so rails under it are dropped.
pub const DEEP_WATER: f32 = 0.45;
/// The skate engine counts rails in a u16.
const MAX_RAILS: usize = u16::MAX as usize;

/// Two points rounded to a grid, ordered, so matching edges key the same.
type Key = ([i32; 3], [i32; 3]);
/// An edge's endpoints with the normal and centroid of the face it came from.
type Edge = (Vec3, Vec3, Vec3, Vec3);
/// A line's direction and origin with the parameter spans of the lips on it.
type Line = (Vec3, Vec3, Vec<(f32, f32)>);

/// How many edges survived each stage of the search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RailCensus {
    /// Distinct edges of walkable faces.
    pub candidates: usize,
    /// Edges the probes found a drop past and no wall over.
    pub lips: usize,
    /// Lips merged along their lines.
    pub runs: usize,
    /// Polylines kept after chaining and the length cut.
    pub rails: usize,
}

/// Rails over a map's collision, in Halo world units (Z up), each a polyline
/// of two or more points. Invisible surfaces are left out: a wall the player
/// cannot see is not grindable.
pub fn find_rails(collision: &Collision) -> (Vec<Vec<Vec3>>, RailCensus) {
    let tris: Vec<[Vec3; 3]> = collision
        .triangles
        .iter()
        .filter(|t| t.flags & surface_flags::INVISIBLE == 0)
        .map(|t| t.points.map(|p| p * INCHES_PER_WORLD_UNIT))
        .collect();
    let (rails, mut census) = find(&tris);
    // Where the collision mesh simply ends (the edge of the world), the ground
    // "falls away" too, and the detector reads the rim as one long ledge.
    // Drop rails that touch the outer XY boundary of the collision.
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for t in &tris {
        for p in t {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
    }
    let margin = 0.5 * INCHES_PER_WORLD_UNIT;
    let on_rim = |p: &Vec3| {
        p.x - lo.x < margin || hi.x - p.x < margin || p.y - lo.y < margin || hi.y - p.y < margin
    };
    let rails: Vec<Vec<Vec3>> = rails
        .into_iter()
        .filter(|rail| !rail.iter().any(on_rim))
        .map(|rail| rail.into_iter().map(|p| p / INCHES_PER_WORLD_UNIT).collect())
        .collect();
    census.rails = rails.len();
    (rails, census)
}

/// Cuts rails (Halo units) where they run through the scenario's kill
/// volumes: the ledges under 120_halo's Warthog run are real lips, but the
/// player dies before reaching them. Where the Guardians shoot
/// ([`crate::scene::guarded`]: Sandtrap's outer wall and dunes) counts too.
/// A rail is split at every segment whose ends or middle lie in a kill
/// volume; pieces of one point are dropped.
pub fn outside_kill_volumes(rails: Vec<Vec<Vec3>>, volumes: &[TriggerVolume]) -> Vec<Vec<Vec3>> {
    let kill: Vec<&TriggerVolume> = volumes.iter().filter(|v| v.kills()).collect();
    let guards = volumes.iter().any(TriggerVolume::guards);
    if kill.is_empty() && !guards {
        return rails;
    }
    let shot = |p: Vec3| guards && crate::scene::guarded(volumes, p).is_some();
    cut_where(rails, |p| kill.iter().any(|v| v.contains(p)) || shot(p))
}

/// Cuts rails (Halo units) where they run more than `depth` under a water
/// surface (`water`: the triangles drawn with water shaders). The water
/// surface is not in the collision, so the lips of a sea bed (Last Resort's
/// beach) are real rails a skater could only reach by sinking. Split as in
/// [`outside_kill_volumes`].
pub fn out_of_deep_water(rails: Vec<Vec<Vec3>>, water: &[[Vec3; 3]], depth: f32) -> Vec<Vec<Vec3>> {
    if water.is_empty() {
        return rails;
    }
    const CELL: f32 = 8.0;
    let cell = |v: f32| (v / CELL).floor() as i32;
    let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
    for (i, t) in water.iter().enumerate() {
        let lo = t[0].min(t[1]).min(t[2]);
        let hi = t[0].max(t[1]).max(t[2]);
        for x in cell(lo.x)..=cell(hi.x) {
            for y in cell(lo.y)..=cell(hi.y) {
                grid.entry((x, y)).or_default().push(i);
            }
        }
    }
    let drowned = |p: Vec3| {
        grid.get(&(cell(p.x), cell(p.y))).is_some_and(|tris| {
            tris.iter()
                .filter_map(|&i| height_over(&water[i], p.x, p.y))
                .any(|surface| surface - p.z > depth)
        })
    };
    cut_where(rails, drowned)
}

/// The height of a triangle over `(x, y)`, when the point is inside its
/// footprint and the triangle is not vertical.
fn height_over(t: &[Vec3; 3], x: f32, y: f32) -> Option<f32> {
    let [a, b, c] = *t;
    let d = (b.x - a.x) * (c.y - a.y) - (c.x - a.x) * (b.y - a.y);
    if d.abs() < 1e-9 {
        return None;
    }
    let u = ((x - a.x) * (c.y - a.y) - (c.x - a.x) * (y - a.y)) / d;
    let v = ((b.x - a.x) * (y - a.y) - (x - a.x) * (b.y - a.y)) / d;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then_some(a.z + u * (b.z - a.z) + v * (c.z - a.z))
}

/// Splits rails at every segment whose ends or middle are `bad`; pieces of
/// one point are dropped.
fn cut_where(rails: Vec<Vec<Vec3>>, bad: impl Fn(Vec3) -> bool) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    for rail in rails {
        let mut piece: Vec<Vec3> = Vec::new();
        for w in rail.windows(2) {
            let (a, b) = (w[0], w[1]);
            if bad(a) || bad(b) || bad((a + b) * 0.5) {
                if piece.len() >= 2 {
                    out.push(std::mem::take(&mut piece));
                }
                piece.clear();
                continue;
            }
            if piece.is_empty() {
                piece.push(a);
            }
            piece.push(b);
        }
        if piece.len() >= 2 {
            out.push(piece);
        }
    }
    out
}

/// Rails over `tris` (map units, z up), each a polyline of two or more points.
pub fn find(tris: &[[Vec3; 3]]) -> (Vec<Vec<Vec3>>, RailCensus) {
    let grid = Grid::build(tris);
    let mut probe = Probe {
        tris,
        grid: &grid,
        stamp: vec![0; tris.len()],
        round: 0,
        scratch: Vec::new(),
    };

    // Every edge of a walkable face, once.
    let key = |v: Vec3| v.to_array().map(|x| (x * 8.).round() as i32);
    let mut edges: HashMap<Key, Edge> = HashMap::new();
    for tri in tris {
        let normal = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
        if normal.z <= UPWARD_Z {
            continue;
        }
        let centroid = (tri[0] + tri[1] + tri[2]) / 3.;
        for i in 0..3 {
            let (a, b) = (tri[i], tri[(i + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            edges.entry(k).or_insert((a, b, normal, centroid));
        }
    }
    let candidates = edges.len();

    let mut lips = Vec::new();
    for (a, b, normal, centroid) in edges.into_values() {
        let along = b - a;
        let len = along.length();
        if len < 1. || along.z.abs() > MAX_SLOPE * len {
            continue;
        }
        let mid = (a + b) * 0.5;
        let mut out = along.cross(normal).normalize_or_zero();
        if out.dot(centroid - mid) > 0. {
            out = -out;
        }
        let samples: &[f32] = if len < 12. {
            &[0.5]
        } else {
            &[0.25, 0.5, 0.75]
        };
        let passing = samples
            .iter()
            .filter(|&&s| probe.is_lip(a + along * s, out))
            .count();
        if passing * 3 >= samples.len() * 2 {
            lips.push((a, b));
        }
    }
    let lip_count = lips.len();

    let runs = merge_collinear(lips);
    let run_count = runs.len();
    let mut rails = chain(runs);
    rails.retain(|rail| polyline_len(rail) >= MIN_RAIL);
    if rails.len() > MAX_RAILS {
        rails.sort_by(|a, b| polyline_len(b).total_cmp(&polyline_len(a)));
        rails.truncate(MAX_RAILS);
    }
    let census = RailCensus {
        candidates,
        lips: lip_count,
        runs: run_count,
        rails: rails.len(),
    };
    (rails, census)
}

fn polyline_len(points: &[Vec3]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}

struct Grid {
    columns: ColumnGrid,
}

impl Grid {
    fn build(tris: &[[Vec3; 3]]) -> Self {
        let mut columns = ColumnGrid::new(CELL);
        for (index, tri) in tris.iter().enumerate() {
            let min = tri[0].min(tri[1]).min(tri[2]);
            let max = tri[0].max(tri[1]).max(tri[2]);
            columns.insert(index as u32, [min.x, min.y], [max.x, max.y]);
        }
        Self { columns }
    }
}

struct Probe<'a> {
    tris: &'a [[Vec3; 3]],
    grid: &'a Grid,
    stamp: Vec<u32>,
    round: u32,
    scratch: Vec<u32>,
}

impl Probe<'_> {
    /// Past `p` along `out` the ground falls away and nothing rises.
    fn is_lip(&mut self, p: Vec3, out: Vec3) -> bool {
        let down_from = p + out * PROBE_OUT + Vec3::Z;
        if self.hit(down_from, -Vec3::Z, 1. + MIN_DROP) {
            return false;
        }
        let across_from = p - out + Vec3::Z * WALL_PROBE_UP;
        !self.hit(across_from, out, 2. + PROBE_OUT)
    }

    /// Whether the segment from `origin` along unit `dir` for `len` crosses
    /// any triangle, either side facing.
    fn hit(&mut self, origin: Vec3, dir: Vec3, len: f32) -> bool {
        let end = origin + dir * len;
        let (min, max) = (origin.min(end), origin.max(end));
        self.round = self.round.wrapping_add(1);
        if self.round == 0 {
            self.stamp.fill(0);
            self.round = 1;
        }
        self.scratch.clear();
        let (stamp, scratch, round) = (&mut self.stamp, &mut self.scratch, self.round);
        self.grid.columns.query([min.x, min.y], [max.x, max.y], |index| {
            let seen = &mut stamp[index as usize];
            if *seen != round {
                *seen = round;
                scratch.push(index);
            }
        });
        self.scratch
            .iter()
            .any(|&index| ray_triangle(origin, dir, len, &self.tris[index as usize]))
    }
}

/// Möller–Trumbore, both faces, hits within `(0, len]`.
fn ray_triangle(origin: Vec3, dir: Vec3, len: f32, tri: &[Vec3; 3]) -> bool {
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let p = dir.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-8 {
        return false;
    }
    let inv = 1. / det;
    let s = origin - tri[0];
    let u = s.dot(p) * inv;
    if !(0. ..=1.).contains(&u) {
        return false;
    }
    let q = s.cross(e1);
    let v = dir.dot(q) * inv;
    if v < 0. || u + v > 1. {
        return false;
    }
    let t = e2.dot(q) * inv;
    t > 1e-4 && t <= len
}

/// Lips on one line, merged into maximal runs where they overlap or touch.
fn merge_collinear(lips: Vec<(Vec3, Vec3)>) -> Vec<(Vec3, Vec3)> {
    let mut lines: HashMap<Key, Line> = HashMap::new();
    for (mut a, mut b) in lips {
        let mut d = (b - a).normalize_or_zero();
        if d == Vec3::ZERO {
            continue;
        }
        let flip =
            d.x < -1e-4 || (d.x.abs() <= 1e-4 && (d.y < -1e-4 || (d.y.abs() <= 1e-4 && d.z < 0.)));
        if flip {
            d = -d;
            std::mem::swap(&mut a, &mut b);
        }
        let o = a - d * a.dot(d);
        let k = (
            (d * 64.).to_array().map(|x| x.round() as i32),
            (o * 2.).to_array().map(|x| x.round() as i32),
        );
        let line = lines.entry(k).or_insert((d, o, Vec::new()));
        line.2.push((a.dot(line.0), b.dot(line.0)));
    }
    let mut runs = Vec::new();
    for (d, o, mut spans) in lines.into_values() {
        spans.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut current = spans[0];
        for &(t0, t1) in &spans[1..] {
            if t0 <= current.1 + 1.5 {
                current.1 = current.1.max(t1);
            } else {
                runs.push((o + d * current.0, o + d * current.1));
                current = (t0, t1);
            }
        }
        runs.push((o + d * current.0, o + d * current.1));
    }
    runs
}

/// Runs joined end to end into polylines through joints where exactly two
/// runs meet at a gentle turn.
fn chain(runs: Vec<(Vec3, Vec3)>) -> Vec<Vec<Vec3>> {
    let node = |v: Vec3| v.to_array().map(|x| x.round() as i32);
    let mut at: HashMap<[i32; 3], Vec<(usize, bool)>> = HashMap::new();
    for (i, (a, b)) in runs.iter().enumerate() {
        at.entry(node(*a)).or_default().push((i, false));
        at.entry(node(*b)).or_default().push((i, true));
    }
    let mut used = vec![false; runs.len()];
    let far = |i: usize, from_end: bool| if from_end { runs[i].0 } else { runs[i].1 };
    let near = |i: usize, from_end: bool| if from_end { runs[i].1 } else { runs[i].0 };
    // The run continuing a polyline at `joint`, arriving along `heading`.
    let next = |joint: Vec3, heading: Vec3, used: &[bool]| -> Option<(usize, bool)> {
        let there = at.get(&node(joint))?;
        if there.len() != 2 {
            return None;
        }
        let &(i, at_end) = there.iter().find(|(i, _)| !used[*i])?;
        let leave = (far(i, at_end) - near(i, at_end)).normalize_or_zero();
        (heading.dot(leave) >= MIN_TURN_COS).then_some((i, at_end))
    };
    let mut rails = Vec::new();
    for start in 0..runs.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let (a, b) = runs[start];
        let mut points = std::collections::VecDeque::from([a, b]);
        // Forward from b, then backward from a.
        let mut tip = b;
        let mut heading = (b - a).normalize_or_zero();
        while let Some((i, at_end)) = next(tip, heading, &used) {
            used[i] = true;
            let to = far(i, at_end);
            heading = (to - tip).normalize_or_zero();
            tip = to;
            points.push_back(to);
        }
        let mut tip = a;
        let mut heading = (a - b).normalize_or_zero();
        while let Some((i, at_end)) = next(tip, heading, &used) {
            used[i] = true;
            let to = far(i, at_end);
            heading = (to - tip).normalize_or_zero();
            tip = to;
            points.push_front(to);
        }
        rails.push(points.into_iter().collect());
    }
    rails
}
