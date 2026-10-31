//! An MCC Halo CE cache file: the header, the tag index and its names, the
//! structure BSP chunks, and `bitmaps.map` beside it, where most bitmaps
//! keep their pixels. Follows `docs/format/halo1.md` sections 1 to 3.
//!
//! The map is read into memory (15 to 20 MB for a multiplayer map); the
//! 185 MB `bitmaps.map` is mapped read-only on first use. Nothing is
//! written.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::{Error, Result};

/// "head" read as a little-endian integer.
const MAGIC: u32 = 0x6865_6164;
/// "foot", the header's last four bytes.
const FOOT: u32 = 0x666F_6F74;
/// "tags", at the end of the tag data header.
const TAGS: u32 = 0x7461_6773;
/// The header is this long; the BSP vertex buffer usually follows it.
pub const HEADER_SIZE: usize = 0x800;
/// Tag data is loaded at this address: a pointer inside it, minus this,
/// plus the tag data's file offset, is a file offset.
pub const TAG_BASE: u32 = 0x5000_0000;

/// Whether `path` is an MCC Halo CE map: `head`, version 13, `foot` at
/// `0x7FC` and a tag data header ending in `tags` where `0x10` says. Halo 2
/// and Halo 3 MCC maps also say `head`, 13, so the rest decides; reads only
/// the header and those four bytes.
pub fn is_halo1(path: &Path) -> bool {
    let mut head = [0u8; HEADER_SIZE];
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    if u32_at(&head, 0) != Some(MAGIC) || u32_at(&head, 4) != Some(13) || u32_at(&head, 0x7FC) != Some(FOOT) {
        return false;
    }
    let Some(tag_data) = u32_at(&head, 0x10) else { return false };
    use std::io::{Seek, SeekFrom};
    let mut tags = [0u8; 4];
    f.seek(SeekFrom::Start(tag_data as u64 + 0x24)).is_ok() && f.read_exact(&mut tags).is_ok() && u32::from_le_bytes(tags) == TAGS
}

pub(crate) fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8], o: usize, max: usize) -> String {
    let s = b.get(o..(o + max).min(b.len())).unwrap_or(&[]);
    let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
    String::from_utf8_lossy(s).into_owned()
}

/// One entry of the tag index (0x20 bytes each).
#[derive(Clone, Debug)]
pub struct Tag {
    pub class: [u8; 4],
    /// Datum: salt in the high 16 bits, index in the low 16.
    pub id: u32,
    /// File offset of the tag's data; `None` for an empty slot. The
    /// structure BSP's entry has none: its data is in its own chunk.
    pub offset: Option<usize>,
    pub name: String,
}

impl Tag {
    pub fn is(&self, class: &[u8; 4]) -> bool {
        &self.class == class
    }

    pub fn class_str(&self) -> String {
        String::from_utf8_lossy(&self.class).into_owned()
    }
}

/// How pointers in a stretch of the file become file offsets: tag data and
/// each structure BSP chunk are loaded at their own address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Space {
    pub base: u32,
    pub start: usize,
    pub len: usize,
}

impl Space {
    pub fn offset(&self, pointer: u32) -> Option<usize> {
        let rel = pointer.checked_sub(self.base)? as usize;
        (rel < self.len).then_some(self.start + rel)
    }
}

/// A structure BSP chunk (scenario `+0x5A4`, `docs/format/halo1.md` 5).
#[derive(Clone, Debug)]
pub struct BspChunk {
    pub tag: u32,
    pub space: Space,
    /// File offset of the sbsp tag's data.
    pub sbsp: usize,
    /// The rendered and lightmap vertices of every lightmap material, one
    /// buffer: its file offset and size (chunk header `+8`, `+4`).
    pub vertices: (usize, usize),
}

/// An open map.
pub struct Map {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub map_name: String,
    pub build: String,
    pub tag_data: usize,
    pub scenario: u32,
    /// The model vertex area's file offset and size; the model index area
    /// follows it (`docs/format/halo1.md` 9).
    pub model_vertices: (usize, usize),
    pub model_data_size: usize,
    pub tags: Vec<Tag>,
    pub space: Space,
    bitmaps: OnceLock<Option<memmap2::Mmap>>,
}

