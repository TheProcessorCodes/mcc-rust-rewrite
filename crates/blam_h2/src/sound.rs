//! Halo 2's map ambience: the structure BSP's background sounds, the
//! scenario's sound scenery, their looping sounds (`lsnd`) and sounds
//! (`snd!`), and the samples behind them, decoded to 16-bit PCM.
//! `docs/format/halo2.md` section 11 has the layouts.
//!
//! MCC keeps the samples out of the `.map`: a sound's permutation names
//! chunks of `sounds_neutral.dat` (or `sounds_remastered.dat` for the
//! remastered set) beside the map, through the sound gestalt (`ugh!`) of the
//! file holding the `snd!` tag. The classic sounds are Xbox ADPCM (IMA ADPCM
//! in 36-byte blocks per channel of 64 samples), decoded here.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use glam::Vec3;

use crate::cache::{Map, Reader};
use crate::{Error, Result};

/// Decoded audio: interleaved 16-bit samples.
#[derive(Clone, Debug)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
    /// Root mean square over every sample, 0..1 of full scale.
    pub fn rms(&self) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }
        let sum: f64 = self.samples.iter().map(|&s| (s as f64 / 32768.0).powi(2)).sum();
        (sum / self.samples.len() as f64).sqrt() as f32
    }
}

/// Where a chunk's bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundFile {
    Neutral,
    Remastered,
}

/// A run of a permutation's samples in a `sounds_*.dat` file.
#[derive(Clone, Copy, Debug)]
pub struct Chunk {
    pub file: SoundFile,
    pub offset: u32,
    /// Stored (encoded) bytes.
    pub size: u32,
    /// Bytes once decoded to 16-bit PCM.
    pub pcm_bytes: u32,
}

#[derive(Clone, Debug)]
pub struct Permutation {
    pub name: String,
    /// 0..1: how often a random pick skips this permutation (1: only ever
    /// played first).
    pub skip: f32,
    pub chunks: Vec<Chunk>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compression {
    BigEndian,
    XboxAdpcm,
    ImaAdpcm,
    LittleEndian,
    Wma,
    Opus,
    Other(u8),
}

/// A `snd!` tag with what its gestalt says about it.
#[derive(Clone, Debug)]
pub struct Sound {
    pub id: u32,
    pub name: String,
    /// `snd!` flags: 0x80 per-language, 0x400 legacy only, 0x800 remastered only.
    pub flags: u16,
    pub class: u8,
    pub rate: u32,
    pub channels: u16,
    pub compression: Compression,
    /// Playback parameters: full volume within, silent beyond (world units).
    pub distance: (f32, f32),
    pub skip_fraction: f32,
    pub gain_db: f32,
    pub permutations: Vec<Permutation>,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub name: String,
    pub gain_db: f32,
    pub looped: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct Detail {
    pub name: String,
    pub sound: Option<u32>,
    /// Seconds between plays.
    pub period: (f32, f32),
    pub gain_db: f32,
    /// Radians.
    pub yaw: (f32, f32),
    pub pitch: (f32, f32),
    /// World units from the listener (or the sound scenery).
    pub distance: (f32, f32),
}

/// A looping sound (`lsnd`).
#[derive(Clone, Debug)]
pub struct Looping {
    pub id: u32,
    pub name: String,
    pub flags: u32,
    /// World units: full volume within, silent beyond.
    pub distance: (f32, f32),
    pub tracks: Vec<Track>,
    pub details: Vec<Detail>,
}

/// One entry of a structure BSP's background sound palette.
#[derive(Clone, Debug)]
pub struct Background {
    pub name: String,
    pub sound: Option<u32>,
    pub inside: Option<u32>,
    /// How many of the BSP's clusters use this entry.
    pub clusters: usize,
}

/// A sound scenery placement and the looping sound it carries.
#[derive(Clone, Debug)]
pub struct Emitter {
    pub position: Vec3,
    pub object: String,
    pub looping: u32,
}

/// The map's ambience, as tags.
#[derive(Clone, Debug, Default)]
pub struct Ambience {
    pub backgrounds: Vec<Background>,
    pub emitters: Vec<Emitter>,
}

fn i8_at(r: &Reader<'_>, off: usize) -> Result<i8> {
    Ok(r.u8(off)? as i8)
}

fn reference(id: u32) -> Option<u32> {
    (id != u32::MAX).then_some(id)
}

/// The BSP background sounds (palette entries some cluster uses) and the
/// scenario's sound scenery with their looping sounds.
pub fn ambience(map: &Map) -> Result<Ambience> {
    let scnr = map
        .first_of(b"scnr")
        .and_then(|t| map.tag_data(t.id))
        .ok_or_else(|| Error::Invalid("no scenario".into()))?;
    let mut out = Ambience::default();
    for bsp in scnr.block(0x210, 0x44)? {
        let Some(sbsp) = map.tag_data(bsp.tag_ref(0x10)?) else {
            continue;
        };
        let palette = sbsp.block(0xD4, 0x64)?;
        let mut used = vec![0usize; palette.len()];
        for c in sbsp.block(0x9C, 0xB0)? {
            if let Some(n) = usize::try_from(c.i16(0x72)?).ok().and_then(|i| used.get_mut(i)) {
                *n += 1;
            }
        }
        for (e, n) in palette.iter().zip(used) {
            out.backgrounds.push(Background {
                name: cstr(e, 0x20),
                sound: reference(e.tag_ref(0x20)?),
                inside: reference(e.tag_ref(0x28)?),
                clusters: n,
            });
        }
    }
    let palette = scnr.block(0xE0, 0x28)?;
    for placed in scnr.block(0xD8, 0x50)? {
        let Some(object) = usize::try_from(placed.i16(0)?).ok().and_then(|i| palette.get(i)) else {
            continue;
        };
        let ssce = object.tag_ref(0)?;
        let Some(o) = map.tag_data(ssce) else {
            continue;
        };
        // Object attachments (0x18 each): the looping sound is one.
        for a in o.block(0x94, 0x18)? {
            let id = a.tag_ref(0)?;
            if map.tag(id).is_some_and(|t| t.is(b"lsnd")) {
                out.emitters.push(Emitter {
                    position: placed.vec3(0x08)?,
                    object: map.tag_name(ssce).unwrap_or_default().to_string(),
                    looping: id,
                });
            }
        }
    }
    Ok(out)
}

fn cstr(r: &Reader<'_>, max: usize) -> String {
    let b = r.map.bytes.get(r.at..r.at + max).unwrap_or(&[]);
    let b = &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())];
    String::from_utf8_lossy(b).into_owned()
}

