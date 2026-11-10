//! The log and crash files, for bug reports: every run writes what it prints
//! to `logs\mcc-<time>.log` in the data folder (`game_paths::settings_dir`,
//! `%LOCALAPPDATA%\MccRustRewrite`), and a panic writes
//! `logs\crash-<time>.txt` beside it with the message, where it happened and
//! a backtrace. The last [`KEEP`] of each are kept. Times are UTC
//! (`2026-11-10_14-03-22`), so the names sort in order.
//!
//! Most of the game's messages are `eprintln!` lines, and bevy's log goes to
//! stderr too, so rather than a tracing layer (which would see only the
//! `info!` / `warn!` half) the process's stdout and stderr are teed: on
//! Windows they are pointed at pipes ([`start`]), and a thread per pipe
//! copies each line to the console as before and to the file (without
//! colour codes). Rust's `print!` and `eprintln!` look the handles up on
//! every write, so everything after [`start`] is caught, as is the output of
//! a program the game starts with the console inherited. Elsewhere only
//! the crash file is written. [`LogGuard`] puts the handles back and lets
//! the threads finish when `main` returns.
//!
//! Nothing here may stop the game: a data folder that cannot be written just
//! means no file.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// How many logs and crash files are kept.
const KEEP: usize = 5;

/// This run's log file, once [`start`] has opened it.
static CURRENT: OnceLock<PathBuf> = OnceLock::new();

/// This run's log file, if there is one.
pub fn current() -> Option<&'static Path> {
    CURRENT.get().map(PathBuf::as_path)
}

/// The folder the logs and crash files go in.
pub fn dir() -> PathBuf {
    game_paths::settings_dir().join("logs")
}

/// Puts the panic hook in and starts writing the log. Keep the guard until
/// the end of `main`.
pub fn start() -> LogGuard {
    install_panic_hook();
    let dir = dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return LogGuard::default();
    }
    prune(&dir, "mcc-", ".log", KEEP - 1);
    let path = dir.join(format!("mcc-{}.log", stamp()));
    let Ok(mut file) = File::create(&path) else {
        return LogGuard::default();
    };
    let _ = writeln!(
        file,
        "MCC Rust Rewrite {} ({} {}), started {} UTC",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        stamp()
    );
    let _ = CURRENT.set(path);
    tee::start(Arc::new(Mutex::new(file)))
}

/// Deletes the oldest `<prefix>*<suffix>` files in `dir` until `keep` are
/// left.
fn prune(dir: &Path, prefix: &str, suffix: &str, keep: usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with(prefix) && n.ends_with(suffix))
        .collect();
    names.sort();
    let extra = names.len().saturating_sub(keep);
    for name in &names[..extra] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// Writes `crash-<time>.txt` for every panic, then lets the panic go on as
/// before (the message on stderr, unwinding).
fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "(no message)".to_string());
        let place = info.location().map_or_else(|| "unknown".to_string(), |l| format!("{}:{}:{}", l.file(), l.line(), l.column()));
        let thread = std::thread::current().name().unwrap_or("unnamed").to_string();
        let report = crash_report(&message, &place, &thread, &std::backtrace::Backtrace::force_capture().to_string());
        let dir = dir();
        if std::fs::create_dir_all(&dir).is_ok() {
            prune(&dir, "crash-", ".txt", KEEP - 1);
            let path = dir.join(format!("crash-{}.txt", stamp()));
            if std::fs::write(&path, report).is_ok() {
                eprintln!("crash report: {}", path.display());
            }
        }
        previous(info);
    }));
}

/// What a crash file says.
fn crash_report(message: &str, place: &str, thread: &str, backtrace: &str) -> String {
    let log = current().map_or_else(|| "none".to_string(), |p| p.display().to_string());
    format!(
        "MCC Rust Rewrite {} ({} {}) crashed at {} UTC.\n\n\
         Panic: {message}\nAt: {place}\nThread: {thread}\nLog: {log}\n\nBacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        stamp()
    )
}

/// The time now, UTC, as `YYYY-MM-DD_HH-MM-SS`.
fn stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    stamp_of(secs)
}

fn stamp_of(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days as i64);
    format!("{y:04}-{m:02}-{d:02}_{:02}-{:02}-{:02}", rest / 3600, rest % 3600 / 60, rest % 60)
}

/// Days since 1970-01-01 to (year, month, day): Howard Hinnant's
/// `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + (m <= 2) as i64, m, d)
}

/// A line as the file gets it: colour codes (`ESC [ ... letter`) taken out.
fn plain(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Keeps the tee going; dropping it (at the end of `main`) points stdout and
/// stderr back at the console and waits briefly for the last lines.
#[derive(Default)]
pub struct LogGuard {
    #[cfg(windows)]
    tee: Option<tee::Tee>,
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(tee) = self.tee.take() {
            tee.stop();
        }
    }
}

#[cfg(windows)]
mod tee {
    //! The Windows tee: stdout and stderr pointed at pipes, read by a thread
    //! each. Only kernel32, which every Windows program links.

    use std::ffi::c_void;
    use std::fs::File;
    use std::io::{BufRead, BufReader, Write};
    use std::mem::ManuallyDrop;
    use std::os::windows::io::{FromRawHandle, RawHandle};
    use std::sync::mpsc::{Receiver, channel};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use super::{LogGuard, plain};

