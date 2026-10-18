//! Buttons and text boxes in an ordinary app's window (e.g. Spotify), through Windows UI
//! Automation, the same way as on web pages: list them, press one, type into one.
//!
//! Never in this assistant's own window, web browsers (the page tools handle those, with
//! their page-address checks), command lines, system and security tools, or password
//! managers. Every action re-finds its target in the same window (same kind, label and
//! position) before acting. Typing uses the browser's checked routine: the window must be
//! in front and the box must have the keyboard focus.

use std::time::{Duration, Instant};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Accessibility::{
    IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationTogglePattern,
    TreeScope_Descendants, UIA_ButtonControlTypeId, UIA_CheckBoxControlTypeId,
    UIA_ComboBoxControlTypeId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_HyperlinkControlTypeId, UIA_InvokePatternId, UIA_TogglePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{GetWindowThreadProcessId, IsWindow};

use super::browser::{
    ControlKind, TypeResult, automation, control_type, is_editable, is_password,
    is_sensitive_label, name_of, type_into_element, value_of,
};
use super::win;

const MAX_LISTED: usize = 80;
const MAX_SCANNED: i32 = 3000;
const MAX_VALUE_CHARS: usize = 200;

/// Programs whose windows the assistant never operates (by executable name, lower case,
/// without ".exe").
const BLOCKED_WINDOW_EXES: &[&str] = &[
    // Web browsers: use the page tools instead.
    "msedge",
    "chrome",
    "firefox",
    "brave",
    "opera",
    "vivaldi",
    "iexplore",
    // Command lines and system/security tools.
    "cmd",
    "powershell",
    "powershell_ise",
    "pwsh",
    "windowsterminal",
    "openconsole",
    "conhost",
    "wsl",
    "bash",
    "regedit",
    "mmc",
    "taskmgr",
    "msconfig",
    "control",
    "systemsettings",
    "sechealthui",
    "mstsc",
    "consent",
    "credentialuibroker",
    "lockapp",
    // Password managers.
    "1password",
    "bitwarden",
    "keepass",
    "keepassxc",
    "dashlane",
    "lastpass",
    "nordpass",
];

/// A button or text box in an app's window, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppControl {
    pub kind: ControlKind,
    pub label: String,
    /// Which control of this kind with this exact label (0 = first) in the window.
    pub ordinal: usize,
    /// The window it is in. Never sent to the model.
    pub hwnd: isize,
    /// Password or payment-like box: never typed into, value never reported.
    pub sensitive: bool,
    /// The start of a (non-sensitive) text box's current text.
    pub value: Option<String>,
    /// How many characters the box held (typing replaces all of them).
    pub text_len: usize,
}

/// Deterministic: may the assistant operate windows of this executable?
pub fn is_blocked_window_exe(exe_stem: &str) -> bool {
    let s = exe_stem.to_lowercase();
    BLOCKED_WINDOW_EXES.contains(&s.as_str())
}

/// Checks the window still exists, isn't ours and isn't blocked. Returns its app's name.
pub fn check_window(hwnd_raw: isize) -> Result<String, String> {
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return Err("That window has closed.".into());
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == std::process::id() {
        return Err("That's the assistant's own window.".into());
    }
    let exe = win::process_image_path(pid);
    let stem = exe
        .as_deref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let app = win::friendly_app_name(exe.as_deref());
    if is_blocked_window_exe(&stem) {
        return Err(format!(
            "For safety I don't press buttons or type in {app}. (For web pages, use the page tools.)"
        ));
    }
    Ok(app)
}

