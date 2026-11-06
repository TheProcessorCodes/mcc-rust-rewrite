//! Reads files out of an Xbox 360 disc image (XDVDFS, "XISO"), read-only.
//!
//! The format, in our own words:
//!
//! - Sectors are 2048 bytes. The game partition starts at 0 in a trimmed
//!   image (extract-xiso, XISO), or after the disc's video partition in a
//!   full dump: 0xFD90000 bytes on XGD2 discs, 0x2080000 on XGD3, 0x18300000
//!   on original Xbox (XGD1) discs.
//! - Sector 32 of the partition is the volume descriptor: the 20 bytes
//!   `MICROSOFT*XBOX*MEDIA`, then the root directory's first sector (u32 LE)
//!   and its size in bytes (u32 LE).
//! - A directory is a table of entries forming a binary search tree, the
//!   root entry first. An entry is: left and right subtree offsets (u16 LE
//!   each, in 4-byte words from the table start, 0 for none), the first
//!   sector (u32 LE), the size (u32 LE), attributes (u8; 0x10 is a
//!   directory), the name length (u8), then the name. A table that starts
//!   0xFFFF is an empty directory.
//! - Sector numbers are relative to the partition start.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

const SECTOR: u64 = 2048;
const MAGIC: &[u8; 20] = b"MICROSOFT*XBOX*MEDIA";
/// Where the game partition can start, most likely first.
const PARTITIONS: [u64; 4] = [0, 0xFD9_0000, 0x208_0000, 0x1830_0000];
const DIRECTORY: u8 = 0x10;

/// An open disc image.
pub struct Xiso {
    file: File,
    base: u64,
    root: Entry,
}

/// A file or directory on the disc.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub sector: u32,
    pub size: u32,
    pub directory: bool,
}

