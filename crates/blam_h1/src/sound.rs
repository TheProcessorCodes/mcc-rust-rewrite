//! Halo CE's sound in MCC: the `snd!` and `lsnd` tags in the map, and the
//! FMOD banks the audio lives in (`halo1\sound\pc\*.fsb`, found through
//! their `lst\*.lst.bin` indexes by tag path). `docs/format/halo1.md` 13
//! has the layouts; probe `probe_h1_sound.py`.
//!
//! MCC's Halo CE has no `sounds.map`: a permutation's sample data reference
//! in the tag points at nothing MCC ships. Instead each bank's index lists
//! tag paths, each with the run of bank samples that are that tag's
//! permutations; the samples are FMOD "CELT" (CELT 0.11.0, decoded by
//! `fsb_celt`), 44.1 kHz.
//!
//! ```no_run
//! let map = blam_h1::Map::open(std::path::Path::new("d40.map"))?;
//! let banks = blam_h1::sound::Banks::open(std::path::Path::new(r"...\halo1\sound\pc"), "english")?;
//! let tag = map.find(b"snd!", r"sound\music\perc\in").unwrap();
//! let ids = banks.samples_of(&tag.name);
//! let pcm = banks.decode(ids[0])?;
//! # Ok::<(), blam_h1::Error>(())
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::{Error, Map, Reader, Result};

// ---- tags ---------------------------------------------------------------

/// A `snd!` tag (the fields we use; `halo1.md` 13.1).
#[derive(Clone, Debug, Default)]
pub struct Sound {
    /// Tag index.
    pub index: u32,
    pub name: String,
    pub flags: u32,
    /// Sound class (`+0x04`, Halo CE's numbering: 32 music, 44 to 47
    /// scripted dialogue, 33 to 35 ambience, ...).
    pub class: u16,
    /// Sample rate of the original release (`+0x06`: 0 22.05 kHz, 1 44.1 kHz).
    pub rate: u32,
    /// (full volume within, silent beyond), world units (`+0x08`, `+0x0C`).
    pub distance: (f32, f32),
    /// Fraction of plays skipped (`+0x10`).
    pub skip: f32,
    /// Random pitch bounds (`+0x14`).
    pub random_pitch: (f32, f32),
    /// Linear gain (`+0x28`): 0.85 on The Maw's lines, 1 on music.
    pub gain: f32,
    /// Gain at script scale 0 and 1 (`+0x40`, `+0x58`): a script's scale
    /// goes between them.
    pub scale_gain: (f32, f32),
    /// 0 mono, 1 stereo (`+0x6C`).
    pub encoding: u16,
    /// 0 none, 1 Xbox ADPCM, 2 IMA ADPCM, 3 Ogg (`+0x6E`; the original
    /// release's encoding, not MCC's).
    pub compression: u16,
    pub pitch_ranges: Vec<PitchRange>,
}

impl Sound {
    /// Every permutation a play picks from (the first `actual` of each
    /// pitch range; the rest are their continuations).
    pub fn actual_permutations(&self) -> impl Iterator<Item = &Permutation> {
        self.pitch_ranges.iter().flat_map(|p| p.permutations.iter().take(p.actual))
    }

    /// The longest play (a permutation with its continuations), seconds,
    /// from the tag alone (the original release's lengths, which MCC's
    /// takes match to a few ms).
    pub fn seconds(&self) -> f32 {
        let per_second = self.rate.max(1) as f32 * 2.0 * if self.encoding == 1 { 2.0 } else { 1.0 };
        let mut longest = 0u64;
        for pr in &self.pitch_ranges {
            for start in 0..pr.actual.min(pr.permutations.len()) {
                let (mut i, mut bytes, mut hops) = (start as i16, 0u64, 0);
                while i >= 0 && (i as usize) < pr.permutations.len() && hops <= pr.permutations.len() {
                    bytes += pr.permutations[i as usize].pcm_bytes as u64;
                    i = pr.permutations[i as usize].next;
                    hops += 1;
                }
                longest = longest.max(bytes);
            }
        }
        longest as f32 / per_second
    }

    /// The gain for a script's scale (`sound_impulse_start snd obj scale`).
    pub fn gain_at(&self, scale: f32) -> f32 {
        let s = scale.clamp(0.0, 1.0);
        self.gain * (self.scale_gain.0 + (self.scale_gain.1 - self.scale_gain.0) * s)
    }
}

