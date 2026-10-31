//! Reads Halo 3 cache files (`.map`) from the user's own MCC install.
//!
//! Everything is read-only: the file is memory-mapped and never written. The
//! layout was worked out from the MCC Halo 3 update 13 files on disk and from
//! public format descriptions; no code from other projects is copied.
//!
//! ```no_run
//! let cache = blam_cache::CacheFile::open("riverworld.map")?;
//! for tag in cache.tags.of_class("sbsp") {
//!     println!("{} {}", cache.tags.class_of(tag).code, tag.name());
//! }
//! # Ok::<(), blam_cache::Error>(())
//! ```

pub mod error;
pub mod header;
pub mod reader;
pub mod resources;
pub mod strings;
pub mod tags;

use std::path::{Path, PathBuf};

pub use error::{Error, Result};
pub use header::{Engine, Header};
pub use reader::{Block, DataRef, Reader, TagRef};
pub use resources::{
    CacheAddress, Page, ResourceData, ResourceEntry, ResourceId, Resources, Segment,
};
pub use strings::{StringId, StringIdLayout, StringTable};
pub use tags::{FourCc, TagClass, TagEntry, TagIndex};

/// A fixed-width, NUL-terminated string at `offset`, at most `max` bytes.
pub fn header_cstr(bytes: &[u8], offset: usize, max: usize) -> String {
    if offset >= bytes.len() {
        return String::new();
    }
    header::cstr_at(bytes, offset, max)
}

/// Builds of MCC Halo 3 this crate knows. The tag layouts in the crates
/// above this one are written for the newest.
pub const KNOWN_BUILDS: &[&str] = &[
    "May 22 2023 20:10:02",
    "Jun 27 2023 08:39:36",
    "Sep  1 2023 11:01:27",
    "Sep  1 2023 11:01:50",
    "Dec 21 2023 22:31:37",
    "Jan  3 2024 10:08:58",
    "Jan  3 2024 10:01:45",
    // MCC Halo: Reach update 13 (`blam_reach`).
    "Jun 21 2023 15:35:31",
];

/// The game a cache file at `path` was written for, from its header alone;
/// `None` when it is not a Halo 3 / Reach cache file (Halo 2's MCC maps say
/// `head` 13 too, but have no section table: check those first).
pub fn engine_of(path: impl AsRef<Path>) -> Option<Engine> {
    use std::io::Read;
    let mut b = vec![0u8; header::HEADER_SIZE];
    std::fs::File::open(path).ok()?.read_exact(&mut b).ok()?;
    Header::parse(&b).ok().map(|h| h.engine)
}

pub struct CacheFile {
    path: PathBuf,
    map: memmap2::Mmap,
    pub header: Header,
    pub strings: StringTable,
    pub tags: TagIndex,
}

impl CacheFile {
    pub fn open(path: impl AsRef<Path>) -> Result<CacheFile> {
        let path = path.as_ref().to_path_buf();
        let io = |source| Error::Io {
            path: path.clone(),
            source,
        };
        let file = std::fs::File::open(&path).map_err(io)?;
        // SAFETY: the game's map files are opened read-only and are not
        // modified while the game is not running; a concurrent modification
        // could only produce garbage reads, which every parser here bounds
        // checks.
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(io)?;
        let header = Header::parse(&map)?;
        let layout = StringIdLayout::MCC_HALO3_U13;
        let strings = StringTable::parse(&map, &header, layout)?;
        let tags = TagIndex::parse(&map, &header)?;
        Ok(CacheFile {
            path,
            map,
            header,
            strings,
            tags,
        })
    }

