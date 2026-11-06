//! Curve evaluator and the three-preset surface blend (spec 2.2, 2.3).

/// A vehicle curve: `x`/`y` knots plus the end tangents `mleft` / `mright` (dy/dx).
#[derive(Clone, Debug, PartialEq)]
pub struct Curve {
    pub x: Vec<f64>,
    pub y: Vec<f64>,
    pub mleft: f64,
    pub mright: f64,
}

impl Curve {
    pub fn new(knots: &[(f64, f64)], mleft: f64, mright: f64) -> Self {
        Self {
            x: knots.iter().map(|k| k.0).collect(),
            y: knots.iter().map(|k| k.1).collect(),
            mleft,
            mright,
        }
    }

    pub fn len(&self) -> usize {
        self.x.len()
    }

    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Evaluates the curve exactly like `00438ee0`.
    ///
    /// - The last segment containing `x` wins (matters at shared knots).
    /// - Outside the knot range the first / last y is returned; the end slopes do not extrapolate.
    /// - Catmull-Rom Hermite in index space: uneven knot spacing is ignored, so it overshoots
    ///   (450F torque peaks at 40.7 lbf ft, above its highest knot).
    /// - The end segments use phantom points built from `mleft` / `mright`.
    ///
    /// Stock curves have 3..10 knots. With 2 knots the original reads a stale slot; this uses both
    /// end slopes instead.
    pub fn eval(&self, x: f64) -> f64 {
        let (xs, ys) = (&self.x, &self.y);
        let n = xs.len();
        match n {
            0 => return 0.0,
            1 => return ys[0],
            _ => {}
        }
        let mut seg = None;
        for i in 0..n - 1 {
            if xs[i] <= x && x <= xs[i + 1] {
                seg = Some(i);
            }
        }
        let Some(i) = seg else {
            return if x < xs[0] { ys[0] } else { ys[n - 1] };
        };
        let dx = xs[i + 1] - xs[i];
        let t = (x - xs[i]) / dx;
        let p1 = ys[i];
        let p2 = ys[i + 1];
        let p0 = if i == 0 { p2 - 2.0 * self.mleft * dx } else { ys[i - 1] };
        let p3 = if i == n - 2 { p1 + 2.0 * self.mright * dx } else { ys[i + 2] };
        let m0 = (p2 - p0) * 0.5;
        let m1 = (p3 - p1) * 0.5;
        let c2 = 3.0 * (p2 - p1) - 2.0 * m0 - m1;
        let c3 = m0 + m1 - 2.0 * (p2 - p1);
        ((c3 * t + c2) * t + m0) * t + p1
    }
}

/// MSVC `_ftol2` after `x + copysign(0.5, x)`: round half away from zero, then truncate.
fn ftol_round(x: f64) -> i64 {
    (x + 0.5f64.copysign(x)) as i64
}

/// Three-preset blend `004390d0`: `a` = (Sticky, Normal, Slippery), `s` = grip or drag index in [0, 1].
///
/// `lerp(a[lo], a[hi], 2s - lo)` with `lo = round(2s - 0.49999999999)`, `hi = round(2s)`. Quirk kept on
/// purpose: when the fractional part of 2s is below 0.5, lo == hi and the result is flat (s = 0.65 is
/// pure Normal).
pub fn blend3(a: &[f64; 3], s: f64) -> f64 {
    let s2 = s * 2.0;
    let lo = ftol_round(s2 - 0.49999999999).clamp(0, 2) as usize;
    let hi = ftol_round(s2).clamp(0, 2) as usize;
    let t = s2 - lo as f64;
    let (a0, a1) = (a[lo], a[hi]);
    if t <= 0.0 {
        a0
    } else if t >= 1.0 {
        a1
    } else {
        a0 + (a1 - a0) * t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_outside_range() {
        let c = Curve::new(&[(0.0, 1.0), (1.0, 2.0), (2.0, 5.0)], 10.0, -10.0);
        assert_eq!(c.eval(-1.0), 1.0);
        assert_eq!(c.eval(3.0), 5.0);
        assert_eq!(c.eval(0.0), 1.0);
        assert_eq!(c.eval(2.0), 5.0);
    }

    #[test]
    fn blend3_table() {
        // grip classes -> StaticMu (1.5, 1.3, 1.0), table in docs\fields\susp-tires.md
        let mu = [1.5, 1.3, 1.0];
        let cases = [(0.1, 1.5), (0.3, 1.38), (0.5, 1.3), (0.65, 1.3), (0.8, 1.12), (0.9, 1.06)];
        for (s, want) in cases {
            assert!((blend3(&mu, s) - want).abs() < 1e-9, "s={s}: {} != {want}", blend3(&mu, s));
        }
        // drag classes -> TerrainDrag (0, 5, 30)
        let drag = [0.0, 5.0, 30.0];
        for (s, want) in [(0.0, 0.0), (0.25, 2.5), (0.75, 17.5), (1.0, 30.0)] {
            assert!((blend3(&drag, s) - want).abs() < 1e-9, "s={s}");
        }
    }
}
