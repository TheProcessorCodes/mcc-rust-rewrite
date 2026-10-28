//! Expands one level of any [`Format`] to 8-bit RGBA on the CPU: for PNG
//! output, for averaging a terrain blend map, and for the viewer when the
//! GPU cannot take the format as it is. Written from the BCn block layouts
//! in the D3D documentation; no decoder was copied.
//!
//! Channel conventions: A8 becomes `(a, a, a, a)` so a mask is visible in
//! a PNG; Y8 is grey; signed formats (BC5, U8V8) map -1..1 to 0..255 with
//! the normal's Z rebuilt in blue; FP16 is clamped to 0..1.

use crate::format::Format;
use crate::{Error, Result};

/// `width * height * 4` bytes of RGBA for one level of one layer.
pub fn decode_level(format: Format, width: u32, height: u32, data: &[u8]) -> Result<Vec<u8>> {
    let need = format.level_size(width, height, 0);
    if (data.len() as u64) < need {
        return Err(Error::Invalid(format!(
            "{} {width}x{height} needs {need} bytes, got {}",
            format.halo_name(),
            data.len()
        )));
    }
    let (w, h) = (width as usize, height as usize);
    let mut out = vec![0u8; w * h * 4];
    if format.is_block_compressed() {
        let bw = w.div_ceil(4);
        let unit = format.unit_bytes() as usize;
        for by in 0..h.div_ceil(4) {
            for bx in 0..bw {
                let o = (by * bw + bx) * unit;
                let block = &data[o..o + unit];
                let texels = decode_block(format, block);
                for (i, t) in texels.iter().enumerate() {
                    let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                    if x < w && y < h {
                        let p = (y * w + x) * 4;
                        out[p..p + 4].copy_from_slice(t);
                    }
                }
            }
        }
        return Ok(out);
    }
    let unit = format.unit_bytes() as usize;
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let s = &data[i * unit..i * unit + unit];
        let rgba = match format {
            Format::A8 => [s[0]; 4],
            Format::Y8 => [s[0], s[0], s[0], 255],
            Format::A4R4G4B4 => {
                let v = u16::from_le_bytes([s[0], s[1]]);
                let n = |shift: u16| ((v >> shift) & 0xF) as u8 * 17;
                [n(8), n(4), n(0), n(12)]
            }
            Format::X8R8G8B8 => [s[2], s[1], s[0], 255],
            Format::A8R8G8B8 => [s[2], s[1], s[0], s[3]],
            Format::U8V8 => {
                let (x, y) = (snorm8(s[0]), snorm8(s[1]));
                [unit_to_u8(x), unit_to_u8(y), unit_to_u8(rebuild_z(x, y)), 255]
            }
            Format::Rgba16F => {
                let c = |k: usize| {
                    let v = half_to_f32(u16::from_le_bytes([s[2 * k], s[2 * k + 1]]));
                    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                };
                [c(0), c(1), c(2), c(3)]
            }
            _ => unreachable!("block formats handled above"),
        };
        px.copy_from_slice(&rgba);
    }
    Ok(out)
}

/// The sixteen texels of one block, row by row.
fn decode_block(format: Format, b: &[u8]) -> [[u8; 4]; 16] {
    let mut out = [[0u8; 4]; 16];
    match format {
        Format::Bc1 => colour_block(b, true, &mut out),
        Format::Bc2 => {
            colour_block(&b[8..16], false, &mut out);
            for (i, t) in out.iter_mut().enumerate() {
                let nibble = (b[i / 2] >> ((i % 2) * 4)) & 0xF;
                t[3] = nibble * 17;
            }
        }
        Format::Bc3 => {
            colour_block(&b[8..16], false, &mut out);
            let a = unorm_channel(&b[0..8]);
            for (t, a) in out.iter_mut().zip(a) {
                t[3] = a;
            }
        }
        Format::Bc4 => {
            for (t, v) in out.iter_mut().zip(unorm_channel(b)) {
                *t = [v, v, v, 255];
            }
        }
        Format::Bc5 => {
            let x = snorm_channel(&b[0..8]);
            let y = snorm_channel(&b[8..16]);
            for i in 0..16 {
                out[i] = [unit_to_u8(x[i]), unit_to_u8(y[i]), unit_to_u8(rebuild_z(x[i], y[i])), 255];
            }
        }
        _ => {}
    }
    out
}

fn rgb565(v: u16) -> [u8; 3] {
    let r = ((v >> 11) & 31) as u32;
    let g = ((v >> 5) & 63) as u32;
    let b = (v & 31) as u32;
    [((r * 527 + 23) >> 6) as u8, ((g * 259 + 33) >> 6) as u8, ((b * 527 + 23) >> 6) as u8]
}

/// A BC1-style colour block. `allow_three` enables the 3-colour mode with
/// transparent black (BC1 only; BC2/BC3 always use four colours).
fn colour_block(b: &[u8], allow_three: bool, out: &mut [[u8; 4]; 16]) {
    let c0 = u16::from_le_bytes([b[0], b[1]]);
    let c1 = u16::from_le_bytes([b[2], b[3]]);
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u32, wb: u32, d: u32| ((a as u32 * wa + b as u32 * wb) / d) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || !allow_three {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 2, 1, 3);
            pal[3][k] = mix(p0[k], p1[k], 1, 2, 3);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 1, 1, 2);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    for (i, t) in out.iter_mut().enumerate() {
        *t = pal[((bits >> (2 * i)) & 3) as usize];
    }
}