    /// Halo 3 or Reach.
    pub fn engine(&self) -> Engine {
        self.header.engine
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The whole file.
    pub fn bytes(&self) -> &[u8] {
        &self.map
    }

    /// Whether the build string is one this crate was written against.
    pub fn is_known_build(&self) -> bool {
        KNOWN_BUILDS.contains(&self.header.build.as_str())
    }

    pub fn string(&self, id: StringId) -> Option<&str> {
        self.strings.get(id)
    }

    pub fn class_name(&self, class: &TagClass) -> &str {
        self.strings.get(class.name).unwrap_or("")
    }

    /// The scenario tag: the `scnr` whose name is the header's scenario path.
    pub fn scenario(&self) -> Option<&TagEntry> {
        self.tags.find("scnr", &self.header.scenario_path)
    }

    /// The single tag of a class the engine looks up by class alone
    /// (`matg`, `play`, `zone`, `ugh!`).
    pub fn global_tag(&self, code: &str) -> Option<&TagEntry> {
        let mut it = self.tags.of_class(code);
        let first = it.next()?;
        it.next().is_none().then_some(first)
    }

    /// A cursor at `file_offset`.
    pub fn reader_at(&self, file_offset: usize) -> Reader<'_> {
        Reader::new(self, file_offset)
    }

    /// File offset of a tag's data.
    pub fn tag_offset(&self, tag: &TagEntry) -> Result<usize> {
        Ok(self.header.packed_pointer_to_file(tag.meta_pointer)? as usize)
    }

    /// A cursor at the start of a tag's data.
    pub fn tag_reader(&self, tag: &TagEntry) -> Result<Reader<'_>> {
        Ok(self.reader_at(self.tag_offset(tag)?))
    }

    /// The map's resource tables.
    pub fn resources(&self) -> Result<Resources<'_>> {
        Resources::new(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The player's riverworld.map, when MCC is installed (`game_paths`).
    fn riverworld() -> Option<CacheFile> {
        let path = game_paths::halo3_map("riverworld")?;
        Some(CacheFile::open(path).expect("riverworld.map opens"))
    }

    #[test]
    fn string_id_layout_splits_bits() {
        let l = StringIdLayout::MCC_HALO3_U13;
        let id = StringId((7 << 27) | (3 << 19) | 0x1234);
        assert_eq!(l.index(id), 0x1234);
        assert_eq!(l.namespace(id), 3);
        assert_eq!(l.length(id), 7);
    }

    #[test]
    fn riverworld_header_and_tables() {
        let Some(cache) = riverworld() else { return };
        let h = &cache.header;
        assert_eq!(h.version, 13);
        assert_eq!(h.build, "Dec 21 2023 22:31:37");
        assert_eq!(h.map_name, "riverworld");
        assert_eq!(h.scenario_path, r"levels\multi\riverworld\riverworld");
        assert_eq!(h.tag_count, 11658);
        assert_eq!(h.string_count, 18806);
        assert_eq!(h.string_namespace_count, 10);
        assert_eq!(h.section_file_base(header::SECTION_DEBUG), 0x4000);
        assert_eq!(h.section_file_base(header::SECTION_RESOURCE), 0x114000);
        assert_eq!(h.section_file_base(header::SECTION_TAG), 0x0575_3000);
        assert_eq!(h.section_file_base(header::SECTION_LOCALE), 0x093D_3000);
        assert_eq!(
            h.section_file_base(header::SECTION_LOCALE) + h.sections[3].size as u64,
            h.file_size as u64
        );
        assert_eq!(h.tag_address_to_file(h.tag_index_pointer).unwrap(), 0x0591_CAF0);

        assert_eq!(cache.tags.classes.len(), 183);
        assert_eq!(cache.tags.len(), 11658);
        assert_eq!(cache.strings.at(1), Some("default"));
        assert_eq!(cache.strings.at(2), Some("reload_1"));
        let scnr = cache.scenario().expect("scenario tag");
        assert_eq!(cache.tags.class_of(scnr).code, "scnr");
        let sbsp: Vec<_> = cache.tags.of_class("sbsp").collect();
        assert_eq!(sbsp.len(), 1);
        assert_eq!(sbsp[0].name(), r"levels\multi\riverworld\riverworld");
        // Every class name is a string id in a namespace above zero, so this
        // exercises the namespace table too.
        for class in &cache.tags.classes {
            assert!(
                !cache.class_name(class).is_empty(),
                "class {} has no name (string id {:#x})",
                class.code,
                class.name.0
            );
        }
        assert_eq!(
            cache.class_name(cache.tags.class_of(scnr)),
            "scenario"
        );
    }
}
