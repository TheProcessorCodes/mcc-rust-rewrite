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
    /// Stock curves have 3..10 knots. With 2 knots the original's first segment reads the unused slot y[2]
    /// as its far neighbour; this uses the end slope `mright` instead.
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
        // OpenReflex bike_curve.c 00438ee0: neighbours prev / cur / next / next2, half differences B and A,
        // C = 3 ((next - cur) - B) - (A - B), D = ((A - B) - 2 C) * 0.33333299 (009774d8, not exactly 1/3).
        let d = xs[i + 1] - xs[i];
        let t = (x - xs[i]) / d;
        let cur = ys[i];
        let next = ys[i + 1];
        let prev = if i == 0 { next - self.mleft * d * 2.0 } else { ys[i - 1] };
        let next2 = if i == n - 2 { self.mright * d * 2.0 + cur } else { ys[i + 2] };
        let a = (next2 - cur) * 0.5;
        let b = (next - prev) * 0.5;
        let ab = a - b;
        let c = ((next - cur) - b) * 3.0 - ab;
        let dd = (ab - c * 2.0) * 0.333_332_99;
        ((dd * t + c) * t + b) * t + cur
    }
}

/// MSVC `_ftol2` after `x + copysign(0.5, x)`: round half away from zero, then truncate.
fn ftol_round(x: f64) -> i64 {
    (x + 0.5f64.copysign(x)) as i64
}

/// Three-preset blend `004390d0` (OpenReflex bike_curve.c IndexedCurve_Interp): `a` = (Sticky, Normal,
/// Slippery), `s` = grip or drag index in [0, 1].
///
/// `lerp_clamped(2s - lo, a[lo], a[hi])` with `lo = round(2s - 0.49999999999)`, `hi = round(2s)`, rounding
/// halves away from zero. Quirk kept on purpose: when the fractional part of 2s is below 0.5, lo == hi and
/// the result is flat (s = 0.65 is pure Normal). The game's table holds 20 entries, one every 0.5 of the
/// index; a grip or drag index in [0, 1] only reaches the first three, so this clamps instead of reading
/// past them.
pub fn blend3(a: &[f64; 3], s: f64) -> f64 {
    let s2 = s * 2.0;
    let lo = ftol_round(s2 - 0.49999999999).clamp(0, 2) as usize;
    let hi = ftol_round(s2).clamp(0, 2) as usize;
    let t = s2 - lo as f64;
    lerp_clamped(t, a[lo], a[hi])
}

/// `00430810` Lerp_Clamped (OpenReflex bike_curve.c): `t <= 0` gives `a`, `t >= 1` gives `b`, else
/// `(b - a) t + a`. A NaN `t` falls through to the formula.
pub fn lerp_clamped(t: f64, a: f64, b: f64) -> f64 {
    if 0.0 >= t {
        a
    } else if t >= 1.0 {
        b
    } else {
        (b - a) * t + a
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
        // at the far end of a segment (t = 1) the 0.33333299 factor misses the knot value by about 1e-6 of
        // the cubic term (here 5.0000144), as in the game
        assert!((c.eval(2.0) - 5.0).abs() < 1e-4);
    }

    #[test]
    fn eval_matches_hermite_form() {
        // inner segment of a 4-knot curve at t = 0.25: prev 0, cur 1, next 4, next2 9 (y = x^2 at x = 1.25)
        let c = Curve::new(&[(0.0, 0.0), (1.0, 1.0), (2.0, 4.0), (3.0, 9.0)], 0.0, 6.0);
        let (b, a) = (2.0, 4.0);
        let cc = 3.0 * ((4.0 - 1.0) - b) - (a - b);
        let d = ((a - b) - 2.0 * cc) * 0.333_332_99;
        let want = ((d * 0.25 + cc) * 0.25 + b) * 0.25 + 1.0;
        assert!((c.eval(1.25) - want).abs() < 1e-12);
        assert!((c.eval(1.25) - 1.5625).abs() < 1e-5);
        // first segment: phantom prev = next - 2 d s0 = -2, so B = 2, A = 0, C = 2, D ~ -2 -> 1.25 at t = 0.5
        let e = Curve::new(&[(0.0, 0.0), (2.0, 2.0), (4.0, 0.0)], 1.0, -1.0);
        assert!((e.eval(1.0) - 1.25).abs() < 1e-5, "{}", e.eval(1.0));
    }

    #[test]
    fn lerp_clamped_ends() {
        assert_eq!(lerp_clamped(-0.5, 2.0, 4.0), 2.0);
        assert_eq!(lerp_clamped(1.5, 2.0, 4.0), 4.0);
        assert_eq!(lerp_clamped(0.25, 2.0, 4.0), 2.5);
        assert!(lerp_clamped(f64::NAN, 2.0, 4.0).is_nan());
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
