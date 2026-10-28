//! MCC Halo 3's FMOD sound bank: `halo3\fmod\pc\sfx.fsb` (FSB5) and its
//! `sfx.fsb.info` sidecar. `docs/format/sound.md` sections 1 and 2.
//!
//! The bank is memory-mapped read-only; a sample's bytes are borrowed from
//! the map.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

/// One encoded sound in the bank: a single `.aif` permutation of a Halo
/// sound tag.
#[derive(Clone, Debug)]
pub struct Sample {
    pub rate: u32,
    pub channels: u16,
    /// Frames (samples per channel) after decoding.
    pub frames: u32,
    /// Byte range in the bank's data section.
    pub offset: usize,
    pub size: usize,
    /// CRC FMOD keys the stripped Vorbis setup header by (chunk 11).
    pub setup_crc: Option<u32>,
    /// Vorbis "intra layers" (chunk 14): multichannel sounds stored as
    /// several interleaved streams. Not decoded.
    pub layers: Option<u32>,
    /// Loop points from the FSB (chunk 3), frames; none in Halo 3's bank.
    pub fsb_loop: Option<(u32, u32)>,
}

/// One record of `<bank>.fsb.info` (280 bytes), in the same order as the
/// bank's samples.
#[derive(Clone, Debug)]
pub struct Info {
    /// What a Halo permutation stores at `+0x10`: djb2 of the source path
    /// without `data\sound\` and `.aif` (see [`djb2`]).
    pub id: u32,
    /// Bytes of 16-bit PCM: frames x channels x 2.
    pub pcm_bytes: u32,
    /// Source channels (1, 2, 4 or 6).
    pub channels: u32,
    /// Last frame of the loop region for loop permutations, 0 for one-shots.
    pub loop_end: u32,
    /// The source file, e.g. `data\sound\levels\riverworld\...\weird_bird2.aif`.
    pub path: String,
}

/// The id hash of the bank's info records: djb2 (`h = h * 33 + byte`,
/// seeded with 5381) over the source path relative to `data\sound\` with no
/// extension, e.g. `characters\bodyfalls\bodythud\bodythud1`.
pub fn djb2(s: &str) -> u32 {
    s.bytes().fold(5381u32, |h, c| h.wrapping_mul(33).wrapping_add(c as u32))
}

pub struct Bank {
    path: PathBuf,
    map: memmap2::Mmap,
    data_base: usize,
    pub samples: Vec<Sample>,
    pub infos: Vec<Info>,
    by_id: HashMap<u32, u32>,
}

const RATES: [u32; 10] = [0, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000];

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::Invalid(format!("read past the end at {o:#x}")))
}

impl Bank {
    /// Opens `<path>` (an FSB5 file) and `<path>.info`.
    pub fn open(path: impl AsRef<Path>) -> Result<Bank> {
        let path = path.as_ref().to_path_buf();
        let io = |source| Error::Io {
            path: path.clone(),
            source,
        };
        let file = std::fs::File::open(&path).map_err(io)?;
        // SAFETY: read-only map of a game file nobody writes while we run;
        // every read below is bounds checked.
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(io)?;
        let mut info_path = path.clone().into_os_string();
        info_path.push(".info");
        let info_path = PathBuf::from(info_path);
        let info = std::fs::read(&info_path).map_err(|source| Error::Io {
            path: info_path.clone(),
            source,
        })?;
        let mut bank = Bank::parse(path, map)?;
        bank.infos = parse_info(&info)?;
        if bank.infos.len() != bank.samples.len() {
            return Err(Error::Invalid(format!(
                "{} info records for {} samples",
                bank.infos.len(),
                bank.samples.len()
            )));
        }
        bank.by_id = bank
            .infos
            .iter()
            .enumerate()
            .map(|(i, r)| (r.id, i as u32))
            .collect();
        Ok(bank)
    }

