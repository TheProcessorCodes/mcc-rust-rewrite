//! Halo 2 bitmaps (`bitm`) decoded into the same [`Texture`] the Halo 3
//! reader makes. The pixels live in `textures.dat`; MCC stores them as
//! plain little-endian rows (or BCn blocks), mip 0 first, not swizzled.
//! Follows `docs/format/halo2.md` section 9.

use blam_texture::{Format, Kind, Texture};

use crate::cache::Map;
use crate::{Error, Result};

/// One image of a bitmap tag, as stored (`0xA8` bytes per image).
#[derive(Clone, Copy, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub kind: u16,
    pub format: u16,
    pub flags: u16,
    /// Levels after the first.
    pub mipmaps: u32,
    pub pointer: u32,
    pub compressed_size: u32,
}

/// The Halo 2 format names, by value.
pub fn format_name(f: u16) -> &'static str {
    match f {
        0 => "A8",
        1 => "Y8",
        2 => "AY8",
        3 => "A8Y8",
        6 => "R5G6B5",
        8 => "A1R5G5B5",
        9 => "A4R4G4B4",
        10 => "X8R8G8B8",
        11 => "A8R8G8B8",
        14 => "DXT1",
        15 => "DXT3",
        16 => "DXT5",
        17 => "P8_bump",
        18 => "P8",
        22 => "U8V8",
        _ => "?",
    }
}

/// The images of a bitmap tag.
pub fn images(map: &Map, bitmap: u32) -> Result<Vec<Image>> {
    let t = map
        .tag_data(bitmap)
        .ok_or_else(|| Error::Invalid(format!("bitmap {bitmap:#x} has no data")))?;
    t.block(0x44, 0xA8)?
        .into_iter()
        .map(|b| {
            Ok(Image {
                width: b.u16(0x4)? as u32,
                height: b.u16(0x6)? as u32,
                depth: b.u8(0x8)? as u32,
                kind: b.u16(0xA)?,
                format: b.u16(0xC)?,
                flags: b.u16(0xE)?,
                mipmaps: b.u16(0x14)?.min(15) as u32,
                pointer: b.u32(0x38)?,
                compressed_size: b.u32(0x50)?,
            })
        })
        .collect()
}

/// Decodes image `index` of a bitmap tag: the whole mip chain of a 2D
/// image. 16-bit and luminance-alpha formats the Halo 3 path does not
/// know are widened to `A8R8G8B8`.
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
    let (stored, widen): (Format, Option<fn(&[u8]) -> [u8; 4]>) = match im.format {
        0 => (Format::A8, None),
        1 => (Format::Y8, None),
        9 => (Format::A4R4G4B4, None),
        10 => (Format::X8R8G8B8, None),
        11 => (Format::A8R8G8B8, None),
        14 => (Format::Bc1, None),
        15 => (Format::Bc2, None),
        16 => (Format::Bc3, None),
        22 => (Format::U8V8, None),
        // Output bytes are B G R A, the A8R8G8B8 order.
        2 => (Format::Y8, Some(|p| [p[0], p[0], p[0], p[0]])),
        3 => (Format::A4R4G4B4, Some(|p| [p[0], p[0], p[0], p[1]])),
        6 => (Format::A4R4G4B4, Some(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
            [(b * 255 / 31) as u8, (g * 255 / 63) as u8, (r * 255 / 31) as u8, 255]
        })),
        8 => (Format::A4R4G4B4, Some(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let (a, r, g, b) = (v >> 15, (v >> 10) & 31, (v >> 5) & 31, v & 31);
            [(b * 255 / 31) as u8, (g * 255 / 31) as u8, (r * 255 / 31) as u8, if a != 0 { 255 } else { 0 }]
        })),
        f => return Err(Error::Unsupported(format!("{name}: format {} ({f})", format_name(f)))),
    };
    if im.flags & 0x8 != 0 && !stored.is_block_compressed() {
        return Err(Error::Unsupported(format!("{name}: swizzled")));
    }
    let data = map.texture_data(im.pointer)?;
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