/// A looping sound's tracks and details.
pub fn looping(map: &Map, id: u32) -> Result<Looping> {
    let r = map.tag_data(id).ok_or_else(|| Error::Invalid(format!("lsnd {id:#x} not found")))?;
    let s = |sid: u32| r.map.string(sid).to_string();
    let mut tracks = Vec::new();
    for t in r.block(0x24, 0x58)? {
        tracks.push(Track {
            name: s(t.u32(0)?),
            gain_db: t.f32(0x08)?,
            looped: reference(t.tag_ref(0x1C)?),
        });
    }
    let mut details = Vec::new();
    for d in r.block(0x2C, 0x34)? {
        details.push(Detail {
            name: s(d.u32(0)?),
            sound: reference(d.tag_ref(0x04)?),
            period: (d.f32(0x0C)?, d.f32(0x10)?),
            gain_db: d.f32(0x14)?,
            yaw: (d.f32(0x1C)?, d.f32(0x20)?),
            pitch: (d.f32(0x24)?, d.f32(0x28)?),
            distance: (d.f32(0x2C)?, d.f32(0x30)?),
        });
    }
    Ok(Looping {
        id,
        name: map.tag_name(id).unwrap_or_default().to_string(),
        flags: r.u32(0x08)?,
        distance: (r.f32(0x14)?, r.f32(0x18)?),
        tracks,
        details,
    })
}

