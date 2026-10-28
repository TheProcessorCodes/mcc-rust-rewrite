//! `bitm` tags and their pixel data: tag -> bitmap block -> resource ->
//! definition -> one or two pages -> mip levels. Follows
//! `docs/format/bitmap.md` (sections 2 to 7).

use blam_cache::{CacheFile, ResourceEntry, ResourceId, Resources};

use crate::format::{Format, max_levels};
use crate::{Error, Result};

/// The resource type every bitmap's pixels are stored as. Type indices
/// differ between maps, so the name is what is compared.
pub const BITMAP_RESOURCE_TYPE: &str = "bitmap_texture_interop_resource";

const BITMAP_BLOCK_SIZE: usize = 56;
const DEFINITION_SIZE: usize = 56;

/// What a bitmap is: a flat image, a volume, a cube map or a stack of
/// flat images.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    D2,
    D3,
    Cube,
    Array,
}

impl Kind {
    pub fn from_halo(value: u16) -> Option<Kind> {
        Some(match value {
            0 => Kind::D2,
            1 => Kind::D3,
            2 => Kind::Cube,
            3 => Kind::Array,
            _ => return None,
        })
    }
}

/// One entry of a bitmap tag's `Bitmaps` block (56 bytes), as stored.
#[derive(Clone, Copy, Debug)]
pub struct BitmapHeader {
    pub width: u16,
    pub height: u16,
    pub depth: u8,
    pub flags: u8,
    pub kind: u16,
    pub format: u16,
    /// Levels below the base: 0 means mip 0 only.
    pub mipmap_count: u8,
    /// Colour curve: 0 unknown, 1 xRGB, 2 gamma 2.0, 3 linear, 4 offset log,
    /// 5 sRGB.
    pub curve: u8,
}

/// A bitmap tag's blocks.
#[derive(Clone, Debug)]
pub struct BitmapTag {
    pub tag_index: u32,
    pub name: String,
    pub bitmaps: Vec<BitmapHeader>,
    /// One resource per bitmap, same index.
    pub resources: Vec<ResourceId>,
    pub interleaved_count: u32,
}

/// The texture description at the start of a bitmap resource's
/// definition (`bitmap.md` 4.1).
#[derive(Clone, Copy, Debug)]
pub struct Definition {
    /// Bytes in the primary page: the whole chain, or mips 1.. when mip 0
    /// is in the secondary page.
    pub primary_size: u32,
    /// Bytes of mip 0 (all faces) in the secondary page, or 0.
    pub secondary_size: u32,
    pub width: u16,
    pub height: u16,
    pub depth: u8,
    /// Levels including the base.
    pub levels: u8,
    pub kind: u8,
    /// Bit 0: mip 0 lives in the secondary page.
    pub flags: u8,
    /// The DXGI format the engine creates; already says sRGB or not.
    pub dxgi: i32,
    pub format: u8,
    pub curve: u8,
}

impl Definition {
    pub fn parse(bytes: &[u8]) -> Result<Definition> {
        if bytes.len() < DEFINITION_SIZE {
            return Err(Error::Invalid(format!(
                "bitmap definition is {} bytes, want at least {DEFINITION_SIZE}",
                bytes.len()
            )));
        }
        let u32_at = |o: usize| u32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
        let u16_at = |o: usize| u16::from_le_bytes([bytes[o], bytes[o + 1]]);
        Ok(Definition {
            primary_size: u32_at(0),
            secondary_size: u32_at(0x14),
            width: u16_at(0x28),
            height: u16_at(0x2A),
            depth: bytes[0x2C],
            levels: bytes[0x2D],
            kind: bytes[0x2E],
            flags: bytes[0x2F],
            dxgi: u32_at(0x30) as i32,
            format: bytes[0x34],
            curve: bytes[0x35],
        })
    }

    pub fn has_secondary(&self) -> bool {
        self.flags & 1 != 0
    }
}

/// Where one page of a bitmap is.
#[derive(Clone, Debug)]
pub struct PageLocation {
    /// `None` for this map, else the shared cache's file name.
    pub file: Option<String>,
    pub page: u16,
    pub offset: i32,
    /// Bytes of this bitmap in the page.
    pub size: u32,
    pub page_compressed: u32,
    pub page_decompressed: u32,
}

/// The resource behind one bitmap of a tag.
#[derive(Clone, Debug)]
pub struct Located {
    pub resource: ResourceId,
    pub entry: u32,
    pub segment: u16,
    pub definition: Definition,
    pub primary: PageLocation,
    pub secondary: Option<PageLocation>,
}

