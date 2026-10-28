//! Halo 3's sound, from the user's own MCC install, read-only.
//!
//! MCC moved Halo 3's audio out of the `.map` files: a `snd!` tag's
//! resource has no data pages, and its permutations name samples of an
//! FMOD bank instead (`halo3\fmod\pc\sfx.fsb`, FSB5 with Vorbis; dialogue
//! in one bank per language). This crate reads both sides: the tags
//! ([`tags`]) and the bank ([`fsb`], [`vorbis`]). `docs/format/sound.md`
//! has the layouts and how they were found.
//!
//! ```no_run
//! let maps = std::path::Path::new(r"C:\...\halo3\maps");
//! let cache = blam_cache::CacheFile::open(maps.join("riverworld.map"))?;
//! let library = blam_sound::Library::open_beside(maps)?;
//! let tags = blam_sound::tags::SoundTags::new(&cache)?;
//! let sound = tags.sound(6786)?;
//! let pcm = library.decode(sound.permutations[0].fmod_id)?;
//! # Ok::<(), blam_sound::Error>(())
//! ```

pub mod fsb;
pub mod tags;
pub mod vorbis;

use std::io::Write;
use std::path::{Path, PathBuf};

pub use fsb::{Bank, Info, Sample};
pub use vorbis::{Decoder, Pcm, Setups};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("not supported: {0}")]
    Unsupported(String),
    #[error("decode: {0}")]
    Decode(String),
    #[error(transparent)]
    Cache(#[from] blam_cache::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// The sound effects bank and the decoder tables, ready to decode by FMOD
/// id.
pub struct Library {
    pub sfx: Bank,
    pub setups: Setups,
}

impl Library {
    /// Opens `halo3\fmod\pc\sfx.fsb` and reads the Vorbis setups from
    /// `halo3\halo3.dll`, given the `halo3\maps` folder.
    pub fn open_beside(maps_dir: impl AsRef<Path>) -> Result<Library> {
        let halo3 = maps_dir
            .as_ref()
            .parent()
            .ok_or_else(|| Error::Invalid("the maps folder has no parent".into()))?;
        Library::open(halo3.join(r"fmod\pc\sfx.fsb"), halo3.join("halo3.dll"))
    }

    pub fn open(bank: impl AsRef<Path>, dll: impl AsRef<Path>) -> Result<Library> {
        let sfx = Bank::open(bank)?;
        let setups = Setups::from_dll(dll)?;
        if let Some(crc) = sfx.setup_crcs().into_iter().find(|c| !setups.verified(*c)) {
            return Err(Error::Unsupported(format!(
                "the bank uses Vorbis setup {crc:#010x}, which could not be rebuilt from the DLL"
            )));
        }
        Ok(Library { sfx, setups })
    }

    /// The bank sample of an FMOD id.
    pub fn sample(&self, fmod_id: u32) -> Option<u32> {
        self.sfx.find(fmod_id)
    }

    /// Decodes the sample an FMOD id names, cut to its loop region when it
    /// has one (the info's loop end).
    pub fn decode(&self, fmod_id: u32) -> Result<Pcm> {
        let i = self
            .sample(fmod_id)
            .ok_or_else(|| Error::Invalid(format!("FMOD id {fmod_id:#010x} is not in the bank")))?;
        let mut pcm = vorbis::decode(&self.sfx, &self.setups, i)?;
        let end = self.sfx.infos[i as usize].loop_end as usize;
        if end > 0 && (end + 1) < pcm.frames() {
            pcm.samples.truncate((end + 1) * pcm.channels as usize);
        }
        Ok(pcm)
    }
}

/// Writes 16-bit PCM WAV.
pub fn write_wav(path: impl AsRef<Path>, pcm: &Pcm) -> std::io::Result<()> {
    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
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
        f.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
    }
    f.flush()
}