/// A sound tag, read through the gestalt (`ugh!`) of the file holding it.
pub fn sound(map: &Map, id: u32) -> Result<Sound> {
    let r = map.tag_data(id).ok_or_else(|| Error::Invalid(format!("snd! {id:#x} not found")))?;
    let file = r.map;
    let ugh = file
        .tags
        .iter()
        .find(|t| t.is(b"ugh!") && t.offset.is_some())
        .and_then(|t| file.tag_data(t.id))
        .ok_or_else(|| Error::Invalid(format!("{} has no sound gestalt", file.map_name)))?;
    let pick = |off: usize, size: usize, index: i64| -> Result<Reader<'_>> {
        let list = ugh.block(off, size)?;
        usize::try_from(index)
            .ok()
            .and_then(|i| list.get(i).copied())
            .ok_or_else(|| Error::Invalid(format!("gestalt block {off:#x}: index {index} of {}", list.len())))
    };
    let codec = pick(0x00, 3, i8_at(&r, 0x03)? as i64)?;
    let rate = match codec.u8(0)? {
        0 => 22_050,
        1 => 44_100,
        2 => 32_000,
        _ => 48_000,
    };
    let channels = match codec.u8(1)? {
        0 => 1,
        1 => 2,
        3 => 4,
        _ => 6,
    };
    let compression = match codec.u8(2)? {
        0 => Compression::BigEndian,
        1 => Compression::XboxAdpcm,
        2 => Compression::ImaAdpcm,
        3 => Compression::LittleEndian,
        4 => Compression::Wma,
        5 => Compression::Opus,
        n => Compression::Other(n),
    };
    let (mut distance, mut skip_fraction, mut gain_db) = ((0.0, 0.0), 0.0, 0.0);
    let playback = r.i16(0x06)?;
    if playback >= 0 {
        let p = pick(0x08, 0x38, playback as i64)?;
        distance = (p.f32(0x00)?, p.f32(0x04)?);
        skip_fraction = p.f32(0x08)?;
        gain_db = p.f32(0x10)?;
    }
    let names = ugh.block(0x18, 4)?;
    let languages = ugh.block(0x60, 8)?;
    // Every language lists the same chunks for a language-neutral sound;
    // the first (English) is read.
    let chunks = match languages.first() {
        Some(l) => l.block(0, 0x10)?,
        None => Vec::new(),
    };
    let ranges = ugh.block(0x28, 0x0C)?;
    let perms = ugh.block(0x30, 0x10)?;
    let flags = r.u16(0x00)?;
    // Flag 0x800 "play only in remastered mode": the remastered set, in
    // `sounds_remastered.dat`; every other language-neutral sound is in
    // `sounds_neutral.dat` (checked on every Xbox ADPCM chunk of lockout and
    // shared.map, `probe_h2_sound.py`). Flag 0x80 "pitch range is language"
    // would mean one file per language, not read here.
    let file = if flags & 0x800 != 0 {
        SoundFile::Remastered
    } else {
        SoundFile::Neutral
    };
    let first_range = r.i16(0x08)?;
    let range_count = i8_at(&r, 0x0A)?;
    let mut permutations = Vec::new();
    for k in 0..range_count.max(0) as i64 {
        let Some(range) = usize::try_from(first_range as i64 + k).ok().and_then(|i| ranges.get(i)) else {
            return Err(Error::Invalid(format!("{}: pitch range {} of {}", map.tag_name(id).unwrap_or("?"), first_range as i64 + k, ranges.len())));
        };
        let (first, count) = (range.i16(0x08)?.max(0) as usize, range.i16(0x0A)?.max(0) as usize);
        for p in perms.get(first..first + count).unwrap_or(&[]) {
            let name = usize::try_from(p.i16(0)?)
                .ok()
                .and_then(|i| names.get(i))
                .map(|n| n.map.string(n.u32(0).unwrap_or(0)).to_string())
                .unwrap_or_default();
            let skip = p.i16(0x02)? as f32 / 32767.0;
            let mut list = Vec::new();
            if let Some(info) = p.block(0x08, 8)?.first() {
                let (first, count) = (info.i16(4)?.max(0) as usize, info.i16(6)?.max(0) as usize);
                for c in chunks.get(first..first + count).unwrap_or(&[]) {
                    let word = c.u32(4)?;
                    // The top byte is flags (0x20: the permutation's last
                    // chunk); the file follows from the sound's own flags.
                    list.push(Chunk {
                        file,
                        offset: c.u32(0)?,
                        size: word & 0x00FF_FFFF,
                        pcm_bytes: c.u32(8)?,
                    });
                }
            }
            permutations.push(Permutation { name, skip, chunks: list });
        }
    }
    Ok(Sound {
        id,
        name: map.tag_name(id).unwrap_or_default().to_string(),
        flags,
        class: r.u8(0x02)?,
        rate,
        channels,
        compression,
        distance,
        skip_fraction,
        gain_db,
        permutations,
    })
}