/// A decoded bitmap: every level, largest first.
#[derive(Clone, Debug)]
pub struct Texture {
    /// The bitmap tag's path.
    pub name: String,
    pub tag_index: u32,
    /// Which bitmap of the tag (usually 0).
    pub bitmap_index: u32,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    /// Faces of a cube (6), slices of an array or volume, else 1.
    pub layers: u32,
    pub format: Format,
    /// The engine samples it as sRGB (the definition's DXGI format).
    pub srgb: bool,
    pub curve: u8,
    /// `mips[l]` holds level `l` of every layer, layer after layer, each
    /// `format.level_size(width, height, l)` bytes.
    pub mips: Vec<Vec<u8>>,
}

impl Texture {
    pub fn level_dims(&self, level: u32) -> (u32, u32) {
        Format::level_dims(self.width, self.height, level)
    }

    /// One layer of one level.
    pub fn level(&self, level: u32, layer: u32) -> Option<&[u8]> {
        let size = self.format.level_size(self.width, self.height, level) as usize;
        let start = size.checked_mul(layer as usize)?;
        self.mips.get(level as usize)?.get(start..start.checked_add(size)?)
    }

    /// Bytes held, all levels and layers.
    pub fn byte_len(&self) -> usize {
        self.mips.iter().map(Vec::len).sum()
    }
}

fn tag_name(cache: &CacheFile, tag_index: u32) -> Result<(String, &str)> {
    let tag = cache
        .tags
        .get(tag_index)
        .ok_or_else(|| Error::Invalid(format!("no tag #{tag_index}")))?;
    Ok((tag.name().to_string(), cache.tags.class_of(tag).code.as_str()))
}

