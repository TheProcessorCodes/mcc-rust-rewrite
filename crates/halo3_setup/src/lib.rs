//! First-launch setup: the games this one is played with, each the
//! player's own copy. The player chooses each one's folder in a Windows
//! folder picker; nothing is searched for. Asked once:
//!
//! - Halo: The Master Chief Collection (its Halo 3 build is checked);
//! - MW2 Skate 3 (the 2010 Rust Rewrite Mashup), for IW4L's Skate 3
//!   converter in it, `skate\iw4l-skate-convert.exe` (this game does not
//!   ship it);
//! - Skate 3 for Xbox 360: a folder holding the extracted game
//!   (`default.xex` with `data` beside it) or the disc image (`.iso`), from
//!   which the few files skating needs are converted once with that
//!   converter;
//! - MX vs ATV Reflex, optional: the dirt bike's model and engine sound.
//!   Without it the bike is our own model and has no engine sound.
//!
//! MCC and Skate 3 are required: cancelling either stops setup, and the game
//! does not start. Going without Reflex is remembered like a folder. The answers go in `setup.env` (`game_paths::settings_path`)
//! and every later launch goes straight in; `halo3 --setup` asks again.
//!
//! The model is IW4L's `launcher/src/first_run.rs`. Nothing is written into
//! any game's folder: the converted data goes under
//! `game_paths::settings_dir`.

mod board;
pub mod xiso;

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use game_paths::{Settings, key};
use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};

const TITLE: &str = "Halo 3 Rust Rewrite";
/// The converter's file name, as IW4L ships it.
const CONVERTER: &str = "iw4l-skate-convert.exe";

/// The Skate 3 files the converter reads, as paths on the disc. `data/anim`
/// is copied whole. Checked against a full conversion of the extracted
/// game: these alone give the same output byte for byte.
const SKATE_DISC_FILES: [&str; 5] = [
    "default.xex",
    "data/big/miscload.big",
    "data/big/miscboot.big",
    "data/big/db.big",
    "data/content/createacharacter.big",
];
const SKATE_DISC_DIRS: [&str; 1] = ["data/anim"];

/// What a headless run says when a game has not been set up yet.
const RUN_SETUP: &str = "Run halo3 once with a window to set it up.";

/// Makes sure MCC and Skate 3 are set up, and Reflex is either set up or
/// declined; an `Err` means the game must not start. With `interactive` false (headless screenshots) it
/// never shows a dialog or writes the setup: it uses only what is saved (or
/// the dev overrides). `redo` forgets the saved answers and asks again.
pub fn prepare(interactive: bool, redo: bool) -> Result<(), String> {
    let path = game_paths::settings_path();
    let mut settings = Settings::read(&path);
    if redo {
        for k in [key::MCC, key::SKATE_ASSETS, key::REFLEX] {
            settings.remove(k);
        }
        settings.write(&path)?;
    }

    let mcc_ready = std::env::var_os("HALO3_MCC").is_some()
        || std::env::var_os("HALO3_MAPS").is_some()
        || game_paths::saved_mcc().is_some();
    let skate_ready = game_paths::skate_assets().is_some_and(|a| game_paths::is_skate_assets(&a));
    let reflex_ready = game_paths::reflex_root().is_some() || game_paths::reflex_skipped();
    if !(mcc_ready && skate_ready) && !interactive {
        let missing: Vec<&str> = [(mcc_ready, "Halo MCC"), (skate_ready, "Skate 3")]
            .into_iter()
            .filter(|(ready, _)| !ready)
            .map(|(_, name)| name)
            .collect();
        return Err(format!("{} not set up yet. {RUN_SETUP}", missing.join(", ")));
    }
    if !(mcc_ready && skate_ready && reflex_ready) && interactive {
        inform(
            "Welcome! This game is played with your own copies of these games:\n\n\
             1. Halo: The Master Chief Collection (PC)\n\
             2. MW2 Skate 3: its folder has the Skate 3 converter (skate\\iw4l-skate-convert.exe)\n\
             3. Skate 3 (Xbox 360): the extracted game or its disc image (.iso)\n\
             4. MX vs ATV Reflex (PC), optional: the dirt bike's model and engine sound\n\n\
             You will be asked for each folder in turn. This is asked once.",
        );
    }

    prepare_mcc(&mut settings, &path, interactive)?;
    let assets = prepare_skate(&mut settings, &path, interactive)?;
    board::ensure(&assets).map_err(|e| format!("Could not prepare the skateboard: {e}"))?;
    prepare_reflex(&mut settings, &path, interactive)
}

