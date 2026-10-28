//! Decoding the bank's Vorbis samples. `docs/format/sound.md` section 3.
//!
//! FSB5 stores bare Vorbis audio packets, each behind a u16 length, and
//! leaves out the three Vorbis headers. The identification header is
//! rebuilt from the sample's rate and channels; the comment header is not
//! needed; the setup header (the codebooks) is one of a few dozen FMOD
//! builds into its runtime, keyed by a CRC the sample names. MCC links
//! FMOD into `halo3.dll`, so the table is read from the user's own copy of
//! that file at run time (nothing of it is stored here).

use std::collections::HashMap;
use std::path::Path;

use lewton::audio::{PreviousWindowRight, read_audio_packet_generic};
use lewton::header::{IdentHeader, SetupHeader, read_header_ident, read_header_setup};
use lewton::samples::InterleavedSamples;

use crate::fsb::Bank;
use crate::{Error, Result};

/// Decoded audio: interleaved f32 in -1..1.
#[derive(Clone, Debug, Default)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.rate.max(1) as f32
    }
    /// Folded down to two channels (mono is copied to both): 4 channels as
    /// front left, front right, back left, back right; 6 as FL FR C LFE BL
    /// BR (the usual WAV order; Halo's own layer order is not checked).
    pub fn to_stereo(&self) -> Pcm {
        let ch = self.channels.max(1) as usize;
        let mut samples = Vec::with_capacity(self.frames() * 2);
        for f in self.samples.chunks_exact(ch) {
            let (l, r) = match ch {
                1 => (f[0], f[0]),
                2 => (f[0], f[1]),
                4 => (0.6 * (f[0] + f[2]), 0.6 * (f[1] + f[3])),
                6 => (
                    0.5 * (f[0] + 0.7 * f[2] + f[4]),
                    0.5 * (f[1] + 0.7 * f[2] + f[5]),
                ),
                _ => {
                    let m = f.iter().sum::<f32>() / ch as f32;
                    (m, m)
                }
            };
            samples.push(l);
            samples.push(r);
        }
        Pcm {
            rate: self.rate,
            channels: 2,
            samples,
        }
    }

    /// Root mean square and peak over all channels.
    pub fn rms_peak(&self) -> (f32, f32) {
        let mut sum = 0.0f64;
        let mut peak = 0.0f32;
        for &s in &self.samples {
            sum += (s as f64) * (s as f64);
            peak = peak.max(s.abs());
        }
        let rms = (sum / self.samples.len().max(1) as f64).sqrt() as f32;
        (rms, peak)
    }
}

/// FMOD's Vorbis setup headers, by the CRC samples name them with.
#[derive(Default)]
pub struct Setups {
    /// The packet, and whether its CRC-32 is the key (rebuilt exactly).
    by_crc: HashMap<u32, (Vec<u8>, bool)>,
}

/// Block sizes of FMOD's Vorbis encoder: 256 and 2048 samples (as powers
/// of two). Checked: every sample's packet count and frame count agree.
const BLOCKSIZES: (u8, u8) = (8, 11);

