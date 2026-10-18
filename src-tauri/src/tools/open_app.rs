//! `open_application`: launches an allowlisted application and verifies it came to the front.
//!
//! The caller names an app from a fixed enum; it never supplies a path or arguments.
//! The executable is resolved here from Windows' own registration and launched
//! directly (no shell), with no arguments.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::Risk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowedApp {
    DefaultBrowser,
}

impl AllowedApp {
    pub fn risk(self) -> Risk {
        match self {
            AllowedApp::DefaultBrowser => Risk::Low,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub app: AllowedApp,
    /// True only if a window from the launched program was observed in the foreground.
    pub verified: bool,
    /// Plain-language name such as "Microsoft Edge".
    pub app_name: String,
}

const VERIFY_TIMEOUT: Duration = Duration::from_secs(8);
const VERIFY_POLL: Duration = Duration::from_millis(200);

/// Rejects a second launch while one is still in flight (duplicate speech events, double clicks).
static IN_FLIGHT: AtomicBool = AtomicBool::new(false);

struct InFlightGuard;

impl InFlightGuard {
    fn acquire() -> Option<Self> {
        IN_FLIGHT
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| InFlightGuard)
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
pub fn open(app: AllowedApp) -> Result<OpenResult, String> {
    use super::win;

    // Confirmation flow does not exist yet, so only low-risk apps may be opened.
    if app.risk() != Risk::Low {
        return Err("That needs your confirmation, which this version can't ask for yet.".into());
    }
    let _guard = InFlightGuard::acquire()
        .ok_or_else(|| "I'm already opening something. Please wait a moment.".to_string())?;

    let exe = match app {
        AllowedApp::DefaultBrowser => win::default_browser_exe(),
    }
    .filter(|p| is_launchable_exe(p))
    .ok_or_else(|| "I couldn't find your web browser.".to_string())?;

    Command::new(&exe).spawn().map_err(|e| {
        crate::diag::log("open_app", format!("open_application: launch failed: {e}"));
        "I tried to open your browser, but Windows didn't start it.".to_string()
    })?;

    let verified = wait_for_foreground(&exe, VERIFY_TIMEOUT, VERIFY_POLL, || {
        win::foreground_window().and_then(|w| w.exe_path)
    });

    Ok(OpenResult {
        app,
        verified,
        app_name: friendly_name(&exe),
    })
}

/// The resolved path must be an existing absolute `.exe`; anything else is refused.
fn is_launchable_exe(p: &Path) -> bool {
    p.is_absolute() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe")) && p.is_file()
}

/// Polls `observe` (the foreground window's executable) until it matches `target` or time runs out.
fn wait_for_foreground(
    target: &Path,
    timeout: Duration,
    poll: Duration,
    mut observe: impl FnMut() -> Option<PathBuf>,
) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if observe().is_some_and(|p| same_path(&p, target)) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(poll);
    }
}

/// Windows paths are case-insensitive.
fn same_path(a: &Path, b: &Path) -> bool {
    a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
}

fn friendly_name(exe: &Path) -> String {
    let stem = exe
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match stem.as_str() {
        "msedge" => "Microsoft Edge",
        "chrome" => "Google Chrome",
        "firefox" => "Firefox",
        "brave" => "Brave",
        "opera" | "launcher" => "Opera",
        "vivaldi" => "Vivaldi",
        _ => "your browser",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDGE: &str = r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe";

    #[test]
    fn only_known_apps_deserialise() {
        assert_eq!(
            serde_json::from_str::<AllowedApp>(r#""default_browser""#).unwrap(),
            AllowedApp::DefaultBrowser
        );
        for bad in [
            r#""notepad""#,
            r#""C:\\Windows\\System32\\cmd.exe""#,
            r#""""#,
            "42",
        ] {
            assert!(serde_json::from_str::<AllowedApp>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn allowlisted_apps_are_low_risk() {
        assert_eq!(AllowedApp::DefaultBrowser.risk(), Risk::Low);
    }

    #[test]
    fn verification_succeeds_when_target_reaches_foreground() {
        let mut seen = vec![
            None,
            Some(PathBuf::from(r"C:\Other\app.exe")),
            Some(EDGE.to_uppercase().into()),
        ]
        .into_iter();
        assert!(wait_for_foreground(
            Path::new(EDGE),
            Duration::from_secs(1),
            Duration::from_millis(1),
            || seen.next().flatten()
        ));
    }

    #[test]
    fn verification_fails_honestly_on_timeout() {
        assert!(!wait_for_foreground(
            Path::new(EDGE),
            Duration::from_millis(30),
            Duration::from_millis(5),
            || Some(PathBuf::from(r"C:\Other\app.exe"))
        ));
    }

    #[test]
    fn refuses_non_exe_and_relative_paths() {
        assert!(!is_launchable_exe(Path::new("msedge.exe")));
        assert!(!is_launchable_exe(Path::new(
            r"C:\Windows\System32\drivers\etc\hosts"
        )));
        assert!(!is_launchable_exe(Path::new(r"C:\does\not\exist.exe")));
    }

    #[test]
    fn concurrent_launch_is_rejected() {
        let first = InFlightGuard::acquire().expect("first acquire");
        assert!(InFlightGuard::acquire().is_none());
        drop(first);
        assert!(InFlightGuard::acquire().is_some());
    }

    #[test]
    fn friendly_names() {
        assert_eq!(friendly_name(Path::new(EDGE)), "Microsoft Edge");
        assert_eq!(
            friendly_name(Path::new(r"C:\x\unknown.exe")),
            "your browser"
        );
    }
}