/// MX vs ATV Reflex, optional: the dirt bike's Dakumoto 450F and its engine
/// recordings. Declining is saved (`game_paths::REFLEX_SKIPPED`) so it is
/// not asked again; without it the bike is our own.
fn prepare_reflex(settings: &mut Settings, path: &Path, interactive: bool) -> Result<(), String> {
    if game_paths::reflex_skipped() {
        return Ok(());
    }
    if std::env::var_os("REFLEX_GAME").is_some() || game_paths::saved_reflex().is_some() {
        return game_paths::reflex_root()
            .map(|_| ())
            .ok_or_else(|| "REFLEX_GAME is not an MX vs ATV Reflex folder.".to_owned());
    }
    // Headless runs never ask or write; they ride our own bike.
    if !interactive {
        return Ok(());
    }
    let have = MessageDialog::new()
        .set_title(TITLE)
        .set_description(
            "MX vs ATV Reflex (optional)\n\n\
             With your copy of Reflex the dirt bike is its Dakumoto 450F with its engine recordings. \
             Without it the bike is our own model and has no engine sound.\n\n\
             Do you have MX vs ATV Reflex and want to choose its folder?",
        )
        .set_buttons(MessageButtons::YesNo)
        .show();
    let root = match have {
        MessageDialogResult::Yes => locate_reflex(),
        _ => None,
    };
    match root {
        Some(root) => settings.set_path(key::REFLEX, &root),
        None => settings.set(key::REFLEX, game_paths::REFLEX_SKIPPED),
    }
    settings.write(path)
}

fn locate_reflex() -> Option<PathBuf> {
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Choose your MX vs ATV Reflex folder")
            .pick_folder()?;
        if game_paths::is_reflex(&picked) {
            return Some(picked);
        }
        inform(&format!(
            "{}\n\ndoesn't look like MX vs ATV Reflex. Select the folder that contains the Database folder \
             (with MXs.dx9.package in it).",
            picked.display()
        ));
    }
}

/// Shows an error and exits.
pub fn fail(message: &str) -> ! {
    eprintln!("setup: {message}");
    MessageDialog::new()
        .set_title(TITLE)
        .set_level(MessageLevel::Error)
        .set_description(message)
        .show();
    std::process::exit(2);
}

fn prepare_mcc(settings: &mut Settings, path: &Path, interactive: bool) -> Result<(), String> {
    // Dev overrides skip the question.
    if std::env::var_os("HALO3_MCC").is_some() || std::env::var_os("HALO3_MAPS").is_some() {
        return Ok(());
    }
    if let Some(saved) = game_paths::saved_mcc() {
        return check_mcc(&saved);
    }
    if !interactive {
        return Err(format!("Halo: The Master Chief Collection is not set up yet. {RUN_SETUP}"));
    }
    let mcc = locate_mcc()?;
    settings.set_path(key::MCC, &mcc);
    settings.write(path)
}

/// Whether the install's Halo 3 is a build the readers understand.
fn check_mcc(root: &Path) -> Result<(), String> {
    let maps = root.join("halo3").join("maps");
    let mut last = None;
    for name in ["shared.map", "riverworld.map"] {
        match blam_cache::CacheFile::open(maps.join(name)) {
            Ok(_) => return Ok(()),
            Err(e @ (blam_cache::Error::UnsupportedBuild(_) | blam_cache::Error::UnsupportedVersion(_))) => {
                return Err(format!(
                    "This copy of Halo: The Master Chief Collection has a Halo 3 this game cannot read yet \
                     ({e}).\n\nIt was written for the Steam release with Halo 3 built {}.",
                    blam_cache::KNOWN_BUILDS.join(", ")
                ));
            }
            Err(e) => last = Some(e.to_string()),
        }
    }
    Err(format!(
        "Halo 3's maps in {} could not be read: {}",
        maps.display(),
        last.unwrap_or_default()
    ))
}