/// A pitch range (0x48 bytes).
#[derive(Clone, Debug, Default)]
pub struct PitchRange {
    pub name: String,
    pub natural_pitch: f32,
    pub bend: (f32, f32),
    /// How many of `permutations` are played; the others continue them
    /// (`next`), the way the original release split long sounds.
    pub actual: usize,
    pub permutations: Vec<Permutation>,
}

/// A permutation (0x7C bytes).
#[derive(Clone, Debug, Default)]
pub struct Permutation {
    pub name: String,
    pub skip: f32,
    pub gain: f32,
    /// The permutation continuing this one, -1 none.
    pub next: i16,
    /// Bytes of 16-bit PCM the original release's data decodes to
    /// (`+0x38`): its length without the bank.
    pub pcm_bytes: u32,
}

/// A looping sound (`lsnd`, `halo1.md` 13.2). The same rules as Halo 3's
/// (`sound.md` 5): the header flags and the track flags mean the same.
#[derive(Clone, Debug, Default)]
pub struct Looping {
    pub index: u32,
    pub name: String,
    /// Bit 1 not a loop (plays its loop once), bit 2 stops music.
    pub flags: u32,
    /// `+0x20`: 5 on music, 3 to 300 on placed loops (taken as the
    /// distance it carries; not checked).
    pub max_distance: f32,
    pub tracks: Vec<LoopTrack>,
    pub details: Vec<LoopDetail>,
}

/// A track (0xA0 bytes).
#[derive(Clone, Debug, Default)]
pub struct LoopTrack {
    /// Bit 0 fade in, bit 1 fade out on stop, bit 2 cross-fade to the
    /// alternate loop.
    pub flags: u32,
    /// Linear.
    pub gain: f32,
    pub fade_in: f32,
    pub fade_out: f32,
    /// Tag indices of the `snd!`s.
    pub start: Option<u32>,
    pub looped: Option<u32>,
    pub end: Option<u32>,
    pub alt_loop: Option<u32>,
    pub alt_end: Option<u32>,
}

/// A detail sound (0x68 bytes): a one-shot scattered around now and then.
#[derive(Clone, Debug, Default)]
pub struct LoopDetail {
    pub sound: Option<u32>,
    /// Seconds between plays.
    pub period: (f32, f32),
    /// Linear.
    pub gain: f32,
    pub flags: u32,
    pub yaw: (f32, f32),
    pub pitch: (f32, f32),
    /// World units.
    pub distance: (f32, f32),
}

/// A tag reference's tag index, when it is of `class`.
fn tag_ref(map: &Map, r: &Reader, off: usize, class: &[u8; 4]) -> Result<Option<u32>> {
    let id = r.tag_ref(off)?;
    Ok(map.tag(id).filter(|t| t.is(class)).map(|_| id & 0xFFFF))
}

fn tag_reader<'a>(map: &'a Map, index: u32, class: &[u8; 4]) -> Result<(&'a str, Reader<'a>)> {
    let t = map
        .tags
        .get(index as usize)
        .filter(|t| t.is(class))
        .ok_or_else(|| Error::Invalid(format!("tag {index} is not a {}", String::from_utf8_lossy(class))))?;
    let r = map.tag_data(t.id).ok_or_else(|| Error::Invalid(format!("{} has no data", t.name)))?;
    Ok((&t.name, r))
}

