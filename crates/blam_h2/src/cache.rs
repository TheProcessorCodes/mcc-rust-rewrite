//! An MCC Halo 2 cache file, inflated into memory: the header, the tag
//! index, tag names and string ids, and the companion files (`shared.map`,
//! `textures.dat`) its tags point into. Follows `docs/format/halo2.md`
//! sections 1 to 4.
//!
//! The file on disk is only read. The inflated copy lives in memory (about
//! 11 MB for a multiplayer map, 45 MB for `shared.map`).

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::{Error, Result};

/// "head" read as a little-endian integer.
const MAGIC: u32 = 0x6865_6164;
/// The MCC Halo 2 header is this long and stored uncompressed.
pub const HEADER_SIZE: usize = 0x380;

/// Whether `path` is an MCC Halo 2 (classic) map: magic `head`, version
/// 13, and a scenario path at `0xD0` starting `scenarios\` (Halo 3 keeps its
/// map name there instead). Reads only the header.
pub fn is_halo2(path: &Path) -> bool {
    let mut head = [0u8; HEADER_SIZE];
    let Ok(mut f) = File::open(path) else {
        return false;
    };
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    u32_at(&head, 0) == Some(MAGIC) && u32_at(&head, 4) == Some(13) && head[0xD0..].starts_with(b"scenarios\\")
}

pub(crate) fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn cstr(b: &[u8], o: usize, max: usize) -> String {
    let s = b.get(o..(o + max).min(b.len())).unwrap_or(&[]);
    let s = &s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())];
    String::from_utf8_lossy(s).into_owned()
}

/// Where a raw-data pointer's bytes live (its top two bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Local,
    MainMenu,
    Shared,
    SinglePlayerShared,
}

impl Location {
    pub fn of(pointer: u32) -> Location {
        match pointer >> 30 {
            0 => Location::Local,
            1 => Location::MainMenu,
            2 => Location::Shared,
            _ => Location::SinglePlayerShared,
        }
    }
}

/// One entry of the tag index.
#[derive(Clone, Debug)]
pub struct Tag {
    pub class: [u8; 4],
    /// Datum: salt in the high 16 bits, index in the low 16.
    pub id: u32,
    /// Offset of the tag's data in this file's inflated bytes; `None` for a
    /// tag that lives in `shared.map` (or an empty slot).
    pub offset: Option<usize>,
    pub size: u32,
    pub name: String,
}

impl Tag {
    pub fn class_str(&self) -> String {
        String::from_utf8_lossy(&self.class).into_owned()
    }

    pub fn is(&self, class: &[u8; 4]) -> bool {
        &self.class == class
    }

    /// The tag's file name, e.g. `coag_ground`.
    pub fn short_name(&self) -> &str {
        self.name.rsplit('\\').next().unwrap_or(&self.name)
    }
}

/// An open, inflated map.
pub struct Map {
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub map_name: String,
    pub scenario_path: String,
    pub compressed: bool,
    pub index_address: usize,
    meta_mask: u32,
    pub classes: Vec<[u8; 4]>,
    pub tags: Vec<Tag>,
    pub strings: Vec<String>,
    shared: OnceLock<Option<Box<Map>>>,
    textures: OnceLock<Option<memmap2::Mmap>>,
}