/// Reads a bitmap tag's header blocks.
pub fn read_tag(cache: &CacheFile, tag_index: u32) -> Result<BitmapTag> {
    let (name, class) = tag_name(cache, tag_index)?;
    if class != "bitm" {
        return Err(Error::Invalid(format!("{name} is a {class} tag, not bitm")));
    }
    let tag = cache.tags.get(tag_index).expect("checked above");
    let base = cache.tag_offset(tag)?;
    let mut r = cache.reader_at(base + 0x60);
    let bitmaps_block = r.block()?;
    let resources_block = r.at(base + 0x8C).block()?;
    let interleaved = r.at(base + 0x98).block()?;

    let bitmaps = bitmaps_block
        .elements(BITMAP_BLOCK_SIZE)
        .map(|o| {
            let mut r = cache.reader_at(o + 4);
            Ok(BitmapHeader {
                width: r.u16()?,
                height: r.u16()?,
                depth: r.u8()?,
                flags: r.u8()?,
                kind: r.u16()?,
                format: r.u16()?,
                mipmap_count: r.at(o + 0x14).u8()?,
                curve: r.at(o + 0x15).u8()?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let resources = resources_block
        .elements(8)
        .map(|o| Ok(ResourceId(cache.reader_at(o).u32()?)))
        .collect::<Result<Vec<_>>>()?;
    Ok(BitmapTag {
        tag_index,
        name,
        bitmaps,
        resources,
        interleaved_count: interleaved.count,
    })
}

/// The live resource entry of a bitmap, or an error for wiped, foreign or
/// stale ids.
fn bitmap_entry(resources: &Resources, id: ResourceId) -> Result<ResourceEntry> {
    if id.is_null() {
        return Err(Error::NoData("null resource id".into()));
    }
    let entry = resources.entry(id)?;
    if entry.is_empty() {
        return Err(Error::NoData(format!("resource {:#010x} is a wiped entry", id.0)));
    }
    if entry.salt != (id.0 >> 16) as u16 {
        return Err(Error::NoData(format!(
            "resource {:#010x}: entry salt {:#06x} does not match",
            id.0, entry.salt
        )));
    }
    match resources.type_name(entry.type_index) {
        Some(BITMAP_RESOURCE_TYPE) => Ok(entry),
        other => Err(Error::Invalid(format!(
            "resource {:#010x} is a {} resource, not a bitmap",
            id.0,
            other.unwrap_or("nameless")
        ))),
    }
}

fn page_location(
    resources: &Resources,
    page: u16,
    offset: i32,
    size: u32,
) -> Result<PageLocation> {
    let info = resources
        .page_info(page)
        .ok_or_else(|| Error::Invalid(format!("page {page} of {}", resources.pages.len())))?;
    let file = info.cache_index.map(|i| {
        resources
            .shared_caches
            .get(i as usize)
            .cloned()
            .unwrap_or_else(|| format!("shared cache {i}"))
    });
    Ok(PageLocation {
        file,
        page,
        offset,
        size,
        page_compressed: info.compressed_size,
        page_decompressed: info.decompressed_size,
    })
}

/// Finds where bitmap `index` of a tag keeps its pixels, without reading them.
pub fn locate(resources: &Resources, tag: &BitmapTag, index: u32) -> Result<Located> {
    if tag.interleaved_count != 0 {
        return Err(Error::Unsupported(format!(
            "{} uses interleaved resources (no installed map does)",
            tag.name
        )));
    }
    let id = *tag.resources.get(index as usize).ok_or_else(|| {
        Error::Invalid(format!(
            "{} has {} resources, no bitmap {index}",
            tag.name,
            tag.resources.len()
        ))
    })?;
    let entry = bitmap_entry(resources, id)?;
    let definition = Definition::parse(resources.definition(&entry)?)?;
    let segment_index = entry
        .segment
        .ok_or_else(|| Error::NoData(format!("{}: resource has no segment", tag.name)))?;
    let segment = resources.segment_of(&entry)?;
    let primary_page = segment
        .primary_page
        .ok_or_else(|| Error::NoData(format!("{}: segment has no primary page", tag.name)))?;
    let primary = page_location(
        resources,
        primary_page,
        segment.primary_offset,
        definition.primary_size,
    )?;
    let secondary = match (definition.has_secondary(), segment.secondary_page) {
        (true, Some(page)) => Some(page_location(
            resources,
            page,
            segment.secondary_offset,
            definition.secondary_size,
        )?),
        (true, None) => {
            return Err(Error::Invalid(format!(
                "{}: definition says mip 0 is in a secondary page but the segment has none",
                tag.name
            )));
        }
        (false, _) => None,
    };
    Ok(Located {
        resource: id,
        entry: entry.index,
        segment: segment_index,
        definition,
        primary,
        secondary,
    })
}

/// The checked shape of a bitmap: everything needed to cut its pages into
/// levels.
struct Shape {
    kind: Kind,
    format: Format,
    srgb: bool,
    width: u32,
    height: u32,
    levels: u32,
    layers: u32,
}

fn shape(name: &str, header: &BitmapHeader, def: &Definition) -> Result<Shape> {
    let bad = |what: String| Error::Invalid(format!("{name}: {what}"));
    if def.width != header.width || def.height != header.height {
        return Err(bad(format!(
            "definition says {}x{}, tag says {}x{}",
            def.width, def.height, header.width, header.height
        )));
    }
    if def.format as u16 != header.format {
        return Err(bad(format!(
            "definition format {} differs from the tag's {}",
            def.format, header.format
        )));
    }
    let format = Format::from_halo(header.format)
        .ok_or_else(|| Error::Unsupported(format!("{name}: bitmap format {}", header.format)))?;
    let srgb = format.check_dxgi(def.dxgi).ok_or_else(|| {
        bad(format!("DXGI format {} does not fit {}", def.dxgi, format.halo_name()))
    })?;
    let kind = Kind::from_halo(header.kind)
        .ok_or_else(|| bad(format!("bitmap type {}", header.kind)))?;
    let (width, height) = (def.width as u32, def.height as u32);
    if width == 0 || height == 0 {
        return Err(bad("zero-sized bitmap".into()));
    }
    let levels = def.levels as u32;
    if levels == 0 || levels > max_levels(width, height) {
        return Err(bad(format!("{levels} levels for {width}x{height}")));
    }
    if levels != header.mipmap_count as u32 + 1 {
        return Err(bad(format!(
            "definition has {levels} levels, the tag {} mips",
            header.mipmap_count
        )));
    }
    let layers = match kind {
        Kind::D2 => 1,
        Kind::Cube => 6,
        Kind::Array => def.depth.max(1) as u32,
        Kind::D3 => {
            return Err(Error::Unsupported(format!(
                "{name}: 3D bitmaps (no installed map has one)"
            )));
        }
    };
    Ok(Shape {
        kind,
        format,
        srgb,
        width,
        height,
        levels,
        layers,
    })
}

fn slice<'d>(data: &'d [u8], start: u64, len: u64, what: &str, name: &str) -> Result<&'d [u8]> {
    let end = start.checked_add(len);
    match end {
        Some(end) if end <= data.len() as u64 => Ok(&data[start as usize..end as usize]),
        _ => Err(Error::Invalid(format!(
            "{name}: {what} needs bytes {start}..{} but the page holds {}",
            start.saturating_add(len),
            data.len()
        ))),
    }
}

/// Decodes bitmap `index` of a `bitm` tag into its levels.
pub fn decode(cache: &CacheFile, resources: &Resources, tag_index: u32, index: u32) -> Result<Texture> {
    let tag = read_tag(cache, tag_index)?;
    decode_from(resources, &tag, index)
}

/// [`decode`] for a tag already read.
pub fn decode_from(resources: &Resources, tag: &BitmapTag, index: u32) -> Result<Texture> {
    let name = tag.name.as_str();
    let header = tag
        .bitmaps
        .get(index as usize)
        .ok_or_else(|| Error::Invalid(format!("{name} has {} bitmaps, no {index}", tag.bitmaps.len())))?;
    let located = locate(resources, tag, index)?;
    let def = &located.definition;
    let s = shape(name, header, def)?;
    let f = s.format;
    let layers = s.layers as u64;
    let level = |l: u32| f.level_size(s.width, s.height, l);
    let chain = f.levels_size(s.width, s.height, 0, s.levels);

    // The sizes the definition states must be exactly what the shape needs
    // (bitmap.md 11.4); anything else means the layout is not understood.
    let (want_primary, want_secondary) = match (s.kind, def.has_secondary()) {
        (Kind::Array, true) => {
            return Err(Error::Unsupported(format!(
                "{name}: an array with a secondary page (no installed map has one)"
            )));
        }
        (_, true) => (layers * (chain - level(0)), layers * level(0)),
        (_, false) => (layers * chain, 0),
    };
    if def.primary_size as u64 != want_primary || def.secondary_size as u64 != want_secondary {
        return Err(Error::Invalid(format!(
            "{name}: definition sizes {:#x} / {:#x}, the {}x{}x{} {} chain of {} levels needs {want_primary:#x} / {want_secondary:#x}",
            def.primary_size,
            def.secondary_size,
            s.width,
            s.height,
            s.layers,
            f.halo_name(),
            s.levels
        )));
    }

    let primary_data = resources.page_data(located.primary.page, located.primary.offset)?;
    let primary = slice(primary_data.bytes(), 0, want_primary, "the primary data", name)?;
    let secondary_data = match &located.secondary {
        Some(p) => Some(resources.page_data(p.page, p.offset)?),
        None => None,
    };
    let secondary = match &secondary_data {
        Some(d) => slice(d.bytes(), 0, want_secondary, "mip 0", name)?,
        None => &[][..],
    };

    let mut mips: Vec<Vec<u8>> = Vec::with_capacity(s.levels as usize);
    match s.kind {
        // Slice-major: each slice's whole chain in turn (bitmap.md 6.4).
        Kind::Array => {
            for l in 0..s.levels {
                let size = level(l);
                let before = f.levels_size(s.width, s.height, 0, l);
                let mut out = Vec::with_capacity((size * layers) as usize);
                for slice_index in 0..layers {
                    let start = slice_index * chain + before;
                    out.extend_from_slice(slice(primary, start, size, "an array slice", name)?);
                }
                mips.push(out);
            }
        }
        // Mip-major: every face of level 0, then every face of level 1 ...
        // With a secondary page, level 0 is all of it and the primary page
        // starts at level 1 (bitmap.md 6.1, 6.3).
        _ => {
            let mut start = 0u64;
            for l in 0..s.levels {
                let len = level(l) * layers;
                let bytes = if l == 0 && def.has_secondary() {
                    slice(secondary, 0, len, "mip 0", name)?
                } else {
                    let b = slice(primary, start, len, "a mip level", name)?;
                    start += len;
                    b
                };
                mips.push(bytes.to_vec());
            }
        }
    }

    Ok(Texture {
        name: tag.name.clone(),
        tag_index: tag.tag_index,
        bitmap_index: index,
        kind: s.kind,
        width: s.width,
        height: s.height,
        layers: s.layers,
        format: f,
        srgb: s.srgb,
        curve: def.curve,
        mips,
    })
}

/// Name of a colour curve value.
pub fn curve_name(curve: u8) -> &'static str {
    match curve {
        0 => "unknown",
        1 => "xRGB",
        2 => "gamma 2.0",
        3 => "linear",
        4 => "offset log",
        5 => "sRGB",
        _ => "?",
    }
}
