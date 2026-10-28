//! Tag classes, the tag index and tag names.

use std::fmt;

use crate::error::{Error, Result};
use crate::header::{Header, SECTION_DEBUG, u16_at, u32_at, u64_at};
use crate::strings::{StringId, bounds};

/// A four-character tag class code such as `sbsp` or `scnr`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FourCc(pub [u8; 4]);

impl FourCc {
    /// A class id as the file stores it: the four characters packed
    /// big-endian into an integer that is then written little-endian.
    pub fn from_class_id(id: u32) -> FourCc {
        FourCc(id.to_be_bytes())
    }
    /// A code from text; shorter codes are padded with spaces, longer ones cut.
    pub fn from_code(s: &str) -> FourCc {
        let mut b = *b"    ";
        for (d, s) in b.iter_mut().zip(s.bytes()) {
            *d = s;
        }
        FourCc(b)
    }
    pub fn is_null(&self) -> bool {
        self.0 == [0xff; 4] || self.0 == [0; 4]
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

impl fmt::Display for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_null() {
            f.write_str("----")
        } else {
            f.write_str(self.as_str())
        }
    }
}

impl fmt::Debug for FourCc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FourCc({self})")
    }
}

impl PartialEq<str> for FourCc {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for FourCc {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TagClass {
    pub code: FourCc,
    pub parent: FourCc,
    pub grandparent: FourCc,
    pub name: StringId,
}

#[derive(Clone, Debug)]
pub struct TagEntry {
    /// Position in the tag index; the low 16 bits of a tag id.
    pub index: u32,
    pub class_index: u16,
    /// The high 16 bits of the tag's full id.
    pub salt: u16,
    /// Tag-data pointer as stored (shifted right by two).
    pub meta_pointer: u32,
    pub name: Option<String>,
}

impl TagEntry {
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }
}

#[derive(Clone, Debug)]
pub struct TagIndex {
    pub classes: Vec<TagClass>,
    /// One slot per tag index; `None` for the empty tag every map carries.
    pub entries: Vec<Option<TagEntry>>,
}

impl TagIndex {
    pub fn parse(file: &[u8], header: &Header) -> Result<TagIndex> {
        // shared.map and campaign.map hold only resources: no tags at all.
        if header.tag_count == 0 || header.tag_index_pointer == 0 {
            return Ok(TagIndex {
                classes: Vec::new(),
                entries: Vec::new(),
            });
        }
        let idx = header.tag_address_to_file(header.tag_index_pointer)? as usize;
        bounds(file, "tag index", idx, 72)?;
        let class_count = u32_at(file, idx) as usize;
        let class_ptr = u64_at(file, idx + 8);
        let tag_count = u32_at(file, idx + 16) as usize;
        let tag_ptr = u64_at(file, idx + 24);
        if tag_count != header.tag_count as usize {
            return Err(Error::Invalid(format!(
                "tag index holds {tag_count} tags but the header says {}",
                header.tag_count
            )));
        }

        let class_off = header.tag_address_to_file(class_ptr)? as usize;
        bounds(file, "tag class table", class_off, class_count * 16)?;
        let classes = (0..class_count)
            .map(|i| {
                let o = class_off + i * 16;
                TagClass {
                    code: FourCc::from_class_id(u32_at(file, o)),
                    parent: FourCc::from_class_id(u32_at(file, o + 4)),
                    grandparent: FourCc::from_class_id(u32_at(file, o + 8)),
                    name: StringId(u32_at(file, o + 12)),
                }
            })
            .collect();

        let name_index =
            header.section_file_offset(SECTION_DEBUG, header.tag_name_index) as usize;
        let name_table =
            header.section_file_offset(SECTION_DEBUG, header.tag_name_table) as usize;
        let name_table_len = header.tag_name_table_size as usize;
        bounds(file, "tag name index", name_index, tag_count * 4)?;
        bounds(file, "tag name table", name_table, name_table_len)?;
        let names = &file[name_table..name_table + name_table_len];

        let tag_off = header.tag_address_to_file(tag_ptr)? as usize;
        bounds(file, "tag table", tag_off, tag_count * 8)?;
        let entries = (0..tag_count)
            .map(|i| {
                let o = tag_off + i * 8;
                let class_index = u16_at(file, o) as i16;
                if class_index < 0 || class_index as usize >= class_count {
                    return None;
                }
                let name_off = u32_at(file, name_index + i * 4) as i32;
                let name = (name_off >= 0 && (name_off as usize) < names.len()).then(|| {
                    let s = &names[name_off as usize..];
                    let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                    String::from_utf8_lossy(&s[..end]).into_owned()
                });
                Some(TagEntry {
                    index: i as u32,
                    class_index: class_index as u16,
                    salt: u16_at(file, o + 2),
                    meta_pointer: u32_at(file, o + 4),
                    name,
                })
            })
            .collect();
        Ok(TagIndex { classes, entries })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn get(&self, index: u32) -> Option<&TagEntry> {
        self.entries.get(index as usize).and_then(|e| e.as_ref())
    }

    pub fn class_of(&self, tag: &TagEntry) -> &TagClass {
        &self.classes[tag.class_index as usize]
    }

    pub fn iter(&self) -> impl Iterator<Item = &TagEntry> {
        self.entries.iter().filter_map(|e| e.as_ref())
    }

    pub fn of_class(&self, code: &str) -> impl Iterator<Item = &TagEntry> {
        let code = FourCc::from_code(code);
        self.iter().filter(move |t| self.class_of(t).code == code)
    }

    /// The tag of class `code` named `name`, compared without case.
    pub fn find(&self, code: &str, name: &str) -> Option<&TagEntry> {
        self.of_class(code)
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }
}
