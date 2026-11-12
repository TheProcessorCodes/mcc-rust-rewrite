//! The new-version check (#45): when the map menu opens, a thread asks
//! GitHub for this project's releases. If one is newer than this build
//! ([`game_paths::VERSION`]), the menu shows a pop-up over the maps (`menu.rs`
//! `Phase::Update`) with the two versions and two buttons: Close, and Open
//! release page, which opens the newer release on GitHub in the browser.
//!
//! The question goes through `curl.exe`, which every Windows since 10 (1803)
//! has, so the game carries no web client of its own. No network, no curl, a
//! slow or odd reply: no pop-up, and the menu never waits for it. Drafts are
//! never offered, and only a version strictly higher than ours is. The check
//! runs once per launch; `MCC_NO_UPDATE_CHECK=1` turns it off.
//!
//! `--menu-shot FILE --menu-update TAG` draws the pop-up for release TAG
//! without asking GitHub.

use std::process::Stdio;
use std::sync::Mutex;
use std::sync::mpsc::Receiver;

/// Where the releases live.
const REPO: &str = "TheProcessorCodes/mcc-rust-rewrite";

/// A release newer than this build.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct NewRelease {
    /// Its tag, as players see it (`beta-0.0.5`).
    pub tag: String,
}

impl NewRelease {
    /// The release's page on GitHub. Built from the tag (letters, digits,
    /// `.`, `-`, `_` only, checked in [`newer_release`]), never taken from
    /// the reply, so nothing GitHub says can make it open some other page.
    pub fn page(&self) -> String {
        format!("https://github.com/{REPO}/releases/tag/{}", self.tag)
    }
}

/// The check, running on its thread.
pub(super) struct UpdateCheck(pub Mutex<Receiver<Option<NewRelease>>>);

/// Starts the check, unless it is turned off.
pub(super) fn start() -> Option<UpdateCheck> {
    if std::env::var_os("MCC_NO_UPDATE_CHECK").is_some_and(|v| v != "0") {
        return None;
    }
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("update check".into())
        .spawn(move || {
            let found = match fetch() {
                Ok(body) => newer_release(&body, game_paths::VERSION),
                Err(e) => {
                    eprintln!("menu: update check: {e}");
                    None
                }
            };
            match &found {
                Some(r) => eprintln!("menu: update check: {} is out (this is {})", r.tag, game_paths::VERSION),
                None => eprintln!("menu: update check: nothing newer than {}", game_paths::VERSION),
            }
            let _ = send.send(found);
        })
        .ok()?;
    Some(UpdateCheck(Mutex::new(receive)))
}

/// curl: Windows' own copy first (not whatever else on the PATH is named
/// curl), then the PATH's.
fn curl() -> std::process::Command {
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let own = std::path::Path::new(&root).join(r"System32\curl.exe");
            if own.is_file() {
                return super::support::quiet_command(own.as_os_str());
            }
        }
    }
    super::support::quiet_command("curl".as_ref())
}

/// The releases list, as GitHub's API gives it (JSON).
fn fetch() -> Result<String, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=30");
    let user_agent = format!("MCC-Rust-Rewrite/{}", game_paths::VERSION);
    let output = curl()
        .args(["--silent", "--show-error", "--fail", "--location"])
        .args(["--connect-timeout", "5", "--max-time", "10", "--max-filesize", "4000000"])
        .args(["--header", "Accept: application/vnd.github+json"])
        .args(["--user-agent", &user_agent])
        .arg(&url)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("cannot start curl: {e}"))?;
    if !output.status.success() {
        let said = String::from_utf8_lossy(&output.stderr);
        return Err(format!("curl failed ({}): {}", output.status, said.trim()));
    }
    String::from_utf8(output.stdout).map_err(|_| "the reply is not text".to_string())
}

/// A version number out of a tag or [`game_paths::VERSION`]: `beta-0.0.4`,
/// `v1.2`, `0.1.0` (missing parts are 0). None for anything else, such as
/// the first release's tag, plain `beta`.
fn version_of(tag: &str) -> Option<[u64; 3]> {
    let number = tag.trim().trim_start_matches("beta-").trim_start_matches(['v', 'V']);
    let mut parts = [0u64; 3];
    let mut count = 0;
    for part in number.split('.') {
        if count == 3 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        parts[count] = part.parse().ok()?;
        count += 1;
    }
    Some(parts)
}

/// The newest release in GitHub's releases list (`body`) that is newer than
/// `ours`, if any; drafts and tags that are not plain version numbers are
/// left out.
fn newer_release(body: &str, ours: &str) -> Option<NewRelease> {
    let ours = version_of(ours)?;
    let releases: serde_json::Value = serde_json::from_str(body).ok()?;
    releases
        .as_array()?
        .iter()
        .filter(|r| !r.get("draft").and_then(|d| d.as_bool()).unwrap_or(false))
        .filter_map(|r| r.get("tag_name")?.as_str())
        .filter(|tag| tag.len() <= 40 && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        .filter_map(|tag| Some((version_of(tag)?, tag)))
        .filter(|(version, _)| *version > ours)
        .max_by_key(|(version, _)| *version)
        .map(|(_, tag)| NewRelease { tag: tag.to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(version_of("beta-0.0.4"), Some([0, 0, 4]));
        assert_eq!(version_of("v1.2"), Some([1, 2, 0]));
        assert_eq!(version_of("0.10.3"), Some([0, 10, 3]));
        assert_eq!(version_of("beta"), None);
        assert_eq!(version_of("beta-0.0.4-rc1"), None);
        assert_eq!(version_of("1.2.3.4"), None);
        assert!(version_of("beta-0.0.10") > version_of("beta-0.0.9"));
    }

    const LIST: &str = r#"[
        {"tag_name": "beta-0.0.6", "draft": true, "prerelease": false},
        {"tag_name": "beta-0.0.5", "draft": false, "prerelease": false, "html_url": "https://example.com/elsewhere"},
        {"tag_name": "beta-0.0.4", "draft": false, "prerelease": false},
        {"tag_name": "beta", "draft": false, "prerelease": true}
    ]"#;

    #[test]
    fn offers_the_newest_published_release() {
        let found = newer_release(LIST, "beta-0.0.4").unwrap();
        assert_eq!(found.tag, "beta-0.0.5");
        // The page is ours, whatever the reply's links say.
        assert_eq!(found.page(), "https://github.com/TheProcessorCodes/mcc-rust-rewrite/releases/tag/beta-0.0.5");
        assert_eq!(newer_release(LIST, "beta-0.0.1").unwrap().tag, "beta-0.0.5");
    }

    #[test]
    fn nothing_when_up_to_date_or_odd() {
        assert_eq!(newer_release(LIST, "beta-0.0.5"), None);
        assert_eq!(newer_release(LIST, "beta-0.1.0"), None);
        assert_eq!(newer_release("not json", "beta-0.0.1"), None);
        assert_eq!(newer_release(r#"{"message": "API rate limit exceeded"}"#, "beta-0.0.1"), None);
        assert_eq!(newer_release(r#"[{"tag_name": "beta-9.9.9 & calc"}]"#, "beta-0.0.1"), None);
        assert_eq!(newer_release("[]", "beta-0.0.1"), None);
    }
}