impl Map {
    pub fn open(path: &Path) -> Result<Map> {
        let raw = std::fs::read(path).map_err(|e| Error::Io(path.to_path_buf(), e))?;
        if raw.len() < HEADER_SIZE || u32_at(&raw, 0) != Some(MAGIC) {
            return Err(Error::Invalid(format!("{} is not a Halo cache file", path.display())));
        }
        if u32_at(&raw, 4) != Some(13) || !raw[0xD0..].starts_with(b"scenarios\\") {
            return Err(Error::Invalid(format!("{} is not an MCC Halo 2 map", path.display())));
        }
        let compressed = u16::from_le_bytes([raw[0x1C], raw[0x1D]]) & 1 != 0;
        let bytes = if compressed { inflate(&raw)? } else { raw };
        let h = |o: usize| u32_at(&bytes, o).unwrap_or(0) as usize;
        let index_address = h(0x10);
        let meta_mask = h(0x2D0) as u32;
        let map_name = cstr(&bytes, 0xB0, 32);
        let scenario_path = cstr(&bytes, 0xD0, 256);

        // Tag index: pointers in it are relative to its own address.
        let ix = |o: usize| u32_at(&bytes, index_address + o).map(|v| v as usize);
        let bad = || Error::Invalid("tag index runs past the file".into());
        let class_table = index_address + ix(0).ok_or_else(bad)?;
        let class_count = ix(4).ok_or_else(bad)?;
        let tag_table = index_address + ix(8).ok_or_else(bad)?;
        let tag_count = ix(24).ok_or_else(bad)?;
        if class_count > 4096 || tag_count > 65536 || tag_table + 16 * tag_count > bytes.len() {
            return Err(bad());
        }
        let fourcc = |o: usize| -> [u8; 4] {
            let mut c = [bytes[o + 3], bytes[o + 2], bytes[o + 1], bytes[o]];
            if c == [0xFF; 4] {
                c = *b"    ";
            }
            c
        };
        let classes = (0..class_count).map(|k| fourcc(class_table + 12 * k)).collect();

        let (name_index, name_table) = (h(0x2C), h(0x24));
        let mut tags = Vec::with_capacity(tag_count);
        for k in 0..tag_count {
            let o = tag_table + 16 * k;
            let id = h(o + 4) as u32;
            let pointer = h(o + 8) as u32;
            let size = h(o + 12) as u32;
            let offset = (id != u32::MAX && pointer != 0)
                .then(|| (pointer.wrapping_sub(meta_mask) as usize).wrapping_add(index_address))
                .filter(|&a| a < bytes.len());
            let name = if id == u32::MAX {
                String::new()
            } else {
                let at = u32_at(&bytes, name_index + 4 * k).unwrap_or(u32::MAX) as usize;
                cstr(&bytes, name_table.saturating_add(at), 256)
            };
            tags.push(Tag {
                class: fourcc(o),
                id,
                offset,
                size,
                name,
            });
        }

        let (string_count, string_table, string_index) = (h(0x30), h(0x34), h(0x3C));
        let strings = (0..string_count.min(1 << 24))
            .map(|k| match u32_at(&bytes, string_index + 4 * k) {
                Some(at) if (at as i32) >= 0 => cstr(&bytes, string_table + at as usize, 256),
                _ => String::new(),
            })
            .collect();

        Ok(Map {
            path: path.to_path_buf(),
            bytes,
            map_name,
            scenario_path,
            compressed,
            index_address,
            meta_mask,
            classes,
            tags,
            strings,
            shared: OnceLock::new(),
            textures: OnceLock::new(),
        })
    }

    /// The tag a datum names, if it is in this index and the salt agrees.
    pub fn tag(&self, id: u32) -> Option<&Tag> {
        if id == u32::MAX {
            return None;
        }
        self.tags.get((id & 0xFFFF) as usize).filter(|t| t.id == id)
    }

    pub fn find(&self, class: &[u8; 4], name: &str) -> Option<&Tag> {
        self.tags.iter().find(|t| t.is(class) && t.name.eq_ignore_ascii_case(name))
    }

    pub fn first_of(&self, class: &[u8; 4]) -> Option<&Tag> {
        self.tags.iter().find(|t| t.is(class) && t.id != u32::MAX)
    }

    /// A string id: the low 24 bits index the string table.
    pub fn string(&self, id: u32) -> &str {
        if id == 0 || id == u32::MAX {
            return "";
        }
        self.strings.get((id & 0xFF_FFFF) as usize).map_or("", String::as_str)
    }

    /// A tag-data pointer (inside a tag) to an offset in this file.
    pub fn address(&self, pointer: u32) -> Option<usize> {
        if pointer == 0 {
            return None;
        }
        Some((pointer.wrapping_sub(self.meta_mask) as usize).wrapping_add(self.index_address)).filter(|&a| a < self.bytes.len())
    }

