//! String ids and the string table they index.
//!
//! A string id packs a namespace and an index. The namespaces' strings are
//! stored one after another in the table, so an id's (namespace, index) pair
//! is turned into a flat table position with the namespace table from the
//! header.

use crate::error::{Error, Result};
use crate::header::{Header, SECTION_DEBUG, u32_at};

/// How the 32 bits of a string id split up. MCC Halo 3 update 13 uses
/// 19 index bits, 8 namespace bits and 5 length bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StringIdLayout {
    pub index_bits: u32,
    pub namespace_bits: u32,
    pub length_bits: u32,
}

impl StringIdLayout {
    pub const MCC_HALO3_U13: StringIdLayout = StringIdLayout {
        index_bits: 19,
        namespace_bits: 8,
        length_bits: 5,
    };
    pub fn index(&self, id: StringId) -> u32 {
        id.0 & ((1 << self.index_bits) - 1)
    }
    pub fn namespace(&self, id: StringId) -> u32 {
        (id.0 >> self.index_bits) & ((1 << self.namespace_bits) - 1)
    }
    pub fn length(&self, id: StringId) -> u32 {
        (id.0 >> (self.index_bits + self.namespace_bits)) & ((1 << self.length_bits) - 1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct StringId(pub u32);

impl StringId {
    pub const NONE: StringId = StringId(0);
    pub fn is_none(self) -> bool {
        self.0 == 0
    }
}

#[derive(Clone, Copy, Debug)]
struct Namespace {
    /// Ids whose index is below this resolve directly (namespace 0 only).
    min: u32,
    /// Flat table position of this namespace's first string.
    start: u32,
}

#[derive(Clone, Debug)]
pub struct StringTable {
    pub layout: StringIdLayout,
    strings: Vec<Option<String>>,
    namespaces: Vec<Option<Namespace>>,
}

impl StringTable {
    pub fn parse(file: &[u8], header: &Header, layout: StringIdLayout) -> Result<StringTable> {
        let count = header.string_count as usize;
        if count == 0 {
            return Ok(StringTable {
                layout,
                strings: Vec::new(),
                namespaces: Vec::new(),
            });
        }
        let index_off = header.section_file_offset(SECTION_DEBUG, header.string_index) as usize;
        let table_off = header.section_file_offset(SECTION_DEBUG, header.string_table) as usize;
        let table_len = header.string_table_size as usize;
        bounds(file, "string index", index_off, count * 4)?;
        bounds(file, "string table", table_off, table_len)?;
        let table = &file[table_off..table_off + table_len];
        let mut strings = Vec::with_capacity(count);
        for i in 0..count {
            let o = u32_at(file, index_off + i * 4) as i32;
            if o < 0 || o as usize >= table.len() {
                strings.push(None);
                continue;
            }
            let o = o as usize;
            let end = table[o..]
                .iter()
                .position(|&c| c == 0)
                .map_or(table.len(), |n| o + n);
            strings.push(Some(String::from_utf8_lossy(&table[o..end]).into_owned()));
        }

        let mut namespaces = Vec::new();
        let ns_count = header.string_namespace_count as usize;
        if ns_count > 1 {
            let ns_off =
                header.section_file_offset(SECTION_DEBUG, header.string_namespace_table) as usize;
            bounds(file, "string namespace table", ns_off, ns_count * 4)?;
            let mask = (1u32 << layout.index_bits) - 1;
            let counts: Vec<u32> = (0..ns_count)
                .map(|i| u32_at(file, ns_off + i * 4) & mask)
                .collect();
            namespaces.resize(ns_count, None);
            // The first `counts[0]` strings of namespace 0 lead the table,
            // namespaces 1.. follow in order, and the rest of namespace 0
            // comes after them all.
            let mut start = counts[0];
            for (i, &c) in counts.iter().enumerate().skip(1) {
                namespaces[i] = Some(Namespace { min: 0, start });
                start += c;
            }
            namespaces[0] = Some(Namespace {
                min: counts[0],
                start,
            });
        }
        Ok(StringTable {
            layout,
            strings,
            namespaces,
        })
    }

    pub fn len(&self) -> usize {
        self.strings.len()
    }
    pub fn is_empty(&self) -> bool {
        self.strings.is_empty()
    }

    /// The string at flat table position `i`.
    pub fn at(&self, i: usize) -> Option<&str> {
        self.strings.get(i).and_then(|s| s.as_deref())
    }

    /// The flat table position a string id resolves to.
    pub fn index_of(&self, id: StringId) -> usize {
        let index = self.layout.index(id);
        if self.namespaces.is_empty() {
            return index as usize;
        }
        let mut ns = self.layout.namespace(id) as usize;
        while ns > 0 && self.namespaces.get(ns).is_none_or(|n| n.is_none()) {
            ns -= 1;
        }
        match self.namespaces.get(ns).copied().flatten() {
            Some(n) if index >= n.min => (index - n.min + n.start) as usize,
            _ => index as usize,
        }
    }

    pub fn get(&self, id: StringId) -> Option<&str> {
        self.at(self.index_of(id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (usize, &str)> {
        self.strings
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_deref().map(|s| (i, s)))
    }
}

pub(crate) fn bounds(file: &[u8], what: &'static str, off: usize, len: usize) -> Result<()> {
    if off.checked_add(len).is_some_and(|end| end <= file.len()) {
        Ok(())
    } else {
        Err(Error::OutOfBounds {
            what,
            offset: off as u64,
            len: len as u64,
        })
    }
}