fn locate_mcc() -> Result<PathBuf, String> {
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Choose your Halo MCC folder")
            .pick_folder()
            .ok_or("Setup cancelled: Halo: The Master Chief Collection is needed to play.")?;
        let Some(root) = game_paths::mcc_from_pick(&picked) else {
            inform(&format!(
                "{}\n\ndoesn't look like the Master Chief Collection with Halo 3 installed. \
                 Select the folder that contains halo3\\maps.",
                picked.display()
            ));
            continue;
        };
        match check_mcc(&root) {
            Ok(()) => return Ok(root),
            Err(e) => inform(&e),
        }
    }
}

/// The converted Skate 3 data: saved (or a dev override), else converted
/// now from the player's Skate 3.
fn prepare_skate(settings: &mut Settings, path: &Path, interactive: bool) -> Result<PathBuf, String> {
    if let Some(assets) = game_paths::skate_assets().filter(|a| game_paths::is_skate_assets(a)) {
        return Ok(assets);
    }
    if !interactive {
        return Err(format!("Skate 3 is not set up yet. {RUN_SETUP}"));
    }
    let assets = convert_skate(settings)?;
    settings.set_path(key::SKATE_ASSETS, &assets);
    settings.write(path)?;
    Ok(assets)
}

const SKATE_CANCELLED: &str = "Setup cancelled: Skate 3 is needed to play.";

/// What the player's Skate 3 folder holds.
#[derive(Debug, PartialEq)]
enum SkateSource {
    /// The extracted game's `default.xex` (its `data` folder beside it).
    Xex(PathBuf),
    /// A disc image.
    Iso(PathBuf),
}

/// The Skate 3 in a chosen folder: `default.xex` in it or in a folder just
/// inside it (extractions often nest one level), else the one `.iso` in it.
fn skate_source(folder: &Path) -> Result<SkateSource, String> {
    let xex = folder.join("default.xex");
    if xex.is_file() {
        return Ok(SkateSource::Xex(xex));
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(folder)
        .map_err(|e| format!("cannot read {}: {e}", folder.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .collect();
    if let Some(xex) = entries.iter().map(|d| d.join("default.xex")).find(|x| x.is_file()) {
        return Ok(SkateSource::Xex(xex));
    }
    let isos: Vec<&PathBuf> = entries
        .iter()
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("iso")))
        .collect();
    match isos.as_slice() {
        [iso] => Ok(SkateSource::Iso((*iso).clone())),
        [] => Err(format!(
            "{}\n\nhas neither Skate 3's default.xex nor a disc image (.iso). Select the folder of the \
             extracted game, or the folder the .iso is in.",
            folder.display()
        )),
        _ => Err(format!(
            "{}\n\nhas more than one .iso. Put Skate 3's disc image in a folder of its own and select that.",
            folder.display()
        )),
    }
}

fn convert_skate(settings: &mut Settings) -> Result<PathBuf, String> {
    let converter = find_converter(settings)?.ok_or(SKATE_CANCELLED)?;
    let dir = game_paths::settings_dir();
    let out = dir.join("skate-data");
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Choose your Skate 3 folder")
            .pick_folder()
            .ok_or(SKATE_CANCELLED)?;
        let result = skate_source(&picked).and_then(|source| match source {
            SkateSource::Iso(iso) => {
                let staging = dir.join("skate-disc");
                let result = unpack_disc(&iso, &staging).and_then(|xex| run_converter(&converter, &xex, &out));
                // The copied game files are only needed for the conversion.
                std::fs::remove_dir_all(&staging).ok();
                result
            }
            SkateSource::Xex(xex) => run_converter(&converter, &xex, &out),
        });
        match result {
            Ok(()) => break,
            Err(e) => inform(&format!("{e}\n\nSelect Skate 3 again.")),
        }
    }
    let assets = out.join("assets");
    if game_paths::is_skate_assets(&assets) {
        Ok(assets)
    } else {
        Err(format!("The Skate 3 conversion finished but {} is incomplete.", assets.display()))
    }
}