    /// `shared.map` beside this map, opened on first use.
    pub fn shared(&self) -> Option<&Map> {
        self.shared
            .get_or_init(|| {
                let path = self.path.with_file_name("shared.map");
                if path == self.path {
                    return None;
                }
                match Map::open(&path) {
                    Ok(m) => Some(Box::new(m)),
                    Err(e) => {
                        eprintln!("shared.map not opened: {e}");
                        None
                    }
                }
            })
            .as_deref()
    }

    /// A reader at the start of a tag's data, in this map or, for a tag
    /// whose index entry here is empty, in `shared.map` (`reader.map` says
    /// which; raw data and pointers inside are that file's).
    pub fn tag_data(&self, id: u32) -> Option<Reader<'_>> {
        let t = self.tag(id)?;
        match t.offset {
            Some(o) => Some(Reader { map: self, at: o }),
            None => {
                let s = self.shared()?;
                let o = s.tag(id)?.offset?;
                Some(Reader { map: s, at: o })
            }
        }
    }

    /// The name of the tag a datum names, looked up here or in `shared.map`.
    pub fn tag_name(&self, id: u32) -> Option<&str> {
        self.tag(id).map(|t| t.name.as_str())
    }

    /// Raw bytes behind a raw-data pointer (`docs/format/halo2.md` 4): the
    /// local file, or `shared.map`. Main-menu and single-player-shared data
    /// are not read.
    pub fn raw(&self, pointer: u32, size: usize) -> Result<&[u8]> {
        let at = (pointer & 0x3FFF_FFFF) as usize;
        let file = match Location::of(pointer) {
            Location::Local => self,
            Location::Shared => match self.shared() {
                Some(s) => s,
                None if self.path.file_name().is_some_and(|n| n.eq_ignore_ascii_case("shared.map")) => self,
                None => return Err(Error::Invalid("raw data in shared.map, which did not open".into())),
            },
            l => return Err(Error::Unsupported(format!("raw data in {l:?}"))),
        };
        file.bytes
            .get(at..at.saturating_add(size))
            .ok_or_else(|| Error::Invalid(format!("raw data {pointer:#x} + {size} runs past the file")))
    }

    /// `textures.dat` beside the map, mapped on first use.
    pub fn textures(&self) -> Option<&[u8]> {
        self.textures
            .get_or_init(|| {
                let path = self.path.with_file_name("textures.dat");
                let f = File::open(&path).ok()?;
                // Read-only map of a file we never write; see memmap2's
                // safety note: the game install is not modified under us.
                unsafe { memmap2::Mmap::map(&f) }.ok()
            })
            .as_deref()
    }

    /// The pixels of one bitmap resource in `textures.dat`: a count, that
    /// many chunk sizes (negative: stored), then the chunks, each a zlib
    /// stream (`docs/format/halo2.md` 9.2).
    pub fn texture_data(&self, pointer: u32) -> Result<Vec<u8>> {
        let data = self.textures().ok_or_else(|| Error::Invalid("textures.dat not found beside the map".into()))?;
        let at = (pointer & 0x3FFF_FFFF) as usize;
        let n = u32_at(data, at).ok_or_else(|| Error::Invalid("bitmap pointer past textures.dat".into()))? as usize;
        if n == 0 || n > 4096 {
            return Err(Error::Invalid(format!("bitmap at {at:#x} lists {n} chunks")));
        }
        let mut cursor = at + 4 + 4 * n;
        let mut out = Vec::new();
        for k in 0..n {
            let size = u32_at(data, at + 4 + 4 * k).unwrap_or(0) as i32;
            let len = size.unsigned_abs() as usize;
            let chunk = data
                .get(cursor..cursor + len)
                .ok_or_else(|| Error::Invalid(format!("bitmap chunk at {cursor:#x} past textures.dat")))?;
            if size < 0 {
                out.extend_from_slice(chunk);
            } else {
                flate2::read::ZlibDecoder::new(chunk)
                    .read_to_end(&mut out)
                    .map_err(|e| Error::Invalid(format!("bitmap chunk at {cursor:#x}: {e}")))?;
            }
            cursor += len;
        }
        Ok(out)
    }
}

