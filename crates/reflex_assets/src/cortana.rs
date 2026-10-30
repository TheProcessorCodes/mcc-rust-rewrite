//! The Cortana graphics kit: holographic blue plastics over the 450F's
//! THQ / Green sheet, with the number plates kept stock.
//!
//! The user made this kit for Reflex itself (`D:\reflex-tools\physics\
//! reflex\master-chief-skin`, `tools\cortana.py`); an install with it
//! already in place is used as is. Any other install has the stock green
//! sheet, which `paint` repaints here the same way: every texel of the
//! sheet learns where on the bike it is (a bake of the plastics in UV
//! space), and the pattern is drawn from that. The random choices come
//! from a different generator than numpy's, so the traces, blocks and
//! glitch bands land elsewhere; the design is the same.

use crate::FT;
use crate::bike::{Part, apply};
use crate::texture::Rgba;

/// Whether a kit sheet is already Cortana's blue rather than THQ green.
pub fn is_cortana(sheet: &Rgba) -> bool {
    let mut sum = [0f64; 3];
    for p in &sheet.pixels {
        for c in 0..3 {
            sum[c] += p[c] as f64;
        }
    }
    sum[2] > sum[1] * 1.15 && sum[2] > sum[0] * 1.5
}

type Rgb = [f32; 3];

const fn hex(v: u32) -> Rgb {
    [((v >> 16) & 255) as f32 / 255.0, ((v >> 8) & 255) as f32 / 255.0, (v & 255) as f32 / 255.0]
}

const BASE: Rgb = hex(0x2F7FD6);
const HI: Rgb = hex(0x6FC3FF);
const DEEP: Rgb = hex(0x123A6B);
const VIO: Rgb = hex(0x5B6CFF);
const CORE: Rgb = hex(0xE6F7FF);

/// The Dakumoto sheet's two side plates and front plate, then the small
/// hardware bits kept stock: boxes in UV (u0, v0, u1, v1).
const PLATES: [[f32; 4]; 3] = [[0.265, 0.40, 0.565, 0.70], [0.555, 0.40, 0.875, 0.70], [0.70, 0.64, 0.89, 0.995]];
const KEEP: [[f32; 4]; 3] = [[0.44, 0.0, 0.50, 0.075], [0.50, 0.03, 0.545, 0.13], [0.82, 0.0, 0.935, 0.09]];

