//! Where the player's own games are. Nothing here names a folder on any one
//! PC: the paths come from, in order,
//!
//! 1. dev overrides in the environment (`HALO3_MCC`, `HALO3_MAPS`,
//!    `HALO2_MAPS`, `HALO3_SKATE_ASSETS`, `IW4L_SKATE_ASSETS`);
//! 2. the first-run setup (`halo3_setup`), saved in `setup.env` under
//!    [`settings_dir`] (`%LOCALAPPDATA%\Halo3Rewrite`), so it is asked once
//!    per user however many copies of the exe there are;
//! 3. for MCC only, Steam's own library list, so tests and the `blam` tool
//!    work before setup has run.
//!
//! No bevy and no other dependencies: the `blam_*` readers' tests use it too.

use std::path::{Path, PathBuf};

/// The setup keys `setup.env` holds.
pub mod key {
    /// The MCC install folder (the one holding `halo3\maps`).
    pub const MCC: &str = "MCC";
    /// The converted Skate 3 data (holding `private\game.json`).
    pub const SKATE_ASSETS: &str = "SKATE_ASSETS";
    /// `off` when the player chose to play without Skate 3.
    pub const SKATE: &str = "SKATE";
    /// The Skate 3 converter, when it is not beside the exe.
    pub const SKATE_CONVERTER: &str = "SKATE_CONVERTER";
}

/// Steam's app id for Halo: The Master Chief Collection.
pub const MCC_STEAM_APP: &str = "976730";
/// MCC's folder name under `steamapps\common`.
pub const MCC_FOLDER: &str = "Halo The Master Chief Collection";

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

/// The MCC install: `%HALO3_MCC%`, the saved setup, else the first Steam
/// library holding one.
pub fn mcc_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("HALO3_MCC") {
        return Some(PathBuf::from(dir));
    }
    saved_mcc().or_else(find_mcc)
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
/// `%IW4L_SKATE_ASSETS%`, else the saved setup. None when the player has
/// not given Skate 3.
pub fn skate_assets() -> Option<PathBuf> {
    ["HALO3_SKATE_ASSETS", "IW4L_SKATE_ASSETS"]
        .iter()
        .find_map(std::env::var_os)
        .map(PathBuf::from)
        .or_else(|| settings().get(key::SKATE_ASSETS).map(PathBuf::from))
}

/// The first MCC found in a Steam library, or an Xbox app install.
pub fn find_mcc() -> Option<PathBuf> {
    steam_libraries()
        .into_iter()
        .map(|l| l.join("steamapps").join("common").join(MCC_FOLDER))
        .chain(('C'..='Z').map(|d| PathBuf::from(format!(r"{d}:\XboxGames\{MCC_FOLDER}\Content"))))
        .find(|p| is_mcc(p))
}

/// Steam's own folder (from the registry, else the usual install places),
/// every library its `libraryfolders.vdf` lists, and `SteamLibrary` on each
/// drive.
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut steams: Vec<PathBuf> = steam_from_registry().into_iter().collect();
    for dir in [r"C:\Program Files (x86)\Steam", r"C:\Program Files\Steam"] {
        steams.push(PathBuf::from(dir));
    }
    let mut libraries = Vec::new();
    for steam in steams {
        if let Ok(text) = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")) {
            libraries.extend(vdf_paths(&text));
        }
        libraries.push(steam);
    }
    for drive in 'C'..='Z' {
        libraries.push(PathBuf::from(format!(r"{drive}:\SteamLibrary")));
    }
    let mut seen = std::collections::HashSet::new();
    libraries.retain(|p| seen.insert(p.to_string_lossy().to_lowercase().replace('/', "\\")));
    libraries
}

/// The `"path"` values of a `libraryfolders.vdf`.
fn vdf_paths(text: &str) -> Vec<PathBuf> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split('"').filter(|f| !f.trim().is_empty());
            (fields.next() == Some("path")).then(|| fields.next()).flatten()
        })
        .map(|p| PathBuf::from(p.replace(r"\\", r"\")))
        .collect()
}

/// `HKCU\Software\Valve\Steam\SteamPath`, through `reg.exe`.
fn steam_from_registry() -> Option<PathBuf> {
    if !cfg!(windows) {
        return None;
    }
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Software\Valve\Steam", "/v", "SteamPath"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().find(|l| l.trim_start().starts_with("SteamPath"))?;
    let value = line.split("REG_SZ").nth(1)?.trim();
    (!value.is_empty()).then(|| PathBuf::from(value.replace('/', "\\")))
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
        s.set(key::SKATE, "off");
        assert_eq!(s.get(key::MCC), Some(r"E:\Games\MCC"));
        assert_eq!(s.get(key::SKATE), Some("off"));
        assert_eq!(s.lines[0], "# mine");
        assert_eq!(s.lines.len(), 3);
        s.remove(key::SKATE);
        assert_eq!(s.get(key::SKATE), None);
    }

    #[test]
    fn vdf_library_paths() {
        let text = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t}\n\t\"1\"\n\t{\n\t\t\"path\"\t\t\"D:\\\\SteamLibrary\"\n\t}\n}\n";
        assert_eq!(
            vdf_paths(text),
            vec![PathBuf::from(r"C:\Program Files (x86)\Steam"), PathBuf::from(r"D:\SteamLibrary")]
        );
    }
}
