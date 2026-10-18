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
    Notepad,
    Calculator,
    Paint,
    FileExplorer,
    Settings,
}

impl AllowedApp {
    pub fn risk(self) -> Risk {
        match self {
            // Opening an everyday app changes nothing by itself.
            AllowedApp::DefaultBrowser
            | AllowedApp::Notepad
            | AllowedApp::Calculator
            | AllowedApp::Paint
            | AllowedApp::FileExplorer
            | AllowedApp::Settings => Risk::Low,
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

/// How each allowlisted app is started, and how we recognise its window afterwards.
struct AppSpec {
    name: &'static str,
    /// Lower-case executable names that may own the app's window.
    window_exes: &'static [&'static str],
    /// A word the app's window title contains (English Windows).
    title_word: &'static str,
}

fn spec(app: AllowedApp) -> AppSpec {
    match app {
        AllowedApp::DefaultBrowser => AppSpec {
            name: "your browser",
            window_exes: &[],
            title_word: "",
        },
        AllowedApp::Notepad => AppSpec {
            name: "Notepad",
            window_exes: &["notepad.exe"],
            title_word: "Notepad",
        },
        AllowedApp::Calculator => AppSpec {
            name: "Calculator",
            window_exes: &["calculatorapp.exe", "calc.exe"],
            title_word: "Calculator",
        },
        AllowedApp::Paint => AppSpec {
            name: "Paint",
            window_exes: &["mspaint.exe"],
            title_word: "Paint",
        },
        AllowedApp::FileExplorer => AppSpec {
            name: "File Explorer",
            window_exes: &["explorer.exe"],
            title_word: "File Explorer",
        },
        AllowedApp::Settings => AppSpec {
            name: "Settings",
            window_exes: &["systemsettings.exe"],
            title_word: "Settings",
        },
    }
}

/// Does the foreground window (title, executable) look like `app`'s window?
fn is_app_window(app: AllowedApp, title: &str, exe: Option<&Path>) -> bool {
    let s = spec(app);
    if title.trim().is_empty() || title == "Program Manager" {
        return false;
    }
    let exe_name = exe
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    s.window_exes.contains(&exe_name.as_str())
        || (!s.title_word.is_empty() && title.contains(s.title_word))
}

#[cfg(windows)]
pub fn open(app: AllowedApp) -> Result<OpenResult, String> {
    use super::win;

    let _guard = InFlightGuard::acquire()
        .ok_or_else(|| "I'm already opening something. Please wait a moment.".to_string())?;

    if app == AllowedApp::DefaultBrowser {
        let exe = win::default_browser_exe()
            .filter(|p| is_launchable_exe(p))
            .ok_or_else(|| "I couldn't find your web browser.".to_string())?;
        Command::new(&exe).spawn().map_err(|e| {
            crate::diag::log("open_app", format!("open_application: launch failed: {e}"));
            "I tried to open your browser, but Windows didn't start it.".to_string()
        })?;
        let verified = wait_for_foreground(&exe, VERIFY_TIMEOUT, VERIFY_POLL, || {
            win::foreground_window().and_then(|w| w.exe_path)
        });
        return Ok(OpenResult {
            app,
            verified,
            app_name: friendly_name(&exe),
        });
    }

    let s = spec(app);
    launch(app).map_err(|e| {
        crate::diag::log(
            "open_app",
            format!("open_application {}: launch failed: {e}", s.name),
        );
        format!("I tried to open {}, but Windows didn't start it.", s.name)
    })?;
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    let mut verified = false;
    while Instant::now() < deadline {
        if let Some(w) = win::foreground_window() {
            if is_app_window(app, &w.title, w.exe_path.as_deref()) {
                verified = true;
                break;
            }
        }
        std::thread::sleep(VERIFY_POLL);
    }
    Ok(OpenResult {
        app,
        verified,
        app_name: s.name.to_string(),
    })
}

/// Starts an allowlisted app: fixed executables in the Windows folders (no shell, no
/// arguments), Paint through its Store alias, Settings through its fixed address.
#[cfg(windows)]
fn launch(app: AllowedApp) -> std::io::Result<()> {
    let windows_dir = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let exe = match app {
        AllowedApp::Notepad => windows_dir.join(r"System32\notepad.exe"),
        AllowedApp::Calculator => windows_dir.join(r"System32\calc.exe"),
        AllowedApp::FileExplorer => windows_dir.join("explorer.exe"),
        AllowedApp::Paint => {
            let alias = std::env::var_os("LOCALAPPDATA")
                .map(|d| PathBuf::from(d).join(r"Microsoft\WindowsApps\mspaint.exe"))
                .filter(|p| p.exists());
            alias.unwrap_or_else(|| windows_dir.join(r"System32\mspaint.exe"))
        }
        AllowedApp::Settings => {
            return super::screen::open_settings_home();
        }
        AllowedApp::DefaultBrowser => unreachable!("handled by the caller"),
    };
    Command::new(exe).spawn().map(|_| ())
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
            r#""cmd""#,
            r#""C:\\Windows\\System32\\cmd.exe""#,
            r#""""#,
            "42",
        ] {
            assert!(serde_json::from_str::<AllowedApp>(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn allowlisted_apps_are_low_risk() {
        for app in [
            AllowedApp::DefaultBrowser,
            AllowedApp::Notepad,
            AllowedApp::Calculator,
            AllowedApp::Paint,
            AllowedApp::FileExplorer,
            AllowedApp::Settings,
        ] {
            assert_eq!(app.risk(), Risk::Low);
        }
    }

    #[test]
    fn app_windows_are_recognised_by_exe_or_title() {
        let p = |s: &str| Some(PathBuf::from(s));
        assert!(is_app_window(
            AllowedApp::Notepad,
            "Untitled - Notepad",
            p(r"C:\x\Notepad.exe").as_deref()
        ));
        assert!(is_app_window(
            AllowedApp::Calculator,
            "Calculator",
            p(r"C:\x\ApplicationFrameHost.exe").as_deref()
        ));
        assert!(is_app_window(
            AllowedApp::FileExplorer,
            "Home",
            p(r"C:\Windows\explorer.exe").as_deref()
        ));
        assert!(!is_app_window(
            AllowedApp::FileExplorer,
            "Program Manager",
            p(r"C:\Windows\explorer.exe").as_deref()
        ));
        assert!(!is_app_window(
            AllowedApp::Paint,
            "Voice Assistant",
            p(r"C:\x\voice-desktop-agent.exe").as_deref()
        ));
    }

    #[test]
    fn new_apps_deserialise_from_their_names_only() {
        for (name, app) in [
            ("notepad", AllowedApp::Notepad),
            ("calculator", AllowedApp::Calculator),
            ("paint", AllowedApp::Paint),
            ("file_explorer", AllowedApp::FileExplorer),
            ("settings", AllowedApp::Settings),
        ] {
            assert_eq!(
                serde_json::from_str::<AllowedApp>(&format!("\"{name}\"")).unwrap(),
                app
            );
        }
        for bad in [
            "\"cmd\"",
            "\"powershell\"",
            "\"regedit\"",
            "\"C:\\\\Windows\\\\notepad.exe\"",
        ] {
            assert!(serde_json::from_str::<AllowedApp>(bad).is_err(), "{bad}");
        }
    }

    /// VISIBLE: opens Notepad and Calculator, checks them, then closes them.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_apps -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "opens real apps briefly"]
    fn live_apps_open() {
        for app in [AllowedApp::Notepad, AllowedApp::Calculator] {
            let r = open(app);
            println!("{app:?}: {r:?}");
            assert!(r.is_ok());
        }
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
