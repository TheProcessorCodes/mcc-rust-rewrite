//! Halo CE bitmaps (`bitm`) decoded into the same [`Texture`] the other
//! readers make. Most pixels live in `bitmaps.map` beside the map; the
//! lightmaps' are in the map itself. MCC stores them as plain
//! little-endian rows (or BCn blocks), mip 0 first, not swizzled.
//! Follows `docs/format/halo1.md` section 8.

use blam_texture::{Format, Kind, Texture};

use crate::cache::Map;
use crate::{Error, Result};

/// The image's pixels are in `bitmaps.map`, at its offset there.
pub const EXTERNAL: u16 = 0x100;

/// One image of a bitmap tag, as stored (`0x30` bytes per image).
#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    /// 0 2D, 1 3D, 2 cube map, 3 white.
    pub kind: u16,
    pub format: u16,
    pub flags: u16,
    /// Levels after the first.
    pub mipmaps: u32,
    pub offset: u32,
    pub size: u32,
}

/// The format names, by value (the same numbers as Halo 2's).
pub fn format_name(f: u16) -> &'static str {
    blam_h2::bitmap::format_name(f)
}

/// The images of a bitmap tag (block at `+0x60`).
pub fn images(map: &Map, bitmap: u32) -> Result<Vec<Image>> {
    let t = map
        .tag_data(bitmap)
        .ok_or_else(|| Error::Invalid(format!("bitmap {bitmap:#x} has no data")))?;
    t.block(0x60, 0x30)?
        .into_iter()
        .map(|b| {
            Ok(Image {
                width: b.u16(0x4)? as u32,
                height: b.u16(0x6)? as u32,
                depth: b.u16(0x8)? as u32,
                kind: b.u16(0xA)?,
                format: b.u16(0xC)?,
                flags: b.u16(0xE)?,
                mipmaps: b.u16(0x14)?.min(15) as u32,
                offset: b.u32(0x18)?,
                size: b.u32(0x1C)?,
            })
        })
        .collect()
}

/// Turns one stored pixel into `B G R A` bytes.
type Widen = fn(&[u8]) -> [u8; 4];

/// R5G6B5 to `B G R A`.
fn r5g6b5(p: &[u8]) -> [u8; 4] {
    let v = u16::from_le_bytes([p[0], p[1]]);
    let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
    [(b * 255 / 31) as u8, (g * 255 / 63) as u8, (r * 255 / 31) as u8, 255]
}

/// Decodes image `index` of a bitmap tag: the whole mip chain of a 2D
/// image. 16-bit and luminance-alpha formats are widened to `A8R8G8B8`.
pub fn decode(map: &Map, bitmap: u32, index: usize) -> Result<Texture> {
    let name = map.tag_name(bitmap).unwrap_or("?").to_string();
    let im = *images(map, bitmap)?
        .get(index)
        .ok_or_else(|| Error::Invalid(format!("{name}: no image {index}")))?;
    if im.kind != 0 {
        return Err(Error::Unsupported(format!("{name}: image type {} (only 2D is drawn)", im.kind)));
    }
    if im.width == 0 || im.height == 0 || im.width > 8192 || im.height > 8192 {
        return Err(Error::Invalid(format!("{name}: {}x{}", im.width, im.height)));
    }
    let (stored, widen): (Format, Option<Widen>) = match im.format {
        0 => (Format::A8, None),
        1 => (Format::Y8, None),
        9 => (Format::A4R4G4B4, None),
        10 => (Format::X8R8G8B8, None),
        11 => (Format::A8R8G8B8, None),
        14 => (Format::Bc1, None),
        15 => (Format::Bc2, None),
        16 => (Format::Bc3, None),
        // Output bytes are B G R A, the A8R8G8B8 order. A4R4G4B4 stands in
        // for any two-byte format's size.
        2 => (Format::Y8, Some(|p| [p[0], p[0], p[0], p[0]])),
        3 => (Format::A4R4G4B4, Some(|p| [p[0], p[0], p[0], p[1]])),
        6 => (Format::A4R4G4B4, Some(r5g6b5)),
        8 => (Format::A4R4G4B4, Some(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (a, r, g, b) = (v >> 15, (v >> 10) & 31, (v >> 5) & 31, v & 31);
            [(b * 255 / 31) as u8, (g * 255 / 31) as u8, (r * 255 / 31) as u8, if a != 0 { 255 } else { 0 }]
        })),
        f => return Err(Error::Unsupported(format!("{name}: format {} ({f})", format_name(f)))),
    };
    let data: &[u8] = if im.flags & EXTERNAL != 0 {
        let file = map
            .bitmaps_file()
            .ok_or_else(|| Error::Invalid("bitmaps.map not found beside the map".into()))?;
        file.get(im.offset as usize..(im.offset as usize).saturating_add(im.size as usize))
            .ok_or_else(|| Error::Invalid(format!("{name}: pixels at {:#x} run past bitmaps.map", im.offset)))?
    } else {
        map.slice(im.offset as usize, im.size as usize)?
    };
    let mut mips = Vec::new();
    let mut at = 0usize;
    for level in 0..=im.mipmaps {
        let size = stored.level_size(im.width, im.height, level) as usize;
        let Some(bytes) = data.get(at..at + size) else { break };
        at += size;
        mips.push(match widen {
            None => bytes.to_vec(),
            Some(f) => {
                let unit = stored.unit_bytes() as usize;
                bytes.chunks_exact(unit).flat_map(f).collect()
            }
        });
        let (w, h) = Format::level_dims(im.width, im.height, level);
        if w == 1 && h == 1 {
            break;
        }
    }
    if mips.is_empty() {
        return Err(Error::Invalid(format!("{name}: {} bytes, mip 0 needs more", data.len())));
    }
    Ok(Texture {
        name,
        tag_index: bitmap & 0xFFFF,
        bitmap_index: index as u32,
        kind: Kind::D2,
        width: im.width,
        height: im.height,
        layers: 1,
        format: if widen.is_some() { Format::A8R8G8B8 } else { stored },
        srgb: false,
        curve: 0,
        mips,
    })
}
