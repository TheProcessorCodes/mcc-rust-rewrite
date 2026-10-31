//! The bikes' engine recordings: `<Reflex>\Database\VehicleEngineAudio`.
//!
//! Each engine size (`MXVehicleEngineSize_MX450f`, ...) is a package with
//! two assets: a `sound`, FMOD Designer's event file (`FEV1`: the events
//! `MXB_450F_IDLE`, `..._THROTTLE_MID`, ... and the clips each one picks
//! from), and a `soundbnk`, a 0x44-byte Reflex header and then an FMOD
//! sample bank (`FSB4`). In the bank, after a 48-byte header (`FSB4`, the
//! sample count, the size of the sample headers, the size of the data),
//! each sample has an 80-byte header: u16 size, the file name (30 bytes,
//! `MXB_450F_IDLE.wav`), u32 length in samples, u32 bytes, loop start and
//! end, mode, rate, ... The data follows the headers, one sample after the
//! other. The 450F's 54 are all mono 44.1 kHz MPEG layer III (mode
//! 0x100220), 96 kbit/s, with zero bytes padding between the frames.
//!
//! The event file's sound definitions say which recordings each event
//! picks from (`sound_defs`): the four throttle events each draw from a
//! pool of seven to eleven takes, played whole, one after another.

use std::path::Path;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::Error;
use crate::package::Package;

/// One recording, decoded.
pub struct EngineClip {
    /// The file name without `.wav`: `MXB_450F_THROTTLE_MID_03`.
    pub name: String,
    pub rate: u32,
    /// Mono, -1 to 1.
    pub samples: Vec<f32>,
}

/// One of the event file's sound definitions: what an event plays.
pub struct SoundDef {
    /// Without the leading `/`: `MXB_450F_THROTTLE_LOW`.
    pub name: String,
    /// The recordings it picks from, as [`EngineClip::name`]s; empty for
    /// the ones that play nothing (`MXB_450F_SHIFT`).
    pub takes: Vec<String>,
}

/// The Dakumoto 450F's engine: every clip in `MX_450F_bank01`, and the
/// sound definitions of `MX_450F_01`.
pub struct Engine {
    pub clips: Vec<EngineClip>,
    pub defs: Vec<SoundDef>,
}

pub fn load_450f(root: &Path) -> Result<Engine, Error> {
    let pkg = Package::open(&root.join("Database"), "VehicleEngineAudio")?;
    let bank = pkg.get("MXVehicleEngineSize_MX450f", "MX_450F_bank01", "soundbnk")?;
    let fev = pkg.get("MXVehicleEngineSize_MX450f", "MX_450F_01", "sound")?;
    Ok(Engine {
        clips: read_bank(&bank)?,
        defs: sound_defs(&fev)?,
    })
}

/// A u32-length-prefixed, NUL-terminated string at `p`, and the offset after it.
fn str_at(b: &[u8], p: usize) -> Result<(String, usize), Error> {
    let n = u32_at(b, p)? as usize;
    let raw = b
        .get(p + 4..p + 4 + n)
        .ok_or_else(|| Error::Format(format!("FEV string at {p}")))?;
    let text = String::from_utf8_lossy(raw.split(|&c| c == 0).next().unwrap_or_default()).into_owned();
    Ok((text, p + 4 + n))
}

/// The sound definitions in an FMOD Designer event file (`FEV1`, as
/// Reflex 1 ships it). They follow the events, as a u32 count and then,
/// per definition: its name (`/MXB_450F_THROTTLE_LOW`), the index of its
/// property template, the entry count, and per entry a u32 kind and a u32
/// weight; a kind-0 entry (a recording) goes on with the file name, the
/// bank name, the index in the bank and the length in ms.
pub fn sound_defs(fev: &[u8]) -> Result<Vec<SoundDef>, Error> {
    // The first name starting with '/' is the first definition's.
    let first = (4..fev.len().saturating_sub(5))
        .find(|&p| {
            fev[p + 4] == b'/'
                && u32_at(fev, p).is_ok_and(|n| (3..128).contains(&n) && fev.get(p + 3 + n as usize) == Some(&0))
        })
        .ok_or_else(|| Error::Format("FEV: no sound definitions".into()))?;
    let count = u32_at(fev, first - 4)? as usize;
    let mut p = first;
    let mut defs = Vec::with_capacity(count);
    for _ in 0..count {
        let (name, next) = str_at(fev, p)?;
        let entries = u32_at(fev, next + 4)? as usize;
        p = next + 8;
        let mut takes = Vec::new();
        for _ in 0..entries {
            let kind = u32_at(fev, p)?;
            p += 8;
            if kind == 0 {
                let (file, next) = str_at(fev, p)?;
                let (_bank, next) = str_at(fev, next)?;
                p = next + 8;
                takes.push(file.trim_end_matches(".wav").to_string());
            }
        }
        defs.push(SoundDef {
            name: name.trim_start_matches('/').to_string(),
            takes,
        });
    }
    Ok(defs)
}