/// Section 1: the header stays as stored; after it come zlib streams listed
/// by the chunk table (size, file offset; a negative size is a stored chunk
/// of that many bytes after a two-byte prefix).
fn inflate(raw: &[u8]) -> Result<Vec<u8>> {
    let h = |o: usize| u32_at(raw, o).unwrap_or(0);
    let (table, count) = (h(0x310) as usize, h(0x314) as usize);
    let stated = h(0x08) as usize;
    if count > 1 << 16 || table + 8 * count > raw.len() {
        return Err(Error::Invalid("compressed chunk table runs past the file".into()));
    }
    let mut out = Vec::with_capacity(stated.max(raw.len()));
    out.extend_from_slice(&raw[..HEADER_SIZE]);
    for k in 0..count {
        let size = h(table + 8 * k) as i32;
        let at = h(table + 8 * k + 4) as usize;
        if size < 0 {
            let len = size.unsigned_abs() as usize;
            let chunk = raw
                .get(at + 2..at + 2 + len)
                .ok_or_else(|| Error::Invalid(format!("stored chunk {k} past the file")))?;
            out.extend_from_slice(chunk);
        } else {
            let chunk = raw
                .get(at..at + size as usize)
                .ok_or_else(|| Error::Invalid(format!("chunk {k} past the file")))?;
            flate2::read::ZlibDecoder::new(chunk)
                .read_to_end(&mut out)
                .map_err(|e| Error::Invalid(format!("chunk {k}: {e}")))?;
        }
    }
    if out.len() != stated {
        return Err(Error::Invalid(format!("inflated to {:#x} bytes, header says {stated:#x}", out.len())));
    }
    Ok(out)
}

/// A bounds-checked view at an offset in one map's bytes.
#[derive(Clone, Copy)]
pub struct Reader<'a> {
    pub map: &'a Map,
    pub at: usize,
}

impl<'a> Reader<'a> {
    pub fn at(self, off: usize) -> Reader<'a> {
        Reader { map: self.map, at: self.at + off }
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
    pub fn u16(&self, off: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.get(off)?))
    }
    pub fn i16(&self, off: usize) -> Result<i16> {
        Ok(i16::from_le_bytes(self.get(off)?))
    }
    pub fn u32(&self, off: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.get(off)?))
    }
    pub fn f32(&self, off: usize) -> Result<f32> {
        Ok(f32::from_le_bytes(self.get(off)?))
    }
    pub fn vec3(&self, off: usize) -> Result<glam::Vec3> {
        Ok(glam::Vec3::new(self.f32(off)?, self.f32(off + 4)?, self.f32(off + 8)?))
    }
    /// An 8-byte tag reference: class, then datum. Returns the datum.
    pub fn tag_ref(&self, off: usize) -> Result<u32> {
        self.u32(off + 4)
    }

    /// The elements of the tag block at `off` (count, then pointer), each
    /// `size` bytes. A block with no elements gives an empty list.
    pub fn block(&self, off: usize, size: usize) -> Result<Vec<Reader<'a>>> {
        let count = self.u32(off)? as usize;
        if count == 0 {
            return Ok(Vec::new());
        }
        let pointer = self.u32(off + 4)?;
        let start = self
            .map
            .address(pointer)
            .ok_or_else(|| Error::Invalid(format!("block at {:#x}: bad pointer {pointer:#x}", self.at + off)))?;
        if count > 1 << 20 || start + count * size > self.map.bytes.len() {
            return Err(Error::Invalid(format!("block at {:#x}: {count} elements of {size} run past the file", self.at + off)));
        }
        Ok((0..count).map(|k| Reader { map: self.map, at: start + k * size }).collect())
    }

    pub fn block_count(&self, off: usize) -> Result<usize> {
        Ok(self.u32(off)? as usize)
    }
}
