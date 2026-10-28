//! The pixel formats MCC Halo 3 stores, and how many bytes a mip level of
//! each takes. Follows `docs/format/bitmap.md` sections 5 and 6.2.

/// A bitmap's pixel format: the eleven formats found in the installed maps
/// plus BC4, which the enum allows and costs nothing to support.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    /// One byte of alpha (DXGI A8_UNORM). Masks.
    A8,
    /// One byte of luminance (DXGI R8_UNORM).
    Y8,
    /// 16-bit `B4 G4 R4 A4`, blue in the low nibble (DXGI B4G4R4A4_UNORM).
    A4R4G4B4,
    /// 32-bit, bytes `B G R x` (DXGI B8G8R8X8_UNORM); alpha is one.
    X8R8G8B8,
    /// 32-bit, bytes `B G R A` (DXGI B8G8R8A8_UNORM).
    A8R8G8B8,
    /// DXT1: 8-byte 4x4 blocks, colour with 1-bit alpha.
    Bc1,
    /// DXT3: 16-byte blocks, colour with explicit 4-bit alpha.
    Bc2,
    /// DXT5: 16-byte blocks, colour with interpolated alpha. The common
    /// colour format.
    Bc3,
    /// DXT5a: 8-byte blocks, one channel.
    Bc4,
    /// DXN: 16-byte blocks, two **signed** channels on PC (DXGI BC5_SNORM).
    /// Every `*_bump` normal map.
    Bc5,
    /// Two signed bytes (DXGI R8G8_SNORM). Water and distortion maps.
    U8V8,
    /// Format 25: four half floats (DXGI R16G16B16A16_FLOAT). UI gradients
    /// and colour-correction tables.
    Rgba16F,
}

impl Format {
    /// The format for a value of the bitmap's format field (block `+0x0C`).
    pub fn from_halo(value: u16) -> Option<Format> {
        Some(match value {
            0 => Format::A8,
            1 => Format::Y8,
            9 => Format::A4R4G4B4,
            10 => Format::X8R8G8B8,
            11 => Format::A8R8G8B8,
            14 => Format::Bc1,
            15 => Format::Bc2,
            16 => Format::Bc3,
            22 => Format::U8V8,
            25 => Format::Rgba16F,
            31 => Format::Bc4,
            33 => Format::Bc5,
            _ => return None,
        })
    }

    /// The Halo name of the format, as the docs and tools spell it.
    pub fn halo_name(self) -> &'static str {
        match self {
            Format::A8 => "A8",
            Format::Y8 => "Y8",
            Format::A4R4G4B4 => "A4R4G4B4",
            Format::X8R8G8B8 => "X8R8G8B8",
            Format::A8R8G8B8 => "A8R8G8B8",
            Format::Bc1 => "DXT1 (BC1)",
            Format::Bc2 => "DXT3 (BC2)",
            Format::Bc3 => "DXT5 (BC3)",
            Format::Bc4 => "DXT5a (BC4)",
            Format::Bc5 => "DXN (BC5 signed)",
            Format::U8V8 => "U8V8",
            Format::Rgba16F => "RGBA FP16",
        }
    }

    /// Whether the definition's DXGI value is a plausible match for this
    /// format, and if so whether it is the sRGB variant.
    pub fn check_dxgi(self, dxgi: i32) -> Option<bool> {
        let (linear, srgb): (&[i32], &[i32]) = match self {
            Format::A8 => (&[65], &[]),
            Format::Y8 => (&[61], &[]),
            Format::A4R4G4B4 => (&[115], &[]),
            Format::X8R8G8B8 => (&[88, 87], &[93, 91]),
            Format::A8R8G8B8 => (&[87], &[91]),
            Format::Bc1 => (&[71], &[72]),
            Format::Bc2 => (&[74], &[75]),
            Format::Bc3 => (&[77], &[78]),
            Format::Bc4 => (&[80], &[]),
            Format::Bc5 => (&[84, 83], &[]),
            Format::U8V8 => (&[51], &[]),
            Format::Rgba16F => (&[10], &[]),
        };
        if linear.contains(&dxgi) {
            Some(false)
        } else if srgb.contains(&dxgi) {
            Some(true)
        } else {
            None
        }
    }

    pub fn is_block_compressed(self) -> bool {
        matches!(
            self,
            Format::Bc1 | Format::Bc2 | Format::Bc3 | Format::Bc4 | Format::Bc5
        )
    }

    /// Bytes per 4x4 block for BCn, bytes per pixel otherwise.
    pub fn unit_bytes(self) -> u32 {
        match self {
            Format::Bc1 | Format::Bc4 => 8,
            Format::Bc2 | Format::Bc3 | Format::Bc5 => 16,
            Format::A8 | Format::Y8 => 1,
            Format::A4R4G4B4 | Format::U8V8 => 2,
            Format::X8R8G8B8 | Format::A8R8G8B8 => 4,
            Format::Rgba16F => 8,
        }
    }

    /// Width and height of one level: `max(1, size >> level)`.
    pub fn level_dims(width: u32, height: u32, level: u32) -> (u32, u32) {
        let shift = |v: u32| v.checked_shr(level).unwrap_or(0).max(1);
        (shift(width), shift(height))
    }

    /// Bytes of one level of one face: whole 4x4 blocks for BCn, tight
    /// pixel rows otherwise, no padding.
    pub fn level_size(self, width: u32, height: u32, level: u32) -> u64 {
        let (w, h) = Format::level_dims(width, height, level);
        let unit = self.unit_bytes() as u64;
        if self.is_block_compressed() {
            w.div_ceil(4) as u64 * h.div_ceil(4) as u64 * unit
        } else {
            w as u64 * h as u64 * unit
        }
    }

    /// Bytes of levels `from..to` of one face.
    pub fn levels_size(self, width: u32, height: u32, from: u32, to: u32) -> u64 {
        (from..to).map(|l| self.level_size(width, height, l)).sum()
    }

    /// Bytes per row of one level (block rows for BCn).
    pub fn row_bytes(self, width: u32, level: u32) -> u64 {
        let (w, _) = Format::level_dims(width, 1, level);
        let unit = self.unit_bytes() as u64;
        if self.is_block_compressed() {
            w.div_ceil(4) as u64 * unit
        } else {
            w as u64 * unit
        }
    }
}