fn scale(a: Rgb, k: f32) -> Rgb {
    a.map(|v| v * k)
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

/// `colour` over `dst` at `alpha` (clamped).
fn over(dst: Rgb, colour: Rgb, alpha: f32) -> Rgb {
    lerp(dst, colour, alpha.clamp(0.0, 1.0))
}

/// A small deterministic generator (splitmix64).
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// 0..1.
    fn random(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    fn uniform(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.random()
    }

    /// `lo..hi`.
    fn integer(&mut self, lo: i64, hi: i64) -> i64 {
        lo + (self.next() % (hi - lo) as u64) as i64
    }
}

/// Value noise in 3D, smoothly interpolated between hashed lattice values.
struct Noise(Vec<f32>);

impl Noise {
    fn new() -> Noise {
        let mut r = Rng(1234);
        Noise((0..1 << 16).map(|_| r.random()).collect())
    }

    fn value(&self, p: [f32; 3], freq: f32, seed: i64) -> f32 {
        let q = p.map(|v| v * freq + seed as f32 * 17.123);
        let i = q.map(|v| v.floor() as i64);
        let f: [f32; 3] = std::array::from_fn(|k| {
            let f = q[k] - i[k] as f32;
            f * f * (3.0 - 2.0 * f)
        });
        let mut out = 0.0;
        for dx in 0..2 {
            let wx = if dx == 1 { f[0] } else { 1.0 - f[0] };
            for dy in 0..2 {
                let wy = if dy == 1 { f[1] } else { 1.0 - f[1] };
                for dz in 0..2 {
                    let wz = if dz == 1 { f[2] } else { 1.0 - f[2] };
                    let k = (i[0] + dx).wrapping_mul(73_856_093)
                        ^ (i[1] + dy).wrapping_mul(19_349_663)
                        ^ (i[2] + dz).wrapping_mul(83_492_791)
                        ^ seed.wrapping_mul(2_654_435_761);
                    out += wx * wy * wz * self.0[(k & 0xFFFF) as usize];
                }
            }
        }
        out
    }

    fn fbm(&self, p: [f32; 3], freq: f32, octaves: i32, seed: i64) -> f32 {
        let (mut sum, mut a, mut total) = (0.0, 1.0, 0.0);
        for o in 0..octaves {
            sum += a * self.value(p, freq * 2f32.powi(o), seed + o as i64);
            total += a;
            a *= 0.5;
        }
        sum / total
    }
}

/// A faint hexagon outline grid on the texture, cells `cell` texels wide.
fn hexgrid(x: usize, y: usize, cell: f32) -> f32 {
    let q = x as f32 / cell;
    let r = y as f32 / (cell * 0.866);
    let rr = r.floor();
    let qq = q + 0.5 * rr.rem_euclid(2.0);
    let fx = qq - qq.floor() - 0.5;
    let fy = r - rr - 0.5;
    let d = fx.abs().max(fx.abs() * 0.5 + fy.abs() * 0.866 * 1.155);
    (1.0 - (d - 0.48).abs() / 0.035).clamp(0.0, 1.0)
}

/// A bool image and the binary morphology the plate mask needs (a cross
/// as the structuring element, nothing set outside).
#[derive(Clone)]
struct Mask {
    w: usize,
    h: usize,
    on: Vec<bool>,
}

impl Mask {
    fn new(w: usize, h: usize) -> Mask {
        Mask { w, h, on: vec![false; w * h] }
    }

    fn at(&self, x: isize, y: isize) -> bool {
        x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h && self.on[y as usize * self.w + x as usize]
    }

    fn step(&self, grow: bool) -> Mask {
        let mut out = Mask::new(self.w, self.h);
        for y in 0..self.h as isize {
            for x in 0..self.w as isize {
                let n = [self.at(x - 1, y), self.at(x + 1, y), self.at(x, y - 1), self.at(x, y + 1)];
                let me = self.at(x, y);
                out.on[y as usize * self.w + x as usize] =
                    if grow { me || n.iter().any(|&v| v) } else { me && n.iter().all(|&v| v) };
            }
        }
        out
    }

    fn dilate(&self, n: usize) -> Mask {
        (0..n).fold(self.clone(), |m, _| m.step(true))
    }

    fn erode(&self, n: usize) -> Mask {
        (0..n).fold(self.clone(), |m, _| m.step(false))
    }

    /// Connected regions (4-neighbour): a label per texel (0 = off) and sizes.
    fn label(&self) -> (Vec<usize>, Vec<usize>) {
        let mut labels = vec![0; self.on.len()];
        let mut sizes = vec![0];
        for start in 0..self.on.len() {
            if !self.on[start] || labels[start] != 0 {
                continue;
            }
            let id = sizes.len();
            sizes.push(0);
            let mut stack = vec![start];
            labels[start] = id;
            while let Some(i) = stack.pop() {
                sizes[id] += 1;
                let (x, y) = ((i % self.w) as isize, (i / self.w) as isize);
                for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                    if self.at(nx, ny) {
                        let j = ny as usize * self.w + nx as usize;
                        if labels[j] == 0 {
                            labels[j] = id;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        (labels, sizes)
    }

    /// Off texels not reachable from the edge without crossing on ones become on.
    fn fill_holes(&self) -> Mask {
        let mut outside = vec![false; self.on.len()];
        let mut stack: Vec<usize> = (0..self.on.len())
            .filter(|&i| {
                let (x, y) = (i % self.w, i / self.w);
                (x == 0 || y == 0 || x + 1 == self.w || y + 1 == self.h) && !self.on[i]
            })
            .collect();
        for &i in &stack {
            outside[i] = true;
        }
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % self.w) as isize, (i / self.w) as isize);
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx >= 0 && ny >= 0 && (nx as usize) < self.w && (ny as usize) < self.h {
                    let j = ny as usize * self.w + nx as usize;
                    if !self.on[j] && !outside[j] {
                        outside[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        Mask {
            w: self.w,
            h: self.h,
            on: outside.iter().map(|&o| !o).collect(),
        }
    }
}

fn floats(stock: &Rgba) -> Vec<Rgb> {
    stock.pixels.iter().map(|p| [p[0], p[1], p[2]].map(|v| v as f32 / 255.0)).collect()
}

/// The stock number-plate backgrounds: near-white, unsaturated texels in
/// the plate boxes, cleaned up into whole plates, minus coloured accents.
fn plate_mask(stock: &[Rgb], w: usize, h: usize) -> Mask {
    let max = |p: &Rgb| p[0].max(p[1]).max(p[2]);
    let min = |p: &Rgb| p[0].min(p[1]).min(p[2]);
    let mut m = Mask::new(w, h);
    for [u0, v0, u1, v1] in PLATES {
        let (x0, x1) = ((u0 * w as f32) as usize, ((u1 * w as f32) as usize).min(w));
        let (y0, y1) = ((v0 * h as f32) as usize, ((v1 * h as f32) as usize).min(h));
        let (bw, bh) = (x1 - x0, y1 - y0);
        let mut white = Mask::new(bw, bh);
        for y in 0..bh {
            for x in 0..bw {
                let p = &stock[(y0 + y) * w + x0 + x];
                white.on[y * bw + x] = min(p) > 0.55 && max(p) - min(p) < 0.12;
            }
        }
        let white = white.erode(2).dilate(2);
        let (labels, sizes) = white.label();
        let Some(&biggest) = sizes.iter().max().filter(|&&b| b > 0) else {
            continue;
        };
        let mut keep = Mask::new(bw, bh);
        for (k, &l) in labels.iter().enumerate() {
            keep.on[k] = l != 0 && sizes[l] as f32 > 0.15 * biggest as f32;
        }
        let keep = keep.dilate(4).erode(4).fill_holes();
        for y in 0..bh {
            for x in 0..bw {
                if keep.on[y * bw + x] {
                    m.on[(y0 + y) * w + x0 + x] = true;
                }
            }
        }
    }
    // Coloured kit accents the hole filling swallowed are painted as kit.
    for (on, p) in m.on.iter_mut().zip(stock) {
        let (hi, lo) = (max(p), min(p));
        if (hi - lo) / hi.max(1e-6) > 0.3 && hi > 0.15 {
            *on = false;
        }
    }
    m
}

fn keep_mask(w: usize, h: usize) -> Mask {
    let mut m = Mask::new(w, h);
    for [u0, v0, u1, v1] in KEEP {
        for y in (v0 * h as f32) as usize..((v1 * h as f32) as usize).min(h) {
            for x in (u0 * w as f32) as usize..((u1 * w as f32) as usize).min(w) {
                m.on[y * w + x] = true;
            }
        }
    }
    m
}

/// Texels left as they are on the stock sheet: the number plates and the
/// small hardware bits.
pub fn kept(stock: &Rgba) -> Vec<bool> {
    let (w, h) = (stock.width as usize, stock.height as usize);
    let plates = plate_mask(&floats(stock), w, h);
    let keep = keep_mask(w, h);
    plates.on.iter().zip(&keep.on).map(|(a, b)| *a || *b).collect()
}

/// Where each texel is on the bike: Reflex model space (feet, +X right),
/// the smoothed normal there, and whether it is the seat. Texels no
/// triangle covers take their nearest covered texel's values.
struct Bake {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    seat: Mask,
}

fn bake(pieces: &[&Part], rest: &impl Fn(usize) -> [[f32; 4]; 4], w: usize, h: usize) -> Bake {
    let mut pos = vec![[0.0f32; 3]; w * h];
    let mut nrm = vec![[0.0f32; 3]; w * h];
    let mut covered = vec![false; w * h];
    let mut seat = Mask::new(w, h);
    for part in pieces {
        let m = rest(part.bone);
        let p: Vec<[f32; 3]> = part
            .positions
            .iter()
            .map(|&v| {
                let b = apply(&m, v);
                [-b[0] / FT, b[1] / FT, b[2] / FT]
            })
            .collect();
        // The file's own winding (the parts were re-wound a, c, b).
        let tris: Vec<[usize; 3]> = part
            .indices
            .chunks_exact(3)
            .map(|t| [t[0] as usize, t[2] as usize, t[1] as usize])
            .filter(|t| t.iter().all(|&i| i < p.len()))
            .collect();
        let mut vn = vec![[0.0f32; 3]; p.len()];
        for t in &tris {
            let (a, b, c) = (p[t[0]], p[t[1]], p[t[2]]);
            let e1 = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let e2 = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
            for &i in t {
                for k in 0..3 {
                    vn[i][k] += n[k];
                }
            }
        }
        let is_seat = part.slot.contains("Seat");
        for t in &tris {
            let xy = t.map(|i| {
                let uv = part.uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                [uv[0] * w as f32, uv[1] * h as f32]
            });
            let [[x0, y0], [x1, y1], [x2, y2]] = xy;
            let den = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
            if den.abs() < 1e-12 {
                continue;
            }
            let minx = x0.min(x1).min(x2).floor().max(0.0) as usize;
            let maxx = (x0.max(x1).max(x2).ceil() as isize).min(w as isize - 1);
            let miny = y0.min(y1).min(y2).floor().max(0.0) as usize;
            let maxy = (y0.max(y1).max(y2).ceil() as isize).min(h as isize - 1);
            if maxx < 0 || maxy < 0 {
                continue;
            }
            for py in miny..=maxy as usize {
                for px in minx..=maxx as usize {
                    let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                    let a = ((y1 - y2) * (fx - x2) + (x2 - x1) * (fy - y2)) / den;
                    let b = ((y2 - y0) * (fx - x2) + (x0 - x2) * (fy - y2)) / den;
                    let c = 1.0 - a - b;
                    if a < -1e-4 || b < -1e-4 || c < -1e-4 {
                        continue;
                    }
                    let k = py * w + px;
                    let (pa, pb, pc) = (p[t[0]], p[t[1]], p[t[2]]);
                    let (na, nb, nc) = (vn[t[0]], vn[t[1]], vn[t[2]]);
                    pos[k] = std::array::from_fn(|i| a * pa[i] + b * pb[i] + c * pc[i]);
                    nrm[k] = std::array::from_fn(|i| a * na[i] + b * nb[i] + c * nc[i]);
                    covered[k] = true;
                    seat.on[k] = is_seat;
                }
            }
        }
    }
    // Uncovered texels take the nearest covered one (a breadth-first sweep
    // out from every covered texel, eight neighbours).
    let mut from: Vec<u32> = (0..w * h).map(|k| if covered[k] { k as u32 } else { u32::MAX }).collect();
    let mut queue: std::collections::VecDeque<usize> = (0..w * h).filter(|&k| covered[k]).collect();
    while let Some(k) = queue.pop_front() {
        let (x, y) = ((k % w) as isize, (k / w) as isize);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w as isize || ny >= h as isize {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if from[j] == u32::MAX {
                    from[j] = from[k];
                    queue.push_back(j);
                }
            }
        }
    }
    for k in 0..w * h {
        if !covered[k] && from[k] != u32::MAX {
            pos[k] = pos[from[k] as usize];
            nrm[k] = nrm[from[k] as usize];
        }
        let n = nrm[k];
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-9);
        nrm[k] = n.map(|v| v / l);
    }
    Bake { pos, nrm, seat }
}

/// Runs `f(y, row)` over every row of `out` on all cores.
fn rows<T: Send>(out: &mut [T], w: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let h = out.len() / w;
    let per = h.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (c, chunk) in out.chunks_mut(per * w).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(w).enumerate() {
                    f(c * per + r, row);
                }
            });
        }
    });
}