/// Reads a `snd!` tag by tag index.
pub fn sound(map: &Map, index: u32) -> Result<Sound> {
    let (name, r) = tag_reader(map, index, b"snd!")?;
    let mut pitch_ranges = Vec::new();
    for pr in r.block(0x98, 0x48)? {
        let permutations = pr
            .block(0x3C, 0x7C)?
            .into_iter()
            .map(|p| {
                Ok(Permutation {
                    name: p.str(0, 32),
                    skip: p.f32(0x20)?,
                    gain: p.f32(0x24)?,
                    next: p.i16(0x2A)?,
                    pcm_bytes: p.u32(0x38)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let actual = (pr.i16(0x2C)?.max(0) as usize).min(permutations.len());
        pitch_ranges.push(PitchRange {
            name: pr.str(0, 32),
            natural_pitch: pr.f32(0x20)?,
            bend: (pr.f32(0x24)?, pr.f32(0x28)?),
            actual: if actual == 0 { permutations.len() } else { actual },
            permutations,
        });
    }
    Ok(Sound {
        index,
        name: name.to_string(),
        flags: r.u32(0)?,
        class: r.u16(4)?,
        rate: if r.u16(6)? == 1 { 44_100 } else { 22_050 },
        distance: (r.f32(8)?, r.f32(0xC)?),
        skip: r.f32(0x10)?,
        random_pitch: (r.f32(0x14)?, r.f32(0x18)?),
        gain: r.f32(0x28)?,
        scale_gain: (r.f32(0x40)?, r.f32(0x58)?),
        encoding: r.u16(0x6C)?,
        compression: r.u16(0x6E)?,
        pitch_ranges,
    })
}

/// Reads an `lsnd` tag by tag index.
pub fn looping(map: &Map, index: u32) -> Result<Looping> {
    let (name, r) = tag_reader(map, index, b"lsnd")?;
    let snd = |e: &Reader, off: usize| tag_ref(map, e, off, b"snd!");
    let mut tracks = Vec::new();
    for e in r.block(0x3C, 0xA0)? {
        tracks.push(LoopTrack {
            flags: e.u32(0)?,
            gain: e.f32(4)?,
            fade_in: e.f32(8)?,
            fade_out: e.f32(0xC)?,
            start: snd(&e, 0x30)?,
            looped: snd(&e, 0x40)?,
            end: snd(&e, 0x50)?,
            alt_loop: snd(&e, 0x80)?,
            alt_end: snd(&e, 0x90)?,
        });
    }
    let mut details = Vec::new();
    for e in r.block(0x48, 0x68)? {
        details.push(LoopDetail {
            sound: snd(&e, 0)?,
            period: (e.f32(0x10)?, e.f32(0x14)?),
            gain: e.f32(0x18)?,
            flags: e.u32(0x1C)?,
            yaw: (e.f32(0x50)?, e.f32(0x54)?),
            pitch: (e.f32(0x58)?, e.f32(0x5C)?),
            distance: (e.f32(0x60)?, e.f32(0x64)?),
        });
    }
    Ok(Looping {
        index,
        name: name.to_string(),
        flags: r.u32(0)?,
        max_distance: r.f32(0x20)?,
        tracks,
        details,
    })
}

/// The `snd!` and `lsnd` tags the scenario's scripts name (its script tag
/// references, `scnr + 0x4B4`, 0x28 each, the reference at `+0x18`), by
/// tag index.
pub fn script_sounds(map: &Map) -> Result<Vec<u32>> {
    let scnr = map.tag_data(map.scenario).ok_or_else(|| Error::Invalid("no scenario data".into()))?;
    let mut out = Vec::new();
    for e in scnr.block(0x4B4, 0x28)? {
        let id = e.tag_ref(0x18)?;
        if let Some(t) = map.tag(id)
            && (t.is(b"snd!") || t.is(b"lsnd"))
            && !out.contains(&(id & 0xFFFF))
        {
            out.push(id & 0xFFFF);
        }
    }
    Ok(out)
}

/// How long each `snd!` the scripts name plays, seconds, by tag index
/// ([`Sound::seconds`]): what `sound_impulse_time` returns (in ticks).
pub fn script_seconds(map: &Map) -> HashMap<u32, f32> {
    script_sounds(map)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| sound(map, i).ok().map(|s| (i, s.seconds())))
        .collect()
}

// ---- background sound per cluster ----------------------------------------

/// What a structure BSP sounds like where: its background sound palette
/// (`sbsp + 0x1FC`, 0x74 each: name, `lsnd` at `+0x20`), each cluster's
/// entry (cluster `+0x4`, i16, -1 none), and a way to find the cluster a
/// point is in (the collision BSP's 3D tree down to a leaf, whose render
/// leaf, `sbsp + 0xE0`, 16 bytes, names the cluster at `+0x8`).
#[derive(Clone, Debug, Default)]
pub struct BspSound {
    /// The scenario's index of the BSP.
    pub bsp: u32,
    /// Background `lsnd` tag index of each palette entry.
    pub palette: Vec<Option<u32>>,
    /// Each cluster's palette entry.
    pub clusters: Vec<Option<u32>>,
    pub finder: ClusterFinder,
}

/// The collision BSP's 3D nodes and planes, and the cluster of each leaf.
#[derive(Clone, Debug, Default)]
pub struct ClusterFinder {
    /// Plane, back child, front child; a child with the top bit set is a
    /// leaf (the low bits its index), -1 solid.
    pub nodes: Vec<[i32; 3]>,
    pub planes: Vec<[f32; 4]>,
    pub leaf_clusters: Vec<i16>,
}

impl ClusterFinder {
    /// The cluster holding `p` (world units), `None` in solid space.
    pub fn cluster(&self, p: glam::Vec3) -> Option<u32> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut node = 0usize;
        for _ in 0..self.nodes.len().min(1 << 16) {
            let [plane, back, front] = *self.nodes.get(node)?;
            let q = self.planes.get((plane & 0x7FFF_FFFF) as usize)?;
            let mut d = q[0] * p.x + q[1] * p.y + q[2] * p.z - q[3];
            if plane < 0 {
                d = -d;
            }
            let child = if d >= 0.0 { front } else { back };
            if child == -1 {
                return None;
            }
            if child < 0 {
                let leaf = (child & 0x7FFF_FFFF) as usize;
                return u32::try_from(*self.leaf_clusters.get(leaf)?).ok();
            }
            node = child as usize;
        }
        None
    }
}

/// The background sounds of the scenario's BSPs `only` (all when `None`).
pub fn bsp_sounds(map: &Map, only: Option<&[u32]>) -> Result<Vec<BspSound>> {
    let mut out = Vec::new();
    for (i, chunk) in map.bsps()?.iter().enumerate() {
        let index = i as u32;
        if only.is_some_and(|o| !o.contains(&index)) {
            continue;
        }
        let sbsp = map.sbsp(chunk);
        let palette = sbsp
            .block(0x1FC, 0x74)?
            .iter()
            .map(|e| tag_ref(map, e, 0x20, b"lsnd"))
            .collect::<Result<Vec<_>>>()?;
        let clusters = sbsp
            .block(0x134, 0x68)?
            .iter()
            .map(|c| Ok(u32::try_from(c.i16(4)?).ok()))
            .collect::<Result<Vec<_>>>()?;
        let mut finder = ClusterFinder::default();
        if let Some(col) = sbsp.block(0xB0, 0x60)?.into_iter().next() {
            for n in col.block(0, 12)? {
                finder.nodes.push([n.i32(0)?, n.i32(4)?, n.i32(8)?]);
            }
            for p in col.block(0xC, 16)? {
                finder.planes.push([p.f32(0)?, p.f32(4)?, p.f32(8)?, p.f32(12)?]);
            }
        }
        for l in sbsp.block(0xE0, 16)? {
            finder.leaf_clusters.push(l.i16(8)?);
        }
        out.push(BspSound {
            bsp: index,
            palette,
            clusters,
            finder,
        });
    }
    Ok(out)
}

// ---- banks --------------------------------------------------------------

/// The banks in the order a tag is looked up: the player's dialogue
/// language first (when MCC has it), then English, music, streams, the
/// effects ("debug") and the ADPCM bank.
pub const BANKS: [&str; 5] = ["sounds_dialogs", "sounds_music", "sounds_stream", "sounds_debug", "sounds_adpcm"];

/// Dialogue languages MCC ships Halo CE banks for (`sounds_dialogs_<l>.fsb`).
pub const LANGUAGES: [&str; 4] = ["french", "german", "italian", "spanish"];

/// The Halo CE dialogue bank of a language name as `game_paths::language`
/// gives it (Halo 3's list): `mexican` plays the Spanish bank; English and
/// anything without a bank is `None` (the English bank).
pub fn dialog_language(language: &str) -> Option<&'static str> {
    let l = if language == "mexican" { "spanish" } else { language };
    LANGUAGES.iter().copied().find(|x| *x == l)
}

/// FSB5 codec numbers seen in MCC's Halo CE banks.
pub const CODEC_IMA_ADPCM: u32 = 7;
pub const CODEC_CELT: u32 = 12;

/// One sample of a bank.
#[derive(Clone, Debug)]
pub struct Sample {
    pub rate: u32,
    pub channels: u16,
    /// Frames (per channel) after decoding.
    pub frames: u32,
    /// Byte range in the bank's data.
    pub offset: usize,
    pub size: usize,
    /// FSB loop chunk (3): start and end frame. Dialogue and effects carry
    /// one spanning the whole sample.
    pub fsb_loop: Option<(u32, u32)>,
    /// The sample's name in the bank (the permutation's, or a remastered
    /// take's: `shotgun_fire_1`).
    pub name: String,
}

/// One FSB5 bank and its index.
pub struct Bank {
    pub name: String,
    pub path: PathBuf,
    pub codec: u32,
    map: memmap2::Mmap,
    data_base: usize,
    pub samples: Vec<Sample>,
    /// Lowercase tag path to (first sample, count), from `lst\<name>.lst.bin`.
    pub index: HashMap<String, (u32, u32)>,
}

fn u32_at(b: &[u8], o: usize) -> Result<u32> {
    b.get(o..o + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::Invalid(format!("read past the end at {o:#x}")))
}

const RATES: [u32; 10] = [0, 8000, 11000, 11025, 16000, 22050, 24000, 32000, 44100, 48000];

/// Reads a `.lst.bin`: u32 version (1), u32 count, then per entry u32
/// length (with the NUL), the tag path, u32 first sample, u32 count.
pub fn read_lst(b: &[u8]) -> Result<Vec<(String, u32, u32)>> {
    if u32_at(b, 0)? != 1 {
        return Err(Error::Invalid("lst version is not 1".into()));
    }
    let n = u32_at(b, 4)? as usize;
    let mut o = 8;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let len = u32_at(b, o)? as usize;
        let s = b.get(o + 4..o + 4 + len).ok_or_else(|| Error::Invalid("lst entry runs past the end".into()))?;
        let s = &s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())];
        let first = u32_at(b, o + 4 + len)?;
        let count = u32_at(b, o + 8 + len)?;
        out.push((String::from_utf8_lossy(s).into_owned(), first, count));
        o += 12 + len;
    }
    Ok(out)
}

