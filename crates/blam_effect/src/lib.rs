//! Halo 3 effects (MCC PC): `effe` tags and what they play, read from a
//! map without a renderer: events, parts (sounds, lights, debris, other
//! effects), particle systems with their emitters (`prt3` particles,
//! `pmov` particle physics), contrails (`cntl`) and lights (`ligh`).
//!
//! Written from the layouts in `docs/format/effects.md`, which were found
//! in the bytes of the user's 120_halo with the probes under
//! `docs/format/scripts/` (`probe_effects.py`). No code from other
//! projects is copied.
//!
//! ```no_run
//! let cache = blam_cache::CacheFile::open("120_halo.map")?;
//! let tag = cache.tags.find("effe", r"levels\solo\120_halo\fx\trench_run\plate_explosion\plate_explosion").unwrap();
//! let lib = blam_effect::Library::load(&cache, &[tag.index]);
//! let effect = lib.effect(tag.index).unwrap();
//! for event in &effect.events {
//!     println!("{} {} parts, {} particle systems", event.name, event.parts.len(), event.systems.len());
//! }
//! # Ok::<(), blam_cache::Error>(())
//! ```

pub mod function;
pub mod library;
pub mod tags;

pub use function::Function;
pub use library::Library;
pub use tags::*;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Cache(#[from] blam_cache::Error),
    #[error(transparent)]
    Texture(#[from] blam_texture::Error),
    #[error("invalid data: {0}")]
    Invalid(String),
    #[error("unsupported: {0}")]
    Unsupported(String),
}

pub type Result<T> = std::result::Result<T, Error>;