/// A grey canvas for the few things drawn as shapes (blocks, traces).
struct Canvas {
    w: usize,
    h: usize,
    v: Vec<f32>,
}

impl Canvas {
    fn new(w: usize, h: usize) -> Canvas {
        Canvas { w, h, v: vec![0.0; w * h] }
    }

    fn set(&mut self, x: isize, y: isize, v: f32) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            self.v[y as usize * self.w + x as usize] = v;
        }
    }

    /// A polyline `width` texels wide.
    fn line(&mut self, pts: &[[f32; 2]], width: usize, v: f32) {
        let r = width as f32 / 2.0;
        for seg in pts.windows(2) {
            let ([ax, ay], [bx, by]) = (seg[0], seg[1]);
            let steps = ((bx - ax).abs().max((by - ay).abs()) * 2.0).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let t = i as f32 / steps as f32;
                let (x, y) = (ax + (bx - ax) * t, ay + (by - ay) * t);
                if width <= 1 {
                    self.set(x.floor() as isize, y.floor() as isize, v);
                    continue;
                }
                for py in (y - r).floor() as isize..=(y + r).ceil() as isize {
                    for px in (x - r).floor() as isize..=(x + r).ceil() as isize {
                        let (dx, dy) = (px as f32 + 0.5 - x, py as f32 + 0.5 - y);
                        if dx * dx + dy * dy <= r * r {
                            self.set(px, py, v);
                        }
                    }
                }
            }
        }
    }

    /// A circle outline of radius `r`, `width` thick.
    fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, v: f32) {
        for py in (cy - r).floor() as isize..=(cy + r).ceil() as isize {
            for px in (cx - r).floor() as isize..=(cx + r).ceil() as isize {
                let d = ((px as f32 + 0.5 - cx).powi(2) + (py as f32 + 0.5 - cy).powi(2)).sqrt();
                if d <= r && d >= r - width {
                    self.set(px, py, v);
                }
            }
        }
    }

    /// Covers of a rectangle: every texel, or a border `width` thick.
    fn rect(&self, b: [f32; 4], width: Option<f32>) -> Vec<(isize, isize)> {
        let [x0, y0, x1, y1] = b.map(|v| v.round() as isize);
        let mut out = Vec::new();
        for y in y0..=y1 {
            for x in x0..=x1 {
                let inside = match width {
                    None => true,
                    Some(wd) => {
                        let wd = wd as isize;
                        x - x0 < wd || x1 - x < wd || y - y0 < wd || y1 - y < wd
                    }
                };
                if inside && x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
                    out.push((x, y));
                }
            }
        }
        out
    }

    /// Three box blurs, roughly a Gaussian of `sigma` texels.
    fn blur(&self, sigma: f32) -> Vec<f32> {
        let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as isize;
        let mut v = self.v.clone();
        let (w, h) = (self.w as isize, self.h as isize);
        for _ in 0..3 {
            for horizontal in [true, false] {
                let (n, lines) = if horizontal { (w, h) } else { (h, w) };
                let index = |line: isize, i: isize| {
                    let i = i.clamp(0, n - 1);
                    (if horizontal { line * w + i } else { i * w + line }) as usize
                };
                let mut out = v.clone();
                for line in 0..lines {
                    let mut sum: f32 = (-r..=r).map(|i| v[index(line, i)]).sum();
                    for i in 0..n {
                        out[index(line, i)] = sum / (2 * r + 1) as f32;
                        sum += v[index(line, i + r + 1)] - v[index(line, i - r)];
                    }
                }
                v = out;
            }
        }
        v
    }
}

