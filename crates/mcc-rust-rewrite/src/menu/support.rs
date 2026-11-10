//! The settings screen's Support page (`menu.rs`): open the data folder,
//! copy a summary for a bug report to the clipboard, and what the converted
//! game data takes up. Plain programs every Windows has do the work
//! (`explorer.exe`, `clip.exe`, `cmd /c ver`); on other systems their usual
//! counterparts are tried, and a missing one is a message, never a crash.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A command without a console window of its own.
fn quiet(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Opens `dir` (made first, if it is not there yet) in the file manager.
pub(super) fn open_folder(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    // Explorer's exit code says nothing useful: it only has to start.
    let mut command = quiet(program);
    command.arg(dir);
    command.spawn().map(|_| ()).map_err(|e| format!("cannot start {program}: {e}"))
}

/// Puts `text` on the clipboard.
pub(super) fn copy(text: &str) -> Result<(), String> {
    // clip.exe reads UTF-16 with a byte order mark as it is; anything else
    // in the console's code page, which mangles non-English paths.
    let windows: [(&str, &[&str]); 1] = [("clip.exe", &[])];
    let others: [(&str, &[&str]); 3] =
        [("pbcopy", &[]), ("wl-copy", &[]), ("xclip", &["-selection", "clipboard"])];
    let tools: &[(&str, &[&str])] = if cfg!(windows) { &windows } else { &others };
    let bytes = if cfg!(windows) {
        let mut b = vec![0xFF, 0xFE];
        b.extend(text.encode_utf16().flat_map(u16::to_le_bytes));
        b
    } else {
        text.as_bytes().to_vec()
    };
    let mut last = String::from("no clipboard program found");
    for (program, args) in tools {
        let child = quiet(program).args(*args).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                last = format!("cannot start {program}: {e}");
                continue;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(&bytes);
        }
        match child.wait() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => last = format!("{program} failed ({status})"),
            Err(e) => last = format!("{program} failed: {e}"),
        }
    }
    Err(last)
}

/// The system's name and version, as its own command says it.
fn os_version() -> String {
    let (program, args): (&str, &[&str]) = if cfg!(windows) { ("cmd", &["/c", "ver"]) } else { ("uname", &["-srm"]) };
    let said = quiet(program)
        .args(args)
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());
    let arch = std::env::consts::ARCH;
    match said {
        Some(s) => format!("{s} ({arch})"),
        None => format!("{} ({arch})", std::env::consts::OS),
    }
}

/// A path as a bug report shows it: under the player's own folders it
/// starts `%LOCALAPPDATA%` or `%USERPROFILE%` instead of their user name.
fn shown(path: &Path) -> String {
    let text = path.display().to_string();
    for var in ["LOCALAPPDATA", "USERPROFILE", "HOME"] {
        if let Some(base) = std::env::var_os(var).map(PathBuf::from)
            && let Ok(rest) = path.strip_prefix(&base)
        {
            return Path::new(&format!("%{var}%")).join(rest).display().to_string();
        }
    }
    text
}