impl Map {
    pub fn open(path: &Path) -> Result<Map> {
        if !is_halo1(path) {
            return Err(Error::Invalid(format!("{} is not an MCC Halo CE map", path.display())));
        }
        let bytes = std::fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
        let h = |o: usize| u32_at(&bytes, o).unwrap_or(0) as usize;
        let tag_data = h(0x10);
        let tag_data_size = h(0x14);
        let map_name = cstr(&bytes, 0x20, 32);
        let build = cstr(&bytes, 0x40, 32);
        if tag_data + tag_data_size > bytes.len() {
            return Err(Error::Invalid("tag data runs past the file".into()));
        }
        let space = Space {
            base: TAG_BASE,
            start: tag_data,
            len: tag_data_size,
        };
        let t = |o: usize| h(tag_data + o);
        let scenario = t(4) as u32;
        let count = t(0xC);
        let model_vertices = (t(0x14), t(0x1C));
        let model_data_size = t(0x20);
        let array = space
            .offset(t(0) as u32)
            .ok_or_else(|| Error::Invalid("tag array pointer outside the tag data".into()))?;
        if count > 65536 || array + 0x20 * count > bytes.len() {
            return Err(Error::Invalid("tag array runs past the file".into()));
        }
        let mut tags = Vec::with_capacity(count);
        for k in 0..count {
            let o = array + 0x20 * k;
            let class = [bytes[o + 3], bytes[o + 2], bytes[o + 1], bytes[o]];
            let id = h(o + 12) as u32;
            let name = space.offset(h(o + 16) as u32).map_or(String::new(), |at| cstr(&bytes, at, 256));
            tags.push(Tag {
                class,
                id,
                offset: space.offset(h(o + 20) as u32),
                name,
            });
        }
        Ok(Map {
            path: path.to_path_buf(),
            bytes,
            map_name,
            build,
            tag_data,
            scenario,
            model_vertices,
            model_data_size,
            tags,
            space,
            bitmaps: OnceLock::new(),
        })
    }

    /// The tag a datum names, if the salt agrees.
    pub fn tag(&self, id: u32) -> Option<&Tag> {
        if id == u32::MAX {
            return None;
        }
        self.tags.get((id & 0xFFFF) as usize).filter(|t| t.id == id)
    }

    pub fn tag_name(&self, id: u32) -> Option<&str> {
        self.tag(id).map(|t| t.name.as_str())
    }

    pub fn class_of(&self, id: u32) -> Option<[u8; 4]> {
        self.tag(id).map(|t| t.class)
    }

    pub fn find(&self, class: &[u8; 4], name: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.is(class) && t.name.eq_ignore_ascii_case(name))
    }

    /// A reader at the start of a tag's data.
    pub fn tag_data(&self, id: u32) -> Option<Reader<'_>> {
        let o = self.tag(id)?.offset?;
        Some(Reader {
            map: self,
            at: o,
            space: self.space,
        })
    }

    /// The scenario's structure BSPs (`+0x5A4`, 0x20 each: chunk file
    /// offset, size, load address, then the sbsp reference at `+0x10`).
    pub fn bsps(&self) -> Result<Vec<BspChunk>> {
        let scnr = self.tag_data(self.scenario).ok_or_else(|| Error::Invalid("no scenario data".into()))?;
        let mut out = Vec::new();
        for e in scnr.block(0x5A4, 0x20)? {
            let start = e.u32(0)? as usize;
            let len = e.u32(4)? as usize;
            let base = e.u32(8)?;
            let tag = e.tag_ref(0x10)?;
            if start + len > self.bytes.len() {
                return Err(Error::Invalid(format!("bsp chunk {start:#x} + {len:#x} runs past the file")));
            }
            let space = Space { base, start, len };
            let head = Reader { map: self, at: start, space };
            let sbsp = space
                .offset(head.u32(0)?)
                .ok_or_else(|| Error::Invalid("bsp chunk: sbsp pointer outside the chunk".into()))?;
            let vertices = (head.u32(8)? as usize, head.u32(4)? as usize);
            if vertices.0 + vertices.1 > self.bytes.len() {
                return Err(Error::Invalid("bsp vertex buffer runs past the file".into()));
            }
            out.push(BspChunk { tag, space, sbsp, vertices });
        }
        Ok(out)
    }

    /// A reader over one BSP chunk's sbsp data.
    pub fn sbsp<'a>(&'a self, chunk: &BspChunk) -> Reader<'a> {
        Reader {
            map: self,
            at: chunk.sbsp,
            space: chunk.space,
        }
    }

    /// `bitmaps.map` beside this map, mapped on first use.
    pub fn bitmaps_file(&self) -> Option<&[u8]> {
        self.bitmaps
            .get_or_init(|| {
                let path = self.path.with_file_name("bitmaps.map");
                let f = File::open(&path).ok()?;
                // Read-only map of a file we never write; see memmap2's
                // safety note: the game install is not modified under us.
                unsafe { memmap2::Mmap::map(&f) }.ok()
            })
            .as_deref()
    }

    /// Bytes at a file offset of this map.
    pub fn slice(&self, at: usize, len: usize) -> Result<&[u8]> {
        self.bytes
            .get(at..at.saturating_add(len))
            .ok_or_else(|| Error::Invalid(format!("{len} bytes at {at:#x} run past {}", self.map_name)))
    }
}