/// The most mip levels a `width x height` image can have (down to 1x1).
pub fn max_levels(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_and_pixel_sizes() {
        assert_eq!(Format::Bc1.unit_bytes(), 8);
        assert_eq!(Format::Bc4.unit_bytes(), 8);
        assert_eq!(Format::Bc2.unit_bytes(), 16);
        assert_eq!(Format::Bc3.unit_bytes(), 16);
        assert_eq!(Format::Bc5.unit_bytes(), 16);
        assert_eq!(Format::A8R8G8B8.unit_bytes(), 4);
        assert_eq!(Format::Rgba16F.unit_bytes(), 8);
        // A 1024x512 DXT1 mip 0 is 256 x 128 blocks of 8 bytes.
        assert_eq!(Format::Bc1.level_size(1024, 512, 0), 0x40000);
        // The 2x2 and 1x1 levels of a BCn texture are still one block.
        assert_eq!(Format::Bc3.level_size(1024, 1024, 9), 16);
        assert_eq!(Format::Bc3.level_size(1024, 1024, 10), 16);
        assert_eq!(Format::Bc1.level_size(8, 2, 0), 16);
        // Non-square: the short side bottoms out at 1.
        assert_eq!(Format::level_dims(1024, 512, 10), (1, 1));
        assert_eq!(Format::level_dims(1024, 512, 9), (2, 1));
        assert_eq!(Format::level_dims(406, 1, 0), (406, 1));
        assert_eq!(Format::Rgba16F.level_size(406, 1, 0), 0xCB0);
        assert_eq!(Format::Bc3.row_bytes(1024, 0), 4096);
        assert_eq!(Format::A8.row_bytes(64, 1), 32);
    }

    #[test]
    fn chain_sizes_match_the_definitions_on_riverworld() {
        // bitmap.md section 8: mountains_snow 1024x512 DXT1, 11 levels;
        // primary holds mips 1..10, secondary mip 0.
        assert_eq!(Format::Bc1.levels_size(1024, 512, 1, 11), 0x15568);
        assert_eq!(Format::Bc1.levels_size(1024, 512, 0, 1), 0x40000);
        // cust_base_floor_dif 1024x1024 DXT5, 9 levels.
        assert_eq!(Format::Bc3.levels_size(1024, 1024, 1, 9), 0x55550);
        // cliff_rock_base_brown 2048x2048 DXT5, 12 levels.
        assert_eq!(Format::Bc3.levels_size(2048, 2048, 1, 12), 0x155570);
        // default_alpha_test, 64x64 A8, 7 levels, whole chain in one page.
        assert_eq!(Format::A8.levels_size(64, 64, 0, 7), 0x1555);
        // plasma_sm_red_mask, A8 512x512, mips 1..9 (no per-level padding).
        assert_eq!(Format::A8.levels_size(512, 512, 1, 10), 0x15555);
        // The riverworld cube map: 6 faces of a 128x128 DXT5, 3 levels.
        assert_eq!(6 * Format::Bc3.levels_size(128, 128, 0, 3), 0x1F800);
        // default_dynamic_cube_map: 64x64 DXT5 cube, 7 levels.
        assert_eq!(6 * Format::Bc3.levels_size(64, 64, 0, 7), 0x80A0);
    }

    #[test]
    fn max_level_counts() {
        assert_eq!(max_levels(1, 1), 1);
        assert_eq!(max_levels(2048, 2048), 12);
        assert_eq!(max_levels(1024, 512), 11);
        assert_eq!(max_levels(0, 0), 1);
    }

    #[test]
    fn halo_values_and_dxgi() {
        assert_eq!(Format::from_halo(16), Some(Format::Bc3));
        assert_eq!(Format::from_halo(33), Some(Format::Bc5));
        assert_eq!(Format::from_halo(25), Some(Format::Rgba16F));
        assert_eq!(Format::from_halo(34), None);
        assert_eq!(Format::Bc3.check_dxgi(78), Some(true));
        assert_eq!(Format::Bc3.check_dxgi(77), Some(false));
        assert_eq!(Format::Bc3.check_dxgi(71), None);
        assert_eq!(Format::Bc5.check_dxgi(84), Some(false));
        assert_eq!(Format::A8R8G8B8.check_dxgi(91), Some(true));
    }
}