impl Xiso {
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut file = File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        for base in PARTITIONS {
            if base + 33 * SECTOR > len {
                continue;
            }
            let mut head = [0u8; 28];
            file.seek(SeekFrom::Start(base + 32 * SECTOR)).map_err(|e| e.to_string())?;
            file.read_exact(&mut head).map_err(|e| e.to_string())?;
            if &head[..20] != MAGIC {
                continue;
            }
            let root = Entry {
                name: String::new(),
                sector: u32::from_le_bytes(head[20..24].try_into().unwrap()),
                size: u32::from_le_bytes(head[24..28].try_into().unwrap()),
                directory: true,
            };
            return Ok(Self { file, base, root });
        }
        Err(format!(
            "{} is not an Xbox 360 disc image (no XDVDFS volume found)",
            path.display()
        ))
    }

    /// The entries of a directory, in name order.
    pub fn list(&mut self, dir: &Entry) -> Result<Vec<Entry>, String> {
        if !dir.directory {
            return Err(format!("{} is not a directory", dir.name));
        }
        if dir.size == 0 {
            return Ok(Vec::new());
        }
        let mut table = vec![0u8; dir.size as usize];
        self.read_at(dir.sector, 0, &mut table)?;
        let mut out = Vec::new();
        // The tree, walked with a stack; `seen` stops a broken image looping.
        let mut stack = vec![0usize];
        let mut seen = std::collections::HashSet::new();
        while let Some(at) = stack.pop() {
            if !seen.insert(at) || at + 14 > table.len() {
                continue;
            }
            let u16_at = |o: usize| u16::from_le_bytes([table[at + o], table[at + o + 1]]);
            let left = u16_at(0);
            if left == 0xFFFF {
                continue;
            }
            let right = u16_at(2);
            let sector = u32::from_le_bytes(table[at + 4..at + 8].try_into().unwrap());
            let size = u32::from_le_bytes(table[at + 8..at + 12].try_into().unwrap());
            let attributes = table[at + 12];
            let len = table[at + 13] as usize;
            let Some(name) = table.get(at + 14..at + 14 + len) else { continue };
            out.push(Entry {
                name: String::from_utf8_lossy(name).into_owned(),
                sector,
                size,
                directory: attributes & DIRECTORY != 0,
            });
            for next in [left, right] {
                if next != 0 {
                    stack.push(next as usize * 4);
                }
            }
        }
        out.sort_by_key(|e| e.name.to_lowercase());
        Ok(out)
    }

    /// The entry at a `/`-separated path, matched without regard to case.
    pub fn find(&mut self, path: &str) -> Result<Entry, String> {
        let mut at = self.root.clone();
        for part in path.split('/').filter(|p| !p.is_empty()) {
            let entries = self.list(&at)?;
            at = entries
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(part))
                .ok_or_else(|| format!("the disc has no {path}"))?;
        }
        Ok(at)
    }

    /// Copies a file off the disc to `out`, reporting the bytes written.
    pub fn extract(&mut self, file: &Entry, out: &Path, progress: &mut dyn FnMut(u64)) -> Result<(), String> {
        if file.directory {
            return Err(format!("{} is a directory", file.name));
        }
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let mut dest = File::create(out).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        let mut buf = vec![0u8; 4 << 20];
        let mut done = 0u64;
        let total = u64::from(file.size);
        while done < total {
            let n = buf.len().min((total - done) as usize);
            self.read_at(file.sector, done, &mut buf[..n])?;
            dest.write_all(&buf[..n]).map_err(|e| format!("cannot write {}: {e}", out.display()))?;
            done += n as u64;
            progress(n as u64);
        }
        Ok(())
    }

    fn read_at(&mut self, sector: u32, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        self.file
            .seek(SeekFrom::Start(self.base + u64::from(sector) * SECTOR + offset))
            .and_then(|_| self.file.read_exact(buf))
            .map_err(|e| format!("disc image read failed: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays out a tiny trimmed image: `default.xex` and `data/big/db.big`.
    /// (A full dump's video partition would make it hundreds of megabytes;
    /// `matches_an_extracted_disc` covers that.)
    fn image(path: &Path) {
        let base = 0u64;
        let mut f = File::create(path).unwrap();
        let mut put = |at: u64, bytes: &[u8]| {
            f.seek(SeekFrom::Start(base + at)).unwrap();
            f.write_all(bytes).unwrap();
        };
        let entry = |left: u16, right: u16, sector: u32, size: u32, attr: u8, name: &str| {
            let mut e = Vec::new();
            e.extend(left.to_le_bytes());
            e.extend(right.to_le_bytes());
            e.extend(sector.to_le_bytes());
            e.extend(size.to_le_bytes());
            e.push(attr);
            e.push(name.len() as u8);
            e.extend(name.as_bytes());
            while e.len() % 4 != 0 {
                e.push(0xFF);
            }
            e
        };
        let mut head = MAGIC.to_vec();
        head.extend(40u32.to_le_bytes());
        head.extend(2048u32.to_le_bytes());
        put(32 * SECTOR, &head);
        // Root: "data" (dir, sector 41) with "default.xex" to its right.
        let data = entry(0, 0, 41, 2048, DIRECTORY, "data");
        let right = (data.len() / 4) as u16;
        let mut root = entry(0, right, 41, 2048, DIRECTORY, "data");
        root.extend(entry(0, 0, 43, 5, 0x80, "default.xex"));
        put(40 * SECTOR, &root);
        put(41 * SECTOR, &entry(0, 0, 42, 2048, DIRECTORY, "big"));
        put(42 * SECTOR, &entry(0, 0, 44, 3, 0x80, "DB.BIG"));
        put(43 * SECTOR, b"xex!!");
        put(44 * SECTOR, b"big");
        put(45 * SECTOR, &[0u8; 1]);
    }

    #[test]
    fn finds_and_extracts_files() {
        let dir = std::env::temp_dir().join(format!("mcc-xiso-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let iso = dir.join("test.iso");
        image(&iso);
        let mut disc = Xiso::open(&iso).unwrap();
        assert_eq!(disc.base, 0);
        let names: Vec<_> = disc.list(&disc.root.clone()).unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["data", "default.xex"]);
        let big = disc.find("data/big/db.big").unwrap();
        let out = dir.join("out").join("db.big");
        disc.extract(&big, &out, &mut |_| {}).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"big");
        let xex = disc.find("DEFAULT.XEX").unwrap();
        assert_eq!(xex.size, 5);
        assert!(disc.find("data/nothing").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A real disc, when one is named: `MCC_TEST_XISO=<iso>` and
    /// `MCC_TEST_XISO_DIR=<the same disc extracted>`. Every file in the
    /// image's root and `default.xex` must match the extracted copy.
    #[test]
    fn matches_an_extracted_disc() {
        let (Some(iso), Some(dir)) = (std::env::var_os("MCC_TEST_XISO"), std::env::var_os("MCC_TEST_XISO_DIR"))
        else {
            return;
        };
        let mut disc = Xiso::open(Path::new(&iso)).unwrap();
        eprintln!("partition at {:#x}", disc.base);
        let root = disc.list(&disc.root.clone()).unwrap();
        for e in &root {
            eprintln!("{} {} {}", if e.directory { "dir " } else { "file" }, e.size, e.name);
        }
        let xex = disc.find("default.xex").unwrap();
        let out = std::env::temp_dir().join(format!("mcc-xiso-real-{}.xex", std::process::id()));
        disc.extract(&xex, &out, &mut |_| {}).unwrap();
        let ours = std::fs::read(&out).unwrap();
        std::fs::remove_file(&out).ok();
        assert_eq!(ours, std::fs::read(Path::new(&dir).join("default.xex")).unwrap());
    }
}