impl Setups {
    /// Reads the setup table out of FMOD's runtime (`halo3.dll`).
    pub fn from_dll(path: impl AsRef<Path>) -> Result<Setups> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| Error::Io {
            path: path.to_path_buf(),
            source,
        })?;
        Setups::scan(&bytes)
    }

    /// Rebuilds every setup packet FMOD's table describes (`sound.md` 3.2).
    ///
    /// The table is 32-byte rows `{u32 n, u64 tail, u32 total}` `{u32 crc,
    /// u64 base, u32 shared}`: setups that differ only in their last
    /// bytes share one `base` packet in the image; the first `shared`
    /// bytes are common, then a setup's own bytes come from `tail` for a
    /// while and from `base` again up to `total`. Where the tail's bytes
    /// stop is not stored as far as we know, so each split is tried until
    /// the CRC-32 of the packet is the row's key (FMOD keys setups by the
    /// CRC of the whole packet). Rows that match no split keep the base
    /// packet as it is.
    pub fn scan(dll: &[u8]) -> Result<Setups> {
        let pe = Pe::parse(dll)?;
        let starts: Vec<usize> = dll
            .windows(7)
            .enumerate()
            .filter(|(_, w)| *w == b"\x05vorbis")
            .map(|(i, _)| i)
            .collect();
        let rd = |at: usize| -> Option<(usize, usize)> {
            let va = u64::from_le_bytes(dll.get(at..at + 8)?.try_into().ok()?);
            let size = u32::from_le_bytes(dll.get(at + 8..at + 12)?.try_into().ok()?) as usize;
            Some((pe.file_offset(va)?, size))
        };
        let mut by_crc = HashMap::new();
        let mut p = 16;
        while p + 16 <= dll.len() {
            let crc = u32::from_le_bytes(dll[p..p + 4].try_into().unwrap());
            if let Some((base, shared)) = rd(p + 4)
                && (64..65536).contains(&shared)
                && starts.binary_search(&base).is_ok()
                && !by_crc.get(&crc).is_some_and(|(_, ok): &(Vec<u8>, bool)| *ok)
            {
                let packet = rd(p - 12)
                    .filter(|&(tail, total)| total >= shared && total < 65536 && tail + (total - shared) <= dll.len() && base + total <= dll.len())
                    .and_then(|(tail, total)| {
                        let own = total - shared;
                        (0..=own).find_map(|k| {
                            let mut v = Vec::with_capacity(total);
                            v.extend_from_slice(&dll[base..base + shared]);
                            v.extend_from_slice(&dll[tail..tail + k]);
                            v.extend_from_slice(&dll[base + shared + k..base + total]);
                            (crc32(&v) == crc).then_some((v, true))
                        })
                    });
                let packet = packet.unwrap_or_else(|| {
                    let k = starts.binary_search(&base).unwrap();
                    let end = starts.get(k + 1).copied().unwrap_or(dll.len()).min(base + 65536);
                    (dll[base..end].to_vec(), false)
                });
                by_crc.insert(crc, packet);
            }
            p += 4;
        }
        if by_crc.is_empty() {
            return Err(Error::Invalid("no Vorbis setup table in the DLL".into()));
        }
        Ok(Setups { by_crc })
    }

    pub fn len(&self) -> usize {
        self.by_crc.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_crc.is_empty()
    }
    pub fn get(&self, crc: u32) -> Option<&[u8]> {
        self.by_crc.get(&crc).map(|(v, _)| v.as_slice())
    }
    /// Whether setup `crc` was rebuilt to a packet whose CRC-32 is `crc`.
    pub fn verified(&self, crc: u32) -> bool {
        self.by_crc.get(&crc).is_some_and(|(_, ok)| *ok)
    }
    /// How many setups were rebuilt exactly.
    pub fn verified_count(&self) -> usize {
        self.by_crc.values().filter(|(_, ok)| *ok).count()
    }
}

/// CRC-32 (IEEE, reflected, as zlib), which FMOD keys its setups by.
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::LazyLock<[u32; 256]> = std::sync::LazyLock::new(|| {
        let mut t = [0u32; 256];
        for (i, e) in t.iter_mut().enumerate() {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *e = c;
        }
        t
    });
    !data
        .iter()
        .fold(!0u32, |c, &b| TABLE[((c ^ b as u32) & 0xFF) as usize] ^ (c >> 8))
}

/// Just enough of a PE image to turn its virtual addresses into file
/// offsets.
struct Pe {
    image_base: u64,
    /// (virtual address, virtual size, file offset, file size)
    sections: Vec<(u64, u64, u64, u64)>,
}

impl Pe {
    fn parse(b: &[u8]) -> Result<Pe> {
        let bad = || Error::Invalid("not a 64-bit PE image".into());
        let u16_at = |o: usize| b.get(o..o + 2).map(|s| u16::from_le_bytes([s[0], s[1]]));
        let u32_at = |o: usize| b.get(o..o + 4).map(|s| u32::from_le_bytes(s.try_into().unwrap()));
        let pe = u32_at(0x3C).ok_or_else(bad)? as usize;
        if b.get(pe..pe + 4) != Some(b"PE\0\0") {
            return Err(bad());
        }
        let count = u16_at(pe + 6).ok_or_else(bad)? as usize;
        let opt_size = u16_at(pe + 20).ok_or_else(bad)? as usize;
        let opt = pe + 24;
        if u16_at(opt) != Some(0x20B) {
            return Err(bad());
        }
        let image_base = b
            .get(opt + 24..opt + 32)
            .map(|s| u64::from_le_bytes(s.try_into().unwrap()))
            .ok_or_else(bad)?;
        let mut sections = Vec::new();
        for i in 0..count {
            let s = opt + opt_size + 40 * i;
            let vsize = u32_at(s + 8).ok_or_else(bad)? as u64;
            let va = u32_at(s + 12).ok_or_else(bad)? as u64;
            let rsize = u32_at(s + 16).ok_or_else(bad)? as u64;
            let raw = u32_at(s + 20).ok_or_else(bad)? as u64;
            sections.push((va, vsize, raw, rsize));
        }
        Ok(Pe {
            image_base,
            sections,
        })
    }

