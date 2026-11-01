//! Halo 3's function blobs (`docs/format/effects.md` 2): the bytes behind
//! every particle, contrail and light property. A blob maps an input in
//! 0..1 to an output between its minimum and maximum, or between two to
//! four colours.
//!
//! Layout (little endian):
//!
//! | offset | field |
//! |---|---|
//! | 0x00 | u8 kind: 0 identity, 1 constant, 3 periodic, 8 curve |
//! | 0x01 | u8 flags: bit 0 ranged (a second curve, picked between by the range input) |
//! | 0x02 | u8 colours: 0 scalar, 1..4 a colour gradient of that many stops |
//! | 0x04 | f32 minimum / u32 colour 0 (`x R G B`) |
//! | 0x08 | f32 maximum / u32 colour 1 |
//! | 0x0C, 0x10 | u32 colours 2 and 3 |
//! | 0x1C | u32 bytes of curve data that follow (curves and periodic) |
//! | 0x20 | the curve data |
//!
//! A curve is a u32 segment count, then per segment a u32 shape (low byte
//! 4: linear, two coefficients; 7: cubic, four; 10: cubic plus three more
//! values), an f32 end (the input it is used up to; the last is
//! `f32::MAX`) and the coefficients, highest power first, in the input
//! itself (not the segment's local input). A ranged curve has two curves
//! one after the other.

use crate::{Error, Result};

/// One curve segment: used for inputs up to `end`.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub end: f32,
    /// `a x^3 + b x^2 + c x + d`; linear segments fill `c` and `d`.
    pub cubic: [f32; 4],
}

/// The shape a function's input goes through before it is scaled to the
/// output range.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// The input itself.
    Identity,
    /// Always the start of the range (or a point in it picked by the
    /// range input, when ranged).
    Constant,
    /// Piecewise polynomials; two when ranged.
    Curve(Vec<Segment>, Option<Vec<Segment>>),
    /// A wave: kind, frequency, phase, and its own output range; two when
    /// ranged.
    Periodic([Wave; 2]),
}

/// One periodic wave (kind 3), as stored: 20 bytes per wave.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wave {
    /// The wave's own kind byte (Halo's periodic function list).
    pub kind: u8,
    pub frequency: f32,
    pub phase: f32,
    pub min: f32,
    pub max: f32,
}

/// A decoded function blob.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    pub kind: u8,
    pub ranged: bool,
    pub shape: Shape,
    pub min: f32,
    pub max: f32,
    /// 0 for a scalar function, else how many colour stops it has.
    pub colour_stops: u8,
    /// The four colour slots, linear RGB (from 8-bit sRGB).
    pub colours: [[f32; 3]; 4],
}

impl Function {
    /// A constant scalar.
    pub fn constant(v: f32) -> Function {
        Function {
            kind: 1,
            ranged: false,
            shape: Shape::Constant,
            min: v,
            max: v,
            colour_stops: 0,
            colours: [[1.0; 3]; 4],
        }
    }

    /// Reads a blob.
    pub fn parse(b: &[u8]) -> Result<Function> {
        if b.len() < 0x20 {
            return Err(Error::Invalid(format!("function blob of {} bytes", b.len())));
        }
        let f = |o: usize| f32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let u = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let kind = b[0];
        let ranged = b[1] & 1 != 0;
        let colour_stops = b[2];
        let colours = [u(4), u(8), u(0xC), u(0x10)].map(rgb);
        let data = &b[0x20..];
        let shape = match kind {
            0 => Shape::Identity,
            1 => Shape::Constant,
            8 => {
                let mut at = 0;
                let first = curve(data, &mut at)?;
                let second = if ranged { Some(curve(data, &mut at)?) } else { None };
                Shape::Curve(first, second)
            }
            3 => {
                let wave = |o: usize| -> Result<Wave> {
                    let w = data.get(o..o + 20).ok_or_else(|| Error::Invalid("short periodic function".into()))?;
                    let g = |k: usize| f32::from_le_bytes(w[k..k + 4].try_into().unwrap());
                    Ok(Wave { kind: w[0], frequency: g(4), phase: g(8), min: g(12), max: g(16) })
                };
                let a = wave(0)?;
                let b = if ranged { wave(20)? } else { a };
                Shape::Periodic([a, b])
            }
            k => return Err(Error::Unsupported(format!("function kind {k}"))),
        };
        Ok(Function {
            kind,
            ranged,
            shape,
            min: f(4),
            max: f(8),
            colour_stops,
            colours,
        })
    }

    /// The shaped input, 0..1 for well-behaved curves.
    pub fn shaped(&self, x: f32, range: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        match &self.shape {
            Shape::Identity => x,
            Shape::Constant => {
                if self.ranged {
                    range
                } else {
                    0.0
                }
            }
            Shape::Curve(a, b) => {
                let ya = eval_curve(a, x);
                match b {
                    Some(b) => ya + (eval_curve(b, x) - ya) * range,
                    None => ya,
                }
            }
            Shape::Periodic([a, b]) => {
                let ya = a.eval(x);
                if self.ranged { ya + (b.eval(x) - ya) * range } else { ya }
            }
        }
    }

