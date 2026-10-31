//! A `.dx9.database` / `.dx9.package` pair: the database lists the assets
//! (`Packages/Package(name, offset)/Asset(name, type)/Heap(offset, size)`,
//! plus a `Compress` child when packed), the package holds their bytes at
//! `Package.offset + Heap.offset`.
//!
//! A packed asset is a u32 giving where the chunks end, then chunks of
//! `{u32 size, u32 packed size, bytes}` (each 64 KiB once unpacked; stored as
//! is when the two sizes match), then a u32 with the real size. Stock files
//! often give only the bytes used in the last chunk there. Packed chunks are
//! Xbox XMem LZX with a 128 KiB window in XNB-style frames: `FF hi lo hi lo`
//! (frame size, block size) or `hi lo` (block size, a 32 KiB frame).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::Error;
use crate::bxml::Bxml;

const CHUNK: usize = 0x10000;

#[derive(Clone, Debug)]
pub struct Asset {
    /// The `Package` it sits in (one model, one colourway, ...).
    pub subpackage: String,
    pub name: String,
    /// `surface`, `model`, `texture`, `material`, ...
    pub kind: String,
    offset: u64,
    size: usize,
    compressed: bool,
}

pub struct Package {
    file: std::sync::Mutex<File>,
    pub assets: Vec<Asset>,
}

impl Package {
    /// `<dir>/<stem>.dx9.database` and its `.dx9.package`, read-only.
    pub fn open(dir: &Path, stem: &str) -> Result<Package, Error> {
        let database = std::fs::read(dir.join(format!("{stem}.dx9.database")))?;
        let db = Bxml::parse(&database)?;
        let file = File::open(dir.join(format!("{stem}.dx9.package")))?;
        let mut assets = Vec::new();
        if let Some(list) = db.find(0, "Packages") {
            for package in db.find_all(list, "Package") {
                let base = db.attr_u64(package, "offset").unwrap_or(0);
                let subpackage = db.attr_str(package, "name").unwrap_or_default().to_string();
                for asset in db.find_all(package, "Asset") {
                    let Some(heap) = db.find(asset, "Heap") else {
                        continue;
                    };
                    assets.push(Asset {
                        subpackage: subpackage.clone(),
                        name: db.attr_str(asset, "name").unwrap_or_default().to_string(),
                        kind: db.attr_str(asset, "type").unwrap_or_default().to_string(),
                        offset: base + db.attr_u64(heap, "offset").unwrap_or(0),
                        size: db.attr_u64(heap, "size").unwrap_or(0) as usize,
                        compressed: db.find(asset, "Compress").is_some(),
                    });
                }
            }
        }
        Ok(Package {
            file: std::sync::Mutex::new(file),
            assets,
        })
    }

    pub fn find(&self, subpackage: &str, name: &str, kind: &str) -> Option<&Asset> {
        self.assets
            .iter()
            .find(|a| a.subpackage == subpackage && a.name == name && a.kind == kind)
    }

    /// An asset's bytes, unpacked.
    pub fn read(&self, asset: &Asset) -> Result<Vec<u8>, Error> {
        let mut blob = vec![0; asset.size];
        {
            let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
            f.seek(SeekFrom::Start(asset.offset))?;
            f.read_exact(&mut blob)?;
        }
        if asset.compressed { unpack(&blob) } else { Ok(blob) }
    }

    /// The asset `name` of `kind` in `subpackage`, unpacked.
    pub fn get(&self, subpackage: &str, name: &str, kind: &str) -> Result<Vec<u8>, Error> {
        let a = self
            .find(subpackage, name, kind)
            .ok_or_else(|| Error::Missing(format!("{subpackage}/{name}.{kind}")))?;
        self.read(a)
    }
}

fn u32_at(b: &[u8], p: usize) -> Result<usize, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()) as usize)
        .ok_or_else(|| Error::Format(format!("packed asset ends at {p}")))
}

/// A packed asset's bytes.
pub fn unpack(blob: &[u8]) -> Result<Vec<u8>, Error> {
    let end = u32_at(blob, 0)?;
    let mut out = Vec::new();
    let mut p = 4;
    while p < end {
        let (size, packed) = (u32_at(blob, p)?, u32_at(blob, p + 4)?);
        p += 8;
        let data = blob
            .get(p..p + packed)
            .ok_or_else(|| Error::Format("packed chunk cut short".into()))?;
        p += packed;
        if packed == size {
            out.extend_from_slice(data);
        } else {
            let start = out.len();
            lzx_chunk(data, size, &mut out)?;
            out.resize(start + size, 0);
        }
    }
    let mut real = u32_at(blob, end)?;
    let chunks = out.len() / CHUNK;
    if chunks > 1 && real <= (chunks - 1) * CHUNK {
        real += (chunks - 1) * CHUNK;
    }
    out.truncate(real.min(out.len()));
    Ok(out)
}

/// One 64 KiB chunk: its own LZX stream, in frames.
fn lzx_chunk(data: &[u8], size: usize, out: &mut Vec<u8>) -> Result<(), Error> {
    let mut lzx = lzxd::Lzxd::new(lzxd::WindowSize::KB128);
    let (mut q, mut got) = (0, 0);
    let byte = |i: usize| data.get(i).copied().map(usize::from).ok_or_else(|| Error::Format("LZX frame header".into()));
    while q < data.len() && got < size {
        let (frame, block) = if data[q] == 0xFF {
            let f = (byte(q + 1)? << 8) | byte(q + 2)?;
            let b = (byte(q + 3)? << 8) | byte(q + 4)?;
            q += 5;
            (f, b)
        } else {
            let b = (byte(q)? << 8) | byte(q + 1)?;
            q += 2;
            (0x8000, b)
        };
        if frame == 0 || block == 0 {
            break;
        }
        let input = data.get(q..q + block).ok_or_else(|| Error::Format("LZX block cut short".into()))?;
        let d = lzx
            .decompress_next(input, frame)
            .map_err(|e| Error::Format(format!("LZX: {e:?}")))?;
        out.extend_from_slice(d);
        got += frame;
        q += block;
    }
    Ok(())
}