    fn file_offset(&self, va: u64) -> Option<usize> {
        let rva = va.checked_sub(self.image_base)?;
        self.sections.iter().find_map(|&(v, vs, raw, rs)| {
            (rva >= v && rva < v + vs.min(rs)).then(|| (rva - v + raw) as usize)
        })
    }
}

/// The 30-byte Vorbis identification header for a sample.
fn ident_packet(channels: u16, rate: u32) -> Vec<u8> {
    let mut p = Vec::with_capacity(30);
    p.push(1);
    p.extend_from_slice(b"vorbis");
    p.extend_from_slice(&0u32.to_le_bytes());
    p.push(channels as u8);
    p.extend_from_slice(&rate.to_le_bytes());
    p.extend_from_slice(&[0; 12]);
    p.push(BLOCKSIZES.0 | (BLOCKSIZES.1 << 4));
    p.push(1);
    p
}

/// A sample's decoder state, for streaming (music) or decoding whole.
///
/// A sample with Vorbis "intra layers" (chunk 14, `n` layers) is one
/// ordinary Vorbis stream of `n` x the header's channels (`sound.md` 3.3):
/// 120_halo's quad ambience is a "mono" sample with 4 layers.
pub struct Decoder<'a> {
    ident: IdentHeader,
    setup: SetupHeader,
    pwr: PreviousWindowRight,
    data: &'a [u8],
    pos: usize,
    frames_left: usize,
    channels: u16,
}

impl<'a> Decoder<'a> {
    pub fn new(bank: &'a Bank, setups: &Setups, sample: u32) -> Result<Decoder<'a>> {
        let s = bank
            .samples
            .get(sample as usize)
            .ok_or_else(|| Error::Invalid(format!("no sample {sample}")))?;
        let channels = s.channels * s.layers.unwrap_or(1).clamp(1, 8) as u16;
        let crc = s
            .setup_crc
            .ok_or_else(|| Error::Invalid(format!("sample {sample} names no setup header")))?;
        let setup = setups
            .get(crc)
            .ok_or_else(|| Error::Unsupported(format!("setup header {crc:#010x} not in the DLL")))?;
        let ident = read_header_ident(&ident_packet(channels, s.rate))
            .map_err(|e| Error::Decode(format!("ident: {e:?}")))?;
        let setup = read_header_setup(setup, channels as u8, BLOCKSIZES)
            .map_err(|e| Error::Decode(format!("setup {crc:#010x}: {e:?}")))?;
        Ok(Decoder {
            ident,
            setup,
            pwr: PreviousWindowRight::new(),
            data: bank.bytes(sample).unwrap_or(&[]),
            pos: 0,
            frames_left: s.frames as usize,
            channels,
        })
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// The next decoded packet's samples (interleaved), or `None` at the
    /// end. The first packet gives none (Vorbis overlap), which is
    /// returned as an empty vector.
    pub fn next_packet(&mut self) -> Result<Option<Vec<f32>>> {
        if self.frames_left == 0 || self.pos + 2 > self.data.len() {
            return Ok(None);
        }
        let len = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]) as usize;
        if len == 0 || len == 0xFFFF || self.pos + 2 + len > self.data.len() {
            return Ok(None);
        }
        let packet = &self.data[self.pos + 2..self.pos + 2 + len];
        self.pos += 2 + len;
        let mut out: InterleavedSamples<f32> =
            read_audio_packet_generic(&self.ident, &self.setup, packet, &mut self.pwr)
                .map_err(|e| Error::Decode(format!("audio packet: {e:?}")))?;
        let ch = self.channels as usize;
        let frames = (out.samples.len() / ch).min(self.frames_left);
        out.samples.truncate(frames * ch);
        self.frames_left -= frames;
        Ok(Some(out.samples))
    }
}

/// Decodes sample `sample` whole.
pub fn decode(bank: &Bank, setups: &Setups, sample: u32) -> Result<Pcm> {
    let s = &bank.samples[sample as usize];
    let mut d = Decoder::new(bank, setups, sample)?;
    let channels = d.channels();
    let mut samples = Vec::with_capacity(s.frames as usize * channels as usize);
    while let Some(p) = d.next_packet()? {
        samples.extend_from_slice(&p);
    }
    Ok(Pcm {
        rate: s.rate,
        channels,
        samples,
    })
}
