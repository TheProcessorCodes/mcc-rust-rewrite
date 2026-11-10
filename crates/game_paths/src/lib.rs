//! Where the player's own games are. Nothing here names a folder on any one
//! PC, and nothing searches the disk: the paths come only from
//!
//! 1. dev overrides in the environment (`MCC_ROOT`, `HALO3_MAPS`,
//!    `HALO2_MAPS`, `HALO1_MAPS`, `HALOREACH_MAPS`, `MCC_SKATE_ASSETS`, `IW4L_SKATE_ASSETS`,
//!    `REFLEX_GAME`, `MCC_LANGUAGE`);
//! 2. the folders the player chose in first-run setup (`mcc_setup`),
//!    saved in `setup.env` under [`settings_dir`]
//!    (`%LOCALAPPDATA%\MccRustRewrite`), so they are asked once per user
//!    however many copies of the exe there are.
//!
//! Before setup has run (and without overrides) every lookup is `None`;
//! tests then skip.
//!
//! No bevy and no other dependencies: the `blam_*` readers' tests use it too.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

/// The setup keys `setup.env` holds.
pub mod key {
    /// The MCC install folder (the one holding `halo3\maps`).
    pub const MCC: &str = "MCC";
    /// The converted Skate 3 data (holding `private\game.json`), or
    /// [`SKATE_SKIPPED`](super::SKATE_SKIPPED) when the player chose to go
    /// without Skate 3.
    pub const SKATE_ASSETS: &str = "SKATE_ASSETS";
    /// The folder the player chose for Skate 3 (the extracted game or the
    /// one holding the `.iso`), kept to show in the settings. Not there
    /// when Skate 3 was skipped.
    pub const SKATE_SOURCE: &str = "SKATE_SOURCE";
    /// The player's name, shown in the kill feed.
    pub const USERNAME: &str = "USERNAME";
    /// The Skate 3 converter, `skate\iw4l-skate-convert.exe` in the player's
    /// MW2 Skate 3 folder.
    pub const SKATE_CONVERTER: &str = "SKATE_CONVERTER";
    /// The MX vs ATV Reflex install, or [`REFLEX_SKIPPED`](super::REFLEX_SKIPPED).
    pub const REFLEX: &str = "REFLEX";
    /// The speedometer's unit: `mph` (the default) or `kph`.
    pub const SPEED_UNIT: &str = "SPEED_UNIT";
    /// The dialogue language (`english`, `french`, ...); optional, see
    /// [`language`](super::language).
    pub const LANGUAGE: &str = "LANGUAGE";
    /// Whether skating is on: `off` turns it off; anything else, or no
    /// line, leaves it on. See [`skating_enabled`](super::skating_enabled).
    pub const SKATING: &str = "SKATING";
}

/// What `REFLEX` holds when the player chose to go without Reflex.
pub const REFLEX_SKIPPED: &str = "skip";

/// What `SKATE_ASSETS` holds when the player chose to go without Skate 3.
pub const SKATE_SKIPPED: &str = "skip";

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

/// Where what the game needs from Reflex is kept once imported
/// (`reflex_assets::imported`): `reflex-1` under [`settings_dir`]. The
/// number goes up when the import's files change, so an old one is redone.
pub fn reflex_import_dir() -> PathBuf {
    settings_dir().join("reflex-1")
}

/// Whether Reflex has been imported (its `done` file is written last).
pub fn reflex_imported() -> bool {
    reflex_import_dir().join("done").is_file()
}

/// Whether the bike has Reflex's model, sound and physics: imported, or an
/// install still there to import from. Once imported the install is not
/// looked at again. Never when the player chose to go without Reflex.
pub fn reflex_ready() -> bool {
    !reflex_skipped() && (reflex_imported() || reflex_root().is_some())
}

