//! Where the player's own games are. Nothing here names a folder on any one
//! PC, and nothing searches the disk: the paths come only from
//!
//! 1. dev overrides in the environment (`HALO3_MCC`, `HALO3_MAPS`,
//!    `HALO2_MAPS`, `HALO1_MAPS`, `HALOREACH_MAPS`, `HALO3_SKATE_ASSETS`, `IW4L_SKATE_ASSETS`,
//!    `REFLEX_GAME`, `HALO3_LANGUAGE`);
//! 2. the folders the player chose in first-run setup (`halo3_setup`),
//!    saved in `setup.env` under [`settings_dir`]
//!    (`%LOCALAPPDATA%\Halo3Rewrite`), so they are asked once per user
//!    however many copies of the exe there are.
//!
//! Before setup has run (and without overrides) every lookup is `None`;
//! tests then skip.
//!
//! No bevy and no other dependencies: the `blam_*` readers' tests use it too.

use std::path::{Path, PathBuf};

/// The setup keys `setup.env` holds.
pub mod key {
    /// The MCC install folder (the one holding `halo3\maps`).
    pub const MCC: &str = "MCC";
    /// The converted Skate 3 data (holding `private\game.json`).
    pub const SKATE_ASSETS: &str = "SKATE_ASSETS";
    /// The Skate 3 converter, `skate\iw4l-skate-convert.exe` in the player's
    /// MW2 Skate 3 folder.
    pub const SKATE_CONVERTER: &str = "SKATE_CONVERTER";
    /// The MX vs ATV Reflex install, or [`REFLEX_SKIPPED`](super::REFLEX_SKIPPED).
    pub const REFLEX: &str = "REFLEX";
    /// The dialogue language (`english`, `french`, ...); optional, see
    /// [`language`](super::language).
    pub const LANGUAGE: &str = "LANGUAGE";
}

/// What `REFLEX` holds when the player chose to go without Reflex.
pub const REFLEX_SKIPPED: &str = "skip";

/// Whether the player chose in setup to play without Reflex (the bike is
/// then our own model, with no engine sound). `REFLEX_GAME=skip` does the same
/// for a dev run.
pub fn reflex_skipped() -> bool {
    match std::env::var_os("REFLEX_GAME") {
        Some(v) => v == REFLEX_SKIPPED,
        None => settings().get(key::REFLEX) == Some(REFLEX_SKIPPED),
    }
}

/// Whether `root` is an MX vs ATV Reflex install (it has
/// `Database\MXs.dx9.package`).
pub fn is_reflex(root: &Path) -> bool {
    root.join("Database").join("MXs.dx9.package").is_file()
}

/// The MX vs ATV Reflex install (the dirt bike's model and engine sound):
/// `%REFLEX_GAME%`, else the one the player chose in setup. Reflex is
/// optional; `None` means the bike uses our own.
pub fn reflex_root() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("REFLEX_GAME").map(PathBuf::from) {
        return is_reflex(&p).then_some(p);
    }
    saved_reflex()
}

/// The Reflex install the setup saved, when it is still there.
pub fn saved_reflex() -> Option<PathBuf> {
    settings().get(key::REFLEX).map(PathBuf::from).filter(|p| is_reflex(p))
}

/// Where the setup and the converted Skate 3 data live:
/// `%HALO3_SETUP_DIR%`, else `%LOCALAPPDATA%\Halo3Rewrite`.
pub fn settings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HALO3_SETUP_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Halo3Rewrite")
}

/// The saved setup file.
pub fn settings_path() -> PathBuf {
    settings_dir().join("setup.env")
}

/// The saved setup.
pub fn settings() -> Settings {
    Settings::read(&settings_path())
}

/// Whether `root` is an MCC install with Halo 3 in it.
pub fn is_mcc(root: &Path) -> bool {
    root.join("halo3").join("maps").join("shared.map").is_file()
}

/// The MCC folder for a pick of MCC itself, its `halo3` folder or its
/// `halo3\maps` folder.
pub fn mcc_from_pick(picked: &Path) -> Option<PathBuf> {
    picked.ancestors().take(3).find(|p| is_mcc(p)).map(Path::to_path_buf)
}

/// The MCC install: `%HALO3_MCC%`, the install around `%HALO3_MAPS%`, the
/// one the player chose in setup.
pub fn mcc_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HALO3_MCC") {
        return Some(PathBuf::from(dir));
    }
    if let Some(maps) = std::env::var_os("HALO3_MAPS").map(PathBuf::from) {
        if let Some(root) = maps.parent().and_then(Path::parent) {
            return Some(root.to_path_buf());
        }
    }
    saved_mcc()
}

/// The MCC install the setup saved, when it is still there.
pub fn saved_mcc() -> Option<PathBuf> {
    settings().get(key::MCC).map(PathBuf::from).filter(|p| is_mcc(p))
}

/// Halo 3's maps folder: `%HALO3_MAPS%`, else `halo3\maps` in MCC.
pub fn halo3_maps() -> Option<PathBuf> {
    std::env::var_os("HALO3_MAPS")
        .map(PathBuf::from)
        .or_else(|| Some(mcc_root()?.join("halo3").join("maps")))
}