/// The `sounds_*.dat` files beside a map, mapped on first use.
pub struct SoundFiles {
    dir: PathBuf,
    neutral: OnceLock<Option<memmap2::Mmap>>,
    remastered: OnceLock<Option<memmap2::Mmap>>,
}

impl SoundFiles {
    pub fn beside(map: &Path) -> SoundFiles {
        SoundFiles {
            dir: map.parent().map(Path::to_path_buf).unwrap_or_default(),
            neutral: OnceLock::new(),
            remastered: OnceLock::new(),
        }
    }

    fn bytes(&self, file: SoundFile) -> Result<&[u8]> {
        let (slot, name) = match file {
            SoundFile::Neutral => (&self.neutral, "sounds_neutral.dat"),
            SoundFile::Remastered => (&self.remastered, "sounds_remastered.dat"),
        };
        slot.get_or_init(|| {
            let f = File::open(self.dir.join(name)).ok()?;
            // Read-only map of a file we never write; the game install is
            // not modified under us.
            unsafe { memmap2::Mmap::map(&f) }.ok()
        })
        .as_deref()
        .ok_or_else(|| Error::Invalid(format!("{name} not found beside the map")))
    }

    /// Decodes one permutation of a sound.
    pub fn decode(&self, sound: &Sound, permutation: usize) -> Result<Pcm> {
        let p = sound
            .permutations
            .get(permutation)
            .ok_or_else(|| Error::Invalid(format!("{}: no permutation {permutation}", sound.name)))?;
        if sound.flags & 0x80 != 0 {
            return Err(Error::Unsupported(format!("{}: per-language samples", sound.name)));
        }
        if sound.compression != Compression::XboxAdpcm {
            return Err(Error::Unsupported(format!("{}: {:?} samples", sound.name, sound.compression)));
        }
        let channels = sound.channels as usize;
        let mut samples = Vec::with_capacity(p.chunks.iter().map(|c| c.pcm_bytes as usize / 2).sum());
        for c in &p.chunks {
            let data = self.bytes(c.file)?;
            let bytes = data
                .get(c.offset as usize..c.offset as usize + c.size as usize)
                .ok_or_else(|| Error::Invalid(format!("{}: chunk at {:#x} runs past the file", sound.name, c.offset)))?;
            let before = samples.len();
            xbox_adpcm(bytes, channels, &mut samples)?;
            if (samples.len() - before) * 2 != c.pcm_bytes as usize {
                return Err(Error::Invalid(format!(
                    "{}: chunk at {:#x} decoded to {} bytes, the gestalt says {}",
                    sound.name,
                    c.offset,
                    (samples.len() - before) * 2,
                    c.pcm_bytes
                )));
            }
        }
        Ok(Pcm {
            rate: sound.rate,
            channels: sound.channels,
            samples,
        })
    }
}