impl Bank {
    /// Opens `<dir>\<name>.fsb` and `<dir>\lst\<name>.lst.bin`.
    pub fn open(dir: &Path, name: &str) -> Result<Bank> {
        let path = dir.join(format!("{name}.fsb"));
        let file = std::fs::File::open(&path).map_err(|e| Error::Io(path.clone(), e))?;
        // SAFETY: read-only map of a game file nobody writes while we run;
        // every read is bounds checked.
        let map = unsafe { memmap2::Mmap::map(&file) }.map_err(|e| Error::Io(path.clone(), e))?;
        let lst_path = dir.join("lst").join(format!("{name}.lst.bin"));
        let lst = std::fs::read(&lst_path).map_err(|e| Error::Io(lst_path.clone(), e))?;
        let b: &[u8] = &map;
        if b.get(0..4) != Some(b"FSB5") {
            return Err(Error::Invalid(format!("{} is not an FSB5 bank", path.display())));
        }
        let version = u32_at(b, 4)?;
        let count = u32_at(b, 8)? as usize;
        let headers = u32_at(b, 0x0C)? as usize;
        let names = u32_at(b, 0x10)? as usize;
        let data = u32_at(b, 0x14)? as usize;
        let codec = u32_at(b, 0x18)?;
        let header_size = if version == 0 { 0x40 } else { 0x3C };
        let data_base = header_size + headers + names;
        if data_base + data > b.len() {
            return Err(Error::Invalid(format!("{} is shorter than its header says", path.display())));
        }
        let mut samples = Vec::with_capacity(count);
        let mut p = header_size;
        let end = header_size + headers;
        for _ in 0..count {
            let v = u32_at(b, p)? as u64 | (u32_at(b, p + 4)? as u64) << 32;
            p += 8;
            let mut s = Sample {
                rate: RATES.get(((v >> 1) & 0xF) as usize).copied().unwrap_or(0),
                channels: if (v >> 5) & 1 != 0 { 2 } else { 1 },
                offset: (((v >> 6) & 0x0FFF_FFFF) * 16) as usize,
                frames: (v >> 34) as u32,
                size: 0,
                fsb_loop: None,
                name: String::new(),
            };
            let mut more = v & 1 != 0;
            while more {
                let c = u32_at(b, p)?;
                p += 4;
                more = c & 1 != 0;
                let size = ((c >> 1) & 0xFF_FFFF) as usize;
                if p + size > end {
                    return Err(Error::Invalid("sample chunk runs past the headers".into()));
                }
                match c >> 25 {
                    1 => s.channels = b[p] as u16,
                    2 => s.rate = u32_at(b, p)?,
                    3 => s.fsb_loop = Some((u32_at(b, p)?, u32_at(b, p + 4)?)),
                    _ => {}
                }
                p += size;
            }
            samples.push(s);
        }
        for i in 0..samples.len() {
            let next = samples.get(i + 1).map_or(data, |n| n.offset);
            samples[i].size = next.saturating_sub(samples[i].offset);
            if names > 0 {
                let at = header_size + headers;
                if let Ok(no) = u32_at(b, at + 4 * i) {
                    let s = b.get(at + no as usize..at + names).unwrap_or(&[]);
                    let s = &s[..s.iter().position(|c| *c == 0).unwrap_or(s.len())];
                    samples[i].name = String::from_utf8_lossy(s).into_owned();
                }
            }
        }
        let mut index = HashMap::new();
        for (path, first, n) in read_lst(&lst)? {
            if (first as usize + n as usize) <= samples.len() {
                index.entry(path.to_ascii_lowercase()).or_insert((first, n));
            }
        }
        Ok(Bank {
            name: name.to_string(),
            path,
            codec,
            map,
            data_base,
            samples,
            index,
        })
    }

