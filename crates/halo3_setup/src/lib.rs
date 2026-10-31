//! First-launch setup: find the player's Halo MCC install, offer to take
//! their Skate 3 (an Xbox 360 disc image or an extracted `default.xex`),
//! convert the few Skate 3 files skating needs with the bundled converter,
//! note MX vs ATV Reflex if the player has it, and save it all in
//! `setup.env` (`game_paths::settings_path`). Every later launch goes
//! straight in; `halo3 --setup` asks again.
//!
//! The model is IW4L's `launcher/src/first_run.rs`. MCC is required; Skate
//! 3 is optional (without it skating is off), and so is Reflex (without it
//! the dirt bike is a stand-in). Nothing is written into any game's folder:
//! the converted data goes under `game_paths::settings_dir`.

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

/// Makes sure MCC (required) and Skate 3 (optional) are set up. With
/// `interactive` false (headless screenshots) it never shows a dialog: it
/// uses what is saved or found, and fails if MCC cannot be found. `redo`
/// forgets the saved answers and asks again.
pub fn prepare(interactive: bool, redo: bool) -> Result<(), String> {
    let path = game_paths::settings_path();
    let mut settings = Settings::read(&path);
    if redo {
        for k in [key::MCC, key::SKATE, key::SKATE_ASSETS, key::REFLEX] {
            settings.remove(k);
        }
        settings.write(&path)?;
    }

    prepare_mcc(&mut settings, &path, interactive)?;
    if let Some(assets) = prepare_skate(&mut settings, &path, interactive)? {
        board::ensure(&assets).map_err(|e| format!("Could not prepare the skateboard: {e}"))?;
    }
    prepare_reflex(&mut settings, &path, interactive)
}

/// MX vs ATV Reflex, optional: the dirt bike's Dakumoto 450F and its engine
/// recordings. Saved without a question when Steam has it; otherwise asked
/// once, and `off` remembers a No. Without it the bike is a stand-in.
fn prepare_reflex(settings: &mut Settings, path: &Path, interactive: bool) -> Result<(), String> {
    if std::env::var_os("REFLEX_GAME").is_some() || game_paths::saved_reflex().is_some() {
        return Ok(());
    }
    if let Some(found) = game_paths::find_reflex() {
        // Headless runs only read; Steam is searched again each time.
        if !interactive {
            return Ok(());
        }
        settings.set_path(key::REFLEX, &found);
        return settings.write(path);
    }
    if settings.get(key::REFLEX) == Some("off") || !interactive {
        return Ok(());
    }
    let answer = MessageDialog::new()
        .set_title(TITLE)
        .set_description(
            "Do you have MX vs ATV Reflex (PC)? It is optional.\n\n\
             The dirt bike uses Reflex's Dakumoto 450F and its engine sounds from your own copy. \
             Choose No to ride a stand-in bike. Run halo3 --setup to be asked again.",
        )
        .set_buttons(MessageButtons::YesNo)
        .show();
    let picked = if answer == MessageDialogResult::Yes { locate_reflex() } else { None };
    match picked {
        Some(root) => settings.set_path(key::REFLEX, &root),
        None => settings.set(key::REFLEX, "off"),
    }
    settings.write(path)
}

