//! FMOD's FSB5 "CELT" samples (codec 12, header flags `0x80000010`: CELT
//! 0.11.0), decoded with the Xiph CELT 0.11.0 library vendored in
//! `third_party/celt-0.11.0` (BSD-3-Clause, built by `build.rs`).
//!
//! A sample's data is a run of frames, each a 4-byte sync word (the bytes
//! `17 C3 0D F3`), a 4-byte little-endian length, then that many bytes of
//! one CELT packet: 512 frames of audio per channel at 44.1 kHz, the mode
//! FMOD encodes with. `docs/format/halo1.md` 13 has the details.

use std::ffi::c_int;
use std::sync::OnceLock;

/// The sync bytes each frame starts with.
pub const SYNC: [u8; 4] = [0x17, 0xC3, 0x0D, 0xF3];
/// Audio frames (per channel) in one CELT packet.
pub const FRAME: usize = 512;
/// The rate FMOD's CELT mode runs at.
pub const RATE: u32 = 44_100;

#[repr(C)]
struct CeltMode {
    _private: [u8; 0],
}
#[repr(C)]
struct CeltDecoder {
    _private: [u8; 0],
}

unsafe extern "C" {
    fn celt_mode_create(fs: i32, frame_size: c_int, error: *mut c_int) -> *mut CeltMode;
    fn celt_decoder_create_custom(mode: *const CeltMode, channels: c_int, error: *mut c_int) -> *mut CeltDecoder;
    fn celt_decoder_destroy(st: *mut CeltDecoder);
    fn celt_decode(st: *mut CeltDecoder, data: *const u8, len: c_int, pcm: *mut i16, frame_size: c_int) -> c_int;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The mode or decoder could not be made (CELT error code).
    Init(i32),
    /// A frame's sync word or length is wrong, at this byte offset.
    Framing(usize),
    /// CELT refused a packet (error code), at this frame.
    Packet { frame: usize, code: i32 },
    /// Only mono and stereo are supported.
    Channels(u16),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Init(c) => write!(f, "CELT could not start (error {c})"),
            Error::Framing(o) => write!(f, "bad FSB CELT frame at byte {o}"),
            Error::Packet { frame, code } => write!(f, "CELT packet {frame} did not decode (error {code})"),
            Error::Channels(n) => write!(f, "{n} channels (only 1 and 2 are supported)"),
        }
    }
}

impl std::error::Error for Error {}

/// The one mode (44.1 kHz, 512-frame packets); never freed, shared by every
/// decoder. CELT only reads a mode after making it.
struct Mode(*mut CeltMode);
unsafe impl Send for Mode {}
unsafe impl Sync for Mode {}

fn mode() -> Result<&'static Mode, Error> {
    static MODE: OnceLock<Result<Mode, i32>> = OnceLock::new();
    MODE.get_or_init(|| {
        let mut err: c_int = 0;
        // SAFETY: plain constructor; the pointer is checked.
        let m = unsafe { celt_mode_create(RATE as i32, FRAME as c_int, &mut err) };
        if m.is_null() || err != 0 { Err(err) } else { Ok(Mode(m)) }
    })
    .as_ref()
    .map_err(|e| Error::Init(*e))
}

struct Decoder(*mut CeltDecoder);

impl Drop for Decoder {
    fn drop(&mut self) {
        // SAFETY: made by celt_decoder_create_custom, destroyed once.
        unsafe { celt_decoder_destroy(self.0) }
    }
}

/// The frames of one sample: (offset of the packet, its length).
pub fn packets(data: &[u8]) -> Result<Vec<(usize, usize)>, Error> {
    let mut out = Vec::new();
    let mut o = 0;
    while o + 8 <= data.len() {
        if data[o..o + 4] != SYNC {
            // Padding after the last frame (samples start on 32-byte
            // boundaries); a few 4-channel samples have a stray partial
            // frame after it.
            if !out.is_empty() && data[o..o + 4] == [0; 4] {
                break;
            }
            return Err(Error::Framing(o));
        }
        let len = u32::from_le_bytes(data[o + 4..o + 8].try_into().unwrap()) as usize;
        if len == 0 || o + 8 + len > data.len() {
            if !out.is_empty() {
                break;
            }
            return Err(Error::Framing(o));
        }
        out.push((o + 8, len));
        o += 8 + len;
    }
    Ok(out)
}

/// Decodes one sample's data to interleaved 16-bit PCM at 44.1 kHz, cut to
/// `frames` (the FSB header's length) when that is given and shorter.
pub fn decode(data: &[u8], channels: u16, frames: Option<usize>) -> Result<Vec<i16>, Error> {
    if !(1..=2).contains(&channels) {
        return Err(Error::Channels(channels));
    }
    let m = mode()?;
    let mut err: c_int = 0;
    // SAFETY: the mode lives for the program; the pointer is checked.
    let st = unsafe { celt_decoder_create_custom(m.0, channels as c_int, &mut err) };
    if st.is_null() || err != 0 {
        return Err(Error::Init(err));
    }
    let dec = Decoder(st);
    let packets = packets(data)?;
    let ch = channels as usize;
    let mut pcm = vec![0i16; packets.len() * FRAME * ch];
    for (i, (at, len)) in packets.iter().enumerate() {
        let out = &mut pcm[i * FRAME * ch..(i + 1) * FRAME * ch];
        // SAFETY: `out` holds exactly FRAME frames of `ch` channels, the
        // packet slice is in bounds.
        let code = unsafe { celt_decode(dec.0, data[*at..].as_ptr(), *len as c_int, out.as_mut_ptr(), FRAME as c_int) };
        if code < 0 {
            return Err(Error::Packet { frame: i, code });
        }
    }
    if let Some(n) = frames
        && n * ch < pcm.len()
    {
        pcm.truncate(n * ch);
    }
    Ok(pcm)
}

#[cfg(test)]
mod tests {
    #[test]
    fn mode_and_framing() {
        assert!(super::mode().is_ok());
        // Two empty-looking frames: framing is read, zero padding ends it.
        let mut d = Vec::new();
        for _ in 0..2 {
            d.extend_from_slice(&super::SYNC);
            d.extend_from_slice(&4u32.to_le_bytes());
            d.extend_from_slice(&[0, 0, 0, 0]);
        }
        d.extend_from_slice(&[0; 8]);
        assert_eq!(super::packets(&d).unwrap(), vec![(8, 4), (20, 4)]);
        assert!(super::packets(&[1, 2, 3, 4, 5, 6, 7, 8]).is_err());
        // A partial frame after the padding ends the sample.
        d.extend_from_slice(&super::SYNC);
        d.extend_from_slice(&64u32.to_le_bytes());
        assert_eq!(super::packets(&d).unwrap().len(), 2);
    }
}