fn u32_at(b: &[u8], p: usize) -> Result<u32, Error> {
    b.get(p..p + 4)
        .map(|s| u32::from_le_bytes(s.try_into().unwrap()))
        .ok_or_else(|| Error::Format(format!("FSB4 ends at {p}")))
}

/// Every sample in a `soundbnk` (its FSB4 bank), decoded.
pub fn read_bank(bank: &[u8]) -> Result<Vec<EngineClip>, Error> {
    let o = bank
        .windows(4)
        .position(|w| w == b"FSB4")
        .ok_or_else(|| Error::Format("no FSB4 bank".into()))?;
    let count = u32_at(bank, o + 4)? as usize;
    let headers = u32_at(bank, o + 8)? as usize;
    let mut p = o + 48;
    let mut data = p + headers;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        let size = bank
            .get(p..p + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]) as usize)
            .ok_or_else(|| Error::Format("FSB4 sample header".into()))?;
        let raw = bank.get(p + 2..p + 32).ok_or_else(|| Error::Format("FSB4 name".into()))?;
        let name = String::from_utf8_lossy(raw.split(|&c| c == 0).next().unwrap_or_default()).to_string();
        let bytes = u32_at(bank, p + 36)? as usize;
        let mode = u32_at(bank, p + 48)?;
        let rate = u32_at(bank, p + 52)?;
        let blob = bank
            .get(data..data + bytes)
            .ok_or_else(|| Error::Format(format!("{name}: data cut short")))?;
        if mode & 0x200 == 0 {
            return Err(Error::Format(format!("{name}: mode {mode:#x} is not MPEG")));
        }
        out.push(EngineClip {
            name: name.trim_end_matches(".wav").to_string(),
            rate,
            samples: decode_mp3(&strip_padding(blob))?,
        });
        data += bytes;
        p += size;
    }
    Ok(out)
}

/// The MPEG frames back to back: FSB puts zero bytes between them.
fn strip_padding(b: &[u8]) -> Vec<u8> {
    const KBPS: [usize; 16] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0];
    const HZ: [usize; 4] = [44100, 48000, 32000, 0];
    let mut out = Vec::with_capacity(b.len());
    let mut p = 0;
    while p + 4 <= b.len() {
        // MPEG-1 layer III sync, then the frame's length from its header.
        if b[p] == 0xFF && b[p + 1] & 0xFE == 0xFA {
            let kbps = KBPS[(b[p + 2] >> 4) as usize];
            let hz = HZ[((b[p + 2] >> 2) & 3) as usize];
            if kbps > 0 && hz > 0 {
                let len = 144 * kbps * 1000 / hz + ((b[p + 2] >> 1) & 1) as usize;
                out.extend_from_slice(&b[p..(p + len).min(b.len())]);
                p += len;
                continue;
            }
        }
        p += 1;
    }
    out
}

fn decode_mp3(bytes: &[u8]) -> Result<Vec<f32>, Error> {
    let bad = |e: symphonia::core::errors::Error| Error::Format(format!("MPEG: {e}"));
    let stream = MediaSourceStream::new(Box::new(std::io::Cursor::new(bytes.to_vec())), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let mut format = symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(bad)?
        .format;
    let track = format.default_track().ok_or_else(|| Error::Format("MPEG: no track".into()))?;
    let id = track.id;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(bad)?;
    let mut out = Vec::new();
    let mut buf: Option<SampleBuffer<f32>> = None;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != id {
            continue;
        }
        let Ok(decoded) = decoder.decode(&packet) else {
            continue;
        };
        let channels = decoded.spec().channels.count().max(1);
        let b = buf.get_or_insert_with(|| SampleBuffer::new(decoded.capacity() as u64, *decoded.spec()));
        if b.capacity() < decoded.capacity() * channels {
            *b = SampleBuffer::new(decoded.capacity() as u64, *decoded.spec());
        }
        b.copy_interleaved_ref(decoded);
        out.extend(b.samples().chunks(channels).map(|f| f.iter().sum::<f32>() / channels as f32));
    }
    Ok(out)
}
