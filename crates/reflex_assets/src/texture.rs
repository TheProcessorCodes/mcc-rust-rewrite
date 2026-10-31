//! `.texture`: a header (`00 CC DD 77`; 0x28 mip count, 0x2C width, 0x30
//! height, 0x34 format: FourCC `DXT1`/`DXT3`/`DXT5` or a D3DFMT number), a
//! mip table at 0x38 (`{u32 size, u32 pitch}` each), and the mips from
//! `max(0x80, 0x38 + 8 · mips)`. Only the top mip is decoded here.

use crate::Error;

/// Top mip, 8-bit RGBA, rows top to bottom.
#[derive(Clone)]
pub struct Rgba {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<[u8; 4]>,
}

fn u32_at(b: &[u8], p: usize) -> Result<usize, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
        .ok_or_else(|| Error::Format(format!("texture ends at {p}")))
}

pub fn read(d: &[u8]) -> Result<Rgba, Error> {
    if u32_at(d, 0)? != 0x77DD_CC00 {
        return Err(Error::Format("not a .texture".into()));
    }
    let (mips, w, h) = (u32_at(d, 0x28)?, u32_at(d, 0x2C)?, u32_at(d, 0x30)?);
    let format = d.get(0x34..0x38).ok_or_else(|| Error::Format("texture format".into()))?;
    let start = 0x80.max(0x38 + 8 * mips);
    let size = u32_at(d, 0x38)?;
    let top = d.get(start..(start + size).min(d.len())).unwrap_or_default();
    let mut pixels = vec![[0u8; 4]; w * h];
    match format {
        b"DXT1" | b"DXT3" | b"DXT5" => {
            let block = if format == b"DXT1" { 8 } else { 16 };
            let (bw, bh) = (w.div_ceil(4), h.div_ceil(4));
            for by in 0..bh {
                for bx in 0..bw {
                    let at = (by * bw + bx) * block;
                    let Some(b) = top.get(at..at + block) else {
                        continue;
                    };
                    let texels = match format {
                        b"DXT1" => bc1(b, true),
                        b"DXT3" => {
                            let mut t = bc1(&b[8..], false);
                            for (i, p) in t.iter_mut().enumerate() {
                                p[3] = ((b[i / 2] >> (4 * (i % 2))) & 0xF) * 17;
                            }
                            t
                        }
                        _ => {
                            let mut t = bc1(&b[8..], false);
                            let a = bc4(&b[..8]);
                            for (p, a) in t.iter_mut().zip(a) {
                                p[3] = a;
                            }
                            t
                        }
                    };
                    for (i, t) in texels.iter().enumerate() {
                        let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                        if x < w && y < h {
                            pixels[y * w + x] = *t;
                        }
                    }
                }
            }
        }
        // A8R8G8B8 and X8R8G8B8: BGRA in memory.
        [0x15, 0, 0, 0] | [0x16, 0, 0, 0] => {
            let opaque = format[0] == 0x16;
            for (p, s) in pixels.iter_mut().zip(top.chunks_exact(4)) {
                *p = [s[2], s[1], s[0], if opaque { 255 } else { s[3] }];
            }
        }
        _ => return Err(Error::Format(format!("texture format {format:?}"))),
    }
    Ok(Rgba {
        width: w as u32,
        height: h as u32,
        pixels,
    })
}

fn rgb565(c: u16) -> [u8; 3] {
    let r = ((c >> 11) & 31) as u32;
    let g = ((c >> 5) & 63) as u32;
    let b = (c & 31) as u32;
    [((r * 527 + 23) >> 6) as u8, ((g * 259 + 33) >> 6) as u8, ((b * 527 + 23) >> 6) as u8]
}

/// A BC1 colour block; `punch` allows the three-colour mode's transparent black.
fn bc1(b: &[u8], punch: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (a, z) = (rgb565(c0), rgb565(c1));
    let mix = |i: usize, wa: u32, wb: u32, d: u32| ((a[i] as u32 * wa + z[i] as u32 * wb) / d) as u8;
    let mut colours = [[a[0], a[1], a[2], 255], [z[0], z[1], z[2], 255], [0; 4], [0; 4]];
    if c0 > c1 || !punch {
        colours[2] = [mix(0, 2, 1, 3), mix(1, 2, 1, 3), mix(2, 2, 1, 3), 255];
        colours[3] = [mix(0, 1, 2, 3), mix(1, 1, 2, 3), mix(2, 1, 2, 3), 255];
    } else {
        colours[2] = [mix(0, 1, 1, 2), mix(1, 1, 1, 2), mix(2, 1, 1, 2), 255];
        colours[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| colours[((bits >> (2 * i)) & 3) as usize])
}

/// A BC4 (DXT5 alpha) block.
fn bc4(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (b[0] as u32, b[1] as u32);
    let mut table = [0u32; 8];
    table[0] = a0;
    table[1] = a1;
    if a0 > a1 {
        for i in 1..7 {
            table[i + 1] = ((7 - i as u32) * a0 + i as u32 * a1) / 7;
        }
    } else {
        for i in 1..5 {
            table[i + 1] = ((5 - i as u32) * a0 + i as u32 * a1) / 5;
        }
        table[6] = 0;
        table[7] = 255;
    }
    let mut bits = 0u64;
    for (i, &x) in b[2..8].iter().enumerate() {
        bits |= (x as u64) << (8 * i);
    }
    std::array::from_fn(|i| table[((bits >> (3 * i)) & 7) as usize] as u8)
}