/// Every button and text box in the window, in order, including ones scrolled out of view
/// (so positions stay stable), with whether each is on screen.
fn controls_in(
    a: &IUIAutomation,
    root: &IUIAutomationElement,
) -> Result<Vec<(ControlKind, IUIAutomationElement, bool)>, String> {
    let fail = |_| "I couldn't look for buttons and boxes in that window.".to_string();
    let mut cond = control_type(a, UIA_ButtonControlTypeId)?;
    for t in [
        UIA_CheckBoxControlTypeId,
        UIA_HyperlinkControlTypeId,
        UIA_EditControlTypeId,
        UIA_ComboBoxControlTypeId,
        UIA_DocumentControlTypeId,
    ] {
        cond = unsafe { a.CreateOrCondition(&cond, &control_type(a, t)?) }.map_err(fail)?;
    }
    let all = unsafe { root.FindAll(TreeScope_Descendants, &cond) }.map_err(fail)?;
    let n = unsafe { all.Length() }.unwrap_or(0).min(MAX_SCANNED);
    Ok((0..n)
        .filter_map(|i| unsafe { all.GetElement(i) }.ok())
        .filter_map(|e| {
            let t = unsafe { e.CurrentControlType() }.ok()?;
            let kind = if t == UIA_EditControlTypeId {
                ControlKind::TextField
            } else if t == UIA_ComboBoxControlTypeId || t == UIA_DocumentControlTypeId {
                // Only ones you can type into (search boxes, text areas), not drop-downs or
                // read-only views.
                if !is_editable(&e) {
                    return None;
                }
                ControlKind::TextField
            } else {
                ControlKind::Button
            };
            let enabled = unsafe { e.CurrentIsEnabled() }
                .map(|b| b.as_bool())
                .unwrap_or(false);
            let on_screen = enabled
                && !unsafe { e.CurrentIsOffscreen() }
                    .map(|b| b.as_bool())
                    .unwrap_or(true);
            Some((kind, e, on_screen))
        })
        .collect())
}

fn root_of(a: &IUIAutomation, hwnd_raw: isize) -> Result<IUIAutomationElement, String> {
    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
    unsafe { a.ElementFromHandle(hwnd) }
        .map_err(|_| "I couldn't look inside that window.".to_string())
}

fn with_ordinals(
    found: Vec<(ControlKind, IUIAutomationElement, bool)>,
) -> Vec<(ControlKind, String, usize, IUIAutomationElement, bool)> {
    let mut seen: std::collections::HashMap<(ControlKind, String), usize> =
        std::collections::HashMap::new();
    found
        .into_iter()
        .filter_map(|(kind, e, on_screen)| {
            let label = name_of(&e);
            if label.is_empty() {
                return None;
            }
            let ordinal = *seen
                .entry((kind, label.clone()))
                .and_modify(|n| *n += 1)
                .or_insert(0);
            Some((kind, label, ordinal, e, on_screen))
        })
        .collect()
}

/// Does `label` match the optional filter (all its words, any order, ignoring case)?
pub fn matches_filter(label: &str, filter: Option<&str>) -> bool {
    let Some(f) = filter else { return true };
    let l = label.to_lowercase();
    f.to_lowercase().split_whitespace().all(|w| l.contains(w))
}

/// The visible buttons and text boxes in the window (optionally only those whose label
/// contains `filter`'s words). At most 80.
pub fn list_controls(
    hwnd: isize,
    filter: Option<&str>,
) -> Result<(String, Vec<AppControl>), String> {
    let app = check_window(hwnd)?;
    let a = automation()?;
    let root = root_of(&a, hwnd)?;
    // Chromium-based apps (Spotify, Teams, Discord...) build their accessibility tree the
    // first time something asks, so the first look can be nearly empty.
    let mut found = controls_in(&a, &root)?;
    let deadline = Instant::now() + Duration::from_secs(4);
    while found.len() < 5 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        found = controls_in(&a, &root)?;
    }
    let mut out = Vec::new();
    for (kind, label, ordinal, e, on_screen) in with_ordinals(found) {
        // Chromium-based apps keep a hidden internal address bar; it's never useful.
        if !on_screen || !matches_filter(&label, filter) || label == "Address and search bar" {
            continue;
        }
        let sensitive =
            kind == ControlKind::TextField && (is_password(&e) || is_sensitive_label(&label));
        let full = if kind == ControlKind::TextField && !sensitive {
            value_of(&e)
        } else {
            None
        };
        let text_len = full.as_deref().map(|v| v.chars().count()).unwrap_or(0);
        let value = full.map(|v| v.chars().take(MAX_VALUE_CHARS).collect());
        out.push(AppControl {
            kind,
            label,
            ordinal,
            hwnd,
            sensitive,
            value,
            text_len,
        });
        if out.len() >= MAX_LISTED {
            break;
        }
    }
    Ok((app, out))
}