    /// The scalar output for input `x` and range input `range` (both 0..1).
    pub fn scalar(&self, x: f32, range: f32) -> f32 {
        let t = self.shaped(x, range);
        self.min + (self.max - self.min) * t
    }

    /// The colour output (linear RGB). A one-stop function is its first
    /// colour; two stops run from the first slot to the last; three from
    /// the first through the second to the last; four through all four.
    pub fn colour(&self, x: f32, range: f32) -> [f32; 3] {
        let t = self.shaped(x, range).clamp(0.0, 1.0);
        let c = &self.colours;
        let stops: Vec<[f32; 3]> = match self.colour_stops {
            0 | 1 => return c[0],
            2 => vec![c[0], c[3]],
            3 => vec![c[0], c[1], c[3]],
            _ => vec![c[0], c[1], c[2], c[3]],
        };
        let n = stops.len() - 1;
        let p = t * n as f32;
        let i = (p.floor() as usize).min(n - 1);
        let k = p - i as f32;
        let (a, b) = (stops[i], stops[i + 1]);
        [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k, a[2] + (b[2] - a[2]) * k]
    }

    /// Whether the output never changes.
    pub fn is_constant(&self) -> bool {
        matches!(self.shape, Shape::Constant) && !self.ranged || self.min == self.max && self.colour_stops <= 1
    }
}

impl Wave {
    /// Our reading of the wave kinds that matter (0 one, 1 zero, 2 cosine,
    /// everything else a cosine too): `min..max` over one cycle.
    fn eval(&self, x: f32) -> f32 {
        let s = match self.kind {
            0 => 1.0,
            1 => 0.0,
            _ => 0.5 - 0.5 * (std::f32::consts::TAU * (x * self.frequency + self.phase)).cos(),
        };
        self.min + (self.max - self.min) * s
    }
}

fn rgb(v: u32) -> [f32; 3] {
    let c = |s: u32| srgb_to_linear(((v >> s) & 0xFF) as f32 / 255.0);
    [c(16), c(8), c(0)]
}

/// sRGB to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn curve(data: &[u8], at: &mut usize) -> Result<Vec<Segment>> {
    let u = |o: usize| -> Result<u32> {
        data.get(o..o + 4)
            .map(|w| u32::from_le_bytes(w.try_into().unwrap()))
            .ok_or_else(|| Error::Invalid("short curve".into()))
    };
    let f = |o: usize| -> Result<f32> { u(o).map(f32::from_bits) };
    let n = u(*at)?;
    if n > 64 {
        return Err(Error::Invalid(format!("curve of {n} segments")));
    }
    *at += 4;
    let mut out = Vec::with_capacity(n as usize);
    for _ in 0..n {
        let shape = u(*at)? & 0xFF;
        let end = f(*at + 4)?;
        let values = match shape {
            4 => 2,
            7 => 4,
            10 => 7,
            s => return Err(Error::Unsupported(format!("curve segment shape {s}"))),
        };
        let c: Vec<f32> = (0..values).map(|k| f(*at + 8 + 4 * k)).collect::<Result<_>>()?;
        let cubic = if values == 2 { [0.0, 0.0, c[0], c[1]] } else { [c[0], c[1], c[2], c[3]] };
        out.push(Segment { end, cubic });
        *at += 8 + 4 * values;
    }
    Ok(out)
}

fn eval_curve(segments: &[Segment], x: f32) -> f32 {
    let Some(s) = segments.iter().find(|s| x <= s.end).or(segments.last()) else {
        return 0.0;
    };
    let [a, b, c, d] = s.cubic;
    ((a * x + b) * x + c) * x + d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn two_segment_curve_is_continuous() {
        // fireball_human_large's alpha: holds 1 to 0.5, then falls to 0.
        let b = blob(&[
            0x00003408, 0, 0x3f800000, 0, 0, 0, 0, 0x34, 2, 0xcd000004, 0x3f000000, 0, 0x3f800000, 7, 0x7f7fffff,
            3.631f32.to_bits(), (-5.301f32).to_bits(), (-0.4038f32).to_bits(), 2.073f32.to_bits(),
        ]);
        let f = Function::parse(&b).unwrap();
        assert!((f.scalar(0.25, 0.0) - 1.0).abs() < 1e-4);
        assert!((f.scalar(0.5, 0.0) - 1.0).abs() < 2e-3);
        assert!(f.scalar(1.0, 0.0).abs() < 2e-3);
    }

    #[test]
    fn ranged_constant_picks_by_range() {
        let b = blob(&[0x00003501, 1.5f32.to_bits(), 1.0f32.to_bits(), 0, 0, 0, 0, 0]);
        let f = Function::parse(&b).unwrap();
        assert_eq!(f.scalar(0.3, 0.0), 1.5);
        assert_eq!(f.scalar(0.3, 1.0), 1.0);
    }

    #[test]
    fn two_colour_gradient_uses_first_and_last_slot() {
        let b = blob(&[0x00023400, 0x00ffffff, 0x3f800000, 0, 0x00000000, 0, 0, 0]);
        let f = Function::parse(&b).unwrap();
        assert_eq!(f.colour(0.0, 0.0), [1.0, 1.0, 1.0]);
        assert_eq!(f.colour(1.0, 0.0), [0.0, 0.0, 0.0]);
    }
}
