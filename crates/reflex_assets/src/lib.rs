//! MX vs ATV Reflex's own files, read from the user's install (never
//! written, never copied into this project): the database/package pairs in
//! `<Reflex>\Database`, and in them the bike meshes, skeletons and
//! textures. `bike::load` puts together the Dakumoto 450F with the Cortana
//! graphics kit for `halo3_bike`; `engine_audio::load_450f` its engine
//! recordings for `halo3_audio`.
//!
//! The formats were worked out in `D:\reflex-tools` (the Reflex track and
//! skin tools); `docs/format/reflex-bike.md` has the notes this is written
//! from.

pub mod bike;
pub mod bxml;
pub mod cortana;
pub mod engine_audio;
pub mod model;
pub mod package;
pub mod surface;
pub mod texture;

use std::path::{Path, PathBuf};

/// Reflex's models are in feet.
pub const FT: f32 = 0.3048;

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Format(String),
    Missing(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Format(s) => write!(f, "bad Reflex file: {s}"),
            Error::Missing(s) => write!(f, "not in the Reflex files: {s}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

/// Whether `root` is a Reflex install (it has `Database\MXs.dx9.package`).
pub fn is_reflex(root: &Path) -> bool {
    game_paths::is_reflex(root)
}

/// The Reflex install: `$REFLEX_GAME` (a developer override), the one
/// first-run setup saved (`halo3_setup`), else Steam's libraries
/// (`game_paths::reflex_root`). `None` when there is no Reflex; the bike is
/// then drawn as sticks.
pub fn find_reflex() -> Option<PathBuf> {
    game_paths::reflex_root()
}