/// The summary Copy diagnostics puts on the clipboard: the version, the
/// system, which games are set up (their folders as [`shown`]), whether
/// Halo 3 reads, the main settings, and where the log is. Not the player's
/// username.
pub(super) fn diagnostics(look: &mcc_core::view::LookSettings) -> String {
    let mut lines = vec![
        format!("MCC Rust Rewrite {}", env!("CARGO_PKG_VERSION")),
        format!("System: {}", os_version()),
    ];
    let mcc = match game_paths::mcc_root() {
        None => "not set up".to_string(),
        Some(root) => {
            let check = match mcc_setup::mcc_ready() {
                Ok(()) => "Halo 3 reads".to_string(),
                Err(mcc_setup::Missing::NotSet) => "not set up".to_string(),
                Err(mcc_setup::Missing::Unreadable(e)) => e.lines().next().unwrap_or_default().to_string(),
            };
            format!("{} ({check})", shown(&root))
        }
    };
    lines.push(format!("Halo MCC: {mcc}"));
    let skate = if game_paths::skate_ready() {
        let from = game_paths::settings().get(game_paths::key::SKATE_SOURCE).map(|s| shown(Path::new(s)));
        let skating = if game_paths::skating_enabled() { "skating on" } else { "skating off" };
        format!("set up{}, {skating}", from.map_or_else(String::new, |f| format!(" from {f}")))
    } else if game_paths::skate_skipped() {
        "not used".to_string()
    } else {
        "not set up".to_string()
    };
    lines.push(format!("Skate 3: {skate}"));
    let reflex = match game_paths::reflex_chosen() {
        _ if game_paths::reflex_skipped() => "not used".to_string(),
        Some(p) if game_paths::reflex_imported() => format!("imported from {}", shown(&p)),
        Some(p) if game_paths::reflex_ready() => format!("{} (not imported yet)", shown(&p)),
        Some(p) => format!("{} (not found)", shown(&p)),
        None => "not set up".to_string(),
    };
    lines.push(format!("MX vs ATV Reflex: {reflex}"));
    if game_paths::reflex_ready() {
        let physics = if mcc_bike::tune::load().model >= 1.5 { "Reflex" } else { "Arcade" };
        lines.push(format!("Bike physics: {physics}"));
    }
    let display = mcc_core::view::Display::load();
    lines.push(format!(
        "Display: {}, vsync {}",
        if display.windowed { "windowed" } else { "borderless fullscreen" },
        if display.vsync { "on" } else { "off" }
    ));
    lines.push(format!(
        "Look: mouse {} %, stick {} %, invert {}, field of view {} %",
        look.mouse,
        look.stick,
        if look.invert { "on" } else { "off" },
        look.fov
    ));
    lines.push(format!("Dialogue: {}", game_paths::language()));
    lines.push(format!("Data folder: {}", shown(&game_paths::settings_dir())));
    if let Some(log) = crate::logfile::current() {
        lines.push(format!("Log: {}", shown(log)));
    }
    // Only the names: a dev override changes where things are looked for.
    let overrides: Vec<&str> = [
        "MCC_ROOT",
        "HALO3_MAPS",
        "HALO2_MAPS",
        "HALO1_MAPS",
        "HALOREACH_MAPS",
        "MCC_SKATE_ASSETS",
        "IW4L_SKATE_ASSETS",
        "REFLEX_GAME",
        "MCC_LANGUAGE",
        "MCC_SETUP_DIR",
    ]
    .into_iter()
    .filter(|v| std::env::var_os(v).is_some())
    .collect();
    if !overrides.is_empty() {
        lines.push(format!("Overrides set: {}", overrides.join(", ")));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    text
}

/// The folders under the data folder that hold converted game data: Skate
/// 3's (and the leftovers of a conversion) and Reflex's import.
fn converted_dirs() -> [(&'static str, PathBuf); 4] {
    let dir = game_paths::settings_dir();
    [
        ("Skate 3", dir.join("skate-data")),
        ("Skate 3", dir.join("skate-data-new")),
        ("Skate 3", dir.join("skate-disc")),
        ("Reflex", game_paths::reflex_import_dir()),
    ]
}

/// Bytes under `dir`, 0 when it is not there.
fn size_of(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => size_of(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

/// What converted data there is, for the row: `Skate 3: 189 MB, Reflex:
/// 10 MB`, or that there is none.
pub(super) fn converted() -> String {
    let mut sizes: Vec<(&str, u64)> = Vec::new();
    for (game, dir) in converted_dirs() {
        let bytes = size_of(&dir);
        match sizes.iter_mut().find(|(g, _)| *g == game) {
            Some((_, b)) => *b += bytes,
            None if bytes > 0 => sizes.push((game, bytes)),
            None => {}
        }
    }
    sizes.retain(|(_, b)| *b > 0);
    if sizes.is_empty() {
        return "None in the data folder".to_string();
    }
    let parts: Vec<String> = sizes.iter().map(|(g, b)| format!("{g}: {}", megabytes(*b))).collect();
    format!("{} (Skate 3 and Reflex as converted at setup)", parts.join(", "))
}

fn megabytes(bytes: u64) -> String {
    format!("{:.0} MB", (bytes as f64 / 1_048_576.0).max(1.0))
}

/// Deletes the converted Skate 3 and Reflex data. Each game whose data is
/// deleted is then played without (as if skipped at setup), until it is
/// set up again in Settings. Says what was deleted.
pub(super) fn delete_converted() -> Result<String, String> {
    let dirs = converted_dirs();
    let had = |game: &str| dirs.iter().any(|(g, d)| *g == game && d.exists());
    let (skate, reflex) = (had("Skate 3"), had("Reflex"));
    if skate {
        mcc_setup::skip_skate()?;
    }
    if reflex {
        mcc_setup::skip_reflex()?;
    }
    let left: Vec<&str> = dirs.iter().filter(|(_, d)| d.exists()).map(|(g, _)| *g).collect();
    if !left.is_empty() {
        return Err(format!("Some of the {} data could not be deleted (is it open somewhere?).", left.join(" and ")));
    }
    Ok(match (skate, reflex) {
        (true, true) => "Deleted the converted Skate 3 and Reflex data. Set them up again here on the Games page.",
        (true, false) => "Deleted the converted Skate 3 data. Set it up again on the Games page.",
        (false, true) => "Deleted the imported Reflex data. Set it up again on the Games page.",
        (false, false) => "There was no converted data to delete.",
    }
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_whole_megabytes() {
        assert_eq!(megabytes(0), "1 MB");
        assert_eq!(megabytes(189 * 1_048_576 + 1000), "189 MB");
    }

    #[test]
    fn diagnostics_leave_out_the_username() {
        let text = diagnostics(&mcc_core::view::LookSettings::default());
        assert!(text.starts_with("MCC Rust Rewrite "));
        assert!(text.contains("Look: mouse 100 %"));
        let name = game_paths::username();
        if name != game_paths::DEFAULT_USERNAME {
            assert!(!text.contains(&format!(": {name}\n")));
        }
    }
}