/// The converter: `%HALO3_SKATE_CONVERTER%`, the saved one, or
/// `skate\iw4l-skate-convert.exe` in the MW2 Skate 3 folder the player
/// chooses (saved for next time). None when the player cancels.
fn find_converter(settings: &mut Settings) -> Result<Option<PathBuf>, String> {
    let known = std::env::var_os("HALO3_SKATE_CONVERTER")
        .map(PathBuf::from)
        .into_iter()
        .chain(settings.get(key::SKATE_CONVERTER).map(PathBuf::from))
        .find(|p| p.is_file());
    if known.is_some() {
        return Ok(known);
    }
    loop {
        let Some(picked) = rfd::FileDialog::new().set_title("Choose your MW2 Skate 3 folder").pick_folder() else {
            return Ok(None);
        };
        if let Some(converter) = converter_in(&picked) {
            settings.set_path(key::SKATE_CONVERTER, &converter);
            return Ok(Some(converter));
        }
        inform(&format!(
            "{}\n\nhas no skate\\{CONVERTER}. Select your MW2 Skate 3 folder (the one with iw4l.exe and \
             the skate folder).",
            picked.display()
        ));
    }
}

/// The converter in a chosen MW2 Skate 3 folder, or in its `skate` folder
/// when that was chosen.
fn converter_in(folder: &Path) -> Option<PathBuf> {
    [folder.join("skate").join(CONVERTER), folder.join(CONVERTER)]
        .into_iter()
        .find(|p| p.is_file())
}

/// Copies the files the converter reads off a Skate 3 disc image into
/// `staging`, returning the copied `default.xex`.
fn unpack_disc(iso: &Path, staging: &Path) -> Result<PathBuf, String> {
    println!("Reading Skate 3 from {}", iso.display());
    let mut disc = xiso::Xiso::open(iso)?;
    let not_skate = |e: String| format!("{} doesn't look like Skate 3 for Xbox 360: {e}", iso.display());
    let mut files = Vec::new();
    for path in SKATE_DISC_FILES {
        files.push((path.to_owned(), disc.find(path).map_err(not_skate)?));
    }
    for dir in SKATE_DISC_DIRS {
        let entry = disc.find(dir).map_err(not_skate)?;
        for e in disc.list(&entry)?.into_iter().filter(|e| !e.directory) {
            files.push((format!("{dir}/{}", e.name), e));
        }
    }
    std::fs::remove_dir_all(staging).ok();
    let total: u64 = files.iter().map(|(_, e)| u64::from(e.size)).sum();
    let mut done = 0u64;
    let mut shown = 0u64;
    for (path, entry) in &files {
        disc.extract(entry, &staging.join(path), &mut |n| {
            done += n;
            if done - shown >= 64 << 20 || done == total {
                shown = done;
                println!("  copied {} of {} MB", done >> 20, total >> 20);
            }
        })?;
    }
    Ok(staging.join("default.xex"))
}

/// Runs the converter with its progress echoed to this console, returning
/// the converter's own error line when it fails.
fn run_converter(converter: &Path, xex: &Path, out: &Path) -> Result<(), String> {
    println!("Converting Skate 3 data from {}", xex.display());
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    }
    let mut child = Command::new(converter)
        .arg("--xex")
        .arg(xex)
        .arg("--out")
        .arg(out)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Could not start the Skate 3 converter: {e}"))?;
    let mut failure = None;
    if let Some(stdout) = child.stdout.take() {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            println!("  {line}");
            if let Some(message) = line.strip_prefix("ERROR: ") {
                failure = Some(message.to_owned());
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        return Ok(());
    }
    Err(failure.unwrap_or_else(|| format!("The Skate 3 converter stopped ({status}).")))
}