    type Handle = *mut c_void;
    const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    const STD_ERROR_HANDLE: u32 = -12i32 as u32;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreatePipe(read: *mut Handle, write: *mut Handle, attributes: *const c_void, size: u32) -> i32;
        fn GetStdHandle(id: u32) -> Handle;
        fn SetStdHandle(id: u32, handle: Handle) -> i32;
        fn GetConsoleMode(handle: Handle, mode: *mut u32) -> i32;
        fn WriteConsoleW(handle: Handle, text: *const u16, len: u32, written: *mut u32, reserved: *const c_void) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }

    /// One teed stream: which it is, the console handle it had, our pipe's
    /// write end, and word from its thread that it is done.
    struct Stream {
        id: u32,
        original: Handle,
        write: Handle,
        done: Receiver<()>,
    }

    pub(super) struct Tee(Vec<Stream>);

    impl Tee {
        /// Points stdout and stderr back at what they were, closes the pipes
        /// (the threads then read to the end) and waits a little for them.
        pub(super) fn stop(self) {
            let _ = std::io::stdout().flush();
            for s in &self.0 {
                unsafe {
                    SetStdHandle(s.id, s.original);
                    CloseHandle(s.write);
                }
            }
            for s in &self.0 {
                // A program started with the console inherited keeps a copy
                // of the pipe open, so do not wait for ever.
                let _ = s.done.recv_timeout(Duration::from_millis(500));
            }
        }
    }

    pub(super) fn start(file: Arc<Mutex<File>>) -> LogGuard {
        let mut streams = Vec::new();
        for id in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
            if let Some(s) = tee_one(id, file.clone()) {
                streams.push(s);
            }
        }
        LogGuard { tee: Some(Tee(streams)) }
    }

    fn tee_one(id: u32, file: Arc<Mutex<File>>) -> Option<Stream> {
        unsafe {
            let original = GetStdHandle(id);
            let (mut read, mut write): (Handle, Handle) = (std::ptr::null_mut(), std::ptr::null_mut());
            if CreatePipe(&mut read, &mut write, std::ptr::null(), 0) == 0 {
                return None;
            }
            if SetStdHandle(id, write) == 0 {
                CloseHandle(read);
                CloseHandle(write);
                return None;
            }
            let reader = File::from_raw_handle(read as RawHandle);
            // The console handle is shared, as a number, with the thread.
            let console = original as usize;
            let (done, finished) = channel();
            let spawned = std::thread::Builder::new().name("log tee".into()).spawn(move || {
                copy(reader, console as Handle, &file);
                let _ = done.send(());
            });
            if spawned.is_err() {
                SetStdHandle(id, original);
                CloseHandle(write);
                return None;
            }
            Some(Stream { id, original, write, done: finished })
        }
    }

    /// Copies lines from the pipe to the console (as before) and the file.
    fn copy(reader: File, console: Handle, file: &Mutex<File>) {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        let mut mode = 0u32;
        let is_console = !console.is_null() && unsafe { GetConsoleMode(console, &mut mode) } != 0;
        // A console or file handle that was there before; never closed here.
        let mut redirected = (!console.is_null() && !is_console)
            .then(|| ManuallyDrop::new(unsafe { File::from_raw_handle(console as RawHandle) }));
        loop {
            line.clear();
            match reader.read_until(b'\n', &mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            let text = String::from_utf8_lossy(&line);
            if is_console {
                let wide: Vec<u16> = text.encode_utf16().collect();
                let mut written = 0;
                unsafe {
                    WriteConsoleW(console, wide.as_ptr(), wide.len() as u32, &mut written, std::ptr::null());
                }
            } else if let Some(out) = redirected.as_mut() {
                let _ = out.write_all(&line);
            }
            if let Ok(mut f) = file.lock() {
                let _ = f.write_all(plain(&text).as_bytes());
                let _ = f.flush();
            }
        }
    }
}

#[cfg(not(windows))]
mod tee {
    use std::fs::File;
    use std::sync::{Arc, Mutex};

    use super::LogGuard;

    /// No tee here: the log file holds only its first line.
    pub(super) fn start(_file: Arc<Mutex<File>>) -> LogGuard {
        LogGuard::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_are_utc_dates() {
        assert_eq!(stamp_of(0), "1970-01-01_00-00-00");
        // 2026-11-10 14:03:22 UTC.
        assert_eq!(stamp_of(1_794_319_402), "2026-11-10_14-03-22");
        assert_eq!(stamp_of(951_782_400), "2000-02-29_00-00-00");
    }

    #[test]
    fn colour_codes_are_taken_out() {
        assert_eq!(plain("\u{1b}[2m2026\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m bevy: hi\n"), "2026  INFO bevy: hi\n");
        assert_eq!(plain("plain"), "plain");
    }

    #[test]
    fn pruning_keeps_the_newest() {
        let dir = std::env::temp_dir().join(format!("mcc-logs-prune-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for n in 1..=7 {
            std::fs::write(dir.join(format!("mcc-2026-11-0{n}_00-00-00.log")), "").unwrap();
        }
        std::fs::write(dir.join("crash-2026-11-01_00-00-00.txt"), "").unwrap();
        prune(&dir, "mcc-", ".log", 4);
        let mut left: Vec<String> =
            std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left.len(), 5);
        assert_eq!(left[0], "crash-2026-11-01_00-00-00.txt");
        assert_eq!(left[1], "mcc-2026-11-04_00-00-00.log");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