fn locate_reflex() -> Option<PathBuf> {
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Select your MX vs ATV Reflex folder")
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
        let found = game_paths::find_mcc().ok_or(
            "Halo: The Master Chief Collection was not found. Run halo3 once without --shot to choose it.",
        )?;
        return check_mcc(&found);
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
    if let Some(found) = game_paths::find_mcc() {
        let use_it = MessageDialog::new()
            .set_title(TITLE)
            .set_description(format!(
                "Found Halo: The Master Chief Collection here:\n\n{}\n\nUse this copy?",
                found.display()
            ))
            .set_buttons(MessageButtons::YesNo)
            .show();
        if use_it == MessageDialogResult::Yes {
            match check_mcc(&found) {
                Ok(()) => return Ok(found),
                Err(e) => inform(&e),
            }
        }
    } else {
        inform(
            "Welcome! This game plays on your own copy of Halo: The Master Chief Collection.\n\n\
             Select your MCC folder (the one containing the halo3 folder). \
             Halo 3 must be installed in it.",
        );
    }
    loop {
        let picked = rfd::FileDialog::new()
            .set_title("Select your Halo: The Master Chief Collection folder")
            .pick_folder()
            .ok_or("Setup cancelled: no Master Chief Collection folder was selected.")?;
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

/// The converted Skate 3 data, or none when the player plays without it.
fn prepare_skate(settings: &mut Settings, path: &Path, interactive: bool) -> Result<Option<PathBuf>, String> {
    if let Some(assets) = game_paths::skate_assets().filter(|a| game_paths::is_skate_assets(a)) {
        return Ok(Some(assets));
    }
    if settings.get(key::SKATE) == Some("off") || !interactive {
        return Ok(None);
    }
    match convert_skate(settings)? {
        Some(assets) => {
            settings.remove(key::SKATE);
            settings.set_path(key::SKATE_ASSETS, &assets);
            settings.write(path)?;
            Ok(Some(assets))
        }
        None => {
            settings.set(key::SKATE, "off");
            settings.write(path)?;
            Ok(None)
        }
    }
}

fn convert_skate(settings: &mut Settings) -> Result<Option<PathBuf>, String> {
    let answer = MessageDialog::new()
        .set_title(TITLE)
        .set_description(
            "Do you have Skate 3 (Xbox 360)?\n\n\
             Skating needs a few files from your own copy: either the disc image (.iso) \
             or the extracted game's default.xex with its data folder beside it.\n\n\
             Choose No to play without skating. Run halo3 --setup to be asked again.",
        )
        .set_buttons(MessageButtons::YesNo)
        .show();
    if answer != MessageDialogResult::Yes {
        return Ok(None);
    }
    let Some(converter) = find_converter(settings)? else {
        return Ok(None);
    };
    let dir = game_paths::settings_dir();
    let out = dir.join("skate-data");
    loop {
        let Some(picked) = rfd::FileDialog::new()
            .set_title("Select your Skate 3 disc image (.iso) or default.xex")
            .add_filter("Skate 3 (.iso or default.xex)", &["iso", "xex"])
            .pick_file()
        else {
            return Ok(None);
        };
        let result = if picked.extension().is_some_and(|e| e.eq_ignore_ascii_case("iso")) {
            let staging = dir.join("skate-disc");
            let result = unpack_disc(&picked, &staging)
                .and_then(|xex| run_converter(&converter, &xex, &out));
            // The copied game files are only needed for the conversion.
            std::fs::remove_dir_all(&staging).ok();
            result
        } else {
            run_converter(&converter, &picked, &out)
        };
        match result {
            Ok(()) => break,
            Err(e) => inform(&format!("{e}\n\nSelect Skate 3 again, or Cancel to play without skating.")),
        }
    }
    let assets = out.join("assets");
    if game_paths::is_skate_assets(&assets) {
        Ok(Some(assets))
    } else {
        Err(format!("The Skate 3 conversion finished but {} is incomplete.", assets.display()))
    }
}

/// The converter: beside the exe (`skate\iw4l-skate-convert.exe`, as the
/// release ships it), `%HALO3_SKATE_CONVERTER%`, the saved one, or one the
/// player points at (saved for next time).
fn find_converter(settings: &mut Settings) -> Result<Option<PathBuf>, String> {
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("skate").join(CONVERTER)));
    let known = beside
        .into_iter()
        .chain(std::env::var_os("HALO3_SKATE_CONVERTER").map(PathBuf::from))
        .chain(settings.get(key::SKATE_CONVERTER).map(PathBuf::from))
        .find(|p| p.is_file());
    if known.is_some() {
        return Ok(known);
    }
    inform(&format!(
        "The Skate 3 converter ({CONVERTER}) is not beside halo3.exe in a skate folder.\n\n\
         Select it (it comes with this game's release, and with the MW2 Skate 3 mashup)."
    ));
    let Some(picked) = rfd::FileDialog::new()
        .set_title(format!("Select {CONVERTER}"))
        .add_filter(CONVERTER, &["exe"])
        .pick_file()
    else {
        return Ok(None);
    };
    settings.set_path(key::SKATE_CONVERTER, &picked);
    Ok(Some(picked))
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