/// Thin circuit-board traces (runs across, down and at 45° with pads at
/// the ends), as a 0..1 mask.
fn circuits(w: usize, h: usize, s: f32, seed: u64, n: usize) -> Canvas {
    let mut rng = Rng(seed);
    let mut c = Canvas::new(w, h);
    let step = 14.0 * s;
    let dirs: [(f32, f32); 8] = [(1., 0.), (0., 1.), (-1., 0.), (0., -1.), (1., 1.), (1., -1.), (-1., 1.), (-1., -1.)];
    for _ in 0..n {
        let (mut x, mut y) = (rng.uniform(0.0, w as f32), rng.uniform(0.0, h as f32));
        let mut d = rng.integer(0, 4) as usize;
        let mut pts = vec![[x, y]];
        for seg in 0..rng.integer(2, 6) {
            let len = rng.integer(2, 9) as f32 * step;
            let (dx, dy) = dirs[d];
            let k = if dx != 0.0 && dy != 0.0 { 1.414 } else { 1.0 };
            x += dx * len / k;
            y += dy * len / k;
            pts.push([x, y]);
            if rng.random() < 0.8 {
                let turn = [4i64, 5, -4, -3, 0][rng.integer(0, 5) as usize];
                d = (d as i64 + turn).rem_euclid(8) as usize;
            }
            let (dx, dy) = dirs[d];
            if seg > 0 && dx != 0.0 && dy != 0.0 && rng.random() < 0.5 {
                d = rng.integer(0, 4) as usize;
            }
        }
        let width = ([1.0, 1.0, 2.0][rng.integer(0, 3) as usize] * s).round().max(1.0) as usize;
        c.line(&pts, width, 1.0);
        let r = 2.5 * s;
        for p in [pts[0], pts[pts.len() - 1]] {
            if rng.random() < 0.8 {
                c.ring(p[0], p[1], r, s.floor().max(1.0), 1.0);
            }
        }
    }
    c
}

