//! The 16 KiB header at the start of every MCC Halo 3 cache file (format
//! version 13, update 6 and later), and the rules that turn the pointers in
//! it into file offsets.
//!
//! The file is laid out as four *sections*. Every 32-bit pointer in the
//! header is an address in a virtual layout where each section sits at
//! `sections[i].address`; adding `section_offsets[i]` (wrapping at 32 bits)
//! turns such an address into a file offset. Tag data (section 2) is
//! addressed differently: with 64-bit addresses relative to `virtual_base`.

use crate::error::{Error, Result};

pub const HEADER_SIZE: usize = 0x4000;
/// "head" read as a little-endian integer.
pub const MAGIC: u32 = 0x6865_6164;
pub const SUPPORTED_VERSION: u32 = 13;

pub const SECTION_DEBUG: usize = 0;
pub const SECTION_RESOURCE: usize = 1;
pub const SECTION_TAG: usize = 2;
pub const SECTION_LOCALE: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Section {
    pub address: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Partition {
    pub address: u64,
    pub size: u64,
}

#[derive(Clone, Debug)]
pub struct Header {
    pub version: u32,
    pub file_size: u32,
    pub tag_data_address: u32,
    pub virtual_size: u32,
    pub tag_count: u32,
    pub tag_name_table: u32,
    pub tag_name_table_size: u32,
    pub tag_name_index: u32,
    pub string_count: u32,
    pub string_table: u32,
    pub string_table_size: u32,
    pub string_index: u32,
    pub string_namespace_count: u32,
    pub string_namespace_table: u32,
    pub build: String,
    pub map_name: String,
    pub scenario_path: String,
    pub virtual_base: u64,
    pub tag_index_pointer: u64,
    pub partitions: [Partition; 6],
    pub section_offsets: [u32; 4],
    pub sections: [Section; 4],
}

pub(crate) fn u16_at(b: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([b[off], b[off + 1]])
}
pub(crate) fn u32_at(b: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]])
}
pub(crate) fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}
pub(crate) fn cstr_at(b: &[u8], off: usize, max: usize) -> String {
    let end = (off + max).min(b.len());
    let s = &b[off..end];
    let s = match s.iter().position(|&c| c == 0) {
        Some(n) => &s[..n],
        None => s,
    };
    String::from_utf8_lossy(s).into_owned()
}

impl Header {
    pub fn parse(b: &[u8]) -> Result<Header> {
        if b.len() < HEADER_SIZE {
            return Err(Error::OutOfBounds {
                what: "header",
                offset: 0,
                len: HEADER_SIZE as u64,
            });
        }
        let magic = u32_at(b, 0);
        if magic != MAGIC {
            return Err(Error::BadMagic(magic));
        }
        let version = u32_at(b, 4);
        if version != SUPPORTED_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let build = cstr_at(b, 0xA0, 32);
        // Only the update-6-and-later layout is implemented. Older MCC builds
        // keep the build string at 0x120 instead and are refused.
        if build.is_empty() {
            return Err(Error::UnsupportedBuild(cstr_at(b, 0x120, 32)));
        }
        let mut partitions = [Partition::default(); 6];
        for (i, p) in partitions.iter_mut().enumerate() {
            let o = 0x300 + i * 16;
            *p = Partition {
                address: u64_at(b, o),
                size: u64_at(b, o + 8),
            };
        }
        let mut section_offsets = [0u32; 4];
        for (i, s) in section_offsets.iter_mut().enumerate() {
            *s = u32_at(b, 0x4CC + i * 4);
        }
        let mut sections = [Section::default(); 4];
        for (i, s) in sections.iter_mut().enumerate() {
            let o = 0x4DC + i * 8;
            *s = Section {
                address: u32_at(b, o),
                size: u32_at(b, o + 4),
            };
        }
        Ok(Header {
            version,
            file_size: u32_at(b, 0x8),
            tag_data_address: u32_at(b, 0x10),
            virtual_size: u32_at(b, 0x14),
            tag_count: u32_at(b, 0x20),
            tag_name_table: u32_at(b, 0x24),
            tag_name_table_size: u32_at(b, 0x28),
            tag_name_index: u32_at(b, 0x2C),
            string_count: u32_at(b, 0x30),
            string_table: u32_at(b, 0x34),
            string_table_size: u32_at(b, 0x38),
            string_index: u32_at(b, 0x3C),
            string_namespace_count: u32_at(b, 0x40),
            string_namespace_table: u32_at(b, 0x44),
            build,
            map_name: cstr_at(b, 0xC0, 32),
            scenario_path: cstr_at(b, 0xE0, 256),
            virtual_base: u64_at(b, 0x2E0),
            tag_index_pointer: u64_at(b, 0x2E8),
            partitions,
            section_offsets,
            sections,
        })
    }

    /// File offset of a 32-bit pointer that addresses `section`'s virtual
    /// space. The header's own pointers address section 0.
    pub fn section_file_offset(&self, section: usize, pointer: u32) -> u64 {
        pointer.wrapping_add(self.section_offsets[section]) as u64
    }

    /// File offset where `section` begins.
    pub fn section_file_base(&self, section: usize) -> u64 {
        self.section_file_offset(section, self.sections[section].address)
    }

    /// File offset of a 64-bit tag-data address.
    pub fn tag_address_to_file(&self, address: u64) -> Result<u64> {
        let rel = address
            .checked_sub(self.virtual_base)
            .ok_or(Error::BadTagAddress(address))?;
        if rel >= self.sections[SECTION_TAG].size as u64 {
            return Err(Error::BadTagAddress(address));
        }
        Ok(self.section_file_base(SECTION_TAG) + rel)
    }

    /// Tag data stores its 32-bit pointers shifted right by two. This undoes
    /// that and gives the file offset.
    pub fn packed_pointer_to_file(&self, packed: u32) -> Result<u64> {
        self.tag_address_to_file((packed as u64) << 2)
    }
}