    /// The encoded bytes of sample `i`.
    pub fn bytes(&self, i: u32) -> Option<&[u8]> {
        let s = self.samples.get(i as usize)?;
        let start = self.data_base + s.offset;
        self.map.get(start..start + s.size)
    }
}

/// Decoded audio: interleaved 16-bit samples.
#[derive(Clone, Debug, Default)]
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
    /// The largest absolute sample, 0..1.
    pub fn peak(&self) -> f32 {
        self.samples.iter().map(|s| (*s as i32).unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0
    }
}

/// A sample in [`Banks`]: the bank's place in [`Banks::banks`] in the top
/// 8 bits, the sample index in the low 24.
pub type SampleId = u32;

pub fn sample_id(bank: usize, sample: u32) -> SampleId {
    (bank as u32) << 24 | (sample & 0x00FF_FFFF)
}

/// Every bank MCC's Halo CE has in `halo1\sound\pc`.
pub struct Banks {
    pub dir: PathBuf,
    pub banks: Vec<Bank>,
    /// The dialogue language bank opened, if any.
    pub language: Option<&'static str>,
}

impl Banks {
    /// Opens the banks in `dir` (`game_paths::halo1_sound()`), with the
    /// dialogue of `language` (`game_paths::language()`) where MCC has it.
    /// A missing bank is left out; none at all is an error.
    pub fn open(dir: &Path, language: &str) -> Result<Banks> {
        let mut banks = Vec::new();
        let lang = dialog_language(language);
        let mut opened = None;
        let mut first_err = None;
        let names = lang
            .map(|l| format!("sounds_dialogs_{l}"))
            .into_iter()
            .chain(BANKS.iter().map(|s| s.to_string()));
        for name in names {
            match Bank::open(dir, &name) {
                Ok(b) => {
                    if name.starts_with("sounds_dialogs_") {
                        opened = lang;
                    }
                    banks.push(b);
                }
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }
        if banks.is_empty() {
            return Err(first_err.unwrap_or_else(|| Error::Invalid("no banks".into())));
        }
        Ok(Banks {
            dir: dir.to_path_buf(),
            banks,
            language: opened,
        })
    }

    /// The samples of a tag (its path, as the map names it), from the first
    /// bank that lists it: the tag's permutations, or MCC's own takes of
    /// them (some remastered effects have more takes than the tag has
    /// permutations). Empty when no bank has it.
    pub fn samples_of(&self, tag_path: &str) -> Vec<SampleId> {
        let key = tag_path.to_ascii_lowercase();
        for (b, bank) in self.banks.iter().enumerate() {
            if let Some((first, n)) = bank.index.get(&key) {
                return (*first..first + n).map(|i| sample_id(b, i)).collect();
            }
        }
        Vec::new()
    }

    pub fn bank_of(&self, id: SampleId) -> Option<&Bank> {
        self.banks.get((id >> 24) as usize)
    }

    pub fn sample(&self, id: SampleId) -> Option<&Sample> {
        self.bank_of(id)?.samples.get((id & 0x00FF_FFFF) as usize)
    }

    /// The sample's length from its header alone, seconds.
    pub fn seconds(&self, id: SampleId) -> f32 {
        self.sample(id).map_or(0.0, |s| s.frames as f32 / s.rate.max(1) as f32)
    }

    /// Decodes a sample.
    pub fn decode(&self, id: SampleId) -> Result<Pcm> {
        let bank = self.bank_of(id).ok_or_else(|| Error::Invalid(format!("no bank for sample {id:#x}")))?;
        let i = id & 0x00FF_FFFF;
        let s = bank.samples.get(i as usize).ok_or_else(|| Error::Invalid(format!("{}: no sample {i}", bank.name)))?;
        let data = bank.bytes(i).ok_or_else(|| Error::Invalid(format!("{}: sample {i} runs past the file", bank.name)))?;
        let frames = s.frames as usize;
        let samples = match bank.codec {
            CODEC_CELT => decode_celt(data, s.channels, frames).map_err(|e| Error::Invalid(format!("{} #{i}: {e}", bank.name)))?,
            CODEC_IMA_ADPCM => decode_ima(data, s.channels, frames),
            c => return Err(Error::Unsupported(format!("{}: FSB codec {c}", bank.name))),
        };
        Ok(Pcm {
            rate: s.rate,
            channels: s.channels,
            samples,
        })
    }
}

/// CELT, with more than two channels as stereo streams whose packets take
/// turns (MCC's 4-channel ambience: two stereo pairs).
fn decode_celt(data: &[u8], channels: u16, frames: usize) -> std::result::Result<Vec<i16>, fsb_celt::Error> {
    if channels <= 2 {
        return fsb_celt::decode(data, channels, Some(frames));
    }
    let streams = channels.div_ceil(2) as usize;
    let packets = fsb_celt::packets(data)?;
    let mut out = vec![0i16; frames * channels as usize];
    for k in 0..streams {
        let ch = if 2 * k + 1 < channels as usize { 2 } else { 1 };
        // This stream's packets, back to back with their framing.
        let mut mine = Vec::new();
        for (at, len) in packets.iter().skip(k).step_by(streams) {
            mine.extend_from_slice(&data[at - 8..at + len]);
        }
        let pcm = fsb_celt::decode(&mine, ch as u16, Some(frames))?;
        for f in 0..frames.min(pcm.len() / ch) {
            for c in 0..ch {
                out[f * channels as usize + 2 * k + c] = pcm[f * ch + c];
            }
        }
    }
    Ok(out)
}

/// FMOD's IMA ADPCM (codec 7): blocks of 36 bytes per channel, a 4-byte
/// header (first sample, step index) and 64 samples of nibbles; stereo
/// blocks interleave the channels every 4 bytes after both headers.
fn decode_ima(data: &[u8], channels: u16, frames: usize) -> Vec<i16> {
    const STEPS: [i32; 89] = [
        7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118,
        130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060,
        1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484,
        7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
    ];
    const INDEX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
    let ch = channels.max(1) as usize;
    let block = 36 * ch;
    let mut out = Vec::with_capacity(frames * ch);
    for b in data.chunks_exact(block) {
        let mut pred = [0i32; 8];
        let mut step = [0i32; 8];
        for c in 0..ch.min(8) {
            pred[c] = i16::from_le_bytes([b[4 * c], b[4 * c + 1]]) as i32;
            step[c] = (b[4 * c + 2] as i32).clamp(0, 88);
        }
        let mut block_pcm = vec![0i16; 64 * ch];
        let body = &b[4 * ch..];
        for (w, word) in body.chunks_exact(4).enumerate() {
            let c = w % ch;
            let base = (w / ch) * 8;
            for (k, nib) in word.iter().flat_map(|x| [x & 0xF, x >> 4]).enumerate() {
                let s = STEPS[step[c] as usize];
                let mut diff = s >> 3;
                if nib & 1 != 0 {
                    diff += s >> 2;
                }
                if nib & 2 != 0 {
                    diff += s >> 1;
                }
                if nib & 4 != 0 {
                    diff += s;
                }
                pred[c] = if nib & 8 != 0 { pred[c] - diff } else { pred[c] + diff }.clamp(-32768, 32767);
                step[c] = (step[c] + INDEX[(nib & 7) as usize]).clamp(0, 88);
                if base + k < 64 {
                    block_pcm[(base + k) * ch + c] = pred[c] as i16;
                }
            }
        }
        out.extend_from_slice(&block_pcm);
        if out.len() >= frames * ch {
            break;
        }
    }
    out.truncate(frames * ch);
    out
}