    fn parse(path: PathBuf, map: memmap2::Mmap) -> Result<Bank> {
        let b: &[u8] = &map;
        if b.get(0..4) != Some(b"FSB5") {
            return Err(Error::Invalid(format!("{} is not an FSB5 bank", path.display())));
        }
        let version = u32_at(b, 4)?;
        let count = u32_at(b, 8)? as usize;
        let headers = u32_at(b, 0x0C)? as usize;
        let names = u32_at(b, 0x10)? as usize;
        let data = u32_at(b, 0x14)? as usize;
        let mode = u32_at(b, 0x18)?;
        if mode != 0x0F {
            return Err(Error::Invalid(format!("bank codec {mode:#x}, not Vorbis (0xF)")));
        }
        let header_size = if version == 0 { 0x40 } else { 0x3C };
        let data_base = header_size + headers + names;
        if data_base + data > b.len() {
            return Err(Error::Invalid("bank shorter than its header says".into()));
        }
        let mut samples = Vec::with_capacity(count);
        let mut p = header_size;
        let end = header_size + headers;
        for _ in 0..count {
            let lo = u32_at(b, p)? as u64;
            let hi = u32_at(b, p + 4)? as u64;
            let v = lo | hi << 32;
            p += 8;
            let mut s = Sample {
                rate: RATES.get(((v >> 1) & 0xF) as usize).copied().unwrap_or(0),
                channels: if (v >> 5) & 1 != 0 { 2 } else { 1 },
                offset: (((v >> 6) & 0x0FFF_FFFF) * 16) as usize,
                frames: (v >> 34) as u32,
                size: 0,
                setup_crc: None,
                layers: None,
                fsb_loop: None,
            };
            let mut more = v & 1 != 0;
            while more {
                let c = u32_at(b, p)?;
                p += 4;
                more = c & 1 != 0;
                let size = ((c >> 1) & 0xFF_FFFF) as usize;
                let kind = c >> 25;
                if p + size > end {
                    return Err(Error::Invalid("sample chunk runs past the headers".into()));
                }
                match kind {
                    1 => s.channels = b[p] as u16,
                    2 => s.rate = u32_at(b, p)?,
                    3 => s.fsb_loop = Some((u32_at(b, p)?, u32_at(b, p + 4)?)),
                    11 => s.setup_crc = Some(u32_at(b, p)?),
                    14 => s.layers = Some(u32_at(b, p)?),
                    _ => {}
                }
                p += size;
            }
            samples.push(s);
        }
        for i in 0..samples.len() {
            let next = samples.get(i + 1).map_or(data, |n| n.offset);
            samples[i].size = next.saturating_sub(samples[i].offset);
        }
        Ok(Bank {
            path,
            map,
            data_base,
            samples,
            infos: Vec::new(),
            by_id: HashMap::new(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The sample a Halo permutation's FMOD id names.
    pub fn find(&self, id: u32) -> Option<u32> {
        self.by_id.get(&id).copied()
    }

    /// The encoded bytes of sample `i`.
    pub fn bytes(&self, i: u32) -> Option<&[u8]> {
        let s = self.samples.get(i as usize)?;
        let start = self.data_base + s.offset;
        self.map.get(start..start + s.size)
    }

    /// Every setup CRC the bank uses.
    pub fn setup_crcs(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.samples.iter().filter_map(|s| s.setup_crc).collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

fn parse_info(b: &[u8]) -> Result<Vec<Info>> {
    if !b.len().is_multiple_of(280) {
        return Err(Error::Invalid(format!("info file of {} bytes is not 280-byte records", b.len())));
    }
    let mut out = Vec::with_capacity(b.len() / 280);
    for r in b.as_chunks::<280>().0 {
        let path = &r[24..];
        let len = path.iter().position(|&c| c == 0).unwrap_or(path.len());
        out.push(Info {
            id: u32_at(r, 0)?,
            pcm_bytes: u32_at(r, 4)?,
            channels: u32_at(r, 8)?,
            loop_end: u32_at(r, 20)?,
            path: String::from_utf8_lossy(&path[..len]).into_owned(),
        });
    }
    Ok(out)
}

/// The id a source path would have, for checking [`Info::id`].
pub fn id_of_path(path: &str) -> u32 {
    let p = path.strip_prefix("data\\sound\\").unwrap_or(path);
    let p = p.rsplit_once('.').map_or(p, |(stem, _)| stem);
    djb2(p)
}
