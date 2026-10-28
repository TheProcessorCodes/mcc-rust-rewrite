//! A cursor over the file for walking tag data: fixed-size fields, string
//! ids, tag references, tag blocks and data references.

use crate::CacheFile;
use crate::error::{Error, Result};
use crate::strings::StringId;
use crate::tags::FourCc;

/// A reference from one tag to another: 16 bytes in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TagRef {
    pub class: FourCc,
    /// Tag index, or `None` for a null reference.
    pub index: Option<u32>,
}

/// A tag block: `count` elements of some fixed size, somewhere in tag data.
/// 12 bytes in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub count: u32,
    /// File offset of the first element, or `None` when the block is empty.
    pub offset: Option<usize>,
}

impl Block {
    pub fn is_empty(&self) -> bool {
        self.count == 0 || self.offset.is_none()
    }
    /// File offset of element `i` when each element is `size` bytes.
    pub fn element(&self, i: u32, size: usize) -> Option<usize> {
        (i < self.count).then_some(self.offset? + i as usize * size)
    }
    pub fn elements(&self, size: usize) -> impl Iterator<Item = usize> + '_ {
        (0..self.count).filter_map(move |i| self.element(i, size))
    }
}

/// A run of raw bytes inside tag data (a "data reference"): 20 bytes in the
/// file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataRef {
    pub size: u32,
    pub offset: Option<usize>,
}

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    cache: &'a CacheFile,
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(cache: &'a CacheFile, pos: usize) -> Reader<'a> {
        Reader { cache, pos }
    }

    pub fn cache(&self) -> &'a CacheFile {
        self.cache
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn seek(&mut self, pos: usize) -> &mut Self {
        self.pos = pos;
        self
    }
    pub fn skip(&mut self, n: usize) -> &mut Self {
        self.pos += n;
        self
    }
    /// A new cursor at `file_offset`.
    pub fn at(&self, file_offset: usize) -> Reader<'a> {
        Reader {
            cache: self.cache,
            pos: file_offset,
        }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let bytes = self.cache.bytes();
        let end = self.pos.checked_add(n).filter(|&e| e <= bytes.len());
        match end {
            Some(end) => {
                let s = &bytes[self.pos..end];
                self.pos = end;
                Ok(s)
            }
            None => Err(Error::OutOfBounds {
                what: "tag data",
                offset: self.pos as u64,
                len: n as u64,
            }),
        }
    }

    pub fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    pub fn i8(&mut self) -> Result<i8> {
        Ok(self.u8()? as i8)
    }
    pub fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub fn i16(&mut self) -> Result<i16> {
        Ok(self.u16()? as i16)
    }
    pub fn u32(&mut self) -> Result<u32> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }
    pub fn u64(&mut self) -> Result<u64> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
    pub fn f32(&mut self) -> Result<f32> {
        Ok(f32::from_bits(self.u32()?))
    }
    pub fn f32x2(&mut self) -> Result<[f32; 2]> {
        Ok([self.f32()?, self.f32()?])
    }
    pub fn f32x3(&mut self) -> Result<[f32; 3]> {
        Ok([self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn f32x4(&mut self) -> Result<[f32; 4]> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    pub fn string_id(&mut self) -> Result<StringId> {
        Ok(StringId(self.u32()?))
    }

    /// A 32-bit tag-data pointer as stored (shifted right by two), turned
    /// into a file offset. Zero is a null pointer.
    pub fn pointer(&mut self) -> Result<Option<usize>> {
        let packed = self.u32()?;
        if packed == 0 {
            return Ok(None);
        }
        Ok(Some(self.cache.header.packed_pointer_to_file(packed)? as usize))
    }

    pub fn tag_ref(&mut self) -> Result<TagRef> {
        let class_id = self.u32()?;
        self.skip(8);
        let id = self.u32()?;
        let index = (id as u16) as i16;
        Ok(TagRef {
            class: FourCc::from_class_id(class_id),
            index: (class_id != u32::MAX && index >= 0).then_some(index as u32),
        })
    }

    pub fn block(&mut self) -> Result<Block> {
        let count = self.u32()?;
        let offset = self.pointer()?;
        self.skip(4);
        Ok(Block {
            count: if offset.is_some() { count } else { 0 },
            offset: if count > 0 { offset } else { None },
        })
    }

    pub fn data_ref(&mut self) -> Result<DataRef> {
        let size = self.u32()?;
        self.skip(8);
        let offset = self.pointer()?;
        self.skip(4);
        Ok(DataRef {
            size: if offset.is_some() { size } else { 0 },
            offset: if size > 0 { offset } else { None },
        })
    }
}
