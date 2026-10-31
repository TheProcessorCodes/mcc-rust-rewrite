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

const STEAM_FOLDER: &str = r"steamapps\common\MX vs ATV Reflex";

/// Whether `root` is a Reflex install (it has `Database\MXs.dx9.package`).
pub fn is_reflex(root: &Path) -> bool {
    root.join("Database").join("MXs.dx9.package").is_file()
}

/// The Reflex install: `$REFLEX_GAME` (a developer override), else Steam's
/// usual places and every Steam library listed in `libraryfolders.vdf`.
/// `None` when there is no Reflex; the bike is then drawn as sticks.
pub fn find_reflex() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("REFLEX_GAME").map(PathBuf::from) {
        return is_reflex(&p).then_some(p);
    }
    steam_libraries()
        .into_iter()
        .map(|lib| lib.join(STEAM_FOLDER))
        .find(|p| is_reflex(p))
}

/// Steam's own folder (from the usual install places) and the libraries
/// its `libraryfolders.vdf` lists.
fn steam_libraries() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut steams: Vec<PathBuf> = ["ProgramFiles(x86)", "ProgramFiles"]
        .iter()
        .filter_map(std::env::var_os)
        .map(|p| PathBuf::from(p).join("Steam"))
        .collect();
    steams.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    for steam in steams {
        if !steam.is_dir() || out.contains(&steam) {
            continue;
        }
        out.push(steam.clone());
        let Ok(vdf) = std::fs::read_to_string(steam.join(r"steamapps\libraryfolders.vdf")) else {
            continue;
        };
        for line in vdf.lines() {
            let parts: Vec<&str> = line.split('"').collect();
            if parts.len() >= 4 && parts[1] == "path" {
                let lib = PathBuf::from(parts[3].replace(r"\\", r"\"));
                if !out.contains(&lib) {
                    out.push(lib);
                }
            }
        }
    }
    out
}