/// Re-finds an observed control in the same window: same kind, label and position.
fn find_again(a: &IUIAutomation, c: &AppControl) -> Result<IUIAutomationElement, String> {
    check_window(c.hwnd)?;
    let root = root_of(a, c.hwnd)?;
    with_ordinals(controls_in(a, &root)?)
        .into_iter()
        .find(|(kind, label, ordinal, _, _)| {
            *kind == c.kind && *label == c.label && *ordinal == c.ordinal
        })
        .map(|(_, _, _, e, _)| e)
        .ok_or_else(|| {
            "That isn't in the window any more, so I didn't touch anything. Ask me to look again."
                .to_string()
        })
}

/// Presses an observed button (Invoke, or Toggle for on/off switches).
pub fn press(c: &AppControl) -> Result<(), String> {
    if c.kind != ControlKind::Button {
        return Err("That's a text box, not a button.".into());
    }
    let a = automation()?;
    let target = find_again(&a, c)?;
    if let Ok(invoke) =
        unsafe { target.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
    {
        unsafe { invoke.Invoke() }.map_err(|_| "Pressing that didn't work.".to_string())?;
    } else if let Ok(toggle) =
        unsafe { target.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) }
    {
        unsafe { toggle.Toggle() }.map_err(|_| "Switching that didn't work.".to_string())?;
    } else {
        return Err("That can't be pressed from here.".into());
    }
    std::thread::sleep(Duration::from_millis(800));
    Ok(())
}

/// Types into an observed text box, replacing what's in it. Never presses Enter.
pub fn type_into(c: &AppControl, text: &str) -> Result<TypeResult, String> {
    if c.kind != ControlKind::TextField {
        return Err("That's a button, not a text box.".into());
    }
    if c.sensitive {
        return Err("That box is for a password or payment details. For your safety I never type into those; please fill it in yourself.".into());
    }
    let a = automation()?;
    let field = find_again(&a, c)?;
    let hwnd = HWND(c.hwnd as *mut core::ffi::c_void);
    type_into_element(hwnd, &field, text, "that app's window")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browsers_command_lines_and_password_managers_are_off_limits() {
        for exe in [
            "msedge",
            "Chrome",
            "cmd",
            "PowerShell",
            "WindowsTerminal",
            "regedit",
            "Taskmgr",
            "SystemSettings",
            "1Password",
            "KeePassXC",
        ] {
            assert!(is_blocked_window_exe(exe), "{exe}");
        }
        for exe in [
            "Spotify",
            "notepad",
            "Teams",
            "Discord",
            "explorer",
            "CalculatorApp",
        ] {
            assert!(!is_blocked_window_exe(exe), "{exe}");
        }
    }

    #[test]
    fn filter_matches_all_words_in_any_order() {
        assert!(matches_filter("Play Kanye West", Some("kanye")));
        assert!(matches_filter("Play Kanye West", Some("west play")));
        assert!(!matches_filter("Play Kanye West", Some("yeat")));
        assert!(matches_filter("Anything", None));
    }

    /// VISIBLE: lists Spotify's controls (Spotify must be open), types "Yeat" into its search
    /// box and lists the matching Play buttons. Nothing is played.
    /// cargo test --manifest-path src-tauri/Cargo.toml live_spotify -- --ignored --nocapture --test-threads=1
    #[test]
    #[ignore = "operates the real Spotify window"]
    fn live_spotify_controls() {
        let launched = super::super::apps::open_by_name("spotify").unwrap();
        println!("opened: {launched:?}");
        let hwnd = launched.hwnd.expect("Spotify window in front");
        let (app, controls) = list_controls(hwnd, None).unwrap();
        println!("{app}: {} controls", controls.len());
        for c in controls.iter().take(25) {
            println!("  {:?} {:?} value={:?}", c.kind, c.label, c.value);
        }
        let search = controls
            .iter()
            .find(|c| c.kind == ControlKind::TextField && c.label.contains("What do you want"))
            .expect("search box");
        let r = type_into(search, "Yeat");
        println!("typed: {r:?}");
        std::thread::sleep(Duration::from_secs(3));
        let (_, plays) = list_controls(hwnd, Some("play yeat")).unwrap();
        for c in &plays {
            println!("  {:?} {:?}", c.kind, c.label);
        }
        assert!(r.is_ok_and(|r| r.verified));
    }
}