/// The Reflex folder the player chose, as saved, whether or not it is still
/// there (to show in the settings). None when skipped or never chosen.
pub fn reflex_chosen() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("REFLEX_GAME").filter(|v| v != REFLEX_SKIPPED) {
        return Some(PathBuf::from(p));
    }
    settings().get(key::REFLEX).filter(|v| *v != REFLEX_SKIPPED).map(PathBuf::from)
}

/// Where the setup and the converted Skate 3 data live:
/// `%MCC_SETUP_DIR%`, else `%LOCALAPPDATA%\MccRustRewrite`.
pub fn settings_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MCC_SETUP_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local").join("share")))
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("MccRustRewrite");
    static MOVED: std::sync::Once = std::sync::Once::new();
    MOVED.call_once(|| move_old_settings(&base.join("Halo3Rewrite"), &dir));
    dir
}

/// Moves the setup saved under the project's old name (`Halo3Rewrite`) to
/// `new`, once, so a player who set up before the rename is not asked again.
/// The saved paths that pointed inside the old folder are pointed at the new.
fn move_old_settings(old: &Path, new: &Path) {
    if new.exists() || !old.is_dir() || std::fs::rename(old, new).is_err() {
        return;
    }
    let file = new.join("setup.env");
    let (Some(old_text), Some(new_text)) = (old.to_str(), new.to_str()) else {
        return;
    };
    if let Ok(saved) = std::fs::read_to_string(&file) {
        let _ = std::fs::write(&file, saved.replace(old_text, new_text));
    }
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

/// The MCC install: `%MCC_ROOT%`, the install around `%HALO3_MAPS%`, the
/// one the player chose in setup.
pub fn mcc_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("MCC_ROOT") {
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

/// MCC's Halo CE sound banks (`sounds_music.fsb`, `sounds_dialogs*.fsb`, ...
/// with their indexes in `lst\`): `%HALO1_SOUND%`, else `sound\pc` beside
/// the Halo CE maps folder (`halo1\sound\pc`).
pub fn halo1_sound() -> Option<PathBuf> {
    std::env::var_os("HALO1_SOUND")
        .map(PathBuf::from)
        .or_else(|| Some(halo1_maps()?.parent()?.join("sound").join("pc")))
}

/// A Halo CE map by bare name (`d40`), when it is installed. For tests.
pub fn halo1_map(name: &str) -> Option<PathBuf> {
    let p = halo1_maps()?.join(format!("{name}.map"));
    p.is_file().then_some(p)
}

/// MCC's Halo: Reach maps: `%HALOREACH_MAPS%`, else `haloreach\maps` in MCC.
pub fn reach_maps() -> Option<PathBuf> {
    std::env::var_os("HALOREACH_MAPS")
        .map(PathBuf::from)
        .or_else(|| Some(mcc_root()?.join("haloreach").join("maps")))
}

/// The dialogue languages MCC ships for Halo 3 (`halo3\fmod\pc\<name>.fsb`).
pub const LANGUAGES: [&str; 6] = ["english", "french", "german", "italian", "mexican", "spanish"];

/// The language Halo 3's dialogue plays in: `%MCC_LANGUAGE%` (a dev
/// override), else `LANGUAGE` in the saved setup (`key::LANGUAGE`; setup
/// does not ask, so it is only there when written by hand), else
/// `english`. Anything not in [`LANGUAGES`] falls back to `english`.
pub fn language() -> &'static str {
    let wanted = std::env::var("MCC_LANGUAGE")
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

/// The dev override for the Skate 3 data, `%MCC_SKATE_ASSETS%` or
/// `%IW4L_SKATE_ASSETS%`: a folder, or [`SKATE_SKIPPED`].
fn skate_override() -> Option<OsString> {
    ["MCC_SKATE_ASSETS", "IW4L_SKATE_ASSETS"].iter().find_map(std::env::var_os)
}

/// Whether a Skate 3 choice is "without Skate 3": the override when one is
/// set (`env`), else the saved `SKATE_ASSETS` (`saved`).
fn skipped(env: Option<&OsStr>, saved: Option<&str>) -> bool {
    match env {
        Some(v) => v == SKATE_SKIPPED,
        None => saved == Some(SKATE_SKIPPED),
    }
}

/// The Skate 3 data a choice names: the override when one is set, else the
/// saved folder; never the skip.
fn chosen_skate(env: Option<&OsStr>, saved: Option<&str>) -> Option<PathBuf> {
    if skipped(env, saved) {
        return None;
    }
    env.map(PathBuf::from).or_else(|| saved.map(PathBuf::from))
}

/// Whether the player chose in setup to play without Skate 3 (no skateboard
/// or grind rails, and the Spartan's skeleton comes from MCC's Master Chief).
/// `MCC_SKATE_ASSETS=skip` does the same for a dev run, and an override
/// naming a folder wins over a saved skip.
pub fn skate_skipped() -> bool {
    skipped(skate_override().as_deref(), settings().get(key::SKATE_ASSETS))
}

/// The converted Skate 3 data: `%MCC_SKATE_ASSETS%` or
/// `%IW4L_SKATE_ASSETS%`, else the saved setup. None before setup has run
/// and when Skate 3 was skipped. See [`skate_ready`] for whether it is usable.
pub fn skate_assets() -> Option<PathBuf> {
    chosen_skate(skate_override().as_deref(), settings().get(key::SKATE_ASSETS))
}

/// Whether Skate 3 is set up: not skipped, its converted data there
/// ([`is_skate_assets`]), and the `rig.json` and `board.json` setup derives
/// from it beside that. The board, skating and the skater's skeleton all
/// need it; without it the Spartan's skeleton comes from MCC's Master Chief
/// and skating is off.
pub fn skate_ready() -> bool {
    skate_assets().is_some_and(|a| {
        is_skate_assets(&a) && a.join("rig.json").is_file() && a.join("board.json").is_file()
    })
}

/// Whether skating is on: Skate 3 is set up ([`skate_ready`]) and the player
/// has not turned it off (on is the default). Off, maps load no grind rails,
/// no skate session and no board, and J does nothing. `MCC_SKATING=off` (or
/// `on`) overrides the saved choice for a dev run; nothing turns it on
/// without Skate 3.
pub fn skating_enabled() -> bool {
    let off = |v: &str| v.trim().eq_ignore_ascii_case("off");
    let chosen = match std::env::var("MCC_SKATING") {
        Ok(v) => !off(&v),
        Err(_) => !settings().get(key::SKATING).is_some_and(off),
    };
    chosen && skate_ready()
}

/// Saves whether skating is on.
pub fn set_skating_enabled(on: bool) -> Result<(), String> {
    let path = settings_path();
    let mut s = Settings::read(&path);
    s.set(key::SKATING, if on { "on" } else { "off" });
    s.write(&path)
}

/// The name a player has until they choose one.
pub const DEFAULT_USERNAME: &str = "Player1";
/// Longest username kept, in characters.
pub const USERNAME_MAX: usize = 24;

/// A typed name made safe to keep: control characters and quotes dropped,
/// ends trimmed, cut to [`USERNAME_MAX`]; [`DEFAULT_USERNAME`] when nothing is left.
pub fn clean_username(typed: &str) -> String {
    let kept: String = typed.chars().filter(|c| !c.is_control() && *c != '"').collect();
    let name: String = kept.trim().chars().take(USERNAME_MAX).collect();
    let name = name.trim_end().to_owned();
    if name.is_empty() { DEFAULT_USERNAME.to_owned() } else { name }
}

/// The player's name (the kill feed): the saved one, else `Player1`.
pub fn username() -> String {
    clean_username(settings().get(key::USERNAME).unwrap_or(DEFAULT_USERNAME))
}

/// Saves the player's name at once and returns what was kept.
pub fn set_username(typed: &str) -> Result<String, String> {
    let name = clean_username(typed);
    let path = settings_path();
    let mut s = Settings::read(&path);
    s.set(key::USERNAME, &name);
    s.write(&path)?;
    Ok(name)
}

/// `KEY=value` lines, rewritten in place: keys that are set are replaced and
/// every other line is kept as the player left it.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    lines: Vec<String>,
}

impl Settings {
    pub fn read(path: &Path) -> Self {
        // An editor may have saved it with a byte order mark.
        let lines = std::fs::read_to_string(path)
            .map(|text| text.trim_start_matches('\u{feff}').lines().map(str::to_owned).collect())
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
    fn usernames_are_cleaned() {
        assert_eq!(clean_username(""), "Player1");
        assert_eq!(clean_username("   "), "Player1");
        assert_eq!(clean_username("  Player \"One\"\t"), "Player One");
        assert_eq!(clean_username(&"x".repeat(40)).chars().count(), USERNAME_MAX);
        let mut s = Settings::default();
        assert_eq!(s.get(key::USERNAME), None);
        s.set(key::USERNAME, "Player One");
        assert_eq!(clean_username(s.get(key::USERNAME).unwrap()), "Player One");
    }

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

    #[test]
    fn skate_skip_comes_from_the_override_else_the_saved_choice() {
        let os = |s: &'static str| Some(OsStr::new(s));
        let folder = r"E:\Games\skate-data\assets";
        // The override wins, whatever is saved.
        assert!(skipped(os("skip"), None));
        assert!(skipped(os("skip"), Some(folder)));
        assert!(!skipped(os(folder), None));
        assert!(!skipped(os(folder), Some(SKATE_SKIPPED)));
        // Without one, the saved choice.
        assert!(skipped(None, Some(SKATE_SKIPPED)));
        assert!(!skipped(None, Some(folder)));
        // Nothing set is not a skip: setup has not asked yet.
        assert!(!skipped(None, None));
    }

    #[test]
    fn skate_assets_are_never_the_skip() {
        let os = |s: &'static str| Some(OsStr::new(s));
        let (mine, saved) = (r"D:\dev\assets", r"E:\Games\skate-data\assets");
        assert_eq!(chosen_skate(os("skip"), Some(saved)), None);
        assert_eq!(chosen_skate(None, Some(SKATE_SKIPPED)), None);
        assert_eq!(chosen_skate(os(mine), Some(SKATE_SKIPPED)), Some(PathBuf::from(mine)));
        assert_eq!(chosen_skate(os(mine), Some(saved)), Some(PathBuf::from(mine)));
        assert_eq!(chosen_skate(None, Some(saved)), Some(PathBuf::from(saved)));
        assert_eq!(chosen_skate(None, None), None);
    }

    #[test]
    fn old_settings_move_to_the_new_name() {
        let base = std::env::temp_dir().join(format!("mcc-settings-move-{}", std::process::id()));
        let (old, new) = (base.join("Halo3Rewrite"), base.join("MccRustRewrite"));
        std::fs::create_dir_all(old.join("skate-data")).unwrap();
        let assets = old.join("skate-data").join("assets");
        let saved = format!("MCC=\"E:\\Games\\MCC\"\nSKATE_ASSETS=\"{}\"\n", assets.display());
        std::fs::write(old.join("setup.env"), saved).unwrap();
        move_old_settings(&old, &new);
        assert!(!old.exists());
        let s = Settings::read(&new.join("setup.env"));
        assert_eq!(s.get(key::MCC), Some(r"E:\Games\MCC"));
        let moved = new.join("skate-data").join("assets");
        assert_eq!(s.get(key::SKATE_ASSETS), moved.to_str());
        // Once the new folder exists the old one is left alone.
        std::fs::create_dir_all(&old).unwrap();
        move_old_settings(&old, &new);
        assert!(old.exists());
        std::fs::remove_dir_all(&base).unwrap();
    }
}