/// The looping sounds the BSPs' clusters play as their background (each
/// once, in palette order).
pub fn background_loops(a: &Ambience) -> Vec<u32> {
    let mut out = Vec::new();
    for b in a.backgrounds.iter().filter(|b| b.clusters > 0) {
        for id in [b.sound, b.inside].into_iter().flatten() {
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

/// Writes 16-bit PCM WAV.
pub fn write_wav(path: &Path, pcm: &Pcm) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(File::create(path)?);
    let data = (pcm.samples.len() * 2) as u32;
    let ch = pcm.channels as u32;
    f.write_all(b"RIFF")?;
    f.write_all(&(36 + data).to_le_bytes())?;
    f.write_all(b"WAVEfmt ")?;
    f.write_all(&16u32.to_le_bytes())?;
    f.write_all(&1u16.to_le_bytes())?;
    f.write_all(&(ch as u16).to_le_bytes())?;
    f.write_all(&pcm.rate.to_le_bytes())?;
    f.write_all(&(pcm.rate * ch * 2).to_le_bytes())?;
    f.write_all(&((ch * 2) as u16).to_le_bytes())?;
    f.write_all(&16u16.to_le_bytes())?;
    f.write_all(b"data")?;
    f.write_all(&data.to_le_bytes())?;
    for &s in &pcm.samples {
        f.write_all(&s.to_le_bytes())?;
    }
    f.flush()
}

const STEPS: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130,
    143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282,
    1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132, 7845, 8630,
    9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const INDEX_STEP: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// Standard IMA ADPCM: one nibble against (predictor, step index).
fn ima(nibble: u8, predictor: &mut i32, index: &mut i32) -> i16 {
    let step = STEPS[*index as usize];
    let mut diff = step >> 3;
    if nibble & 1 != 0 {
        diff += step >> 2;
    }
    if nibble & 2 != 0 {
        diff += step >> 1;
    }
    if nibble & 4 != 0 {
        diff += step;
    }
    if nibble & 8 != 0 {
        *predictor -= diff;
    } else {
        *predictor += diff;
    }
    *predictor = (*predictor).clamp(-32768, 32767);
    *index = (*index + INDEX_STEP[(nibble & 7) as usize]).clamp(0, 88);
    *predictor as i16
}

/// Xbox ADPCM: blocks of 36 bytes per channel. Each channel's block starts
/// with a 4-byte header (i16 sample, u8 step index, u8 zero); then the
/// channels' data comes in turns of 4 bytes (8 samples, low nibble first),
/// 8 turns: 64 samples per channel per block. The header sample is the
/// first output sample and the decoder's start; the last nibble is not
/// output (64 samples, as the gestalt's decoded sizes count them).
pub fn xbox_adpcm(data: &[u8], channels: usize, out: &mut Vec<i16>) -> Result<()> {
    if channels == 0 {
        return Err(Error::Invalid("Xbox ADPCM with no channels".into()));
    }
    let block = 36 * channels;
    if data.len() % block != 0 {
        return Err(Error::Invalid(format!("Xbox ADPCM: {} bytes is not whole {block}-byte blocks", data.len())));
    }
    let mut frame = vec![[0i16; 64]; channels];
    for b in data.chunks_exact(block) {
        for (ch, f) in frame.iter_mut().enumerate() {
            let h = &b[4 * ch..4 * ch + 4];
            let mut predictor = i16::from_le_bytes([h[0], h[1]]) as i32;
            let mut index = h[2] as i32;
            if index > 88 || h[3] != 0 {
                return Err(Error::Invalid(format!("Xbox ADPCM: bad block header {h:02x?}")));
            }
            f[0] = predictor as i16;
            let mut n = 1;
            'turns: for turn in 0..8 {
                let at = 4 * channels + (turn * channels + ch) * 4;
                for &byte in &b[at..at + 4] {
                    for nibble in [byte & 0x0F, byte >> 4] {
                        if n == 64 {
                            break 'turns;
                        }
                        f[n] = ima(nibble, &mut predictor, &mut index);
                        n += 1;
                    }
                }
            }
        }
        for i in 0..64 {
            for f in &frame {
                out.push(f[i]);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silent_block_decodes_to_64_samples() {
        // Header: sample 100, index 0. Nibble 0 at step 7 adds 7 >> 3 = 0;
        // nibble 7 adds 7/8 + 7/4 + 7/2 + 7 of the step and raises the index.
        let mut block = vec![0u8; 36];
        block[0] = 100;
        let mut out = Vec::new();
        xbox_adpcm(&block, 1, &mut out).unwrap();
        assert_eq!(out.len(), 64);
        assert!(out.iter().all(|&v| v == 100));
        block[4..].fill(0x77);
        out.clear();
        xbox_adpcm(&block, 1, &mut out).unwrap();
        assert_eq!((out[0], out[1]), (100, 100 + 0 + 1 + 3 + 7));
        assert!(out.windows(2).all(|w| w[1] >= w[0]));
    }

    #[test]
    fn stereo_interleaves_channels() {
        let mut block = vec![0u8; 72];
        block[0] = 10;
        block[4] = 0xF6; // -10
        block[5] = 0xFF;
        let mut out = Vec::new();
        xbox_adpcm(&block, 2, &mut out).unwrap();
        assert_eq!(out.len(), 128);
        assert_eq!((out[0], out[1]), (10, -10));
    }

    #[test]
    fn rejects_bad_header() {
        let mut block = vec![0u8; 36];
        block[2] = 89;
        assert!(xbox_adpcm(&block, 1, &mut Vec::new()).is_err());
    }
}