fn inform(message: &str) {
    MessageDialog::new().set_title(TITLE).set_description(message).show();
}

#[cfg(test)]
mod tests {
    use std::io::{Seek, SeekFrom, Write};
    use std::path::Path;

    /// Writes a trimmed XISO holding `files` (disc path, source file), each
    /// directory's entries chained down the right-hand side of its tree.
    fn write_xiso(out: &Path, files: &[(&str, std::path::PathBuf)]) {
        #[derive(Default)]
        struct Dir {
            dirs: std::collections::BTreeMap<String, Dir>,
            files: Vec<(String, std::path::PathBuf)>,
        }
        let mut root = Dir::default();
        for (path, src) in files {
            let parts: Vec<&str> = path.split('/').collect();
            let mut d = &mut root;
            for p in &parts[..parts.len() - 1] {
                d = d.dirs.entry(p.to_string()).or_default();
            }
            d.files.push((parts[parts.len() - 1].to_owned(), src.clone()));
        }
        let mut f = std::fs::File::create(out).unwrap();
        let mut next = 34u32; // sectors 0..33 hold the descriptor
        // Returns (sector, size) of the directory table written.
        fn emit(d: &Dir, f: &mut std::fs::File, next: &mut u32) -> (u32, u32) {
            let sectors = |n: u64| n.div_ceil(2048) as u32;
            let mut entries: Vec<(String, u32, u32, u8)> = Vec::new();
            for (name, sub) in &d.dirs {
                let (s, n) = emit(sub, f, next);
                entries.push((name.clone(), s, n, 0x10));
            }
            for (name, src) in &d.files {
                let len = std::fs::metadata(src).unwrap().len();
                let at = *next;
                *next += sectors(len).max(1);
                f.seek(SeekFrom::Start(u64::from(at) * 2048)).unwrap();
                std::io::copy(&mut std::fs::File::open(src).unwrap(), f).unwrap();
                entries.push((name.clone(), at, len as u32, 0x80));
            }
            let mut table = Vec::new();
            for (i, (name, sector, size, attr)) in entries.iter().enumerate() {
                let len = (14 + name.len()).next_multiple_of(4);
                let right = if i + 1 < entries.len() { ((table.len() + len) / 4) as u16 } else { 0 };
                table.extend(0u16.to_le_bytes());
                table.extend(right.to_le_bytes());
                table.extend(sector.to_le_bytes());
                table.extend(size.to_le_bytes());
                table.push(*attr);
                table.push(name.len() as u8);
                table.extend(name.as_bytes());
                table.resize(table.len().next_multiple_of(4), 0xFF);
            }
            let at = *next;
            *next += sectors(table.len() as u64).max(1);
            f.seek(SeekFrom::Start(u64::from(at) * 2048)).unwrap();
            f.write_all(&table).unwrap();
            (at, table.len() as u32)
        }
        let (sector, size) = emit(&root, &mut f, &mut next);
        let mut head = b"MICROSOFT*XBOX*MEDIA".to_vec();
        head.extend(sector.to_le_bytes());
        head.extend(size.to_le_bytes());
        f.seek(SeekFrom::Start(32 * 2048)).unwrap();
        f.write_all(&head).unwrap();
    }