/// A bounds-checked view at a file offset, with the address space its
/// pointers live in.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub map: &'a Map,
    pub at: usize,
    pub space: Space,
}

impl<'a> Reader<'a> {
    pub fn at(self, off: usize) -> Reader<'a> {
        Reader { at: self.at + off, ..self }
    }

    fn get<const N: usize>(&self, off: usize) -> Result<[u8; N]> {
        let o = self.at + off;
        self.map
            .bytes
            .get(o..o + N)
            .and_then(|s| s.try_into().ok())
            .ok_or_else(|| Error::Invalid(format!("read at {o:#x} past the end of {}", self.map.map_name)))
    }
    pub fn u8(&self, off: usize) -> Result<u8> {
        Ok(self.get::<1>(off)?[0])
    }
    pub fn i8(&self, off: usize) -> Result<i8> {
        Ok(self.get::<1>(off)?[0] as i8)
    }
    pub fn u16(&self, off: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.get(off)?))
    }
    pub fn i16(&self, off: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.get(off)?))
    }
    pub fn u32(&self, off: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.get(off)?))
    }
    pub fn i32(&self, off: usize) -> Result<i32> {
        Ok(i32::from_le_bytes(self.get(off)?))
    }
    pub fn f32(&self, off: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.get(off)?))
    }
    pub fn vec3(&self, off: usize) -> Result<glam::Vec3> {
        Ok(glam::Vec3::new(self.f32(off)?, self.f32(off + 4)?, self.f32(off + 8)?))
    }
    /// A C string stored in place (names of up to 32 bytes).
    pub fn str(&self, off: usize, max: usize) -> String {
        cstr(&self.map.bytes, self.at + off, max)
    }
    /// A 16-byte tag reference: class, name address, name length, datum.
    /// Returns the datum.
    pub fn tag_ref(&self, off: usize) -> Result<u32> {
        self.u32(off + 12)
    }

    /// The elements of the 12-byte block at `off` (count, pointer, unused),
    /// each `size` bytes. A block with no elements gives an empty list.
    pub fn block(&self, off: usize, size: usize) -> Result<Vec<Reader<'a>>> {
        let count = self.u32(off)? as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let pointer = self.u32(off + 4)?;
        let start = self
            .space
            .offset(pointer)
            .ok_or_else(|| Error::Invalid(format!("block at {:#x}: bad pointer {pointer:#x}", self.at + off)))?;
        if count > 1 << 20 || start + count * size > self.map.bytes.len() {
            return Err(Error::Invalid(format!("block at {:#x}: {count} elements of {size} run past the file", self.at + off)));
        }
        Ok((0..count).map(|k| Reader { at: start + k * size, ..*self }).collect())
    }

    pub fn block_count(&self, off: usize) -> Result<usize> {
        Ok(self.u32(off)? as usize)
    }
}
