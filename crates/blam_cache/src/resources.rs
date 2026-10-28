//! Resources: the big blobs (vertex buffers, textures, sounds) that live
//! outside tag data, in compressed pages, sometimes in another file.
//!
//! Two global tags describe them. The `zone` tag (cache_file_resource_gestalt)
//! has one entry per resource: who owns it, where its *definition* lies in
//! the gestalt's fixup data, which segment holds its bytes, and the fixups
//! that say where each buffer starts inside those bytes. The `play` tag
//! (cache_file_resource_layout_table) maps segments to pages, and pages to a
//! byte range in this map or in a shared cache such as `shared.map`. A page
//! whose compressed size differs from its decompressed size is a raw deflate
//! stream. Follows `docs/format/sbsp-render-geometry.md` section 4 and
//! `docs/format/resources.md`.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use crate::error::{Error, Result};
use crate::header::SECTION_RESOURCE;
use crate::reader::{Block, TagRef};
use crate::strings::StringId;
use crate::CacheFile;

/// A resource reference as a tag stores it: the low 16 bits index the
/// gestalt's entries, the high 16 are a salt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ResourceId(pub u32);

impl ResourceId {
    pub fn index(self) -> u32 {
        self.0 & 0xFFFF
    }
    pub fn is_null(self) -> bool {
        self.0 == 0 || self.0 == u32::MAX
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ResourceEntry {
    pub index: u32,
    pub owner: TagRef,
    pub salt: u16,
    /// Index into the gestalt's resource type list; -1 for an empty entry.
    pub type_index: i8,
    /// Byte range of the resource's definition inside the fixup data.
    pub fixup_offset: u32,
    pub fixup_size: u32,
    pub location_type: i16,
    pub segment: Option<u16>,
    /// Where the definition's root structure is, as a [`CacheAddress`].
    pub definition_address: CacheAddress,
    fixups: Block,
}

impl ResourceEntry {
    pub fn is_empty(&self) -> bool {
        self.type_index < 0
    }
}

/// An address inside a resource: three bits of kind, 29 bits of offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CacheAddress(pub u32);

impl CacheAddress {
    /// Offset into the fixed-up definition.
    pub const DEFINITION: u32 = 1;
    /// Offset into the resource's primary data.
    pub const DATA: u32 = 2;
    /// Offset into the resource's secondary (optional) data.
    pub const SECONDARY: u32 = 3;

    pub fn kind(self) -> u32 {
        self.0 >> 29
    }
    pub fn offset(self) -> usize {
        (self.0 & 0x1FFF_FFFF) as usize
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Page {
    /// `None` for this map, else an index into the shared cache list.
    pub cache_index: Option<u16>,
    pub data_offset: i32,
    pub compressed_size: u32,
    pub decompressed_size: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub primary_page: Option<u16>,
    pub secondary_page: Option<u16>,
    pub primary_offset: i32,
    pub secondary_offset: i32,
}

/// One resource's bytes: a view into a decompressed page.
#[derive(Clone)]
pub struct ResourceData {
    page: Arc<Vec<u8>>,
    start: usize,
}

impl ResourceData {
    pub fn bytes(&self) -> &[u8] {
        &self.page[self.start..]
    }
    pub fn len(&self) -> usize {
        self.page.len() - self.start
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

fn opt_index(v: i16) -> Option<u16> {
    (v >= 0).then_some(v as u16)
}

/// Decompressed pages by (cache index, page index).
type PageCache = HashMap<(Option<u16>, u16), Arc<Vec<u8>>>;

pub struct Resources<'a> {
    cache: &'a CacheFile,
    entries: Block,
    /// Names of the resource types (`zone + 4`), by type index.
    type_names: Vec<StringId>,
    fixup_data: (usize, usize),
    pub shared_caches: Vec<String>,
    pub pages: Vec<Page>,
    pub segments: Vec<Segment>,
    page_cache: Mutex<PageCache>,
    shared: Mutex<HashMap<u16, Arc<CacheFile>>>,
}

const ENTRY_SIZE: usize = 64;

impl<'a> Resources<'a> {
    pub fn new(cache: &'a CacheFile) -> Result<Resources<'a>> {
        let zone = cache
            .global_tag("zone")
            .ok_or_else(|| Error::Invalid("no zone tag (resource gestalt)".into()))?;
        let zone_off = cache.tag_offset(zone)?;
        // Resource types: 28 bytes each, the name's string id at +24
        // (`docs/format/resources.md` 3.3). The order differs per map.
        let type_names = cache
            .reader_at(zone_off + 4)
            .block()?
            .elements(28)
            .map(|o| cache.reader_at(o + 24).string_id())
            .collect::<Result<Vec<_>>>()?;
        let mut r = cache.reader_at(zone_off + 0x64);
        let entries = r.block()?;
        r.seek(zone_off + 0x148);
        let fixups = r.data_ref()?;
        let fixup_data = match fixups.offset {
            Some(o) => (o, fixups.size as usize),
            None => (0, 0),
        };

        // The layout table is its own tag, or lives inside the gestalt when
        // that tag's pointer is zero.
        let play_off = match cache.global_tag("play") {
            Some(play) if play.meta_pointer != 0 => cache.tag_offset(play)?,
            _ => zone_off + 0x1C,
        };
        let mut r = cache.reader_at(play_off + 0xC);
        let shared_block = r.block()?;
        let pages_block = r.block()?;
        r.seek(play_off + 0x3C);
        let segments_block = r.block()?;

        let shared_caches = shared_block
            .elements(264)
            .map(|o| crate::header_cstr(cache.bytes(), o, 32))
            .collect();
        let pages = pages_block
            .elements(88)
            .map(|o| {
                let mut r = cache.reader_at(o + 4);
                Ok(Page {
                    cache_index: opt_index(r.i16()?),
                    data_offset: r.at(o + 8).i32()?,
                    compressed_size: r.at(o + 0xC).u32()?,
                    decompressed_size: r.at(o + 0x10).u32()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let segments = segments_block
            .elements(16)
            .map(|o| {
                let mut r = cache.reader_at(o);
                Ok(Segment {
                    primary_page: opt_index(r.i16()?),
                    secondary_page: opt_index(r.i16()?),
                    primary_offset: r.i32()?,
                    secondary_offset: r.i32()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Resources {
            cache,
            entries,
            type_names,
            fixup_data,
            shared_caches,
            pages,
            segments,
            page_cache: Mutex::new(HashMap::new()),
            shared: Mutex::new(HashMap::new()),
        })
    }

    pub fn entry_count(&self) -> u32 {
        self.entries.count
    }

    /// The name of a resource type, e.g. `bitmap_texture_interop_resource`.
    /// Type indices differ between maps, so compare names, not numbers.
    pub fn type_name(&self, type_index: i8) -> Option<&'a str> {
        let id = *self.type_names.get(usize::try_from(type_index).ok()?)?;
        self.cache.string(id)
    }

    /// The segment a resource's bytes live in.
    pub fn segment_of(&self, entry: &ResourceEntry) -> Result<Segment> {
        let seg_index = entry.segment.ok_or_else(|| {
            Error::Invalid(format!("resource {} has no data (segment -1)", entry.index))
        })?;
        self.segments.get(seg_index as usize).copied().ok_or_else(|| {
            Error::Invalid(format!(
                "resource {} names segment {seg_index} of {}",
                entry.index,
                self.segments.len()
            ))
        })
    }

    /// The bytes of one page from `offset` on, decompressing it on first
    /// use. Unlike [`Resources::data`] this does not choose between a
    /// segment's primary and secondary page; the caller says which.
    pub fn page_data(&self, page_index: u16, offset: i32) -> Result<ResourceData> {
        let page = self.page(page_index)?;
        if offset < 0 || page.data_offset < 0 || page.compressed_size == 0 {
            return Err(Error::Invalid(format!("page {page_index} is empty or absent")));
        }
        let bytes = self.page_bytes(page_index, &page)?;
        if offset as usize > bytes.len() {
            return Err(Error::Invalid(format!(
                "offset {offset} is past the end of {}-byte page {page_index}",
                bytes.len()
            )));
        }
        Ok(ResourceData {
            page: bytes,
            start: offset as usize,
        })
    }

    /// A page's location record.
    pub fn page_info(&self, index: u16) -> Option<Page> {
        self.pages.get(index as usize).copied()
    }

    pub fn entry(&self, id: ResourceId) -> Result<ResourceEntry> {
        let index = id.index();
        let o = self.entries.element(index, ENTRY_SIZE).ok_or_else(|| {
            Error::Invalid(format!(
                "resource {index} is past the gestalt's {} entries",
                self.entries.count
            ))
        })?;
        let mut r = self.cache.reader_at(o);
        let owner = r.tag_ref()?;
        let salt = r.u16()?;
        let type_index = r.i8()?;
        r.skip(1);
        let fixup_offset = r.u32()?;
        let fixup_size = r.u32()?;
        r.seek(o + 0x20);
        let location_type = r.i16()?;
        let segment = opt_index(r.i16()?);
        let definition_address = CacheAddress(r.u32()?);
        let fixups = r.block()?;
        Ok(ResourceEntry {
            index,
            owner,
            salt,
            type_index,
            fixup_offset,
            fixup_size,
            location_type,
            segment,
            definition_address,
            fixups,
        })
    }

    /// The resource's definition: a small struct the owning tag understands
    /// (for render geometry, the vertex and index buffer descriptions).
    pub fn definition(&self, entry: &ResourceEntry) -> Result<&'a [u8]> {
        let (base, size) = self.fixup_data;
        let start = base + entry.fixup_offset as usize;
        let end = start + entry.fixup_size as usize;
        if entry.fixup_offset as usize + entry.fixup_size as usize > size
            || end > self.cache.bytes().len()
        {
            return Err(Error::Invalid(format!(
                "resource {} definition runs past the fixup data",
                entry.index
            )));
        }
        Ok(&self.cache.bytes()[start..end])
    }

    /// The definition with its pointer slots filled in: every fixup writes
    /// an address at an offset inside the definition, turning the stale
    /// runtime pointers into [`CacheAddress`]es that can be resolved.
    pub fn fixed_definition(&self, entry: &ResourceEntry) -> Result<Vec<u8>> {
        let mut def = self.definition(entry)?.to_vec();
        for o in entry.fixups.elements(8) {
            let mut r = self.cache.reader_at(o);
            let at = r.u32()? as usize;
            let address = r.u32()?;
            if at + 4 <= def.len() {
                def[at..at + 4].copy_from_slice(&address.to_le_bytes());
            }
        }
        Ok(def)
    }

    /// Where each buffer of the resource starts inside its data, in fixup
    /// order. The top four bits of a target carry a location code and are
    /// masked off.
    pub fn fixup_offsets(&self, entry: &ResourceEntry) -> Result<Vec<u32>> {
        entry
            .fixups
            .elements(8)
            .map(|o| Ok(self.cache.reader_at(o + 4).u32()? & 0x0FFF_FFFF))
            .collect()
    }

    /// The resource's bytes, decompressing its page on first use.
    pub fn data(&self, entry: &ResourceEntry) -> Result<ResourceData> {
        let seg_index = entry.segment.ok_or_else(|| {
            Error::Invalid(format!("resource {} has no data (segment -1)", entry.index))
        })?;
        let segment = *self.segments.get(seg_index as usize).ok_or_else(|| {
            Error::Invalid(format!("resource {} names segment {seg_index} of {}", entry.index, self.segments.len()))
        })?;
        let (mut page_index, mut offset) = match segment.secondary_page {
            Some(p) => (p, segment.secondary_offset),
            None => (
                segment.primary_page.ok_or_else(|| {
                    Error::Invalid(format!("resource {} segment has no page", entry.index))
                })?,
                segment.primary_offset,
            ),
        };
        let mut page = self.page(page_index)?;
        if let Some(primary) = segment.primary_page
            && (page.data_offset < 0 || page.compressed_size == 0)
        {
            page_index = primary;
            offset = segment.primary_offset;
            page = self.page(page_index)?;
        }
        if offset < 0 || page.data_offset < 0 || page.compressed_size == 0 {
            return Err(Error::Invalid(format!("resource {} has an empty page", entry.index)));
        }
        let bytes = self.page_bytes(page_index, &page)?;
        if offset as usize > bytes.len() {
            return Err(Error::Invalid(format!(
                "resource {} starts at {offset} in a {}-byte page",
                entry.index,
                bytes.len()
            )));
        }
        Ok(ResourceData {
            page: bytes,
            start: offset as usize,
        })
    }

    fn page(&self, index: u16) -> Result<Page> {
        self.pages
            .get(index as usize)
            .copied()
            .ok_or_else(|| Error::Invalid(format!("page {index} of {}", self.pages.len())))
    }

    fn shared_cache(&self, index: u16) -> Result<Arc<CacheFile>> {
        let mut shared = self.shared.lock().unwrap();
        if let Some(c) = shared.get(&index) {
            return Ok(c.clone());
        }
        let name = self.shared_caches.get(index as usize).ok_or_else(|| {
            Error::Invalid(format!("shared cache {index} of {}", self.shared_caches.len()))
        })?;
        // The table says `maps\shared.map`; only the file name matters.
        let file = name.rsplit(['\\', '/']).next().unwrap_or(name);
        let dir = self
            .cache
            .path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default();
        let opened = Arc::new(CacheFile::open(dir.join(file))?);
        shared.insert(index, opened.clone());
        Ok(opened)
    }

    fn page_bytes(&self, index: u16, page: &Page) -> Result<Arc<Vec<u8>>> {
        let key = (page.cache_index, index);
        if let Some(p) = self.page_cache.lock().unwrap().get(&key) {
            return Ok(p.clone());
        }
        let holder: Arc<CacheFile>;
        let file: &CacheFile = match page.cache_index {
            None => self.cache,
            Some(i) => {
                holder = self.shared_cache(i)?;
                &holder
            }
        };
        // Pages sit in the resource section of whichever file holds them.
        let base = file.header.section_file_base(SECTION_RESOURCE) as usize;
        let start = base + page.data_offset as usize;
        let end = start + page.compressed_size as usize;
        let all = file.bytes();
        if end > all.len() {
            return Err(Error::OutOfBounds {
                what: "resource page",
                offset: start as u64,
                len: page.compressed_size as u64,
            });
        }
        let raw = &all[start..end];
        let bytes = if page.compressed_size == page.decompressed_size {
            raw.to_vec()
        } else {
            // Deflate expands at most ~1032x; never trust a stated size beyond that.
            let cap = (page.decompressed_size as usize).min(raw.len().saturating_mul(1032));
            let mut out = Vec::with_capacity(cap);
            flate2::read::DeflateDecoder::new(raw)
                .take(page.decompressed_size as u64 + 1)
                .read_to_end(&mut out)
                .map_err(|e| Error::Invalid(format!("page {index} does not inflate: {e}")))?;
            if out.len() != page.decompressed_size as usize {
                return Err(Error::Invalid(format!(
                    "page {index} inflated to {} bytes, expected {}",
                    out.len(),
                    page.decompressed_size
                )));
            }
            out
        };
        let bytes = Arc::new(bytes);
        self.page_cache.lock().unwrap().insert(key, bytes.clone());
        Ok(bytes)
    }

    /// Forget decompressed pages (they can be 90 MB each).
    pub fn clear_page_cache(&self) {
        self.page_cache.lock().unwrap().clear();
    }
}