/// The stock sheet repainted Cortana. `pieces` are the parts drawn with the
/// sheet; `rest` gives a bone's rest transform from the bike frame.
pub fn paint(stock: &Rgba, pieces: &[&Part], rest: impl Fn(usize) -> [[f32; 4]; 4]) -> Rgba {
    let (w, h) = (stock.width as usize, stock.height as usize);
    let s = w as f32 / 2048.0;
    let stock_rgb = floats(stock);
    let bk = bake(pieces, &rest, w, h);
    let noise = Noise::new();
    let mut rng = Rng(117);

    // Base: deep to base to highlight on flowing 3D noise, lit from above;
    // violet patches; the faint hex grid; scanlines across the bike.
    let mut col = vec![[0.0f32; 3]; w * h];
    rows(&mut col, w, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let k = y * w + x;
            let (p, n) = (bk.pos[k], bk.nrm[k]);
            let f = noise.fbm(p, 0.9, 4, 1);
            let t = (0.18 + 1.3 * (f - 0.5) + 0.22 * n[1].clamp(-1.0, 1.0)).clamp(0.0, 1.0);
            let mut c = lerp(scale(DEEP, 0.8), BASE, (t * 1.6).clamp(0.0, 1.0));
            c = lerp(c, HI, ((t - 0.72) * 2.0).clamp(0.0, 1.0));
            let vio = ((noise.fbm(p, 0.6, 3, 7) - 0.58).clamp(0.0, 1.0) * 2.2).clamp(0.0, 1.0);
            c = lerp(c, lerp(c, VIO, 0.75), vio);
            c = over(c, HI, hexgrid(x, y, 22.0 * s) * 0.16);
            let scan = if (p[1] * 40.0).rem_euclid(1.0) < 0.18 { 0.22 } else { 0.0 };
            *out = over(c, scale(DEEP, 0.7), scan);
        }
    });

    // Glitch slices: bands across the bike that take a shifted, bluer copy.
    let bands: Vec<[f32; 6]> = (0..14)
        .map(|_| {
            let (y0, height) = (rng.uniform(-0.9, 2.0), rng.uniform(0.012, 0.06));
            let (off, period, on) = (rng.uniform(0.0, 3.0), rng.uniform(0.8, 2.5), rng.uniform(0.3, 1.2));
            [y0, height, off, period, on, rng.uniform(0.5, 1.0)]
        })
        .collect();
    let shift = (37.0 * s) as usize;
    let before = col.clone();
    rows(&mut col, w, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let p = bk.pos[y * w + x];
            let gl = bands
                .iter()
                .filter(|b| p[1] > b[0] && p[1] < b[0] + b[1] && (p[2] + b[2]).rem_euclid(b[3]) < b[4])
                .map(|b| b[5])
                .fold(0.0f32, f32::max);
            if gl > 0.0 {
                let src = before[y * w + (x + w - shift % w) % w];
                let shifted = [src[0] * 0.9 + 0.06, src[1] * 1.1 + 0.06, src[2] * 1.45 + 0.06];
                let c = *out;
                let mixed = lerp(c, shifted, 0.7);
                *out = std::array::from_fn(|i| mixed[i] + (VIO[i] - c[i]) * 0.25 * gl);
            }
        }
    });

    // Data blocks: small rectangles, some hollow.
    let mut blocks = vec![[0.0f32; 3]; w * h];
    let mut alpha = Canvas::new(w, h);
    let count = (520.0 * s * s * 4.0) as usize / 4 + 260;
    for _ in 0..count {
        let (cx, cy) = (rng.uniform(0.0, w as f32), rng.uniform(0.0, h as f32));
        let (bw, bh) = (rng.uniform(3.0, 26.0) * s, rng.uniform(2.0, 9.0) * s);
        let c = [HI, CORE, VIO, scale(BASE, 1.3)][rng.integer(0, 4) as usize].map(|v| (v.clamp(0.0, 1.0) * 255.0).floor() / 255.0);
        let a = rng.uniform(70.0, 200.0).floor() / 255.0;
        let hollow = rng.random() < 0.3;
        for (x, y) in alpha.rect([cx, cy, cx + bw, cy + bh], hollow.then(|| s.floor().max(1.0))) {
            let k = y as usize * w + x as usize;
            blocks[k] = c;
            alpha.v[k] = a;
        }
    }
    for ((c, b), a) in col.iter_mut().zip(&blocks).zip(&alpha.v) {
        *c = std::array::from_fn(|i| c[i] * (1.0 - a * 0.8) + b[i] * a * 0.8);
    }

    // Circuit traces with a glow.
    let traces = circuits(w, h, s, 5, 95);
    let glow = traces.blur(4.0 * s);
    for ((c, t), g) in col.iter_mut().zip(&traces.v).zip(&glow) {
        let lit: Rgb = std::array::from_fn(|i| (c[i] + HI[i] * g * 1.4 * 0.55).clamp(0.0, 1.0));
        *c = over(lit, CORE, t * 0.9);
    }

    // Bright diagonal streaks across the bike.
    let streaks: Vec<[f32; 3]> = (0..7)
        .map(|_| [rng.uniform(-3.0, 3.0), rng.uniform(0.01, 0.04), rng.uniform(0.25, 0.6)])
        .collect();
    for (c, p) in col.iter_mut().zip(&bk.pos) {
        let u = p[2] * 0.55 + p[1];
        let streak = streaks
            .iter()
            .map(|[o, wd, k]| (-((u - o) / wd).powi(2)).exp() * k)
            .fold(0.0f32, f32::max);
        *c = std::array::from_fn(|i| (c[i] + (HI[i] * 0.6 + 0.2) * streak).clamp(0.0, 1.0));
    }

    // The seat: deep navy, a fainter hex and the traces. The HR mesh has
    // one-texel slivers of seat along plastic edges; they are dropped.
    let seat = bk.seat.erode(1).dilate(1 + (3.0 * s) as usize);
    rows(&mut col, w, |y, row| {
        for (x, out) in row.iter_mut().enumerate() {
            let k = y * w + x;
            if !seat.on[k] {
                continue;
            }
            let f = noise.fbm(bk.pos[k], 6.0, 3, 9).clamp(0.0, 1.0);
            let c = lerp(scale(DEEP, 0.45), scale(DEEP, 0.9), f);
            *out = over(c, scale(HI, 0.8), hexgrid(x, y, 14.0 * s) * 0.12 + traces.v[k] * 0.25);
        }
    });

    // The plates and hardware stay stock, with a thin dark trim round the plates.
    let plates = plate_mask(&stock_rgb, w, h);
    let keep = keep_mask(w, h);
    let edge = plates.dilate(((4.0 * s) as usize).max(1));
    for k in 0..w * h {
        if edge.on[k] && !plates.on[k] {
            col[k] = scale(DEEP, 0.5);
        }
        if plates.on[k] || keep.on[k] {
            col[k] = stock_rgb[k];
        }
    }

    Rgba {
        width: stock.width,
        height: stock.height,
        pixels: col
            .iter()
            .map(|c| {
                let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                [r, g, b, 255]
            })
            .collect(),
    }
}