/// Halo 2 (classic)'s maps folder: `%HALO2_MAPS%`, else MCC's
/// `halo2\h2_maps_win64_dx11`.
pub fn halo2_maps() -> Option<PathBuf> {
    std::env::var_os("HALO2_MAPS")
        .map(PathBuf::from)
        .or_else(|| Some(mcc_root()?.join("halo2").join("h2_maps_win64_dx11")))
}

/// MCC's Halo: Combat Evolved maps: `%HALO1_MAPS%`, else `halo1\maps` in MCC.
pub fn halo1_maps() -> Option<PathBuf> {
    std::env::var_os("HALO1_MAPS")
        .map(PathBuf::from)
        .or_else(|| Some(mcc_root()?.join("halo1").join("maps")))
}

/// MCC's Halo: Reach maps: `%HALOREACH_MAPS%`, else `haloreach\maps` in MCC.
pub fn reach_maps() -> Option<PathBuf> {
    std::env::var_os("HALOREACH_MAPS")
        .map(PathBuf::from)
        .or_else(|| Some(mcc_root()?.join("haloreach").join("maps")))
}

/// The dialogue languages MCC ships for Halo 3 (`halo3\fmod\pc\<name>.fsb`).
pub const LANGUAGES: [&str; 6] = ["english", "french", "german", "italian", "mexican", "spanish"];

/// The language Halo 3's dialogue plays in: `%HALO3_LANGUAGE%` (a dev
/// override), else `LANGUAGE` in the saved setup (`key::LANGUAGE`; setup
/// does not ask, so it is only there when written by hand), else
/// `english`. Anything not in [`LANGUAGES`] falls back to `english`.
pub fn language() -> &'static str {
    let wanted = std::env::var("HALO3_LANGUAGE")
        .ok()
        .or_else(|| settings().get(key::LANGUAGE).map(str::to_string))
        .map(|l| l.trim().to_ascii_lowercase());
    wanted
        .and_then(|w| LANGUAGES.iter().copied().find(|l| *l == w))
        .unwrap_or("english")
}

/// A Halo 3 map by bare name (`riverworld`), when it is installed. For tests.
pub fn halo3_map(name: &str) -> Option<PathBuf> {
    let p = halo3_maps()?.join(format!("{name}.map"));
    p.is_file().then_some(p)
}

/// A Halo 2 map by bare name (`coagulation`), when it is installed.
pub fn halo2_map(name: &str) -> Option<PathBuf> {
    let p = halo2_maps()?.join(format!("{name}.map"));
    p.is_file().then_some(p)
}

/// Every file the skate host, the rider and the board read, beside
/// `board.json` and `rig.json` (which setup derives from `skater.glb`).
pub const SKATE_FILES: [&str; 4] = [
    "private/skater.glb",
    "private/game.json",
    "private/stock/physics-skeletons.json",
    "private/stock/skater-collections.json",
];

/// Whether `assets` holds converted Skate 3 data.
pub fn is_skate_assets(assets: &Path) -> bool {
    SKATE_FILES.iter().all(|f| assets.join(f).is_file())
}

/// The converted Skate 3 data: `%HALO3_SKATE_ASSETS%` or
/// `%IW4L_SKATE_ASSETS%`, else the saved setup. None before setup has run.
pub fn skate_assets() -> Option<PathBuf> {
    ["HALO3_SKATE_ASSETS", "IW4L_SKATE_ASSETS"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .or_else(|| settings().get(key::SKATE_ASSETS).map(PathBuf::from))
}

/// `KEY=value` lines, rewritten in place: keys that are set are replaced and
/// every other line is kept as the player left it.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    lines: Vec<String>,
}

impl Settings {
    pub fn read(path: &Path) -> Self {
        let lines = std::fs::read_to_string(path)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default();
        Self { lines }
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().rev().find_map(|line| {
            let (name, value) = line.split_once('=')?;
            (name.trim() == key).then(|| value.trim().trim_matches('"')).filter(|v| !v.is_empty())
        })
    }

    pub fn set(&mut self, key: &str, value: &str) {
        self.remove(key);
        self.lines.push(format!("{key}=\"{value}\""));
    }

    pub fn set_path(&mut self, key: &str, value: &Path) {
        self.set(key, &value.display().to_string());
    }

    pub fn remove(&mut self, key: &str) {
        self.lines.retain(|line| line.split_once('=').is_none_or(|(name, _)| name.trim() != key));
    }

    /// Writes the file, creating its folder.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let mut text = self.lines.join("\n");
        text.push('\n');
        std::fs::write(path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_replace_keys_and_keep_other_lines() {
        let mut s = Settings { lines: vec!["# mine".into(), "MCC=\"old\"".into()] };
        s.set(key::MCC, r"E:\Games\MCC");
        s.set(key::REFLEX, r"E:\Games\Reflex");
        assert_eq!(s.get(key::MCC), Some(r"E:\Games\MCC"));
        assert_eq!(s.get(key::REFLEX), Some(r"E:\Games\Reflex"));
        assert_eq!(s.lines[0], "# mine");
        assert_eq!(s.lines.len(), 3);
        s.remove(key::REFLEX);
        assert_eq!(s.get(key::REFLEX), None);
    }
}
