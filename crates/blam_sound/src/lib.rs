//! Halo 3's sound, from the user's own MCC install, read-only.
//!
//! MCC moved Halo 3's audio out of the `.map` files: a `snd!` tag's
//! resource has no data pages, and its permutations name samples of an
//! FMOD bank instead (`halo3\fmod\pc\sfx.fsb`, FSB5 with Vorbis; dialogue
//! in one bank per language, `english.fsb` ..., read the same way). This crate reads both sides: the tags
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
pub mod weapons;

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

/// The languages MCC ships Halo 3's dialogue in: one bank each,
/// `halo3\fmod\pc\<language>.fsb` (`sound.md` 0).
pub const LANGUAGES: [&str; 6] = ["english", "french", "german", "italian", "mexican", "spanish"];

/// The sound effects bank, the dialogue bank of one language when asked
/// for, and the decoder tables, ready to decode by FMOD id.
pub struct Library {
    pub sfx: Bank,
    /// The language bank (`english.fsb` ...): every line of dialogue.
    pub dialog: Option<Bank>,
    pub setups: Setups,
}

impl Library {
    /// Opens `halo3\fmod\pc\sfx.fsb` and reads the Vorbis setups from
    /// `halo3\halo3.dll`, given the `halo3\maps` folder. No dialogue.
    pub fn open_beside(maps_dir: impl AsRef<Path>) -> Result<Library> {
        Library::open_beside_with(maps_dir, None)
    }

    /// Like [`Library::open_beside`], with the dialogue bank of `language`
    /// (one of [`LANGUAGES`]) as well.
    pub fn open_beside_with(maps_dir: impl AsRef<Path>, language: Option<&str>) -> Result<Library> {
        let halo3 = maps_dir
            .as_ref()
            .parent()
            .ok_or_else(|| Error::Invalid("the maps folder has no parent".into()))?;
        let mut lib = Library::open(halo3.join(r"fmod\pc\sfx.fsb"), halo3.join("halo3.dll"))?;
        if let Some(lang) = language {
            lib.add_dialog(language_bank(halo3, lang)?)?;
        }
        Ok(lib)
    }

    pub fn open(bank: impl AsRef<Path>, dll: impl AsRef<Path>) -> Result<Library> {
        let sfx = Bank::open(bank)?;
        let setups = Setups::from_dll(dll)?;
        check_setups(&sfx, &setups)?;
        Ok(Library {
            sfx,
            dialog: None,
            setups,
        })
    }

    /// Opens a language bank (`<halo3>\fmod\pc\english.fsb`) for the
    /// dialogue.
    pub fn add_dialog(&mut self, bank: impl AsRef<Path>) -> Result<()> {
        let bank = Bank::open(bank)?;
        check_setups(&bank, &self.setups)?;
        self.dialog = Some(bank);
        Ok(())
    }

    /// The sfx bank's sample of an FMOD id (not the dialogue's: see
    /// [`Library::find`]).
    pub fn sample(&self, fmod_id: u32) -> Option<u32> {
        self.sfx.find(fmod_id)
    }

    /// The bank and sample of an FMOD id: the sfx bank first, then the
    /// dialogue bank.
    pub fn find(&self, fmod_id: u32) -> Option<(&Bank, u32)> {
        if let Some(i) = self.sfx.find(fmod_id) {
            return Some((&self.sfx, i));
        }
        let d = self.dialog.as_ref()?;
        d.find(fmod_id).map(|i| (d, i))
    }

    /// Decodes the sample an FMOD id names, cut to its loop region when it
    /// has one (the info's loop end).
    pub fn decode(&self, fmod_id: u32) -> Result<Pcm> {
        let (bank, i) = self
            .find(fmod_id)
            .ok_or_else(|| Error::Invalid(format!("FMOD id {fmod_id:#010x} is not in the banks")))?;
        let mut pcm = vorbis::decode(bank, &self.setups, i)?;
        let end = bank.infos[i as usize].loop_end as usize;
        if end > 0 && (end + 1) < pcm.frames() {
            pcm.samples.truncate((end + 1) * pcm.channels as usize);
        }
        Ok(pcm)
    }
}

fn check_setups(bank: &Bank, setups: &Setups) -> Result<()> {
    if let Some(crc) = bank.setup_crcs().into_iter().find(|c| !setups.verified(*c)) {
        return Err(Error::Unsupported(format!(
            "{} uses Vorbis setup {crc:#010x}, which could not be rebuilt from the DLL",
            bank.path().display()
        )));
    }
    Ok(())
}

/// The dialogue bank of `language` (one of [`LANGUAGES`]) in the `halo3`
/// folder: `fmod\pc\<language>.fsb`.
pub fn language_bank(halo3_dir: &Path, language: &str) -> Result<PathBuf> {
    if !LANGUAGES.contains(&language) {
        return Err(Error::Invalid(format!("no dialogue language {language:?}; MCC has {LANGUAGES:?}")));
    }
    Ok(halo3_dir.join("fmod").join("pc").join(format!("{language}.fsb")))
}

/// Seconds of every sample of a bank by FMOD id, from its `.info` sidecar
/// alone (frames = PCM bytes / channels / 2, at 48 kHz, the only rate the
/// banks hold): what a script waiting on a line's length needs, without
/// opening the bank.
pub fn info_seconds(bank: &Path) -> Result<std::collections::HashMap<u32, f32>> {
    let mut info = bank.as_os_str().to_owned();
    info.push(".info");
    let info = PathBuf::from(info);
    let bytes = std::fs::read(&info).map_err(|source| Error::Io { path: info, source })?;
    Ok(fsb::parse_info(&bytes)?
        .into_iter()
        .filter(|r| r.channels > 0)
        .map(|r| (r.id, r.pcm_bytes as f32 / (r.channels as f32 * 2.0) / 48_000.0))
        .collect())
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