fn indices48(b: &[u8]) -> u64 {
    let mut v = 0u64;
    for (k, &byte) in b[2..8].iter().enumerate() {
        v |= (byte as u64) << (8 * k);
    }
    v
}

/// A BC4 / BC3-alpha block, unsigned.
fn unorm_channel(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let mut pal = [0u32; 8];
    pal[0] = a0;
    pal[1] = a1;
    if a0 > a1 {
        for k in 1..7 {
            pal[k + 1] = ((7 - k as u32) * a0 + k as u32 * a1) / 7;
        }
    } else {
        for k in 1..5 {
            pal[k + 1] = ((5 - k as u32) * a0 + k as u32 * a1) / 5;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let bits = indices48(b);
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (3 * i)) & 7) as usize] as u8;
    }
    out
}

/// A signed BC4 block (one channel of BC5_SNORM), values -1..1.
fn snorm_channel(b: &[u8]) -> [f32; 16] {
    let (a0, a1) = (snorm8(b[0]), snorm8(b[1]));
    let mut pal = [0f32; 8];
    pal[0] = a0;
    pal[1] = a1;
    if (b[0] as i8) > (b[1] as i8) {
        for k in 1..7 {
            pal[k + 1] = ((7 - k) as f32 * a0 + k as f32 * a1) / 7.0;
        }
    } else {
        for k in 1..5 {
            pal[k + 1] = ((5 - k) as f32 * a0 + k as f32 * a1) / 5.0;
        }
        pal[6] = -1.0;
        pal[7] = 1.0;
    }
    let bits = indices48(b);
    let mut out = [0f32; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = pal[((bits >> (3 * i)) & 7) as usize];
    }
    out
}

fn snorm8(v: u8) -> f32 {
    ((v as i8) as f32 / 127.0).max(-1.0)
}

fn unit_to_u8(v: f32) -> u8 {
    ((v.clamp(-1.0, 1.0) * 0.5 + 0.5) * 255.0 + 0.5) as u8
}

fn rebuild_z(x: f32, y: f32) -> f32 {
    (1.0 - x * x - y * y).max(0.0).sqrt()
}

fn half_to_f32(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = ((h >> 10) & 0x1F) as i32;
    let man = (h & 0x3FF) as f32;
    sign * match exp {
        0 => man * 2f32.powi(-24),
        31 => {
            if man == 0.0 {
                f32::INFINITY
            } else {
                f32::NAN
            }
        }
        e => (1.0 + man / 1024.0) * 2f32.powi(e - 15),
    }
}

/// The mean of each RGBA channel over a decoded image, 0..1.
pub fn channel_means(rgba: &[u8]) -> [f32; 4] {
    let mut sum = [0u64; 4];
    let n = rgba.len() / 4;
    for px in rgba.as_chunks::<4>().0 {
        for k in 0..4 {
            sum[k] += px[k] as u64;
        }
    }
    if n == 0 {
        return [0.0; 4];
    }
    sum.map(|s| s as f32 / n as f32 / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bc1_block_from_mountains_snow() {
        // bitmap.md section 8: the first block of mountains_snow.
        let b = [0x55, 0x84, 0x4e, 0x63, 0x2d, 0xa9, 0x2b, 0x2b];
        let px = decode_level(Format::Bc1, 4, 4, &b).unwrap();
        // Indices row 0: 1 3 2 0; colour 0 = (132,138,173), 1 = (99,105,115).
        assert_eq!(&px[0..3], &[99, 105, 115]);
        assert_eq!(&px[12..15], &[132, 138, 173]);
        assert_eq!(px[3], 255);
    }

    #[test]
    fn bc3_alpha_from_cust_base_floor() {
        let b = [0x00, 0x1b, 0x49, 0x42, 0x86, 0xdb, 0x24, 0xc0, 0xc7, 0x31, 0x41, 0x08, 0x00, 0x8a, 0xff, 0x57];
        let px = decode_level(Format::Bc3, 4, 4, &b).unwrap();
        let alphas: Vec<u8> = px.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
        // bitmap.md: 27 27 27 27 16 16 27 16 10 10 10 5 5 0 0 0 (rounded
        // differently by one here and there).
        let want = [27, 27, 27, 27, 16, 16, 27, 16, 10, 10, 10, 5, 5, 0, 0, 0];
        for (g, w) in alphas.iter().zip(want) {
            assert!((*g as i32 - w).abs() <= 1, "{alphas:?}");
        }
    }

    #[test]
    fn bc5_zero_block_is_a_flat_normal() {
        let px = decode_level(Format::Bc5, 4, 4, &[0u8; 16]).unwrap();
        assert_eq!(&px[0..4], &[128, 128, 255, 255]);
    }

    #[test]
    fn partial_blocks_and_short_data() {
        // A 2x2 BC1 level is one block; only four texels come out.
        assert_eq!(decode_level(Format::Bc1, 2, 2, &[0u8; 8]).unwrap().len(), 16);
        assert!(decode_level(Format::Bc3, 8, 8, &[0u8; 16]).is_err());
        assert_eq!(decode_level(Format::A8R8G8B8, 1, 1, &[0x40, 0x40, 0x40, 0xff]).unwrap(), [64, 64, 64, 255]);
    }

    #[test]
    fn halves() {
        assert_eq!(half_to_f32(0x3C00), 1.0);
        assert_eq!(half_to_f32(0x3800), 0.5);
        assert_eq!(half_to_f32(0), 0.0);
    }
}