    /// Skate 3 from a disc image, end to end, when
    /// `HALO3_TEST_SKATE_GAME` (the extracted game, read only),
    /// `HALO3_TEST_SKATE_CONVERTER` and `HALO3_TEST_SKATE_ASSETS` (an IW4L
    /// conversion of the same game) are set: the files the converter reads
    /// are packed into an image under `HALO3_TEST_SCRATCH`, unpacked the way
    /// setup does, converted, and compared file by file with IW4L's.
    #[test]
    fn skate_from_a_disc_image_matches_iw4l() {
        let var = |k: &str| std::env::var_os(k).map(std::path::PathBuf::from);
        let (Some(game), Some(converter), Some(iw4l), Some(scratch)) = (
            var("HALO3_TEST_SKATE_GAME"),
            var("HALO3_TEST_SKATE_CONVERTER"),
            var("HALO3_TEST_SKATE_ASSETS"),
            var("HALO3_TEST_SCRATCH"),
        ) else {
            return;
        };
        std::fs::create_dir_all(&scratch).unwrap();
        let mut files: Vec<(&str, std::path::PathBuf)> =
            super::SKATE_DISC_FILES.iter().map(|p| (*p, game.join(p))).collect();
        let anim: Vec<String> = std::fs::read_dir(game.join("data/anim"))
            .unwrap()
            .map(|e| format!("data/anim/{}", e.unwrap().file_name().to_string_lossy()))
            .collect();
        files.extend(anim.iter().map(|p| (p.as_str(), game.join(p))));
        let iso = scratch.join("skate3-test.iso");
        write_xiso(&iso, &files);
        let staging = scratch.join("skate-disc");
        let xex = super::unpack_disc(&iso, &staging).unwrap();
        let out = scratch.join("skate-data");
        super::run_converter(&converter, &xex, &out).unwrap();
        std::fs::remove_dir_all(&staging).ok();
        std::fs::remove_file(&iso).ok();
        let assets = out.join("assets");
        super::board::ensure(&assets).unwrap();
        let mut stack = vec![iw4l.clone()];
        let mut checked = 0;
        while let Some(dir) = stack.pop() {
            for e in std::fs::read_dir(&dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                let rel = p.strip_prefix(&iw4l).unwrap();
                let ours = std::fs::read(assets.join(rel)).unwrap_or_else(|_| panic!("missing {}", rel.display()));
                assert!(ours == std::fs::read(&p).unwrap(), "{} differs", rel.display());
                checked += 1;
            }
        }
        eprintln!("{checked} files match IW4L's conversion");
    }

    #[test]
    fn skate_folder_holds_xex_nested_xex_or_one_iso() {
        use super::{SkateSource, skate_source};
        let root = std::env::temp_dir().join(format!("halo3-skate-source-{}", std::process::id()));
        let make = |rel: &str| {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
            p
        };
        let flat = make("flat/default.xex");
        assert_eq!(skate_source(&root.join("flat")).unwrap(), SkateSource::Xex(flat));
        let nested = make("nested/Skate 3/default.xex");
        assert_eq!(skate_source(&root.join("nested")).unwrap(), SkateSource::Xex(nested));
        let iso = make("disc/skate3.ISO");
        assert_eq!(skate_source(&root.join("disc")).unwrap(), SkateSource::Iso(iso));
        make("two/a.iso");
        make("two/b.iso");
        assert!(skate_source(&root.join("two")).is_err());
        std::fs::create_dir_all(root.join("empty")).unwrap();
        assert!(skate_source(&root.join("empty")).is_err());
        std::fs::remove_dir_all(&root).ok();
    }

    /// `board.json` and `rig.json` from the user's converted data match what
    /// IW4L wrote, when `HALO3_TEST_SKATE_ASSETS` names an IW4L asset folder
    /// that has both. Written to a temp copy; the folder is not touched.
    #[test]
    fn board_matches_iw4l() {
        let Some(src) = std::env::var_os("HALO3_TEST_SKATE_ASSETS").map(std::path::PathBuf::from) else {
            return;
        };
        let tmp = std::env::temp_dir().join(format!("halo3-board-test-{}", std::process::id()));
        std::fs::create_dir_all(tmp.join("private")).unwrap();
        std::fs::copy(src.join("private/skater.glb"), tmp.join("private/skater.glb")).unwrap();
        super::board::export(&tmp).unwrap();
        for f in ["board.json", "rig.json"] {
            assert!(std::fs::read(tmp.join(f)).unwrap() == std::fs::read(src.join(f)).unwrap(), "{f} differs");
        }
        std::fs::remove_dir_all(&tmp).ok();
    }
}
